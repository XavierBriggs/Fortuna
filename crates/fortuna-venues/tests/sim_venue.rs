//! T0.3 tests: sim venue with seeded fault injection. Written from spec 5.2
//! before implementation.
//!
//! The sim venue is the DST workhorse: configurable book, deterministic
//! matching against visible depth, YES/NO translation, balance reservation,
//! and seeded faults (place-timeout-but-placed, reject, ack delay, transient
//! fill drop, fill duplication, cancel ambiguity, outage). Same seed + same
//! call sequence => identical behavior.

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::market::{Action, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, PlaceOrder, SimVenue};
use fortuna_venues::{
    Cursor, Market, MarketFilter, MarketStatus, PriceLevel, SettlementMeta, Venue, VenueError,
};
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn level(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: fortuna_core::market::Contracts::new(qty),
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

fn test_market(id: &str, category: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("Test market {id}"),
        category: category.to_string(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "test".into(),
            resolution_source: "test-source".into(),
            expected_lag_hours: 1,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    }
}

/// Venue with one market, book yes_bids [45c x 10], yes_asks [55c x 10, 60c x 10],
/// $100.00 starting balance, and the given faults.
fn venue_with(faults: FaultConfig) -> (Arc<SimClock>, SimVenue) {
    let clock = Arc::new(SimClock::new(t0()));
    let v = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock.clone(),
        fee_model(),
        faults,
        Cents::new(10_000),
    );
    v.add_market(test_market("TEST-MKT", "weather"));
    v.set_book(
        &mkt("TEST-MKT"),
        vec![level(45, 10)],
        vec![level(55, 10), level(60, 10)],
    )
    .unwrap();
    (clock, v)
}

fn venue() -> (Arc<SimClock>, SimVenue) {
    venue_with(FaultConfig::none(1))
}

fn order(coid: &str, side: Side, action: Action, price: i64, qty: i64) -> PlaceOrder {
    PlaceOrder {
        market: mkt("TEST-MKT"),
        side,
        action,
        limit_price: Cents::new(price),
        qty: fortuna_core::market::Contracts::new(qty),
        client_order_id: fortuna_core::market::ClientOrderId::new(coid).unwrap(),
    }
}

fn drain_fills(v: &SimVenue) -> Vec<fortuna_venues::Fill> {
    let mut out = Vec::new();
    let mut cursor = Cursor::start();
    loop {
        let page = futures::executor::block_on(v.fills_since(cursor.clone())).unwrap();
        if page.fills.is_empty() && page.next_cursor == cursor {
            return out;
        }
        cursor = page.next_cursor.clone();
        out.extend(page.fills);
        if out.len() > 1000 {
            panic!("fill drain did not terminate");
        }
    }
}

// ---- matching ----

#[test]
fn buy_yes_crossing_fills_through_visible_depth() {
    let (_c, v) = venue();
    let id = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 60, 15))
        .unwrap();
    let fills = drain_fills(&v);
    assert_eq!(fills.len(), 2);
    assert_eq!((fills[0].price, fills[0].qty.raw()), (Cents::new(55), 10));
    assert_eq!((fills[1].price, fills[1].qty.raw()), (Cents::new(60), 5));
    assert!(fills.iter().all(|f| !f.is_maker));
    assert!(fills.iter().all(|f| f.venue_order_id == id));
    // fee(55c x 10) = ceil(17.325) = 18; fee(60c x 5) = ceil(8.4) = 9.
    assert_eq!(fills[0].fee, Cents::new(18));
    assert_eq!(fills[1].fee, Cents::new(9));
    // Balance: 10_000 - (550 + 300) - (18 + 9) = 9_123.
    assert_eq!(
        futures::executor::block_on(v.balance()).unwrap(),
        Cents::new(9_123)
    );
    // Position: +15 yes.
    let pos = futures::executor::block_on(v.positions()).unwrap();
    assert_eq!(pos.len(), 1);
    assert_eq!((pos[0].yes, pos[0].no), (15, 0));
    // Visible depth consumed: 55-level gone, 5 left at 60.
    let book = futures::executor::block_on(v.book(&mkt("TEST-MKT"))).unwrap();
    assert_eq!(book.yes_asks, vec![level(60, 5)]);
}

