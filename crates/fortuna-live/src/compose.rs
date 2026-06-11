//! Composition wiring for cognition (T4.1 requirements 3 + 4) — the two
//! GAPS residue lines, closed as code with call sites:
//!
//! - `DegradeScrape` is the scrape-delta consumer for
//!   `fortuna_ops::alerts::degrade_alerts`: it remembers the last-seen
//!   counter totals, diffs per scrape (saturating — a process restart's
//!   counter reset is not a burst), and returns the alerts the daemon
//!   routes through Slack (every routed message also writes an audit
//!   row at the routing site).
//! - `calibration_for_scope` fetches the scope's latest fitted params +
//!   resolved history from the ledger and produces the
//!   `CalibrationContext` + sizing quality that feed
//!   `SynthesisStrategy` and `SimRunner::set_calibration_quality`. No
//!   params row => `None` (the strategy structurally prices no edge —
//!   that IS the design); a params row that does not PARSE is corrupt
//!   configuration and errors loudly, never a silent "uncalibrated".

use fortuna_cognition::beliefs::calibration_curve;
use fortuna_cognition::calibration::{calibration_quality, CalibrationParams};
use fortuna_cognition::cycle::CalibrationContext;
use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo, LedgerError};
use fortuna_ops::alerts::{degrade_alerts, DegradeSignals, DegradeThresholds};
use fortuna_ops::MessageKind;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComposeError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error(
        "calibration_params row for {scope} does not parse: {reason} (corrupt config; refusing)"
    )]
    CorruptParams { scope: String, reason: String },
}

/// Reliability-curve bucket count for the quality computation. Ten
/// deciles is the weekly-review convention; quality only needs a stable
/// grouping, not resolution.
const QUALITY_BUCKETS: usize = 10;

/// Fetch the synthesis scope's calibration state from the ledger.
/// Returns the context for `SynthesisConfig.calibration` and the quality
/// for `SimRunner::set_calibration_quality` (both fail-closed shapes).
pub async fn calibration_for_scope(
    params: &CalibrationParamsRepo,
    beliefs: &BeliefsRepo,
    model_id: &str,
    strategy: &str,
    category: &str,
    kind: &str,
) -> Result<(Option<CalibrationContext>, f64), ComposeError> {
    let row = params.latest(model_id, strategy, category, kind).await?;
    let stats = beliefs.resolved_stats(category).await?;
    let resolved_n = stats.len();
    let samples: Vec<(f64, bool)> = stats.iter().map(|s| (s.p, s.outcome)).collect();
    let curve = calibration_curve(&samples, QUALITY_BUCKETS);
    let quality = calibration_quality(&curve, resolved_n);

    let ctx = match row {
        None => None,
        Some(r) => {
            let parsed: CalibrationParams =
                serde_json::from_value(r.params).map_err(|e| ComposeError::CorruptParams {
                    scope: format!("{model_id}/{strategy}/{category}/{kind}"),
                    reason: e.to_string(),
                })?;
            Some(CalibrationContext {
                params: parsed,
                resolved_n,
            })
        }
    };
    Ok((ctx, quality))
}

/// The degrade-alert scrape consumer (GAPS residue line 1). One instance
/// lives for the daemon's lifetime; feed it the runner's counter TOTALS
/// each scrape and route what it returns.
pub struct DegradeScrape {
    thresholds: DegradeThresholds,
    last_budget_breaches: u64,
    last_cognition_failures: u64,
}

impl DegradeScrape {
    pub fn new(thresholds: DegradeThresholds) -> DegradeScrape {
        DegradeScrape {
            thresholds,
            last_budget_breaches: 0,
            last_cognition_failures: 0,
        }
    }

    /// Diff the totals against the previous scrape and produce alerts.
    /// Saturating: a counter that went BACKWARD (restart) yields a zero
    /// delta, not an underflowed burst.
    pub fn scrape(
        &mut self,
        budget_breaches_total: u64,
        cognition_failures_total: u64,
    ) -> Vec<(MessageKind, String)> {
        let signals = DegradeSignals {
            budget_breaches_delta: budget_breaches_total.saturating_sub(self.last_budget_breaches),
            cognition_failures_delta: cognition_failures_total
                .saturating_sub(self.last_cognition_failures),
        };
        self.last_budget_breaches = budget_breaches_total;
        self.last_cognition_failures = cognition_failures_total;
        degrade_alerts(&signals, &self.thresholds)
    }
}
