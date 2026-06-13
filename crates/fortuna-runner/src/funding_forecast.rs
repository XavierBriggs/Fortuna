//! `funding_forecast` — a ZERO-CAPITAL belief-producer strategy
//! (docs/design/perp-strategies-and-scalar-claims.md §2.2; GAPS R1 adjudication).
//!
//! # What it is
//!
//! `funding_forecast` is the first scalar belief consumer. It rides the
//! `PerpTick` seam (the venue funding ESTIMATE + `next_funding_time`), forecasts
//! the next FINALIZED funding rate as a quantile fan, and emits it as a
//! [`ScalarBeliefDraft`] through the additive `Strategy::drain_scalar_beliefs`
//! egress seam (design §2.5). It **proposes nothing** — `on_event` always
//! returns `Ok(vec![])`. There is no `Proposal`, no `ProposedLeg`, no `Cents`,
//! no sizing: I6 holds vacuously because there is no order to size (design §7).
//! It is scored by `CrpsPinballRule` against the realized funding rate at
//! `next_funding_time` (design §1.2/§2.2).
//!
//! # The input — the recorded estimate, used DIRECTLY (GAPS R1, BINDING)
//!
//! The PRIMARY input is the venue's recorded funding ESTIMATE, used **directly**:
//! the point forecast is `finalize_funding_rate(estimate)`. The estimate already
//! IS the venue's running time-weighted average of the premium index over
//! `[last_funding_time, now)` (the running TWAP). Feeding it back into
//! [`fortuna_core::perp::FundingWindow`] (a per-candle premium mean) would
//! compute a "mean of means" — wrong. So `FundingWindow` is NOT used in the
//! primary path here; it stays the SECONDARY path (the `mark − reference`
//! premium proxy, labeled approximate, design §2.3) for a future modelling
//! change. The unpublished premium-index formula is never re-derived (research
//! §11; the same not-re-deriving discipline as `FundingAccrual`/`FundingWindow`).
//!
//! # The dispersion model (rung-0; documented, CRPS-measured)
//!
//! The quantile fan is the point forecast `p` plus a deterministic dispersion
//! that NARROWS as the window elapses. The shape, with `remaining` candles left
//! before `next_funding_time` (out of `FUNDING_CANDLES_PER_WINDOW`):
//!
//! ```text
//! band = DISPERSION_SCALE · sqrt(remaining / FUNDING_CANDLES_PER_WINDOW)
//! q = 0.1 →  v = clamp(p − 1.282·band, ±FUNDING_RATE_CLAMP)
//! q = 0.5 →  v = p                                  (the median is the point forecast)
//! q = 0.9 →  v = clamp(p + 1.282·band, ±FUNDING_RATE_CLAMP)
//! ```
//!
//! - `DISPERSION_SCALE = 0.002` (a ±0.2% maximum half-band scale at the
//!   window's start) — a deliberately conservative rung-0 width well inside the
//!   venue's ±2% finalization clamp (even the widest tail spread,
//!   `1.282·0.002 ≈ 0.26%`, stays an order of magnitude under the ±2% cap).
//! - `1.282` is the standard-normal 0.9-quantile multiplier (so the ±band·1.282
//!   spread reads as a ~80% central interval under a normal prior). This is a
//!   modelling CHOICE, not a venue fact.
//! - `sqrt(remaining/window)` makes the band shrink to 0 as the window closes
//!   (`remaining == 0` ⇒ band 0 ⇒ all three quantile values equal `p`): early
//!   in the window the estimate is noisier, near close it is nearly final.
//!
//! This shape is the rung-0 modelling choice the design (§2.3) says CRPS then
//! MEASURES and calibration later REFINES; it is recorded in ASSUMPTIONS.md. The
//! symmetric clamp can collapse the band toward the ±2% cap; the construction
//! keeps the quantile values non-decreasing (see [`build_quantiles`]) so the
//! emitted distribution always passes `validate_scalar`.

use crate::{CoreHandle, Proposal, RunnerError, Stage, Strategy, StrategyKind, StrategyMetrics};
use async_trait::async_trait;
use fortuna_cognition::scalar_beliefs::ScalarBeliefDraft;
use fortuna_cognition::scoring::{PredictiveDistribution, Quantile};
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, StrategyId};
use fortuna_core::perp::{finalize_funding_rate, FUNDING_CANDLES_PER_WINDOW};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

