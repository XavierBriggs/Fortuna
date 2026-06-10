//! T0.6 tests: IntentGroup completion policy + flatten planner. Spec 5.4.

use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentGroupId, IntentId};
use fortuna_core::market::{Action, Contracts, MarketId, Side};
use fortuna_core::money::Cents;
use fortuna_exec::{
    decide_complete_or_unwind, plan_flatten, CompleteOrUnwind, FlattenDecision, GroupDecision,
    GroupPolicy, GroupStatus, GroupTracker, IntentRecord, IntentStatus, IntentView, OrderSnapshot,
    RemainingLeg,
};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn ts(offset_ms: i64) -> UtcTimestamp {
    t0().checked_add_millis(offset_ms).unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn fees() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
        "#,
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn book(market: &str, bids: Vec<(i64, i64)>, asks: Vec<(i64, i64)>) -> OrderBook {
    OrderBook {
        market: mkt(market),
        as_of: t0(),
        yes_bids: bids
            .into_iter()
            .map(|(p, q)| PriceLevel {
                price: Cents::new(p),
                qty: Contracts::new(q),
            })
            .collect(),
        yes_asks: asks
            .into_iter()
            .map(|(p, q)| PriceLevel {
                price: Cents::new(p),
                qty: Contracts::new(q),
            })
            .collect(),
    }
}

/// Hand-rolled IntentView with controllable records.
#[derive(Default)]
struct FakeView {
    records: BTreeMap<IntentId, IntentRecord>,
}

impl IntentView for FakeView {
    fn intent_record(&self, id: IntentId) -> Option<&IntentRecord> {
        self.records.get(&id)
    }
}

fn record(
    intent: IntentId,
    market: &str,
    price: i64,
    qty: i64,
    filled: i64,
    status: IntentStatus,
) -> IntentRecord {
    IntentRecord {
        order: OrderSnapshot {
            intent_id: intent,
            strategy: fortuna_core::market::StrategyId::new("s1").unwrap(),
            venue: fortuna_core::market::VenueId::new("sim").unwrap(),
            market: mkt(market),
            side: Side::Yes,
            action: Action::Buy,
            limit_price: Cents::new(price),
            qty: Contracts::new(qty),
            client_order_id: fortuna_core::market::ClientOrderId::from_intent(intent),
        },
        group: None,
        status,
        venue_order_id: None,
        cum_filled: Contracts::new(filled),
        cancel_requested: false,
        created_at: t0(),
        last_event_at: t0(),
    }
}

fn ids(n: usize) -> Vec<IntentId> {
    let mut g = IdGen::new(42);
    (0..n)
        .map(|_| IntentId::new(g.next(t0()).unwrap()))
        .collect()
}

fn policy() -> GroupPolicy {
    GroupPolicy {
        max_unhedged_notional: Cents::new(500),
        max_leg_open_ms: 30_000,
        value_per_set: Cents::new(100),
        min_completion_edge_bps: 100,
    }
}

// ---- group tracker ----

#[test]
fn fully_filled_group_completes() {
    let leg_ids = ids(3);
    let mut view = FakeView::default();
    view.records.insert(
        leg_ids[0],
        record(leg_ids[0], "A", 40, 10, 10, IntentStatus::Filled),
    );
    view.records.insert(
        leg_ids[1],
        record(leg_ids[1], "B", 35, 10, 10, IntentStatus::Filled),
    );
    let group_id = IntentGroupId::new(ids(1)[0].ulid());
    let mut tracker = GroupTracker::default();
    tracker.open(group_id, policy(), vec![leg_ids[0], leg_ids[1]], t0());
    let decisions = tracker.evaluate(&view, ts(1_000));
    assert!(matches!(
        decisions.as_slice(),
        [GroupDecision::Complete { .. }]
    ));
    assert_eq!(
        tracker.group(group_id).unwrap().status,
        GroupStatus::Complete
    );
}

#[test]
fn unhedged_notional_breach_triggers_before_duration() {
    let leg_ids = ids(2);
    let mut view = FakeView::default();
    // Leg A filled 600c of notional; leg B nothing: spread 600 > 500 cap.
    view.records.insert(
        leg_ids[0],
        record(leg_ids[0], "A", 60, 10, 10, IntentStatus::Filled),
    );
    view.records.insert(
        leg_ids[1],
        record(leg_ids[1], "B", 30, 10, 0, IntentStatus::Acked),
    );
    let group_id = IntentGroupId::new(ids(1)[0].ulid());
    let mut tracker = GroupTracker::default();
    tracker.open(group_id, policy(), leg_ids.clone(), t0());
    // Well within the duration bound: the notional bound trips first.
    let decisions = tracker.evaluate(&view, ts(1_000));
    match decisions.as_slice() {
        [GroupDecision::Breached {
            reason,
            unfilled_legs,
            ..
        }] => {
            assert!(reason.contains("unhedged notional"));
            assert_eq!(unfilled_legs, &vec![leg_ids[1]]);
        }
        other => panic!("expected breach, got {other:?}"),
    }
}

#[test]
fn duration_breach_triggers_when_balanced_but_slow() {
    let leg_ids = ids(2);
    let mut view = FakeView::default();
    // Balanced partial fills (no notional breach), but time passes.
    view.records.insert(
        leg_ids[0],
        record(leg_ids[0], "A", 40, 10, 5, IntentStatus::PartiallyFilled),
    );
    view.records.insert(
        leg_ids[1],
        record(leg_ids[1], "B", 40, 10, 5, IntentStatus::PartiallyFilled),
    );
    let group_id = IntentGroupId::new(ids(1)[0].ulid());
    let mut tracker = GroupTracker::default();
    tracker.open(group_id, policy(), leg_ids, t0());
    assert!(tracker.evaluate(&view, ts(29_000)).is_empty()); // within bound
    let decisions = tracker.evaluate(&view, ts(31_000));
    match decisions.as_slice() {
        [GroupDecision::Breached { reason, .. }] => assert!(reason.contains("leg open")),
        other => panic!("expected duration breach, got {other:?}"),
    }
}

// ---- complete-or-unwind ----

#[test]
fn taker_complete_when_edge_still_clears_floor() {
    // Remaining: buy 10 NO on market B. Yes bids at 62 x 20 -> NO price 38.
    // Cost = 380 + fee(38c, 10 taker: 0.07*10*0.38*0.62 = $0.164920 -> 17c)
    // = 397. Value = 10 sets x 100 = 1000... but the other leg already cost
    // its fill; value_per_set here prices ONLY remaining-completion value:
    // for this test policy value_per_set=45 means value 450 vs cost 397:
    // net 53 on 397 = 1335 bps >= 100 -> complete.
    let books = BTreeMap::from([(mkt("B"), book("B", vec![(62, 20)], vec![(70, 5)]))]);
    let remaining = vec![RemainingLeg {
        market: mkt("B"),
        side: Side::No,
        action: Action::Buy,
        remaining: Contracts::new(10),
    }];
    let mut p = policy();
    p.value_per_set = Cents::new(45);
    let decision = decide_complete_or_unwind(&remaining, &books, &fees(), &p, t0());
    match decision {
        CompleteOrUnwind::TakerComplete {
            est_cost,
            net_edge_bps,
        } => {
            assert_eq!(est_cost, Cents::new(397));
            assert_eq!(net_edge_bps, 1335);
        }
        other => panic!("expected TakerComplete, got {other:?}"),
    }
}

#[test]
fn unwind_when_completion_edge_is_gone() {
    // Same leg, but the book moved: yes bids at 50 -> NO costs 50.
    // Cost = 500 + fee(50c,10: ceil(17.5)=18) = 518 > value 450 -> unwind.
    let books = BTreeMap::from([(mkt("B"), book("B", vec![(50, 20)], vec![(70, 5)]))]);
    let remaining = vec![RemainingLeg {
        market: mkt("B"),
        side: Side::No,
        action: Action::Buy,
        remaining: Contracts::new(10),
    }];
    let mut p = policy();
    p.value_per_set = Cents::new(45);
    let decision = decide_complete_or_unwind(&remaining, &books, &fees(), &p, t0());
    assert!(matches!(decision, CompleteOrUnwind::Unwind { .. }));
}

#[test]
fn unwind_when_depth_is_insufficient_or_book_missing() {
    let remaining = vec![RemainingLeg {
        market: mkt("B"),
        side: Side::No,
        action: Action::Buy,
        remaining: Contracts::new(10),
    }];
    // Missing book.
    let decision =
        decide_complete_or_unwind(&remaining, &BTreeMap::new(), &fees(), &policy(), t0());
    assert!(matches!(decision, CompleteOrUnwind::Unwind { .. }));
    // Thin book: only 3 contracts of the 10 available.
    let books = BTreeMap::from([(mkt("B"), book("B", vec![(62, 3)], vec![]))]);
    let decision = decide_complete_or_unwind(&remaining, &books, &fees(), &policy(), t0());
    match decision {
        CompleteOrUnwind::Unwind { reason } => assert!(reason.contains("insufficient depth")),
        other => panic!("expected Unwind, got {other:?}"),
    }
}

// ---- flatten planner ----

#[test]
fn flatten_plan_walks_depth_and_prices_the_exit() {
    // Long 30 YES on A. Bids: 48 x 20, 45 x 20.
    // Walk: 20@48 = 960 - fee(48,20: 0.07*20*0.48*0.52 = $0.349440 -> 35) = 925
    //     + 10@45 = 450 - fee(45,10: 0.07*10*0.45*0.55 = $0.173250 -> 18) = 432
    // proceeds = 1357. Mark = touch 48 x 30 = 1440. Cost vs mark = 83.
    let books = BTreeMap::from([(mkt("A"), book("A", vec![(48, 20), (45, 20)], vec![(55, 5)]))]);
    let plan = plan_flatten(&[(mkt("A"), 30)], &books, &fees(), t0());
    assert_eq!(plan.legs.len(), 1);
    assert_eq!(plan.total_est_proceeds, Cents::new(1_357));
    assert_eq!(plan.total_mark_value, Cents::new(1_440));
    assert_eq!(plan.est_cost_vs_mark, Cents::new(83));
    assert!(!plan.any_unfillable);
    // Decision: bound 100 -> auto; bound 50 -> operator confirm.
    assert_eq!(plan.decide(Cents::new(100)), FlattenDecision::AutoFlatten);
    assert_eq!(
        plan.decide(Cents::new(50)),
        FlattenDecision::NeedsOperatorConfirm
    );
}

#[test]
fn flatten_short_position_exits_through_mirrored_asks() {
    // Long 10 NO (net -10). Yes asks 55 x 20 -> NO bid 45.
    // Proceeds = 450 - fee(45c,10: 18) = 432; mark = 45 x 10 = 450; cost 18.
    let books = BTreeMap::from([(mkt("A"), book("A", vec![(40, 5)], vec![(55, 20)]))]);
    let plan = plan_flatten(&[(mkt("A"), -10)], &books, &fees(), t0());
    assert_eq!(plan.total_est_proceeds, Cents::new(432));
    assert_eq!(plan.total_mark_value, Cents::new(450));
    assert_eq!(plan.est_cost_vs_mark, Cents::new(18));
}

#[test]
fn thin_book_forces_freeze_and_cancel() {
    // Long 30, only 5 contracts of bid depth: unfillable -> freeze.
    let books = BTreeMap::from([(mkt("A"), book("A", vec![(48, 5)], vec![]))]);
    let plan = plan_flatten(&[(mkt("A"), 30)], &books, &fees(), t0());
    assert!(plan.any_unfillable);
    assert!(matches!(
        plan.decide(Cents::new(1_000_000)),
        FlattenDecision::FreezeAndCancel { .. }
    ));
    // Missing book entirely: same.
    let plan = plan_flatten(&[(mkt("A"), 30)], &BTreeMap::new(), &fees(), t0());
    assert!(plan.any_unfillable);
}

#[test]
fn flat_positions_produce_an_empty_plan() {
    let plan = plan_flatten(&[(mkt("A"), 0)], &BTreeMap::new(), &fees(), t0());
    assert!(plan.legs.is_empty());
    assert_eq!(plan.est_cost_vs_mark, Cents::ZERO);
    assert_eq!(plan.decide(Cents::ZERO), FlattenDecision::AutoFlatten);
}
