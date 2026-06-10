//! T0.7 tests: per-side position lots. Spec 5.13/5.14 + the pair-value rule
//! (YES and NO never net: a held pair pays $1 at settlement regardless of
//! outcome, and the books must price that).

use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueOrderId};
use fortuna_core::money::Cents;
use fortuna_state::{PositionBook, PositionLifecycle, StateError};
use fortuna_venues::Fill;
use proptest::prelude::*;

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn fill(n: u64, side: Side, action: Action, price: i64, qty: i64, fee: i64) -> Fill {
    Fill {
        fill_id: format!("f-{n}"),
        venue_order_id: VenueOrderId::new(format!("v-{n}")).unwrap(),
        client_order_id: ClientOrderId::new(format!("c-{n}")).unwrap(),
        market: mkt("M"),
        side,
        action,
        price: Cents::new(price),
        qty: Contracts::new(qty),
        fee: Cents::new(fee),
        is_maker: false,
        at: fortuna_core::clock::UtcTimestamp::from_epoch_millis(0).unwrap(),
    }
}

// ---- opens and adds ----

#[test]
fn buys_accumulate_per_side_lots_without_netting() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 10, 2))
        .unwrap();
    book.apply_fill(&fill(2, Side::No, Action::Buy, 55, 10, 3))
        .unwrap();
    let p = book.position(&mkt("M")).unwrap();
    // THE pair-value rule: both lots held, nothing netted away.
    assert_eq!((p.yes.qty, p.no.qty), (10, 10));
    assert_eq!(p.yes.cost_basis, Cents::new(400));
    assert_eq!(p.no.cost_basis, Cents::new(550));
    assert_eq!(p.net_yes(), 0); // exposure view only
    assert_eq!(p.fees_paid, Cents::new(5));
    assert_eq!(p.realized_pnl, Cents::ZERO);
}

#[test]
fn adds_average_into_the_lot_basis() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 10, 0))
        .unwrap();
    book.apply_fill(&fill(2, Side::Yes, Action::Buy, 50, 5, 0))
        .unwrap();
    let p = book.position(&mkt("M")).unwrap();
    assert_eq!(p.yes.qty, 15);
    assert_eq!(p.yes.cost_basis, Cents::new(650));
}

// ---- reductions (close-only) ----

#[test]
fn sell_realizes_proportional_basis_floor_division() {
    let mut book = PositionBook::new();
    // Basis 100 over 3 held (buy 1@34 + 2@33).
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 34, 1, 0))
        .unwrap();
    book.apply_fill(&fill(2, Side::Yes, Action::Buy, 33, 2, 0))
        .unwrap();
    let p = book.position(&mkt("M")).unwrap();
    assert_eq!((p.yes.qty, p.yes.cost_basis), (3, Cents::new(100)));

    // Sell 1 @ 50: closed basis = floor(100 * 1 / 3) = 33; pnl = 50 - 33 = 17.
    book.apply_fill(&fill(3, Side::Yes, Action::Sell, 50, 1, 0))
        .unwrap();
    let p = book.position(&mkt("M")).unwrap();
    assert_eq!((p.yes.qty, p.yes.cost_basis), (2, Cents::new(67)));
    assert_eq!(p.realized_pnl, Cents::new(17));

    // Sell the remaining 2 @ 50: closed = 67 exactly; pnl += 100 - 67 = 33.
    book.apply_fill(&fill(4, Side::Yes, Action::Sell, 50, 2, 0))
        .unwrap();
    let p = book.position(&mkt("M")).unwrap();
    assert_eq!((p.yes.qty, p.yes.cost_basis), (0, Cents::ZERO));
    assert_eq!(p.realized_pnl, Cents::new(50)); // 150 proceeds - 100 basis
    assert!(p.is_flat());
}

#[test]
fn no_side_sells_realize_against_the_no_lot() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::No, Action::Buy, 60, 4, 0))
        .unwrap();
    book.apply_fill(&fill(2, Side::No, Action::Sell, 70, 4, 0))
        .unwrap();
    let p = book.position(&mkt("M")).unwrap();
    assert!(p.is_flat());
    assert_eq!(p.realized_pnl, Cents::new(40)); // (70-60) x 4
}

