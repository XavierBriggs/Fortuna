//! fortuna-core: deterministic foundation. Spec 5.1.
//!
//! Owns: `Clock` (real + sim), `Cents` (checked integer-cent money), ULID ids,
//! `BusEvent`, the single-threaded deterministic event bus, and replay
//! record/playback. No IO, no Postgres, no venues. Determinism rule: identical
//! seed + identical inputs => byte-identical event stream (T0.2 test).

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod clock;
pub mod ids;
pub mod money;

pub mod bus {
    //! `BusEvent` (bus message; distinct from canonical events, spec 5.12),
    //! deterministic single-threaded dispatch, replay recorder/player.
    //! Implemented in T0.2.
}
