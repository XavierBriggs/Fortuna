//! T0.7 tests: reservation ledger (spec 5.14). Written BEFORE implementation.
//!
//! Contract under test: per-strategy envelopes from config; reserve fails
//! closed (unknown strategy, negative amount, duplicate intent, envelope
//! exceeded); exactly-at-envelope passes. release is idempotent: exactly
//! one Ok(true) per reservation, never a double-decrement. Reservations are
//! DERIVED state: rebuild() wholesale-reconstructs from open intents so a
//! crash can never leak a reservation. Rebuild ACCEPTS over-envelope totals
//! (a reduced envelope at boot is legitimate; refusing to load would brick
//! recovery) but over_envelope() exposes the condition and any new reserve
//! fails until the old ones unwind.

use fortuna_core::ids::{IntentId, Ulid};
use fortuna_core::money::Cents;
use fortuna_state::{ReservationLedger, StateError};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn envelopes() -> BTreeMap<String, Cents> {
    BTreeMap::from([
        ("alpha".to_string(), Cents::new(1_000)),
        ("beta".to_string(), Cents::new(500)),
    ])
}

fn iid(n: u128) -> IntentId {
    IntentId::new(Ulid::from_parts(1, n))
}

#[test]
fn reserve_within_envelope_updates_totals_and_headroom() {
    let mut l = ReservationLedger::new(envelopes());
    l.reserve("alpha", iid(1), Cents::new(400)).unwrap();
    assert_eq!(l.active_total("alpha"), Cents::new(400));
    assert_eq!(l.headroom("alpha").unwrap(), Cents::new(600));
    assert_eq!(l.active_total("beta"), Cents::ZERO);
    assert!(!l.over_envelope("alpha"));
}

#[test]
fn reserve_exactly_at_envelope_passes() {
    let mut l = ReservationLedger::new(envelopes());
    l.reserve("alpha", iid(1), Cents::new(1_000)).unwrap();
    assert_eq!(l.headroom("alpha").unwrap(), Cents::ZERO);
    assert!(
        !l.over_envelope("alpha"),
        "at envelope is not over envelope"
    );
}

#[test]
fn one_cent_over_envelope_fails() {
    let mut l = ReservationLedger::new(envelopes());
    let err = l.reserve("alpha", iid(1), Cents::new(1_001)).unwrap_err();
    assert!(matches!(err, StateError::EnvelopeExceeded { .. }));
    assert_eq!(
        l.active_total("alpha"),
        Cents::ZERO,
        "failed reserve holds nothing"
    );

    // Cumulative: 600 + 401 > 1000 must fail and leave 600 standing.
    l.reserve("alpha", iid(2), Cents::new(600)).unwrap();
    let err = l.reserve("alpha", iid(3), Cents::new(401)).unwrap_err();
    assert!(matches!(err, StateError::EnvelopeExceeded { .. }));
    assert_eq!(l.active_total("alpha"), Cents::new(600));
}

#[test]
fn unknown_strategy_fails_closed() {
    let mut l = ReservationLedger::new(envelopes());
    let err = l.reserve("ghost", iid(1), Cents::new(1)).unwrap_err();
    assert!(matches!(err, StateError::UnknownStrategy { .. }));
    assert_eq!(l.active_total("ghost"), Cents::ZERO);
    assert!(
        l.headroom("ghost").is_err(),
        "headroom of unknown strategy errors"
    );
}

#[test]
fn negative_amount_fails_closed() {
    let mut l = ReservationLedger::new(envelopes());
    let err = l.reserve("alpha", iid(1), Cents::new(-1)).unwrap_err();
    assert!(matches!(err, StateError::NegativeReservation { .. }));
}

#[test]
fn zero_amount_reserve_is_allowed_and_inert() {
    let mut l = ReservationLedger::new(envelopes());
    l.reserve("alpha", iid(1), Cents::ZERO).unwrap();
    assert_eq!(l.active_total("alpha"), Cents::ZERO);
    assert!(l.release(iid(1)).unwrap());
}

