//! `perp_event_basis_v2` — the propose-only, mechanical, Sim-stage perp/bracket
//! BASIS STRATEGY v2 (docs/design/perp-strategies-and-scalar-claims.md §3.3;
//! GAPS "TRACK C — slice-3b-v2").
//!
//! # What it is
//!
//! This is the v2 successor to [`crate::perp_event_basis`] (rung-0). Where
//! rung-0 compared the perp mark to the ladder's implied MEDIAN and proposed a
//! single bin, v2 prices a per-bracket fair-probability vector `q_j` (A3) on the
//! BRTI settlement ANCHOR (A6), gates the ladder for no-arb coherence (A9),
//! horizon-gates the dispersion (A5), and runs a per-bin expected-value gate
//! with maker adverse-selection (A4+A8) that decides which UNSIZED maker legs to
//! propose.
//!
//! ## Slice history
//!
//! - **V3 (steps 2+3: A3 + A6 anchor + A9).** Wired the kernel
//!   ([`fortuna_cognition::basis_v2`]) into the strategy seam and recorded the
//!   [`V2Eval`] snapshot; it PROPOSED NOTHING (the per-bin EV gate was deferred),
//!   and used the per-step σ DIRECTLY as the model dispersion (a stand-in for the
//!   τ-regime σ).
//! - **V4 (steps 4+5: A5 horizon gating + A4/A8 EV gate) — THIS SLICE, the first
//!   that PROPOSES.** Replaces V3's per-step σ stand-in with the τ-regime-scaled
//!   σ_τ (A5), adds the three load-bearing vetoes (>48h horizon, τ-unknown, stale
//!   anchor), and emits ONE UNSIZED maker leg per ladder bin whose per-bin EV
//!   clears the threshold (A4+A8). Still Sim-stage, still unsized (I6/I7); the EV
//!   is an honest f64 edge claim, NEVER a size.
//!
//! # Discipline (the house invariants this respects — identical to rung-0)
//!
//! - **I6 (propose-only / unsized).** No leg carries a quantity. Every emitted
//!   leg is an UNSIZED `Cents` maker join; the harness sizes.
//! - **I7 (Sim-stage, no auto-promotion).** [`Strategy::stage`] is
//!   [`Stage::Sim`]; the EV is an honest edge claim, never an auto-promotion.
//! - **Money discipline.** `f64` appears ONLY in the forecast domain: the BRTI
//!   anchor lifted to BTC dollars at the SAME single boundary rung-0 uses
//!   (`PerpPrice::to_dollars().to_f64() × 10_000`), the dispersion σ/σ_τ, the τ
//!   math, the probabilities, and the EV. The only money types are `Cents` /
//!   `PerpPrice`. The ONE documented f64→`Cents` boundary is the fair-value
//!   `Cents::new((q_j · 100).round() as i64)` clamped to `[1, 99]` (see
//!   [`Self::fair_cents_from_q`]); the leg's `limit_price` is the bin's own best
//!   YES bid (already `Cents`). No `panic!`/`unwrap()`/`expect()` anywhere —
//!   every fallible step uses `let … else { <degrade> }` or `match`, degrading a
//!   degenerate/missing/stale input to "propose nothing".
//! - **Clock.** Time comes from `core.now` (a `UtcTimestamp`) via
//!   [`UtcTimestamp::epoch_millis`], never `SystemTime::now()`. τ and the Δ
//!   observation-interval are both measured in epoch-millis deltas.
//! - **Untrusted data (spec 5.11).** Quotes, the anchor, the anchor's capture
//!   time, and `close_at` are validated by shape (non-finite / ≤0 / missing /
//!   stale ⇒ veto), never trusted blindly.
//!
//! # The σ estimator (DC-1, strategy state)
//!
//! On each matching `PerpTick` the strategy lifts the BRTI anchor
//! (`funding.reference_price`, A6 — NOT the perp mark) to BTC dollars and pushes
//! it into a bounded ring ([`PerpEventBasisV2Config::vol_buf_len`]). Between
//! consecutive anchors it forms the per-step log-return `r = ln(aₜ / aₜ₋₁)` and
//! maintains an EWMA of `r²` with decay λ
//! ([`PerpEventBasisV2Config::ewma_lambda`]):
//!
//! ```text
//! varₜ = r²                       (seed, on the FIRST return)
//! varₜ = λ·varₜ₋₁ + (1−λ)·r²ₜ     (each subsequent return)
//! σ_step = sqrt(varₜ)  clamped to [sigma_floor, sigma_ceiling]
//! ```
//!
//! σ is "ready" only after at least [`PerpEventBasisV2Config::min_vol_obs`]
//! returns have been folded in; until then the strategy is INACTIVE — it records
//! no [`V2Eval`] and proposes nothing. Every step is guarded: a non-positive
//! anchor or a non-finite return SKIPS that update (no panic).
//!
//! # The Δ observation-interval estimator (A5, DC-1)
//!
//! σ_step is a per-STEP dispersion; to scale it to the bracket horizon τ we need
//! the time UNIT a step represents — the spacing between consecutive BRTI ticks.
//! The strategy maintains an EWMA (same λ) of the per-step gap
//! `Δt_ms = obs_atₜ.epoch_millis() − obs_at₍ₜ₋₁₎.epoch_millis()` between
//! consecutive matching ticks, guarded `> 0` (a non-positive or absent gap is
//! SKIPPED, no panic). `Δ_ms` is undefined until the first positive gap is
//! folded in; while undefined the horizon scaling cannot be formed and every bin
//! is [`HorizonRegime::Disabled`] (propose nothing).
//!
//! # A5 — horizon gating (the V4 refinement that replaces V3's per-step σ)
//!
//! Per TARGET bracket, τ = `close_at.epoch_millis() − core.now.epoch_millis()`
//! ([`PerpEventBasisV2Config::direct_max_ms`] / `vol_adjusted_max_ms` are the
//! regime boundaries):
//!
//! - `close_at` absent (market not in `core.markets`, or `close_at` `None`) OR
//!   `τ ≤ 0` ⇒ [`HorizonRegime::Disabled`] (the conservative DC-4 fallback).
//! - `0 < τ ≤ direct_max_ms` ⇒ [`HorizonRegime::Direct`] (short horizon; τ small
//!   ⇒ σ_τ naturally tight — the spec's "tight point forecast").
//! - `direct_max_ms < τ ≤ vol_adjusted_max_ms` ⇒ [`HorizonRegime::VolAdjusted`]
//!   (σ scales with √τ; the F widens).
//! - `τ > vol_adjusted_max_ms` ⇒ [`HorizonRegime::Disabled`] (the >48h veto: the
//!   point-forecast+σ model is not trustworthy that far out).
//!
//! Both `Direct` and `VolAdjusted` price with the SAME horizon-scaled dispersion
//! (DC-1/A5):
//!
//! ```text
//! σ_τ = σ_step · sqrt(τ_ms / Δ_ms)   clamped to [sigma_floor, sigma_ceiling]
//! ```
//!
//! the regime enum is recorded for diagnostics and drives the `Disabled` veto.
//! σ_τ REPLACES V3's per-step σ in the [`bracket_fair_probs`] call (this is the
//! V4 refinement V3's doc promised). A bin whose Δ is not yet measured, or whose
//! σ_τ is non-finite / ≤ 0, is treated as `Disabled` (no proposal).
//!
//! # A6 — stale-anchor veto (load-bearing)
//!
//! If `core.now − funding.obs_at > max_anchor_age_ms` the BRTI anchor is stale
//! and untrustworthy; mis-anchoring mis-prices every `q_j`, so the WHOLE tick is
//! disabled (propose nothing) and the staleness is recorded in [`V2Eval`].
//!
//! # The per-tick pass (A6-fresh → A9 → A6-anchor → A3 → A5 → A4+A8 → A10)
//!
//! When σ is ready, the anchor valid, and the anchor FRESH (A6), each tick:
//! 1. **A9.** [`validate_ladder_no_arb`] on the bins from `core.books`; an
//!    `Incoherent` ladder is recorded and the model does NOT price.
//! 2. **A6 anchor + A5 + A3.** S₀ = the BRTI reference in BTC dollars; σ_τ is the
//!    horizon-scaled dispersion (per the regime); `q_j = bracket_fair_probs(bins,
//!    {anchor:S₀, sigma:σ_τ})`.
//! 3. **A4+A8 EV gate.** For each priced bin (mapped back to its catalog market
//!    by STRIKE — `bracket_fair_probs` returns canonical PRICE order, not catalog
//!    order), `EV_j = q_j − ask_j − fee_j − slippage − reserve − adverse`; a bin
//!    clears only when `EV_j > ev_threshold` (strict), has a takeable ASK, and a
//!    best BID to join. Each clearing bin emits ONE unsized `Passive`/`Buy`/`Yes`
//!    maker leg joining its best bid (deduped on `(market, limit_cents)`).
//! 4. **A10 diagnostic.** The rung-0 implied median ([`compute_basis`]) for the
//!    SAME bins is stored as a HEALTH metric — NOT a signal, never a gate.
//!
//! The full [`V2Eval`] is stored in `last_eval`; the clearing legs are returned.

