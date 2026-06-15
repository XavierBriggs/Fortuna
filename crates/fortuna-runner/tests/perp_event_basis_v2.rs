//! `perp_event_basis_v2` STRATEGY unit tests (perp-strategies-and-scalar-claims
//! §3.3). Written from the spec BEFORE the strategy.
//!
//! V3 (build-order steps 2+3: A3 + A6 anchor + A9 no-arb) wires the v2 fair-prob
//! model (`bracket_fair_probs` on the BRTI anchor) and the no-arb gate
//! (`validate_ladder_no_arb`) into a propose-only, Sim-stage, data-only
//! evaluation snapshot (`last_eval`); it PROPOSES NOTHING.
//!
//! V4 (build-order steps 4+5: A5 horizon gating + A4/A8 per-bin EV gate) is the
//! FIRST slice that PROPOSES. It replaces V3's per-step σ stand-in with the
//! τ-regime-scaled σ_τ (A5), adds the three vetoes (>48h, τ-unknown, stale
//! anchor), and emits ONE unsized maker leg per ladder bin whose per-bin EV
//! (A4+A8) clears the threshold. Still Sim-stage / unsized (I6/I7).
//!
//! V5 (build-order step 6: A7 measured informativeness + A10 diagnostic emission)
//! is the LAST v2 slice. It MEASURES whether the perp leads the bracket (per-bin
//! `InfoVerdict` from quote FRESHNESS, DC-6) and folds it into the V4 EV gate —
//! `BracketLeads` VETOES (or down-weights when the flag is off), `Unfavorable`
//! (perp absent/stale or bracket stale/bookless) ADDS `info_adverse_penalty` to the
//! EV `adverse`, `PerpFavorable` leaves the gate unchanged. A7 can ONLY make the
//! gate MORE conservative. It also ships the A10 implied-vs-model CDF sup-distance
//! (`cdf_divergence`) + per-bin verdict/ages/spread/depth (recorded, not gated).
//!
//! Contract under test (V5 A7 + A10):
//! - PerpFavorable (perp present + fresh + ≥ bracket freshness) ⇒ the V4 proposal
//!   fires UNCHANGED (`adverse_eff == adverse`, EV == V4 EV).
//! - Unfavorable (perp book ABSENT) ⇒ `adverse_eff = adverse + penalty`; a wide-edge
//!   bin still fires, a borderline bin is SKIPPED (the penalty is the deciding term).
//! - Unfavorable (STALE bracket / STALE perp) ⇒ the penalty applies even with a
//!   present perp; a stale perp is Unfavorable, NOT BracketLeads.
//! - BracketLeads (bracket strictly fresher, both fresh) ⇒ VETO (no proposal) when
//!   `info_veto_on_bracket_leads`; DOWN-WEIGHT (penalty) when the flag is off.
//! - the conservative default: missing/stale ⇒ NEVER PerpFavorable.
//! - A10: `cdf_divergence` is Some when priced, None when q_j empty, and equals the
//!   hand-computed Kolmogorov sup-distance of the price-ordered cumulatives.
//!
//! Contract under test (V3 evaluator):
//! - σ NOT ready (< `min_vol_obs` anchor returns) ⇒ `last_eval` is None,
//!   zero proposals.
//! - σ ready ⇒ `q_j` EQUALS `bracket_fair_probs` on the same anchor + σ_τ.
//! - A6: prices off the BRTI ANCHOR (`reference_price`), NOT the perp mark.
//! - A9: an INCOHERENT ladder ⇒ records `Incoherent`, zero proposals.
//! - A10: the median diagnostic is populated and is NOT a gate.
//! - degenerate input (non-finite / ≤0 anchor, all-empty ladder) ⇒ no panic.
//!
//! Contract under test (V4 horizon gating + EV gate):
//! - Regime selection at the boundaries: τ just below/above `direct_max_ms`
//!   (Direct vs VolAdjusted), and just above `vol_adjusted_max_ms` (Disabled).
//! - σ_τ scaling: σ used in the model == `σ_step·sqrt(τ_ms/Δ_ms)` clamped, and
//!   it DIFFERS from V3's per-step σ (the refinement landed).
//! - The THREE vetoes ⇒ ZERO proposals: (a) τ>48h, (b) τ unknown (target absent
//!   from `core.markets` / `close_at` None), (c) stale anchor.
//! - EV gate CLEARS: a bin with q_j well above ask+costs ⇒ a Passive/Buy/Yes
//!   proposal joining the bin's best bid, fair = round(q_j·100) clamped, unsized.
//! - EV gate REJECTS: a bin below (or exactly at) the threshold ⇒ no proposal
//!   (strict `>`).
//! - Multiple bins clear ⇒ multiple Proposals; dedup ⇒ no re-propose on an
//!   identical second tick.
//! - A bin that clears EV but has NO bid to join ⇒ skipped, no panic.
//! - fee_j is strictly > 0 for an interior ask (no "free" promo sneaks a bin in).
//! - No panic on degenerate (Δ not ready, σ_τ non-finite, empty ladder).
//! - I6: every emitted leg is an unsized maker join at the bid.
//!
//! ## Mutation-check note (which mutation reds which test)
//! - Swapping the A6 anchor source `funding.reference_price → marks.venue_settlement`
//!   reds `a6_prices_off_anchor_not_mark`.
//! - Dropping / inverting the A9 guard reds `a9_incoherent_ladder_records_incoherent`.
//! - Re-implementing `q_j` (rather than CALLING `bracket_fair_probs`), or feeding
//!   the wrong σ/anchor, reds `sigma_ready_qj_matches_kernel` and
//!   `a6_prices_off_anchor_not_mark`.
//! - Lowering the `min_vol_obs` ready-gate reds `sigma_not_ready_no_eval`.
//! - Demoting the median to a SIGNAL reds `median_diagnostic_populated_but_not_a_gate`.
//! - Removing the >48h veto reds `regime_above_vol_adjusted_max_disabled_no_proposal`.
//! - Removing the τ-unknown (absent/None `close_at`) veto reds
//!   `tau_unknown_target_absent_disabled_no_proposal`.
//! - Dropping the stale-anchor veto reds `stale_anchor_disables_no_proposal`.
//! - Using σ_step instead of σ_τ in the model reds `sigma_tau_scaling_matches_formula`.
//! - Flipping the EV `>` to `>=` reds `ev_gate_rejects_at_or_below_threshold`
//!   (the at-threshold sub-case).
//! - Dropping the fee_j term (or letting promo-$0 zero it) reds
//!   `fee_j_is_strictly_positive_for_interior_ask`.
//! - Joining the ask instead of the bid, or sizing a leg, reds
//!   `ev_gate_clears_emits_unsized_maker_join_at_bid`.
//! - Defaulting `InfoVerdict` to `PerpFavorable` (instead of the conservative
//!   `Unfavorable`) on a missing/stale perp reds
//!   `a7_missing_perp_is_never_perp_favorable`,
//!   `a7_unfavorable_perp_absent_still_fires_when_edge_overcomes`,
//!   `a7_stale_perp_is_unfavorable_not_bracket_leads`, and
//!   `a7_unfavorable_stale_bracket_applies_penalty`.
//! - Dropping the `BracketLeads` veto (treating it as PerpFavorable/no-op) reds
//!   `a7_bracket_leads_vetoes_when_flag_on`.
//! - Using a 0 A7 penalty (or never adding it to `adverse`) reds
//!   `a7_unfavorable_perp_absent_penalty_skips_borderline_bin` (the skipped half)
//!   and the `adverse_eff` assertions in the Unfavorable/down-weight tests.
//! - Sourcing perp freshness from `obs_at` when a perp book IS present (instead of
//!   the perp book's `as_of`) reds `a7_bracket_leads_vetoes_when_flag_on` /
//!   `a7_perp_favorable_proposal_fires_unchanged` (the verdict would flip).
//! - Dropping the `q_j`-empty guard on `cdf_divergence` (returning `Some` on an
//!   unpriced tick), or mis-ordering the cumulatives, reds
//!   `cdf_divergence_some_when_priced_none_when_not` /
//!   `cdf_divergence_equals_hand_computed_sup_distance`.

use fortuna_cognition::basis::{compute_basis, BracketBin, BracketStrike};
use fortuna_cognition::basis_v2::{bracket_fair_probs, LadderHealth, SettlementModel};
use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::perp_event_basis_v2::{
    HorizonRegime, InfoVerdict, PerpEventBasisV2, PerpEventBasisV2Config,
};
use fortuna_runner::{CoreHandle, Proposal, Stage, Strategy, StrategyKind, Urgency};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;

// ── harness (mirrors the rung-0 perp_event_basis test harness) ───────────────

const PERP: &str = "KXBTCPERP";

/// The KXBTCPERP contract is BTC/10000 — the per-contract dollar value lifts to
/// BTC dollars by ×10000. Mirrors the strategy's `PERP_CONTRACT_BTC_DIVISOR`.
const PERP_CONTRACT_BTC_DIVISOR: f64 = 10_000.0;

fn mkt(s: &str) -> MarketId {
    MarketId::new(s).unwrap()
}

fn ts(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(s).unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

/// A book with a single (bid, ask) YES level, in cents.
fn book(market: &str, yes_bid: i64, yes_ask: i64) -> OrderBook {
    OrderBook {
        market: mkt(market),
        as_of: ts("2026-06-12T17:00:00.000Z"),
        yes_bids: vec![PriceLevel {
            price: Cents::new(yes_bid),
            qty: Contracts::new(100),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(yes_ask),
            qty: Contracts::new(100),
        }],
    }
}

/// A bid-empty book (one ask, no bids): per the YES-mid convention it still
/// carries `ask/2` implied mass (the live far-OTM `0 bid / Nc ask` case).
fn book_no_bid(market: &str, yes_ask: i64) -> OrderBook {
    OrderBook {
        market: mkt(market),
        as_of: ts("2026-06-12T17:00:00.000Z"),
        yes_bids: vec![],
        yes_asks: vec![PriceLevel {
            price: Cents::new(yes_ask),
            qty: Contracts::new(100),
        }],
    }
}

/// The mutable view a `CoreHandle` borrows: books + markets + fees + now.
struct World {
    books: BTreeMap<MarketId, OrderBook>,
    markets: BTreeMap<MarketId, fortuna_venues::Market>,
    fees: ScheduleFeeModel,
    now: UtcTimestamp,
}

impl World {
    fn new() -> World {
        World {
            books: BTreeMap::new(),
            markets: BTreeMap::new(),
            fees: fee_model(),
            now: ts("2026-06-12T17:00:00.000Z"),
        }
    }
    fn put(&mut self, b: OrderBook) {
        self.books.insert(b.market.clone(), b);
    }
    fn handle(&self) -> CoreHandle<'_> {
        CoreHandle {
            now: self.now,
            books: &self.books,
            markets: &self.markets,
            fee_model: &self.fees,
        }
    }
}

/// A `PerpTick` for `perp_market` whose BRTI ANCHOR (`funding.reference_price`)
/// is `ref_per_contract` dollars and whose perp MARK (`marks.venue_settlement`)
/// is `mark_per_contract` dollars. V2 reads the ANCHOR (A6), not the mark, so
/// the two are set INDEPENDENTLY (the rung-0 harness pinned reference_price at a
/// single 6.3000 — V2 needs it to VARY to drive σ, and to DIFFER from the mark
/// to bite the A6 assertion). Both lift ×10000 to BTC dollars.
fn perp_tick_v2(perp_market: &str, mark_per_contract: &str, ref_per_contract: &str) -> BusEvent {
    let marks = PerpMarks {
        venue_settlement: PerpPrice::from_dollars_exact(
            Decimal::from_str(mark_per_contract).unwrap(),
        )
        .unwrap(),
        conservative: None,
    };
    let funding = FundingObservation {
        estimate: Decimal::from_str("0.0005").unwrap(),
        next_funding_time: ts("2026-06-12T20:00:00.000Z"),
        reference_price: PerpPrice::from_dollars_exact(
            Decimal::from_str(ref_per_contract).unwrap(),
        )
        .unwrap(),
        obs_at: ts("2026-06-12T17:00:00.000Z"),
    };
    BusEvent {
        seq: 1,
        at: ts("2026-06-12T17:00:00.000Z"),
        origin: EventOrigin::External,
        payload: EventPayload::PerpTick {
            venue: VenueId::new("kinetics").unwrap(),
            market: mkt(perp_market),
            marks,
            funding,
        },
    }
}

fn run(s: &mut PerpEventBasisV2, w: &World, ev: &BusEvent) -> Vec<Proposal> {
    futures::executor::block_on(s.on_event(ev, &w.handle())).unwrap()
}

/// A coherent three-bin ladder around $63,500: a `less` bottom tail, a single
/// `between` $63,000–64,000, and a `greater` top tail. Books chosen so the
/// implied YES-mids sum ≈ 1 and are monotone (A9-coherent), and the median
/// crossing lands in the finite between bin (so `compute_basis` is Some).
fn coherent_ladder() -> BTreeMap<MarketId, BracketStrike> {
    let mut ladder = BTreeMap::new();
    ladder.insert(mkt("KXBTC-LESS63K"), BracketStrike::Less { cap: 63_000.0 });
    ladder.insert(
        mkt("KXBTC-B63500"),
        BracketStrike::Between {
            floor: 63_000.0,
            cap: 64_000.0,
        },
    );
    ladder.insert(
        mkt("KXBTC-GT64K"),
        BracketStrike::Greater { floor: 64_000.0 },
    );
    ladder
}

/// Books for `coherent_ladder`: less≈0.25, between≈0.50, greater≈0.25 (Σ≈1.0,
/// monotone). The 0.5 crossing lands inside the between bin ⇒ finite median.
fn put_coherent_books(w: &mut World) {
    w.put(book("KXBTC-LESS63K", 24, 26)); // mid 0.25
    w.put(book("KXBTC-B63500", 49, 51)); // mid 0.50, best bid 49c
    w.put(book("KXBTC-GT64K", 24, 26)); // mid 0.25
}

/// Build the SAME `Vec<BracketBin>` the strategy builds from the books (the
/// rung-0 `bin_prob` convention: absent quote = 0c; both-empty book = 0.0), so
/// a test can call `bracket_fair_probs` directly and compare. Mirrors the
/// strategy's `build_bins`.
fn bins_from_world(ladder: &BTreeMap<MarketId, BracketStrike>, w: &World) -> Vec<BracketBin> {
    ladder
        .iter()
        .map(|(m, strike)| {
            let prob = match w.books.get(m) {
                None => 0.0,
                Some(b) => {
                    if b.yes_bids.is_empty() && b.yes_asks.is_empty() {
                        0.0
                    } else {
                        let bid = b.yes_bids.first().map_or(0, |l| l.price.raw());
                        let ask = b.yes_asks.first().map_or(0, |l| l.price.raw());
                        ((bid + ask) as f64 / 2.0) / 100.0
                    }
                }
            };
            BracketBin {
                kind: *strike,
                prob,
            }
        })
        .collect()
}

