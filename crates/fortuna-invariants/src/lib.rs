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