use crate::{
    CoreHandle, Proposal, ProposedLeg, RunnerError, Stage, Strategy, StrategyKind, StrategyMetrics,
    Urgency,
};
use async_trait::async_trait;
use fortuna_cognition::basis::{compute_basis, BracketBin, BracketStrike};
use fortuna_cognition::basis_v2::{
    bracket_fair_probs, validate_ladder_no_arb, BracketFairProb, LadderHealth, SettlementModel,
};
use fortuna_core::book::OrderBook;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use rust_decimal::prelude::ToPrimitive;
use std::collections::{BTreeMap, HashSet, VecDeque};

/// The KXBTCPERP contract is BTC/10000, so a per-contract value in dollars is
/// scaled back to a BTC-spot dollar value by ×10000 (`$6.3000 → $63,000`).
/// This is the SAME anchor/mark boundary scale rung-0 fixes
/// ([`crate::perp_event_basis`]); v2 applies it to the BRTI reference (A6), not
/// the perp mark.
const PERP_CONTRACT_BTC_DIVISOR: f64 = 10_000.0;

/// Highest cent price a binary YES leg can claim as fair value: a contract is
/// never worth a full 100c before settlement, so a model probability of `1.0`
/// (a saturated tail) is clamped to 99c. The floor of `1` keeps a degenerate
/// near-zero `q` a strictly-positive, well-formed cent price. Mirrors rung-0's
/// `MAX_FAIR_CENTS` and `MechExtremes`.
const MAX_FAIR_CENTS: i64 = 99;
const MIN_FAIR_CENTS: i64 = 1;

/// The cents-per-probability-unit scale: a YES contract pays $1.00 = 100c, so a
/// model probability `q ∈ [0,1]` maps to `q · 100` cents and a cent ask maps to
/// `ask_cents / 100` probability-units. The single documented f64↔cents bridge
/// of the EV domain.
const CENTS_PER_PROB: f64 = 100.0;

/// The horizon regime selected for a target bracket from τ = `close_at − now`
/// (A5). `Direct` and `VolAdjusted` both PRICE (with the τ-scaled σ_τ); only the
/// regime label and the `Disabled` veto differ. `Disabled` is the conservative
/// fallback for an unknown/expired horizon (DC-4) AND the explicit >48h veto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizonRegime {
    /// `0 < τ ≤ direct_max_ms`: short horizon, σ_τ naturally tight.
    Direct,
    /// `direct_max_ms < τ ≤ vol_adjusted_max_ms`: σ scales with √τ; F widens.
    VolAdjusted,
    /// `close_at` unknown/absent, `τ ≤ 0`, or `τ` beyond `vol_adjusted_max_ms`
    /// (the past-48h veto): the point-forecast+σ model is not trustworthy ⇒
    /// propose nothing. Also the per-bin fallback when σ_τ cannot be formed.
    Disabled,
}

