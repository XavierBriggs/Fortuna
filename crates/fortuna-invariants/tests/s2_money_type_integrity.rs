//! S2 (CONSTITUTION structural invariant): money-type integrity.
//!
//! `Cents` is the integer-cent money type; every arithmetic op is CHECKED and
//! returns an error on overflow rather than wrapping or panicking (the spec's
//! "checked ops" rule, STANDARDS money rules). This pins that guarantee so a
//! future `+`/`*`/`-` that silently wraps cannot land.
//!
//! The type-LEVEL separation of `PerpPrice` from `Cents` (S2's "cannot
//! cross-assign" clause) is pinned by a compile_fail doc-test in src/lib.rs.
//! Fee rounding-against-us is exercised by the venue fee tests
//! (fortuna-venues). This file owns the checked-arithmetic clause.
//!
//! ADDITIONS-ONLY (protected crate): never weaken these assertions.

use fortuna_core::money::Cents;

#[test]
fn s2_money_uses_checked_arithmetic_and_never_wraps() {
    // Overflow / underflow are ERRORS, never silent wraps.
    assert!(
        Cents::new(i64::MAX).checked_add(Cents::new(1)).is_err(),
        "add overflow must error, not wrap to a negative balance"
    );
    assert!(
        Cents::new(i64::MIN).checked_sub(Cents::new(1)).is_err(),
        "sub underflow must error"
    );
    assert!(
        Cents::new(i64::MAX).checked_mul(2).is_err(),
        "mul overflow must error"
    );
    assert!(
        Cents::new(i64::MIN).checked_neg().is_err(),
        "negating i64::MIN overflows and must error"
    );

    // Well-formed arithmetic still succeeds and is EXACT (compare raw cents so
    // this does not depend on a PartialEq impl).
    assert_eq!(
        Cents::new(50).checked_add(Cents::new(50)).unwrap().raw(),
        100
    );
    assert_eq!(Cents::new(7).checked_mul(3).unwrap().raw(), 21);
    assert_eq!(
        Cents::new(100).checked_sub(Cents::new(40)).unwrap().raw(),
        60
    );
}