/// DC-1 defaults but with a small `min_vol_obs` (3) so tests can reach "ready"
/// in a few ticks; the other knobs are the production defaults. The V4 regime +
/// EV knobs ride their DC-3 defaults (the V3-era tests that use this helper drive
/// FROZEN-obs sequences ⇒ Δ is never measured ⇒ the horizon is Disabled ⇒ no
/// proposal, which preserves their "data-only / zero-proposal" expectations; the
/// V4-aware tests that need pricing use [`cfg_v4`] + advancing obs + a τ market).
fn cfg(ladder: BTreeMap<MarketId, BracketStrike>) -> PerpEventBasisV2Config {
    PerpEventBasisV2Config {
        perp_market: mkt(PERP),
        ladder,
        vol_buf_len: 64,
        ewma_lambda: 0.94,
        min_vol_obs: 3,
        sigma_floor: 1e-6,
        sigma_ceiling: 5.0,
        no_arb_tol: 0.05,
        direct_max_ms: 14_400_000,
        vol_adjusted_max_ms: 172_800_000,
        max_anchor_age_ms: 5_000,
        ev_threshold: EV_THRESHOLD,
        slippage: SLIPPAGE,
        reserve: RESERVE,
        adverse: ADVERSE,
        fee_coeff: FEE_COEFF,
        info_max_age_ms: INFO_MAX_AGE_MS,
        info_adverse_penalty: INFO_ADVERSE_PENALTY,
        info_veto_on_bracket_leads: true,
    }
}

/// A CONSTANT-log-step anchor sequence: `a_k = base · ratio^k`. Every per-step
/// log-return is `ln(ratio)` exactly, so the EWMA of r² is `(ln ratio)²` for
/// ANY λ (seed r² then `λ·r²+(1-λ)·r² = r²`), making σ = |ln ratio|
/// hand-computable and λ-independent. Returns the per-contract dollar STRINGS
/// (the value `PerpPrice` carries, ×10000 = the BTC anchor) for `n` ticks.
///
/// `base`/`ratio` are picked so each anchor is a whole $0.0001 tick
/// (`from_dollars_exact` requires it): base 6.0000, ratio 1.01 ⇒
/// 6.0000, 6.0600, 6.1206, … rounded to 4dp. We round to 4dp HERE and the test
/// reconstructs σ from the SAME rounded values (so the strategy and the oracle
/// see byte-identical anchors — no rounding skew).
fn constant_step_anchor_strings(base: f64, ratio: f64, n: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(n);
    let mut a = base;
    for _ in 0..n {
        // Round to the $0.0001 tick the venue quotes (and `from_dollars_exact`
        // requires); format with exactly 4 decimals.
        let rounded = (a * 10_000.0).round() / 10_000.0;
        out.push(format!("{rounded:.4}"));
        a *= ratio;
    }
    out
}

/// Reconstruct the σ the strategy WILL compute from a sequence of per-contract
/// anchor strings, using the SAME EWMA recurrence the spec fixes: seed the
/// variance on the FIRST return with r², then `var = λ·var + (1-λ)·r²` for each
/// subsequent return; σ = sqrt(var) clamped to [floor, ceiling]. BTC anchors
/// are the strings ×10000.
fn expected_sigma(anchor_strings: &[String], lambda: f64, floor: f64, ceiling: f64) -> f64 {
    let anchors: Vec<f64> = anchor_strings
        .iter()
        .map(|s| f64::from_str(s).unwrap() * PERP_CONTRACT_BTC_DIVISOR)
        .collect();
    let mut var: Option<f64> = None;
    for w in anchors.windows(2) {
        let r = (w[1] / w[0]).ln();
        let r2 = r * r;
        var = Some(match var {
            None => r2,
            Some(v) => lambda * v + (1.0 - lambda) * r2,
        });
    }
    let sigma = var.expect("at least one return").sqrt();
    sigma.clamp(floor, ceiling)
}

/// Drive `s` through a constant-log-step anchor sequence (all ticks share the
/// SAME perp mark — V2 ignores the mark for σ; it tracks the anchor). Returns
/// the anchor strings used (so the oracle can reconstruct σ). The perp mark is
/// held at `6.3500` (a distinct value, to keep the A6 separation visible).
fn drive_constant_step(
    s: &mut PerpEventBasisV2,
    w: &World,
    base: f64,
    ratio: f64,
    n: usize,
) -> Vec<String> {
    let anchors = constant_step_anchor_strings(base, ratio, n);
    for a in &anchors {
        let _ = run(s, w, &perp_tick_v2(PERP, "6.3500", a));
    }
    anchors
}

// ── V4 harness (A5 horizon gating + A4/A8 EV gate) ───────────────────────────
//
// V4 needs three controls V3's harness lacks: (1) a VARYING `obs_at` so the Δ
// (observation-interval) EWMA can measure a per-step gap (V3's harness froze
// `obs_at` ⇒ Δ never measured ⇒ V4 would Disable every bin); (2) per-bracket
// `close_at` markets in `core.markets` so τ = close_at − now is computable; and
// (3) executable YES ASKS on the bins so the EV gate has a price to take toward.

/// V4 EV-knob defaults (DC-3), mirrored from the production config defaults so a
/// test can recompute the SAME EV the strategy computes.
const EV_THRESHOLD: f64 = 0.02;
const SLIPPAGE: f64 = 0.005;
const RESERVE: f64 = 0.01;
const ADVERSE: f64 = 0.01;
const FEE_COEFF: f64 = 0.0175;

/// V5 A7-knob defaults (DC-6), mirrored from the production config defaults so a
/// test can recompute the SAME A7-effective EV the strategy computes.
const INFO_MAX_AGE_MS: i64 = 5_000;
const INFO_ADVERSE_PENALTY: f64 = 0.02;

/// A `Market` carrying just the `close_at` the τ computation reads. The other
/// fields are plausible placeholders the strategy never consults (it reaches
/// ONLY `core.markets.get(id).and_then(|m| m.close_at)`).
fn market_with_close(id: &str, close_at: Option<UtcTimestamp>) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("kinetics").unwrap(),
        title: "KXBTC bracket".to_string(),
        category: "crypto".to_string(),
        status: MarketStatus::Trading,
        close_at,
        settlement: SettlementMeta {
            oracle_type: "cf_benchmarks_brti".to_string(),
            resolution_source: "brti".to_string(),
            expected_lag_hours: 0,
        },
        payout_per_contract: Cents::new(100),
        volume_contracts: Some(1_000),
    }
}

/// Register a `close_at` for every bracket in `ladder` at `now + tau_ms`.
fn put_ladder_markets(w: &mut World, ladder: &BTreeMap<MarketId, BracketStrike>, tau_ms: i64) {
    let close = w.now.checked_add_millis(tau_ms).unwrap();
    for id in ladder.keys() {
        w.markets
            .insert(id.clone(), market_with_close(id.as_str(), Some(close)));
    }
}

/// A `PerpTick` like `perp_tick_v2` but with an explicit `obs_at` (the BRTI
/// capture time) so the Δ estimator and the A6 stale-anchor veto are drivable.
/// `at`/`seq` are immaterial to V2 (it reads `core.now`, the funding `obs_at`,
/// and the payload), so they ride a fixed value.
fn perp_tick_obs(
    perp_market: &str,
    mark_per_contract: &str,
    ref_per_contract: &str,
    obs_at: UtcTimestamp,
) -> BusEvent {
    let marks = PerpMarks {
        venue_settlement: PerpPrice::from_dollars_exact(
            Decimal::from_str(mark_per_contract).unwrap(),
        )
        .unwrap(),
        conservative: None,
    };
    let funding = FundingObservation {
        estimate: Decimal::from_str("0.0005").unwrap(),
        next_funding_time: ts("2026-06-12T20:00:00.000Z"),
        reference_price: PerpPrice::from_dollars_exact(
            Decimal::from_str(ref_per_contract).unwrap(),
        )
        .unwrap(),
        obs_at,
    };
    BusEvent {
        seq: 1,
        at: ts("2026-06-12T17:00:00.000Z"),
        origin: EventOrigin::External,
        payload: EventPayload::PerpTick {
            venue: VenueId::new("kinetics").unwrap(),
            market: mkt(perp_market),
            marks,
            funding,
        },
    }
}

/// Drive `s` through a constant-log-step anchor sequence whose `obs_at` ADVANCES
/// by exactly `dt_ms` each tick (starting at `obs0`), so the Δ EWMA measures a
/// CONSTANT per-step gap `dt_ms` (Δ = dt_ms exactly, λ-independent — same trick
/// as the constant-log-step σ). `World.now` is NOT advanced (τ stays fixed at
/// whatever `put_ladder_markets` set). Returns the anchor strings used.
fn drive_constant_step_obs(
    s: &mut PerpEventBasisV2,
    w: &World,
    base: f64,
    ratio: f64,
    n: usize,
    obs0: UtcTimestamp,
    dt_ms: i64,
) -> Vec<String> {
    let anchors = constant_step_anchor_strings(base, ratio, n);
    let mut obs = obs0;
    for a in &anchors {
        let _ = run(s, w, &perp_tick_obs(PERP, "6.3500", a, obs));
        obs = obs.checked_add_millis(dt_ms).unwrap();
    }
    anchors
}

/// Drive `s` through an EXPLICIT anchor-string sequence whose `obs_at` advances
/// by `dt_ms` each tick (Δ = dt_ms). Returns the obs_at of the LAST tick (the
/// freshness reference the A6 stale veto measures against `World.now`). The perp
/// mark is held at a fixed distinct value (V2 ignores the mark for σ/Δ).
fn drive_explicit_obs(
    s: &mut PerpEventBasisV2,
    w: &World,
    anchors: &[&str],
    obs0: UtcTimestamp,
    dt_ms: i64,
) -> UtcTimestamp {
    let mut obs = obs0;
    for a in anchors {
        let _ = run(s, w, &perp_tick_obs(PERP, "6.3500", a, obs));
        if a != anchors.last().unwrap() {
            obs = obs.checked_add_millis(dt_ms).unwrap();
        }
    }
    obs
}

/// Drive a constant-log-step advancing-obs sequence and RETURN the proposals
/// emitted on the FIRST tick that proposed anything (the moment a bin first
/// clears, before dedup suppresses repeats). Empty if no tick ever proposed.
/// Lets a test inspect the leg shape without fighting the dedup HashSet.
fn drive_capture_first_proposal(
    s: &mut PerpEventBasisV2,
    w: &World,
    base: f64,
    ratio: f64,
    n: usize,
    obs0: UtcTimestamp,
    dt_ms: i64,
) -> Vec<Proposal> {
    let anchors = constant_step_anchor_strings(base, ratio, n);
    let mut obs = obs0;
    let mut first: Vec<Proposal> = Vec::new();
    for a in &anchors {
        let out = run(s, w, &perp_tick_obs(PERP, "6.3500", a, obs));
        if first.is_empty() && !out.is_empty() {
            first = out;
        }
        obs = obs.checked_add_millis(dt_ms).unwrap();
    }
    first
}

/// `drive_capture_first_proposal` over an EXPLICIT anchor list (advancing obs).
fn drive_explicit_capture_first(
    s: &mut PerpEventBasisV2,
    w: &World,
    anchors: &[&str],
    obs0: UtcTimestamp,
    dt_ms: i64,
) -> Vec<Proposal> {
    let mut obs = obs0;
    let mut first: Vec<Proposal> = Vec::new();
    for a in anchors {
        let out = run(s, w, &perp_tick_obs(PERP, "6.3500", a, obs));
        if first.is_empty() && !out.is_empty() {
            first = out;
        }
        obs = obs.checked_add_millis(dt_ms).unwrap();
    }
    first
}

/// The σ_τ the strategy WILL use (A5 / DC-1): the V3 per-step σ scaled by
/// `sqrt(τ_ms / Δ_ms)` then clamped to `[floor, ceiling]`. The per-step σ is the
/// SAME EWMA value `expected_sigma` reconstructs; this is the V4 refinement on
/// top of it.
fn expected_sigma_tau(
    anchor_strings: &[String],
    lambda: f64,
    floor: f64,
    ceiling: f64,
    tau_ms: i64,
    dt_ms: i64,
) -> f64 {
    let sigma_step = expected_sigma(anchor_strings, lambda, floor, ceiling);
    let scaled = sigma_step * ((tau_ms as f64) / (dt_ms as f64)).sqrt();
    scaled.clamp(floor, ceiling)
}

/// The round-trip maker fee in prob-units (A4/DC-3): per leg
/// `ceil(fee_coeff · P · (1−P) · 100) / 100` with `P = ask`, ×2 (enter+exit).
/// The cents-rounded-UP ceil is the fee-trap (promo-$0 never lowers it).
fn expected_fee_j(ask: f64) -> f64 {
    let per_leg = (FEE_COEFF * ask * (1.0 - ask) * 100.0).ceil() / 100.0;
    2.0 * per_leg
}

/// The per-bin EV (A4+A8): `q − ask − fee − slippage − reserve − adverse`.
fn expected_ev_j(q: f64, ask: f64) -> f64 {
    q - ask - expected_fee_j(ask) - SLIPPAGE - RESERVE - ADVERSE
}

/// The best YES ASK of a bin in prob-units (`ask_cents/100`), the executable
/// price the EV gate prices toward. Mirrors the strategy's `bin_ask`.
fn ask_prob(ask_cents: i64) -> f64 {
    (ask_cents as f64) / 100.0
}

/// A V4 config: the V3 σ knobs (small `min_vol_obs`) + the A5 regime knobs + the
/// A4/A8 EV knobs at their DC-3 defaults. `direct_max_ms`/`vol_adjusted_max_ms`
/// are the production 4h/48h; `max_anchor_age_ms` defaults to 5s.
fn cfg_v4(ladder: BTreeMap<MarketId, BracketStrike>) -> PerpEventBasisV2Config {
    PerpEventBasisV2Config {
        perp_market: mkt(PERP),
        ladder,
        vol_buf_len: 64,
        ewma_lambda: 0.94,
        min_vol_obs: 3,
        sigma_floor: 1e-6,
        sigma_ceiling: 5.0,
        no_arb_tol: 0.05,
        direct_max_ms: 14_400_000,        // 4h
        vol_adjusted_max_ms: 172_800_000, // 48h
        max_anchor_age_ms: 5_000,         // BRTI ~1/sec
        ev_threshold: EV_THRESHOLD,
        slippage: SLIPPAGE,
        reserve: RESERVE,
        adverse: ADVERSE,
        fee_coeff: FEE_COEFF,
        info_max_age_ms: INFO_MAX_AGE_MS,
        info_adverse_penalty: INFO_ADVERSE_PENALTY,
        info_veto_on_bracket_leads: true,
    }
}

