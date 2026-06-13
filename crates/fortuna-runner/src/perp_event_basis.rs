//! `perp_event_basis` — the propose-only, mechanical, Sim-stage perp/bracket
//! BASIS STRATEGY (docs/design/perp-strategies-and-scalar-claims.md §3, §3.1,
//! §7; GAPS "TRACK C — slice 3b").
//!
//! # What it is
//!
//! On each perp `PerpTick` this strategy compares the perp settlement mark
//! against a KXBTC bracket ladder's implied median (via the existing
//! [`fortuna_cognition::basis`] kernel — never reimplemented here) and, when
//! the signed basis clears the configured fee-trap, PROPOSES buying
//! (maker-only) the SINGLE bracket bin the perp forecast points to. It is the
//! exec-boundary money op the basis-kernel module deferred: the kernel is the
//! `f64` forecast-domain signal; this strategy turns a tradeable signal into
//! ONE UNSIZED maker leg priced in `Cents`.
//!
//! # Discipline (the house invariants this respects)
//!
//! - **I6 (propose-only / unsized).** It returns a [`Proposal`] whose single
//!   [`ProposedLeg`] carries NO quantity — sizing belongs to the harness. The
//!   strategy never sizes, never execs, never mutates external state. The leg's
//!   `fair_value` is an honest deterministic edge claim (the join limit plus a
//!   configured premium) the gates re-check; gaming it games our own risk math.
//! - **Money discipline.** `f64` appears ONLY in the forecast domain (the perp
//!   mark converted to BTC dollars at the single boundary in step 3, and the
//!   bracket probabilities the kernel consumes) — exactly as the kernel is
//!   `f64`-cognition. The only money types are `Cents` (the leg's `limit_price`
//!   / `fair_value`) and `PerpPrice` (the mark, converted once at the boundary).
//!   No `f64` ever touches a `Cents` price; no `panic!`/`unwrap`/`expect`.
//! - **Maker-only.** The leg JOINS the bin's own best YES bid (`Urgency::
//!   Passive`); it never crosses.
//!
//! # The catalog (why the strategy holds its own ladder)
//!
//! [`fortuna_venues::Market`] does not carry strike_type/floor_strike/cap_strike,
//! so the strategy CANNOT read bracket strikes from `core.markets`. Instead it
//! HOLDS ITS OWN CATALOG ([`PerpEventBasisConfig::ladder`]), injected at
//! construction: each KXBTC bracket `MarketId` mapped to its
//! [`BracketStrike`]. Where the catalog comes from at runtime (the Kalshi
//! market list at daemon startup) is the slice-4 daemon concern, OUT OF SCOPE
//! here — the strategy is catalog-driven and fixture/unit-tested.
//!
//! # The leg-selection rule (rung-0: "buy the bracket the perp points to")
//!
//! Given the perp's BTC-dollar mark, the target bin is the catalog market whose
//! strike range CONTAINS the mark (see [`PerpEventBasis::target_market`]):
//! a `between {floor,cap}` with `floor <= mark < cap`, else the open tail that
//! contains it (`greater` above the top between bin, `less` below the bottom).
//! A `between` containment always wins over a tail; if nothing contains the
//! mark, no leg is proposed.

