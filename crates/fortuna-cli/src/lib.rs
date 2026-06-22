//! fortuna-cli library surface — the command-handler modules that are testable
//! as a library (integration tests call them directly with an injected PgPool).
//!
//! The binary (`src/main.rs`) re-exports from here; tests import from here.
//! No production code lives in this file; only module declarations.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

pub mod backtest_cmd;
