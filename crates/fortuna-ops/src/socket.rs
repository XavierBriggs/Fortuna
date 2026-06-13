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
//! Sub-slice A2 (built here, below `dispatch_envelope`) adds the ack-first
//! envelope LOOP over a mockable `SlackSocketTransport`/`SlackSocketConn`:
//! ack-before-process, envelope-id dedup (bounded ring), capped-exponential
//! reconnect that survives both transport loss and Slack's
//! `disconnect`/refresh_requested lifecycle, and a cancel watch — mirroring the
//! Kalshi WS dial seam (fortuna-venues `kalshi::dial`). The loop is transport-
//! generic and never reads wall time directly (a `Sleeper` is its only time edge).
//! A1+A2 add ZERO new fortuna-ops dependency.
//!
//! DEFERRED (ledgered GAPS, slice B — operator-gated): the fortuna-live daemon
//! wiring (HaltRequestSink → gate halt path; EphemeralSender → SlackRouter), the
//! REAL `apps.connections.open` + tokio-tungstenite WSS `SlackSocketTransport`
//! (the only new dep then; it must also configure a WS ping/pong timeout so a
//! half-open socket surfaces as a `recv` error — Slack has no app-level keep-
//! alive, unlike Kalshi), the `[slack.socket_mode]` config + `FORTUNA_SLACK_APP_TOKEN`
//! (xapp-, env-only) secret, and the operator-run LIVE exercise.

use crate::OpsError;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::time::Duration;

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
    /// Default-empty: the `hello`/`disconnect` PROTOCOL frames legitimately carry
    /// NO envelope_id (only event envelopes do). The loop routes protocol frames
    /// by `kind` and guards event handling on a non-empty id, so an empty id is
    /// never acked.
    #[serde(default)]
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

// ===========================================================================
// Sub-slice A2 — the ack-first envelope LOOP.
//
// The transport/connection seam mirrors the Kalshi WS dial
// (fortuna-venues `kalshi::dial`): a stateless `SlackSocketTransport` mints a
// fresh WSS URL (via `apps.connections.open`) and opens a connection on each
// (re)dial; an open `SlackSocketConn` reads/writes frames. The loop is generic
// over both, so its safety teeth are proven through a mock transport with NO
// live socket — exactly as the dial proved redial against MockWsTransport.
// ===========================================================================

/// How a Socket Mode connection ended — the redial decision input. (Mirror of
/// the dial's `DisconnectCause`.) All variants are transient: the loop redials
/// indefinitely; the distinction is for ops visibility and for separating a
/// PLANNED refresh (no backoff escalation) from a genuine failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketDisconnect {
    /// Slack asked us to refresh (`disconnect` envelope, reason
    /// `refresh_requested`) — reconnect promptly; NOT a failure.
    RefreshRequested,
    /// `apps.connections.open` or the WS upgrade returned an HTTP error.
    ConnectHttpError { status: u16 },
    /// The socket reset without a WebSocket close handshake.
    ResetWithoutClose,
    /// Any other transport-level loss.
    Transport,
}

/// Mints a fresh WSS URL and opens a Socket Mode connection. Stateless/shared
/// (`&self`): the loop calls it once per (re)dial. The live impl
/// (`apps.connections.open` + tokio-tungstenite) is slice B; tests use a mock.
#[async_trait]
pub trait SlackSocketTransport {
    async fn connect(&self) -> Result<Box<dyn SlackSocketConn>, SocketDisconnect>;
}

/// Read/write frames on an OPEN Socket Mode connection. `Ok(Some(text))` is a
/// text frame; `Ok(None)` is a clean server close (→ reconnect); `Err` is a
/// transport loss. The only thing the loop ever sends is an ack frame — Slack
/// Socket Mode has NO client subscribe step (it pushes all events for the app).
#[async_trait]
pub trait SlackSocketConn: Send {
    async fn send(&mut self, frame: &Value) -> Result<(), SocketDisconnect>;
    async fn recv(&mut self) -> Result<Option<String>, SocketDisconnect>;
}

/// The redial backoff sleep, INJECTED so the loop never embeds wall time (house
/// rule; mirror of the dial's `Sleeper`). The loop reads no clock — this sleep is
/// its only time dependency.
#[async_trait]
pub trait Sleeper: Send + Sync {
    async fn sleep(&self, dur: Duration);
}

