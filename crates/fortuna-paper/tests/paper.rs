//! T1.2 tests: paper-fill realism. Written from spec Section 11 BEFORE
//! implementation.
//!
//! THE RULES (verbatim from the spec): "maker fills in paper count ONLY when
//! the market trades through the limit price (not touches), with a
//! configurable quantity haircut; touch-fill optimism is the classic
//! paper-trading inflation and would corrupt every gate below; taker paper
//! fills assume crossing the visible book at displayed depth, never mid."
//!
//! The first test below is the doctrine test: it FAILS if anyone ever
//! "fills" at touch. It is permanent. Do not weaken it.

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_paper::{PaperConfig, PaperVenue};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::{Cursor, Market, MarketStatus, PriceLevel, SettlementMeta, Venue};
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
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

fn test_market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("paper").unwrap(),
        title: format!("paper market {id}"),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    }
}

fn lvl(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: Contracts::new(qty),
    }
}

fn paper(haircut_pct: u8) -> (Arc<SimClock>, PaperVenue) {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = PaperVenue::new(
        VenueId::new("paper").unwrap(),
        clock.clone(),
        fee_model(),
        PaperConfig {
            maker_haircut_pct: haircut_pct,
        },
        Cents::new(100_000),
    )
    .unwrap();
    venue.add_market(test_market("PM"));
    venue
        .apply_book(
            &mkt("PM"),
            vec![lvl(45, 50)],
            vec![lvl(55, 50), lvl(60, 50)],
        )
        .unwrap();
    (clock, venue)
}

/// Gate an order through a permissive real pipeline (I1 everywhere).
fn gated(seed: u64, side: Side, action: Action, price: i64, qty: i64) -> fortuna_gates::GatedOrder {
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
        market: mkt("PM"),
        side,
        action,
        limit_price: Cents::new(price),
        qty: Contracts::new(qty),
        // Honest fair values per direction: buys believe value above the
        // limit, sells below (the gates verify the edge sign).
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

// =========================================================================
// THE DOCTRINE TEST (spec Section 11): a print AT the limit price is a
// touch, not a trade-through, and MUST NOT fill. If this test ever fails,
// someone has reintroduced the classic paper-trading inflation. Do not
// weaken it; fix the engine.
// =========================================================================
#[test]
fn touch_prints_never_fill_resting_orders() {
    let (_c, v) = paper(50);
    // Resting bid at 50 (above best bid 45, below ask 55).
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 50, 10))).unwrap();
    // The market prints AT 50 — a touch. Repeatedly.
    for _ in 0..5 {
        v.apply_public_trade(&mkt("PM"), Cents::new(50), 100)
            .unwrap();
    }
    assert!(
        all_fills(&v).is_empty(),
        "TOUCH-FILL INFLATION: a print at the limit must never fill in paper"
    );
    // Sell side symmetrically: resting ask at 58; prints AT 58 must not fill.
    futures::executor::block_on(v.place(gated(2, Side::Yes, Action::Buy, 55, 5))).unwrap(); // own 5 first (taker)
    futures::executor::block_on(v.place(gated(3, Side::Yes, Action::Sell, 58, 5))).unwrap();
    let before = all_fills(&v).len();
    for _ in 0..5 {
        v.apply_public_trade(&mkt("PM"), Cents::new(58), 100)
            .unwrap();
    }
    assert_eq!(
        all_fills(&v).len(),
        before,
        "TOUCH-FILL INFLATION on the ask side"
    );
}

#[test]
fn trade_through_fills_at_our_price_with_haircut() {
    let (_c, v) = paper(50); // 50% haircut
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 50, 10))).unwrap();
    // Print BELOW our bid: the market traded through us.
    v.apply_public_trade(&mkt("PM"), Cents::new(49), 8).unwrap();
    let fills = all_fills(&v);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].price, Cents::new(50), "maker fills at OUR limit");
    assert_eq!(fills[0].qty.raw(), 4, "50% haircut of the 8-lot print");
    assert!(fills[0].is_maker);
}