/// Construction config for [`PerpEventBasisV2`] (catalog-driven; the σ knobs are
/// the DC-1 defaults, all overridable). Later v2 slices ADD fields (the τ-regime
/// knobs for A5, the EV-gate margins for A4/A8); that is additive and fine.
#[derive(Debug, Clone)]
pub struct PerpEventBasisV2Config {
    /// The perp whose `PerpTick` triggers the evaluation (e.g. `"KXBTCPERP"`).
    /// A `PerpTick` for any other market is ignored (no σ update, no eval).
    pub perp_market: MarketId,
    /// The KXBTC bracket ladder: each bracket `MarketId` → its strike(s). The
    /// strategy reads each market's book from `core.books` to build the bins;
    /// a catalog market with no/illiquid book contributes probability `0.0`
    /// (the rung-0 `bin_prob` convention, reused verbatim).
    pub ladder: BTreeMap<MarketId, BracketStrike>,
    /// DC-1: the bounded anchor-ring capacity (number of recent BRTI-anchor
    /// BTC-dollar values retained). Caps the strategy's σ state; default 64.
    pub vol_buf_len: usize,
    /// DC-1: the EWMA decay λ for the running variance of log-returns
    /// (`varₜ = λ·varₜ₋₁ + (1−λ)·r²`). Default 0.94 (the RiskMetrics daily
    /// decay). Closer to 1 ⇒ slower-moving σ.
    pub ewma_lambda: f64,
    /// DC-1: the minimum number of per-step log-returns that must be folded into
    /// the EWMA before σ is "ready". Until then the strategy is INACTIVE
    /// (records no eval, proposes nothing). Default 20.
    pub min_vol_obs: usize,
    /// DC-1: a small strictly-positive σ floor. A ready σ is clamped UP to this
    /// so the lognormal model never sees a zero/degenerate dispersion. Default
    /// `1e-6`.
    pub sigma_floor: f64,
    /// DC-1: the σ ceiling. A ready σ is clamped DOWN to this to bound a
    /// pathological vol spike. Default `5.0`.
    pub sigma_ceiling: f64,
    /// A9 / DC-5: the YES-sum tolerance passed to [`validate_ladder_no_arb`] —
    /// the ladder is coherent only when `|Σ implied YES − 1| ≤ no_arb_tol`.
    /// Default `0.05`.
    pub no_arb_tol: f64,

    // ── A5: horizon-regime boundaries (DC; all overridable) ──────────────────
    /// A5: the `Direct`/`VolAdjusted` boundary in epoch-millis. `0 < τ ≤
    /// direct_max_ms` is [`HorizonRegime::Direct`]. Default 4h (`14_400_000`).
    pub direct_max_ms: i64,
    /// A5: the `VolAdjusted`/`Disabled` boundary (the >48h veto) in epoch-millis.
    /// `direct_max_ms < τ ≤ vol_adjusted_max_ms` is
    /// [`HorizonRegime::VolAdjusted`]; beyond it is [`HorizonRegime::Disabled`].
    /// Default 48h (`172_800_000`).
    pub vol_adjusted_max_ms: i64,
    /// A6: the stale-anchor veto age in epoch-millis. If `now − funding.obs_at >
    /// max_anchor_age_ms` the BRTI anchor is stale ⇒ the whole tick is disabled
    /// (propose nothing). BRTI updates ~1/sec; default `5_000`.
    pub max_anchor_age_ms: i64,

    // ── A4 + A8: the per-bin EV-gate knobs (DC-3; all overridable) ────────────
    /// A4: the strict EV threshold in probability-units. A bin clears only when
    /// `EV_j > ev_threshold`. Default `0.02`.
    pub ev_threshold: f64,
    /// A4: the configured slippage margin in probability-units (≈ ½ tick).
    /// Default `0.005`.
    pub slippage: f64,
    /// A4: the configured reserve margin in probability-units. Default `0.01`.
    pub reserve: f64,
    /// A8: the maker adverse-selection penalty in probability-units — a passive
    /// bid fills preferentially when flow is informed against it, so the realized
    /// fill is worse than `q_j − ask_j` implies. V4 uses a CONSTANT baseline;
    /// V5/A7 will upgrade it per-bin from relative informativeness. Default
    /// `0.01`.
    pub adverse: f64,
    /// A4 / amendment C: the maker fee COEFFICIENT for the fee-trap round-trip
    /// fee `2 · ceil(fee_coeff · P · (1−P) · 100) / 100` (P = the YES ask in
    /// probability-units). The Kalshi quadratic maker rate; default `0.0175`.
    /// The cents-rounded-UP ceil is the fee-trap — a promo-$0 never lowers it.
    pub fee_coeff: f64,
}

/// One bin's A4+A8 EV-gate result, recorded for diagnostics (A10) and pinned by
/// tests. Carries the model probability, the executable ask, the computed EV,
/// and whether the bin was proposed. `f64`-forecast throughout (never money).
#[derive(Debug, Clone, PartialEq)]
pub struct BinEv {
    /// The bin's price-axis strike(s) (the key that maps it back to the catalog).
    pub kind: BracketStrike,
    /// A3: the model fair probability `q_j` for this bin.
    pub q: f64,
    /// A4: the executable YES ASK in probability-units (`ask_cents / 100`), or
    /// `None` when the bin had no ask to take toward (then it is never proposed).
    pub ask: Option<f64>,
    /// A4+A8: the per-bin EV `q − ask − fee − slippage − reserve − adverse`, or
    /// `None` when there was no ask to price against.
    pub ev: Option<f64>,
    /// Whether THIS bin was emitted as a proposal (EV cleared the strict
    /// threshold AND a best YES bid existed to join AND the regime priced).
    pub proposed: bool,
}