/// Production sleeper: real tokio wall-clock sleep at the IO edge.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, dur: Duration) {
        tokio::time::sleep(dur).await;
    }
}

/// The bounded envelope-id dedup ring's default capacity. Socket Mode delivers
/// at-least-once with an unpublished retry schedule (research "Uncertainties"),
/// so handling must be idempotent; a bounded ring keeps memory O(cap).
pub const DEDUP_RING_CAP: usize = 1024;

const DEFAULT_BACKOFF_BASE: Duration = Duration::from_millis(500);
const DEFAULT_BACKOFF_CAP: Duration = Duration::from_secs(30);

/// Capped-exponential redial state (mirror of the dial's `WsDial`), with one
/// addition: a PLANNED refresh resets the counter and reconnects at base, so a
/// refresh storm never escalates `apps.connections.open` calls.
#[derive(Debug, Clone)]
pub struct SocketDial {
    base: Duration,
    cap: Duration,
    consecutive_failures: u32,
}

impl Default for SocketDial {
    fn default() -> Self {
        SocketDial::new(DEFAULT_BACKOFF_BASE, DEFAULT_BACKOFF_CAP)
    }
}

impl SocketDial {
    pub fn new(base: Duration, cap: Duration) -> Self {
        SocketDial {
            base,
            cap,
            consecutive_failures: 0,
        }
    }

    /// A fresh connection opened: reset the failure counter.
    pub fn on_connected(&mut self) {
        self.consecutive_failures = 0;
    }

    /// Slack's planned refresh (`disconnect`/refresh_requested): NOT a failure.
    /// Reset the counter (a later genuine error backs off from base) and
    /// reconnect after the BASE delay — never zero, so a refresh storm cannot
    /// hot-loop `apps.connections.open`.
    pub fn on_clean_reconnect(&mut self) -> Duration {
        self.consecutive_failures = 0;
        self.base
    }

    /// A genuine lost/refused connection: increment and back off (capped
    /// exponential).
    pub fn on_connection_lost(&mut self) -> Duration {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.backoff()
    }

    fn backoff(&self) -> Duration {
        // `consecutive_failures` is already incremented, so subtract 1 for the
        // zero-indexed exponent; `.min(20)` caps the shift before `cap` clamps.
        let n = self.consecutive_failures.saturating_sub(1).min(20);
        let scaled = self.base.saturating_mul(2u32.saturating_pow(n));
        scaled.min(self.cap)
    }
}

/// Bounded envelope-id dedup: a FIFO ring + membership set. Evicts the oldest id
/// when full. An empty id is never recorded and never reported as a duplicate
/// (protocol frames carry no envelope_id and never reach here).
struct EnvelopeDedup {
    cap: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
}

impl EnvelopeDedup {
    fn new(cap: usize) -> Self {
        EnvelopeDedup {
            cap: cap.max(1),
            order: VecDeque::new(),
            seen: HashSet::new(),
        }
    }

    /// True iff `id` was already durably recorded. An empty id is never a dup.
    fn contains(&self, id: &str) -> bool {
        !id.is_empty() && self.seen.contains(id)
    }

    /// Durably record `id` (evicting the oldest when at capacity). No-op for an
    /// empty or already-present id. Recorded SEPARATELY from [`Self::contains`] so
    /// the loop can decline to record a FAILED dispatch — a halt that didn't apply
    /// must stay retryable, not be swallowed as a future duplicate.
    fn record(&mut self, id: &str) {
        if id.is_empty() || self.seen.contains(id) {
            return;
        }
        if self.order.len() >= self.cap {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        self.order.push_back(id.to_string());
        self.seen.insert(id.to_string());
    }
}

/// What the loop did with one inbound frame. The callback receives one per frame
/// (tests assert the sequence; the daemon — slice B — persists these as audit
/// rows). `Dispatched` carries the A1 [`EnvelopeOutcome`] decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopEvent {
    /// `hello` received — the connection is live. No ack, no dispatch.
    Hello,
    /// `disconnect` received (refresh_requested) — the loop will reconnect.
    DisconnectRequested,
    /// An envelope_id-bearing frame was ACKED then DISPATCHED (see the outcome).
    Dispatched(EnvelopeOutcome),
    /// An envelope_id already seen — ACKED again (stop retries) but NOT
    /// re-dispatched (idempotency).
    Duplicate,
    /// A frame that could not be parsed as an envelope — skipped (untrusted
    /// data, no panic). Not acked: there is no envelope_id to ack.
    Unparseable,
}

