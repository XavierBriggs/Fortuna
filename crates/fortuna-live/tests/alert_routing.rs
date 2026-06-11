//! T4.1 hard requirement 3 tail: degrade-scrape alerts ROUTE to Slack
//! (when a router is configured) and ALWAYS land as audit rows (spec 8:
//! every outbound message is also an audit row). A Slack send failure is
//! audited, never silently dropped. Written red-first against
//! daemon::route_alerts; the transport is a recording mock — no network.

use async_trait::async_trait;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;
use fortuna_live::daemon::route_alerts;
use fortuna_ops::{MessageKind, OpsError, SlackConfig, SlackRouter, SlackTransport};
use fortuna_runner::{AuditSink, RunnerConfig, RunnerError, SimRunner};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Shared-handle audit sink so the test can inspect the rows
/// `apply_external_alert` writes AFTER the runner consumes the box —
/// the audit half of "route AND audit" (spec 8).
#[derive(Clone, Default)]
struct SharedSink(Arc<Mutex<Vec<(String, serde_json::Value)>>>);

impl SharedSink {
    fn alert_rows(&self) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| k == "alert")
            .map(|(_, p)| p.clone())
            .collect()
    }
}

impl AuditSink for SharedSink {
    fn append(
        &mut self,
        kind: &str,
        _ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        self.0.lock().unwrap().push((kind.to_string(), payload));
        Ok(())
    }
}

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
}

#[derive(Clone, Default)]
struct MockTransport {
    calls: Arc<Mutex<Vec<Value>>>,
    fail: bool,
}

#[async_trait]
impl SlackTransport for MockTransport {
    async fn post_message(&self, _token: &str, body: Value) -> Result<Value, OpsError> {
        self.calls.lock().unwrap().push(body);
        if self.fail {
            Err(OpsError::Slack {
                code: "channel_not_found".into(),
            })
        } else {
            Ok(json!({"ok": true, "channel": "C0OPS", "ts": "111.222"}))
        }
    }
}