/// A PUBLIC-readable snapshot of the most recent v2 evaluation (A10 data; the
/// "C produces the numbers" half of the §9 data-vs-view split). Carries the
/// inputs and outputs of one tick's A6→A9→A5→A3→A4/A8→A10 pass so tests (and a
/// future telemetry emitter) can inspect what the model saw, priced, and
/// proposed. The proposal DECISION lives in `on_event`; this is the record.
#[derive(Debug, Clone)]
pub struct V2Eval {
    /// A6: the settlement anchor S₀ used, in BTC dollars (the BRTI
    /// `reference_price` ×10000 — NEVER the perp mark).
    pub anchor: f64,
    /// A5: the horizon-scaled dispersion σ_τ actually fed to the model
    /// (`σ_step · sqrt(τ/Δ)` clamped). Equal to [`Self::sigma_tau`]; kept under
    /// the historical `sigma` name so V3's snapshot readers still compile. When
    /// the regime is `Disabled` (no pricing) this is the per-step σ_step that
    /// WOULD have been scaled (diagnostic only).
    pub sigma: f64,
    /// A5: the horizon-scaled dispersion σ_τ (the same value as [`Self::sigma`];
    /// named explicitly for diagnostics). For a `Disabled` tick this is the
    /// per-step σ_step (unscaled) — no σ_τ was formed.
    pub sigma_tau: f64,
    /// A5: the horizon regime selected for the target bracket(s) this tick. The
    /// ladder shares one `close_at` in the common KXBTC case, so a single regime
    /// is recorded; a mixed ladder records the regime of the priced bins (all
    /// share the same τ when they share `close_at`).
    pub regime: HorizonRegime,
    /// A5: τ = `close_at − now` in epoch-millis for the target bracket(s), or
    /// `None` when `close_at` was unknown/absent (the τ-unknown veto).
    pub tau_ms: Option<i64>,
    /// A5/DC-1: the EWMA observation-interval Δ_ms (BRTI tick spacing) used to
    /// scale σ_step into σ_τ, or `None` until the first positive gap is measured.
    pub delta_ms: Option<f64>,
    /// A6: `true` when the BRTI anchor was STALE (`now − obs_at >
    /// max_anchor_age_ms`) ⇒ the whole tick was disabled (propose nothing).
    pub anchor_stale: bool,
    /// A9: the ladder no-arb health verdict for THIS tick's implied mids.
    pub health: LadderHealth,
    /// A3: the per-bracket model fair-probability vector `q_j` (in canonical
    /// PRICE order, as [`bracket_fair_probs`] returns). EMPTY when the ladder was
    /// incoherent (A9), the model degenerate, the anchor stale (A6), or the
    /// horizon `Disabled` (A5) — the strategy does not price those.
    pub q_j: Vec<BracketFairProb>,
    /// A4+A8: the per-bin EV results (one per priced bin, canonical price order).
    /// EMPTY whenever `q_j` is empty (no pricing happened).
    pub bin_evs: Vec<BinEv>,
    /// A10: the rung-0 implied MEDIAN diagnostic for the SAME bins (a HEALTH
    /// metric, NOT a signal). `None` when the ladder has no finite median (empty
    /// / all-zero / open-tail-crossing), exactly as [`compute_basis`] returns.
    pub median_diagnostic: Option<f64>,
    /// The number of per-step log-returns folded into the EWMA so far (the
    /// "readiness" counter; always ≥ `min_vol_obs` when a `V2Eval` exists).
    pub obs_count: usize,
}

/// The propose-only mechanical perp/bracket basis strategy v2 (V4: A5 horizon
/// gating + A4/A8 EV gate; the first slice that proposes).
pub struct PerpEventBasisV2 {
    id: StrategyId,
    cfg: PerpEventBasisV2Config,
    metrics: StrategyMetrics,
    /// DC-1: the bounded ring of recent BRTI-anchor BTC-dollar values. The back
    /// element is the previous anchor (for the next log-return); capped at
    /// `cfg.vol_buf_len`.
    anchors: VecDeque<f64>,
    /// DC-1: the EWMA variance of log-returns. `None` until the FIRST valid
    /// return seeds it with `r²`.
    ewma_var: Option<f64>,
    /// DC-1: the number of per-step log-returns folded into `ewma_var` (the
    /// readiness counter; σ is "ready" once this ≥ `cfg.min_vol_obs`).
    return_count: usize,
    /// A5/DC-1: the `obs_at` of the PREVIOUS matching tick (the BRTI capture
    /// time), to form the per-step observation gap `Δt_ms`. `None` before the
    /// first matching tick.
    prev_obs_at_ms: Option<i64>,
    /// A5/DC-1: the EWMA of the per-step observation gap `Δt_ms` (BRTI tick
    /// spacing), the time unit σ_step is expressed over. `None` until the first
    /// STRICTLY-POSITIVE gap is folded in (a non-positive/absent gap is skipped).
    ewma_delta_ms: Option<f64>,
    /// The dedup set: a `(market, join-limit cents)` already proposed. The
    /// identical leg is not re-proposed until the bin or its best bid moves
    /// (mirrors rung-0's [`crate::perp_event_basis::PerpEventBasis`]).
    proposed: HashSet<(MarketId, i64)>,
    /// The most recent evaluation snapshot (A10 data), or `None` before σ is
    /// ready or on a degenerate tick. PUBLIC-readable via [`Self::last_eval`].
    last_eval: Option<V2Eval>,
}

impl PerpEventBasisV2 {
    /// Construct the strategy. The only failure mode is an invalid strategy id
    /// (a fixed literal, so it never fires in practice — but the constructor
    /// stays fallible, no `unwrap`, per the money-path discipline; mirrors
    /// rung-0's [`crate::perp_event_basis::PerpEventBasis::new`]).
    pub fn new(cfg: PerpEventBasisV2Config) -> Result<Self, RunnerError> {
        Ok(PerpEventBasisV2 {
            id: StrategyId::new("perp_event_basis_v2").map_err(|e| RunnerError::Config {
                reason: e.to_string(),
            })?,
            cfg,
            metrics: StrategyMetrics::default(),
            anchors: VecDeque::new(),
            ewma_var: None,
            return_count: 0,
            prev_obs_at_ms: None,
            ewma_delta_ms: None,
            proposed: HashSet::new(),
            last_eval: None,
        })
    }

    /// The most recent evaluation snapshot (A10 data), or `None` when σ is not
    /// yet ready or the last matching tick was degenerate. Read-only inspection
    /// for tests and (later) the telemetry emitter; it is never a gate.
    pub fn last_eval(&self) -> Option<&V2Eval> {
        self.last_eval.as_ref()
    }

    /// The YES-mid probability of one catalog market from its book, in `[0,1]`.
    ///
    /// REUSED VERBATIM from rung-0 ([`crate::perp_event_basis`]): the
    /// conventional YES mid `((bid + ask)/2)/100` (cents → probability), where an
    /// ABSENT quote on one side counts as the `0c` floor; only a bin with NO
    /// quote on EITHER side (or no book) carries no implied mass → `0.0`. The
    /// kernel/fixture correctness depends on this exact convention (a one-sided
    /// `0 bid / Nc ask` bin contributes `ask/2`, never 0), so it is copied, not
    /// re-derived. `f64` here is the forecast domain (a probability), never
    /// money.
    fn bin_prob(book: Option<&OrderBook>) -> f64 {
        let Some(book) = book else {
            return 0.0;
        };
        // A bin with no quote on either side carries no implied mass.
        if book.yes_bids.is_empty() && book.yes_asks.is_empty() {
            return 0.0;
        }
        let bid = book.yes_bids.first().map_or(0, |l| l.price.raw());
        let ask = book.yes_asks.first().map_or(0, |l| l.price.raw());
        ((bid + ask) as f64 / 2.0) / 100.0
    }