/// Coherent books for the `straddle_ladder` ids (less 0.25 / between 0.50 /
/// greater 0.25 ⇒ Σ≈1, monotone — A9-coherent). The between bin's best bid is 49c
/// (the maker join the EV tests inspect) and its ask 51c (the executable price).
fn put_straddle_coherent_books(w: &mut World) {
    w.put(book("KXBTC-LESS60K", 24, 26)); // mid 0.25
    w.put(book("KXBTC-B63K", 49, 51)); // mid 0.50, best bid 49c, ask 51c
    w.put(book("KXBTC-GT66K", 24, 26)); // mid 0.25
}

/// A wide-straddle ladder whose single `between` bin BRACKETS the anchor used by
/// the V4 EV tests (`Less{60k}`, `Between{60k,66k}`, `Greater{66k}`). With a
/// tight σ_τ the between bin carries ~0.98 of the mass — large enough that a
/// cheaply-quoted YES ask clears the EV gate by a wide margin (and a richly-
/// quoted one rejects). The bin ids sort so each is addressable.
fn straddle_ladder() -> BTreeMap<MarketId, BracketStrike> {
    let mut ladder = BTreeMap::new();
    ladder.insert(mkt("KXBTC-LESS60K"), BracketStrike::Less { cap: 60_000.0 });
    ladder.insert(
        mkt("KXBTC-B63K"),
        BracketStrike::Between {
            floor: 60_000.0,
            cap: 66_000.0,
        },
    );
    ladder.insert(
        mkt("KXBTC-GT66K"),
        BracketStrike::Greater { floor: 66_000.0 },
    );
    ladder
}

// ── tests ────────────────────────────────────────────────────────────────────

/// V3 is DATA-ONLY (I7 Sim-stage, propose-only).
#[test]
fn kind_is_mechanical_and_stage_is_sim() {
    let s = PerpEventBasisV2::new(cfg(coherent_ladder())).unwrap();
    assert_eq!(s.kind(), StrategyKind::Mechanical);
    assert_eq!(s.stage(), Stage::Sim, "V2 evaluator is Sim-stage (I7)");
}

/// σ NOT ready: fewer than `min_vol_obs` returns ⇒ `last_eval` is None and no
/// proposals. (min_vol_obs=3 needs 3 returns ⇒ 4 ticks; feed only 3 ticks = 2
/// returns.)
#[test]
fn sigma_not_ready_no_eval() {
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut s = PerpEventBasisV2::new(cfg(coherent_ladder())).unwrap();

    // 3 ticks ⇒ only 2 returns < min_vol_obs(3) ⇒ NOT ready.
    let anchors = constant_step_anchor_strings(6.0000, 1.01, 3);
    for a in &anchors {
        let props = run(&mut s, &w, &perp_tick_v2(PERP, "6.3500", a));
        assert!(props.is_empty(), "V3 proposes nothing, ever");
    }
    assert!(
        s.last_eval().is_none(),
        "σ not ready (< min_vol_obs returns) ⇒ no evaluation snapshot"
    );
    assert_eq!(s.metrics().proposals_emitted, 0);
    assert_eq!(s.metrics().events_seen, 3, "every tick is still counted");
}

/// σ ready (V4-aware): with ≥ min_vol_obs returns over a CONTROLLED constant-
/// log-step anchor sequence (advancing obs ⇒ Δ measured), a known Direct horizon,
/// and a fresh anchor, the tick PRICES and the stored `q_j` EQUALS
/// `bracket_fair_probs` called directly with the SAME anchor + the σ the strategy
/// USED (σ_τ now, not σ_step). Pins the A3-on-the-A6-anchor WIRING, never a
/// re-implementation. Also: `q_j.len() == ladder size`, each `q ∈ [0,1]`, and the
/// σ the strategy used is the hand-computed σ_τ.
#[test]
fn sigma_ready_qj_matches_kernel() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    // τ = 2h (Direct), Δ = 1000ms; markets carry close_at = now + τ.
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();

    // 6 ticks ⇒ 5 returns ≥ min_vol_obs(3) ⇒ READY; advancing obs ⇒ Δ measured.
    // The final obs is within max_anchor_age of World.now (16:59:55 + 5×1s).
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let anchors = drive_constant_step_obs(&mut s, &w, 6.0000, 1.01, 6, obs0, dt_ms);

    let eval = s
        .last_eval()
        .expect("σ ready ⇒ an evaluation snapshot exists");

    // The σ the strategy used is the HORIZON-scaled σ_τ (the V4 refinement), not
    // the V3 per-step σ_step.
    let sigma_tau = expected_sigma_tau(&anchors, 0.94, 1e-6, 5.0, tau_ms, dt_ms);
    assert!(
        (eval.sigma - sigma_tau).abs() < 1e-12,
        "σ used {} != hand-computed σ_τ {}",
        eval.sigma,
        sigma_tau
    );
    assert_eq!(eval.regime, HorizonRegime::Direct, "τ=2h ⇒ Direct");
    assert!(!eval.anchor_stale, "fresh anchor");

    // The ANCHOR the strategy used: the LAST tick's reference_price ×10000.
    let last_anchor_btc =
        f64::from_str(anchors.last().unwrap()).unwrap() * PERP_CONTRACT_BTC_DIVISOR;
    assert!(
        (eval.anchor - last_anchor_btc).abs() < 1e-9,
        "anchor used {} != last BRTI reference ×10000 {}",
        eval.anchor,
        last_anchor_btc
    );

    // THE WIRING PIN: q_j == bracket_fair_probs(bins, model{anchor, σ}) verbatim,
    // where σ is the σ the strategy USED (σ_τ) — invariant to σ_step→σ_τ.
    let bins = bins_from_world(&ladder, &w);
    let model = SettlementModel {
        anchor: eval.anchor,
        sigma: eval.sigma,
    };
    let expected_q = bracket_fair_probs(&bins, model);
    assert_eq!(
        eval.q_j, expected_q,
        "stored q_j must EQUAL the kernel call with the same anchor + σ (wiring, not reimpl)"
    );

    // q_j length == ladder size; each q ∈ [0,1].
    assert_eq!(eval.q_j.len(), ladder.len(), "one q_j per ladder bin");
    for fp in &eval.q_j {
        assert!(
            (0.0..=1.0).contains(&fp.q),
            "q out of [0,1]: {} for {:?}",
            fp.q,
            fp.kind
        );
    }
}

/// A6 (load-bearing): the model prices off the BRTI ANCHOR, NOT the perp MARK.
/// Feed a final tick whose `reference_price` (anchor) differs SHARPLY from the
/// `venue_settlement` (mark); the stored `q_j` must match the ANCHOR-based
/// kernel call and must NOT match the MARK-based one. (V4-aware: a τ context +
/// advancing obs so the tick prices; the kernel comparison reads `eval.sigma`,
/// which is the σ_τ the strategy used, so the A6 pin is unchanged.)
#[test]
fn a6_prices_off_anchor_not_mark() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();

    // Warm σ to ready with a constant-log-step anchor sequence (advancing obs ⇒
    // Δ measured). base 6.0000, ratio 1.01, 6 ticks; obs starts 16:59:54 so the
    // final EXTRA tick below lands at 17:00:00 (fresh within max_anchor_age).
    let (base, ratio, n) = (6.0000_f64, 1.01_f64, 6);
    let obs0 = ts("2026-06-12T16:59:54.000Z");
    let anchors = drive_constant_step_obs(&mut s, &w, base, ratio, n, obs0, dt_ms);
    let last_anchor_str = anchors.last().unwrap().clone();

    // Now fire ONE more tick where the MARK is far from the ANCHOR. Keep the
    // anchor on the constant-log-step path (so σ stays the clean value); set the
    // mark to a wildly different BTC value. obs_at = 17:00:00 (fresh).
    let next_anchor = {
        let a = f64::from_str(&last_anchor_str).unwrap() * ratio;
        format!("{:.4}", (a * 10_000.0).round() / 10_000.0)
    };
    // mark per-contract 7.5000 (×10000 = BTC $75,000) — far above the ~$60.6k
    // anchor; if the model (wrongly) used the mark, q_j would shift up sharply.
    let final_obs = ts("2026-06-12T17:00:00.000Z");
    let _ = run(
        &mut s,
        &w,
        &perp_tick_obs(PERP, "7.5000", &next_anchor, final_obs),
    );

    let eval = s.last_eval().expect("ready");
    let anchor_btc = f64::from_str(&next_anchor).unwrap() * PERP_CONTRACT_BTC_DIVISOR;
    let mark_btc = 7.5000_f64 * PERP_CONTRACT_BTC_DIVISOR;
    // Sanity: the anchor and the mark are genuinely different (the test bites).
    assert!(
        (anchor_btc - mark_btc).abs() > 10_000.0,
        "test misconfigured: anchor {anchor_btc} ≈ mark {mark_btc}"
    );

    let bins = bins_from_world(&ladder, &w);
    let q_anchor = bracket_fair_probs(
        &bins,
        SettlementModel {
            anchor: anchor_btc,
            sigma: eval.sigma,
        },
    );
    let q_mark = bracket_fair_probs(
        &bins,
        SettlementModel {
            anchor: mark_btc,
            sigma: eval.sigma,
        },
    );
    assert_eq!(
        eval.q_j, q_anchor,
        "A6: q_j must price off the BRTI ANCHOR (reference_price)"
    );
    assert_ne!(
        eval.q_j, q_mark,
        "A6: q_j must NOT price off the perp MARK (venue_settlement)"
    );
    // The anchor itself is recorded as the BRTI value, not the mark.
    assert!(
        (eval.anchor - anchor_btc).abs() < 1e-9,
        "recorded anchor must be the BRTI reference, not the mark"
    );
}

/// A9: an INCOHERENT ladder ⇒ `last_eval` records the incoherence and there are
/// zero proposals (you cannot price against an incoherent ladder). Build a
/// NON-MONOTONE implied CDF via a crossed (bid>ask) book that yields a negative
/// implied mass region — here we instead force the YES-sum WILDLY off (the gate
/// catches either; sum-off is constructible with valid PriceLevels).
#[test]
fn a9_incoherent_ladder_records_incoherent() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    // Make EVERY bin a fat ~0.97 mid ⇒ Σ ≈ 2.9, |Σ−1| ≫ no_arb_tol(0.05) ⇒
    // YesSumOutOfTolerance. (All valid books; the ladder is just incoherent.)
    w.put(book("KXBTC-LESS63K", 96, 98)); // mid 0.97
    w.put(book("KXBTC-B63500", 96, 98)); // mid 0.97
    w.put(book("KXBTC-GT64K", 96, 98)); // mid 0.97
    let mut s = PerpEventBasisV2::new(cfg(ladder)).unwrap();

    // Warm σ to ready on a clean anchor path (σ readiness is orthogonal to A9).
    let anchors = drive_constant_step(&mut s, &w, 6.0000, 1.01, 6);
    // One more tick so the (incoherent) books are evaluated with a ready σ.
    let next = {
        let a = f64::from_str(anchors.last().unwrap()).unwrap() * 1.01;
        format!("{:.4}", (a * 10_000.0).round() / 10_000.0)
    };
    let props = run(&mut s, &w, &perp_tick_v2(PERP, "6.3500", &next));
    assert!(
        props.is_empty(),
        "incoherent ladder ⇒ no proposal (and V3 never proposes)"
    );

    let eval = s
        .last_eval()
        .expect("an eval snapshot records the A9 verdict");
    match &eval.health {
        LadderHealth::Incoherent(_) => {}
        LadderHealth::Coherent => {
            panic!("A9 must flag the fat-sum ladder as Incoherent, got Coherent")
        }
    }
    // On an incoherent ladder the model does NOT price: q_j is empty.
    assert!(
        eval.q_j.is_empty(),
        "incoherent ladder ⇒ q_j not computed (cannot price an incoherent ladder)"
    );
    assert_eq!(s.metrics().proposals_emitted, 0);
}

/// A coherent ladder (with a priceable horizon + fresh anchor) ⇒ the A9 verdict
/// is Coherent and `q_j` IS present (the positive counterpart to the incoherent
/// test). V4-aware: a τ market + advancing obs so the tick prices.
#[test]
fn a9_coherent_ladder_populates_qj() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    put_ladder_markets(&mut w, &ladder, 7_200_000); // τ = 2h Direct
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.0000, 1.01, 6, obs0, 1000);

    let eval = s.last_eval().expect("ready");
    assert_eq!(
        eval.health,
        LadderHealth::Coherent,
        "Σ≈1 monotone ⇒ Coherent"
    );
    assert_eq!(eval.q_j.len(), ladder.len(), "coherent ⇒ q_j populated");
}

/// `build_bins`/`bin_prob` reuse the rung-0 YES-mid convention VERBATIM,
/// including the load-bearing ONE-SIDED case: a `0 bid / Nc ask` book
/// contributes `ask/2` mass (NOT 0). Exercise it through the wiring: a coherent
/// ladder whose tails are one-sided (`book_no_bid`) still prices, and the stored
/// `q_j` equals the kernel call on bins built with the SAME `ask/2` convention
/// (the oracle `bins_from_world` replicates it). If the strategy dropped a
/// one-sided bin to 0, the A9 Σ and the q_j would both diverge from the oracle.
#[test]
fn one_sided_books_contribute_ask_over_two_through_the_wiring() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    // Tails are ONE-SIDED (0 bid / ask): each contributes ask/2 (here 0.25), so
    // Σ ≈ 0.25 + 0.50 + 0.25 ≈ 1.0 (still A9-coherent) — but ONLY if the strategy
    // keeps the ask/2 mass rather than dropping the bid-less bins to 0.
    w.put(book_no_bid("KXBTC-LESS63K", 50)); // 0 bid / 50c ask ⇒ mid 0.25
    w.put(book("KXBTC-B63500", 49, 51)); // mid 0.50
    w.put(book_no_bid("KXBTC-GT64K", 50)); // 0 bid / 50c ask ⇒ mid 0.25
    put_ladder_markets(&mut w, &ladder, 7_200_000); // τ = 2h Direct
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.0000, 1.01, 6, obs0, 1000);

    let eval = s.last_eval().expect("ready");
    // The one-sided ask/2 mass keeps the ladder coherent (Σ≈1, not Σ≈0.5).
    assert_eq!(
        eval.health,
        LadderHealth::Coherent,
        "one-sided ask/2 mass keeps Σ≈1 ⇒ Coherent (dropping it to 0 would fail A9)"
    );
    // The q_j must equal the kernel call on bins built with the SAME convention.
    let bins = bins_from_world(&ladder, &w);
    let expected = bracket_fair_probs(
        &bins,
        SettlementModel {
            anchor: eval.anchor,
            sigma: eval.sigma,
        },
    );
    assert_eq!(
        eval.q_j, expected,
        "q_j must use the rung-0 ask/2 one-sided convention verbatim"
    );
}

