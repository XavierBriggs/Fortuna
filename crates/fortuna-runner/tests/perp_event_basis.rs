//! `perp_event_basis` STRATEGY unit + live-fixture tests (perp-strategies-and-
//! scalar-claims §3/§3.1/§7; GAPS "TRACK C — slice 3b"). Written from the spec
//! BEFORE the strategy.
//!
//! Contract under test (the propose-only mechanical basis strategy):
//! - A tradeable basis → EXACTLY ONE `Passive`/`Buy`/`Yes` proposal on the bin
//!   CONTAINING the perp forecast; `limit == best yes_bid`; `fair == limit +
//!   premium` (clamped ≤ 99); `calibrated_p == None`; UNSIZED (no qty field).
//! - A non-tradeable basis, a `None` median, a foreign-market `PerpTick`, an
//!   empty target bid, or a degenerate fair → ZERO proposals.
//! - The fee-trap is a STRICT `>` at `fee_floor + min_basis` (kernel boundary).
//! - Dedup: the identical (target, limit) leg fires once; a move re-proposes.
//! - DETERMINISM: identical inputs twice → identical `Vec<Proposal>`.
//! - `target_market` rung-0 selection: between-containment, the open tails, and
//!   the none-contains case.
//! - LIVE e2e: the committed paired-cycle recording produces one proposal on
//!   the `$63,500–63,999.99` bin (the bin containing the $63,906 perp mark).

use fortuna_cognition::basis::BracketStrike;
use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::perp_event_basis::{PerpEventBasis, PerpEventBasisConfig};
use fortuna_runner::{CoreHandle, Proposal, Strategy, Urgency};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

// ── harness ──────────────────────────────────────────────────────────────────

const PERP: &str = "KXBTCPERP";

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

/// A book with a single (bid, ask) YES level. `qty` is fixed; the strategy
/// reads price only (it is unsized). Prices are in cents.
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

/// A book that is bid-empty (one ask level, no bids): the strategy cannot JOIN a
/// bid here (so it is never a tradeable TARGET), but per the YES-mid convention
/// the bin still carries `ask/2` implied mass for the median (the live far-OTM
/// case — `0 bid / Nc ask`).
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

