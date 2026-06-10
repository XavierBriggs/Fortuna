//! T0.6 tests: intent journal state machine + order manager. Written from
//! spec 5.4 before implementation.
//!
//! Contract: every intent is journaled BEFORE any network call; client ids
//! derive from intent ids so crash resubmission is idempotent; delivery is
//! at-least-once with idempotent dedup; boot reconciliation runs before any
//! strategy wakes (fills drained, journal matched against venue open orders,
//! orphans cancelled and alerted, stuck intents advanced).

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::{
    ExecError, ExecPolicy, IntentJournal, IntentStatus, MemoryJournal, OrderManager, SubmitOutcome,
};
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{Cursor, Market, MarketStatus, PriceLevel, SettlementMeta, Venue};
use std::collections::BTreeSet;
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
        venue: VenueId::new("sim").unwrap(),
        title: format!("market {id}"),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        payout_per_contract: Cents::new(100),
    }
}

fn level(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: Contracts::new(qty),
    }
}

fn venue_with(faults: FaultConfig, clock: Arc<SimClock>) -> SimVenue {
    let v = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock,
        fee_model(),
        faults,
        Cents::new(100_000),
    );
    v.add_market(test_market("M1"));
    v.set_book(&mkt("M1"), vec![level(45, 50)], vec![level(55, 50)])
        .unwrap();
    v
}

/// Gate a candidate through a permissive real pipeline (I1: even tests get
/// orders only from the pipeline).
fn gate(candidate: CandidateOrder, clock: &SimClock) -> fortuna_gates::GatedOrder {
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

        [per_strategy.s1]
        max_exposure_cents = 10000000
        max_order_notional_cents = 10000000
        min_net_edge_bps = 0

        [rate.sim]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap();
    let mut pipeline = GatePipeline::new(cfg).unwrap();
    let fees = fee_model();
    let recent = BTreeSet::new();
    let inputs = GateInputs {
        now: clock.now(),
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

fn candidate(seed: u64, market: &str, price: i64, qty: i64) -> CandidateOrder {
    let mut g = IdGen::new(seed);
    let intent = IntentId::new(g.next(t0()).unwrap());
    CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("s1").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market: mkt(market),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(price),
        qty: Contracts::new(qty),
        fair_value: Cents::new(price + 5),
        client_order_id: ClientOrderId::from_intent(intent),
    }
}

fn manager(clock: Arc<SimClock>) -> OrderManager<MemoryJournal> {
    OrderManager::recover(MemoryJournal::default(), clock, ExecPolicy::default()).unwrap()
}

// ---- submission ----

#[test]
fn submit_journals_before_the_network_and_acks_on_success() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());

    let order = gate(candidate(1, "M1", 50, 5), &clock);
    let intent = order.intent_id();
    let out = futures::executor::block_on(m.submit(order, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Acked { .. }));

    let rec = m.intent(intent).unwrap();
    assert_eq!(rec.status, IntentStatus::Acked);
    assert!(rec.venue_order_id.is_some());
    // The journal shows Created -> SubmitAttempted -> Acked, in order.
    let kinds = m.journal().event_kinds_for(intent);
    assert_eq!(kinds, vec!["created", "submit_attempted", "acked"]);
}

#[test]
fn submit_with_already_exists_is_recovery_not_failure() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());

    // First submission acks.
    let order = gate(candidate(2, "M1", 50, 5), &clock);
    let intent = order.intent_id();
    futures::executor::block_on(m.submit(order, &venue)).unwrap();
    let first_venue_id = m.intent(intent).unwrap().venue_order_id.clone().unwrap();

    // A second gated order with the SAME intent (same client order id, as a
    // crash-resubmission would produce) resolves to the same venue order.
    let order2 = gate(candidate(2, "M1", 50, 5), &clock);
    let out = futures::executor::block_on(m.submit(order2, &venue)).unwrap();
    match out {
        SubmitOutcome::Acked { venue_order_id } => assert_eq!(venue_order_id, first_venue_id),
        other => panic!("expected Acked via AlreadyExists, got {other:?}"),
    }
    // No duplicate order at the venue.
    assert_eq!(venue.resting_orders().len(), 1);
}

