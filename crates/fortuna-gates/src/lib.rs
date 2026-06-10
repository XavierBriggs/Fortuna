//! fortuna-gates: the universal gate pipeline (I1). Spec 5.3, checks 1-10.
//!
//! `GatedOrder` is a sealed type: private fields, constructible only by the
//! pipeline after all checks pass. Venues accept `GatedOrder` exclusively,
//! enforcing I1 at the type level. Every check emits an audit verdict record.
//! Fail-closed: any error in any check rejects the order.
//!
//! Checks (spec 5.3): halts, capital threshold, position caps, price sanity,
//! size sanity, fee-adjusted edge floor, rate limits (I3 dual token bucket),
//! idempotency, same-event exposure cap (via edges), internal netting.
//! The pipeline itself is T0.5; the sealed type ships first (T0.3) so the
//! Venue trait can bind to it.

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

mod order;

pub use order::GatedOrder;