/// A book with NO quote on EITHER side: the only shape that carries zero implied
/// mass (a one-sided book carries `present_side/2`). Used to build an all-zero
/// ladder (→ `None` median).
fn book_empty(market: &str) -> OrderBook {
    OrderBook {
        market: mkt(market),
        as_of: ts("2026-06-12T17:00:00.000Z"),
        yes_bids: vec![],
        yes_asks: vec![],
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

/// A `PerpTick` for `perp_market` whose settlement mark is `per_contract`
/// dollars (the strategy lifts it ×10000 to BTC dollars). `reference_price` and
/// `funding` carry plausible, unused-by-the-strategy values.
fn perp_tick(perp_market: &str, per_contract: &str) -> BusEvent {
    let marks = PerpMarks {
        venue_settlement: PerpPrice::from_dollars_exact(Decimal::from_str(per_contract).unwrap())
            .unwrap(),
        conservative: None,
    };
    let funding = FundingObservation {
        estimate: Decimal::from_str("0.0005").unwrap(),
        next_funding_time: ts("2026-06-12T20:00:00.000Z"),
        reference_price: PerpPrice::from_dollars_exact(Decimal::from_str("6.3000").unwrap())
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

use rust_decimal::Decimal;

fn run(s: &mut PerpEventBasis, w: &World, ev: &BusEvent) -> Vec<Proposal> {
    futures::executor::block_on(s.on_event(ev, &w.handle())).unwrap()
}

/// A small three-bin catalog around $63,906: a `less` bottom tail (≤ $60k), a
/// `between` $63,500–63,999.99 (the bin that contains the live mark), and a
/// `greater` top tail (≥ $70k). Returned with the bracket ids so tests can
/// reference the expected target.
fn three_bin_ladder() -> (BTreeMap<MarketId, BracketStrike>, String, String, String) {
    let less_id = "KXBTC-LESS60K".to_string();
    let between_id = "KXBTC-B63750".to_string();
    let greater_id = "KXBTC-GT70K".to_string();
    let mut ladder = BTreeMap::new();
    ladder.insert(mkt(&less_id), BracketStrike::Less { cap: 60_000.0 });
    ladder.insert(
        mkt(&between_id),
        BracketStrike::Between {
            floor: 63_500.0,
            cap: 63_999.99,
        },
    );
    ladder.insert(mkt(&greater_id), BracketStrike::Greater { floor: 70_000.0 });
    (ladder, less_id, between_id, greater_id)
}

/// Default tradeable config: fee_floor $10 + min_basis $5 ⇒ tradeable needs
/// |basis| > $15; premium 2c.
fn cfg(ladder: BTreeMap<MarketId, BracketStrike>) -> PerpEventBasisConfig {
    PerpEventBasisConfig {
        perp_market: mkt(PERP),
        ladder,
        fee_floor_dollars: 10.0,
        min_basis_dollars: 5.0,
        edge_premium_cents: 2,
    }
}

/// Populate `w` with books whose implied median lands INSIDE the `between` bin
/// but well BELOW the perp mark ⇒ a tradeable POSITIVE basis on the between bin
/// (the target). A small `less`-tail mass plus a mid-priced `between` bin puts
/// the 0.5 crossing near the lower part of the between range (median ≈ $63,742,
/// basis ≈ +$164 > $15). The between bin's best bid is 60c (the join the leg
/// tests). The `greater` tail is empty (prob 0).
///
/// (Note: mass cannot be parked in the `less` OPEN tail past 0.5 — that yields
/// a None median; the crossing must land in a finite `between` bin.)
fn books_tradeable_target_between(w: &mut World, less: &str, between: &str, greater: &str) {
    w.put(book(less, 1, 3)); // prob ≈ 0.02 (tiny bottom-tail mass)
    w.put(book(between, 60, 62)); // prob ≈ 0.61, best bid 60c (the join)
    w.put(book(greater, 0, 2)); // bid-empty ⇒ prob 0
}

// ── unit tests (each names the mutation it pins) ─────────────────────────────

/// 1. A tradeable basis → exactly ONE Passive/Buy/Yes proposal on the bin
///    CONTAINING the perp forecast; limit == best yes_bid; fair == limit +
///    premium (clamped ≤ 99); calibrated_p None; UNSIZED (no qty field exists).
#[test]
fn tradeable_basis_emits_one_passive_buy_on_the_containing_bin() {
    let (ladder, less, between, greater) = three_bin_ladder();
    let mut w = World::new();
    // The implied median lands inside the between bin (≈ $63,742) but well below
    // the perp $63,906 ⇒ a +$164 basis > $15 ⇒ tradeable. Target = the between
    // bin that contains $63,906; its best bid is 60c (the join).
    books_tradeable_target_between(&mut w, &less, &between, &greater);
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();

    let props = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert_eq!(props.len(), 1, "exactly one proposal on a tradeable basis");
    let p = &props[0];
    assert_eq!(p.urgency, Urgency::Passive, "maker-only");
    assert!(p.group_policy.is_none(), "single-leg: no group policy");
    assert_eq!(p.legs.len(), 1, "exactly one leg");
    let leg = &p.legs[0];
    assert_eq!(
        leg.market,
        mkt(&between),
        "target is the containing between bin"
    );
    assert_eq!(leg.side, Side::Yes);
    assert_eq!(leg.action, Action::Buy);
    // limit == best yes_bid of the between bin (60c here).
    assert_eq!(
        leg.limit_price,
        Cents::new(60),
        "limit joins the bin's best bid"
    );
    // fair == limit + premium (60 + 2 = 62), under the 99 clamp.
    assert_eq!(leg.fair_value, Cents::new(62), "fair == limit + premium");
    assert_eq!(
        leg.calibrated_p, None,
        "mechanical leg carries no calibrated_p"
    );
    assert_eq!(s.metrics().proposals_emitted, 1);
}

/// 2. A non-tradeable basis (|basis| < fee_floor + min_basis) → zero proposals.
#[test]
fn non_tradeable_basis_emits_nothing() {
    // A between bin CENTERED on the perp mark ($63,806–64,006, center $63,906)
    // with all the mass ⇒ the median interpolates to exactly $63,906 ⇒ |basis|
    // ≈ 0 < $15 ⇒ NOT tradeable. (The default ladder's between bin centers on
    // $63,750 — $156 from the mark — which WOULD be tradeable, so this test
    // builds its own mark-centered bin.)
    let mut ladder = BTreeMap::new();
    ladder.insert(mkt("KXBTC-LESS60K"), BracketStrike::Less { cap: 60_000.0 });
    ladder.insert(
        mkt("KXBTC-B63906"),
        BracketStrike::Between {
            floor: 63_806.0,
            cap: 64_006.0,
        },
    );
    ladder.insert(
        mkt("KXBTC-GT70K"),
        BracketStrike::Greater { floor: 70_000.0 },
    );
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 0, 2)); // bid-empty ⇒ prob 0
    w.put(book("KXBTC-B63906", 95, 97)); // prob ≈ 0.96 (all the mass)
    w.put(book("KXBTC-GT70K", 0, 2)); // bid-empty ⇒ prob 0
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
    let props = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert!(props.is_empty(), "a sub-floor basis must not propose");
    assert_eq!(s.metrics().proposals_emitted, 0);
}

/// 3. A `None` median (all-zero-probability ladder) → zero proposals.
#[test]
fn none_median_emits_nothing() {
    let (ladder, less, between, greater) = three_bin_ladder();
    let mut w = World::new();
    // Every bin has NO quote on either side ⇒ prob 0 ⇒ sum_p == 0 ⇒ kernel None.
    // (A one-sided book would carry ask/2 mass, so a genuinely-empty book is the
    // only way to an all-zero ladder.)
    w.put(book_empty(&less));
    w.put(book_empty(&between));
    w.put(book_empty(&greater));
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
    let props = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert!(
        props.is_empty(),
        "a None median yields no signal, no proposal"
    );
}

/// 3b. A `None` median from a 0.5 crossing in an OPEN tail → zero proposals.
#[test]
fn none_median_open_tail_crossing_emits_nothing() {
    // A two-bin ladder where the cumulative 0.5 crossing lands in the open
    // `greater` tail (no finite width to interpolate ⇒ kernel None). Mass: a
    // tiny `less` tail + a dominant `greater` tail.
    let mut ladder = BTreeMap::new();
    ladder.insert(mkt("KXBTC-LESS60K"), BracketStrike::Less { cap: 60_000.0 });
    ladder.insert(
        mkt("KXBTC-GT70K"),
        BracketStrike::Greater { floor: 70_000.0 },
    );
    let mut w = World::new();
    w.put(book("KXBTC-LESS60K", 1, 3)); // prob ≈ 0.02
    w.put(book("KXBTC-GT70K", 95, 97)); // prob ≈ 0.96 ⇒ 0.5 crosses here
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
    let props = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert!(
        props.is_empty(),
        "open-tail crossing → None median → no proposal"
    );
}

/// 4. A `PerpTick` whose market != cfg.perp_market → zero proposals.
#[test]
fn foreign_perp_market_emits_nothing() {
    let (ladder, less, between, greater) = three_bin_ladder();
    let mut w = World::new();
    books_tradeable_target_between(&mut w, &less, &between, &greater);
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
    // A PerpTick for a DIFFERENT perp (would be tradeable if it matched).
    let props = run(&mut s, &w, &perp_tick("KXETHPERP", "6.3906"));
    assert!(props.is_empty(), "a non-matching perp market is ignored");
    assert_eq!(s.metrics().events_seen, 1, "the event was still counted");
}

/// 4b. A non-`PerpTick` event → zero proposals.
#[test]
fn non_perp_tick_event_emits_nothing() {
    let (ladder, less, between, greater) = three_bin_ladder();
    let mut w = World::new();
    books_tradeable_target_between(&mut w, &less, &between, &greater);
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
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
    assert!(props.is_empty(), "a non-PerpTick event is ignored");
}

/// 5. The target bin has empty yes_bids (cannot join) → zero proposals.
#[test]
fn target_bin_with_no_bid_emits_nothing() {
    let (ladder, less, between, greater) = three_bin_ladder();
    let mut w = World::new();
    // The median must LAND in the between target (a VALID, tradeable basis) so
    // the strategy reaches the join step — only then does the missing bid bite.
    // The between carries the dominant mass (ask 99 ⇒ prob ≈ 0.495) but has NO
    // bid; the tails are tiny. Median ≈ $63,745 (inside the between), perp
    // $63,906 ⇒ basis ≈ +$161 > $15 ⇒ tradeable, target = the between bin — but
    // it has no best bid to join.
    w.put(book(&less, 1, 3)); // prob ≈ 0.02
    w.put(book_no_bid(&between, 99)); // bid-empty TARGET, ask 99 ⇒ prob ≈ 0.495
    w.put(book(&greater, 0, 2)); // prob ≈ 0.01
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
    let props = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert!(
        props.is_empty(),
        "tradeable basis on the target, but no best bid to join ⇒ no proposal"
    );
}

/// 5b. The target bin has NO BOOK AT ALL (not in core.books) → zero proposals
///     (the bin still contributes prob 0.0 to the ladder; the leg cannot join).
#[test]
fn target_bin_with_no_book_emits_nothing() {
    let (ladder, less, _between, greater) = three_bin_ladder();
    let mut w = World::new();
    // less + greater have books (mass low ⇒ tradeable), but the between target
    // has NO book entry at all.
    w.put(book(&less, 95, 97));
    w.put(book(&greater, 0, 2));
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
    let props = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert!(
        props.is_empty(),
        "no book on the target ⇒ no join ⇒ no proposal"
    );
}

/// 6. Fee-trap STRICT boundary: |basis| exactly == fee_floor + min_basis →
///    NOT tradeable (zero proposals); just past → one proposal.
#[test]
fn fee_trap_is_strict_at_the_combined_floor() {
    // Construct a clean degenerate ladder whose median is an EXACT round number
    // so we can place the perp mark exactly `floor+margin` above it. A single
    // `between` $50,000–60,000 bin with all the mass: median interpolates to
    // its CENTER, $55,000 (cum reaches 0.5 at frac 0.5 ⇒ floor + 0.5*width).
    let mut ladder = BTreeMap::new();
    ladder.insert(
        mkt("KXBTC-B55K"),
        BracketStrike::Between {
            floor: 50_000.0,
            cap: 60_000.0,
        },
    );
    // Median is exactly $55,000. fee_floor 10 + min_basis 5 = 15 combined.
    // perp_btc = 55,015 ⇒ |basis| == 15.0 exactly ⇒ STRICT > fails ⇒ NOT
    // tradeable. perp_per_contract = 5.5015 (×10000 = 55,015).
    let mut w = World::new();
    w.put(book("KXBTC-B55K", 95, 97)); // mid 0.96, the only mass ⇒ median 55,000
    let at_floor_cfg = PerpEventBasisConfig {
        perp_market: mkt(PERP),
        ladder: ladder.clone(),
        fee_floor_dollars: 10.0,
        min_basis_dollars: 5.0,
        edge_premium_cents: 2,
    };
    let mut s_at = PerpEventBasis::new(at_floor_cfg).unwrap();
    let at = run(&mut s_at, &w, &perp_tick(PERP, "5.5015"));
    assert!(
        at.is_empty(),
        "|basis| exactly at the floor is NOT tradeable (strict >)"
    );

    // Just past: perp_btc = 55,016 ⇒ |basis| == 16.0 > 15 ⇒ tradeable ⇒ one
    // proposal (the perp mark $55,016 is inside the $50k–60k between bin).
    let just_past_cfg = PerpEventBasisConfig {
        perp_market: mkt(PERP),
        ladder,
        fee_floor_dollars: 10.0,
        min_basis_dollars: 5.0,
        edge_premium_cents: 2,
    };
    let mut s_past = PerpEventBasis::new(just_past_cfg).unwrap();
    let past = run(&mut s_past, &w, &perp_tick(PERP, "5.5016"));
    assert_eq!(
        past.len(),
        1,
        "just past the floor is tradeable (one proposal)"
    );
    assert_eq!(past[0].legs[0].market, mkt("KXBTC-B55K"));
}

/// 7. Dedup: the identical repeat tick → no second proposal; a tick that moves
///    the target/limit → a new proposal.
#[test]
fn dedup_identical_leg_then_reproposes_on_a_move() {
    let (ladder, less, between, greater) = three_bin_ladder();
    let mut w = World::new();
    books_tradeable_target_between(&mut w, &less, &between, &greater);
    let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();

    // First tick → one proposal (limit 60c on the between bin, the join).
    let first = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].legs[0].limit_price, Cents::new(60));

    // Identical repeat tick → deduped (same target + same limit).
    let again = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert!(again.is_empty(), "identical (target, limit) leg is deduped");

    // Move the target bin's best bid (60c → 4c): a NEW (target, limit) key ⇒ a
    // new proposal (the median stays in the between bin, basis ≈ +$256 > $15).
    w.put(book(&between, 4, 6));
    let moved = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert_eq!(moved.len(), 1, "a moved limit re-proposes");
    assert_eq!(moved[0].legs[0].limit_price, Cents::new(4));
    assert_eq!(
        moved[0].legs[0].fair_value,
        Cents::new(6),
        "fair tracks the new limit"
    );
}