fn router(mock: MockTransport) -> SlackRouter {
    let cfg = SlackConfig {
        channels: ["trading", "alerts", "review", "digest", "ops"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let ids: BTreeMap<String, String> = [
        ("trading", "C0TRADING"),
        ("alerts", "C0ALERTS"),
        ("review", "C0REVIEW"),
        ("digest", "C0DIGEST"),
        ("ops", "C0OPS"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    SlackRouter::new(&cfg, ids, "xoxb-test".to_string(), Box::new(mock)).unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula=\"quadratic\"\neffective_date=\"2026-01-01\"\ntaker_coeff=\"0.07\"\nmaker_coeff=\"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn runner_with(sink: SharedSink) -> SimRunner {
    let config = RunnerConfig {
        seed: 1,
        gate_config: toml::from_str(
            "[global]\nmax_total_exposure_cents=1\nmax_daily_loss_cents=1\nmin_order_contracts=1\nmax_order_contracts=1\nprice_band_cents=1\nmax_cross_cents=1\nper_market_exposure_cents=1\nper_event_exposure_cents=1\nrequire_event_mapping=false\n[rate.sim]\nburst=1\nsustained_per_min=1\nmarket_burst=1\nmarket_sustained_per_min=1\n",
        )
        .unwrap(),
        exec_policy: fortuna_exec::ExecPolicy::default(),
        envelopes: BTreeMap::new(),
        max_daily_loss: Cents::new(1),
        fee_model: fee_model(),
        markets: vec![],
        starting_cash: Cents::new(1),
        faults: FaultConfig::none(1),
        mark_policy: MarkPolicy {
            max_book_age_ms: 1,
            max_spread_cents: 1,
        },
        max_sets_per_proposal: 1,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    };
    SimRunner::new(config, vec![], Box::new(sink), t0()).unwrap()
}

#[tokio::test]
async fn alerts_route_to_slack_and_audit() {
    let mock = MockTransport::default();
    let r = router(mock.clone());
    let sink = SharedSink::default();
    let mut runner = runner_with(sink.clone());
    let alerts = vec![
        (MessageKind::Alert, "budget breach x2".to_string()),
        (MessageKind::Ops, "cognition failures x6".to_string()),
    ];
    let failures = route_alerts(Some(&r), &mut runner, &alerts).await;
    assert_eq!(failures, 0);
    assert_eq!(
        mock.calls.lock().unwrap().len(),
        2,
        "both alerts posted to Slack"
    );
    // The AUDIT half (spec 8): each routed alert also wrote an audit row
    // carrying its message + the slack ts marker.
    let rows = sink.alert_rows();
    assert_eq!(rows.len(), 2, "both alerts audited");
    assert!(
        rows.iter()
            .any(|p| p["message"].as_str().unwrap_or("").contains("slack ts")),
        "routed-ok alert records the slack ts"
    );
}

#[tokio::test]
async fn no_router_still_audits_zero_posts() {
    let sink = SharedSink::default();
    let mut runner = runner_with(sink.clone());
    let alerts = vec![(MessageKind::Alert, "x".to_string())];
    // No router => no network; the audit row is the ONLY effect (the
    // operator still sees the alert in the trail).
    let failures = route_alerts(None, &mut runner, &alerts).await;
    assert_eq!(failures, 0);
    assert_eq!(
        sink.alert_rows().len(),
        1,
        "the alert is audited with no router"
    );
}

#[tokio::test]
async fn slack_send_failure_is_counted_never_silent() {
    let mock = MockTransport {
        fail: true,
        ..MockTransport::default()
    };
    let r = router(mock.clone());
    let sink = SharedSink::default();
    let mut runner = runner_with(sink.clone());
    let alerts = vec![(MessageKind::Alert, "breach".to_string())];
    let failures = route_alerts(Some(&r), &mut runner, &alerts).await;
    assert_eq!(
        failures, 1,
        "the failed send is counted for dead-man escalation"
    );
    assert_eq!(
        mock.calls.lock().unwrap().len(),
        1,
        "the post was attempted"
    );
    // Even a FAILED send is audited (never silently dropped) — with the
    // failure recorded in the row.
    let rows = sink.alert_rows();
    assert_eq!(rows.len(), 1, "the failed-send alert is audited");
    assert!(
        rows[0]["message"]
            .as_str()
            .unwrap_or("")
            .contains("SLACK SEND FAILED"),
        "the audit row records the send failure"
    );
}

#[test]
fn build_router_none_token_is_slack_disabled_not_an_error() {
    use fortuna_live::daemon::build_slack_router;
    let cfg = SlackConfig {
        channels: ["trading", "alerts", "review", "digest", "ops"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let r = build_slack_router(
        &cfg,
        None,
        BTreeMap::new(),
        Box::new(MockTransport::default()),
    )
    .unwrap();
    assert!(
        r.is_none(),
        "no bot token => Slack disabled (alerts still audit)"
    );
}

#[test]
fn build_router_with_token_and_ids_succeeds() {
    use fortuna_live::daemon::build_slack_router;
    let cfg = SlackConfig {
        channels: ["trading", "alerts", "review", "digest", "ops"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let ids: BTreeMap<String, String> = [
        ("trading", "C0TRADING"),
        ("alerts", "C0ALERTS"),
        ("review", "C0REVIEW"),
        ("digest", "C0DIGEST"),
        ("ops", "C0OPS"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let r = build_slack_router(
        &cfg,
        Some("xoxb-x"),
        ids,
        Box::new(MockTransport::default()),
    )
    .unwrap();
    assert!(r.is_some());
}

#[test]
fn build_router_missing_channel_id_is_loud_not_silent_none() {
    use fortuna_live::daemon::build_slack_router;
    let cfg = SlackConfig {
        channels: ["trading", "alerts", "review", "digest", "ops"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    // Token present but the env lacks a channel id the config names: a
    // half-configured Slack must NOT look like "Slack off".
    let partial: BTreeMap<String, String> = [("trading", "C0TRADING")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let r = build_slack_router(
        &cfg,
        Some("xoxb-x"),
        partial,
        Box::new(MockTransport::default()),
    );
    assert!(
        r.is_err(),
        "missing channel id with a token present is a loud error"
    );
}
