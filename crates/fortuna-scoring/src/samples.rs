//! Aggregation sample shapes consumed by the scorecard layer.
//!
//! These are the immutable `(prediction, realized)` pairs the calibration and
//! scorecard aggregation reduces over. They are deliberately minimal and pure:
//! a `CalibrationSample` is one binary forecast/outcome pair (for Brier/CORP/DM
//! reliability), and a `ScalarSample` is one quantile-ladder forecast paired
//! with its realized value (for CRPS/PIT). The aggregation/scorecard layer in
//! other crates fills these from the ledger; no producer/source/scope string
//! ever appears here.

use crate::rules::Quantile;
use serde::{Deserialize, Serialize};

/// One binary forecast paired with its realized outcome.
///
/// `p` is the forecast probability of the event; `outcome` is whether the event
/// occurred. Consumed by the calibration/CORP/Diebold–Mariano aggregation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationSample {
    /// Forecast probability of the event, in [0, 1].
    pub p: f64,
    /// Whether the event was realized.
    pub outcome: bool,
}

/// One scalar quantile-ladder forecast paired with its realized value.
///
/// `quantiles` is the predictive quantile ladder; `realized` is the value that
/// actually occurred. Consumed by the CRPS/PIT aggregation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScalarSample {
    /// Predictive quantile ladder.
    pub quantiles: Vec<Quantile>,
    /// Realized value.
    pub realized: f64,
}
