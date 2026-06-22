// PROTECTED CRATE: invariant tests live in tests/. See CLAUDE.md.

//! Executable invariants I1-I7 (spec Section 3).
//!
//! The doc-tests below are part of I1's enforcement: `GatedOrder` must be
//! unconstructible outside the gate pipeline, at compile time.
//!
//! Path witness (guards the compile_fail siblings against passing vacuously
//! if the type were ever moved or renamed):
//!
//! ```
//! use fortuna_gates::GatedOrder;
//! fn _witness(_: &GatedOrder) {}
//! ```
//!
//! I1 (type-level): constructing a `GatedOrder` outside fortuna-gates does
//! not compile — its fields are private and its only constructor is
//! pub(crate) inside the pipeline:
//!
//! ```compile_fail
//! let forged = fortuna_gates::GatedOrder {};
//! ```
//!
//! I1 (type-level, serde): `GatedOrder` implements `Serialize` for audit but
//! deliberately NOT `Deserialize` — deserialization would be a constructor
//! bypass:
//!
//! ```compile_fail
//! fn requires_deserialize<T: serde::de::DeserializeOwned>() {}
//! requires_deserialize::<fortuna_gates::GatedOrder>();
//! ```
//!
//! Perp path witness (spec 5.15 / T5.B3 ADDITION; guards the perp
//! compile_fail siblings against passing vacuously if the type were ever
//! moved or renamed):
//!
//! ```
//! use fortuna_gates::perp::GatedPerpOrder;
//! fn _perp_witness(_: &GatedPerpOrder) {}
//! ```
//!
//! I1 (type-level, perps): constructing a `GatedPerpOrder` outside
//! fortuna-gates does not compile — its fields are private and its only
//! constructor is pub(crate) inside the perp gate arm:
//!
//! ```compile_fail
//! let forged = fortuna_gates::perp::GatedPerpOrder {};
//! ```
//!
//! I1 (type-level, serde, perps): `GatedPerpOrder` implements `Serialize`
//! for audit but deliberately NOT `Deserialize`:
//!
//! ```compile_fail
//! fn requires_deserialize<T: serde::de::DeserializeOwned>() {}
//! requires_deserialize::<fortuna_gates::perp::GatedPerpOrder>();
//! ```
//!
//! S2 (type-level, money; CONSTITUTION structural invariant): `PerpPrice` and
//! `Cents` are separate types and a value of one cannot be used where the other
//! is expected (no implicit conversion; perps reach `Cents` only through the
//! explicit, round-against-us conversion fns). Path witness (so the sibling
//! fails for the RIGHT reason, not because a type was renamed):
//!
//! ```
//! fn _needs_cents(_: fortuna_core::money::Cents) {}
//! fn _ok(c: fortuna_core::money::Cents) { _needs_cents(c) }
//! ```
//!
//! Separation (must NOT compile — a `PerpPrice` is not a `Cents`):
//!
//! ```compile_fail
//! fn _needs_cents(_: fortuna_core::money::Cents) {}
//! fn _bad(p: fortuna_core::perp::PerpPrice) { _needs_cents(p) }
//! ```