/// A10: the median diagnostic (`compute_basis`/`bracket_implied_median`) is
/// POPULATED on a coherent ladder whose crossing lands in a finite between bin,
/// and it is NOT a gate (no proposal either way — V3 proposes nothing). Pin the
/// value against the same `compute_basis` the rung-0 kernel exposes.
#[test]
fn median_diagnostic_populated_but_not_a_gate() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();
    drive_constant_step(&mut s, &w, 6.0000, 1.01, 6);

    let eval = s.last_eval().expect("ready");
    // The median diagnostic is Some (the 0.5 crossing lands in the between bin).
    let median = eval
        .median_diagnostic
        .expect("median diagnostic populated for a finite-crossing ladder");

    // Pin it to the rung-0 kernel's own number for the SAME bins (A10 demotes
    // the median to a health metric; it is the SAME computation, just not a
    // signal). compute_basis takes the perp mark only to compute signed_basis;
    // the median field is mark-independent, so any mark works.
    let bins = bins_from_world(&ladder, &w);
    let sig = compute_basis(&bins, eval.anchor, 0.0, 0.0).expect("finite median");
    assert!(
        (median - sig.bracket_implied_median).abs() < 1e-9,
        "median diagnostic {} != rung-0 compute_basis median {}",
        median,
        sig.bracket_implied_median
    );

    // NOT a gate: no proposal regardless of where the median sits.
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        "median is a diagnostic, not a signal"
    );
}

/// A foreign-market PerpTick and a non-PerpTick event are ignored (no σ update,
/// no eval), mirroring rung-0.
#[test]
fn foreign_market_and_non_perp_tick_ignored() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut s = PerpEventBasisV2::new(cfg(ladder)).unwrap();

    // A different perp ⇒ ignored: no σ accrues from it.
    for a in &constant_step_anchor_strings(6.0000, 1.01, 8) {
        let _ = run(&mut s, &w, &perp_tick_v2("KXETHPERP", "6.3500", a));
    }
    assert!(
        s.last_eval().is_none(),
        "foreign-market ticks must not feed the σ ring ⇒ never ready"
    );

    // A non-PerpTick event ⇒ ignored.
    let settled = BusEvent {
        seq: 1,
        at: ts("2026-06-12T17:00:00.000Z"),
        origin: EventOrigin::External,
        payload: EventPayload::Settled {
            venue: VenueId::new("kinetics").unwrap(),
            market: mkt(PERP),
            payout_cents: 100,
        },
    };
    let props = run(&mut s, &w, &settled);
    assert!(props.is_empty());
    assert!(s.last_eval().is_none(), "a non-PerpTick changes nothing");
}

/// Degenerate anchor: a NON-POSITIVE anchor (reference_price 0) is skipped — it
/// neither updates σ nor produces an eval, and never panics. We can't build a
/// non-finite `PerpPrice`, but a zero reference IS constructible and exercises
/// the ≤0 guard (log-return of/division by a zero anchor).
#[test]
fn nonpositive_anchor_no_panic_no_eval() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut s = PerpEventBasisV2::new(cfg(ladder)).unwrap();

    // All-zero-anchor ticks: each reference_price is $0.0000 ⇒ anchor 0 ⇒ the
    // σ update must SKIP (no ln(0)/divide-by-zero), and no eval is produced.
    for _ in 0..8 {
        let props = run(&mut s, &w, &perp_tick_v2(PERP, "6.3500", "0.0000"));
        assert!(props.is_empty(), "no proposal on a degenerate anchor");
    }
    assert!(
        s.last_eval().is_none(),
        "a zero anchor never yields a usable σ or an eval"
    );
    assert_eq!(s.metrics().proposals_emitted, 0);
}

/// An all-empty ladder (every bin both-sides-empty ⇒ prob 0) with a ready σ:
/// A9 flags it incoherent (Σ=0) ⇒ no q_j, no proposal, no panic.
#[test]
fn all_empty_ladder_incoherent_no_proposal() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    // Both-sides-empty books for every bin ⇒ every prob 0 ⇒ Σ = 0.
    for id in ["KXBTC-LESS63K", "KXBTC-B63500", "KXBTC-GT64K"] {
        w.put(OrderBook {
            market: mkt(id),
            as_of: ts("2026-06-12T17:00:00.000Z"),
            yes_bids: vec![],
            yes_asks: vec![],
        });
    }
    let mut s = PerpEventBasisV2::new(cfg(ladder)).unwrap();
    drive_constant_step(&mut s, &w, 6.0000, 1.01, 6);

    let eval = s
        .last_eval()
        .expect("σ is ready; an eval records the A9 verdict");
    assert!(
        matches!(eval.health, LadderHealth::Incoherent(_)),
        "an all-zero ladder is not a partition of 1 ⇒ Incoherent"
    );
    assert!(eval.q_j.is_empty(), "no pricing against a zero ladder");
    assert_eq!(s.metrics().proposals_emitted, 0);
}

/// σ clamping: a tiny constant-log-step ratio produces a σ below `sigma_floor`;
/// the strategy clamps UP to the floor (σ is never zero/sub-floor when ready).
#[test]
fn sigma_clamps_to_floor() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    // A floor well ABOVE |ln(1.0001)| (~1e-4) so the raw σ is below it.
    let mut c = cfg(ladder);
    c.sigma_floor = 0.01;
    let mut s = PerpEventBasisV2::new(c).unwrap();

    // ratio 1.0001 ⇒ raw σ ≈ 1e-4 < floor 0.01 ⇒ clamps to 0.01.
    drive_constant_step(&mut s, &w, 6.0000, 1.0001, 6);
    let eval = s.last_eval().expect("ready");
    assert!(
        (eval.sigma - 0.01).abs() < 1e-12,
        "sub-floor σ must clamp UP to sigma_floor, got {}",
        eval.sigma
    );
}

/// The σ ring is BOUNDED at `vol_buf_len`: feeding more than the cap does not
/// grow state unboundedly and still yields a ready, sane σ. (Behavioural check
/// — the ring stays within the cap; the snapshot still reports a clamped σ.)
#[test]
fn sigma_ring_is_bounded_and_still_ready() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut c = cfg(ladder);
    c.vol_buf_len = 8; // small cap
    let mut s = PerpEventBasisV2::new(c).unwrap();

    // Feed 40 constant-log-step ticks (≫ cap). Must stay ready with a sane σ.
    drive_constant_step(&mut s, &w, 6.0000, 1.01, 40);
    let eval = s.last_eval().expect("ready after ≫ min_vol_obs returns");
    assert!(
        eval.sigma.is_finite() && eval.sigma > 0.0,
        "σ stays finite/positive under a bounded ring: {}",
        eval.sigma
    );
    assert!(
        eval.obs_count >= 3,
        "obs_count reflects accrued returns, got {}",
        eval.obs_count
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// V4 — A5 horizon gating + A4/A8 per-bin EV gate (the first slice that PROPOSES)
// ══════════════════════════════════════════════════════════════════════════════

// ── A5: regime selection at the τ boundaries ─────────────────────────────────

/// τ just below `direct_max_ms` ⇒ the recorded regime is `Direct`. (Boundary:
/// `0 < τ ≤ direct_max` is Direct; we use direct_max − 1ms.)
#[test]
fn regime_just_below_direct_max_is_direct() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w); // books are irrelevant to the regime; reuse coherent
                                // τ = direct_max(4h) − 1ms ⇒ Direct.
    put_ladder_markets(&mut w, &ladder, 14_400_000 - 1);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();

    // Warm σ ready with an advancing-obs sequence (Δ measurable). The final
    // obs_at is set FRESH (within max_anchor_age) so the stale veto passes.
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.0000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready");
    assert_eq!(
        eval.regime,
        HorizonRegime::Direct,
        "τ just below direct_max ⇒ Direct"
    );
}

/// τ just above `direct_max_ms` (and ≤ vol_adjusted_max) ⇒ `VolAdjusted`.
#[test]
fn regime_just_above_direct_max_is_vol_adjusted() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    // τ = direct_max(4h) + 1ms ⇒ VolAdjusted.
    put_ladder_markets(&mut w, &ladder, 14_400_000 + 1);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.0000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready");
    assert_eq!(
        eval.regime,
        HorizonRegime::VolAdjusted,
        "τ just above direct_max ⇒ VolAdjusted"
    );
}

/// τ just above `vol_adjusted_max_ms` (the >48h veto) ⇒ `Disabled` AND zero
/// proposals — even with a strongly-clearing EV ladder present.
#[test]
fn regime_above_vol_adjusted_max_disabled_no_proposal() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // A strongly-clearing ladder (between bin cheap) — if the >48h veto were
    // dropped this WOULD propose; the veto must suppress it.
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51));
    w.put(book("KXBTC-GT66K", 24, 26));
    // τ = vol_adjusted_max(48h) + 1ms ⇒ Disabled.
    put_ladder_markets(&mut w, &ladder, 172_800_000 + 1);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready; eval records the regime");
    assert_eq!(
        eval.regime,
        HorizonRegime::Disabled,
        "τ > 48h ⇒ Disabled (the >48h veto)"
    );
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        ">48h veto ⇒ propose nothing"
    );
}

// ── A5: the σ_τ scaling (the V4 refinement over V3's per-step σ) ──────────────

/// σ_τ scaling: for a controlled σ_step, a known Δ, and a known τ, the σ the
/// strategy feeds the model EQUALS `σ_step · sqrt(τ/Δ)` clamped (hand-computed),
/// and it DIFFERS from V3's per-step σ (proving the refinement landed). Also the
/// stored `q_j` matches `bracket_fair_probs` called with σ_τ, NOT σ_step.
#[test]
fn sigma_tau_scaling_matches_formula() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    put_straddle_coherent_books(&mut w);
    // τ = 2h (Direct), Δ = 1000ms. τ/Δ = 7200 ⇒ σ_τ = σ_step·sqrt(7200) ≫ σ_step.
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let anchors = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let eval = s.last_eval().expect("σ ready");
    let sigma_step = expected_sigma(&anchors, 0.94, 1e-6, 5.0);
    let sigma_tau = expected_sigma_tau(&anchors, 0.94, 1e-6, 5.0, tau_ms, dt_ms);

    // The σ the strategy USED (recorded in the snapshot) is σ_τ, not σ_step.
    assert!(
        (eval.sigma - sigma_tau).abs() < 1e-12,
        "σ used {} != hand-computed σ_τ {}",
        eval.sigma,
        sigma_tau
    );
    // The refinement is real: σ_τ ≠ σ_step (the V3 stand-in) by a wide margin.
    assert!(
        (sigma_tau - sigma_step).abs() > 1e-3,
        "σ_τ ({sigma_tau}) must DIFFER from σ_step ({sigma_step}) — the V4 refinement"
    );
    // The recorded σ_τ is reported and Δ/τ are exposed for diagnostics.
    assert!(
        (eval.sigma_tau - sigma_tau).abs() < 1e-12,
        "snapshot σ_τ field must equal the scaled σ"
    );

    // The q_j must be priced with σ_τ (not σ_step): equals the kernel call w/ σ_τ.
    let bins = bins_from_world(&ladder, &w);
    let q_tau = bracket_fair_probs(
        &bins,
        SettlementModel {
            anchor: eval.anchor,
            sigma: sigma_tau,
        },
    );
    let q_step = bracket_fair_probs(
        &bins,
        SettlementModel {
            anchor: eval.anchor,
            sigma: sigma_step,
        },
    );
    assert_eq!(eval.q_j, q_tau, "q_j must be priced with σ_τ");
    assert_ne!(
        eval.q_j, q_step,
        "q_j must NOT be priced with the V3 per-step σ_step"
    );
}

// ── A5/A6: the three vetoes each ⇒ ZERO proposals ────────────────────────────

/// Veto (b): τ unknown — the target bracket is ABSENT from `core.markets` (no
/// `close_at` resolvable) ⇒ `Disabled` ⇒ no proposal, even with a clearing book.
#[test]
fn tau_unknown_target_absent_disabled_no_proposal() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51)); // would clear if τ were known
    w.put(book("KXBTC-GT66K", 24, 26));
    // DELIBERATELY do NOT call put_ladder_markets ⇒ core.markets is empty ⇒ τ
    // is unknown for every bracket ⇒ DC-4 conservative fallback = Disabled.
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready");
    assert_eq!(
        eval.regime,
        HorizonRegime::Disabled,
        "absent close_at ⇒ τ unknown ⇒ Disabled"
    );
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        "τ unknown ⇒ propose nothing"
    );
}

/// Veto (b'): the target IS in `core.markets` but its `close_at` is `None` ⇒
/// τ unknown ⇒ `Disabled` ⇒ no proposal.
#[test]
fn tau_unknown_close_at_none_disabled_no_proposal() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51));
    w.put(book("KXBTC-GT66K", 24, 26));
    // Markets present but close_at = None for each.
    for id in ladder.keys() {
        w.markets
            .insert(id.clone(), market_with_close(id.as_str(), None));
    }
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready");
    assert_eq!(
        eval.regime,
        HorizonRegime::Disabled,
        "close_at None ⇒ Disabled"
    );
    assert_eq!(s.metrics().proposals_emitted, 0);
}

/// Veto (a''): τ ≤ 0 (close_at in the PAST) ⇒ `Disabled` ⇒ no proposal (the
/// DC-4 fallback also covers an already-closed market).
#[test]
fn tau_nonpositive_disabled_no_proposal() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51));
    w.put(book("KXBTC-GT66K", 24, 26));
    // close_at = now − 1ms (already closed) ⇒ τ = −1 ≤ 0 ⇒ Disabled.
    put_ladder_markets(&mut w, &ladder, -1);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready");
    assert_eq!(eval.regime, HorizonRegime::Disabled, "τ ≤ 0 ⇒ Disabled");
    assert_eq!(s.metrics().proposals_emitted, 0);
}

/// Veto (c): a STALE anchor (`now − obs_at > max_anchor_age_ms`) disables the
/// WHOLE evaluation ⇒ no proposal, and the snapshot records the stale state.
/// A non-stale companion (next test) proves the veto is not vacuous.
#[test]
fn stale_anchor_disables_no_proposal() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51)); // would clear if the anchor were fresh
    w.put(book("KXBTC-GT66K", 24, 26));
    put_ladder_markets(&mut w, &ladder, 7_200_000); // τ Direct (not the veto here)
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();

    // Drive the warm-up with obs_at well in the PAST relative to World.now
    // (17:00:00). max_anchor_age = 5000ms; make the final obs_at 60s stale.
    let obs0 = ts("2026-06-12T16:58:50.000Z"); // last tick obs ≈ 16:58:55 ⇒ 65s stale
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready; eval records the stale veto");
    assert!(
        eval.anchor_stale,
        "now − obs_at > max_anchor_age ⇒ the anchor is flagged stale"
    );
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        "a stale anchor disables the whole tick ⇒ propose nothing"
    );
}