#[test]
fn duplicate_intent_reservation_fails_closed() {
    let mut l = ReservationLedger::new(envelopes());
    l.reserve("alpha", iid(1), Cents::new(100)).unwrap();
    let err = l.reserve("alpha", iid(1), Cents::new(100)).unwrap_err();
    assert!(matches!(err, StateError::DuplicateReservation { .. }));
    assert_eq!(l.active_total("alpha"), Cents::new(100), "no double-count");
}

#[test]
fn release_exactly_once_never_double_frees() {
    let mut l = ReservationLedger::new(envelopes());
    l.reserve("alpha", iid(1), Cents::new(400)).unwrap();

    assert!(l.release(iid(1)).unwrap(), "first release frees");
    assert_eq!(l.active_total("alpha"), Cents::ZERO);

    assert!(!l.release(iid(1)).unwrap(), "second release is a no-op");
    assert_eq!(
        l.active_total("alpha"),
        Cents::ZERO,
        "totals never double-decrement"
    );

    // Prove the total did not go below zero: the full envelope is again
    // reservable, and not one cent more.
    l.reserve("alpha", iid(2), Cents::new(1_000)).unwrap();
    let err = l.reserve("alpha", iid(3), Cents::new(1)).unwrap_err();
    assert!(matches!(err, StateError::EnvelopeExceeded { .. }));
}

#[test]
fn release_of_unknown_intent_returns_false() {
    let mut l = ReservationLedger::new(envelopes());
    assert!(!l.release(iid(99)).unwrap());
}

#[test]
fn rebuild_replaces_all_prior_state() {
    let mut l = ReservationLedger::new(envelopes());
    l.reserve("alpha", iid(1), Cents::new(900)).unwrap();

    let rebuilt = ReservationLedger::rebuild(
        envelopes(),
        vec![(iid(9), "alpha".to_string(), Cents::new(200))],
    )
    .unwrap();

    let mut l = rebuilt;
    assert_eq!(
        l.active_total("alpha"),
        Cents::new(200),
        "old state is gone"
    );
    assert!(!l.release(iid(1)).unwrap(), "pre-rebuild reservation gone");
    assert!(l.release(iid(9)).unwrap());
    assert_eq!(l.active_total("alpha"), Cents::ZERO);
}

#[test]
fn rebuild_accepts_over_envelope_and_flags_it() {
    // Conservative choice: at boot a reduced envelope may legitimately sit
    // below already-open intents. Refusing to load would brick recovery;
    // instead the ledger loads, exposes over_envelope(), and refuses NEW
    // reservations until the old ones unwind.
    let mut l = ReservationLedger::rebuild(
        envelopes(),
        vec![
            (iid(1), "alpha".to_string(), Cents::new(800)),
            (iid(2), "alpha".to_string(), Cents::new(700)),
        ],
    )
    .unwrap();

    assert_eq!(l.active_total("alpha"), Cents::new(1_500));
    assert!(l.over_envelope("alpha"));
    assert_eq!(
        l.headroom("alpha").unwrap(),
        Cents::new(-500),
        "negative headroom reported"
    );

    let err = l.reserve("alpha", iid(3), Cents::new(1)).unwrap_err();
    assert!(matches!(err, StateError::EnvelopeExceeded { .. }));

    // Unwinding restores normal operation.
    assert!(l.release(iid(1)).unwrap());
    assert!(!l.over_envelope("alpha"));
    l.reserve("alpha", iid(3), Cents::new(300)).unwrap();
    assert_eq!(l.active_total("alpha"), Cents::new(1_000));
}

