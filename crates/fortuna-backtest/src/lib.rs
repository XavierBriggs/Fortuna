//! fortuna-backtest: generic, integrity-gated backtest subsystem (WS3).
//!
//! Replays any [`source::HistoricalSource`] through the proven WS1/WS2
//! scoring rules and produces an honest, overfitting-deflated GO/NO-GO
//! behind four integrity gates (G-PIT, G-DEAD, G-PARITY, G-TRUTH).
//!
//! **Decoupling invariant:** `crates/fortuna-backtest/src/` contains no
//! source-name literals. The only source-coupled code (`src/sources/`)
//! arrives in S6. A grep gate enforces this; see the BUILD_PLAN boundary.
//!
//! **S1 scope:** source/record contracts, portable JSONL serialization, and the
//! universe manifest.
//!
//! **S2 scope (this slice):** the replay harness — the as-of join (G-PIT, strict
//! `available_at < decided_at`), idempotent + deterministic replay through the
//! SAME `fortuna-scoring` rules and the SAME ledger write path as live
//! (G-PARITY). No deflation math, no adapters.

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

pub mod asof;
/// The real `validate` edge provider (W7): assembles the per-`(scope, config)`
/// OOS Brier-skill + CLV series + per-row label windows from a historical source,
/// scoring through the SAME `fortuna-scoring` path as live. Carries NO
/// source-name literals (the decoupling grep gate).
pub mod edge_provider;
pub mod harness;
pub mod manifest;
pub mod records;
pub mod source;
/// Source-coupled adapters (S6). The ONLY place in the crate where
/// source-name literals are permitted; the decoupling grep gate excludes
/// `src/sources/`.
pub mod sources;
pub mod sweep;