/// The ack frame: `{"envelope_id": "<id>"}` (research "Ack contract"). A bare
/// envelope_id ack is always valid; an optional response payload (slash replies)
/// is a slice-B concern.
fn ack_frame(envelope_id: &str) -> Value {
    serde_json::json!({ "envelope_id": envelope_id })
}

/// THE Socket Mode listener loop: connect, pump one session acking-and-
/// dispatching each envelope, and on ANY end redial after [`SocketDial`]'s
/// backoff — indefinitely, until `cancel` flips true. A planned refresh
/// reconnects at base without escalating; a genuine failure backs off. Both the
/// in-flight pump AND the backoff sleep are cancellable (a stop never waits out a
/// healthy stream or a backoff). `on_event` receives one [`LoopEvent`] per frame.
#[allow(clippy::too_many_arguments)]
pub async fn run_socket_loop<T, S, F>(
    transport: &T,
    config: &SocketModeConfig,
    sink: &dyn HaltRequestSink,
    ephemeral: &dyn EphemeralSender,
    mut dial: SocketDial,
    sleeper: &S,
    mut cancel: tokio::sync::watch::Receiver<bool>,
    mut on_event: F,
) where
    T: SlackSocketTransport + ?Sized,
    S: Sleeper + ?Sized,
    F: FnMut(LoopEvent),
{
    let mut dedup = EnvelopeDedup::new(DEDUP_RING_CAP);
    loop {
        if *cancel.borrow() {
            return;
        }
        let backoff = match transport.connect().await {
            Ok(mut conn) => {
                dial.on_connected();
                let cause = tokio::select! {
                    _ = cancel.changed() => return,
                    cause = pump_socket(
                        conn.as_mut(), config, sink, ephemeral, &mut dedup, &mut on_event,
                    ) => cause,
                };
                match cause {
                    SocketDisconnect::RefreshRequested => dial.on_clean_reconnect(),
                    _ => dial.on_connection_lost(),
                }
            }
            Err(_cause) => dial.on_connection_lost(),
        };
        tokio::select! {
            _ = cancel.changed() => return,
            _ = sleeper.sleep(backoff) => {}
        }
    }
}