#[test]
fn rebuild_with_unknown_strategy_loads_flagged_and_fail_closed() {
    // Open intents for a strategy no longer in config must still load (they
    // need managing) but the strategy is over-envelope by definition and no
    // new reservation is possible.
    let mut l = ReservationLedger::rebuild(
        envelopes(),
        vec![(iid(1), "ghost".to_string(), Cents::new(100))],
    )
    .unwrap();
    assert_eq!(l.active_total("ghost"), Cents::new(100));
    assert!(l.over_envelope("ghost"));
    assert!(matches!(
        l.reserve("ghost", iid(2), Cents::new(1)).unwrap_err(),
        StateError::UnknownStrategy { .. }
    ));
    assert!(l.release(iid(1)).unwrap(), "still releasable to unwind");
}

#[test]
fn rebuild_duplicate_intent_is_corrupt_state_and_errors() {
    let err = ReservationLedger::rebuild(
        envelopes(),
        vec![
            (iid(1), "alpha".to_string(), Cents::new(100)),
            (iid(1), "alpha".to_string(), Cents::new(200)),
        ],
    )
    .unwrap_err();
    assert!(matches!(err, StateError::DuplicateReservation { .. }));
}

#[test]
fn rebuild_negative_amount_is_corrupt_state_and_errors() {
    let err = ReservationLedger::rebuild(
        envelopes(),
        vec![(iid(1), "alpha".to_string(), Cents::new(-5))],
    )
    .unwrap_err();
    assert!(matches!(err, StateError::NegativeReservation { .. }));
}

// -------------------------------------------------------------- property

#[derive(Debug, Clone)]
enum Op {
    Reserve {
        slot: u128,
        alpha: bool,
        amount: i64,
    },
    Release {
        slot: u128,
    },
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0u128..8, any::<bool>(), 0i64..700).prop_map(|(slot, alpha, amount)| Op::Reserve {
            slot,
            alpha,
            amount
        }),
        (0u128..8).prop_map(|slot| Op::Release { slot }),
    ]
}

proptest! {
    /// Under arbitrary interleavings of reserve/release: the ledger's
    /// per-strategy totals always equal the sum of live reservations in a
    /// reference model, successful reserves never exceed the envelope, and
    /// a release is honored exactly once per live reservation.
    #[test]
    fn ledger_totals_match_reference_model(ops in prop::collection::vec(arb_op(), 0..60)) {
        let mut l = ReservationLedger::new(envelopes());
        let mut model: BTreeMap<u128, (String, i64)> = BTreeMap::new();

        for op in ops {
            match op {
                Op::Reserve { slot, alpha, amount } => {
                    let strat = if alpha { "alpha" } else { "beta" };
                    let envelope = if alpha { 1_000 } else { 500 };
                    let live: i64 = model.values()
                        .filter(|(s, _)| s == strat)
                        .map(|(_, a)| a)
                        .sum();

                    let already_live = model.contains_key(&slot);
                    let res = l.reserve(strat, iid(slot), Cents::new(amount));
                    if already_live {
                        let dup = matches!(res, Err(StateError::DuplicateReservation { .. }));
                        prop_assert!(dup, "expected DuplicateReservation, got {:?}", res);
                    } else if live + amount > envelope {
                        let exceeded = matches!(res, Err(StateError::EnvelopeExceeded { .. }));
                        prop_assert!(exceeded, "expected EnvelopeExceeded, got {:?}", res);
                    } else {
                        prop_assert!(res.is_ok());
                        model.insert(slot, (strat.to_string(), amount));
                    }
                }
                Op::Release { slot } => {
                    let released = l.release(iid(slot)).unwrap();
                    prop_assert_eq!(released, model.remove(&slot).is_some());
                }
            }

            for strat in ["alpha", "beta"] {
                let expected: i64 = model.values()
                    .filter(|(s, _)| s == strat)
                    .map(|(_, a)| a)
                    .sum();
                prop_assert_eq!(l.active_total(strat), Cents::new(expected));
                let envelope = if strat == "alpha" { 1_000 } else { 500 };
                prop_assert!(l.active_total(strat).raw() <= envelope);
            }
        }
    }
}