    /// Build the ladder bins from the catalog + the point-in-time books (one
    /// [`BracketBin`] per catalog market, in `BTreeMap` order). Mirrors rung-0's
    /// `build_bins`; the kernel re-sorts into canonical price order, so the
    /// iteration order here is immaterial to the outputs but stays deterministic.
    fn build_bins(&self, core: &CoreHandle<'_>) -> Vec<BracketBin> {
        self.cfg
            .ladder
            .iter()
            .map(|(mkt, strike)| BracketBin {
                kind: *strike,
                prob: Self::bin_prob(core.books.get(mkt)),
            })
            .collect()
    }

    /// DC-1: fold one freshly-observed BRTI anchor (BTC dollars) into the σ
    /// state. Pushes it into the bounded ring, and if there is a previous anchor
    /// computes the per-step log-return `r = ln(aₜ / aₜ₋₁)` and updates the EWMA
    /// of `r²` (seeding on the first return). Returns nothing; readiness is read
    /// from `self.return_count`.
    ///
    /// GUARDS (untrusted data; no panic): a non-finite or `≤ 0` anchor is NOT
    /// pushed and produces no return (it cannot be a denominator/`ln` operand);
    /// a previous anchor that is `≤ 0` (only possible if a prior guard let one
    /// through — it cannot) or a non-finite computed return is skipped. So a
    /// degenerate feed neither poisons σ nor advances readiness.
    fn update_sigma(&mut self, anchor_btc: f64) {
        // Reject a degenerate anchor outright: it is neither a valid ring entry
        // (a future denominator) nor a valid `ln` operand.
        if !anchor_btc.is_finite() || anchor_btc <= 0.0 {
            return;
        }

        // Compute the return against the PREVIOUS anchor (the ring's back), if
        // any, BEFORE pushing this one.
        if let Some(&prev) = self.anchors.back() {
            // `prev` was screened (>0, finite) when it was pushed, and
            // `anchor_btc` is screened above, so neither the division nor the
            // `ln` can produce a NaN/inf from a bad operand. The extra
            // `is_finite` on `r` is belt-and-suspenders (e.g. an absurd ratio
            // overflow) — degrade by skipping, never panic.
            let r = (anchor_btc / prev).ln();
            if r.is_finite() {
                let r2 = r * r;
                self.ewma_var = Some(match self.ewma_var {
                    None => r2,
                    Some(v) => self.cfg.ewma_lambda * v + (1.0 - self.cfg.ewma_lambda) * r2,
                });
                self.return_count = self.return_count.saturating_add(1);
            }
        }

        // Push the new anchor, holding the ring to its configured cap. A cap of
        // 0 keeps no history (so no returns ever form) — degenerate but safe.
        self.anchors.push_back(anchor_btc);
        while self.anchors.len() > self.cfg.vol_buf_len {
            self.anchors.pop_front();
        }
    }

    /// DC-1: the ready, clamped σ — `Some(sqrt(ewma_var)` clamped to
    /// `[sigma_floor, sigma_ceiling]`) once at least `min_vol_obs` returns have
    /// been folded in; `None` while not ready (so the strategy stays INACTIVE).
    /// The clamp guarantees a ready σ is finite and strictly positive (the floor
    /// is `> 0`), so [`bracket_fair_probs`] never sees a degenerate dispersion
    /// from a ready strategy.
    fn ready_sigma(&self) -> Option<f64> {
        if self.return_count < self.cfg.min_vol_obs {
            return None;
        }
        let var = self.ewma_var?;
        if !var.is_finite() || var < 0.0 {
            return None;
        }
        let sigma = var
            .sqrt()
            .clamp(self.cfg.sigma_floor, self.cfg.sigma_ceiling);
        // Defensive: a non-finite floor/ceiling config could defeat the clamp.
        if sigma.is_finite() && sigma > 0.0 {
            Some(sigma)
        } else {
            None
        }
    }

    /// A5/DC-1: fold one matching tick's BRTI capture time (`obs_at`, epoch
    /// millis) into the observation-interval EWMA. The per-step gap
    /// `Δt_ms = obs_atₜ − obs_at₍ₜ₋₁₎` is folded with the same λ as σ; the first
    /// strictly-positive gap SEEDS it. Returns nothing; readiness is read from
    /// [`Self::ready_delta`].
    ///
    /// GUARDS (untrusted timestamps; no panic): a non-positive gap (a frozen or
    /// non-monotone `obs_at`) or the very first tick (no previous `obs_at`) is
    /// SKIPPED — it neither seeds nor advances Δ. `prev_obs_at_ms` is ALWAYS
    /// updated to the current tick so the next gap is measured against it.
    fn update_delta(&mut self, obs_at_ms: i64) {
        if let Some(prev) = self.prev_obs_at_ms {
            // `i64` subtraction of two real epoch-millis cannot overflow in
            // practice (both are bounded calendar times); a non-positive gap is
            // skipped (frozen/backwards `obs_at`).
            let gap = obs_at_ms.saturating_sub(prev);
            if gap > 0 {
                let g = gap as f64;
                self.ewma_delta_ms = Some(match self.ewma_delta_ms {
                    None => g,
                    Some(d) => self.cfg.ewma_lambda * d + (1.0 - self.cfg.ewma_lambda) * g,
                });
            }
        }
        self.prev_obs_at_ms = Some(obs_at_ms);
    }

    /// A5/DC-1: the measured observation interval Δ_ms (the BRTI tick spacing),
    /// or `None` until the first positive gap is folded in. A `Some` value is
    /// guaranteed finite and `> 0` (it is an EWMA of strictly-positive gaps), so
    /// it is a safe denominator for the σ_τ scaling.
    fn ready_delta(&self) -> Option<f64> {
        let d = self.ewma_delta_ms?;
        if d.is_finite() && d > 0.0 {
            Some(d)
        } else {
            None
        }
    }