#[test]
fn partial_fill_rests_the_remainder_as_maker() {
    let (_c, v) = venue();
    let id = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 55, 15))
        .unwrap();
    let fills = drain_fills(&v);
    assert_eq!(fills.len(), 1);
    assert_eq!((fills[0].price, fills[0].qty.raw()), (Cents::new(55), 10));
    let resting = v.resting_orders();
    assert_eq!(resting.len(), 1);
    assert_eq!(resting[0].0, id);
    assert_eq!(resting[0].1.qty.raw(), 5);
}

#[test]
fn resting_order_fills_when_a_public_trade_crosses_it() {
    let (_c, v) = venue();
    // Rest a yes bid at 50 (does not cross the 55 ask).
    v.place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    assert!(drain_fills(&v).is_empty());
    // Public sell crossing down to 48 fills us at OUR price (50), maker.
    v.inject_public_order(&mkt("TEST-MKT"), Side::Yes, Action::Sell, Cents::new(48), 8)
        .unwrap();
    let fills = drain_fills(&v);
    assert_eq!(fills.len(), 1);
    assert_eq!((fills[0].price, fills[0].qty.raw()), (Cents::new(50), 5));
    assert!(fills[0].is_maker);
    // Maker fee 0.0175 * 5 * 0.5 * 0.5 = $0.021875 -> 2.1875c -> 3c.
    assert_eq!(fills[0].fee, Cents::new(3));
}

#[test]
fn same_price_resting_orders_fill_in_placement_order() {
    let (_c, v) = venue();
    let first = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    let second = v
        .place_raw(order("c2", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    v.inject_public_order(&mkt("TEST-MKT"), Side::Yes, Action::Sell, Cents::new(50), 5)
        .unwrap();
    let fills = drain_fills(&v);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].venue_order_id, first);
    assert_eq!(v.resting_orders()[0].0, second);
}

#[test]
fn buy_no_translates_against_yes_bids() {
    let (_c, v) = venue();
    // NO ask liquidity = 100 - yes_bid = 100 - 45 = 55 <= limit 60 -> fill.
    v.place_raw(order("c1", Side::No, Action::Buy, 60, 10))
        .unwrap();
    let fills = drain_fills(&v);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].side, Side::No);
    assert_eq!((fills[0].price, fills[0].qty.raw()), (Cents::new(55), 10));
    // Position: long 10 NO (tracked separately from YES).
    let pos = futures::executor::block_on(v.positions()).unwrap();
    assert_eq!((pos[0].yes, pos[0].no), (0, 10));
    // Yes bid consumed.
    let book = futures::executor::block_on(v.book(&mkt("TEST-MKT"))).unwrap();
    assert!(book.yes_bids.is_empty());
}

#[test]
fn sell_requires_a_held_position() {
    let (_c, v) = venue();
    let err = v
        .place_raw(order("c1", Side::Yes, Action::Sell, 40, 5))
        .unwrap_err();
    assert!(matches!(err, VenueError::Rejected { .. }));
    // Buy 10 yes, then selling 5 into the 45 bid works.
    v.place_raw(order("c2", Side::Yes, Action::Buy, 55, 10))
        .unwrap();
    v.place_raw(order("c3", Side::Yes, Action::Sell, 45, 5))
        .unwrap();
    let pos = futures::executor::block_on(v.positions()).unwrap();
    assert_eq!((pos[0].yes, pos[0].no), (5, 0));
}

#[test]
fn unknown_market_is_not_found() {
    let (_c, v) = venue();
    let mut o = order("c1", Side::Yes, Action::Buy, 50, 5);
    o.market = mkt("NO-SUCH-MKT");
    assert!(matches!(v.place_raw(o), Err(VenueError::NotFound { .. })));
}

// ---- balance reservation ----

#[test]
fn orders_beyond_available_balance_are_rejected() {
    let clock = Arc::new(SimClock::new(t0()));
    let v = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock,
        fee_model(),
        FaultConfig::none(1),
        Cents::new(100), // $1.00
    );
    v.add_market(test_market("TEST-MKT", "weather"));
    v.set_book(&mkt("TEST-MKT"), vec![], vec![level(55, 10)])
        .unwrap();
    // Worst case cost 10 x 50c = 500c > 100c available.
    let err = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 10))
        .unwrap_err();
    assert!(matches!(err, VenueError::Rejected { .. }));
}