/// 8. Fair clamp / no-edge: a premium that pushes fair past 99 clamps to 99; a
///    degenerate case where fair <= limit → zero proposals.
#[test]
fn fair_clamps_to_99_and_no_edge_emits_nothing() {
    // Clamp case: target bin best bid 98c, premium 5 ⇒ raw fair 103 → clamp 99.
    // Median must be LOW (so the basis is a big positive) while the target bin
    // (still the one containing the perp mark) trades at 98c. A near-1.0
    // probability bin at 98c pins the median INTO that bin, which would make the
    // basis ~0 — so instead use a SEPARATE low-mass less tail to drag the
    // median down while the between bin trades rich.
    //
    // Mass split: less tail ≈ 0.5 + between ≈ 0.5 so the cumulative 0.5 crossing
    // lands at the bottom of the between bin (median ≈ $63,500), perp at
    // $63,906 ⇒ basis ≈ +$406 > $15 ⇒ tradeable; the between bin's 98c bid is
    // the join.
    let (ladder, less, between, greater) = three_bin_ladder();
    let mut w = World::new();
    w.put(book(&less, 49, 51)); // prob ≈ 0.50
    w.put(book(&between, 98, 99)); // prob ≈ 0.985, best bid 98c (the join)
    w.put(book(&greater, 0, 2)); // prob ≈ 0.01
    let clamp_cfg = PerpEventBasisConfig {
        perp_market: mkt(PERP),
        ladder,
        fee_floor_dollars: 10.0,
        min_basis_dollars: 5.0,
        edge_premium_cents: 5,
    };
    let mut s = PerpEventBasis::new(clamp_cfg).unwrap();
    let props = run(&mut s, &w, &perp_tick(PERP, "6.3906"));
    assert_eq!(props.len(), 1, "still proposes with a clamped fair");
    assert_eq!(props[0].legs[0].limit_price, Cents::new(98));
    assert_eq!(
        props[0].legs[0].fair_value,
        Cents::new(99),
        "fair clamps to 99"
    );

    // No-edge case: target best bid 99c (max), premium 1 ⇒ fair = min(100,99) =
    // 99 == limit ⇒ fair <= limit ⇒ no proposal. Use a between bin at 99c.
    let (ladder2, less2, between2, greater2) = three_bin_ladder();
    let mut w2 = World::new();
    w2.put(book(&less2, 49, 51));
    w2.put(book(&between2, 99, 99)); // best bid 99c (degenerate top tick)
    w2.put(book(&greater2, 0, 2));
    let noedge_cfg = PerpEventBasisConfig {
        perp_market: mkt(PERP),
        ladder: ladder2,
        fee_floor_dollars: 10.0,
        min_basis_dollars: 5.0,
        edge_premium_cents: 1,
    };
    let mut s2 = PerpEventBasis::new(noedge_cfg).unwrap();
    let props2 = run(&mut s2, &w2, &perp_tick(PERP, "6.3906"));
    assert!(
        props2.is_empty(),
        "fair == limit (99) leaves no edge ⇒ no proposal"
    );
}