#[test]
fn through_prints_accumulate_until_filled() {
    let (_c, v) = paper(50);
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 50, 5))).unwrap();
    v.apply_public_trade(&mkt("PM"), Cents::new(49), 4).unwrap(); // 2 after haircut
    v.apply_public_trade(&mkt("PM"), Cents::new(48), 4).unwrap(); // 2 more
    v.apply_public_trade(&mkt("PM"), Cents::new(47), 4).unwrap(); // 1 (capped at remaining)
    let fills = all_fills(&v);
    let total: i64 = fills.iter().map(|f| f.qty.raw()).sum();
    assert_eq!(total, 5);
    assert!(fills
        .iter()
        .all(|f| f.price == Cents::new(50) && f.is_maker));
    // Fully filled: further prints do nothing.
    v.apply_public_trade(&mkt("PM"), Cents::new(40), 50)
        .unwrap();
    let total2: i64 = all_fills(&v).iter().map(|f| f.qty.raw()).sum();
    assert_eq!(total2, 5);
}

#[test]
fn taker_crosses_displayed_depth_never_mid() {
    let (_c, v) = paper(50);
    // Book asks: 55x50, 60x50. Buy 60 @ limit 60: walks displayed depth.
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 60, 60))).unwrap();
    let fills = all_fills(&v);
    assert_eq!(fills.len(), 2);
    assert_eq!((fills[0].price, fills[0].qty.raw()), (Cents::new(55), 50));
    assert_eq!((fills[1].price, fills[1].qty.raw()), (Cents::new(60), 10));
    assert!(fills.iter().all(|f| !f.is_maker));
    // Never mid: every fill price IS a displayed level, mechanically. The
    // assertion above pins exact level prices; a mid (57c) is unrepresentable.
}

#[test]
fn taker_remainder_rests_under_maker_rules() {
    let (_c, v) = paper(50);
    // Only 50 displayed at 55; buy 60 @ 55: 50 fill taker, 10 rest.
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 55, 60))).unwrap();
    assert_eq!(all_fills(&v).len(), 1);
    // Touch at 55: NO fill for the resting remainder.
    v.apply_public_trade(&mkt("PM"), Cents::new(55), 100)
        .unwrap();
    assert_eq!(all_fills(&v).len(), 1);
    // Through at 54: haircut fill.
    v.apply_public_trade(&mkt("PM"), Cents::new(54), 10)
        .unwrap();
    let fills = all_fills(&v);
    assert_eq!(fills.len(), 2);
    assert_eq!(fills[1].qty.raw(), 5);
    assert!(fills[1].is_maker);
}

#[test]
fn sell_side_trade_through_is_a_print_above_our_ask() {
    let (_c, v) = paper(100); // no haircut: full print qty
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 55, 10))).unwrap(); // own 10
    futures::executor::block_on(v.place(gated(2, Side::Yes, Action::Sell, 58, 10))).unwrap();
    let before = all_fills(&v).len();
    v.apply_public_trade(&mkt("PM"), Cents::new(57), 100)
        .unwrap(); // below: nothing
    assert_eq!(all_fills(&v).len(), before);
    v.apply_public_trade(&mkt("PM"), Cents::new(59), 6).unwrap(); // through!
    let fills = all_fills(&v);
    assert_eq!(fills.len(), before + 1);
    let last = fills.last().unwrap();
    assert_eq!(last.price, Cents::new(58));
    assert_eq!(last.qty.raw(), 6);
    assert!(last.is_maker);
}

#[test]
fn no_side_orders_translate_to_yes_space_for_trade_through() {
    let (_c, v) = paper(100);
    // Resting buy NO at 30 == yes-space ask at 70: prints ABOVE 70 fill it.
    futures::executor::block_on(v.place(gated(1, Side::No, Action::Buy, 30, 10))).unwrap();
    v.apply_public_trade(&mkt("PM"), Cents::new(70), 50)
        .unwrap(); // touch: no
    assert!(all_fills(&v).is_empty());
    v.apply_public_trade(&mkt("PM"), Cents::new(71), 4).unwrap(); // through
    let fills = all_fills(&v);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].side, Side::No);
    assert_eq!(
        fills[0].price,
        Cents::new(30),
        "NO order fills at its NO price"
    );
    assert_eq!(fills[0].qty.raw(), 4);
}