/// The rung-0 dispersion half-band scale at the window's start (`remaining ==
/// FUNDING_CANDLES_PER_WINDOW`): ±0.2%, an order of magnitude inside the
/// venue's ±2% `FUNDING_RATE_CLAMP` (a conservative width). The band shrinks
/// as `sqrt(remaining/window)`.
pub const DISPERSION_SCALE: f64 = 0.002;

/// The standard-normal 0.9-quantile (and, by symmetry, |0.1-quantile|)
/// multiplier: the q=0.1/0.9 forecast values sit `±Z90 · band` around the
/// median, reading the band as a ~80% central interval under a normal prior.
const Z90: f64 = 1.282;

/// The clamp bound as `f64` (the scalar quantile values are cognition-`f64`,
/// never money). `finalize_funding_rate` already clamps the point forecast to
/// `±FUNDING_RATE_CLAMP` in `Decimal`; this is the `f64` mirror used to clamp
/// the dispersed quantile values back into the venue's payable range.
const CLAMP_F64: f64 = 0.02;

/// Per-market window-tracking state. NO `FundingWindow` here: the primary
/// forecast uses the estimate directly (GAPS R1), so there is no per-candle
/// accumulation to hold. Only the last-seen window key + estimate, so a window
/// roll (a new `next_funding_time`) can be detected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FundingForecastState {
    /// The `next_funding_time` of the window this market last observed. A
    /// change means the previous window finalized and a new one opened — the
    /// state is reset for that market.
    pub last_next_funding_time: Option<UtcTimestamp>,
    /// The most recent estimate observed in the current window (diagnostic /
    /// roll bookkeeping). `Decimal` — the venue-payload rate domain.
    pub last_estimate: Option<Decimal>,
}

/// The zero-capital funding-rate belief producer (design §2.2).
pub struct FundingForecast {
    id: StrategyId,
    markets: BTreeMap<MarketId, FundingForecastState>,
    metrics: StrategyMetrics,
    pending: Vec<ScalarBeliefDraft>,
}

impl FundingForecast {
    /// Construct the strategy. The only failure mode is an invalid strategy id
    /// (it is a fixed literal, so this never fires in practice — but the
    /// constructor stays fallible, no `unwrap`, per the money-path discipline).
    pub fn new() -> Result<Self, RunnerError> {
        Ok(FundingForecast {
            id: StrategyId::new("funding_forecast").map_err(|e| RunnerError::Config {
                reason: e.to_string(),
            })?,
            markets: BTreeMap::new(),
            metrics: StrategyMetrics::default(),
            pending: Vec::new(),
        })
    }

    /// Candles remaining before `next_funding_time`, derived from the injected
    /// observation time (`obs_at` → `next_funding_time`) — NEVER `SystemTime`.
    /// `((next_funding_time − obs_at) / 1min)`, clamped to
    /// `[0, FUNDING_CANDLES_PER_WINDOW]`: a past-due or far-future
    /// `next_funding_time` (clock skew, a stale frame) degrades to the nearest
    /// in-range value rather than producing a nonsense band.
    fn remaining_candles(obs_at: UtcTimestamp, next_funding_time: UtcTimestamp) -> usize {
        let delta_ms = next_funding_time
            .epoch_millis()
            .saturating_sub(obs_at.epoch_millis());
        if delta_ms <= 0 {
            return 0;
        }
        // One candle per minute. Integer division floors — a partial final
        // minute does not count as a whole remaining candle.
        let candles = delta_ms / 60_000;
        let max = FUNDING_CANDLES_PER_WINDOW as i64;
        candles.clamp(0, max) as usize
    }