/// 9. DETERMINISM: identical inputs twice → identical `Vec<Proposal>`.
#[test]
fn determinism_identical_inputs_identical_proposals() {
    fn once() -> Vec<Proposal> {
        let (ladder, less, between, greater) = three_bin_ladder();
        let mut w = World::new();
        books_tradeable_target_between(&mut w, &less, &between, &greater);
        let mut s = PerpEventBasis::new(cfg(ladder)).unwrap();
        run(&mut s, &w, &perp_tick(PERP, "6.3906"))
    }
    let a = once();
    let b = once();
    // Proposal is not PartialEq; compare the load-bearing fields + thesis.
    assert_eq!(a.len(), b.len());
    for (pa, pb) in a.iter().zip(b.iter()) {
        assert_eq!(pa.urgency, pb.urgency);
        assert_eq!(pa.thesis, pb.thesis, "thesis is byte-identical across runs");
        assert_eq!(pa.legs.len(), pb.legs.len());
        for (la, lb) in pa.legs.iter().zip(pb.legs.iter()) {
            assert_eq!(la.market, lb.market);
            assert_eq!(la.side, lb.side);
            assert_eq!(la.action, lb.action);
            assert_eq!(la.limit_price, lb.limit_price);
            assert_eq!(la.fair_value, lb.fair_value);
            assert_eq!(la.calibrated_p, lb.calibrated_p);
        }
    }
}