#[test]
fn over_close_is_a_discrepancy_error_not_a_flip() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 5, 0))
        .unwrap();
    let err = book
        .apply_fill(&fill(2, Side::Yes, Action::Sell, 50, 6, 0))
        .unwrap_err();
    assert!(matches!(
        err,
        StateError::OverClose {
            held: 5,
            closing: 6,
            ..
        }
    ));
    // Atomic: nothing changed, not even fees.
    let p = book.position(&mkt("M")).unwrap();
    assert_eq!((p.yes.qty, p.yes.cost_basis), (5, Cents::new(200)));
    assert_eq!(p.fees_paid, Cents::ZERO);
}

#[test]
fn selling_a_side_never_touches_the_other_lot() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 10, 0))
        .unwrap();
    book.apply_fill(&fill(2, Side::No, Action::Buy, 55, 10, 0))
        .unwrap();
    book.apply_fill(&fill(3, Side::Yes, Action::Sell, 45, 10, 0))
        .unwrap();
    let p = book.position(&mkt("M")).unwrap();
    assert_eq!((p.yes.qty, p.no.qty), (0, 10));
    assert_eq!(p.no.cost_basis, Cents::new(550));
    assert_eq!(p.realized_pnl, Cents::new(50)); // 450 - 400
}

// ---- fees ----

#[test]
fn fees_accumulate_separately_including_rebates() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 10, 18))
        .unwrap();
    book.apply_fill(&fill(2, Side::Yes, Action::Buy, 40, 10, -5))
        .unwrap(); // rebate
    let p = book.position(&mkt("M")).unwrap();
    assert_eq!(p.fees_paid, Cents::new(13));
    assert_eq!(p.yes.cost_basis, Cents::new(800)); // fees never in basis
}

// ---- invalid fills ----

#[test]
fn invalid_fills_are_rejected() {
    let mut book = PositionBook::new();
    assert!(matches!(
        book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 0, 0)),
        Err(StateError::InvalidFill { .. })
    ));
    assert!(matches!(
        book.apply_fill(&fill(2, Side::Yes, Action::Buy, -1, 5, 0)),
        Err(StateError::InvalidFill { .. })
    ));
    assert!(book.position(&mkt("M")).is_none());
}

// ---- settlement ----

#[test]
fn settlement_pays_the_winning_lot_and_realizes_both() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 10, 0))
        .unwrap(); // 400
    book.apply_fill(&fill(2, Side::No, Action::Buy, 55, 10, 0))
        .unwrap(); // 550
    let payout = book
        .apply_settlement(&mkt("M"), Side::Yes, Cents::new(100))
        .unwrap();
    // The PAIR pays: winning YES lot 10 x 100 = 1000 from the venue.
    assert_eq!(payout, Cents::new(1_000));
    let p = book.position(&mkt("M")).unwrap();
    assert!(p.is_flat());
    // PnL: (1000 - 400) + (0 - 550) = +50: the sum-arb economics.
    assert_eq!(p.realized_pnl, Cents::new(50));
    assert_eq!(p.lifecycle, PositionLifecycle::Open);
}

#[test]
fn settlement_of_a_losing_only_position() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::No, Action::Buy, 30, 5, 0))
        .unwrap();
    let payout = book
        .apply_settlement(&mkt("M"), Side::Yes, Cents::new(100))
        .unwrap();
    assert_eq!(payout, Cents::ZERO);
    assert_eq!(
        book.position(&mkt("M")).unwrap().realized_pnl,
        Cents::new(-150)
    );
}

#[test]
fn settlement_on_untracked_market_errors() {
    let mut book = PositionBook::new();
    assert!(matches!(
        book.apply_settlement(&mkt("GHOST"), Side::Yes, Cents::new(100)),
        Err(StateError::UnknownMarket { .. })
    ));
}

#[test]
fn settlement_resets_lifecycle() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 5, 0))
        .unwrap();
    book.set_lifecycle(&mkt("M"), PositionLifecycle::ResolutionPending)
        .unwrap();
    book.apply_settlement(&mkt("M"), Side::Yes, Cents::new(100))
        .unwrap();
    assert_eq!(
        book.position(&mkt("M")).unwrap().lifecycle,
        PositionLifecycle::Open
    );
}

