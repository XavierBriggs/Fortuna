//! fortuna-venues: the Venue trait and adapters. Spec 5.2.
//!
//! `Venue::place` accepts only `GatedOrder` (type-level I1). `FeeModel` is a
//! config-schedule interpreter (quadratic p(1-p) | flat bps | tiered, with
//! maker/taker variants, category multipliers, effective_date versioning);
//! per-fill reconciliation of charged vs modeled fee writes a discrepancy on
//! mismatch. Market carries settlement metadata (oracle type, resolution
//! source, expected lag, benchmark anchoring inputs).
//!
//! Adapters: sim/ (seeded fault injection: ack delay, dropped/dup fills,
//! crash hooks - the DST workhorse), kalshi/ (built ONLY against
//! fixtures/kalshi/), polymarket/ (fixtures-gated; stub + GAPS entry if absent).