    /// A5: classify the horizon regime for a target bracket from τ = `close_at −
    /// now` in epoch-millis. `close_at` absent (the bracket is not in
    /// `core.markets`, or its `close_at` is `None`) OR `τ ≤ 0` ⇒
    /// [`HorizonRegime::Disabled`] (the conservative DC-4 fallback); `0 < τ ≤
    /// direct_max_ms` ⇒ `Direct`; `direct_max_ms < τ ≤ vol_adjusted_max_ms` ⇒
    /// `VolAdjusted`; `τ > vol_adjusted_max_ms` ⇒ `Disabled` (the >48h veto).
    /// Returns the regime AND τ (so the caller records both). A pure function of
    /// `(close_at, now, cfg)`.
    fn classify_regime(
        &self,
        close_at: Option<UtcTimestamp>,
        now: UtcTimestamp,
    ) -> (HorizonRegime, Option<i64>) {
        let Some(close_at) = close_at else {
            // τ unknown ⇒ Disabled, τ recorded as None.
            return (HorizonRegime::Disabled, None);
        };
        let tau_ms = close_at.epoch_millis() - now.epoch_millis();
        if tau_ms <= 0 {
            // Already closed / non-positive horizon ⇒ Disabled (but τ is known).
            (HorizonRegime::Disabled, Some(tau_ms))
        } else if tau_ms <= self.cfg.direct_max_ms {
            (HorizonRegime::Direct, Some(tau_ms))
        } else if tau_ms <= self.cfg.vol_adjusted_max_ms {
            (HorizonRegime::VolAdjusted, Some(tau_ms))
        } else {
            // The >48h veto.
            (HorizonRegime::Disabled, Some(tau_ms))
        }
    }

    /// A5/DC-1: the horizon-scaled dispersion `σ_τ = σ_step · sqrt(τ_ms / Δ_ms)`
    /// clamped to `[sigma_floor, sigma_ceiling]`. Returns `None` (treat the bin
    /// as Disabled — no proposal) when `τ_ms ≤ 0`, Δ is degenerate (its caller
    /// already screens it), or the scaled σ_τ is non-finite / `≤ 0` after the
    /// clamp. A `Some` value is finite and strictly positive, so
    /// [`bracket_fair_probs`] never sees a degenerate dispersion.
    fn sigma_tau(&self, sigma_step: f64, tau_ms: i64, delta_ms: f64) -> Option<f64> {
        if tau_ms <= 0 || !delta_ms.is_finite() || delta_ms <= 0.0 {
            return None;
        }
        let ratio = (tau_ms as f64) / delta_ms;
        if !ratio.is_finite() || ratio < 0.0 {
            return None;
        }
        let scaled =
            (sigma_step * ratio.sqrt()).clamp(self.cfg.sigma_floor, self.cfg.sigma_ceiling);
        if scaled.is_finite() && scaled > 0.0 {
            Some(scaled)
        } else {
            None
        }
    }

    /// A4 / amendment C: the round-trip maker fee in probability-units for a YES
    /// ask `p`: per leg `ceil(fee_coeff · p · (1−p) · 100) / 100` (cents-rounded
    /// UP — the fee-trap, so a promo-$0 can never lower it), ×2 for enter+exit.
    /// `p` is the executable ask in probability-units (forecast domain — this is
    /// an EV term, NEVER a money type). C = 1 contract (the leg is UNSIZED).
    fn fee_round_trip(&self, p: f64) -> f64 {
        let per_leg = (self.cfg.fee_coeff * p * (1.0 - p) * CENTS_PER_PROB).ceil() / CENTS_PER_PROB;
        2.0 * per_leg
    }

    /// A4: the bin's best EXECUTABLE YES ASK in probability-units
    /// (`best_yes_ask_cents / 100`), or `None` when the bin has no ask (you
    /// cannot buy/much less join toward a non-existent offer ⇒ skip the bin).
    fn bin_ask(book: Option<&OrderBook>) -> Option<f64> {
        let ask_cents = book?.yes_asks.first()?.price.raw();
        Some((ask_cents as f64) / CENTS_PER_PROB)
    }

    /// The bin's best YES BID as a `Cents` join limit, or `None` when there is no
    /// bid to join (a maker-only leg cannot rest without a price ⇒ skip the bin).
    fn bin_best_bid(book: Option<&OrderBook>) -> Option<Cents> {
        Some(book?.yes_bids.first()?.price)
    }

    /// The ONE documented f64→`Cents` boundary of the EV domain: the leg's honest
    /// fair value `Cents::new((q · 100).round() as i64)` clamped to
    /// `[MIN_FAIR_CENTS, MAX_FAIR_CENTS]` (`[1, 99]`). A model probability of
    /// `1.0` (a saturated tail) clamps to 99c; a near-zero `q` clamps up to 1c.
    /// `q` is a forecast-domain probability; the cents result is the only money
    /// value this strategy mints, and the gates re-check net edge from it.
    fn fair_cents_from_q(q: f64) -> Cents {
        // `q` is screened finite ∈ [0,1] before this is called (it is a kernel
        // output); the clamp makes the cast total even on a degenerate q.
        let raw = (q * CENTS_PER_PROB).round() as i64;
        Cents::new(raw.clamp(MIN_FAIR_CENTS, MAX_FAIR_CENTS))
    }

    /// A4+A8: the per-bin EV in probability-units
    /// `q − ask − fee_round_trip(ask) − slippage − reserve − adverse`. Pure
    /// forecast-domain f64; the strict `> ev_threshold` decision lives in the
    /// caller (mirroring the rung-0 fee-trap strictness).
    fn ev_for_bin(&self, q: f64, ask: f64) -> f64 {
        q - ask - self.fee_round_trip(ask) - self.cfg.slippage - self.cfg.reserve - self.cfg.adverse
    }