use crate::{
    CoreHandle, Proposal, ProposedLeg, RunnerError, Stage, Strategy, StrategyKind, StrategyMetrics,
    Urgency,
};
use async_trait::async_trait;
use fortuna_cognition::basis::{compute_basis, BracketBin, BracketStrike};
use fortuna_core::book::OrderBook;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::market::{Action, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use rust_decimal::prelude::ToPrimitive;
use std::collections::{BTreeMap, HashSet};

/// The KXBTCPERP contract is BTC/10000, so a per-contract mark in dollars is
/// scaled back to a BTC-spot dollar value by ×10000 (`$6.3906 → $63,906`).
/// This is the perp-mark boundary scale the kernel doc fixes (the kernel takes
/// the BTC value; the strategy converts `PerpPrice` once at the boundary).
const PERP_CONTRACT_BTC_DIVISOR: f64 = 10_000.0;

/// Highest cent price a binary YES leg can claim as fair value: a contract is
/// never worth a full 100c before settlement, so the premium is clamped to 99
/// (mirrors `MechExtremes`).
const MAX_FAIR_CENTS: i64 = 99;

/// Construction config for [`PerpEventBasis`] (catalog-driven; see module doc).
#[derive(Debug, Clone)]
pub struct PerpEventBasisConfig {
    /// The perp whose `PerpTick` triggers the comparison (e.g. `"KXBTCPERP"`).
    /// A `PerpTick` for any other market is ignored.
    pub perp_market: MarketId,
    /// The KXBTC bracket ladder: each bracket `MarketId` → its strike(s). The
    /// strategy reads each market's book from `core.books` to build the bins;
    /// a catalog market with no/illiquid book contributes probability `0.0`.
    pub ladder: BTreeMap<MarketId, BracketStrike>,
    /// The assumed post-promo round-trip fee floor in dollars (amendment C),
    /// passed straight to [`compute_basis`] (NOT recomputed from a `FeeModel`).
    pub fee_floor_dollars: f64,
    /// The additional configured edge margin in dollars, passed to
    /// [`compute_basis`]. The basis must clear `fee_floor + min_basis`.
    pub min_basis_dollars: f64,
    /// The honest fair-value premium (cents) added to the join limit (mirrors
    /// `MechExtremes::bias_premium_cents`). The gates re-check net edge from it.
    pub edge_premium_cents: i64,
}

/// The propose-only mechanical perp/bracket basis strategy (design §3).
pub struct PerpEventBasis {
    id: StrategyId,
    cfg: PerpEventBasisConfig,
    metrics: StrategyMetrics,
    /// Dedup key: a `(target market, join limit cents)` already proposed. The
    /// identical leg is not re-proposed until the target or its limit moves.
    proposed: HashSet<(MarketId, i64)>,
}

impl PerpEventBasis {
    /// Construct the strategy. The only failure mode is an invalid strategy id
    /// (a fixed literal, so this never fires in practice — but the constructor
    /// stays fallible, no `unwrap`, per the money-path discipline).
    pub fn new(cfg: PerpEventBasisConfig) -> Result<Self, RunnerError> {
        Ok(PerpEventBasis {
            id: StrategyId::new("perp_event_basis").map_err(|e| RunnerError::Config {
                reason: e.to_string(),
            })?,
            cfg,
            metrics: StrategyMetrics::default(),
            proposed: HashSet::new(),
        })
    }

    /// The YES-mid probability of one catalog market from its book, in `[0,1]`.
    ///
    /// The conventional YES mid `((bid + ask)/2) / 100` (cents → probability),
    /// where an ABSENT quote on one side counts as the `0c` floor. So a live
    /// far-OTM bin quoted `0 bid / 2c ask` (no buyers, one resting seller — the
    /// COMMON case: 32 of the live fixture's 50 active bins) implies `ask/2`, NOT
    /// zero. This is REQUIRED for correctness: dropping every one-sided bin to
    /// `0.0` discards the ask-side mass of the whole low tail, which shifts the
    /// implied median UP and inflates the basis — it would make the strategy's
    /// basis DIVERGE from the GAPS-validated kernel number ($63,961.53 / −$55.53,
    /// the "two independent sources agree <0.1%" evidence). Treating an absent
    /// quote as `0c` exactly reproduces the kernel/fixture handling of a recorded
    /// `"0.0000"` bid, so the strategy layer and the kernel layer agree. Only a
    /// bin with NO quote on EITHER side (or no book at all) carries no implied
    /// mass → `0.0` (the kernel handles prob `0.0` safely). `f64` here is the
    /// forecast domain (a probability), never money.
    ///
    /// (The symmetric high tail — a `bid / no-ask` bin — is treated as `bid/2`
    /// by the same "absent = 0c" rule; it does not occur in the live fixture
    /// (0 zero-ask bins) and a sharper `(bid+100)/2` high-tail convention is a
    /// deferred refinement, recorded in ASSUMPTIONS.)
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

    /// Build the ladder bins from the catalog + the point-in-time books. One
    /// [`BracketBin`] per catalog market, in the catalog's (sorted by `MarketId`)
    /// order; the kernel re-sorts by price position, so order here is immaterial
    /// to the median but kept deterministic by the `BTreeMap` iteration.
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

    /// The rung-0 target market: the catalog market whose strike range CONTAINS
    /// the perp's BTC-dollar mark. Deterministic:
    ///
    /// 1. A `between {floor,cap}` with `floor <= perp_btc < cap` ALWAYS wins
    ///    (the workhorse bins are mutually exclusive, so at most one contains
    ///    the mark; ties are impossible for a well-formed partition).
    /// 2. Otherwise an open tail that contains the mark: `greater {floor}` with
    ///    `perp_btc >= floor` (the mark is above the top between bin), or
    ///    `less {cap}` with `perp_btc < cap` (the mark is below the bottom).
    /// 3. If nothing contains the mark, `None` (no leg).
    ///
    /// Iteration is over the sorted `BTreeMap`, so the result is deterministic.
    /// A pure function of `(catalog, perp_btc)` — no book/probability input.
    pub fn target_market(&self, perp_btc: f64) -> Option<&MarketId> {
        // Pass 1: a containing `between` bin wins outright.
        if let Some((mkt, _)) = self.cfg.ladder.iter().find(|(_, strike)| match strike {
            BracketStrike::Between { floor, cap } => *floor <= perp_btc && perp_btc < *cap,
            _ => false,
        }) {
            return Some(mkt);
        }
        // Pass 2: the open tail that contains the mark (only reached when no
        // `between` did — so a `greater`/`less` is selected only outside the
        // resolved between range, exactly the rung-0 rule).
        if let Some((mkt, _)) = self.cfg.ladder.iter().find(|(_, strike)| match strike {
            BracketStrike::Greater { floor } => perp_btc >= *floor,
            BracketStrike::Less { cap } => perp_btc < *cap,
            BracketStrike::Between { .. } => false,
        }) {
            return Some(mkt);
        }
        None
    }
}

#[async_trait]
impl Strategy for PerpEventBasis {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }

    /// Mechanical: deterministic, no mind, no cognition spend (design §3).
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }

    /// Sim only (design §3/§7; I7 — no auto-promotion).
    fn stage(&self) -> Stage {
        Stage::Sim
    }

    /// On a matching `PerpTick`, compute the basis and (when tradeable) propose
    /// ONE maker-only buy on the bin the perp points to. Every other path
    /// returns `Ok(vec![])`. No panic/unwrap anywhere; the only money op is the
    /// `Cents` leg pricing.
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;

        // 1. Only this strategy's perp's PerpTick triggers.
        let EventPayload::PerpTick { market, marks, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        if market != &self.cfg.perp_market {
            return Ok(Vec::new());
        }

        // 2. The ladder bins from the catalog + point-in-time books.
        let bins = self.build_bins(core);

        // 3. The perp mark → BTC-spot dollars, at the ONE price-domain boundary.
        //    `to_dollars` is exact (Decimal); the ×10000 lifts the per-contract
        //    value to the BTC value the kernel compares. On a (degenerate)
        //    Decimal→f64 failure, degrade to no proposal — never unwrap.
        let Some(per_contract) = marks.venue_settlement.to_dollars().to_f64() else {
            return Ok(Vec::new());
        };
        let perp_btc = per_contract * PERP_CONTRACT_BTC_DIVISOR;

        // 4. The basis signal. `None` median (empty/all-zero/non-finite/
        //    open-tail-crossing ladder) → no proposal; not tradeable → no
        //    proposal (the strict fee-trap `>` lives in the kernel).
        let Some(sig) = compute_basis(
            &bins,
            perp_btc,
            self.cfg.fee_floor_dollars,
            self.cfg.min_basis_dollars,
        ) else {
            return Ok(Vec::new());
        };
        if !sig.is_tradeable {
            return Ok(Vec::new());
        }

        // 5. Leg selection: the single bin the perp forecast points to.
        let Some(target) = self.target_market(perp_btc) else {
            return Ok(Vec::new());
        };
        let target = target.clone();

        // 6. Join the target's own best YES bid (maker-only); require one.
        let Some(book) = core.books.get(&target) else {
            return Ok(Vec::new());
        };
        let Some(best_bid) = book.yes_bids.first() else {
            return Ok(Vec::new());
        };
        let limit = best_bid.price;
        // Honest fair value: join limit + premium, clamped ≤ 99c. If the clamp
        // eats the whole premium there is no edge claim left → no proposal.
        let fair = (limit.raw() + self.cfg.edge_premium_cents).min(MAX_FAIR_CENTS);
        if fair <= limit.raw() {
            return Ok(Vec::new());
        }

        // 7. Dedup the identical (target, limit) leg.
        if !self.proposed.insert((target.clone(), limit.raw())) {
            return Ok(Vec::new());
        }

        // 8. Emit ONE unsized maker leg (I6 — no qty field).
        self.metrics.proposals_emitted += 1;
        Ok(vec![Proposal {
            legs: vec![ProposedLeg {
                market: target.clone(),
                side: Side::Yes,
                action: Action::Buy,
                limit_price: limit,
                fair_value: Cents::new(fair),
                calibrated_p: None,
            }],
            group_policy: None,
            urgency: Urgency::Passive,
            manifest_hash: None,
            thesis: format!(
                "perp/bracket basis: signed_basis ${:.2} (>{:.2} fee+margin) — perp mark \
                 ${:.2} vs ladder implied median ${:.2}; join YES bid {} on target bin {} \
                 (fair {}c = limit+{}c premium)",
                sig.signed_basis,
                self.cfg.fee_floor_dollars + self.cfg.min_basis_dollars,
                sig.perp_mark,
                sig.bracket_implied_median,
                limit,
                target,
                fair,
                self.cfg.edge_premium_cents,
            ),
        }])
    }

    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}