/// 10. `target_market` rung-0 helper: between-containment, greater-tail,
///     less-tail, and the none-contains case. A pure function of (catalog, mark).
#[test]
fn target_market_selects_the_containing_rung() {
    let (ladder, less, between, greater) = three_bin_ladder();
    let s = PerpEventBasis::new(cfg(ladder)).unwrap();

    // Inside the between bin [63_500, 63_999.99) → the between bin.
    assert_eq!(s.target_market(63_906.0), Some(&mkt(&between)));
    // At the between floor (inclusive lower bound) → the between bin.
    assert_eq!(s.target_market(63_500.0), Some(&mkt(&between)));
    // Above the top between bin AND above the greater floor → the greater tail.
    assert_eq!(s.target_market(72_000.0), Some(&mkt(&greater)));
    // Below the bottom between bin AND below the less cap → the less tail.
    assert_eq!(s.target_market(55_000.0), Some(&mkt(&less)));
    // In the GAP between the less cap ($60k) and the between floor ($63.5k):
    // no between contains it, and neither tail does (≥ less.cap and <
    // greater.floor) → None.
    assert_eq!(
        s.target_market(61_000.0),
        None,
        "an uncovered gap selects nothing"
    );
    // In the gap ABOVE the between cap ($64k) and BELOW the greater floor
    // ($70k) → also None.
    assert_eq!(
        s.target_market(67_000.0),
        None,
        "the upper gap selects nothing"
    );
    // Exactly at the between cap (exclusive upper bound) is NOT in the between
    // bin; it falls in the upper gap → None.
    assert_eq!(s.target_market(63_999.99), None, "the cap is exclusive");
}