#[test]
fn crash_resubmission_resolves_via_venue_already_exists() {
    // The full spec-5.4 story: timeout-but-placed, crash, rebuild, re-gate
    // the same intent (same client order id by construction), resubmit; the
    // venue answers ORDER_ALREADY_EXISTS and the intent acks to the
    // original order. Exactly one order ever exists at the venue.
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(
        FaultConfig {
            place_timeout_but_placed_pm: 1000,
            ..FaultConfig::none(7)
        },
        clock.clone(),
    );
    let mut m = manager(clock.clone());
    let order = gate(candidate(40, "M1", 40, 5), &clock);
    let intent = order.intent_id();
    let out = futures::executor::block_on(m.submit(order, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Unknown { .. }));

    // CRASH and rebuild: status folds back to Submitted, no venue id known.
    let journal = m.into_journal();
    let mut m2 = OrderManager::recover(journal, clock.clone(), ExecPolicy::default()).unwrap();
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::Submitted);

    // Resubmit the re-gated identical intent: AlreadyExists -> Acked.
    let order2 = gate(candidate(40, "M1", 40, 5), &clock);
    let out = futures::executor::block_on(m2.submit(order2, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Acked { .. }));
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::Acked);
    assert_eq!(venue.resting_orders().len(), 1);
}

#[test]
fn submit_timeout_leaves_intent_submitted_for_reconciliation() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(
        FaultConfig {
            place_timeout_but_placed_pm: 1000,
            ..FaultConfig::none(7)
        },
        clock.clone(),
    );
    let mut m = manager(clock.clone());
    let order = gate(candidate(3, "M1", 50, 5), &clock);
    let intent = order.intent_id();
    let out = futures::executor::block_on(m.submit(order, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Unknown { .. }));
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Submitted);
}

#[test]
fn submit_rejection_is_terminal() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(
        FaultConfig {
            place_reject_pm: 1000,
            ..FaultConfig::none(7)
        },
        clock.clone(),
    );
    let mut m = manager(clock.clone());
    let order = gate(candidate(4, "M1", 50, 5), &clock);
    let intent = order.intent_id();
    let out = futures::executor::block_on(m.submit(order, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Rejected { .. }));
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Rejected);
}

// ---- one working order per (strategy, market, side) ----

#[test]
fn second_working_order_on_same_key_is_refused() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());

    // Resting bid at 40 (does not cross the 55 ask).
    let o1 = gate(candidate(5, "M1", 40, 5), &clock);
    futures::executor::block_on(m.submit(o1, &venue)).unwrap();

    let o2 = gate(candidate(6, "M1", 41, 5), &clock);
    let err = futures::executor::block_on(m.submit(o2, &venue)).unwrap_err();
    assert!(matches!(err, ExecError::WorkingOrderExists { .. }));

    // After cancelling the first, the replacement goes through (re-quote).
    let first = m
        .working_order(&StrategyId::new("s1").unwrap(), &mkt("M1"), Side::Yes)
        .unwrap();
    futures::executor::block_on(m.cancel_intent(first, &venue)).unwrap();
    let o3 = gate(candidate(7, "M1", 41, 5), &clock);
    futures::executor::block_on(m.submit(o3, &venue)).unwrap();
}

// ---- fills ----

#[test]
fn fills_apply_exactly_once_and_advance_partial_to_filled() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());

    // Crossing buy: fills immediately at 55 x 5.
    let order = gate(candidate(8, "M1", 55, 5), &clock);
    let intent = order.intent_id();
    futures::executor::block_on(m.submit(order, &venue)).unwrap();

    let page = futures::executor::block_on(venue.fills_since(Cursor::start())).unwrap();
    assert_eq!(page.fills.len(), 1);
    let fill = &page.fills[0];

    let applied = m.ingest_fill(fill).unwrap();
    assert!(applied.applied);
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Filled);
    assert_eq!(m.intent(intent).unwrap().cum_filled.raw(), 5);

    // The same fill again (at-least-once delivery): ignored, state unchanged.
    let applied = m.ingest_fill(fill).unwrap();
    assert!(!applied.applied);
    assert_eq!(m.intent(intent).unwrap().cum_filled.raw(), 5);
}