#[test]
fn resting_orders_reserve_funds_and_cancel_releases_them() {
    let (_c, v) = venue();
    let before = futures::executor::block_on(v.balance()).unwrap();
    let id = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    let during = futures::executor::block_on(v.balance()).unwrap();
    // Reserved: 5 x 50c = 250 plus worst-case taker fee ceil(0.07*5*0.25*100)
    // = ceil(8.75) = 9 -> 259.
    assert_eq!(before.checked_sub(during).unwrap(), Cents::new(259));
    futures::executor::block_on(v.cancel(&id)).unwrap();
    assert_eq!(futures::executor::block_on(v.balance()).unwrap(), before);
}

// ---- fills cursor ----

#[test]
fn fills_since_paginates_with_a_stable_cursor() {
    let (_c, v) = venue();
    v.place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap();
    let page1 = futures::executor::block_on(v.fills_since(Cursor::start())).unwrap();
    assert_eq!(page1.fills.len(), 1);
    v.place_raw(order("c2", Side::Yes, Action::Buy, 60, 10))
        .unwrap();
    let page2 = futures::executor::block_on(v.fills_since(page1.next_cursor.clone())).unwrap();
    assert_eq!(page2.fills.len(), 1);
    assert_ne!(page1.fills[0].fill_id, page2.fills[0].fill_id);
    // Re-reading from the same cursor is idempotent when no faults are armed.
    let again = futures::executor::block_on(v.fills_since(page1.next_cursor)).unwrap();
    assert_eq!(again.fills.len(), 1);
    assert_eq!(again.fills[0].fill_id, page2.fills[0].fill_id);
}

// ---- idempotency ----