// ---- void ----

#[test]
fn void_refunds_both_lot_bases_and_leaves_pnl_untouched() {
    let mut book = PositionBook::new();
    book.apply_fill(&fill(1, Side::Yes, Action::Buy, 40, 10, 0))
        .unwrap();
    book.apply_fill(&fill(2, Side::No, Action::Buy, 55, 4, 0))
        .unwrap();
    book.apply_fill(&fill(3, Side::Yes, Action::Sell, 50, 5, 0))
        .unwrap(); // realize 50
    let refund = book.apply_void_refund(&mkt("M")).unwrap();
    // Remaining basis: yes 200 (half of 400) + no 220 = 420.
    assert_eq!(refund, Cents::new(420));
    let p = book.position(&mkt("M")).unwrap();
    assert!(p.is_flat());
    assert_eq!(p.realized_pnl, Cents::new(50)); // untouched by the void
}

// ---- lifecycle ----

#[test]
fn set_lifecycle_unknown_market_errors() {
    let mut book = PositionBook::new();
    assert!(matches!(
        book.set_lifecycle(&mkt("GHOST"), PositionLifecycle::Disputed),
        Err(StateError::UnknownMarket { .. })
    ));
}

// ---- properties ----

#[derive(Debug, Clone)]
struct ArbFill {
    yes: bool,
    buy: bool,
    price: i64,
    qty: i64,
}

fn arb_fill() -> impl Strategy<Value = ArbFill> {
    (any::<bool>(), any::<bool>(), 1i64..100, 1i64..50).prop_map(|(yes, buy, price, qty)| ArbFill {
        yes,
        buy,
        price,
        qty,
    })
}

proptest! {
    /// For all close-only fill sequences: lot quantities equal the signed
    /// per-side sums of APPLIED fills; conservation holds at every step:
    /// yes.basis + no.basis == (applied buys gross - applied sells gross)
    /// + realized_pnl... rearranged: basis_total == cash_into_positions +
    /// realized? No: basis_total == cash_into_positions - (-realized);
    /// see assertion. Over-closing sells error without corrupting state.
    #[test]
    fn prop_lot_sums_and_conservation(fills in proptest::collection::vec(arb_fill(), 1..60)) {
        let mut book = PositionBook::new();
        let (mut yes_sum, mut no_sum) = (0i64, 0i64);
        let mut net_cash_out = 0i64; // buys gross - sells gross, applied only
        for (n, f) in fills.iter().enumerate() {
            let side = if f.yes { Side::Yes } else { Side::No };
            let action = if f.buy { Action::Buy } else { Action::Sell };
            let result = book.apply_fill(&fill(n as u64, side, action, f.price, f.qty, 1));
            match result {
                Ok(()) => {
                    let signed = if f.buy { f.qty } else { -f.qty };
                    if f.yes { yes_sum += signed } else { no_sum += signed }
                    net_cash_out += if f.buy { f.price * f.qty } else { -(f.price * f.qty) };
                }
                Err(StateError::OverClose { .. }) => {} // refused, state intact
                Err(other) => return Err(TestCaseError::fail(format!("unexpected: {other}"))),
            }
            let Some(p) = book.position(&mkt("M")) else { continue };
            prop_assert!(p.yes.qty >= 0 && p.no.qty >= 0);
            prop_assert_eq!(p.yes.qty, yes_sum);
            prop_assert_eq!(p.no.qty, no_sum);
            // Conservation: what the open lots are carried at equals net cash
            // paid in, plus what was booked as realized profit (proceeds
            // above basis reduce cash but raise realized, exactly).
            let basis_total = p.yes.cost_basis.raw() + p.no.cost_basis.raw();
            prop_assert_eq!(
                basis_total, net_cash_out + p.realized_pnl.raw(),
                "conservation: basis {} != net cash {} + realized {}",
                basis_total, net_cash_out, p.realized_pnl.raw()
            );
        }
    }
}