#[test]
fn partial_fill_then_remainder() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());

    // Rest a bid at 50 for 10, then two public sells fill 4 and 6.
    let order = gate(candidate(9, "M1", 50, 10), &clock);
    let intent = order.intent_id();
    futures::executor::block_on(m.submit(order, &venue)).unwrap();
    venue
        .inject_public_order(&mkt("M1"), Side::Yes, Action::Sell, Cents::new(50), 4)
        .unwrap();
    venue
        .inject_public_order(&mkt("M1"), Side::Yes, Action::Sell, Cents::new(50), 6)
        .unwrap();

    let page = futures::executor::block_on(venue.fills_since(Cursor::start())).unwrap();
    assert_eq!(page.fills.len(), 2);
    m.ingest_fill(&page.fills[0]).unwrap();
    assert_eq!(
        m.intent(intent).unwrap().status,
        IntentStatus::PartiallyFilled
    );
    m.ingest_fill(&page.fills[1]).unwrap();
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Filled);
}

#[test]
fn fill_after_local_cancel_is_applied_and_audited() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());

    let order = gate(candidate(10, "M1", 50, 5), &clock);
    let intent = order.intent_id();
    futures::executor::block_on(m.submit(order, &venue)).unwrap();
    // Fill happens at the venue...
    venue
        .inject_public_order(&mkt("M1"), Side::Yes, Action::Sell, Cents::new(50), 5)
        .unwrap();
    // ...but we cancel locally before seeing it (cancel returns NotFound
    // because the order is already gone; the manager treats that as
    // cancelled pending reconciliation).
    futures::executor::block_on(m.cancel_intent(intent, &venue)).unwrap();
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Cancelled);

    // The late fill arrives: position truth wins; it is applied and the
    // journal carries the late-fill audit note.
    let page = futures::executor::block_on(venue.fills_since(Cursor::start())).unwrap();
    let applied = m.ingest_fill(&page.fills[0]).unwrap();
    assert!(applied.applied);
    assert!(applied.late_after_cancel);
    assert_eq!(m.intent(intent).unwrap().cum_filled.raw(), 5);
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Cancelled);
}

#[test]
fn overfill_is_an_error_not_a_silent_cap() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());
    let order = gate(candidate(11, "M1", 55, 5), &clock);
    futures::executor::block_on(m.submit(order, &venue)).unwrap();
    let page = futures::executor::block_on(venue.fills_since(Cursor::start())).unwrap();
    let mut forged = page.fills[0].clone();
    m.ingest_fill(&page.fills[0]).unwrap();
    // A second, different fill id pushing cum beyond qty: discrepancy.
    forged.fill_id = "forged-overfill".into();
    assert!(matches!(
        m.ingest_fill(&forged),
        Err(ExecError::Overfill { .. })
    ));
}

#[test]
fn orphan_fill_with_unknown_client_order_id_is_an_error() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());
    // Place directly at the venue, bypassing the manager (models another
    // process / manual order): its fill is an orphan to this journal.
    venue
        .place_raw(fortuna_venues::sim::PlaceOrder {
            market: mkt("M1"),
            side: Side::Yes,
            action: Action::Buy,
            limit_price: Cents::new(55),
            qty: Contracts::new(1),
            client_order_id: ClientOrderId::new("not-ours").unwrap(),
        })
        .unwrap();
    let page = futures::executor::block_on(venue.fills_since(Cursor::start())).unwrap();
    assert!(matches!(
        m.ingest_fill(&page.fills[0]),
        Err(ExecError::OrphanFill { .. })
    ));
}

// ---- cancel ----

#[test]
fn cancel_resting_intent() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());
    let order = gate(candidate(12, "M1", 40, 5), &clock);
    let intent = order.intent_id();
    futures::executor::block_on(m.submit(order, &venue)).unwrap();
    futures::executor::block_on(m.cancel_intent(intent, &venue)).unwrap();
    assert_eq!(m.intent(intent).unwrap().status, IntentStatus::Cancelled);
    assert!(venue.resting_orders().is_empty());
}

#[test]
fn cancel_unknown_intent_is_an_error() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());
    let mut g = IdGen::new(99);
    let ghost = IntentId::new(g.next(t0()).unwrap());
    assert!(matches!(
        futures::executor::block_on(m.cancel_intent(ghost, &venue)),
        Err(ExecError::UnknownIntent { .. })
    ));
}

// ---- TTL sweep ----