#[test]
fn duplicate_client_order_id_is_refused_with_the_existing_order() {
    // Kalshi rejects duplicate client_order_ids with ORDER_ALREADY_EXISTS
    // (docs/research/venue/kalshi-fees-2026-06-09). The sim mirrors that;
    // exec treats it as success-equivalent on crash resubmission.
    let (_c, v) = venue();
    let a = v
        .place_raw(order("same", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    let balance_after_first = futures::executor::block_on(v.balance()).unwrap();
    match v
        .place_raw(order("same", Side::Yes, Action::Buy, 50, 5))
        .unwrap_err()
    {
        VenueError::AlreadyExists { existing } => assert_eq!(existing, a),
        other => panic!("expected AlreadyExists, got {other:?}"),
    }
    assert_eq!(
        futures::executor::block_on(v.balance()).unwrap(),
        balance_after_first
    );
    assert_eq!(v.resting_orders().len(), 1);
}

// ---- cancel ----

#[test]
fn cancel_removes_resting_order_and_unknown_is_not_found() {
    let (_c, v) = venue();
    let id = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    futures::executor::block_on(v.cancel(&id)).unwrap();
    assert!(v.resting_orders().is_empty());
    assert!(matches!(
        futures::executor::block_on(v.cancel(&id)),
        Err(VenueError::NotFound { .. })
    ));
}

// ---- settlement ----

#[test]
fn settle_pays_winning_positions_and_clears_them() {
    let (_c, v) = venue();
    v.place_raw(order("c1", Side::Yes, Action::Buy, 60, 15))
        .unwrap();
    let before = futures::executor::block_on(v.balance()).unwrap();
    let payout = v.settle_market(&mkt("TEST-MKT"), Side::Yes).unwrap();
    assert_eq!(payout, Cents::new(1_500)); // 15 x $1
    assert_eq!(
        futures::executor::block_on(v.balance()).unwrap(),
        before.checked_add(payout).unwrap()
    );
    assert!(futures::executor::block_on(v.positions())
        .unwrap()
        .is_empty());
    // Settling again: market no longer settleable.
    assert!(v.settle_market(&mkt("TEST-MKT"), Side::Yes).is_err());
}

#[test]
fn settle_pays_each_side_independently_pair_value_preserved() {
    // Hold 10 YES (cost 550 + 18 fee) AND 10 NO. NO liquidity comes from the
    // yes-bid mirror at 100-45=55 (cost 550 + fee 18). A YES settlement pays
    // the YES lot 1000; the NO lot pays 0. The pair's $1 value is never
    // netted away.
    let (_c, v) = venue();
    v.place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap();
    v.place_raw(order("c2", Side::No, Action::Buy, 55, 10))
        .unwrap();
    let pos = futures::executor::block_on(v.positions()).unwrap();
    assert_eq!((pos[0].yes, pos[0].no), (10, 10));
    let payout = v.settle_market(&mkt("TEST-MKT"), Side::Yes).unwrap();
    assert_eq!(payout, Cents::new(1_000));
}

#[test]
fn settle_pays_nothing_to_the_losing_side() {
    let (_c, v) = venue();
    v.place_raw(order("c1", Side::No, Action::Buy, 60, 10))
        .unwrap(); // long 10 NO
    let before = futures::executor::block_on(v.balance()).unwrap();
    let payout = v.settle_market(&mkt("TEST-MKT"), Side::Yes).unwrap();
    assert_eq!(payout, Cents::ZERO);
    assert_eq!(futures::executor::block_on(v.balance()).unwrap(), before);
}

// ---- book hygiene ----

#[test]
fn set_book_rejects_crossed_unsorted_or_nonpositive_books() {
    let (_c, v) = venue();
    // Crossed: bid 60 >= ask 55.
    assert!(v
        .set_book(&mkt("TEST-MKT"), vec![level(60, 5)], vec![level(55, 5)])
        .is_err());
    // Unsorted bids (must be descending).
    assert!(v
        .set_book(
            &mkt("TEST-MKT"),
            vec![level(40, 5), level(45, 5)],
            vec![level(55, 5)]
        )
        .is_err());
    // Non-positive quantity.
    assert!(v
        .set_book(&mkt("TEST-MKT"), vec![level(45, 0)], vec![])
        .is_err());
}

// ---- markets and trait object ----

#[test]
fn markets_filter_by_category_through_the_venue_trait() {
    let (_c, v) = venue();
    v.add_market(test_market("OTHER-MKT", "politics"));
    let dyn_v: &dyn Venue = &v;
    let all = futures::executor::block_on(dyn_v.markets(MarketFilter::default())).unwrap();
    assert_eq!(all.len(), 2);
    let weather = futures::executor::block_on(dyn_v.markets(MarketFilter {
        category: Some("weather".into()),
        status: None,
    }))
    .unwrap();
    assert_eq!(weather.len(), 1);
    assert_eq!(weather[0].id, mkt("TEST-MKT"));
    // Fee model reachable through the trait.
    let _fm = dyn_v.fee_model();
}

// ---- faults (all seeded, deterministic) ----

#[test]
fn fault_place_timeout_but_placed_leaves_the_order_live() {
    let (_c, v) = venue_with(FaultConfig {
        place_timeout_but_placed_pm: 1000,
        ..FaultConfig::none(7)
    });
    // Resting variant: timeout returned, order actually resting.
    let err = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .unwrap_err();
    assert!(matches!(err, VenueError::Timeout { .. }));
    assert_eq!(v.resting_orders().len(), 1);
}

#[test]
fn fault_place_timeout_on_crossing_order_still_fills() {
    let (_c, v) = venue_with(FaultConfig {
        place_timeout_but_placed_pm: 1000,
        ..FaultConfig::none(7)
    });
    let err = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap_err();
    assert!(matches!(err, VenueError::Timeout { .. }));
    // The fill exists and is discoverable by polling: the classic
    // crash-between-submission-and-ack scenario.
    assert_eq!(drain_fills(&v).len(), 1);
}

#[test]
fn fault_clean_reject_places_nothing() {
    let (_c, v) = venue_with(FaultConfig {
        place_reject_pm: 1000,
        ..FaultConfig::none(7)
    });
    let err = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap_err();
    assert!(matches!(err, VenueError::Rejected { .. }));
    assert!(v.resting_orders().is_empty());
    assert!(drain_fills(&v).is_empty());
}

#[test]
fn fault_ack_delay_defers_matching_until_tick() {
    let (_c, v) = venue_with(FaultConfig {
        ack_delay_pm: 1000,
        ..FaultConfig::none(7)
    });
    v.place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap();
    assert!(drain_fills(&v).is_empty()); // accepted but not yet processed
    v.tick().unwrap();
    assert_eq!(drain_fills(&v).len(), 1);
}

#[test]
fn fault_ack_delay_cancel_race_kills_the_pending_order() {
    let (_c, v) = venue_with(FaultConfig {
        ack_delay_pm: 1000,
        ..FaultConfig::none(7)
    });
    let id = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap();
    futures::executor::block_on(v.cancel(&id)).unwrap();
    v.tick().unwrap();
    assert!(drain_fills(&v).is_empty()); // cancel won the race
}

#[test]
fn fault_dropped_fill_is_delivered_on_a_later_poll() {
    let (_c, v) = venue_with(FaultConfig {
        drop_fill_pm: 1000,
        ..FaultConfig::none(7)
    });
    v.place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap();
    let page1 = futures::executor::block_on(v.fills_since(Cursor::start())).unwrap();
    assert!(page1.fills.is_empty()); // withheld this poll
    let page2 = futures::executor::block_on(v.fills_since(page1.next_cursor)).unwrap();
    assert_eq!(page2.fills.len(), 1); // delivered late
}

#[test]
fn fault_duplicated_fill_is_delivered_twice() {
    let (_c, v) = venue_with(FaultConfig {
        dup_fill_pm: 1000,
        ..FaultConfig::none(7)
    });
    v.place_raw(order("c1", Side::Yes, Action::Buy, 55, 10))
        .unwrap();
    let page1 = futures::executor::block_on(v.fills_since(Cursor::start())).unwrap();
    assert_eq!(page1.fills.len(), 1);
    let page2 = futures::executor::block_on(v.fills_since(page1.next_cursor)).unwrap();
    assert_eq!(page2.fills.len(), 1);
    assert_eq!(page1.fills[0].fill_id, page2.fills[0].fill_id); // same fill, twice
    let page3 = futures::executor::block_on(v.fills_since(page2.next_cursor)).unwrap();
    assert!(page3.fills.is_empty()); // duplication happens once
}

#[test]
fn fault_cancel_timeout_ambiguity_both_arms() {
    // Arm A: timeout reported but the cancel actually happened.
    let (_c, v) = venue_with(FaultConfig {
        cancel_timeout_cancelled_pm: 1000,
        ..FaultConfig::none(7)
    });
    let id = v
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    assert!(matches!(
        futures::executor::block_on(v.cancel(&id)),
        Err(VenueError::Timeout { .. })
    ));
    assert!(v.resting_orders().is_empty());

    // Arm B: timeout reported and the order is still live.
    let (_c2, v2) = venue_with(FaultConfig {
        cancel_timeout_not_cancelled_pm: 1000,
        ..FaultConfig::none(7)
    });
    let id2 = v2
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .unwrap();
    assert!(matches!(
        futures::executor::block_on(v2.cancel(&id2)),
        Err(VenueError::Timeout { .. })
    ));
    assert_eq!(v2.resting_orders().len(), 1);
}

#[test]
fn outage_window_fails_all_calls_until_the_clock_passes() {
    let (clock, v) = venue();
    let until = t0().checked_add_millis(5_000).unwrap();
    v.set_outage_until(until);
    assert!(matches!(
        v.place_raw(order("c1", Side::Yes, Action::Buy, 50, 5)),
        Err(VenueError::Outage { .. })
    ));
    assert!(matches!(
        futures::executor::block_on(v.book(&mkt("TEST-MKT"))),
        Err(VenueError::Outage { .. })
    ));
    clock.advance_millis(5_000).unwrap();
    assert!(v
        .place_raw(order("c1", Side::Yes, Action::Buy, 50, 5))
        .is_ok());
}

#[test]
fn same_seed_and_call_sequence_produce_identical_behavior() {
    let run = || {
        let (_c, v) = venue_with(FaultConfig {
            place_timeout_but_placed_pm: 300,
            place_reject_pm: 200,
            ack_delay_pm: 300,
            drop_fill_pm: 400,
            dup_fill_pm: 300,
            ..FaultConfig::none(99)
        });
        let mut log = String::new();
        for i in 0..20 {
            let res = v.place_raw(order(
                &format!("c{i}"),
                if i % 2 == 0 { Side::Yes } else { Side::No },
                Action::Buy,
                50 + (i % 3),
                1 + (i % 4),
            ));
            log.push_str(&format!("{i}:{res:?};"));
            if i % 5 == 0 {
                v.tick().unwrap();
            }
        }
        v.tick().unwrap();
        for f in drain_fills(&v) {
            log.push_str(&format!("{}@{}x{};", f.fill_id, f.price, f.qty));
        }
        log.push_str(&format!(
            "bal={}",
            futures::executor::block_on(v.balance()).unwrap()
        ));
        log
    };
    assert_eq!(run(), run());
}
