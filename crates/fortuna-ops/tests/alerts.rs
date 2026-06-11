//! F1: the ops-layer degrade alert rule (spec line 238: "budget breach
//! degrades to mechanical-only AND ALERTS"). The runner counts breaches
//! and audits every degraded cycle; THIS rule turns the scrape deltas
//! into routed messages. Budget breaches alert on EVERY occurrence;
//! other cognition failures alert on a sustained threshold (transient
//! provider blips are ops-channel noise, not pages).

use fortuna_ops::alerts::{degrade_alerts, DegradeSignals, DegradeThresholds};
use fortuna_ops::MessageKind;

fn thresholds() -> DegradeThresholds {
    DegradeThresholds {
        failure_alert_threshold: 5,
    }
}

#[test]
fn every_budget_breach_alerts() {
    let alerts = degrade_alerts(
        &DegradeSignals {
            budget_breaches_delta: 1,
            cognition_failures_delta: 1,
        },
        &thresholds(),
    );
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].0, MessageKind::Alert);
    assert!(alerts[0].1.contains("budget"), "{}", alerts[0].1);

    // Three breaches in one scrape: still one message, carrying the count
    // (a page storm helps nobody).
    let alerts = degrade_alerts(
        &DegradeSignals {
            budget_breaches_delta: 3,
            cognition_failures_delta: 3,
        },
        &thresholds(),
    );
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0].1.contains('3'));
}

#[test]
fn failure_threshold_separates_blips_from_outages() {
    // Below threshold (and no breaches): nothing pages.
    let alerts = degrade_alerts(
        &DegradeSignals {
            budget_breaches_delta: 0,
            cognition_failures_delta: 4,
        },
        &thresholds(),
    );
    assert!(alerts.is_empty(), "{alerts:?}");

    // At threshold: an Alert names the failure burst.
    let alerts = degrade_alerts(
        &DegradeSignals {
            budget_breaches_delta: 0,
            cognition_failures_delta: 5,
        },
        &thresholds(),
    );
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].0, MessageKind::Alert);
    assert!(alerts[0].1.contains("degraded"), "{}", alerts[0].1);

    // Quiet scrape: silence.
    assert!(degrade_alerts(
        &DegradeSignals {
            budget_breaches_delta: 0,
            cognition_failures_delta: 0,
        },
        &thresholds(),
    )
    .is_empty());
}
