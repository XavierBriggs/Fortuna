//! Slack Socket Mode listener — Sub-slice A1: the DECISION LOGIC. Built to the
//! research contract (docs/research/ops/slack-api-2026-06-09/research.md).
//!
//! This module owns what an inbound Slack interaction is ALLOWED to do, with the
//! safety teeth the contract requires:
//! - **I2 (re-arm is CLI-only):** a re-arm action over Slack is REFUSED — there is
//!   NO code path here that un-halts, and the `HaltRequestSink` trait exposes only
//!   `request_halt` (no `rearm`/`clear`). A compromised Slack token cannot un-halt.
//! - **Halt-ONLY routing:** an authorized kill-request routes to an INJECTED
//!   `HaltRequestSink` (the daemon wires it to the gate halt path in the deferred
//!   wiring slice) — NOT to the I4 standalone kill-switch (out-of-band, no Slack dep).
//! - **Allow-list:** Slack provides no per-user restriction, so the handler checks
//!   `user.id` (and an optional `team.id`) against config before acting; anyone else
//!   is told "not authorized" and nothing happens (fail-closed; empty allow-list =
//!   nobody can halt via Slack).
//! - **Untrusted data (spec 5.11):** every payload field is DATA, never instructions
//!   — `action_id` is matched against an ENUM (not a raw string dispatch), the
//!   reason `value`/`text` is bounded and carried as an opaque string, malformed
//!   payloads are a no-op outcome, never a panic.
//!
//! DEFERRED (ledgered GAPS, next slices): A2 = the ack-first envelope LOOP over a
//! mockable `SlackSocketTransport`/`SlackSocketConn` (envelope-id dedup, capped
//! reconnect, cancel). B = the fortuna-live daemon wiring (HaltRequestSink →
//! gate halt path), the real `apps.connections.open` + tokio-tungstenite WSS
//! transport, the `[slack.socket_mode]` config + `FORTUNA_SLACK_APP_TOKEN`
//! (xapp-) secret, and the operator-run LIVE exercise. Sub-slice A1 adds ZERO
//! new fortuna-ops dependency.

use crate::OpsError;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

/// The block-action / slash action-ids this listener understands. They must
/// match the Slack app manifest's `action_id`s. Anything else is `Unknown` —
/// a no-op (never a string-dispatch on untrusted input).
pub const ACTION_REQUEST_HALT: &str = "fortuna_request_halt";
pub const ACTION_REQUEST_REARM: &str = "fortuna_request_rearm";

/// The reason string (slash text / button value) is UNTRUSTED — bounded before
/// it is carried anywhere.
const MAX_REASON_CHARS: usize = 500;

/// The refusal shown when someone clicks a re-arm control over Slack (I2).
const REARM_REFUSED_TEXT: &str =
    "Re-arm is not available over Slack (I2: a halted system is re-armed only \
     out-of-band via the CLI — `fortuna rearm`).";
const NOT_AUTHORIZED_TEXT: &str =
    "Not authorized: your Slack user id is not on the kill-request allow-list.";

/// Where an AUTHORIZED kill-request goes. Defined here so the listener has zero
/// dependency on fortuna-runner / fortuna-gates; the daemon (deferred wiring)
/// implements it against the gate halt path. Halt-ONLY by construction — there
/// is deliberately no `rearm` method (I2).
#[async_trait]
pub trait HaltRequestSink: Send + Sync {
    /// Signal an operator-requested halt. `envelope_id` is the unique id for
    /// idempotency; `reason` is UNTRUSTED, already bounded.
    async fn request_halt(&self, envelope_id: &str, reason: &str) -> Result<(), OpsError>;
}

/// Sends an ephemeral reply (e.g. "not authorized", "re-arm refused"). The
/// daemon wires this to the Slack send path (response_url / SlackRouter); tests
/// record the strings. Kept separate so the listener doesn't need the full
/// `SlackRouter` channel map in tests.
#[async_trait]
pub trait EphemeralSender: Send + Sync {
    async fn send_ephemeral(&self, text: &str) -> Result<(), OpsError>;
}

/// Who may trigger a halt over Slack. Config-driven, fail-closed (empty set =
/// nobody). `allowed_team_id`, if set, drops envelopes from other workspaces.
#[derive(Debug, Clone, Default)]
pub struct SocketModeConfig {
    pub allowed_user_ids: BTreeSet<String>,
    pub allowed_team_id: Option<String>,
}

/// The outer Socket Mode envelope (research §"every delivery arrives as ...").
#[derive(Debug, Deserialize)]
pub struct SlackEnvelope {
    pub envelope_id: String,
    #[serde(rename = "type")]
    pub kind: EnvelopeType,
    /// UNTRUSTED raw payload — never interpreted as instructions.
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(default)]
    pub accepts_response_payload: bool,
}

/// Envelope type discrimination. Unknown absorbs any unrecognized / future type
/// (no panic, no error — a no-op outcome).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeType {
    Interactive,
    SlashCommands,
    Disconnect,
    Hello,
    Unknown(String),
}

impl<'de> Deserialize<'de> for EnvelopeType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "interactive" => EnvelopeType::Interactive,
            "slash_commands" => EnvelopeType::SlashCommands,
            "disconnect" => EnvelopeType::Disconnect,
            "hello" => EnvelopeType::Hello,
            _ => EnvelopeType::Unknown(s),
        })
    }
}

/// The parsed action-id (matched against this ENUM, never a raw-string dispatch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnownActionId {
    RequestHalt,
    RequestRearm,
    Unknown(String),
}