#[test]
fn ttl_sweep_cancels_only_expired_working_orders() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = OrderManager::recover(
        MemoryJournal::default(),
        clock.clone(),
        ExecPolicy {
            default_ttl_ms: 10_000,
            ..ExecPolicy::default()
        },
    )
    .unwrap();

    let o1 = gate(candidate(13, "M1", 40, 5), &clock);
    let i1 = o1.intent_id();
    futures::executor::block_on(m.submit(o1, &venue)).unwrap();

    clock.advance_millis(6_000).unwrap();
    venue.add_market(test_market("M2"));
    venue
        .set_book(&mkt("M2"), vec![level(45, 50)], vec![level(55, 50)])
        .unwrap();
    let mut c2 = candidate(14, "M2", 40, 5);
    c2.market = mkt("M2");
    let o2 = gate(c2, &clock);
    let i2 = o2.intent_id();
    futures::executor::block_on(m.submit(o2, &venue)).unwrap();

    // 6s later: i1 is 12s old (expired), i2 is 6s old (alive).
    clock.advance_millis(6_000).unwrap();
    let swept = futures::executor::block_on(m.sweep_ttl(&venue)).unwrap();
    assert_eq!(swept, vec![i1]);
    assert_eq!(m.intent(i1).unwrap().status, IntentStatus::Cancelled);
    assert_eq!(m.intent(i2).unwrap().status, IntentStatus::Acked);
}

// ---- boot reconciliation (crash recovery) ----

#[test]
fn boot_adopts_timeout_orphaned_acks_and_closes_unsubmitted() {
    let clock = Arc::new(SimClock::new(t0()));
    // Timeout fault: order IS at the venue but we never heard the ack.
    let venue = venue_with(
        FaultConfig {
            place_timeout_but_placed_pm: 1000,
            ..FaultConfig::none(7)
        },
        clock.clone(),
    );
    let mut m = manager(clock.clone());
    let order = gate(candidate(15, "M1", 40, 5), &clock);
    let intent = order.intent_id();
    let out = futures::executor::block_on(m.submit(order, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Unknown { .. }));

    // CRASH: rebuild the manager from the surviving journal.
    let journal = m.into_journal();
    let mut m2 = OrderManager::recover(journal, clock.clone(), ExecPolicy::default()).unwrap();
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::Submitted);

    let report = futures::executor::block_on(m2.boot_reconcile(&venue)).unwrap();
    assert_eq!(report.adopted, vec![intent]);
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::Acked);
    assert!(m2.intent(intent).unwrap().venue_order_id.is_some());
}

#[test]
fn boot_cancels_venue_orphans_and_reports_them() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    // An order the journal knows nothing about (another process / manual).
    venue
        .place_raw(fortuna_venues::sim::PlaceOrder {
            market: mkt("M1"),
            side: Side::Yes,
            action: Action::Buy,
            limit_price: Cents::new(40),
            qty: Contracts::new(3),
            client_order_id: ClientOrderId::new("foreign-order").unwrap(),
        })
        .unwrap();
    let mut m = manager(clock.clone());
    let report = futures::executor::block_on(m.boot_reconcile(&venue)).unwrap();
    assert_eq!(report.orphans_cancelled.len(), 1);
    assert!(venue.resting_orders().is_empty());
}

#[test]
fn boot_closes_created_but_never_submitted_intents() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());
    // Crash between persistence and submission: journal Created only.
    let order = gate(candidate(16, "M1", 40, 5), &clock);
    let intent = order.intent_id();
    m.journal_created_for_test(&order);
    drop(order);

    let journal = m.into_journal();
    let mut m2 = OrderManager::recover(journal, clock.clone(), ExecPolicy::default()).unwrap();
    let report = futures::executor::block_on(m2.boot_reconcile(&venue)).unwrap();
    assert_eq!(report.closed_unsubmitted, vec![intent]);
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::BootClosed);
}

