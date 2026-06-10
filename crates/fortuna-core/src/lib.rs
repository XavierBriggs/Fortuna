//! fortuna-core: deterministic foundation. Spec 5.1.
//!
//! Owns: `Clock` (real + sim), `Cents` (checked integer-cent money), ULID ids,
//! `BusEvent`, the single-threaded deterministic event bus, and replay
//! record/playback. No IO, no Postgres, no venues. Determinism rule: identical
//! seed + identical inputs => byte-identical event stream (T0.2 test).
//!
//! Build order: T0.1 (Clock/Cents/ids) then T0.2 (bus/replay). See BUILD_PLAN.md.

pub mod clock {
    //! Injected time. `SystemTime::now()` outside this module is a defect.
}
pub mod money {
    //! `Cents(i64)` newtype, checked arithmetic, fee rounding always against us.
}
pub mod bus {
    //! `BusEvent` (bus message; distinct from canonical events, spec 5.12),
    //! deterministic single-threaded dispatch, replay recorder/player.
}
