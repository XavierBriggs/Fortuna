//! `perp_event_basis_v2` — the propose-only, mechanical, Sim-stage perp/bracket
//! BASIS STRATEGY v2, DATA-ONLY rung (docs/design/perp-strategies-and-scalar-
//! claims.md §3.3, build-order steps 2+3: A3 + A6 anchor + A9 no-arb;
//! GAPS "TRACK C — slice-3b-v2").
//!
//! # What it is (this slice, V3)
//!
//! This is the v2 successor to [`crate::perp_event_basis`] (rung-0). Where
//! rung-0 compared the perp mark to the ladder's implied MEDIAN and proposed a
//! single bin, v2 prices a per-bracket fair-probability vector `q_j` (A3) on the
//! BRTI settlement ANCHOR (A6) and gates the ladder for no-arb coherence (A9).
//!
//! **V3 PROPOSES NOTHING.** It wires the v2 kernel
//! ([`fortuna_cognition::basis_v2`]) into the strategy seam and records a
//! PUBLIC-readable evaluation snapshot ([`V2Eval`]) on every matching tick, but
//! it emits ZERO proposals: the per-bin expected-value gate that turns `q_j`
//! into UNSIZED maker legs is the V4 slice. Each `on_event` returns `Ok(vec![])`.
//! This is a DATA-ONLY Sim-stage observation, NOT an edge claim (I7).
//!
//! # Discipline (the house invariants this respects — identical to rung-0)
//!
//! - **I6 (propose-only / unsized).** No leg carries a quantity. V3 proposes
//!   nothing at all; the eventual V4 EV gate will emit only UNSIZED `Cents` legs.
//! - **I7 (Sim-stage, no auto-promotion).** [`Strategy::stage`] is
//!   [`Stage::Sim`]; this snapshot is data, never a promotion trigger.
//! - **Money discipline.** `f64` appears ONLY in the forecast domain: the BRTI
//!   anchor lifted to BTC dollars at the SAME single boundary rung-0 uses
//!   (`PerpPrice::to_dollars().to_f64() × 10_000`), the dispersion σ, and the
//!   probabilities the kernel consumes. The only money types are `Cents` /
//!   `PerpPrice`. No `f64` touches a `Cents`; no `panic!`/`unwrap()`/`expect()`
//!   anywhere — every fallible step uses `let … else { <degrade> }` or `match`,
//!   degrading a degenerate/missing input to "no evaluation / propose nothing".
//! - **Clock.** Time, when read, comes from `core.now` (a `UtcTimestamp`), never
//!   `SystemTime::now()`. V3 does not yet need τ (that is the V4/A5 slice).
//! - **Untrusted data (spec 5.11).** Quotes and the anchor are validated by
//!   shape (non-finite / ≤0 ⇒ skip), never trusted blindly.
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
//! no [`V2Eval`] (`last_eval()` stays `None`) and proposes nothing. Every
//! arithmetic step is guarded: a non-positive anchor or a non-finite return
//! SKIPS that update (no panic), so a degenerate feed can never poison σ.
//!
//! **√τ horizon-scaling of σ is DELIBERATELY DEFERRED to the V4/A5 slice.** V3
//! uses the per-step σ_step DIRECTLY as the [`SettlementModel::sigma`] so the
//! `q_j` wiring is exercised and testable NOW; V4 will replace σ_step with the
//! τ-regime-scaled σ (short-horizon direct / vol-adjusted / >48h veto) without
//! changing this seam.
//!
//! # The per-tick evaluation (A9 → A6 → A3 → A10)
//!
//! When σ is ready and the anchor is valid, each matching tick:
//! 1. **A9 first.** [`validate_ladder_no_arb`] on the bins built from
//!    `core.books`. An `Incoherent` verdict is recorded and the model does NOT
//!    price (you cannot compare `q_j` to an incoherent price vector).
//! 2. **A6 anchor.** S₀ = the BRTI reference (`funding.reference_price`) in BTC
//!    dollars — never the perp mark.
//! 3. **A3.** `q_j = bracket_fair_probs(bins, SettlementModel{anchor:S₀,
//!    sigma:σ_step})`. An empty vector (degenerate model) records "no pricing".
//! 4. **A10 diagnostic.** The rung-0 implied median ([`compute_basis`]) for the
//!    SAME bins is computed and stored as a HEALTH metric — it is NOT a signal
//!    and never gates anything here.
//!
//! The full [`V2Eval`] is stored in `last_eval`; `on_event` returns `Ok(vec![])`.

use crate::{CoreHandle, Proposal, RunnerError, Stage, Strategy, StrategyKind, StrategyMetrics};
use async_trait::async_trait;
use fortuna_cognition::basis::{compute_basis, BracketBin, BracketStrike};
use fortuna_cognition::basis_v2::{
    bracket_fair_probs, validate_ladder_no_arb, BracketFairProb, LadderHealth, SettlementModel,
};
use fortuna_core::book::OrderBook;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::market::{MarketId, StrategyId};
use rust_decimal::prelude::ToPrimitive;
use std::collections::{BTreeMap, VecDeque};

/// The KXBTCPERP contract is BTC/10000, so a per-contract value in dollars is
/// scaled back to a BTC-spot dollar value by ×10000 (`$6.3000 → $63,000`).
/// This is the SAME anchor/mark boundary scale rung-0 fixes
/// ([`crate::perp_event_basis`]); v2 applies it to the BRTI reference (A6), not
/// the perp mark.
const PERP_CONTRACT_BTC_DIVISOR: f64 = 10_000.0;

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
}

