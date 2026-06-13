//! T4.2 item 2(ii): book-driven recorded-stream replay into `PaperVenue`,
//! composing the two MECHANICAL strategies over the OPERATOR-RECORDED Kalshi
//! websocket fixtures. Written from docs/design/track-a-completion-queue.md
//! item 2(ii) and spec Section 11 (paper-fill realism).
//!
//! "TRULY LIVE-TESTED" here = the SAME parser/assembler/PaperVenue seam the
//! live dial will drive (`KalshiWsParser` -> `BookAssembler` ->
//! `fortuna_paper::feed_stream_event`) is exercised against the VERBATIM
//! frames the operator captured on the demo venue
//! (fixtures/kalshi/ws__orderbook_trade_*.jsonl) — not synthetic and not
//! doc-derived frames. If the recorded wire diverges from what the adapter
//! expects, these tests red.
//!
//! FIXTURE BLOCK (load-bearing — never fabricate): the recorded session was a
//! QUIET market and captured ZERO public `trade` frames (fixtures/kalshi/
//! README.md: "WS capture is from a quiet market (5-7 frames: subscribed +
//! snapshot + deltas)"; GAPS.md operator queue, "public WS `trade` frame ...
//! gates the paper-engine trade-through replay"). Paper maker fills are
//! TRADE-driven (spec 11: only a print strictly THROUGH a resting limit
//! fills, never a touch, never a book move), so a book-only replay can assert
//! book state and strategy decisions but CANNOT exercise trade-through fills.
//! That portion stays fixture-blocked until the operator captures a
//! busy-market trade frame; we ledger it, we never invent one. The fill PATH
//! itself is proven live by fortuna-paper's
//! `a_recorded_stream_drives_paper_fills_through_the_assembler` (a synthetic
//! trade-through, in that crate's own tests).

use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_paper::{feed_stream_event, PaperConfig, PaperVenue};
use fortuna_runner::mech_extremes::{MechExtremes, MechExtremesConfig};
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{CoreHandle, Strategy};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::kalshi::ws::{KalshiWsEvent, KalshiWsParser};
use fortuna_venues::stream::{BookAssembler, StreamEvent};
use fortuna_venues::{Cursor, Market, MarketStatus, SettlementMeta, Venue};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// The single market the operator traded during the 2026-06-11 recording.
const RECORDED_TICKER: &str = "KXWTACHALLENGERMATCH-26JUN11JIMLEP-LEP";

const HOUR_MS: i64 = 3_600_000;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T06:31:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn level(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: Contracts::new(qty),
    }
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            maker_coeff = "0.0175"
        "#,
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

/// The recorded market with caller-chosen catalog metadata.
fn recorded_market(
    volume: Option<i64>,
    status: MarketStatus,
    close_at: Option<UtcTimestamp>,
) -> Market {
    Market {
        id: mkt(RECORDED_TICKER),
        venue: VenueId::new("kalshi").unwrap(),
        title: "recorded demo market (KXWTACHALLENGERMATCH)".into(),
        category: "weather".into(),
        status,
        close_at,
        settlement: SettlementMeta {
            oracle_type: "ap".into(),
            resolution_source: "ap".into(),
            expected_lag_hours: 2,
        },
        volume_contracts: volume,
        payout_per_contract: Cents::new(100),
    }
}

/// An eligible, far-from-close, low-volume version of the recorded market
/// (so any strategy abstention is about the PRICE, not the metadata guards).
fn eligible_recorded_market() -> Market {
    recorded_market(
        Some(50_000),
        MarketStatus::Trading,
        Some(t0().checked_add_millis(48 * HOUR_MS).unwrap()),
    )
}

fn paper_venue() -> (Arc<SimClock>, PaperVenue) {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = PaperVenue::new(
        VenueId::new("paper").unwrap(),
        clock.clone(),
        fee_model(),
        PaperConfig {
            maker_haircut_pct: 50,
        },
        Cents::new(1_000_000),
    )
    .unwrap();
    (clock, venue)
}

/// Verbatim non-blank frames of a recorded WS fixture.
fn fixture_frames(name: &str) -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/kalshi")
        .join(name);
    let body =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

