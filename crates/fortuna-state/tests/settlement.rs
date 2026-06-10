//! T1.4: settlement entry lifecycle (spec 5.13) + position reversal.
//!
//! Doctrine under test:
//! - Settlement entries move pending -> posted -> confirmed | reversed,
//!   and every transition is a NEW entry superseding the old one (the
//!   append-only Pg table refuses UPDATE; the in-memory ledger mirrors
//!   that shape exactly). Illegal transitions error, never coerce.
//! - A venue reversal restores the EXACT pre-settlement lots (quantity
//!   and basis), claws back the payout, and unwinds realized PnL to the
//!   cent — then a corrected re-settlement produces the same result it
//!   would have produced the first time (conservation through the chain).
//! - capital_in_limbo reports pending+posted value (lifecycle metric).
//!
//! Written BEFORE src/settlement.rs per the repository TDD doctrine.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_state::{
    PositionBook, SettlementLedger, SettlementSnapshot, SettlementStatus, StateError,
};
use fortuna_venues::Fill;

fn t(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(1_770_000_000_000 + ms).unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn fill(market: &str, side: Side, action: Action, price: i64, qty: i64, n: u64) -> Fill {
    Fill {
        fill_id: format!("f-{n}"),
        venue_order_id: fortuna_core::market::VenueOrderId::new(format!("v-{n}")).unwrap(),
        client_order_id: fortuna_core::market::ClientOrderId::new(format!("c-{n}")).unwrap(),
        market: mkt(market),
        side,
        action,
        price: Cents::new(price),
        qty: fortuna_core::market::Contracts::new(qty),
        fee: Cents::new(1),
        at: t(0),
        is_maker: true,
    }
}

// ------------------------------------------------------- the entry chain

#[test]
fn entries_advance_by_superseding_rows() {
    let mut ledger = SettlementLedger::new();
    let e1 = ledger
        .record_pending(
            "s-1".to_string(),
            mkt("KXA"),
            VenueId::new("sim").unwrap(),
            Cents::new(1_000),
            serde_json::json!({"winner": "Yes"}),
            t(0),
        )
        .unwrap();
    assert_eq!(
        ledger.head(&mkt("KXA")).unwrap().status,
        SettlementStatus::Pending
    );

    let e2 = ledger
        .advance(
            "s-2".to_string(),
            &mkt("KXA"),
            SettlementStatus::Posted,
            t(1),
        )
        .unwrap();
    let head = ledger.head(&mkt("KXA")).unwrap();
    assert_eq!(head.status, SettlementStatus::Posted);
    assert_eq!(head.entry_id, e2);
    assert_eq!(head.supersedes.as_deref(), Some(e1.as_str()));
    assert_eq!(
        head.amount_cents,
        Cents::new(1_000),
        "amount carries through"
    );

    ledger
        .advance(
            "s-3".to_string(),
            &mkt("KXA"),
            SettlementStatus::Confirmed,
            t(2),
        )
        .unwrap();
    assert_eq!(
        ledger.head(&mkt("KXA")).unwrap().status,
        SettlementStatus::Confirmed
    );
    // The chain is full history, oldest first.
    let chain = ledger.chain(&mkt("KXA"));
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0].status, SettlementStatus::Pending);
    assert_eq!(chain[2].status, SettlementStatus::Confirmed);
}

#[test]
fn illegal_transitions_error_never_coerce() {
    let mut ledger = SettlementLedger::new();
    // No chain at all: nothing to advance.
    assert!(ledger
        .advance(
            "x-1".to_string(),
            &mkt("KXA"),
            SettlementStatus::Posted,
            t(0)
        )
        .is_err());

    ledger
        .record_pending(
            "s-1".to_string(),
            mkt("KXA"),
            VenueId::new("sim").unwrap(),
            Cents::new(500),
            serde_json::json!({}),
            t(0),
        )
        .unwrap();
    // pending -> confirmed skips posted.
    assert!(ledger
        .advance(
            "x-2".to_string(),
            &mkt("KXA"),
            SettlementStatus::Confirmed,
            t(1)
        )
        .is_err());
    // pending -> pending is not a transition.
    assert!(ledger
        .advance(
            "x-3".to_string(),
            &mkt("KXA"),
            SettlementStatus::Pending,
            t(1)
        )
        .is_err());
    // A second pending while one is open is a duplicate, not a chain.
    assert!(ledger
        .record_pending(
            "x-4".to_string(),
            mkt("KXA"),
            VenueId::new("sim").unwrap(),
            Cents::new(500),
            serde_json::json!({}),
            t(1),
        )
        .is_err());
}

#[test]
fn reversal_supersedes_and_allows_a_corrected_resettlement() {
    let mut ledger = SettlementLedger::new();
    ledger
        .record_pending(
            "s-1".to_string(),
            mkt("KXA"),
            VenueId::new("sim").unwrap(),
            Cents::new(1_000),
            serde_json::json!({"winner": "Yes"}),
            t(0),
        )
        .unwrap();
    ledger
        .advance(
            "s-2".to_string(),
            &mkt("KXA"),
            SettlementStatus::Posted,
            t(1),
        )
        .unwrap();
    ledger
        .advance(
            "s-3".to_string(),
            &mkt("KXA"),
            SettlementStatus::Confirmed,
            t(2),
        )
        .unwrap();

    // Venue correction: reverse, then a NEW pending for the re-settlement.
    let rev = ledger
        .reverse(
            "s-4".to_string(),
            &mkt("KXA"),
            serde_json::json!({"claw": 1000}),
            t(3),
        )
        .unwrap();
    assert_eq!(ledger.head(&mkt("KXA")).unwrap().entry_id, rev);
    assert_eq!(
        ledger.head(&mkt("KXA")).unwrap().status,
        SettlementStatus::Reversed
    );

    // Re-settle with the corrected outcome.
    ledger
        .record_pending(
            "s-5".to_string(),
            mkt("KXA"),
            VenueId::new("sim").unwrap(),
            Cents::ZERO,
            serde_json::json!({"winner": "No"}),
            t(4),
        )
        .unwrap();
    assert_eq!(
        ledger.head(&mkt("KXA")).unwrap().status,
        SettlementStatus::Pending
    );
    assert_eq!(ledger.chain(&mkt("KXA")).len(), 5, "full history retained");
}