// ── live-fixture e2e (the operator's "test with live data" requirement) ──────

/// The committed paired-cycle recording: the KXBTCPERP perp + the KXBTC ladder.
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.json")
}

#[derive(Debug, Deserialize)]
struct PairedCycle {
    perp: FxPerp,
    kxbtc_ladder: Vec<FxLadderMarket>,
}

#[derive(Debug, Deserialize)]
struct FxPerp {
    ticker: String,
    /// The per-contract settlement value (BTC/10000), the value `PerpPrice`
    /// carries directly (the strategy lifts it ×10000 back to BTC dollars).
    settlement_mark_per_contract_dollars: String,
    /// The already-scaled BTC value, kept only to PRINT the relationship.
    settlement_mark_dollars: String,
}

#[derive(Debug, Deserialize)]
struct FxLadderMarket {
    ticker: String,
    strike_type: String,
    floor_strike: Option<f64>,
    cap_strike: Option<f64>,
    yes_bid_dollars: String,
    yes_ask_dollars: String,
    status: String,
}

/// Parse a YES dollar-string (`"0.06"` = $0.06 = 6 cents) into integer cents.
/// The venue quotes on a $1 payout, so dollars × 100 is the cent price.
fn dollars_to_cents(s: &str) -> i64 {
    let d = Decimal::from_str(s).unwrap_or_else(|e| panic!("yes dollar-string {s:?}: {e}"));
    let cents = d * Decimal::from(100);
    cents
        .round()
        .to_string()
        .parse::<i64>()
        .unwrap_or_else(|e| panic!("cents {cents} → i64: {e}"))
}