    /// A5: the REPRESENTATIVE horizon regime + τ for the ladder, for the ONE σ_τ
    /// the kernel call needs. KXBTC brackets share one settlement in the common
    /// case, so this is exact; for a mixed ladder it takes the NEAREST positive,
    /// in-window horizon (the shortest τ ⇒ the tightest, most conservative σ_τ)
    /// and the EV loop additionally vetoes any bin whose OWN regime is Disabled.
    ///
    /// Walks every catalog bracket, classifies each via [`Self::classify_regime`]
    /// off `core.markets[bracket].close_at` and `core.now`, and returns:
    /// - the bracket with the smallest POSITIVE `Direct`/`VolAdjusted` τ (regime
    ///   + τ), if any priceable bracket exists; else
    /// - `(Disabled, None)` when no bracket has a known, positive, in-window
    ///   horizon (every bracket vetoed — τ unknown, expired, or >48h).
    fn representative_regime(&self, core: &CoreHandle<'_>) -> (HorizonRegime, Option<i64>) {
        let mut best: Option<(HorizonRegime, i64)> = None;
        for id in self.cfg.ladder.keys() {
            let close_at = core.markets.get(id).and_then(|m| m.close_at);
            let (regime, tau) = self.classify_regime(close_at, core.now);
            // Only a priceable (non-Disabled) bracket with a positive τ is a
            // candidate; pick the nearest such horizon.
            if regime != HorizonRegime::Disabled {
                if let Some(tau) = tau {
                    let take = match best {
                        None => true,
                        Some((_, best_tau)) => tau < best_tau,
                    };
                    if take {
                        best = Some((regime, tau));
                    }
                }
            }
        }
        match best {
            Some((regime, tau)) => (regime, Some(tau)),
            None => (HorizonRegime::Disabled, None),
        }
    }

    /// Map a priced bin's [`BracketStrike`] back to its catalog `(MarketId,
    /// BracketStrike)` by EXACT strike equality. [`bracket_fair_probs`] returns
    /// the `q_j` vector in canonical PRICE order (it re-sorts), NOT the catalog's
    /// order, so a bin must be matched by its strike — never by vector position.
    /// The strike `f64`s are copied verbatim from the catalog into the kernel and
    /// back out, so `==` is exact (no rounding occurs on the round trip). Returns
    /// the FIRST matching catalog entry (a well-formed ladder has at most one bin
    /// per strike); `None` for a strike with no catalog match.
    fn catalog_entry_for(&self, kind: &BracketStrike) -> Option<(&MarketId, &BracketStrike)> {
        self.cfg.ladder.iter().find(|(_, strike)| *strike == kind)
    }
}

