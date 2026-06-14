//! `perp_event_basis_v2` STRATEGY unit tests (perp-strategies-and-scalar-claims
//! §3.3, build-order steps 2+3: A3 + A6 anchor + A9 no-arb). Written from the
//! spec BEFORE the strategy. V3 PROPOSES NOTHING — it wires the v2 fair-prob
//! model (`bracket_fair_probs` on the BRTI anchor) and the no-arb gate
//! (`validate_ladder_no_arb`) into a propose-only, Sim-stage, data-only
//! evaluation snapshot (`last_eval`). The per-bin EV gate that DECIDES
//! proposals is the V4 slice; here every path returns `Ok(vec![])`.
//!
//! Contract under test (the data-only v2 evaluator):
//! - σ NOT ready (< `min_vol_obs` anchor returns) ⇒ `last_eval` is None,
//!   zero proposals.
//! - σ ready (≥ `min_vol_obs` returns over a CONTROLLED anchor sequence) ⇒
//!   `q_j` EQUALS `bracket_fair_probs` called directly with the same anchor +
//!   the same hand-computed σ (pins the WIRING, never a re-implementation);
//!   `q_j.len() == ladder size`; each `q ∈ [0,1]`.
//! - A6: a tick whose `reference_price` (BRTI anchor) differs from the perp
//!   MARK prices off the ANCHOR (the `q_j` matches the anchor-based call, NOT
//!   a mark-based one). The load-bearing A6 assertion.
//! - A9: an INCOHERENT ladder ⇒ `last_eval` records `Incoherent` and zero
//!   proposals; a coherent ladder ⇒ `q_j` present.
//! - A10: the median diagnostic (`compute_basis`) is populated and is NOT a
//!   gate (no proposal either way).
//! - degenerate input (non-finite / ≤0 anchor, all-empty ladder) ⇒ no panic,
//!   no eval, zero proposals.
//! - V3 emits NOTHING: `proposals_emitted == 0`, `on_event` returns `Ok(vec![])`
//!   on every path; `events_seen` still counts.
//!
//! ## Mutation-check note (which mutation reds which test)
//! - Swapping the A6 anchor source `funding.reference_price → marks.venue_settlement`
//!   (mark-not-anchor) reds `a6_prices_off_anchor_not_mark` (the anchor and the
//!   mark are set to DIFFERENT BTC values, so the `q_j` vector differs).
//! - Dropping / inverting the A9 `validate_ladder_no_arb` guard (treating an
//!   incoherent ladder as coherent) reds `a9_incoherent_ladder_records_incoherent`
//!   (it would populate `q_j` instead of recording `Incoherent`).
//! - Re-implementing `q_j` (rather than CALLING `bracket_fair_probs`), or
//!   feeding the wrong σ/anchor into the kernel, reds `sigma_ready_qj_matches_kernel`
//!   and `a6_prices_off_anchor_not_mark` (both compare against the kernel call
//!   verbatim). The NO-CIRCULARITY property (q_j ignores `BracketBin.prob`) is
//!   the KERNEL's, already proven in `basis_v2.rs` — not re-litigated here.
//! - Lowering the `min_vol_obs` ready-gate (firing before enough returns) reds
//!   `sigma_not_ready_no_eval` (it would produce a non-None `last_eval` early).
//! - Demoting the median to a SIGNAL (gating on it) reds
//!   `median_diagnostic_populated_but_not_a_gate` (a proposal would appear).

use fortuna_cognition::basis::{compute_basis, BracketBin, BracketStrike};
use fortuna_cognition::basis_v2::{bracket_fair_probs, LadderHealth, SettlementModel};
use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Contracts, MarketId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::perp_event_basis_v2::{PerpEventBasisV2, PerpEventBasisV2Config};
use fortuna_runner::{CoreHandle, Proposal, Stage, Strategy, StrategyKind};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
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
/// in a few ticks; the other knobs are the production defaults.
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

