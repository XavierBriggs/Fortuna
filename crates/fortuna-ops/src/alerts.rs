//! Degrade alert rules (F1; spec line 238: "budget breach degrades to
//! mechanical-only AND ALERTS").
//!
//! Pure rules over scrape DELTAS: the live composition reads the runner
//! counters each scrape, diffs against the previous scrape, and routes
//! whatever this returns through the SlackRouter. Budget breaches alert
//! on EVERY occurrence (one message per scrape, carrying the count — a
//! page storm helps nobody); other cognition failures alert only on a
//! sustained burst (a transient provider blip is not a page), with the
//! threshold in config.

use crate::slack::MessageKind;

/// Counter deltas since the previous scrape.
#[derive(Debug, Clone, Copy)]
pub struct DegradeSignals {
    pub budget_breaches_delta: u64,
    pub cognition_failures_delta: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct DegradeThresholds {
    /// Cognition failures within one scrape window at or above this
    /// alert (below: visible in metrics, silent in Slack).
    pub failure_alert_threshold: u64,
}

/// The rule. Deterministic; returns routed (kind, text) messages.
pub fn degrade_alerts(
    signals: &DegradeSignals,
    thresholds: &DegradeThresholds,
) -> Vec<(MessageKind, String)> {
    let mut out = Vec::new();
    if signals.budget_breaches_delta > 0 {
        out.push((
            MessageKind::Alert,
            format!(
                "cognition BUDGET BREACH: {} cycle(s) degraded to mechanical-only \
                 since the last scrape — the mind is cost-starved; review \
                 per_cycle/daily budget caps and model spend (audit kind=cognition, \
                 degrade=budget_exhausted)",
                signals.budget_breaches_delta
            ),
        ));
    }
    if thresholds.failure_alert_threshold > 0
        && signals.cognition_failures_delta >= thresholds.failure_alert_threshold
    {
        out.push((
            MessageKind::Alert,
            format!(
                "cognition failure burst: {} cycle(s) degraded since the last scrape \
                 (threshold {}) — provider/schema/refusal kinds are in the audit log \
                 (kind=cognition)",
                signals.cognition_failures_delta, thresholds.failure_alert_threshold
            ),
        ));
    }
    out
}