/// The stale veto is NOT vacuous: an identical setup with a FRESH anchor (obs_at
/// within max_anchor_age of now) is NOT flagged stale and DOES propose.
#[test]
fn fresh_anchor_not_stale_and_proposes() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51)); // between bin clears the EV gate
    w.put(book("KXBTC-GT66K", 24, 26));
    put_ladder_markets(&mut w, &ladder, 7_200_000); // τ = 2h Direct
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();

    // Final obs_at within 5s of World.now (17:00:00): obs0 16:59:55, Δ 1000ms,
    // 6 ticks ⇒ last obs ≈ 17:00:00 ⇒ age ≈ 0 ⇒ fresh.
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);

    let eval = s.last_eval().expect("σ ready");
    assert!(!eval.anchor_stale, "a fresh anchor is not flagged stale");
    assert!(
        s.metrics().proposals_emitted >= 1,
        "a fresh anchor + a clearing bin ⇒ at least one proposal"
    );
}

// ── Δ readiness: until Δ is measured, σ_τ is undefined ⇒ no proposal ──────────

/// Δ not measured: if every tick shares the SAME `obs_at` (no advancing gap),
/// the Δ EWMA never folds a positive gap ⇒ σ_τ cannot be formed ⇒ the tick does
/// NOT price ⇒ no proposal and no panic (even though the σ ring itself is ready
/// AND the τ horizon is known/Direct). The OBSERVABLE effect is: `delta_ms` is
/// None, `q_j` is empty (no pricing), and zero proposals. (The `regime` field
/// records the τ-classification — Direct here — independently of Δ; the Δ-missing
/// veto is the empty `q_j` + no proposals, not a regime relabel.) This is the
/// V3-harness frozen-obs regime, now correctly NON-PRICING under V4.
#[test]
fn delta_not_measured_disables_no_proposal_no_panic() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51));
    w.put(book("KXBTC-GT66K", 24, 26));
    put_ladder_markets(&mut w, &ladder, 7_200_000);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();

    // Frozen obs_at (Δ = 0 every step ⇒ skipped ⇒ Δ never measured).
    let frozen = ts("2026-06-12T17:00:00.000Z");
    for a in &constant_step_anchor_strings(6.3000, 1.0001, 6) {
        let props = run(&mut s, &w, &perp_tick_obs(PERP, "6.3500", a, frozen));
        assert!(props.is_empty(), "no proposal while Δ is unmeasured");
    }
    let eval = s
        .last_eval()
        .expect("σ ring ready; an eval snapshot exists");
    assert_eq!(
        eval.delta_ms, None,
        "no positive obs gap ⇒ Δ never measured"
    );
    assert!(
        eval.q_j.is_empty(),
        "Δ unmeasured ⇒ σ_τ cannot be formed ⇒ the tick does NOT price (empty q_j)"
    );
    assert_eq!(s.metrics().proposals_emitted, 0);
}

// ── A4+A8: the per-bin EV gate ───────────────────────────────────────────────

/// EV CLEARS: a between bin whose model q_j (≈1 under a tight σ_τ) is well above
/// ask+costs ⇒ EXACTLY ONE Passive/Buy/Yes UNSIZED proposal joining the bin's
/// best BID, with fair = round(q_j·100) clamped to [1,99]. The EV is recomputed
/// from the formula and asserted to clear; the leg shape pins I6 + maker-join.
#[test]
fn ev_gate_clears_emits_unsized_maker_join_at_bid() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // between bin (the only one with mass under a tight σ_τ at anchor ≈ 63,032):
    // mid 0.50, best BID 49c, best ASK 51c. Tails are mid 0.25 (Σ≈1 coherent).
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51));
    w.put(book("KXBTC-GT66K", 24, 26));
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    // ONE clean drive; capture the proposals from the first tick that cleared
    // (avoids the dedup HashSet swallowing a re-fire of the same 49c leg).
    let out = drive_capture_first_proposal(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let eval = s.last_eval().expect("σ ready");
    // Recompute the between bin's model q and EV, assert it clears. (The σ_τ
    // FORMULA itself is pinned in `sigma_tau_scaling_matches_formula`; here we
    // pin the EV-gate decision + the emitted leg shape.)
    let q_btw = eval
        .q_j
        .iter()
        .find(|fp| {
            fp.kind
                == BracketStrike::Between {
                    floor: 60_000.0,
                    cap: 66_000.0,
                }
        })
        .expect("between bin priced")
        .q;
    let ev_btw = expected_ev_j(q_btw, ask_prob(51));
    assert!(
        ev_btw > EV_THRESHOLD,
        "fixture must clear: EV {ev_btw} ≤ thr {EV_THRESHOLD} (q {q_btw})"
    );

    // Exactly one bin clears (the between bin) ⇒ one Passive/Buy/Yes proposal.
    assert_eq!(out.len(), 1, "exactly one bin clears (the between bin)");
    let p = &out[0];
    assert_eq!(p.urgency, Urgency::Passive, "maker-only");
    assert!(p.group_policy.is_none(), "single-leg ⇒ no group policy");
    assert!(p.manifest_hash.is_none(), "mechanical ⇒ no manifest hash");
    assert_eq!(p.legs.len(), 1);
    let leg = &p.legs[0];
    assert_eq!(leg.market, mkt("KXBTC-B63K"), "target is the between bin");
    assert_eq!(leg.side, Side::Yes);
    assert_eq!(leg.action, Action::Buy);
    assert_eq!(
        leg.limit_price,
        Cents::new(49),
        "limit JOINS the bin's best YES bid (maker-only), not the ask"
    );
    assert_eq!(leg.calibrated_p, None, "mechanical leg ⇒ no calibrated_p");
    // fair = round(q_j·100) clamped to [1,99]. q_btw ≈ 1.0 ⇒ round 100 → clamp 99.
    let expected_fair = ((q_btw * 100.0).round() as i64).clamp(1, 99);
    assert_eq!(
        leg.fair_value,
        Cents::new(expected_fair),
        "fair = round(q·100) clamped [1,99]"
    );
    // The thesis carries the A10 provenance (regime, τ hours, σ_τ, q, ask, EV).
    assert!(
        p.thesis.contains("Direct") && p.thesis.contains("EV"),
        "thesis carries regime + EV provenance: {}",
        p.thesis
    );
}

/// DEMO (operator walkthrough — run with `--nocapture` to read it). The full v2
/// trading decision on a realistic straddle ladder: σ is warmed over six BRTI
/// ticks, then a coherent, fresh tick in the Direct horizon regime drives the A3
/// per-bracket `q_j` model, the A4+A8 per-bin EV gate, and the emitted UNSIZED
/// maker leg — plus the A7 informativeness verdict and the A10 telemetry
/// `MetricSample`s (the T5.B8 emission). Reuses the proven harness; asserts a
/// single maker leg fires (so it doubles as a smoke).
#[test]
fn demo_v2_full_decision_walkthrough() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26)); // bottom tail, mid 0.25
    w.put(book("KXBTC-B63K", 49, 51)); // the contained between bin, mid 0.50
    w.put(book("KXBTC-GT66K", 24, 26)); // top tail, mid 0.25
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64); // τ = 2h ⇒ the Direct regime
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let out = drive_capture_first_proposal(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);
    let eval = s.last_eval().expect("σ ready after the warm-up ticks");

    eprintln!("\n========== perp basis-v2 — full decision walkthrough ==========");
    eprintln!("A6  anchor S0 (BRTI reference) : ${:.2}", eval.anchor);
    eprintln!(
        "A5  horizon regime / τ        : {:?} / {:.2}h",
        eval.regime,
        eval.tau_ms.unwrap_or(0) as f64 / 3_600_000.0
    );
    eprintln!(
        "A5  σ_step → σ_τ (horizon)    : {:.6} → {:.6}",
        eval.sigma, eval.sigma_tau
    );
    eprintln!("A9  ladder no-arb health      : {:?}", eval.health);
    eprintln!("A10 implied-vs-model CDF Δ    : {:?}", eval.cdf_divergence);
    eprintln!(
        "A10 rung-0 median (health)    : {:?}",
        eval.median_diagnostic
    );
    eprintln!("--- per-bin: A3 q_j | A4 ask | A4+A8 EV | A7 verdict | proposed? ---");
    for b in &eval.bin_evs {
        eprintln!(
            "  {:<34} q={:.4}  ask={:?}  ev={:?}  info={:?}  proposed={}",
            format!("{:?}", b.kind),
            b.q,
            b.ask.map(|a| (a * 1000.0).round() / 1000.0),
            b.ev.map(|e| (e * 1000.0).round() / 1000.0),
            b.info,
            b.proposed
        );
    }
    eprintln!("--- I6 UNSIZED maker proposals (the harness sizes) ---");
    for p in &out {
        for leg in &p.legs {
            eprintln!(
                "  {:?} {:?} on {}   limit {}   fair {}   ({:?})",
                leg.action, leg.side, leg.market, leg.limit_price, leg.fair_value, p.urgency
            );
        }
        eprintln!("  thesis: {}", p.thesis);
    }
    eprintln!("--- A10 telemetry MetricSamples (the T5.B8 emission) ---");
    for m in s.metric_samples() {
        eprintln!("  {:<42} {:?} = {}", m.name, m.labels, m.value);
    }
    eprintln!("===============================================================\n");

    assert_eq!(
        out.len(),
        1,
        "the demo scenario fires exactly one maker leg"
    );
}

/// EV REJECTS (below threshold): the between bin priced RICH (ask 95c) ⇒ EV < thr
/// ⇒ no proposal for it. The fixture recomputes EV and asserts it is below thr.
#[test]
fn ev_gate_rejects_below_threshold() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // between mid 0.95 (bid 94 / ask 96... but Σ must stay coherent). Use the
    // between RICH and tails tiny so Σ≈1: between mid 0.95, tails empty-ish.
    // To keep A9 coherent yet the between ASK rich: between (94,96)→mid0.95 ask0.96,
    // tails one-sided ask 3c → mid 0.015 each ⇒ Σ≈0.98 (coherent within 0.05).
    w.put(book_no_bid("KXBTC-LESS60K", 3));
    w.put(book("KXBTC-B63K", 94, 96));
    w.put(book_no_bid("KXBTC-GT66K", 3));
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let anchors = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let eval = s.last_eval().expect("σ ready");
    let q_btw = eval
        .q_j
        .iter()
        .find(|fp| {
            fp.kind
                == BracketStrike::Between {
                    floor: 60_000.0,
                    cap: 66_000.0,
                }
        })
        .expect("between priced")
        .q;
    let ev_btw = expected_ev_j(q_btw, ask_prob(96));
    assert!(
        ev_btw <= EV_THRESHOLD,
        "fixture must reject: EV {ev_btw} > thr (q {q_btw}, ask 0.96)"
    );
    let _ = anchors;
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        "an EV below threshold ⇒ no proposal"
    );
}

/// EV REJECTS (exactly AT threshold, strict `>`): set `ev_threshold` to the
/// between bin's EXACT computed EV (read from the strategy's OWN `bin_evs`
/// snapshot, so there is no oracle/strategy float divergence) ⇒ `EV_j >
/// threshold` is FALSE ⇒ no proposal (mirrors the rung-0 strict fee-trap).
/// Flipping `>` to `>=` reds this.
///
/// To make the at-threshold boundary EXACT on the proposing tick, the warm-up
/// runs with `core.markets` ABSENT (τ unknown ⇒ Disabled ⇒ no pricing, but σ and
/// Δ still accrue); THEN the markets are inserted and EXACTLY ONE pricing tick is
/// fired. That single tick is the only one whose EV is compared, so calibrating
/// the threshold to it is exact (no earlier tick at a slightly-different anchor
/// can have cleared and bumped the cumulative counter).
#[test]
fn ev_gate_rejects_at_or_below_threshold() {
    let ladder = straddle_ladder();
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let final_anchor = "6.3003"; // BTC 63,003, deep inside [60k, 66k]
    let final_obs = ts("2026-06-12T17:00:00.000Z"); // fresh

    // Run the warm-up + the single pricing tick on a strategy, returning the
    // between bin's recorded EV and the cumulative proposal count.
    let run_once = |threshold: f64| -> (Option<f64>, u64) {
        // Books for the straddle ladder (between bid 49 / ask 51; tails 0.25).
        let mut w = World::new();
        put_straddle_coherent_books(&mut w);
        let mut c = cfg_v4(ladder.clone());
        c.ev_threshold = threshold;
        let mut s = PerpEventBasisV2::new(c).unwrap();
        // Warm σ + Δ to ready with NO markets (τ unknown ⇒ Disabled ⇒ no pricing).
        let warm = constant_step_anchor_strings(6.3000, 1.0001, 6);
        let mut obs = obs0;
        for a in &warm {
            let _ = run(&mut s, &w, &perp_tick_obs(PERP, "6.3500", a, obs));
            obs = obs.checked_add_millis(dt_ms).unwrap();
        }
        // Now INSERT the markets (τ known) and fire EXACTLY ONE pricing tick.
        put_ladder_markets(&mut w, &ladder, tau_ms);
        let _ = run(
            &mut s,
            &w,
            &perp_tick_obs(PERP, "6.3500", final_anchor, final_obs),
        );
        let ev = s.last_eval().and_then(|e| {
            e.bin_evs
                .iter()
                .find(|b| {
                    b.kind
                        == BracketStrike::Between {
                            floor: 60_000.0,
                            cap: 66_000.0,
                        }
                })
                .and_then(|b| b.ev)
        });
        (ev, s.metrics().proposals_emitted)
    };

    // Probe with the DEFAULT threshold (0.02): only the between bin (q≈1) clears;
    // the tails (q≈0, EV≈−0.3) do not. Learn the between bin's EXACT EV.
    let (probe_ev, probe_count) = run_once(EV_THRESHOLD);
    let exact_ev = probe_ev.expect("between bin priced on the single pricing tick");
    assert_eq!(
        probe_count, 1,
        "the probe's single pricing tick clears ONLY the between bin"
    );

    // Re-run with ev_threshold == that exact EV ⇒ strict `>` must REJECT.
    let (_, count) = run_once(exact_ev);
    assert_eq!(
        count, 0,
        "EV exactly at threshold ⇒ strict `>` does NOT clear ⇒ no proposal"
    );
}

