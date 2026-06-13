//! T4.2 item 2(v) Sub-slice A1 — Slack Socket Mode listener DECISION LOGIC.
//! Written from docs/research/ops/slack-api-2026-06-09/research.md. Proves the
//! safety teeth: I2 re-arm refusal, the user-id allow-list, halt-only routing to
//! the injected sink, and untrusted-data handling (spec 5.11). The envelope LOOP
//! (ack/dedup/reconnect over a WS transport) is Sub-slice A2; daemon wiring +
//! real WSS + the xapp- token are deferred (GAPS).

use async_trait::async_trait;
use fortuna_ops::OpsError;
use fortuna_ops::{
    dispatch_envelope, EnvelopeOutcome, EnvelopeType, EphemeralSender, HaltRequestSink,
    SlackEnvelope, SocketModeConfig, ACTION_REQUEST_HALT, ACTION_REQUEST_REARM,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Mutex;

struct MockSink {
    calls: Mutex<Vec<(String, String)>>,
    fail: bool,
}
impl MockSink {
    fn new() -> Self {
        MockSink {
            calls: Mutex::new(Vec::new()),
            fail: false,
        }
    }
    fn failing() -> Self {
        MockSink {
            calls: Mutex::new(Vec::new()),
            fail: true,
        }
    }
    fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().unwrap().clone()
    }
}
#[async_trait]
impl HaltRequestSink for MockSink {
    async fn request_halt(&self, envelope_id: &str, reason: &str) -> Result<(), OpsError> {
        self.calls
            .lock()
            .unwrap()
            .push((envelope_id.to_string(), reason.to_string()));
        if self.fail {
            Err(OpsError::Transport {
                reason: "mock sink failure".into(),
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct MockEphemeral {
    sent: Mutex<Vec<String>>,
}
impl MockEphemeral {
    fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }
}
#[async_trait]
impl EphemeralSender for MockEphemeral {
    async fn send_ephemeral(&self, text: &str) -> Result<(), OpsError> {
        self.sent.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

fn config() -> SocketModeConfig {
    SocketModeConfig {
        allowed_user_ids: ["U_OK".to_string()].into_iter().collect::<BTreeSet<_>>(),
        allowed_team_id: None,
    }
}

fn interactive(envelope_id: &str, payload: Value) -> SlackEnvelope {
    SlackEnvelope {
        envelope_id: envelope_id.to_string(),
        kind: EnvelopeType::Interactive,
        payload: Some(payload),
        accepts_response_payload: false,
    }
}

fn block_actions(user: &str, action_id: &str, value: &str) -> Value {
    json!({
        "user": { "id": user },
        "team": { "id": "T_HOME" },
        "actions": [ { "action_id": action_id, "value": value } ]
    })
}

async fn run(
    env: &SlackEnvelope,
    cfg: &SocketModeConfig,
) -> (EnvelopeOutcome, MockSink, MockEphemeral) {
    let sink = MockSink::new();
    let eph = MockEphemeral::default();
    let outcome = dispatch_envelope(env, cfg, &sink, &eph).await;
    (outcome, sink, eph)
}

// --------------------------------------------------------------------------

#[tokio::test]
async fn allow_listed_kill_request_routes_to_the_sink() {
    let env = interactive(
        "e1",
        block_actions("U_OK", ACTION_REQUEST_HALT, "runaway XBTUSD"),
    );
    let (outcome, sink, eph) = run(&env, &config()).await;
    assert_eq!(outcome, EnvelopeOutcome::HaltRequested);
    assert_eq!(
        sink.calls(),
        vec![("e1".to_string(), "runaway XBTUSD".to_string())]
    );
    assert!(eph.sent().is_empty(), "no ephemeral on the happy path");
}

#[tokio::test]
async fn rearm_action_is_refused_and_emits_no_halt() {
    // I2: a re-arm control over Slack must NEVER un-halt. No sink call.
    let env = interactive("e2", block_actions("U_OK", ACTION_REQUEST_REARM, "oops"));
    let (outcome, sink, eph) = run(&env, &config()).await;
    assert_eq!(outcome, EnvelopeOutcome::RearmRefused);
    assert!(sink.calls().is_empty(), "re-arm must emit NO halt-request");
    assert_eq!(eph.sent().len(), 1);
    assert!(
        eph.sent()[0].contains("Re-arm is not available"),
        "explicit refusal"
    );
}

#[tokio::test]
async fn non_allow_listed_user_is_unauthorized_no_halt() {
    let env = interactive("e3", block_actions("U_INTRUDER", ACTION_REQUEST_HALT, "x"));
    let (outcome, sink, eph) = run(&env, &config()).await;
    assert_eq!(outcome, EnvelopeOutcome::Unauthorized);
    assert!(sink.calls().is_empty());
    assert_eq!(eph.sent().len(), 1);
    assert!(eph.sent()[0].contains("Not authorized"));
}

#[tokio::test]
async fn empty_allow_list_is_fail_closed() {
    let cfg = SocketModeConfig {
        allowed_user_ids: BTreeSet::new(),
        allowed_team_id: None,
    };
    let env = interactive("e4", block_actions("U_OK", ACTION_REQUEST_HALT, "x"));
    let (outcome, sink, _eph) = run(&env, &cfg).await;
    assert_eq!(
        outcome,
        EnvelopeOutcome::Unauthorized,
        "empty allow-list = nobody"
    );
    assert!(sink.calls().is_empty());
}

#[tokio::test]
async fn untrusted_value_is_carried_as_data_not_executed() {
    // An injection-shaped value must be carried verbatim as a bounded reason
    // string — never interpreted (spec 5.11). No panic.
    let nasty = "]; DROP TABLE halt_events; --<script>alert(1)</script>";
    let env = interactive("e5", block_actions("U_OK", ACTION_REQUEST_HALT, nasty));
    let (outcome, sink, _eph) = run(&env, &config()).await;
    assert_eq!(outcome, EnvelopeOutcome::HaltRequested);
    assert_eq!(sink.calls()[0].1, nasty, "carried verbatim as opaque data");
}

#[tokio::test]
async fn an_overlong_reason_is_bounded() {
    let long = "A".repeat(5_000);
    let env = interactive("e6", block_actions("U_OK", ACTION_REQUEST_HALT, &long));
    let (_outcome, sink, _eph) = run(&env, &config()).await;
    assert_eq!(
        sink.calls()[0].1.chars().count(),
        500,
        "reason bounded to 500 chars"
    );
}

#[tokio::test]
async fn unknown_action_id_is_a_no_op() {
    let env = interactive(
        "e7",
        block_actions("U_OK", "fortuna_some_future_button", "x"),
    );
    let (outcome, sink, eph) = run(&env, &config()).await;
    assert_eq!(outcome, EnvelopeOutcome::IgnoredUnknownAction);
    assert!(sink.calls().is_empty());
    assert!(eph.sent().is_empty());
}

#[tokio::test]
async fn wrong_team_is_dropped_before_acting() {
    let cfg = SocketModeConfig {
        allowed_user_ids: ["U_OK".to_string()].into_iter().collect(),
        allowed_team_id: Some("T_HOME".to_string()),
    };
    // Same allow-listed user, but the envelope's team is foreign.
    let mut payload = block_actions("U_OK", ACTION_REQUEST_HALT, "x");
    payload["team"]["id"] = json!("T_ATTACKER");
    let env = interactive("e8", payload);
    let (outcome, sink, _eph) = run(&env, &cfg).await;
    assert_eq!(outcome, EnvelopeOutcome::WrongTeam);
    assert!(sink.calls().is_empty());
}

#[tokio::test]
async fn slash_kill_from_allow_listed_user_routes_to_sink() {
    let env = SlackEnvelope {
        envelope_id: "e9".to_string(),
        kind: EnvelopeType::SlashCommands,
        payload: Some(json!({
            "user_id": "U_OK", "team_id": "T_HOME",
            "command": "/fortuna-kill", "text": "flatten now"
        })),
        accepts_response_payload: true,
    };
    let (outcome, sink, _eph) = run(&env, &config()).await;
    assert_eq!(outcome, EnvelopeOutcome::HaltRequested);
    assert_eq!(
        sink.calls(),
        vec![("e9".to_string(), "flatten now".to_string())]
    );
}

#[tokio::test]
async fn slash_wrong_team_is_dropped_before_acting() {
    // The slash handler has its own (flat-key) team check; exercise it.
    let cfg = SocketModeConfig {
        allowed_user_ids: ["U_OK".to_string()].into_iter().collect(),
        allowed_team_id: Some("T_HOME".to_string()),
    };
    let env = SlackEnvelope {
        envelope_id: "e9b".to_string(),
        kind: EnvelopeType::SlashCommands,
        payload: Some(json!({
            "user_id": "U_OK", "team_id": "T_ATTACKER",
            "command": "/fortuna-kill", "text": "x"
        })),
        accepts_response_payload: true,
    };
    let (outcome, sink, _eph) = run(&env, &cfg).await;
    assert_eq!(outcome, EnvelopeOutcome::WrongTeam);
    assert!(sink.calls().is_empty());
}

#[tokio::test]
async fn unknown_envelope_type_and_hello_are_no_ops() {
    for kind in [
        EnvelopeType::Hello,
        EnvelopeType::Disconnect,
        EnvelopeType::Unknown("x".into()),
    ] {
        let env = SlackEnvelope {
            envelope_id: "e10".to_string(),
            kind,
            payload: None,
            accepts_response_payload: false,
        };
        let (outcome, sink, eph) = run(&env, &config()).await;
        assert_eq!(outcome, EnvelopeOutcome::IgnoredNonAction);
        assert!(sink.calls().is_empty() && eph.sent().is_empty());
    }
}

#[tokio::test]
async fn malformed_interactive_payload_is_a_no_op() {
    let env = SlackEnvelope {
        envelope_id: "e11".to_string(),
        kind: EnvelopeType::Interactive,
        payload: None,
        accepts_response_payload: false,
    };
    let (outcome, sink, _eph) = run(&env, &config()).await;
    assert_eq!(outcome, EnvelopeOutcome::MalformedPayload);
    assert!(sink.calls().is_empty());
}

#[tokio::test]
async fn sink_error_surfaces_as_sink_error_outcome() {
    let env = interactive("e12", block_actions("U_OK", ACTION_REQUEST_HALT, "x"));
    let sink = MockSink::failing();
    let eph = MockEphemeral::default();
    let outcome = dispatch_envelope(&env, &config(), &sink, &eph).await;
    assert_eq!(outcome, EnvelopeOutcome::SinkError);
    assert_eq!(sink.calls().len(), 1, "the request was attempted");
}

#[tokio::test]
async fn envelope_type_deserializes_from_the_wire_strings() {
    let cases = [
        ("interactive", EnvelopeType::Interactive),
        ("slash_commands", EnvelopeType::SlashCommands),
        ("hello", EnvelopeType::Hello),
        ("disconnect", EnvelopeType::Disconnect),
        (
            "events_api",
            EnvelopeType::Unknown("events_api".to_string()),
        ),
    ];
    for (wire, expected) in cases {
        let env: SlackEnvelope =
            serde_json::from_value(json!({ "envelope_id": "e", "type": wire })).unwrap();
        assert_eq!(env.kind, expected, "type {wire}");
    }
}
