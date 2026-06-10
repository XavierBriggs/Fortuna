//! T0.7 tests: account views (spec 5.14) and exposure accounting (spec
//! 5.13). Written BEFORE implementation.
//!
//! Contract under test: settled = venue-confirmed cash; committed = resting
//! order cost + active reservations; floating = pending settlements at
//! guaranteed minimum + OPEN positions at conservative marks. Positions in
//! ResolutionPending or Disputed are EXCLUDED from floating (out of
//! bankroll) while their worst-case exposure is reported separately
//! (exposure_in_limbo) for gate inputs: reversal risk stays in exposure.
//! total = settled + floating (checked); deployable = settled - committed
//! (checked; MAY go negative). All arithmetic checked; overflow propagates.

use fortuna_core::money::{Cents, MoneyError};
use fortuna_state::{build_account_view, PositionLifecycle, PositionValuation, StateError};

fn val(lifecycle: PositionLifecycle, mark: i64, worst_case: i64) -> PositionValuation {
    PositionValuation {
        lifecycle,
        mark_value: Cents::new(mark),
        worst_case_exposure: Cents::new(worst_case),
    }
}

#[test]
fn hand_computed_view_with_open_and_limbo_positions() {
    let view = build_account_view(
        Cents::new(10_000), // settled
        Cents::new(1_500),  // resting order cost
        Cents::new(500),    // active reservations
        Cents::new(300),    // pending settlements at guaranteed minimum
        vec![
            val(PositionLifecycle::Open, 700, 700),
            val(PositionLifecycle::Open, 250, 300),
            val(PositionLifecycle::Disputed, 400, 900),
            val(PositionLifecycle::ResolutionPending, 100, 200),
        ],
    )
    .unwrap();

    assert_eq!(view.settled, Cents::new(10_000));
    assert_eq!(view.committed, Cents::new(2_000));
    // floating: 300 pending + 700 + 250 open marks. Limbo marks excluded.
    assert_eq!(view.floating, Cents::new(1_250));
    assert_eq!(view.total, Cents::new(11_250));
    assert_eq!(view.deployable, Cents::new(8_000));
    // limbo: worst-case exposure of Disputed + ResolutionPending.
    assert_eq!(view.exposure_in_limbo, Cents::new(1_100));
}

#[test]
fn disputed_position_alone_is_out_of_bankroll_but_in_exposure() {
    let view = build_account_view(
        Cents::new(5_000),
        Cents::ZERO,
        Cents::ZERO,
        Cents::new(40),
        vec![val(PositionLifecycle::Disputed, 999, 1_234)],
    )
    .unwrap();
    assert_eq!(
        view.floating,
        Cents::new(40),
        "disputed mark must not float"
    );
    assert_eq!(view.total, Cents::new(5_040));
    assert_eq!(view.exposure_in_limbo, Cents::new(1_234));
}

#[test]
fn deployable_can_go_negative_when_committed_exceeds_settled() {
    // Allowed and reported as-is: the gates treat negative deployable as
    // no headroom; the view must not mask it.
    let view = build_account_view(
        Cents::new(100),
        Cents::new(150),
        Cents::new(25),
        Cents::ZERO,
        vec![],
    )
    .unwrap();
    assert_eq!(view.deployable, Cents::new(-75));
}

#[test]
fn empty_inputs_give_all_zero_view() {
    let view =
        build_account_view(Cents::ZERO, Cents::ZERO, Cents::ZERO, Cents::ZERO, vec![]).unwrap();
    assert_eq!(view.settled, Cents::ZERO);
    assert_eq!(view.committed, Cents::ZERO);
    assert_eq!(view.floating, Cents::ZERO);
    assert_eq!(view.total, Cents::ZERO);
    assert_eq!(view.deployable, Cents::ZERO);
    assert_eq!(view.exposure_in_limbo, Cents::ZERO);
}

#[test]
fn total_overflow_propagates_as_error() {
    let err = build_account_view(
        Cents::new(i64::MAX),
        Cents::ZERO,
        Cents::ZERO,
        Cents::new(1),
        vec![],
    )
    .unwrap_err();
    assert!(matches!(
        err,
        StateError::Money(MoneyError::Overflow { .. })
    ));
}

#[test]
fn committed_overflow_propagates_as_error() {
    let err = build_account_view(
        Cents::ZERO,
        Cents::new(i64::MAX),
        Cents::new(1),
        Cents::ZERO,
        vec![],
    )
    .unwrap_err();
    assert!(matches!(
        err,
        StateError::Money(MoneyError::Overflow { .. })
    ));
}

#[test]
fn floating_sum_overflow_propagates_as_error() {
    let err = build_account_view(
        Cents::ZERO,
        Cents::ZERO,
        Cents::ZERO,
        Cents::new(i64::MAX),
        vec![val(PositionLifecycle::Open, 1, 1)],
    )
    .unwrap_err();
    assert!(matches!(
        err,
        StateError::Money(MoneyError::Overflow { .. })
    ));
}
