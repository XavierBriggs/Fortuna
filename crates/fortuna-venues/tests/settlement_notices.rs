//! T1.4: the settlement-notice stream (spec 5.13 — "FORTUNA never assumes
//! settlement, only reconciles it"). Every venue exposes
//! `settlements_since(cursor)`: an at-least-once, cursor-paged stream of
//! authoritative settlement records (winner, void, correction). The sim
//! venue emits notices on settle/void/reversal so the runner's processor
//! and the DST can exercise every 5.13 path.
//!
//! Written BEFORE the implementation per the repository TDD doctrine.

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::market::{Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{
    Cursor, Market, MarketStatus, SettlementMeta, SettlementOutcome, Venue, VenueError,
};
use futures::executor::block_on;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap()
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

fn market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("m {id}"),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 1,
        },
        volume_contracts: Some(100),
        payout_per_contract: Cents::new(100),
    }
}

fn venue() -> SimVenue {
    let clock = Arc::new(SimClock::new(t0()));
    let v = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock,
        fee_model(),
        FaultConfig::none(7),
        Cents::new(100_000),
    );
    v.add_market(market("KXA"));
    v.add_market(market("KXB"));
    v
}

#[test]
fn settle_emits_a_winner_notice_and_the_cursor_advances() {
    let v = venue();
    v.settle_market(&mkt("KXA"), Side::Yes).unwrap();

    let page = block_on(v.settlements_since(Cursor::start())).unwrap();
    assert_eq!(page.notices.len(), 1);
    let n = &page.notices[0];
    assert_eq!(n.market, mkt("KXA"));
    assert_eq!(n.outcome, SettlementOutcome::Winner(Side::Yes));
    assert!(!n.notice_id.is_empty());

    // At-least-once stream: re-polling from start re-serves; polling from
    // next_cursor is empty until something new settles.
    let again = block_on(v.settlements_since(Cursor::start())).unwrap();
    assert_eq!(again.notices.len(), 1);
    let after = block_on(v.settlements_since(page.next_cursor.clone())).unwrap();
    assert!(after.notices.is_empty());
    assert_eq!(after.next_cursor, page.next_cursor, "terminal page holds");

    v.settle_market(&mkt("KXB"), Side::No).unwrap();
    let more = block_on(v.settlements_since(page.next_cursor)).unwrap();
    assert_eq!(more.notices.len(), 1);
    assert_eq!(more.notices[0].outcome, SettlementOutcome::Winner(Side::No));
}

#[test]
fn void_refunds_position_cost_and_emits_a_voided_notice() {
    let v = venue();
    // Buy 10 YES @ market (cross the configured book).
    v.set_book(
        &mkt("KXA"),
        vec![fortuna_venues::PriceLevel {
            price: Cents::new(40),
            qty: Contracts::new(50),
        }],
        vec![fortuna_venues::PriceLevel {
            price: Cents::new(45),
            qty: Contracts::new(50),
        }],
    )
    .unwrap();
    let before = v.inspect_totals().0;

    // Trade via the venue's own test order path is heavyweight here; the
    // refund math is what matters, so seed a position directly.
    v.seed_position(&mkt("KXA"), 10, 0, Cents::new(450));

    let refund = v.void_market(&mkt("KXA")).unwrap();
    assert_eq!(refund, Cents::new(450), "void refunds exact cost basis");
    let after = v.inspect_totals().0;
    assert_eq!(
        after,
        before.checked_add(Cents::new(450)).unwrap(),
        "refund lands in venue cash"
    );

    let page = block_on(v.settlements_since(Cursor::start())).unwrap();
    assert_eq!(page.notices.len(), 1);
    assert_eq!(page.notices[0].outcome, SettlementOutcome::Voided);

    // A voided market cannot settle afterwards.
    assert!(v.settle_market(&mkt("KXA"), Side::Yes).is_err());
}

#[test]
fn reversal_claws_back_and_emits_a_corrected_notice() {
    let v = venue();
    v.seed_position(&mkt("KXA"), 10, 0, Cents::new(600));
    let cash_start = v.inspect_totals().0;

    // Settle YES: venue pays 10 x 100c.
    v.settle_market(&mkt("KXA"), Side::Yes).unwrap();
    assert_eq!(
        v.inspect_totals().0,
        cash_start.checked_add(Cents::new(1_000)).unwrap()
    );

    // Venue correction: actually NO won. Claw 1000, pay 0.
    v.reverse_settlement(&mkt("KXA"), Side::No).unwrap();
    assert_eq!(
        v.inspect_totals().0,
        cash_start,
        "claw 1000, re-pay 0 (we held no NO)"
    );

    let page = block_on(v.settlements_since(Cursor::start())).unwrap();
    assert_eq!(page.notices.len(), 2, "original + correction");
    assert_eq!(
        page.notices[0].outcome,
        SettlementOutcome::Winner(Side::Yes)
    );
    assert_eq!(page.notices[1].outcome, SettlementOutcome::Winner(Side::No));
    assert_ne!(
        page.notices[0].notice_id, page.notices[1].notice_id,
        "the correction is a NEW notice, not an edit"
    );

    // Reversing an unsettled market errors.
    assert!(v.reverse_settlement(&mkt("KXB"), Side::Yes).is_err());
}

#[test]
fn settlement_stream_honors_outage_faults() {
    let v = venue();
    v.set_outage_until(t0().checked_add_millis(60_000).unwrap());
    let err = block_on(v.settlements_since(Cursor::start())).unwrap_err();
    assert!(matches!(err, VenueError::Outage { .. }));
}