/// Pump one open connection until it ends, returning the cause. Per frame:
/// ack-FIRST (before any processing — the 3s deadline), then dedup, then
/// dispatch. Protocol frames (`hello`/`disconnect`) carry no envelope_id and are
/// not acked.
async fn pump_socket<C, F>(
    conn: &mut C,
    config: &SocketModeConfig,
    sink: &dyn HaltRequestSink,
    ephemeral: &dyn EphemeralSender,
    dedup: &mut EnvelopeDedup,
    on_event: &mut F,
) -> SocketDisconnect
where
    C: SlackSocketConn + ?Sized,
    F: FnMut(LoopEvent),
{
    loop {
        let text = match conn.recv().await {
            Ok(Some(t)) => t,
            // A clean server close → reconnect (treat as a transport end).
            Ok(None) => return SocketDisconnect::Transport,
            Err(cause) => return cause,
        };
        // Untrusted (spec 5.11): a frame that is not valid envelope JSON is a
        // no-op — no panic, and unackable (no envelope_id), so just continue.
        let env: SlackEnvelope = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(_) => {
                on_event(LoopEvent::Unparseable);
                continue;
            }
        };
        match env.kind {
            // Protocol frames: no envelope_id, never acked.
            EnvelopeType::Hello => on_event(LoopEvent::Hello),
            EnvelopeType::Disconnect => {
                on_event(LoopEvent::DisconnectRequested);
                return SocketDisconnect::RefreshRequested;
            }
            // Event envelopes (interactive / slash / events_api-as-Unknown) carry
            // an envelope_id and MUST be acked so Slack stops retrying.
            EnvelopeType::Interactive | EnvelopeType::SlashCommands | EnvelopeType::Unknown(_) => {
                if env.envelope_id.is_empty() {
                    // Defensive: an event frame with no id is unackable (the
                    // contract guarantees event envelopes carry one).
                    on_event(LoopEvent::Unparseable);
                    continue;
                }
                // ACK FIRST (research "ack first, process after"; the 3s
                // deadline). A failed ack means the socket is gone — we did NOT
                // record dedup or dispatch, so Slack redelivers on the new
                // connection (at-least-once preserved).
                if let Err(cause) = conn.send(&ack_frame(&env.envelope_id)).await {
                    return cause;
                }
                // Idempotency: a DURABLY-handled envelope is acked (above) but
                // acted on at most once.
                if dedup.contains(&env.envelope_id) {
                    on_event(LoopEvent::Duplicate);
                    continue;
                }
                let outcome = dispatch_envelope(&env, config, sink, ephemeral).await;
                // Record only a DURABLE decision. A `SinkError` means the halt did
                // NOT apply: leave the id UNrecorded so a Slack redelivery
                // RE-ATTEMPTS the halt (the safe direction) instead of being
                // swallowed as a duplicate. Every other outcome is terminal and is
                // recorded so a redelivery is suppressed.
                if outcome != EnvelopeOutcome::SinkError {
                    dedup.record(&env.envelope_id);
                }
                on_event(LoopEvent::Dispatched(outcome));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Pure-logic units (no transport): the backoff schedule and the dedup ring.
    //! The end-to-end loop teeth (ack-first, reconnect, cancel) live in
    //! tests/socket_loop.rs over the mock transport.
    use super::*;

    #[test]
    fn backoff_is_capped_exponential_from_base() {
        let mut d = SocketDial::new(Duration::from_millis(250), Duration::from_secs(4));
        let got: Vec<Duration> = (0..7).map(|_| d.on_connection_lost()).collect();
        assert_eq!(
            got,
            vec![
                Duration::from_millis(250),  // 1
                Duration::from_millis(500),  // 2
                Duration::from_millis(1000), // 3
                Duration::from_millis(2000), // 4
                Duration::from_millis(4000), // 5 (cap)
                Duration::from_millis(4000), // 6 (held at cap)
                Duration::from_millis(4000), // 7
            ]
        );
    }

    #[test]
    fn a_success_resets_the_backoff_to_base() {
        let mut d = SocketDial::new(Duration::from_millis(250), Duration::from_secs(4));
        d.on_connection_lost();
        d.on_connection_lost();
        assert_eq!(d.on_connection_lost(), Duration::from_millis(1000));
        d.on_connected();
        assert_eq!(
            d.on_connection_lost(),
            Duration::from_millis(250),
            "after a success the next failure is base again"
        );
    }

    #[test]
    fn a_clean_reconnect_uses_base_and_does_not_escalate() {
        let mut d = SocketDial::new(Duration::from_millis(250), Duration::from_secs(4));
        d.on_connection_lost();
        d.on_connection_lost(); // failures now 2
                                // A planned refresh resets to base regardless of prior failures...
        assert_eq!(d.on_clean_reconnect(), Duration::from_millis(250));
        assert_eq!(d.on_clean_reconnect(), Duration::from_millis(250));
        // ...and leaves the counter reset, so a later genuine failure is base.
        assert_eq!(d.on_connection_lost(), Duration::from_millis(250));
    }

    #[test]
    fn dedup_detects_a_repeat_and_ignores_empty() {
        let mut d = EnvelopeDedup::new(8);
        assert!(!d.contains("e1"), "first sighting is not a dup");
        d.record("e1");
        assert!(d.contains("e1"), "recorded id is a dup");
        // An empty id is never a dup and is never recorded.
        assert!(!d.contains(""));
        d.record("");
        assert!(!d.contains(""));
    }

    #[test]
    fn dedup_ring_evicts_the_oldest_when_full() {
        let mut d = EnvelopeDedup::new(2);
        d.record("e1");
        d.record("e2");
        d.record("e3"); // evicts e1 (oldest)
        assert!(!d.contains("e1"), "evicted id is seen as fresh again");
        assert!(
            d.contains("e2") && d.contains("e3"),
            "ring holds the 2 newest"
        );
        // Re-recording e1 evicts e2 (now the oldest); ring is {e3, e1}.
        d.record("e1");
        assert!(!d.contains("e2"), "e2 evicted by e1's re-insertion");
        assert!(d.contains("e3") && d.contains("e1"));
    }
}