    /// Build the {0.1, 0.5, 0.9} quantile fan around point forecast `p` for a
    /// window with `remaining` candles left.
    ///
    /// Guarantees the result passes `validate_scalar`: q strictly increasing
    /// (0.1 < 0.5 < 0.9), v non-decreasing, all finite. The symmetric
    /// `±FUNDING_RATE_CLAMP` clamp can collapse the band when `p` is near the
    /// ±2% cap; because `p ∈ [−CLAMP_F64, CLAMP_F64]` (it is
    /// `finalize_funding_rate`'d, then defensively re-clamped) the clamped
    /// low/median/high stay ordered `v_low ≤ p ≤ v_high` (proof in the module
    /// doc). At `remaining == 0` the band is 0 and all three values equal `p`
    /// (equal v is non-decreasing — still valid).
    fn build_quantiles(p: f64, remaining: usize) -> Vec<Quantile> {
        // p is finalized + clamped upstream; re-clamp defensively so the
        // ordering proof (which assumes |p| ≤ CLAMP_F64) cannot be defeated by
        // a Decimal→f64 rounding ULP.
        let p = p.clamp(-CLAMP_F64, CLAMP_F64);
        let frac = remaining as f64 / FUNDING_CANDLES_PER_WINDOW as f64;
        let band = DISPERSION_SCALE * frac.sqrt();
        let spread = Z90 * band;
        let v_low = (p - spread).clamp(-CLAMP_F64, CLAMP_F64);
        let v_high = (p + spread).clamp(-CLAMP_F64, CLAMP_F64);
        vec![
            Quantile { q: 0.1, v: v_low },
            Quantile { q: 0.5, v: p },
            Quantile { q: 0.9, v: v_high },
        ]
    }
}

#[async_trait]
impl Strategy for FundingForecast {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }

    /// Mechanical: deterministic, no mind, no cognition spend (design §2.2).
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }

    /// Sim only (design §2.2/§7; I7 — no auto-promotion).
    fn stage(&self) -> Stage {
        Stage::Sim
    }

    /// Consume `PerpTick`s; emit a scalar belief; PROPOSE NOTHING.
    ///
    /// Every path returns `Ok(vec![])` — a belief-producer never trades
    /// (zero-capital, design §2.2). Non-`PerpTick` events are ignored.
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;
        let EventPayload::PerpTick {
            market, funding, ..
        } = &ev.payload
        else {
            return Ok(Vec::new());
        };

        // 1. Window-roll detection: a new `next_funding_time` means the prior
        //    window finalized; reset this market's state.
        let state = self.markets.entry(market.clone()).or_default();
        if state.last_next_funding_time != Some(funding.next_funding_time) {
            *state = FundingForecastState::default();
        }

        // 2. Point forecast: the recorded estimate, finalized DIRECTLY (R1).
        //    `finalize_funding_rate` clamps to ±2% + applies the zero
        //    threshold, matching the rate the venue would pay.
        let point_decimal = finalize_funding_rate(funding.estimate);
        // Decimal → cognition-f64 (the scalar quantile domain; never money).
        // A funding rate is a tiny fraction; `to_f64` cannot lose it. The
        // `unwrap_or` keeps the path panic-free without ever firing in
        // practice (Decimal in ±0.02 is exactly representable in range).
        let point = point_decimal.to_f64().unwrap_or(0.0);

        // 3. Remaining-in-window from the injected times (never SystemTime).
        let remaining = Self::remaining_candles(funding.obs_at, funding.next_funding_time);

        // 4. The quantile fan (validated-by-construction).
        let quantiles = Self::build_quantiles(point, remaining);
        let predictive = PredictiveDistribution::Scalar {
            quantiles,
            unit: "rate".to_string(),
        };

        // 5. Emit the scalar belief draft. The event_key keys the forecast by
        //    (market, the window it resolves at) so two windows never collide.
        let draft = ScalarBeliefDraft {
            event_key: format!("{market}:{}", funding.next_funding_time.to_iso8601()),
            predictive,
            horizon: funding.next_funding_time,
            evidence: serde_json::json!({
                "estimate": funding.estimate.to_string(),
                "point_forecast": point_decimal.to_string(),
                "remaining_candles": remaining,
            }),
            provenance: serde_json::Value::default(),
        };
        self.pending.push(draft);
        self.metrics.beliefs_drafted += 1;

        // 6. Record the window state.
        state.last_next_funding_time = Some(funding.next_funding_time);
        state.last_estimate = Some(funding.estimate);

        // Zero-capital: NEVER a proposal.
        Ok(Vec::new())
    }

    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }

    /// The additive scalar egress seam (design §2.5): hand off the buffered
    /// drafts, leaving the buffer empty (drain-once).
    fn drain_scalar_beliefs(&mut self) -> Vec<ScalarBeliefDraft> {
        std::mem::take(&mut self.pending)
    }
}
