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

// ---- E5a: the fraction-of-headroom budget is INTEGER money ----

#[test]
fn kelly_budget_is_integer_exact_and_never_exceeds_headroom() {
    // f = 1.0 (full Kelly at certainty x fraction 1.0): the budget is the
    // headroom EXACTLY — no float drift at the money boundary.
    let contracts = kelly_contracts(
        1.0,
        Cents::new(50),
        1.0,
        Cents::new(999_999_999_999), // ~$10B: float would lose ulps here
        Cents::new(1),
        i64::MAX,
    )
    .unwrap();
    assert_eq!(contracts, 999_999_999_999, "exact at f = 1.0");

    // Conservative: for any fraction the spend never exceeds headroom*f.
    for (p, frac, headroom, cost) in [
        (0.7, 0.25, 1_000_000_i64, 61_i64),
        (0.9, 0.25, 300_000, 62),
        (0.55, 0.1, 12_345_678, 7),
        (1.0, 1.0, i64::MAX / 200_000, 99),
    ] {
        let f = kelly_binary(p, Cents::new(60), frac).unwrap();
        let n = kelly_contracts(
            p,
            Cents::new(60),
            frac,
            Cents::new(headroom),
            Cents::new(cost),
            i64::MAX,
        )
        .unwrap();
        let spend = (n as f64) * (cost as f64);
        assert!(
            spend <= headroom as f64 * f + 1e-6,
            "spend {spend} exceeds budget for p={p} frac={frac}"
        );
        assert!(n >= 0);
    }

    // Saturating scale: an astronomically large headroom cannot overflow.
    let n = kelly_contracts(
        1.0,
        Cents::new(50),
        1.0,
        Cents::new(i64::MAX),
        Cents::new(1),
        1_000,
    )
    .unwrap();
    assert_eq!(n, 1_000, "cap binds; no overflow panic");
}