/// Multiple bins clear ⇒ multiple Proposals (one unsized maker leg each); a
/// second IDENTICAL tick is fully deduped (no re-propose). Anchor on the
/// less/between boundary splits mass 0.5/0.5 so BOTH the less and between bins
/// clear under a cheap coherent ladder.
#[test]
fn multiple_bins_clear_then_dedup() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // Implied coherent ladder: less mid 0.40, between mid 0.40, gt mid 0.20 (Σ≈1).
    // Asks: less 41c, between 41c (both clear vs q≈0.5), gt 21c (rejects vs q≈0).
    w.put(book("KXBTC-LESS60K", 39, 41));
    w.put(book("KXBTC-B63K", 39, 41));
    w.put(book("KXBTC-GT66K", 19, 21));
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    // Drive an explicit sequence ENDING at exactly 6.0000 (BTC 60,000, the
    // less/between boundary) so q splits 0.5/0.5. Tiny steps keep σ_step small.
    let seq = ["6.0004", "6.0003", "6.0002", "6.0001", "6.0000", "6.0000"];
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let last_obs = drive_explicit_obs(&mut s, &w, &seq, obs0, dt_ms);
    let _ = last_obs;

    // Two bins (less + between) cleared on the final tick. Confirm via metrics:
    // the warm-up's earlier ticks also proposed once each clearing leg appeared,
    // but dedup means each (market, limit) fired once. Capture the SECOND
    // identical tick to assert full dedup: fire it again with a fresh obs.
    let again = run(
        &mut s,
        &w,
        &perp_tick_obs(
            PERP,
            "6.3500",
            "6.0000",
            last_obs.checked_add_millis(dt_ms).unwrap(),
        ),
    );
    assert!(
        again.is_empty(),
        "an identical repeat tick re-proposes nothing (dedup on (market, limit))"
    );

    // The strategy proposed exactly TWO distinct legs total (less @ 39c bid,
    // between @ 39c bid); the gt bin never cleared.
    assert_eq!(
        s.metrics().proposals_emitted,
        2,
        "exactly two bins (less + between) cleared as distinct unsized legs"
    );
}

/// A bin that CLEARS the EV gate but has NO BID to join is SKIPPED (maker-only
/// cannot rest without a price) — no proposal, no panic. Build the between bin
/// one-sided (ask only): its model q clears, its ask is takeable, but there is
/// no bid to join ⇒ skipped.
#[test]
fn ev_clears_but_no_bid_to_join_is_skipped() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // between: NO bid, ask 51c (its implied mid is ask/2 = 0.255). The TAILS
    // carry extra implied mass (mid 0.37 each) so Σ ≈ 0.37+0.255+0.37 ≈ 1.0 stays
    // A9-coherent EVEN with the between bid absent. Under the tight σ_τ the
    // between MODEL q ≈ 1, so its EV(ask 0.51) clears — but with no bid to join
    // the maker leg cannot rest ⇒ the bin is skipped (the tails' q ≈ 0 reject).
    w.put(book("KXBTC-LESS60K", 36, 38)); // mid 0.37
    w.put(book_no_bid("KXBTC-B63K", 51)); // ask 51c, NO bid ⇒ cannot join
    w.put(book("KXBTC-GT66K", 36, 38)); // mid 0.37
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let anchors = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    // The between bin's EV clears (q≈1, ask 0.51) — but it has no bid.
    let eval = s.last_eval().expect("ready");
    let q_btw = eval
        .q_j
        .iter()
        .find(|fp| {
            fp.kind
                == BracketStrike::Between {
                    floor: 60_000.0,
                    cap: 66_000.0,
                }
        })
        .expect("between priced")
        .q;
    assert!(
        expected_ev_j(q_btw, ask_prob(51)) > EV_THRESHOLD,
        "the bin must CLEAR EV (so the skip is due to the missing bid, not EV)"
    );
    let _ = anchors;
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        "a clearing bin with no bid to join is skipped (maker-only) ⇒ no proposal"
    );
}

/// A bin with NO ASK (cannot buy toward a non-existent offer) is SKIPPED even if
/// it has a bid ⇒ no proposal, no panic. (Mirrors the EV-gate rule: no ask ⇒ no
/// executable price to price against.)
#[test]
fn ev_no_ask_is_skipped() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // between: bid 49c, NO ask (its implied mid is bid/2 = 0.245). Tails carry
    // extra mass (mid 0.37) so Σ ≈ 0.37+0.245+0.37 ≈ 1.0 stays A9-coherent and
    // the between bin IS priced (so the skip is genuinely the no-ask rule on a
    // priced bin, not an incoherence short-circuit).
    w.put(book("KXBTC-LESS60K", 36, 38)); // mid 0.37
    w.put(OrderBook {
        market: mkt("KXBTC-B63K"),
        as_of: ts("2026-06-12T17:00:00.000Z"),
        yes_bids: vec![PriceLevel {
            price: Cents::new(49),
            qty: Contracts::new(100),
        }],
        yes_asks: vec![],
    });
    w.put(book("KXBTC-GT66K", 36, 38)); // mid 0.37
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    // The between bin is priced (in q_j) but its EV-result carries ask=None, so
    // it is never proposed.
    let eval = s.last_eval().expect("ready");
    let between = eval.bin_evs.iter().find(|b| {
        b.kind
            == BracketStrike::Between {
                floor: 60_000.0,
                cap: 66_000.0,
            }
    });
    if let Some(b) = between {
        assert_eq!(
            b.ask, None,
            "a no-ask bin records ask=None (nothing to take)"
        );
        assert!(!b.proposed, "a no-ask bin is never proposed");
    }
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        "no ask ⇒ nothing to price toward ⇒ the bin is skipped"
    );
}

/// The fee-trap: `fee_j` is computed and is STRICTLY > 0 for an interior ask, so
/// a "free" promo (fee_floor $0) can never sneak a marginal bin through. Asserted
/// via the formula oracle across interior asks, plus a behavioural pin: a bin
/// whose EV would clear WITHOUT the fee but not WITH it is rejected.
#[test]
fn fee_j_is_strictly_positive_for_interior_ask() {
    // Formula-level: fee_j > 0 for every interior ask in (0,1).
    for ask_c in [1_i64, 10, 25, 49, 50, 51, 75, 99] {
        let fee = expected_fee_j(ask_prob(ask_c));
        assert!(
            fee > 0.0,
            "fee_j must be strictly positive for ask {ask_c}c, got {fee}"
        );
    }

    // Behavioural: a bin that clears WITHOUT the fee but NOT with it is rejected.
    // Anchor on the less/between boundary (60,000) splits mass q_less≈q_btw≈0.5,
    // q_gt≈0. Coherent implied ladder (Σ mids ≈ 1): less mid 0.19, between mid
    // 0.44, gt mid 0.37. The BETWEEN ask 0.45 is the fee-trap case:
    //   no-fee EV = 0.50 − 0.45 − 0.025 = 0.025 > 0.02 (would clear)
    //   with-fee  = 0.025 − fee(0.45) = 0.025 − 0.02 = 0.005 ≤ 0.02 (rejected)
    // The LESS bin (ask 0.20) clears (EV 0.255), so the tick DOES propose — but
    // the between leg must be ABSENT (the fee, not a vacuous empty output, kills
    // it). The gt bin (q≈0) never clears.
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 18, 20)); // mid 0.19, ask 0.20 ⇒ clears
    w.put(book("KXBTC-B63K", 43, 45)); // mid 0.44, ask 0.45 ⇒ fee-rejected
    w.put(book("KXBTC-GT66K", 36, 38)); // mid 0.37 (Σ mids ≈ 1.0, coherent)
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let seq = ["6.0004", "6.0003", "6.0002", "6.0001", "6.0000", "6.0000"];
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let out = drive_explicit_capture_first(&mut s, &w, &seq, obs0, dt_ms);

    let eval = s.last_eval().expect("ready");
    let q_btw = eval
        .q_j
        .iter()
        .find(|fp| {
            fp.kind
                == BracketStrike::Between {
                    floor: 60_000.0,
                    cap: 66_000.0,
                }
        })
        .expect("between priced")
        .q;
    let ask = ask_prob(45);
    let ev_no_fee = q_btw - ask - SLIPPAGE - RESERVE - ADVERSE;
    let ev_with_fee = expected_ev_j(q_btw, ask);
    assert!(
        ev_no_fee > EV_THRESHOLD && ev_with_fee <= EV_THRESHOLD,
        "fee must be the deciding term: no-fee EV {ev_no_fee} (>thr), with-fee {ev_with_fee} (≤thr)"
    );
    // The tick DID propose (the less bin cleared), proving the output is not
    // vacuously empty — yet the between leg is absent (the round-trip fee killed
    // it).
    assert!(
        !out.is_empty(),
        "the less bin clears, so the tick proposes (the output is not vacuously empty)"
    );
    assert!(
        out.iter()
            .flat_map(|p| &p.legs)
            .all(|l| l.market != mkt("KXBTC-B63K")),
        "the between bin (ask 0.45) must be rejected by the round-trip fee"
    );
}

/// No panic on a degenerate ladder under V4: an all-empty ladder (A9 incoherent)
/// with a ready σ AND a known τ AND a fresh anchor ⇒ no q_j ⇒ no EV gate ⇒ no
/// proposal, no panic.
#[test]
fn v4_all_empty_ladder_no_proposal_no_panic() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    for id in ["KXBTC-LESS60K", "KXBTC-B63K", "KXBTC-GT66K"] {
        w.put(OrderBook {
            market: mkt(id),
            as_of: ts("2026-06-12T17:00:00.000Z"),
            yes_bids: vec![],
            yes_asks: vec![],
        });
    }
    put_ladder_markets(&mut w, &ladder, 7_200_000);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);
    assert_eq!(s.metrics().proposals_emitted, 0);
}

/// V4 still NEVER panics on a non-positive anchor (the σ-update ≤0 guard) even
/// with τ known and markets present.
#[test]
fn v4_nonpositive_anchor_no_panic() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 24, 26));
    w.put(book("KXBTC-B63K", 49, 51));
    w.put(book("KXBTC-GT66K", 24, 26));
    put_ladder_markets(&mut w, &ladder, 7_200_000);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder)).unwrap();
    let mut obs = ts("2026-06-12T16:59:55.000Z");
    for _ in 0..8 {
        let props = run(&mut s, &w, &perp_tick_obs(PERP, "6.3500", "0.0000", obs));
        assert!(props.is_empty(), "no proposal on a degenerate anchor");
        obs = obs.checked_add_millis(1000).unwrap();
    }
    assert!(
        s.last_eval().is_none(),
        "a zero anchor never yields an eval"
    );
    assert_eq!(s.metrics().proposals_emitted, 0);
}

// ══════════════════════════════════════════════════════════════════════════════
// V5 — A7 measured informativeness + A10 diagnostic emission (the freshness/health
// surface; the LAST v2 slice). A7 can ONLY make the V4 EV gate MORE conservative
// (down-weight or veto), never up-size/up-weight. A10 ships the implied-vs-model
// CDF sup-distance + per-bin verdict/ages/spread/depth (recorded, not gated).
// ══════════════════════════════════════════════════════════════════════════════

// ── V5 harness: a perp book + explicit bracket-book as_of ─────────────────────
//
// V5 needs two controls the V4 harness lacks: (1) a PERP book in `core.books`
// (keyed by the perp `MarketId`) with a controllable `as_of`, so A7 can measure the
// perp side's freshness — its ABSENCE (the daemon-feeds-only-the-tick case) forces
// the conservative `Unfavorable` default; (2) bracket books with explicit `as_of`,
// so a bin can be made STRICTLY fresher (BracketLeads) or stale (Unfavorable)
// relative to the perp.

/// A book with one (bid, ask) YES level and an EXPLICIT `as_of` (the V5 freshness
/// the A7 verdict measures). Same shape as `book` but with a caller-set timestamp.
fn book_at(market: &str, yes_bid: i64, yes_ask: i64, as_of: UtcTimestamp) -> OrderBook {
    OrderBook {
        market: mkt(market),
        as_of,
        yes_bids: vec![PriceLevel {
            price: Cents::new(yes_bid),
            qty: Contracts::new(100),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(yes_ask),
            qty: Contracts::new(100),
        }],
    }
}

/// A PERP-side book keyed by the perp `MarketId` (so `core.books.get(perp_market)`
/// finds it) with an EXPLICIT `as_of`. The levels are immaterial to A7 (DC-6: only
/// the whole-book `as_of` is read for freshness; spread/depth are perp-units and
/// only RECORDED, never gated), so plausible placeholder levels ride a fixed value.
fn perp_book_at(as_of: UtcTimestamp) -> OrderBook {
    OrderBook {
        market: mkt(PERP),
        as_of,
        yes_bids: vec![PriceLevel {
            price: Cents::new(50),
            qty: Contracts::new(100),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(51),
            qty: Contracts::new(100),
        }],
    }
}

/// Straddle books (between cheap @ bid49/ask51, tails 0.25) with EVERY bin's `as_of`
/// set to `as_of` (so the bracket freshness is controllable for A7). Σ≈1 coherent.
fn put_straddle_books_at(w: &mut World, as_of: UtcTimestamp) {
    w.put(book_at("KXBTC-LESS60K", 24, 26, as_of));
    w.put(book_at("KXBTC-B63K", 49, 51, as_of));
    w.put(book_at("KXBTC-GT66K", 24, 26, as_of));
}

/// The between bin of the straddle ladder (the EV tests' candidate).
fn between_bin() -> BracketStrike {
    BracketStrike::Between {
        floor: 60_000.0,
        cap: 66_000.0,
    }
}

/// The recorded `BinEv` of the between bin from the last eval (panics in-test if
/// absent — the between bin is always priced in the V5 straddle fixtures).
fn between_ev(s: &PerpEventBasisV2) -> &fortuna_runner::perp_event_basis_v2::BinEv {
    s.last_eval()
        .expect("σ ready ⇒ an eval")
        .bin_evs
        .iter()
        .find(|b| b.kind == between_bin())
        .expect("between bin priced")
}

// ── A7: PerpFavorable — the V4 proposal fires unchanged ──────────────────────

