//! Purging + embargo (research §2 — "the #1 lie-prevention").
//!
//! A belief's label is `Y = f([t0, t1])` (issue-time `t0`, resolution-time
//! `t1`). Plain cross-validation leaks because overlapping label windows put
//! `X_t ≈ X_{t+1}` and `Y_t ≈ Y_{t+1}` in different folds; the leak inflates OOS
//! performance and so **understates** overfitting.
//!
//! - **Purge (exact, verbatim §2):** a train label `i` overlaps a test label `j`
//!   iff `train.t0 ≤ test.t1 AND train.t1 ≥ test.t0`. Drop matching observations
//!   **from train only**.
//! - **Embargo (one-sided, verbatim §2):** also drop a train window that starts
//!   within `h` *after* a test window's end (`test.t1 ≤ train.t0 ≤ test.t1 + h`).
//!   This is implemented by extending each test window to `t1 + h` **before** the
//!   overlap test. It is one-sided — a pre-test train window is never embargoed.
//!
//! Time is a plain integer-millis newtype here so the module needs no external
//! time dependency (`chrono`/`time` would break the crate's purity invariant).

use serde::{Deserialize, Serialize};

/// A label window `[t0, t1]` in integer "millis" (any consistent integer time
/// unit). `t0` is issue-time, `t1` is resolution-time; `t1 ≥ t0` is expected but
/// not enforced (a degenerate `t1 < t0` simply never overlaps anything).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelWindow {
    /// Window start (issue-time), inclusive.
    pub t0: i64,
    /// Window end (resolution-time), inclusive.
    pub t1: i64,
}

impl LabelWindow {
    /// Construct a window from `[t0, t1]`.
    pub fn new(t0: i64, t1: i64) -> Self {
        Self { t0, t1 }
    }
}

/// A non-negative time span in the same integer-millis unit as [`LabelWindow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Duration(i64);

impl Duration {
    /// A zero-length embargo (purge only, no embargo).
    pub fn zero() -> Self {
        Duration(0)
    }

    /// An embargo of `millis` time units. Negative values are clamped to zero
    /// (an embargo cannot be negative — that would drop *pre-test* windows,
    /// breaking the one-sided property).
    pub fn from_millis(millis: i64) -> Self {
        Duration(millis.max(0))
    }

    /// The span as integer millis (always `≥ 0`).
    pub fn millis(self) -> i64 {
        self.0
    }
}

/// Returns the indices of `train` to **KEEP** after purging every train window
/// that overlaps any (embargo-extended) test window.
///
/// Overlap test: `train.t0 ≤ test.t1' AND train.t1 ≥ test.t0`, where
/// `test.t1' = test.t1 + embargo` (one-sided extension applied before the test).
///
/// Degenerate inputs are well-defined and never panic: an empty `test` set
/// purges nothing (every train index is kept); an empty `train` set yields an
/// empty keep-list.
pub fn purge_embargo(train: &[LabelWindow], test: &[LabelWindow], embargo: Duration) -> Vec<usize> {
    let h = embargo.millis();
    train
        .iter()
        .enumerate()
        .filter_map(|(i, tr)| {
            let overlaps_any = test.iter().any(|te| {
                let te_end = te.t1.saturating_add(h);
                // overlap iff train.t0 <= test.t1' && train.t1 >= test.t0
                tr.t0 <= te_end && tr.t1 >= te.t0
            });
            if overlaps_any {
                None
            } else {
                Some(i)
            }
        })
        .collect()
}