struct ParsedFixture {
    events: Vec<StreamEvent>,
    subscribed: usize,
    snapshots: usize,
    deltas: usize,
    trades: usize,
}

/// Parse every recorded frame through the REAL Kalshi WS parser, asserting the
/// recorded wire is fully typed and GAPLESS (no parse error, no `SeqGap`).
fn parse_fixture(name: &str) -> ParsedFixture {
    let mut parser = KalshiWsParser::new();
    let mut events = Vec::new();
    let (mut subscribed, mut snapshots, mut deltas, mut trades) = (0, 0, 0, 0);
    for (i, frame) in fixture_frames(name).into_iter().enumerate() {
        let parsed = parser
            .parse_frame(&frame)
            .unwrap_or_else(|e| panic!("frame {i} of {name} failed to parse: {e}\n  {frame}"));
        match parsed {
            KalshiWsEvent::Stream(ev) => {
                match &ev {
                    StreamEvent::BookSnapshot { .. } => snapshots += 1,
                    StreamEvent::BookDelta { .. } => deltas += 1,
                    StreamEvent::Trade { .. } => trades += 1,
                }
                events.push(ev);
            }
            KalshiWsEvent::Subscribed { .. } => subscribed += 1,
            KalshiWsEvent::SeqGap { sid, expected, got } => panic!(
                "recorded fixture {name} has a sequence gap on sid {sid} \
                 (expected {expected}, got {got}); the operator capture must be contiguous"
            ),
            other => panic!("unexpected control frame in {name}: {other:?}"),
        }
    }
    ParsedFixture {
        events,
        subscribed,
        snapshots,
        deltas,
        trades,
    }
}

/// Replay a recorded fixture's BOOK frames into a PaperVenue through the
/// production seam (`feed_stream_event`), returning the final assembled book.
/// The market must already be registered on `venue`.
fn replay_book(venue: &PaperVenue, name: &str) -> OrderBook {
    let parsed = parse_fixture(name);
    assert_eq!(
        parsed.trades, 0,
        "{name}: a quiet-market recording carries no public trade frames (the fixture block)"
    );
    let mut assembler = BookAssembler::new();
    for ev in parsed.events {
        feed_stream_event(venue, &mut assembler, ev, t0()).unwrap();
    }
    futures::executor::block_on(venue.book(&mkt(RECORDED_TICKER))).unwrap()
}

fn replayed(market: Market, name: &str) -> (Arc<SimClock>, PaperVenue, OrderBook) {
    let (clock, venue) = paper_venue();
    venue.add_market(market);
    let book = replay_book(&venue, name);
    (clock, venue, book)
}

