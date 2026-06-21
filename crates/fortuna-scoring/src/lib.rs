//! fortuna-scoring: the pure, decoupled scoring library (spec 5.5, 5.15).
//!
//! This crate carries ONLY the scoring math: proper scoring rules over
//! immutable predictive claims plus the calibration/aggregation sample shapes
//! the scorecard layer consumes. It depends on **nothing but `std`, `serde`,
//! and `thiserror`** — no `sqlx`, `tokio`, `reqwest`, Postgres, `Clock`, or any
//! `fortuna-*` crate — so "the math is not coupled to IO or a producer" is
//! compiler-enforced, and WS3 can reuse it without pulling in cognition.
//!
//! `scope`, `producer`, and `source` are strings handled only at the
//! aggregation/scorecard layer in other crates; no metric here ever sees them.

pub mod corp;
pub mod dm;
pub mod murphy_diagram;
pub mod pav;
pub mod pit;
pub mod rules;
pub mod samples;
pub mod scorecard;

pub use corp::*;
pub use dm::*;
pub use murphy_diagram::*;
pub use pav::*;
pub use pit::*;
pub use rules::*;
pub use samples::*;
pub use scorecard::*;
