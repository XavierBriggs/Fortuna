//! T0.10 tests: the shared sizing library. Spec 5.14/5.9; PROMPT edge-case
//! floor: Kelly with p at 0.0/0.5/1.0; envelope free balance exactly equal
//! to order cost.

use fortuna_core::money::Cents;
use fortuna_state::{affordable_sets, kelly_binary, kelly_contracts};

#[test]
fn affordable_sets_floors_and_caps() {
    assert_eq!(affordable_sets(Cents::new(1_000), Cents::new(300), 10), 3);
    // Exactly equal: the whole headroom buys exactly one set.
    assert_eq!(affordable_sets(Cents::new(300), Cents::new(300), 10), 1);
    assert_eq!(affordable_sets(Cents::new(299), Cents::new(300), 10), 0);
    assert_eq!(affordable_sets(Cents::new(10_000), Cents::new(1), 5), 5); // cap binds
    assert_eq!(affordable_sets(Cents::ZERO, Cents::new(300), 10), 0);
    assert_eq!(affordable_sets(Cents::new(-5), Cents::new(300), 10), 0);
    assert_eq!(affordable_sets(Cents::new(1_000), Cents::ZERO, 10), 0);
    assert_eq!(affordable_sets(Cents::new(1_000), Cents::new(300), 0), 0);
}

#[test]
fn kelly_at_the_probability_boundaries() {
    // p = 0: clamped to zero.
    assert_eq!(kelly_binary(0.0, Cents::new(50), 0.25).unwrap(), 0.0);
    // p = 0.5 at a fair 50c: zero edge, zero size.
    assert_eq!(kelly_binary(0.5, Cents::new(50), 0.25).unwrap(), 0.0);
    // p = 1: certainty -> full Kelly 1.0 x fraction.
    let f = kelly_binary(1.0, Cents::new(50), 0.25).unwrap();
    assert!((f - 0.25).abs() < 1e-12);
}

#[test]
fn kelly_known_value() {
    // p=0.6 at 50c: f* = (60-50)/50 = 0.2; quarter Kelly = 0.05.
    let f = kelly_binary(0.6, Cents::new(50), 0.25).unwrap();
    assert!((f - 0.05).abs() < 1e-12);
}

#[test]
fn kelly_rejects_garbage() {
    assert!(kelly_binary(-0.1, Cents::new(50), 0.25).is_err());
    assert!(kelly_binary(1.1, Cents::new(50), 0.25).is_err());
    assert!(kelly_binary(f64::NAN, Cents::new(50), 0.25).is_err());
    assert!(kelly_binary(0.5, Cents::new(0), 0.25).is_err());
    assert!(kelly_binary(0.5, Cents::new(100), 0.25).is_err());
    assert!(kelly_binary(0.5, Cents::new(50), 1.5).is_err());
}

#[test]
fn kelly_contracts_floors_into_integer_money() {
    // p=0.6@50c quarter-Kelly = 0.05 of 10_000c headroom = 500c budget;
    // all-in 52c/contract -> 9 contracts (floor), cap 100.
    let n = kelly_contracts(
        0.6,
        Cents::new(50),
        0.25,
        Cents::new(10_000),
        Cents::new(52),
        100,
    )
    .unwrap();
    assert_eq!(n, 9);
    // Cap binds.
    let n = kelly_contracts(
        0.6,
        Cents::new(50),
        0.25,
        Cents::new(10_000),
        Cents::new(52),
        4,
    )
    .unwrap();
    assert_eq!(n, 4);
    // No headroom -> zero.
    let n = kelly_contracts(0.9, Cents::new(50), 0.25, Cents::ZERO, Cents::new(52), 100).unwrap();
    assert_eq!(n, 0);
}