/// Gate an order through a permissive real pipeline (I1: every order is gated).
/// Gate inputs are intentionally minimal — `book: None` skips the book-dependent
/// checks; the purpose here is only to obtain a resting order, not to test gates.
fn gated(seed: u64, side: Side, action: Action, price: i64, qty: i64) -> fortuna_gates::GatedOrder {
    use fortuna_core::ids::{IdGen, IntentId};
    use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};
    use std::collections::BTreeSet;
    let cfg: GateConfig = toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 10000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 10000000
        per_event_exposure_cents = 10000000
        require_event_mapping = false

        [per_strategy.paper_test]
        max_exposure_cents = 10000000
        max_order_notional_cents = 10000000
        min_net_edge_bps = 0

        [rate.paper]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap();
    let mut pipeline = GatePipeline::new(cfg).unwrap();
    let mut g = IdGen::new(seed);
    let intent = IntentId::new(g.next(t0()).unwrap());
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("paper_test").unwrap(),
        venue: VenueId::new("paper").unwrap(),
        market: mkt(RECORDED_TICKER),
        side,
        action,
        limit_price: Cents::new(price),
        qty: Contracts::new(qty),
        fair_value: match action {
            Action::Buy => Cents::new((price + 5).min(99)),
            Action::Sell => Cents::new((price - 5).max(1)),
        },
        client_order_id: ClientOrderId::from_intent(intent),
    };
    let fees = fee_model();
    let recent = BTreeSet::new();
    let inputs = GateInputs {
        now: t0(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: None,
        last_trade_price: Some(Cents::new(50)),
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    pipeline.evaluate(&candidate, &inputs).gated.unwrap()
}

fn all_fills(v: &PaperVenue) -> Vec<fortuna_venues::Fill> {
    futures::executor::block_on(v.fills_since(Cursor::start()))
        .unwrap()
        .fills
}

// --------------------------------------------------------------------------
// 1-2. The recorded wire parses, fully typed and gapless, with no trade frames.
// --------------------------------------------------------------------------

#[test]
fn recorded_yes_fixture_parses_gapless_and_fully_typed_with_no_trade_frames() {
    let p = parse_fixture("ws__orderbook_trade_yes.jsonl");
    assert_eq!(
        p.subscribed, 2,
        "two subscribe acks (orderbook_delta + trade)"
    );
    assert_eq!(p.snapshots, 1, "one opening snapshot");
    assert_eq!(p.deltas, 2, "two recorded book deltas");
    assert_eq!(
        p.trades, 0,
        "quiet market: ZERO public trade prints (the fixture block)"
    );
    // The stream events are exactly the snapshot + the two deltas, in order.
    assert!(matches!(p.events[0], StreamEvent::BookSnapshot { .. }));
    assert_eq!(p.events.len(), 3);
}

#[test]
fn recorded_noleg_fixture_parses_gapless_and_fully_typed_with_no_trade_frames() {
    let p = parse_fixture("ws__orderbook_trade_noleg.jsonl");
    assert_eq!(p.subscribed, 2);
    assert_eq!(p.snapshots, 1);
    assert_eq!(p.deltas, 4, "four recorded book deltas");
    assert_eq!(p.trades, 0, "the fixture block: no trade frames");
    assert_eq!(p.events.len(), 5);
}

// --------------------------------------------------------------------------
// 3-4. The recorded frames assemble into the EXACT book inside PaperVenue.
// --------------------------------------------------------------------------

#[test]
fn recorded_yes_fixture_replays_into_the_exact_book_in_paper_venue() {
    let (_c, _v, book) = replayed(eligible_recorded_market(), "ws__orderbook_trade_yes.jsonl");
    // YES 0.47x3 (untouched). NO 0.52: snapshot 1, delta -1 (empties the
    // level), delta +2 => 2 resting. Asks are the NO side on the YES scale.
    assert_eq!(book.yes_bids, vec![level(47, 3)]);
    assert_eq!(book.yes_asks, vec![level(52, 2)]);
}

#[test]
fn recorded_noleg_fixture_replays_into_the_exact_book_in_paper_venue() {
    let (_c, _v, book) = replayed(
        eligible_recorded_market(),
        "ws__orderbook_trade_noleg.jsonl",
    );
    // YES 0.47: snapshot 3, -3 (empties), +3 => 3. NO 0.48: snapshot 2, -2
    // (empties), +1 => 1. The transient empty book between the deltas is a
    // valid (one-/zero-sided) book and replays without a torn-state error.
    assert_eq!(book.yes_bids, vec![level(47, 3)]);
    assert_eq!(book.yes_asks, vec![level(48, 1)]);
}

// --------------------------------------------------------------------------
// 5. Book-only replay produces NO fills — trade-through stays fixture-blocked.
// --------------------------------------------------------------------------

#[test]
fn book_only_replay_yields_no_fills_trade_through_stays_fixture_blocked() {
    let (_c, venue) = paper_venue();
    venue.add_market(eligible_recorded_market());

    // A resting maker bid at 50 — between the recorded 47 bid and 52 ask. On
    // the (initially empty) book it rests rather than crosses.
    futures::executor::block_on(venue.place(gated(7, Side::Yes, Action::Buy, 50, 10))).unwrap();

    // Replay the recorded book-only stream. The book moves to 47/52, but with
    // no public trade prints the resting bid never fills: a paper maker fill
    // requires a trade THROUGH the limit (spec 11), which a book delta is not.
    let book = replay_book(&venue, "ws__orderbook_trade_yes.jsonl");
    assert_eq!(
        book.yes_bids,
        vec![level(47, 3)],
        "the replayed book is live"
    );

    let fills = all_fills(&venue);
    assert!(
        fills.is_empty(),
        "book-only replay must not fill a resting order (only trade-throughs may): {fills:?}"
    );
    // Trade-through replay against RECORDED data stays fixture-blocked until
    // the operator captures a busy-market trade frame (GAPS.md operator
    // queue). The fill path is proven live by fortuna-paper's
    // a_recorded_stream_drives_paper_fills_through_the_assembler.
}

// --------------------------------------------------------------------------
// 6. Both mechanical strategies consume the replayed recorded book.
// --------------------------------------------------------------------------

#[test]
fn mech_strategies_consume_the_replayed_recorded_book_and_abstain_correctly() {
    // The strategies see the EXACT recorded book replayed through PaperVenue.
    let meta = eligible_recorded_market();
    let (_c, _v, book) = replayed(meta.clone(), "ws__orderbook_trade_yes.jsonl");
    assert_eq!(book.yes_bids, vec![level(47, 3)]);
    assert_eq!(book.yes_asks, vec![level(52, 2)]);

    let books = BTreeMap::from([(mkt(RECORDED_TICKER), book.clone())]);
    let markets = BTreeMap::from([(mkt(RECORDED_TICKER), meta)]);
    let fees = fee_model();
    let core = CoreHandle {
        now: t0(),
        books: &books,
        markets: &markets,
        fee_model: &fees,
    };
    let ev = BusEvent {
        seq: 1,
        at: t0(),
        origin: EventOrigin::External,
        payload: EventPayload::BookSnapshot {
            venue: VenueId::new("paper").unwrap(),
            book: book.clone(),
        },
    };

    // mech_extremes: the market is ELIGIBLE (Trading, volume 50k <= cap, close
    // 48h out), so the only reason to abstain is the price — YES 47c / NO 48c
    // (= 100-52) are mid-book, nowhere near the 90c extreme. It processes the
    // event and correctly abstains. (events_seen proves the recorded book
    // reached it; mech_extremes' own unit tests prove it FIRES on an extreme.)
    let mut extremes = MechExtremes::new(MechExtremesConfig {
        extreme_min_cents: 90,
        bias_premium_cents: 2,
        max_volume_contracts: 100_000,
        min_ms_to_close: HOUR_MS,
    })
    .unwrap();
    let out = futures::executor::block_on(extremes.on_event(&ev, &core)).unwrap();
    assert!(
        out.is_empty(),
        "a 47/52 recorded book is not a price extreme: {out:?}"
    );
    assert_eq!(
        extremes.metrics().events_seen,
        1,
        "the recorded book event reached mech_extremes"
    );

    // mech_structural: a bracket arb needs EVERY leg's book to certify. The
    // recording captured ONE market, so the bracket is incomplete and the scan
    // abstains (no fabricated complement). A multi-market bracket fixture is
    // the ledgered unblock for a positive structural-arb replay (GAPS.md).
    let mut structural = MechStructural::new(MechStructuralConfig {
        bracket_sets: vec![vec![mkt(RECORDED_TICKER), mkt("KXBRACKETLEG2-26JUN11-B")]],
        min_edge_cents_per_set: 2,
        max_unhedged_notional: Cents::new(100_000),
        max_leg_open_ms: 60_000,
        min_completion_edge_bps: 50,
    })
    .unwrap();
    let out = futures::executor::block_on(structural.on_event(&ev, &core)).unwrap();
    assert!(
        out.is_empty(),
        "a single-market recording cannot complete a bracket: {out:?}"
    );
    assert_eq!(
        structural.metrics().events_seen,
        1,
        "the recorded book event reached mech_structural"
    );
}

// --------------------------------------------------------------------------
// 7. Liveness controls: a QUALIFYING input FIRES each strategy in this exact
//    composition — so test 6's abstentions are genuine decisions about the
//    recorded data, not dead wiring (the missing complement / non-extreme
//    price are the real causes). The qualifying inputs here are SYNTHETIC
//    (clearly NOT recorded venue data, NOT committed fixtures): they exercise
//    strategy LOGIC, the part the recorded quiet market cannot.
// --------------------------------------------------------------------------

#[test]
fn a_qualifying_input_fires_each_strategy_in_the_replay_composition() {
    let fees = fee_model();

    // mech_extremes FIRES on a synthetic 92/93 extreme book (eligible market):
    // proves the recorded-book abstention in test 6 is the non-extreme price.
    let ctl_id = mkt("KXEXTREMECTL-26JUN11-X");
    let ctl_book = OrderBook {
        market: ctl_id.clone(),
        as_of: t0(),
        yes_bids: vec![level(92, 50)],
        yes_asks: vec![level(93, 50)],
    };
    let ctl_market = Market {
        id: ctl_id.clone(),
        venue: VenueId::new("sim").unwrap(),
        title: "synthetic extreme control (NOT recorded)".into(),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: Some(t0().checked_add_millis(48 * HOUR_MS).unwrap()),
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        volume_contracts: Some(50_000),
        payout_per_contract: Cents::new(100),
    };
    let books = BTreeMap::from([(ctl_id.clone(), ctl_book.clone())]);
    let markets = BTreeMap::from([(ctl_id.clone(), ctl_market)]);
    let core = CoreHandle {
        now: t0(),
        books: &books,
        markets: &markets,
        fee_model: &fees,
    };
    let ev = BusEvent {
        seq: 1,
        at: t0(),
        origin: EventOrigin::External,
        payload: EventPayload::BookSnapshot {
            venue: VenueId::new("sim").unwrap(),
            book: ctl_book,
        },
    };
    let mut extremes = MechExtremes::new(MechExtremesConfig {
        extreme_min_cents: 90,
        bias_premium_cents: 2,
        max_volume_contracts: 100_000,
        min_ms_to_close: HOUR_MS,
    })
    .unwrap();
    let out = futures::executor::block_on(extremes.on_event(&ev, &core)).unwrap();
    assert_eq!(out.len(), 1, "mech_extremes must fire on a 92/93 extreme");
    assert_eq!(out[0].legs[0].side, Side::Yes);
    assert_eq!(
        out[0].legs[0].limit_price,
        Cents::new(92),
        "joins the YES bid"
    );

    // mech_structural FIRES when the RECORDED market's replayed book (one leg,
    // best YES ask 52c) is joined by a complement leg priced so the YES basket
    // is buyable below 100c: proves test 6's abstention is the MISSING leg, not
    // a pre-scan bail. The complement is a synthetic control, never a fixture.
    let (_c, _v, recorded_book) =
        replayed(eligible_recorded_market(), "ws__orderbook_trade_yes.jsonl");
    let leg2 = mkt("KXBRACKETLEG2-26JUN11-B");
    let leg2_book = OrderBook {
        market: leg2.clone(),
        as_of: t0(),
        yes_bids: vec![],
        yes_asks: vec![level(30, 50)], // 52 + 30 = 82c basket + fees << 100c
    };
    let books = BTreeMap::from([
        (mkt(RECORDED_TICKER), recorded_book.clone()),
        (leg2.clone(), leg2_book),
    ]);
    let markets = BTreeMap::from([(mkt(RECORDED_TICKER), eligible_recorded_market())]);
    let core = CoreHandle {
        now: t0(),
        books: &books,
        markets: &markets,
        fee_model: &fees,
    };
    let ev = BusEvent {
        seq: 1,
        at: t0(),
        origin: EventOrigin::External,
        payload: EventPayload::BookSnapshot {
            venue: VenueId::new("paper").unwrap(),
            book: recorded_book,
        },
    };
    let mut structural = MechStructural::new(MechStructuralConfig {
        bracket_sets: vec![vec![mkt(RECORDED_TICKER), leg2]],
        min_edge_cents_per_set: 2,
        max_unhedged_notional: Cents::new(100_000),
        max_leg_open_ms: 60_000,
        min_completion_edge_bps: 50,
    })
    .unwrap();
    let out = futures::executor::block_on(structural.on_event(&ev, &core)).unwrap();
    assert_eq!(
        out.len(),
        1,
        "completing the bracket cheaply must fire the scan over the recorded leg"
    );
    assert_eq!(out[0].legs.len(), 2, "both bracket legs are proposed");
}
