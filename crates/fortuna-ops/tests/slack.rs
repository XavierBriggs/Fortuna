//! Slack router tests, written from spec Section 8 (channel routing; every
//! message is also an audit row — the caller audits the returned
//! `SentMessage`) and docs/research/ops/slack-api-2026-06-09/research.md
//! (`ok` envelope, 429/Retry-After, Block Kit approval shape).

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use fortuna_ops::{
    approval_message, MessageKind, OpsError, SentMessage, SlackConfig, SlackRouter, SlackTransport,
};

/// Records every call and replays scripted responses (default: a generic
/// `ok: true` envelope).
#[derive(Clone, Default)]
struct MockTransport {
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    script: Arc<Mutex<VecDeque<Result<Value, OpsError>>>>,
}

impl MockTransport {
    fn respond(&self, response: Result<Value, OpsError>) {
        self.script.lock().unwrap().push_back(response);
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl SlackTransport for MockTransport {
    async fn post_message(&self, token: &str, body: Value) -> Result<Value, OpsError> {
        self.calls.lock().unwrap().push((token.to_string(), body));
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(json!({"ok": true, "channel": "CDEF", "ts": "111.222"})))
    }
}

fn five_channel_config() -> SlackConfig {
    SlackConfig {
        channels: ["trading", "alerts", "review", "digest", "ops"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

fn five_channel_ids() -> BTreeMap<String, String> {
    [
        ("trading", "C0TRADING"),
        ("alerts", "C0ALERTS"),
        ("review", "C0REVIEW"),
        ("digest", "C0DIGEST"),
        ("ops", "C0OPS"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn router(mock: &MockTransport) -> SlackRouter {
    SlackRouter::new(
        &five_channel_config(),
        five_channel_ids(),
        "xoxb-test-token".to_string(),
        Box::new(mock.clone()),
    )
    .unwrap()
}

// ---------------------------------------------------------------- routing --

#[test]
fn route_maps_each_kind_per_spec_section_8() {
    assert_eq!(SlackRouter::route(MessageKind::Trading), "trading");
    assert_eq!(SlackRouter::route(MessageKind::Alert), "alerts");
    assert_eq!(SlackRouter::route(MessageKind::Review), "review");
    assert_eq!(SlackRouter::route(MessageKind::Digest), "digest");
    assert_eq!(SlackRouter::route(MessageKind::Ops), "ops");
}

#[test]
fn construction_succeeds_with_all_five_channels_and_ids() {
    let mock = MockTransport::default();
    let _ = router(&mock); // router() unwraps construction
}

#[test]
fn construction_fails_closed_when_a_channel_id_is_missing() {
    let mut ids = five_channel_ids();
    ids.remove("review");
    // SlackRouter intentionally has no Debug (it holds the bot token), so
    // unwrap_err() is unavailable; match instead.
    let result = SlackRouter::new(
        &five_channel_config(),
        ids,
        "xoxb-test-token".to_string(),
        Box::new(MockTransport::default()),
    );
    match result {
        Err(OpsError::MissingSecret { name }) => {
            assert_eq!(name, "FORTUNA_SLACK_CHANNEL_REVIEW");
        }
        Err(other) => panic!("expected MissingSecret, got {other:?}"),
        Ok(_) => panic!("construction unexpectedly succeeded"),
    }
}

#[test]
fn construction_fails_closed_when_a_route_target_is_not_configured() {
    let config = SlackConfig {
        channels: ["trading", "alerts", "review", "ops"] // no "digest"
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    let result = SlackRouter::new(
        &config,
        five_channel_ids(),
        "xoxb-test-token".to_string(),
        Box::new(MockTransport::default()),
    );
    match result {
        Err(OpsError::Config { .. }) => {}
        Err(other) => panic!("expected Config error, got {other:?}"),
        Ok(_) => panic!("construction unexpectedly succeeded"),
    }
}

// ------------------------------------------------------------------- send --

#[tokio::test]
async fn send_posts_routed_channel_id_text_and_bearer_token() {
    let mock = MockTransport::default();
    let r = router(&mock);
    r.send(MessageKind::Alert, "drawdown at 80% of limit")
        .await
        .unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    let (token, body) = &calls[0];
    assert_eq!(token, "xoxb-test-token");
    assert_eq!(body["channel"], "C0ALERTS");
    assert_eq!(body["text"], "drawdown at 80% of limit");
}

#[tokio::test]
async fn send_ok_returns_sent_message_for_the_audit_row() {
    let mock = MockTransport::default();
    mock.respond(Ok(json!({
        "ok": true,
        "channel": "C0TRADING",
        "ts": "1717933200.000100",
        "message": {}
    })));
    let r = router(&mock);
    let sent = r
        .send(MessageKind::Trading, "filled 10 @ 43c")
        .await
        .unwrap();
    assert_eq!(
        sent,
        SentMessage {
            channel_id: "C0TRADING".to_string(),
            text: "filled 10 @ 43c".to_string(),
            response_ts: Some("1717933200.000100".to_string()),
        }
    );
}

#[tokio::test]
async fn send_ok_without_ts_yields_none_response_ts() {
    let mock = MockTransport::default();
    mock.respond(Ok(json!({"ok": true})));
    let r = router(&mock);
    let sent = r.send(MessageKind::Ops, "heartbeat anomaly").await.unwrap();
    assert_eq!(sent.response_ts, None);
}

#[tokio::test]
async fn each_kind_posts_to_its_own_channel_id() {
    let mock = MockTransport::default();
    let r = router(&mock);
    for kind in [
        MessageKind::Trading,
        MessageKind::Alert,
        MessageKind::Review,
        MessageKind::Digest,
        MessageKind::Ops,
    ] {
        r.send(kind, "x").await.unwrap();
    }
    let channels: Vec<String> = mock
        .calls()
        .iter()
        .map(|(_, body)| body["channel"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        channels,
        vec!["C0TRADING", "C0ALERTS", "C0REVIEW", "C0DIGEST", "C0OPS"]
    );
}

// -------------------------------------------------------- failure parsing --

#[tokio::test]
async fn ok_false_with_http_200_becomes_a_typed_slack_error() {
    let mock = MockTransport::default();
    mock.respond(Ok(json!({"ok": false, "error": "channel_not_found"})));
    let r = router(&mock);
    let err = r.send(MessageKind::Alert, "x").await.unwrap_err();
    match err {
        OpsError::Slack { code } => assert_eq!(code, "channel_not_found"),
        other => panic!("expected Slack error, got {other:?}"),
    }
}

#[tokio::test]
async fn ok_false_without_an_error_code_fails_closed() {
    let mock = MockTransport::default();
    mock.respond(Ok(json!({"ok": false})));
    let r = router(&mock);
    let err = r.send(MessageKind::Alert, "x").await.unwrap_err();
    assert!(matches!(err, OpsError::Slack { .. }), "got {err:?}");
}

#[tokio::test]
async fn response_missing_the_ok_field_fails_closed() {
    let mock = MockTransport::default();
    mock.respond(Ok(json!({"channel": "C0ALERTS"})));
    let r = router(&mock);
    let err = r.send(MessageKind::Alert, "x").await.unwrap_err();
    assert!(matches!(err, OpsError::Slack { .. }), "got {err:?}");
}

#[tokio::test]
async fn http_429_surfaces_as_typed_rate_limited_with_retry_after() {
    let mock = MockTransport::default();
    mock.respond(Err(OpsError::RateLimited {
        retry_after_secs: Some(30),
    }));
    let r = router(&mock);
    let err = r
        .send(MessageKind::Digest, "daily digest")
        .await
        .unwrap_err();
    match err {
        OpsError::RateLimited { retry_after_secs } => assert_eq!(retry_after_secs, Some(30)),
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

// -------------------------------------------------------------- block kit --

#[test]
fn approval_message_matches_the_research_doc_shape_exactly() {
    // Shape: docs/research/ops/slack-api-2026-06-09/research.md,
    // "Block Kit minimal interactive message".
    let blocks = approval_message(
        "*Approve flatten-all for `alpha-1`?*",
        "approve_flatten",
        "reject_flatten",
        "halt:01JXAMPLEULID",
    );
    let expected = json!([
        {
            "type": "section",
            "text": { "type": "mrkdwn", "text": "*Approve flatten-all for `alpha-1`?*" }
        },
        {
            "type": "actions",
            "block_id": "halt:01JXAMPLEULID",
            "elements": [
                {
                    "type": "button",
                    "text": { "type": "plain_text", "text": "Approve" },
                    "style": "primary",
                    "action_id": "approve_flatten",
                    "value": "halt:01JXAMPLEULID"
                },
                {
                    "type": "button",
                    "text": { "type": "plain_text", "text": "Reject" },
                    "style": "danger",
                    "action_id": "reject_flatten",
                    "value": "halt:01JXAMPLEULID"
                }
            ]
        }
    ]);
    assert_eq!(blocks, expected);
}
