//! T5.B3 slice-2 tests: perp margin equity composition for I2 halt math
//! (spec 5.15, written from spec text BEFORE implementation).
//!
//! Contract under test: "I2 drawdown math includes funding paid/received
//! and margin unrealized PnL, marked at the venue's settlement mark per the
//! conservative-marking policy (if the venue mark and our conservative mark
//! disagree, the worse-for-us number governs halt math)."
//!
//! `equity_with_margin` composes the halt-math equity: event-contract
//! equity + every perp margin account's conservative equity (balance +
//! worse-for-us unrealized PnL + pending funding). The unmarked flag
//! (a position valued on the venue's number alone) propagates so callers
//! can alert on degraded marking without blocking halt math.

use fortuna_core::market::{Contracts, MarketId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpMarks, PerpPosition, PerpPrice};
use fortuna_state::{equity_with_margin, StateError};
use proptest::prelude::*;

fn long_pos(qty: i64, entry: i64) -> PerpPosition {
    PerpPosition {
        market: MarketId::new("KXBTCPERP").unwrap(),
        qty: Contracts::new(qty),
        avg_entry: PerpPrice::new(entry),
    }
}

fn marks(venue: i64, ours: Option<i64>) -> PerpMarks {
    PerpMarks {
        venue_settlement: PerpPrice::new(venue),
        conservative: ours.map(PerpPrice::new),
    }
}

#[test]
fn no_margin_accounts_is_event_equity_unchanged() {
    let out = equity_with_margin(Cents::new(1_000_000), &[]).unwrap();
    assert_eq!(out.total, Cents::new(1_000_000));
    assert!(!out.unmarked_flag);
}

#[test]
fn margin_equity_adds_balance_upnl_and_funding() {
    // Long 1,000 @ entry $6.2600 marked at $5.7600: uPnL = -5,000 tt x
    // 1,000 = -50,000c. Balance 100,000c, pending funding -30c.
    let account = MarginAccountView::compute(
        Cents::new(100_000),
        &[(long_pos(1_000, 62_600), marks(57_600, Some(57_600)))],
        Cents::new(-30),
    )
    .unwrap();
    assert_eq!(account.equity, Cents::new(49_970));
    let out = equity_with_margin(Cents::new(1_000_000), &[account]).unwrap();
    assert_eq!(out.total, Cents::new(1_049_970));
    assert!(!out.unmarked_flag);
}

#[test]
fn worse_for_us_mark_governs_composed_equity() {
    // Venue settlement mark says the position is whole (uPnL 0); our
    // conservative mark says -50,000c. The worse number governs halt math
    // (spec 5.15) — the composed total must reflect the loss.
    let account = MarginAccountView::compute(
        Cents::new(100_000),
        &[(long_pos(1_000, 62_600), marks(62_600, Some(57_600)))],
        Cents::ZERO,
    )
    .unwrap();
    let out = equity_with_margin(Cents::new(1_000_000), &[account]).unwrap();
    assert_eq!(out.total, Cents::new(1_050_000)); // 1,000,000 + 100,000 - 50,000
}

#[test]
fn funding_paid_lowers_halt_equity() {
    // Funding is part of I2 drawdown math: a pending funding debit lowers
    // the composed equity even with flat positions.
    let account =
        MarginAccountView::compute(Cents::new(100_000), &[], Cents::new(-50_000)).unwrap();
    let out = equity_with_margin(Cents::new(1_000_000), &[account]).unwrap();
    assert_eq!(out.total, Cents::new(1_050_000));
}

#[test]
fn multiple_accounts_sum() {
    let a = MarginAccountView::compute(Cents::new(100_000), &[], Cents::ZERO).unwrap();
    let b = MarginAccountView::compute(Cents::new(25_000), &[], Cents::new(-5_000)).unwrap();
    let out = equity_with_margin(Cents::new(1_000_000), &[a, b]).unwrap();
    assert_eq!(out.total, Cents::new(1_120_000));
}

#[test]
fn unmarked_flag_propagates() {
    let degraded = MarginAccountView::compute(
        Cents::new(100_000),
        &[(long_pos(10, 62_600), marks(62_600, None))],
        Cents::ZERO,
    )
    .unwrap();
    assert!(degraded.unmarked_flag);
    let clean = MarginAccountView::compute(Cents::new(50_000), &[], Cents::ZERO).unwrap();
    let out = equity_with_margin(Cents::new(1_000_000), &[clean.clone(), degraded]).unwrap();
    assert!(out.unmarked_flag);
    let out = equity_with_margin(Cents::new(1_000_000), &[clean]).unwrap();
    assert!(!out.unmarked_flag);
}

#[test]
fn overflow_is_error_not_panic() {
    let account = MarginAccountView::compute(Cents::new(i64::MAX), &[], Cents::ZERO).unwrap();
    let result = equity_with_margin(Cents::new(1), &[account]);
    assert!(matches!(result, Err(StateError::Money(_))));
}

proptest! {
    #[test]
    fn prop_composition_matches_i128_reference(
        event in -1_000_000_000i64..1_000_000_000,
        balances in proptest::collection::vec(-1_000_000_000i64..1_000_000_000, 0..5),
    ) {
        let accounts: Vec<_> = balances
            .iter()
            .map(|b| MarginAccountView::compute(Cents::new(*b), &[], Cents::ZERO).unwrap())
            .collect();
        let out = equity_with_margin(Cents::new(event), &accounts).unwrap();
        let reference: i128 = i128::from(event)
            + balances.iter().map(|b| i128::from(*b)).sum::<i128>();
        prop_assert_eq!(i128::from(out.total.raw()), reference);
    }
}