impl KnownActionId {
    fn parse(s: &str) -> KnownActionId {
        match s {
            ACTION_REQUEST_HALT => KnownActionId::RequestHalt,
            ACTION_REQUEST_REARM => KnownActionId::RequestRearm,
            _ => KnownActionId::Unknown(s.to_string()),
        }
    }
}

/// The decision the listener reached for one envelope. The (deferred) daemon
/// persists this as an audit row; tests assert it directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeOutcome {
    /// An authorized kill-request was forwarded to the sink.
    HaltRequested,
    /// The sink itself errored (the request was authorized; delivery failed).
    SinkError,
    /// A re-arm control was clicked — REFUSED (I2); no halt emitted.
    RearmRefused,
    /// The user.id was not on the allow-list — ephemeral "not authorized".
    Unauthorized,
    /// The envelope came from a non-allow-listed team.id — dropped.
    WrongTeam,
    /// A recognized interaction with an action_id we don't act on — no-op.
    IgnoredUnknownAction,
    /// hello / disconnect / unknown envelope type — no-op.
    IgnoredNonAction,
    /// Interactive/slash envelope with a missing/malformed payload — no-op.
    MalformedPayload,
}

fn truncate_reason(s: &str) -> String {
    s.chars().take(MAX_REASON_CHARS).collect()
}

/// True iff `user_id` is on the allow-list. The team restriction is enforced
/// SEPARATELY by the callers BEFORE this, so a foreign-team envelope is dropped
/// as `WrongTeam` (distinct from `Unauthorized`). Fail-closed: an absent user_id
/// or an empty allow-list → false.
fn user_allowed(user_id: Option<&str>, config: &SocketModeConfig) -> bool {
    user_id
        .map(|id| config.allowed_user_ids.contains(id))
        .unwrap_or(false)
}

/// Route one envelope to the right handler. Ack is the LOOP's responsibility
/// (Sub-slice A2); this is the pure decision + side-effect (sink / ephemeral).
pub async fn dispatch_envelope(
    env: &SlackEnvelope,
    config: &SocketModeConfig,
    sink: &dyn HaltRequestSink,
    ephemeral: &dyn EphemeralSender,
) -> EnvelopeOutcome {
    match env.kind {
        EnvelopeType::Interactive => handle_block_actions(env, config, sink, ephemeral).await,
        EnvelopeType::SlashCommands => handle_slash(env, config, sink, ephemeral).await,
        // hello / disconnect / unknown carry no operator action.
        EnvelopeType::Disconnect | EnvelopeType::Hello | EnvelopeType::Unknown(_) => {
            EnvelopeOutcome::IgnoredNonAction
        }
    }
}

async fn handle_block_actions(
    env: &SlackEnvelope,
    config: &SocketModeConfig,
    sink: &dyn HaltRequestSink,
    ephemeral: &dyn EphemeralSender,
) -> EnvelopeOutcome {
    let Some(payload) = env.payload.as_ref() else {
        return EnvelopeOutcome::MalformedPayload;
    };
    // serde_json Index returns Null (never panics) on a missing key/index.
    let user_id = payload["user"]["id"].as_str();
    let team_id = payload["team"]["id"].as_str();

    if config.allowed_team_id.is_some() && team_id != config.allowed_team_id.as_deref() {
        return EnvelopeOutcome::WrongTeam;
    }
    if !user_allowed(user_id, config) {
        let _ = ephemeral.send_ephemeral(NOT_AUTHORIZED_TEXT).await;
        return EnvelopeOutcome::Unauthorized;
    }

    let action_id = payload["actions"][0]["action_id"].as_str().unwrap_or("");
    match KnownActionId::parse(action_id) {
        KnownActionId::RequestHalt => {
            let reason = truncate_reason(payload["actions"][0]["value"].as_str().unwrap_or(""));
            match sink.request_halt(&env.envelope_id, &reason).await {
                Ok(()) => EnvelopeOutcome::HaltRequested,
                Err(_) => EnvelopeOutcome::SinkError,
            }
        }
        // I2: re-arm is CLI-only. Refuse loudly; emit NO halt, touch no sink.
        KnownActionId::RequestRearm => {
            let _ = ephemeral.send_ephemeral(REARM_REFUSED_TEXT).await;
            EnvelopeOutcome::RearmRefused
        }
        // Untrusted action_id we don't act on: no-op (the daemon audits it).
        KnownActionId::Unknown(_) => EnvelopeOutcome::IgnoredUnknownAction,
    }
}

async fn handle_slash(
    env: &SlackEnvelope,
    config: &SocketModeConfig,
    sink: &dyn HaltRequestSink,
    ephemeral: &dyn EphemeralSender,
) -> EnvelopeOutcome {
    let Some(payload) = env.payload.as_ref() else {
        return EnvelopeOutcome::MalformedPayload;
    };
    // Slash payloads use FLAT keys (research doc shape).
    let user_id = payload["user_id"].as_str();
    let team_id = payload["team_id"].as_str();

    if config.allowed_team_id.is_some() && team_id != config.allowed_team_id.as_deref() {
        return EnvelopeOutcome::WrongTeam;
    }
    if !user_allowed(user_id, config) {
        let _ = ephemeral.send_ephemeral(NOT_AUTHORIZED_TEXT).await;
        return EnvelopeOutcome::Unauthorized;
    }

    // Only /fortuna-kill is registered (no re-arm command exists, I2). The
    // command text is UNTRUSTED: bounded, carried as an opaque reason — never
    // parsed as a sub-command or instruction.
    let reason = truncate_reason(payload["text"].as_str().unwrap_or(""));
    match sink.request_halt(&env.envelope_id, &reason).await {
        Ok(()) => EnvelopeOutcome::HaltRequested,
        Err(_) => EnvelopeOutcome::SinkError,
    }
}