#[test]
fn reversal_requires_a_posted_or_confirmed_head() {
    let mut ledger = SettlementLedger::new();
    ledger
        .record_pending(
            "s-1".to_string(),
            mkt("KXA"),
            VenueId::new("sim").unwrap(),
            Cents::new(100),
            serde_json::json!({}),
            t(0),
        )
        .unwrap();
    // Reversing a merely-pending settlement makes no sense (nothing hit
    // the books yet).
    assert!(ledger
        .reverse("x-1".to_string(), &mkt("KXA"), serde_json::json!({}), t(1))
        .is_err());
}

#[test]
fn capital_in_limbo_counts_pending_and_posted_only() {
    let mut ledger = SettlementLedger::new();
    ledger
        .record_pending(
            "s-1".to_string(),
            mkt("KXA"),
            VenueId::new("sim").unwrap(),
            Cents::new(1_000),
            serde_json::json!({}),
            t(0),
        )
        .unwrap();
    ledger
        .record_pending(
            "s-2".to_string(),
            mkt("KXB"),
            VenueId::new("sim").unwrap(),
            Cents::new(300),
            serde_json::json!({}),
            t(0),
        )
        .unwrap();
    assert_eq!(ledger.capital_in_limbo().unwrap(), Cents::new(1_300));

    ledger
        .advance(
            "s-3".to_string(),
            &mkt("KXA"),
            SettlementStatus::Posted,
            t(1),
        )
        .unwrap();
    assert_eq!(
        ledger.capital_in_limbo().unwrap(),
        Cents::new(1_300),
        "posted is still in limbo (not venue-confirmed)"
    );

    ledger
        .advance(
            "s-4".to_string(),
            &mkt("KXA"),
            SettlementStatus::Confirmed,
            t(2),
        )
        .unwrap();
    assert_eq!(
        ledger.capital_in_limbo().unwrap(),
        Cents::new(300),
        "confirmed leaves limbo"
    );
}

// --------------------------------------------------- position reversal

/// Build a book holding 10 YES @ 60c (basis 600) and 4 NO @ 30c (basis 120).
fn seeded_book() -> PositionBook {
    let mut book = PositionBook::new();
    book.apply_fill(&fill("KXA", Side::Yes, Action::Buy, 60, 10, 1))
        .unwrap();
    book.apply_fill(&fill("KXA", Side::No, Action::Buy, 30, 4, 2))
        .unwrap();
    book
}

#[test]
fn reversal_restores_lots_and_unwinds_realized_pnl_exactly() {
    let mut book = seeded_book();
    let before = book.position(&mkt("KXA")).unwrap().clone();
    let snap = book.settlement_snapshot(&mkt("KXA")).unwrap();

    // Settle YES at 100c: payout 1000, pnl = 1000 - 600 - 120 = 280.
    let payout = book
        .apply_settlement(&mkt("KXA"), Side::Yes, Cents::new(100))
        .unwrap();
    assert_eq!(payout, Cents::new(1_000));
    assert_eq!(
        book.position(&mkt("KXA")).unwrap().realized_pnl,
        Cents::new(280)
    );

    // Venue correction: reverse. Clawback owed = the payout; lots and
    // realized PnL return to their pre-settlement values exactly.
    let claw = book.reverse_settlement(&mkt("KXA"), &snap).unwrap();
    assert_eq!(claw, Cents::new(1_000));
    let after = book.position(&mkt("KXA")).unwrap();
    assert_eq!(after.yes.qty, before.yes.qty);
    assert_eq!(after.yes.cost_basis, before.yes.cost_basis);
    assert_eq!(after.no.qty, before.no.qty);
    assert_eq!(after.no.cost_basis, before.no.cost_basis);
    assert_eq!(after.realized_pnl, before.realized_pnl);
    assert_eq!(
        after.lifecycle,
        fortuna_state::PositionLifecycle::ResolutionPending,
        "a reversed market is back in limbo, not freely tradable"
    );

    // Corrected re-settlement (NO wins): payout 4*100 = 400,
    // pnl = 400 - 120 - 600 = -320.
    let payout2 = book
        .apply_settlement(&mkt("KXA"), Side::No, Cents::new(100))
        .unwrap();
    assert_eq!(payout2, Cents::new(400));
    assert_eq!(
        book.position(&mkt("KXA")).unwrap().realized_pnl,
        Cents::new(-320)
    );
}

#[test]
fn reversal_of_an_unsettled_or_repopulated_market_errors() {
    let mut book = seeded_book();
    let snap = book.settlement_snapshot(&mkt("KXA")).unwrap();

    // Not settled yet: lots are live, reversal must refuse (it would
    // double the position).
    assert!(matches!(
        book.reverse_settlement(&mkt("KXA"), &snap),
        Err(StateError::IllegalReversal { .. })
    ));

    // Unknown market errors.
    let ghost: SettlementSnapshot = snap.clone();
    assert!(book.reverse_settlement(&mkt("KXGHOST"), &ghost).is_err());
}

#[test]
fn snapshot_of_unknown_market_errors() {
    let book = PositionBook::new();
    assert!(book.settlement_snapshot(&mkt("KXNONE")).is_err());
}