#[test]
fn live_paired_cycle_proposes_on_the_containing_bin() {
    // 1. Load the committed live recording.
    let path = fixture_path();
    let bytes = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read live fixture {}: {e}", path.display()));
    let cycle: PairedCycle = serde_json::from_str(&bytes)
        .unwrap_or_else(|e| panic!("parse live fixture {}: {e}", path.display()));

    // 2. Build cfg.ladder from the active kxbtc_ladder entries, and populate a
    //    book for each ladder MarketId from its yes_bid/ask dollar-strings.
    let mut ladder: BTreeMap<MarketId, BracketStrike> = BTreeMap::new();
    let mut w = World::new();
    let mut n_between = 0usize;
    for m in &cycle.kxbtc_ladder {
        if m.status != "active" {
            continue;
        }
        let strike = match m.strike_type.as_str() {
            "between" => {
                n_between += 1;
                BracketStrike::Between {
                    floor: m.floor_strike.expect("between has a floor"),
                    cap: m.cap_strike.expect("between has a cap"),
                }
            }
            "greater" => BracketStrike::Greater {
                floor: m.floor_strike.expect("greater has a floor"),
            },
            "less" => BracketStrike::Less {
                cap: m.cap_strike.expect("less has a cap"),
            },
            other => panic!("unexpected strike_type {other:?} in the live fixture"),
        };
        let id = mkt(&m.ticker);
        ladder.insert(id.clone(), strike);
        let bid = dollars_to_cents(&m.yes_bid_dollars);
        let ask = dollars_to_cents(&m.yes_ask_dollars);
        // The book validator wants prices in (0,100) and bid < ask; the live
        // quotes that are 0/`ask` (illiquid bid) would fail PriceLevel's >0
        // rule, so represent a 0-bid bin as bid-empty (its prob is 0 anyway).
        let b = if bid <= 0 {
            book_no_bid(&m.ticker, ask.max(1))
        } else {
            book(&m.ticker, bid, ask.max(bid + 1))
        };
        w.put(b);
    }
    assert_eq!(n_between, 48, "live ladder has 48 active between bins");
    assert_eq!(
        ladder.len(),
        50,
        "48 between + 1 greater + 1 less, all active"
    );

    // 3. cfg per the operator spec.
    let cfg = PerpEventBasisConfig {
        perp_market: mkt(&cycle.perp.ticker),
        ladder,
        fee_floor_dollars: 10.0,
        min_basis_dollars: 5.0,
        edge_premium_cents: 2,
    };
    let mut s = PerpEventBasis::new(cfg).unwrap();

    // 4. Fire a PerpTick whose settlement mark is the per-contract value
    //    ("6.3906" → PerpPrice raw 63906; ×10000 → BTC $63,906).
    let ev = perp_tick(
        &cycle.perp.ticker,
        &cycle.perp.settlement_mark_per_contract_dollars,
    );
    let props = run(&mut s, &w, &ev);

    // ── ASSERT: exactly one Passive/Buy/Yes proposal on the between bin whose
    //    [floor,cap] contains $63,906 (the $63,500–63,999.99 bin). ──
    assert_eq!(
        props.len(),
        1,
        "the live tradeable basis yields exactly one proposal"
    );
    let leg = &props[0].legs[0];
    assert_eq!(props[0].urgency, Urgency::Passive);
    assert_eq!(leg.side, Side::Yes);
    assert_eq!(leg.action, Action::Buy);
    // Confirm the target bin actually contains $63,906.
    let target_strike = cycle
        .kxbtc_ladder
        .iter()
        .find(|m| m.ticker == leg.market.as_str())
        .expect("target is a ladder market");
    let floor = target_strike
        .floor_strike
        .expect("the target between bin has a floor");
    let cap = target_strike
        .cap_strike
        .expect("the target between bin has a cap");
    assert!(
        floor <= 63_906.0 && 63_906.0 < cap,
        "target bin [{floor}, {cap}) must contain $63,906"
    );

    // CROSS-LAYER CONSISTENCY (the validated reference). The strategy rebuilds
    // bin probabilities from the runtime INTEGER-CENT books; it must reproduce
    // the SAME basis the kernel e2e (basis_live_fixture.rs) pins from the raw
    // dollar quotes — implied median ≈ $63,961.53, signed basis ≈ −$55.53 (the
    // GAPS "two independent sources agree <0.1%" evidence). All live YES quotes
    // are whole-cent, so the cent books carry identical information; in
    // particular the 32 zero-bid bins must contribute ask/2 mass, NOT be dropped
    // to 0 (dropping them shifts the median to ~$64,133 / basis ~−$227). The
    // strategy reports both numbers in its thesis.
    assert!(
        props[0].thesis.contains("median $63961"),
        "strategy must reproduce the validated implied median ~$63,961 (got: {})",
        props[0].thesis
    );
    assert!(
        props[0].thesis.contains("signed_basis $-55"),
        "strategy must reproduce the validated signed basis ~−$55 (got: {})",
        props[0].thesis
    );

    // The headline printout (the proposal on real data).
    let perp_btc: f64 = cycle.perp.settlement_mark_dollars.parse().unwrap();
    println!("[perp_event_basis — LIVE paired cycle KXBTCPERP vs KXBTC]");
    println!(
        "  perp settlement_mark (per-contract): ${}  ×10000 → BTC ${perp_btc}",
        cycle.perp.settlement_mark_per_contract_dollars
    );
    println!("  target bin     : {} = [{floor}, {cap})", leg.market);
    println!("  join limit     : {}", leg.limit_price);
    println!("  fair value     : {}", leg.fair_value);
    println!("  thesis         : {}", props[0].thesis);
}