#[test]
fn haircut_bounds_are_validated_and_applied() {
    // 0% is refused at construction (a paper venue that can never fill
    // makers would silently bias GO/NO-GO numbers).
    let clock = Arc::new(SimClock::new(t0()));
    assert!(PaperVenue::new(
        VenueId::new("paper").unwrap(),
        clock.clone(),
        fee_model(),
        PaperConfig {
            maker_haircut_pct: 0
        },
        Cents::new(1_000),
    )
    .is_err());
    assert!(PaperVenue::new(
        VenueId::new("paper").unwrap(),
        clock,
        fee_model(),
        PaperConfig {
            maker_haircut_pct: 101
        },
        Cents::new(1_000),
    )
    .is_err());

    // 100%: the full print quantity counts.
    let (_c, v) = paper(100);
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 50, 10))).unwrap();
    v.apply_public_trade(&mkt("PM"), Cents::new(49), 7).unwrap();
    assert_eq!(all_fills(&v)[0].qty.raw(), 7);

    // 25%: floor(7 * 25%) = 1.
    let (_c2, v2) = paper(25);
    futures::executor::block_on(v2.place(gated(2, Side::Yes, Action::Buy, 50, 10))).unwrap();
    v2.apply_public_trade(&mkt("PM"), Cents::new(49), 7)
        .unwrap();
    assert_eq!(all_fills(&v2)[0].qty.raw(), 1);
}

#[test]
fn accounting_mirrors_venue_semantics() {
    let (_c, v) = paper(50);
    let start = futures::executor::block_on(v.balance()).unwrap();
    // Resting buy reserves worst-case cost like a real venue.
    futures::executor::block_on(v.place(gated(1, Side::Yes, Action::Buy, 50, 10))).unwrap();
    let after_rest = futures::executor::block_on(v.balance()).unwrap();
    assert!(after_rest < start);
    // Trade-through fill: cash spent, reservation trimmed, position booked.
    v.apply_public_trade(&mkt("PM"), Cents::new(49), 20)
        .unwrap();
    let pos = futures::executor::block_on(v.positions()).unwrap();
    assert_eq!(pos.len(), 1);
    assert_eq!(pos[0].yes, 10);
}

#[test]
fn paper_live_parity_through_the_order_manager() {
    // The Strategy/exec interface cannot tell paper from live: the manager
    // round-trips unchanged (spec 11 parity requirement).
    use fortuna_exec::{ExecPolicy, IntentStatus, MemoryJournal, OrderManager};
    let (_c, v) = paper(50);
    let clock = Arc::new(SimClock::new(t0()));
    let mut m = futures::executor::block_on(OrderManager::recover(
        MemoryJournal::default(),
        clock,
        ExecPolicy::default(),
    ))
    .unwrap();
    let order = gated(9, Side::Yes, Action::Buy, 50, 10);
    let intent = order.intent_id();
    futures::executor::block_on(m.submit(order, &v)).unwrap();
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Acked);
    v.apply_public_trade(&mkt("PM"), Cents::new(48), 10)
        .unwrap(); // budget 5 of 10: partial
    let page = futures::executor::block_on(v.fills_since(Cursor::start())).unwrap();
    for f in &page.fills {
        futures::executor::block_on(m.ingest_fill(f)).unwrap();
    }
    assert_eq!(
        m.intent(intent).unwrap().status,
        IntentStatus::PartiallyFilled
    );
    let report = futures::executor::block_on(m.boot_reconcile(&v)).unwrap();
    assert!(report.orphans_cancelled.is_empty());
}