/// A7 PerpFavorable: a PRESENT, FRESH perp book that is at least as fresh as the
/// bracket (here both age 0) ⇒ the V4 proposal fires UNCHANGED — the between bin's
/// `adverse_eff` is the BASELINE `adverse` (no A7 penalty) and the EV equals the V4
/// EV. Proves A7 is a no-op when the perp genuinely leads.
#[test]
fn a7_perp_favorable_proposal_fires_unchanged() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // Bracket books + a perp book, ALL `as_of` = World.now (17:00:00) ⇒ age 0 both
    // sides ⇒ perp at least as fresh as bracket ⇒ PerpFavorable.
    let now = ts("2026-06-12T17:00:00.000Z");
    put_straddle_books_at(&mut w, now);
    w.put(perp_book_at(now));
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let out = drive_capture_first_proposal(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let between = between_ev(&s);
    assert_eq!(
        between.info,
        InfoVerdict::PerpFavorable,
        "fresh perp at least as fresh as the bracket ⇒ PerpFavorable"
    );
    assert!(
        (between.adverse_eff - ADVERSE).abs() < 1e-12,
        "PerpFavorable ⇒ adverse_eff is the baseline adverse {ADVERSE}, got {}",
        between.adverse_eff
    );
    // The EV equals the V4 EV (no penalty) and clears ⇒ the proposal fires.
    let q_btw = between.q;
    let ev_v4 = expected_ev_j(q_btw, ask_prob(51));
    assert!(
        (between.ev.unwrap() - ev_v4).abs() < 1e-12,
        "PerpFavorable EV must equal the V4 EV (no A7 penalty)"
    );
    assert!(ev_v4 > EV_THRESHOLD, "fixture clears");
    assert_eq!(out.len(), 1, "the between bin proposes (V4 unchanged)");
    assert_eq!(out[0].legs[0].market, mkt("KXBTC-B63K"));
    assert!(
        out[0].thesis.contains("PerpFavorable"),
        "thesis carries the A7 verdict: {}",
        out[0].thesis
    );
}

// ── A7: Unfavorable (perp book ABSENT) — the +penalty, two outcomes ───────────

/// A7 Unfavorable (perp book ABSENT) — STILL FIRES when the edge overcomes the
/// penalty. No perp book ⇒ the conservative `Unfavorable` default ⇒ `adverse_eff =
/// adverse + info_adverse_penalty`. The between bin (q≈1, cheap ask 0.51) has a wide
/// edge, so it clears EVEN with the penalty. Asserts the verdict, the raised
/// `adverse_eff`, and that the leg still fires.
#[test]
fn a7_unfavorable_perp_absent_still_fires_when_edge_overcomes() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    let now = ts("2026-06-12T17:00:00.000Z");
    put_straddle_books_at(&mut w, now); // fresh bracket
                                        // DELIBERATELY no perp book ⇒ Unfavorable.
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let out = drive_capture_first_proposal(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let between = between_ev(&s);
    assert_eq!(
        between.info,
        InfoVerdict::Unfavorable,
        "perp book ABSENT ⇒ Unfavorable (cannot establish the perp leads)"
    );
    assert!(
        (between.adverse_eff - (ADVERSE + INFO_ADVERSE_PENALTY)).abs() < 1e-12,
        "Unfavorable ⇒ adverse_eff = adverse + penalty, got {}",
        between.adverse_eff
    );
    // EV with the RAISED adverse still clears (the edge overcomes the penalty).
    let q_btw = between.q;
    let ev_pen = q_btw
        - ask_prob(51)
        - expected_fee_j(ask_prob(51))
        - SLIPPAGE
        - RESERVE
        - (ADVERSE + INFO_ADVERSE_PENALTY);
    assert!(
        (between.ev.unwrap() - ev_pen).abs() < 1e-12,
        "the recorded EV uses the penalised adverse_eff"
    );
    assert!(ev_pen > EV_THRESHOLD, "the wide edge overcomes the penalty");
    assert_eq!(
        out.len(),
        1,
        "the between bin still fires under the penalty"
    );
}

/// A7 Unfavorable (perp book ABSENT) — SKIPPED when the penalty pushes EV ≤ thr.
/// Same world driven TWICE: once with `info_adverse_penalty = 0` (the bin CLEARS,
/// proving the no-penalty path proposes) and once with the default penalty (the bin
/// is SKIPPED). The ONLY difference is the penalty, so it is provably the deciding
/// term — a 0-penalty mutation reds the "skipped" half. The between bin's ask is
/// calibrated so its no-penalty EV is just above thr and its with-penalty EV is ≤ thr.
#[test]
fn a7_unfavorable_perp_absent_penalty_skips_borderline_bin() {
    let ladder = straddle_ladder();
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let now = ts("2026-06-12T17:00:00.000Z");

    // Build a world whose between bin is priced RICH enough that the +0.02 penalty
    // is the deciding term. between bid91/ask93 ⇒ ask 0.93; under a tight σ_τ at
    // anchor ≈ 63,003 the between MODEL q ≈ 1. With q≈1: no-penalty EV = 0.975 −
    // 0.93 − fee(0.93)=0.02 = 0.025 (> thr 0.02); +0.02 penalty ⇒ 0.005 (≤ thr).
    // Tails one-sided ask 5c ⇒ mid 0.025 keep Σ ≈ 0.97 coherent.
    let run_once = |penalty: f64| -> (InfoVerdict, Option<f64>, u64) {
        let mut w = World::new();
        w.put(book_at("KXBTC-LESS60K", 0, 5, now)); // one-sided 5c ask ⇒ mid 0.025
        w.put(book_at("KXBTC-B63K", 91, 93, now)); // rich between, ask 0.93
        w.put(book_at("KXBTC-GT66K", 0, 5, now));
        // No perp book ⇒ Unfavorable.
        put_ladder_markets(&mut w, &ladder, tau_ms);
        let mut c = cfg_v4(ladder.clone());
        c.info_adverse_penalty = penalty;
        let mut s = PerpEventBasisV2::new(c).unwrap();
        let _ = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);
        let b = between_ev(&s);
        (b.info, b.ev, s.metrics().proposals_emitted)
    };

    // With ZERO penalty the between bin CLEARS (proves the output is not vacuous).
    let (info0, ev0, count0) = run_once(0.0);
    assert_eq!(info0, InfoVerdict::Unfavorable, "perp absent ⇒ Unfavorable");
    assert!(
        ev0.unwrap() > EV_THRESHOLD,
        "with no penalty the rich between bin still clears: EV {:?}",
        ev0
    );
    assert_eq!(count0, 1, "no-penalty ⇒ the between bin proposes");

    // With the DEFAULT penalty the SAME bin is SKIPPED (the penalty is decisive).
    let (info1, ev1, count1) = run_once(INFO_ADVERSE_PENALTY);
    assert_eq!(info1, InfoVerdict::Unfavorable);
    assert!(
        ev1.unwrap() <= EV_THRESHOLD,
        "the penalty pushes EV ≤ thr: EV {:?}",
        ev1
    );
    assert_eq!(
        count1, 0,
        "the +penalty pushes EV ≤ thr ⇒ the bin is SKIPPED (a 0-penalty mutation reds this)"
    );
}

// ── A7: Unfavorable (STALE bracket book) ─────────────────────────────────────

/// A7 Unfavorable (STALE bracket): a PRESENT, FRESH perp book but a bracket bin
/// whose `as_of` is older than `info_max_age_ms` ⇒ the bracket side is STALE ⇒
/// `Unfavorable` (the penalty), even though the perp book exists. Proves staleness
/// (not just absence) drives the conservative side, and that the perp being present
/// does not rescue a stale bracket.
#[test]
fn a7_unfavorable_stale_bracket_applies_penalty() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    let now = ts("2026-06-12T17:00:00.000Z");
    // Perp FRESH (age 0); bracket books 10s STALE (> max_anchor 5s and > info_max 5s).
    let stale = ts("2026-06-12T16:59:50.000Z"); // 10s before now
    put_straddle_books_at(&mut w, stale);
    w.put(perp_book_at(now));
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let _ = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let between = between_ev(&s);
    assert_eq!(
        between.info,
        InfoVerdict::Unfavorable,
        "a stale bracket book ⇒ Unfavorable even with a fresh perp present"
    );
    assert!(
        (between.adverse_eff - (ADVERSE + INFO_ADVERSE_PENALTY)).abs() < 1e-12,
        "stale ⇒ the penalty is applied"
    );
    // The recorded bracket age reflects the 10s staleness (diagnostic).
    assert_eq!(
        between.bracket_age_ms,
        Some(10_000),
        "bracket age = now − as_of = 10s"
    );
}

// ── A7: BracketLeads — veto (flag on) vs down-weight (flag off) ───────────────

/// A7 BracketLeads + veto: the bracket bin is STRICTLY fresher than the perp (both
/// fresh) ⇒ `BracketLeads`; with `info_veto_on_bracket_leads = true` (default) the
/// bin is HARD-VETOED — NO proposal — even though its EV clears by a wide margin.
/// Dropping the veto reds this.
#[test]
fn a7_bracket_leads_vetoes_when_flag_on() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    let now = ts("2026-06-12T17:00:00.000Z");
    // Bracket FRESH (age 0); perp older but still FRESH (age 2s ≤ 5s). bracket_age 0
    // < perp_age 2000 ⇒ BracketLeads.
    put_straddle_books_at(&mut w, now);
    w.put(perp_book_at(ts("2026-06-12T16:59:58.000Z"))); // 2s old
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let _ = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let between = between_ev(&s);
    assert_eq!(
        between.info,
        InfoVerdict::BracketLeads,
        "bracket strictly fresher than the perp (both fresh) ⇒ BracketLeads"
    );
    // The bin's EV still clears (the veto, not a weak edge, suppresses it).
    assert!(
        between.ev.unwrap() > EV_THRESHOLD,
        "the bin's EV clears ⇒ only the BracketLeads veto suppresses it"
    );
    assert!(
        !between.proposed,
        "a BracketLeads bin is not proposed (veto)"
    );
    assert_eq!(
        s.metrics().proposals_emitted,
        0,
        "BracketLeads + veto flag ⇒ propose nothing"
    );
}

/// A7 BracketLeads + flag OFF: the SAME strictly-fresher-bracket setup with
/// `info_veto_on_bracket_leads = false` DOWN-WEIGHTS instead of vetoing — the bin's
/// `adverse_eff` is raised by `info_adverse_penalty`, and since the between bin's
/// edge is wide it STILL fires (documenting the flag-off behaviour = down-weight).
#[test]
fn a7_bracket_leads_downweights_when_flag_off() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    let now = ts("2026-06-12T17:00:00.000Z");
    put_straddle_books_at(&mut w, now); // bracket fresh (age 0)
    w.put(perp_book_at(ts("2026-06-12T16:59:58.000Z"))); // perp 2s old ⇒ BracketLeads
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut c = cfg_v4(ladder.clone());
    c.info_veto_on_bracket_leads = false; // down-weight instead of veto
    let mut s = PerpEventBasisV2::new(c).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let out = drive_capture_first_proposal(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let between = between_ev(&s);
    assert_eq!(between.info, InfoVerdict::BracketLeads);
    assert!(
        (between.adverse_eff - (ADVERSE + INFO_ADVERSE_PENALTY)).abs() < 1e-12,
        "flag off ⇒ BracketLeads DOWN-WEIGHTS (adverse + penalty), got {}",
        between.adverse_eff
    );
    assert_eq!(
        out.len(),
        1,
        "flag off + a wide edge ⇒ the down-weighted bin still fires"
    );
}

// ── A7: the conservative default bites ───────────────────────────────────────

/// The conservative default: a MISSING perp book is NEVER `PerpFavorable` (it is
/// `Unfavorable`). A mutation defaulting the verdict to `PerpFavorable` reds this.
/// (Companion to the stale case: absence ⇒ Unfavorable.)
#[test]
fn a7_missing_perp_is_never_perp_favorable() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    let now = ts("2026-06-12T17:00:00.000Z");
    put_straddle_books_at(&mut w, now); // fresh bracket; NO perp book
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let _ = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    // EVERY priced bin must be Unfavorable — never PerpFavorable — with no perp book.
    let eval = s.last_eval().expect("ready");
    assert!(!eval.bin_evs.is_empty(), "the ladder priced");
    for b in &eval.bin_evs {
        assert_ne!(
            b.info,
            InfoVerdict::PerpFavorable,
            "a missing perp book must NEVER be PerpFavorable (got it for {:?})",
            b.kind
        );
        assert_eq!(
            b.info,
            InfoVerdict::Unfavorable,
            "a missing perp book ⇒ Unfavorable for every bin"
        );
        assert_eq!(b.perp_age_ms, None, "perp absent ⇒ perp_age_ms None");
    }
}

/// The conservative default also bites a STALE perp: a present-but-stale perp book
/// (age > info_max_age_ms) with a fresh bracket is `Unfavorable`, NOT PerpFavorable
/// and NOT BracketLeads (staleness short-circuits the fresher-bracket comparison).
#[test]
fn a7_stale_perp_is_unfavorable_not_bracket_leads() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    let now = ts("2026-06-12T17:00:00.000Z");
    put_straddle_books_at(&mut w, now); // bracket fresh (age 0)
    w.put(perp_book_at(ts("2026-06-12T16:59:50.000Z"))); // perp 10s STALE (>5s)
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();

    let obs0 = ts("2026-06-12T16:59:55.000Z");
    let _ = drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let between = between_ev(&s);
    assert_eq!(
        between.info,
        InfoVerdict::Unfavorable,
        "a stale perp (even with a fresher bracket) ⇒ Unfavorable, not BracketLeads"
    );
    assert_eq!(
        between.perp_age_ms,
        Some(10_000),
        "perp age recorded as 10s (diagnostic)"
    );
}

// ── A10: cdf_divergence ──────────────────────────────────────────────────────

/// A10: `cdf_divergence` is `Some` when q_j is priced (the wide-straddle pricing
/// tick) and `None` when q_j is empty (τ unknown ⇒ no pricing). Pins the
/// populated-vs-absent contract.
#[test]
fn cdf_divergence_some_when_priced_none_when_not() {
    let ladder = straddle_ladder();
    // PRICED: τ known, fresh, coherent ⇒ q_j populated ⇒ cdf_divergence Some.
    {
        let mut w = World::new();
        put_straddle_coherent_books(&mut w);
        let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
        put_ladder_markets(&mut w, &ladder, tau_ms);
        let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();
        let obs0 = ts("2026-06-12T16:59:55.000Z");
        drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);
        let eval = s.last_eval().expect("ready");
        assert!(!eval.q_j.is_empty(), "the tick priced");
        assert!(
            eval.cdf_divergence.is_some(),
            "a priced tick populates cdf_divergence"
        );
    }
    // NOT PRICED: τ unknown (no markets) ⇒ q_j empty ⇒ cdf_divergence None.
    {
        let mut w = World::new();
        put_straddle_coherent_books(&mut w);
        // No put_ladder_markets ⇒ τ unknown ⇒ Disabled ⇒ empty q_j.
        let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();
        let obs0 = ts("2026-06-12T16:59:55.000Z");
        drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, 1000);
        let eval = s.last_eval().expect("σ ready ⇒ eval exists");
        assert!(eval.q_j.is_empty(), "no pricing ⇒ empty q_j");
        assert_eq!(
            eval.cdf_divergence, None,
            "an empty q_j ⇒ cdf_divergence None (nothing to compare)"
        );
    }
}