#[async_trait]
impl Strategy for PerpEventBasisV2 {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }

    /// Mechanical: deterministic, no mind, no cognition spend (design §3.3 — v2
    /// is still a mechanical strategy; the model is a closed-form CDF, not an
    /// LLM).
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }

    /// Sim only (design §3.3/§7; I7 — no auto-promotion). The EV is an honest
    /// edge claim, never a size or an auto-promotion.
    fn stage(&self) -> Stage {
        Stage::Sim
    }

    /// On a matching `PerpTick`: fold σ and Δ, then (when σ is ready, the anchor
    /// valid+fresh, the ladder coherent, and the horizon priced) run the
    /// A6→A9→A5→A3→A4/A8→A10 evaluation, store the [`V2Eval`] snapshot, and emit
    /// ONE unsized maker leg per bin whose EV clears. No panic/unwrap anywhere;
    /// every degenerate/missing/stale input degrades to "propose nothing".
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;

        // 1. Only this strategy's perp's PerpTick triggers. The BRTI anchor is
        //    `funding.reference_price` (A6) — NOT `marks.venue_settlement`.
        let EventPayload::PerpTick {
            market, funding, ..
        } = &ev.payload
        else {
            return Ok(Vec::new());
        };
        if market != &self.cfg.perp_market {
            return Ok(Vec::new());
        }

        // 2. The BRTI anchor → BTC-spot dollars, at the SAME single price-domain
        //    boundary rung-0 uses (`to_dollars` is exact Decimal; ×10000 lifts
        //    the per-contract value to the BTC value the kernel prices on). On a
        //    (degenerate) Decimal→f64 failure, degrade to no eval — never unwrap.
        let Some(per_contract) = funding.reference_price.to_dollars().to_f64() else {
            return Ok(Vec::new());
        };
        let anchor_btc = per_contract * PERP_CONTRACT_BTC_DIVISOR;

        // 3. Fold the anchor into the σ estimator AND the Δ observation-interval
        //    estimator (A5/DC-1). A degenerate anchor is skipped inside
        //    `update_sigma`; `update_delta` always records `obs_at` for the next
        //    gap (a non-positive/frozen gap is skipped). Both no-panic.
        self.update_sigma(anchor_btc);
        self.update_delta(funding.obs_at.epoch_millis());

        // 4. Until σ is ready, the strategy is INACTIVE: no eval, no proposal.
        let Some(sigma_step) = self.ready_sigma() else {
            return Ok(Vec::new());
        };
        if !anchor_btc.is_finite() || anchor_btc <= 0.0 {
            // A ready σ but a degenerate CURRENT anchor (e.g. a zero-reference
            // tick after warm-up): cannot price this tick. Leave `last_eval`
            // unchanged (the prior good snapshot, if any) and propose nothing.
            return Ok(Vec::new());
        }

        // 5. A6 STALE-ANCHOR veto (load-bearing). If the BRTI capture time is
        //    older than `max_anchor_age_ms`, the anchor is untrustworthy and
        //    mis-prices every q_j ⇒ DISABLE the whole tick. Record the staleness;
        //    do not price; propose nothing. (`core.now` is the injected clock.)
        let anchor_age_ms = core.now.epoch_millis() - funding.obs_at.epoch_millis();
        let anchor_stale = anchor_age_ms > self.cfg.max_anchor_age_ms;

        // 6. Build the ladder bins + the A10 median diagnostic (mark-independent;
        //    reuse `compute_basis`, read ONLY `bracket_implied_median`).
        let bins = self.build_bins(core);
        let median_diagnostic =
            compute_basis(&bins, anchor_btc, 0.0, 0.0).map(|s| s.bracket_implied_median);

        // 7. A9 no-arb gate (only meaningful if the anchor is fresh — a stale
        //    anchor disables regardless). An incoherent ladder records the
        //    verdict with empty q_j and proposes nothing.
        let health = validate_ladder_no_arb(&bins, self.cfg.no_arb_tol);

        // 8. A5 HORIZON regime + τ for the target bracket(s). KXBTC ladders share
        //    one settlement in the common case, so a single representative τ
        //    suffices; a mixed ladder is handled by gating each bin on ITS OWN
        //    regime in the EV loop (step 10). The representative τ is the NEAREST
        //    positive horizon across the catalog (conservative: shortest τ ⇒
        //    tightest σ_τ); if NO bracket has a positive, in-window horizon the
        //    representative regime is Disabled and the whole tick proposes
        //    nothing. τ is read from `core.markets[bracket].close_at` vs
        //    `core.now`.
        let (repr_regime, repr_tau_ms) = self.representative_regime(core);

        // 9. σ_τ (A5): the horizon-scaled dispersion that REPLACES V3's per-step
        //    σ in the kernel call. `None` when Δ is unmeasured, τ is non-positive,
        //    or the scaled σ_τ is degenerate ⇒ treat the tick as Disabled.
        let delta_ms = self.ready_delta();
        let sigma_tau = match (repr_tau_ms, delta_ms) {
            (Some(tau), Some(delta)) => self.sigma_tau(sigma_step, tau, delta),
            _ => None,
        };

        // The tick PRICES only when: the anchor is fresh (A6), the ladder is
        // coherent (A9), the representative regime is not Disabled (A5 >48h /
        // τ-unknown veto), and σ_τ is well-formed (Δ ready + finite σ_τ). Any
        // failure ⇒ empty q_j ⇒ no EV gate ⇒ propose nothing.
        let prices = !anchor_stale
            && matches!(health, LadderHealth::Coherent)
            && repr_regime != HorizonRegime::Disabled
            && sigma_tau.is_some();

        let (q_j, sigma_used) = match (prices, sigma_tau) {
            (true, Some(st)) => (
                // A6 anchor + A5 σ_τ + A3 q_j: price the per-bracket fair
                // probabilities off the BRTI anchor S₀ with the HORIZON-scaled σ_τ.
                bracket_fair_probs(
                    &bins,
                    SettlementModel {
                        anchor: anchor_btc,
                        sigma: st,
                    },
                ),
                st,
            ),
            // Not pricing: record the per-step σ_step in the snapshot's σ fields
            // (diagnostic — no σ_τ was formed) and carry an empty q_j.
            _ => (Vec::new(), sigma_step),
        };

        // 10. A4+A8 PER-BIN EV gate. For each priced bin, map it back to its
        //     catalog `(MarketId, book)` by STRIKE (the kernel returns canonical
        //     PRICE order, NOT catalog order, so position is meaningless), compute
        //     `EV_j = q − ask − fee − slippage − reserve − adverse`, and emit ONE
        //     unsized `Passive`/`Buy`/`Yes` maker leg joining the bin's best BID
        //     when `EV_j > ev_threshold` (strict) AND an ask exists AND a bid
        //     exists AND the bin's OWN regime is not Disabled. Dedup on
        //     `(market, limit_cents)`.
        let mut proposals: Vec<Proposal> = Vec::new();
        let mut bin_evs: Vec<BinEv> = Vec::with_capacity(q_j.len());
        for fp in &q_j {
            // Map this priced bin back to its catalog market by exact strike
            // equality (strikes are copied verbatim from the catalog, so `==`
            // holds). A bin with no catalog match cannot be addressed/joined.
            let Some((market, _strike)) = self.catalog_entry_for(&fp.kind) else {
                bin_evs.push(BinEv {
                    kind: fp.kind,
                    q: fp.q,
                    ask: None,
                    ev: None,
                    proposed: false,
                });
                continue;
            };
            let market = market.clone();
            let book = core.books.get(&market);

            // The executable YES ask (the price you take toward). No ask ⇒ skip.
            let Some(ask) = Self::bin_ask(book) else {
                bin_evs.push(BinEv {
                    kind: fp.kind,
                    q: fp.q,
                    ask: None,
                    ev: None,
                    proposed: false,
                });
                continue;
            };
            let ev = self.ev_for_bin(fp.q, ask);

            // Per-bin regime veto (mixed-ladder safety): a bin whose own horizon
            // is Disabled is never proposed even if the representative priced.
            let (bin_regime, _bin_tau) =
                self.classify_regime(core.markets.get(&market).and_then(|m| m.close_at), core.now);

            // The EV must STRICTLY clear, a best bid must exist to join (maker-
            // only cannot rest without a price), and the bin's regime must price.
            let clears = ev > self.cfg.ev_threshold;
            let best_bid = Self::bin_best_bid(book);
            let mut proposed = false;
            if clears && bin_regime != HorizonRegime::Disabled {
                if let Some(limit) = best_bid {
                    // Dedup: the identical (market, limit) leg fires once until
                    // the bin or its best bid moves.
                    if self.proposed.insert((market.clone(), limit.raw())) {
                        let fair = Self::fair_cents_from_q(fp.q);
                        proposals.push(Proposal {
                            legs: vec![ProposedLeg {
                                market: market.clone(),
                                side: Side::Yes,
                                action: Action::Buy,
                                limit_price: limit,
                                fair_value: fair,
                                calibrated_p: None,
                            }],
                            group_policy: None,
                            urgency: Urgency::Passive,
                            manifest_hash: None,
                            thesis: format!(
                                "perp/bracket basis v2 (A4+A8 EV): regime {regime:?}, \
                                 τ {tau_h:.2}h, σ_τ {sigma:.5}; bin {kind:?} q {q:.4} \
                                 vs YES ask {ask:.4} ⇒ EV {ev:.4} (> thr {thr:.4}); join \
                                 YES bid {limit} on {market} (fair {fair} = round(q·100) \
                                 clamped, UNSIZED — the harness sizes, I6)",
                                regime = bin_regime,
                                tau_h = repr_tau_ms.unwrap_or(0) as f64 / 3_600_000.0,
                                sigma = sigma_used,
                                kind = fp.kind,
                                q = fp.q,
                                ask = ask,
                                ev = ev,
                                thr = self.cfg.ev_threshold,
                                limit = limit,
                                market = market,
                                fair = fair,
                            ),
                        });
                        self.metrics.proposals_emitted += 1;
                        proposed = true;
                    }
                }
            }
            bin_evs.push(BinEv {
                kind: fp.kind,
                q: fp.q,
                ask: Some(ask),
                ev: Some(ev),
                proposed,
            });
        }

        // 11. Store the full evaluation snapshot (A10 data) and return the
        //     clearing legs (may be empty). The snapshot records the regime, τ,
        //     σ_τ, the stale flag, q_j, and the per-bin EV results.
        self.last_eval = Some(V2Eval {
            anchor: anchor_btc,
            sigma: sigma_used,
            sigma_tau: sigma_used,
            regime: repr_regime,
            tau_ms: repr_tau_ms,
            delta_ms,
            anchor_stale,
            health,
            q_j,
            bin_evs,
            median_diagnostic,
            obs_count: self.return_count,
        });

        Ok(proposals)
    }

    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}