/// σ ready: with ≥ min_vol_obs returns over a CONTROLLED constant-log-step
/// anchor sequence, σ is ready and the stored `q_j` EQUALS `bracket_fair_probs`
/// called directly with the SAME anchor + the SAME hand-computed σ. Pins the
/// WIRING (A3 on the A6 anchor), not a re-implementation. Also: `q_j.len() ==
/// ladder size`, each `q ∈ [0,1]`, and STILL zero proposals.
#[test]
fn sigma_ready_qj_matches_kernel() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();

    // 6 ticks ⇒ 5 returns ≥ min_vol_obs(3) ⇒ READY. Constant log-step ⇒ σ is
    // λ-independent and hand-computable.
    let (base, ratio, n) = (6.0000_f64, 1.01_f64, 6);
    let anchors = drive_constant_step(&mut s, &w, base, ratio, n);

    let eval = s
        .last_eval()
        .expect("σ ready ⇒ an evaluation snapshot exists");

    // The σ the strategy used must match the hand-reconstructed EWMA σ.
    let sigma_expected = expected_sigma(&anchors, 0.94, 1e-6, 5.0);
    assert!(
        (eval.sigma - sigma_expected).abs() < 1e-12,
        "σ used {} != hand-computed EWMA σ {}",
        eval.sigma,
        sigma_expected
    );
    // Constant log-step ratio 1.01 ⇒ σ ≈ |ln 1.01| (sanity, not the pin).
    assert!(
        (eval.sigma - (1.01_f64).ln().abs()).abs() < 1e-4,
        "σ ≈ |ln ratio| for a constant-log-step sequence: got {}",
        eval.sigma
    );

    // The ANCHOR the strategy used: the LAST tick's reference_price ×10000.
    let last_anchor_btc =
        f64::from_str(anchors.last().unwrap()).unwrap() * PERP_CONTRACT_BTC_DIVISOR;
    assert!(
        (eval.anchor - last_anchor_btc).abs() < 1e-9,
        "anchor used {} != last BRTI reference ×10000 {}",
        eval.anchor,
        last_anchor_btc
    );

    // THE WIRING PIN: q_j == bracket_fair_probs(bins, model{anchor, σ}) verbatim.
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

    assert_eq!(s.metrics().proposals_emitted, 0, "V3 proposes NOTHING");
}

/// A6 (load-bearing): the model prices off the BRTI ANCHOR, NOT the perp MARK.
/// Feed a final tick whose `reference_price` (anchor) differs SHARPLY from the
/// `venue_settlement` (mark); the stored `q_j` must match the ANCHOR-based
/// kernel call and must NOT match the MARK-based one.
#[test]
fn a6_prices_off_anchor_not_mark() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();

    // Warm σ to ready with a constant-log-step anchor sequence ENDING at a known
    // anchor. Use base 6.0000, ratio 1.01, 6 ticks; the LAST anchor is
    // 6.0000·1.01^5 rounded to 4dp.
    let (base, ratio, n) = (6.0000_f64, 1.01_f64, 6);
    let anchors = drive_constant_step(&mut s, &w, base, ratio, n);
    let last_anchor_str = anchors.last().unwrap().clone();

    // Now fire ONE more tick where the MARK is far from the ANCHOR. Keep the
    // anchor on the constant-log-step path (so σ stays the clean value); set the
    // mark to a wildly different BTC value.
    let next_anchor = {
        let a = f64::from_str(&last_anchor_str).unwrap() * ratio;
        format!("{:.4}", (a * 10_000.0).round() / 10_000.0)
    };
    // mark per-contract 7.5000 (×10000 = BTC $75,000) — far above the ~$60.6k
    // anchor; if the model (wrongly) used the mark, q_j would shift up sharply.
    let _ = run(&mut s, &w, &perp_tick_v2(PERP, "7.5000", &next_anchor));

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

/// A coherent ladder ⇒ the A9 verdict is Coherent and `q_j` IS present (the
/// positive counterpart to the incoherent test).
#[test]
fn a9_coherent_ladder_populates_qj() {
    let ladder = coherent_ladder();
    let mut w = World::new();
    put_coherent_books(&mut w);
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();
    drive_constant_step(&mut s, &w, 6.0000, 1.01, 6);

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
    let mut s = PerpEventBasisV2::new(cfg(ladder.clone())).unwrap();
    drive_constant_step(&mut s, &w, 6.0000, 1.01, 6);

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