/// A PUBLIC-readable snapshot of the most recent v2 evaluation (A10 data; the
/// "C produces the numbers" half of the §9 data-vs-view split). Carries the
/// inputs and outputs of one tick's A9→A6→A3→A10 pass so tests (and a future
/// telemetry emitter) can inspect what the model saw and produced. Purely
/// diagnostic: NOTHING here gates a proposal in V3 (which proposes nothing).
#[derive(Debug, Clone)]
pub struct V2Eval {
    /// A6: the settlement anchor S₀ used, in BTC dollars (the BRTI
    /// `reference_price` ×10000 — NEVER the perp mark).
    pub anchor: f64,
    /// A5 (V3 stand-in): the per-step σ used as [`SettlementModel::sigma`]
    /// (clamped to `[sigma_floor, sigma_ceiling]`). V4 replaces this with the
    /// τ-regime-scaled σ.
    pub sigma: f64,
    /// A9: the ladder no-arb health verdict for THIS tick's implied mids.
    pub health: LadderHealth,
    /// A3: the per-bracket model fair-probability vector `q_j`. EMPTY when the
    /// ladder was incoherent (A9) or the model degenerate — the strategy does
    /// not price against an incoherent/degenerate ladder.
    pub q_j: Vec<BracketFairProb>,
    /// A10: the rung-0 implied MEDIAN diagnostic for the SAME bins (a HEALTH
    /// metric, NOT a signal). `None` when the ladder has no finite median (empty
    /// / all-zero / open-tail-crossing), exactly as [`compute_basis`] returns.
    pub median_diagnostic: Option<f64>,
    /// The number of per-step log-returns folded into the EWMA so far (the
    /// "readiness" counter; always ≥ `min_vol_obs` when a `V2Eval` exists).
    pub obs_count: usize,
}

/// The propose-only mechanical perp/bracket basis strategy v2 (DATA-ONLY V3).
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

    /// Sim only (design §3.3/§7; I7 — no auto-promotion). V3 is data-only.
    fn stage(&self) -> Stage {
        Stage::Sim
    }

    /// On a matching `PerpTick`: update σ, then (when σ is ready and the anchor
    /// is valid) run the A9→A6→A3→A10 evaluation and store the [`V2Eval`]
    /// snapshot. ALWAYS returns `Ok(vec![])` — V3 proposes NOTHING (the per-bin
    /// EV gate is the V4 slice). No panic/unwrap anywhere.
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

        // 3. Fold the anchor into the σ estimator (DC-1). A degenerate anchor is
        //    skipped inside `update_sigma` (no panic, no readiness advance).
        self.update_sigma(anchor_btc);

        // 4. Until σ is ready, the strategy is INACTIVE: no eval, no proposal.
        //    (Readiness is independent of the current anchor's validity, but a
        //    degenerate current anchor would also have failed `lognormal_cdf`
        //    below — we screen it explicitly so the eval is never built on one.)
        let Some(sigma) = self.ready_sigma() else {
            return Ok(Vec::new());
        };
        if !anchor_btc.is_finite() || anchor_btc <= 0.0 {
            // A ready σ but a degenerate CURRENT anchor (e.g. a zero-reference
            // tick after warm-up): cannot price this tick. Leave `last_eval`
            // unchanged (the prior good snapshot, if any) and propose nothing.
            return Ok(Vec::new());
        }

        // 5. Build the ladder bins from the catalog + point-in-time books (the
        //    rung-0 `bin_prob` convention, reused verbatim in `build_bins`).
        let bins = self.build_bins(core);

        // 6. A10 diagnostic FIRST (mark-independent): the rung-0 implied median
        //    for the SAME bins, demoted to a HEALTH metric (NOT a signal). The
        //    perp mark is irrelevant to the median field, so we pass the anchor
        //    and zero floors purely to reuse `compute_basis`; we read ONLY
        //    `bracket_implied_median`.
        let median_diagnostic =
            compute_basis(&bins, anchor_btc, 0.0, 0.0).map(|s| s.bracket_implied_median);

        // 7. A9 no-arb gate. An incoherent ladder ⇒ record the verdict with an
        //    EMPTY q_j (you cannot price against an incoherent price vector) and
        //    propose nothing.
        let health = validate_ladder_no_arb(&bins, self.cfg.no_arb_tol);
        let q_j = match health {
            LadderHealth::Coherent => {
                // 8. A6 anchor + A3 q_j: price the per-bracket fair probabilities
                //    off the BRTI anchor S₀ with the per-step σ. An empty vector
                //    (degenerate model / bad strike) records "no pricing".
                bracket_fair_probs(
                    &bins,
                    SettlementModel {
                        anchor: anchor_btc,
                        sigma,
                    },
                )
            }
            LadderHealth::Incoherent(_) => Vec::new(),
        };

        // 9. Store the full evaluation snapshot (A10 data). RETURN nothing —
        //    V3 proposes NOTHING; the per-bin EV gate is the V4 slice. Do NOT
        //    increment `proposals_emitted` (none are emitted).
        self.last_eval = Some(V2Eval {
            anchor: anchor_btc,
            sigma,
            health,
            q_j,
            median_diagnostic,
            obs_count: self.return_count,
        });

        Ok(Vec::new())
    }

    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}