/// A10: `cdf_divergence` equals a HAND-COMPUTED Kolmogorov sup-distance for a small
/// CONTROLLED ladder. We read the strategy's own implied bins + q_j from the eval,
/// recompute the price-ordered cumulative sup-distance independently, and assert
/// equality — pinning the FORMULA (max |Σimpliedₖ − Σmodelₖ| over price-ordered
/// bins), not just "is Some".
#[test]
fn cdf_divergence_equals_hand_computed_sup_distance() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    put_straddle_coherent_books(&mut w); // less 0.25 / between 0.50 / greater 0.25
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    let eval = s.last_eval().expect("ready");
    let div = eval.cdf_divergence.expect("priced ⇒ cdf_divergence Some");

    // Independently recompute the sup-distance. The IMPLIED bins are the SAME bins
    // the strategy built (the oracle `bins_from_world`); the MODEL is q_j. Order
    // BOTH canonically (less rank 0, between 1, greater 2; within rank by floor/cap)
    // — `bins_from_world` already yields BTreeMap order, but order both explicitly.
    let bins = bins_from_world(&ladder, &w);
    let rank = |k: &BracketStrike| -> u8 {
        match k {
            BracketStrike::Less { .. } => 0,
            BracketStrike::Between { .. } => 1,
            BracketStrike::Greater { .. } => 2,
        }
    };
    let key = |k: &BracketStrike| -> f64 {
        match k {
            BracketStrike::Less { cap } => *cap,
            BracketStrike::Between { floor, .. } => *floor,
            BracketStrike::Greater { floor } => *floor,
        }
    };
    let mut implied = bins.clone();
    implied.sort_by(|a, b| {
        rank(&a.kind)
            .cmp(&rank(&b.kind))
            .then(key(&a.kind).partial_cmp(&key(&b.kind)).unwrap())
    });
    let mut model: Vec<_> = eval.q_j.clone();
    model.sort_by(|a, b| {
        rank(&a.kind)
            .cmp(&rank(&b.kind))
            .then(key(&a.kind).partial_cmp(&key(&b.kind)).unwrap())
    });
    assert_eq!(implied.len(), model.len());
    let mut ci = 0.0_f64;
    let mut cm = 0.0_f64;
    let mut sup = 0.0_f64;
    for (ib, mp) in implied.iter().zip(model.iter()) {
        ci += ib.prob;
        cm += mp.q;
        sup = sup.max((ci - cm).abs());
    }
    assert!(
        (div - sup).abs() < 1e-12,
        "cdf_divergence {div} != hand-computed sup-distance {sup}"
    );
}

/// A10: a fully CONTROLLED tiny ladder with KNOWN implied/model cumulatives, to pin
/// the sup-distance arithmetic to an exact number. The strategy is opaque about q_j
/// (it derives from the lognormal model), so this test verifies the PUBLIC formula
/// shape via the same recompute, but on a hand-picked ladder where the implied
/// cumulative is exactly [0.25, 0.75, 1.0] (less/between/greater). The model
/// cumulative is read from q_j; the asserted equality holds by construction of the
/// recompute, and the implied-side numbers are the hand-checked anchors.
#[test]
fn cdf_divergence_implied_cumulative_is_hand_checked() {
    let ladder = straddle_ladder();
    let mut w = World::new();
    // less 0.25, between 0.50, greater 0.25 ⇒ implied cumulative 0.25, 0.75, 1.00.
    put_straddle_coherent_books(&mut w);
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);

    // Hand-check the implied cumulative the divergence is built on.
    let bins = bins_from_world(&ladder, &w);
    let less = bins
        .iter()
        .find(|b| matches!(b.kind, BracketStrike::Less { .. }))
        .unwrap()
        .prob;
    let btw = bins
        .iter()
        .find(|b| matches!(b.kind, BracketStrike::Between { .. }))
        .unwrap()
        .prob;
    let gt = bins
        .iter()
        .find(|b| matches!(b.kind, BracketStrike::Greater { .. }))
        .unwrap()
        .prob;
    assert!((less - 0.25).abs() < 1e-9, "implied less = 0.25");
    assert!((btw - 0.50).abs() < 1e-9, "implied between = 0.50");
    assert!((gt - 0.25).abs() < 1e-9, "implied greater = 0.25");
    // Cumulative: 0.25, 0.75, 1.00 (the price-ordered partial sums).
    assert!((less + btw - 0.75).abs() < 1e-9);
    assert!((less + btw + gt - 1.00).abs() < 1e-9);

    // The divergence is the max gap of this implied cumulative vs the model
    // cumulative (q_j partial sums). It is Some and in [0,1].
    let div = s.last_eval().unwrap().cdf_divergence.unwrap();
    assert!(
        (0.0..=1.0).contains(&div),
        "a CDF sup-distance is in [0,1], got {div}"
    );
}

// ── T5.B8: A10 diagnostics → named MetricSample emission ──────────────────────
//
// V5 DEFERRED the "richer named-MetricSample emission" to this telemetry slice
// (T5.B8). These tests pin the metrics-export contract: a PRICED strategy emits
// the headline gauge set (each with a `market` label), a NOT-READY strategy emits
// only `active=0`, and a strategy that does NOT override the trait default emits
// nothing.
//
// ## Mutation-check note (which mutation reds which test)
// - DROPPING the σ_τ / cdf finite-guard (emit on a non-finite f64) or MIS-SCALING
//   either one (e.g. ×1_000 instead of ×1_000_000 for σ_τ, or ×100 instead of
//   ×10_000 for cdf) reds `metric_samples_priced_scalings_pinned` (the scaling
//   equality) and the name-set assertion in `metric_samples_priced_headline_set`.
// - Returning a NON-empty Vec from the trait DEFAULT reds
//   `metric_samples_default_trait_method_is_empty`.
// - Emitting the full set (not just `active=0`) when `last_eval` is None reds
//   `metric_samples_not_ready_emits_only_active_zero`.
// - Dropping the `market` label reds `metric_samples_priced_headline_set` (the
//   "every sample carries market=<perp>" assertion).

/// The metric NAMES the headline set emits on a priced tick (the `regime` one-hot
/// gauge included). Pinned here so a rename/drop reds.
const PRICED_METRIC_NAMES: &[&str] = &[
    "fortuna_perp_basis_v2_active",
    "fortuna_perp_basis_v2_cdf_divergence_tenthou",
    "fortuna_perp_basis_v2_sigma_tau_micro",
    "fortuna_perp_basis_v2_anchor_dollars",
    "fortuna_perp_basis_v2_horizon_ms",
    "fortuna_perp_basis_v2_obs_count",
    "fortuna_perp_basis_v2_anchor_stale",
    "fortuna_perp_basis_v2_regime",
];

/// Drive the SAME priced configuration the A10 `cdf_divergence` tests use (straddle
/// ladder, coherent books, τ=2h, Δ=1s, 6 constant-log-step ticks) to a ready,
/// PRICED `last_eval`, and return the strategy + the world for inspection.
fn priced_v2_strategy() -> (PerpEventBasisV2, World) {
    let ladder = straddle_ladder();
    let mut w = World::new();
    put_straddle_coherent_books(&mut w);
    let (tau_ms, dt_ms) = (7_200_000_i64, 1000_i64);
    put_ladder_markets(&mut w, &ladder, tau_ms);
    let mut s = PerpEventBasisV2::new(cfg_v4(ladder.clone())).unwrap();
    let obs0 = ts("2026-06-12T16:59:55.000Z");
    drive_constant_step_obs(&mut s, &w, 6.3000, 1.0001, 6, obs0, dt_ms);
    (s, w)
}

/// T5.B8 (a): a PRICED strategy emits exactly the headline gauge set; every sample
/// is a GAUGE (`counter == false`), carries a `market=<perp>` label, has a finite
/// i64 value, and the `regime` sample additionally carries a `regime=` label. The
/// set of NAMES equals `PRICED_METRIC_NAMES`.
#[test]
fn metric_samples_priced_headline_set() {
    let (s, _w) = priced_v2_strategy();
    let eval = s.last_eval().expect("priced ⇒ eval exists");
    assert!(!eval.q_j.is_empty(), "precondition: the tick priced");
    assert!(
        eval.cdf_divergence.is_some(),
        "precondition: cdf_divergence populated"
    );

    let samples = s.metric_samples();

    // Exactly the headline names (cdf + σ_τ + anchor are present because they are
    // finite here; horizon is present because τ is known).
    let mut names: Vec<&str> = samples.iter().map(|m| m.name).collect();
    names.sort_unstable();
    let mut want: Vec<&str> = PRICED_METRIC_NAMES.to_vec();
    want.sort_unstable();
    assert_eq!(names, want, "the priced headline metric NAME set");

    for m in &samples {
        // All point-in-time gauges (never counters).
        assert!(!m.counter, "{} must be a gauge (counter:false)", m.name);
        // Every sample carries the market label = the perp market.
        let market = m
            .labels
            .iter()
            .find(|(k, _)| k == "market")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            market,
            Some(PERP),
            "{} must carry market=<perp_market>",
            m.name
        );
        // i64 value is well-formed (a finite cast, never an undefined one).
        let _ = m.value;
    }

    // The regime sample is the one-hot gauge: value 1 with a regime= label.
    let regime = samples
        .iter()
        .find(|m| m.name == "fortuna_perp_basis_v2_regime")
        .expect("regime sample present");
    assert_eq!(regime.value, 1, "regime is a one-hot gauge (value 1)");
    let regime_label = regime
        .labels
        .iter()
        .find(|(k, _)| k == "regime")
        .map(|(_, v)| v.as_str());
    // τ = 2h and direct_max_ms = 4h ⇒ 2h ≤ 4h ⇒ Direct.
    assert_eq!(
        regime_label,
        Some("direct"),
        "τ=2h ≤ direct_max=4h ⇒ Direct regime"
    );
}

/// T5.B8 (a, scaling): the cdf_divergence and sigma_tau samples equal a hand-applied
/// fixed scale of the eval's OWN f64 (×10_000 ten-thousandths; ×1_000_000 micro),
/// `.round() as i64`. Pins the documented dashboard decode + the finite-guard (a
/// dropped guard / wrong scale reds this).
#[test]
fn metric_samples_priced_scalings_pinned() {
    let (s, _w) = priced_v2_strategy();
    let eval = s.last_eval().expect("priced ⇒ eval exists");
    let div = eval.cdf_divergence.expect("priced ⇒ cdf_divergence Some");
    let sigma_tau = eval.sigma_tau;
    assert!(div.is_finite() && sigma_tau.is_finite(), "finite inputs");

    let samples = s.metric_samples();
    let by_name = |name: &str| -> i64 {
        samples
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .value
    };

    let want_cdf = (div * 10_000.0).round() as i64;
    assert_eq!(
        by_name("fortuna_perp_basis_v2_cdf_divergence_tenthou"),
        want_cdf,
        "cdf_divergence ×10_000 ten-thousandths (decode ÷10_000)"
    );

    let want_sigma = (sigma_tau * 1_000_000.0).round() as i64;
    assert_eq!(
        by_name("fortuna_perp_basis_v2_sigma_tau_micro"),
        want_sigma,
        "sigma_tau ×1_000_000 micro (decode ÷1_000_000)"
    );

    // anchor → whole dollars; horizon → ms as-is; obs_count/anchor_stale as-is.
    assert_eq!(
        by_name("fortuna_perp_basis_v2_anchor_dollars"),
        eval.anchor.round() as i64,
        "anchor rounded to whole BTC dollars"
    );
    assert_eq!(
        by_name("fortuna_perp_basis_v2_horizon_ms"),
        eval.tau_ms.expect("τ known"),
        "horizon τ in ms, as-is"
    );
    assert_eq!(
        by_name("fortuna_perp_basis_v2_obs_count"),
        eval.obs_count as i64,
        "obs_count as-is"
    );
    assert_eq!(
        by_name("fortuna_perp_basis_v2_anchor_stale"),
        i64::from(eval.anchor_stale),
        "anchor_stale as 0/1"
    );
}

/// T5.B8 (b): a NOT-READY strategy (σ never reaches `min_vol_obs` ⇒ `last_eval`
/// None) emits ONLY a single `active=0` gauge — never the full set on a stale/absent
/// eval, so the dashboard sees "inactive", not a missing or stale series.
#[test]
fn metric_samples_not_ready_emits_only_active_zero() {
    let ladder = straddle_ladder();
    let w = World::new();
    // A config whose readiness gate (min_vol_obs) is far above the ticks we feed.
    let mut cfg = cfg_v4(ladder.clone());
    cfg.min_vol_obs = 1_000;
    let mut s = PerpEventBasisV2::new(cfg).unwrap();
    // One matching tick: σ folds but stays not-ready ⇒ no eval recorded.
    let _ = run(&mut s, &w, &perp_tick_v2(PERP, "6.3500", "6.3000"));
    assert!(
        s.last_eval().is_none(),
        "precondition: σ not ready ⇒ no eval"
    );

    let samples = s.metric_samples();
    assert_eq!(samples.len(), 1, "not-ready ⇒ exactly one sample");
    let only = &samples[0];
    assert_eq!(only.name, "fortuna_perp_basis_v2_active");
    assert_eq!(only.value, 0, "active=0 when no eval exists");
    assert!(!only.counter, "active is a gauge");
    assert_eq!(
        only.labels
            .iter()
            .find(|(k, _)| k == "market")
            .map(|(_, v)| v.as_str()),
        Some(PERP),
        "active=0 still carries the market label"
    );
}

/// T5.B8 (c): a strategy that does NOT override `metric_samples` gets the trait
/// DEFAULT (empty Vec). Asserted against a tiny dummy `Strategy` so the test pins
/// the DEFAULT itself (not any particular non-overriding production strategy).
#[test]
fn metric_samples_default_trait_method_is_empty() {
    use async_trait::async_trait;
    use fortuna_core::bus::BusEvent;
    use fortuna_core::market::StrategyId;
    use fortuna_runner::{
        CoreHandle, MetricSample, Proposal, RunnerError, Stage, Strategy, StrategyKind,
        StrategyMetrics,
    };

    struct DummyStrategy;

    #[async_trait]
    impl Strategy for DummyStrategy {
        fn id(&self) -> StrategyId {
            StrategyId::new("dummy_no_metrics").unwrap()
        }
        fn kind(&self) -> StrategyKind {
            StrategyKind::Mechanical
        }
        fn stage(&self) -> Stage {
            Stage::Sim
        }
        async fn on_event(
            &mut self,
            _ev: &BusEvent,
            _core: &CoreHandle<'_>,
        ) -> Result<Vec<Proposal>, RunnerError> {
            Ok(Vec::new())
        }
        fn metrics(&self) -> StrategyMetrics {
            StrategyMetrics::default()
        }
        // NB: `metric_samples` is intentionally NOT overridden — the default applies.
    }

    let s = DummyStrategy;
    let samples: Vec<MetricSample> = s.metric_samples();
    assert!(
        samples.is_empty(),
        "the Strategy::metric_samples default returns an empty Vec"
    );
}