#[test]
fn boot_resolves_submitted_intent_that_never_reached_the_venue() {
    let clock = Arc::new(SimClock::new(t0()));
    // Transient API error: the submit died en route; nothing at the venue.
    let venue = venue_with(
        FaultConfig {
            api_error_pm: 1000,
            ..FaultConfig::none(7)
        },
        clock.clone(),
    );
    let mut m = manager(clock.clone());
    let order = gate(candidate(17, "M1", 40, 5), &clock);
    let intent = order.intent_id();
    let out = futures::executor::block_on(m.submit(order, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Unknown { .. }));

    // Recovery against a now-healthy venue: not at venue, no fills -> closed.
    let healthy = venue_with(FaultConfig::none(2), clock.clone());
    let journal = m.into_journal();
    let mut m2 = OrderManager::recover(journal, clock.clone(), ExecPolicy::default()).unwrap();
    let report = futures::executor::block_on(m2.boot_reconcile(&healthy)).unwrap();
    assert_eq!(report.closed_unsubmitted, vec![intent]);
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::BootClosed);
}

#[test]
fn boot_applies_fills_that_happened_while_dead() {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());
    let order = gate(candidate(18, "M1", 50, 5), &clock);
    let intent = order.intent_id();
    futures::executor::block_on(m.submit(order, &venue)).unwrap();

    // We die; the market fills us while we're dead.
    venue
        .inject_public_order(&mkt("M1"), Side::Yes, Action::Sell, Cents::new(50), 5)
        .unwrap();

    let journal = m.into_journal();
    let mut m2 = OrderManager::recover(journal, clock.clone(), ExecPolicy::default()).unwrap();
    let report = futures::executor::block_on(m2.boot_reconcile(&venue)).unwrap();
    assert_eq!(report.fills_applied, 1);
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::Filled);
}

#[test]
fn recovery_rebuild_is_idempotent() {
    // Folding the same journal twice yields identical state (determinism).
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(FaultConfig::none(1), clock.clone());
    let mut m = manager(clock.clone());
    for n in 20..24 {
        let order = gate(candidate(n, "M1", 40 + (n as i64 % 3), 2), &clock);
        let _ = futures::executor::block_on(m.submit(order, &venue));
    }
    let journal = m.into_journal();
    let m1 = OrderManager::recover(journal.clone(), clock.clone(), ExecPolicy::default()).unwrap();
    let m2 = OrderManager::recover(journal, clock.clone(), ExecPolicy::default()).unwrap();
    assert_eq!(format!("{:?}", m1.intents()), format!("{:?}", m2.intents()));
}

#[test]
fn fill_after_boot_close_is_applied_and_flagged() {
    // The venue withheld a fill during the boot drain, boot closed the
    // intent, the fill arrives later: reality wins, status stays BootClosed.
    let clock = Arc::new(SimClock::new(t0()));
    let venue = venue_with(
        FaultConfig {
            api_error_pm: 1000, // submit dies en route; nothing at venue yet
            ..FaultConfig::none(7)
        },
        clock.clone(),
    );
    let mut m = manager(clock.clone());
    let order = gate(candidate(50, "M1", 50, 5), &clock);
    let intent = order.intent_id();
    let coid = order.client_order_id().clone();
    let out = futures::executor::block_on(m.submit(order, &venue)).unwrap();
    assert!(matches!(out, SubmitOutcome::Unknown { .. }));

    // Boot against a healthy venue closes it (no evidence).
    let healthy = venue_with(FaultConfig::none(2), clock.clone());
    let journal = m.into_journal();
    let mut m2 = OrderManager::recover(journal, clock.clone(), ExecPolicy::default()).unwrap();
    futures::executor::block_on(m2.boot_reconcile(&healthy)).unwrap();
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::BootClosed);

    // A fill for that coid arrives anyway (the order WAS placed somewhere
    // in reality): applied, flagged, status unchanged.
    healthy
        .place_raw(fortuna_venues::sim::PlaceOrder {
            market: mkt("M1"),
            side: Side::Yes,
            action: Action::Buy,
            limit_price: Cents::new(55),
            qty: Contracts::new(5),
            client_order_id: coid,
        })
        .unwrap();
    let page = futures::executor::block_on(healthy.fills_since(Cursor::start())).unwrap();
    let app = m2.ingest_fill(&page.fills[0]).unwrap();
    assert!(app.applied);
    assert!(app.late_after_cancel);
    assert_eq!(m2.intent(intent).unwrap().status, IntentStatus::BootClosed);
    assert_eq!(m2.intent(intent).unwrap().cum_filled.raw(), 5);
}
