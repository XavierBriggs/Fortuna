//! Source-coupled adapters — the ONLY place in `fortuna-backtest` that may
//! name a concrete producer/venue. The crate's decoupling grep gate excludes
//! `src/sources/` for exactly this reason.
//!
//! Everything above this module (the records, the trait, the harness, the
//! gates, the sweep) is source-agnostic by construction; an adapter here maps a
//! concrete archive's native schema onto the generic [`crate::source`]
//! contracts.

pub mod aeolus_archive;
