//! Slack client + channel router (spec Section 8; contract:
//! `docs/research/ops/slack-api-2026-06-09/research.md` — built EXACTLY to
//! that doc, no invented API behavior).
//!
//! Send-only: this module posts via `chat.postMessage`. Interactivity
//! (Socket Mode envelopes, button callbacks, slash commands) is a later
//! phase; [`approval_message`] only BUILDS the Block Kit JSON those phases
//! will post.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::{slack_channel_env_var, SlackConfig};
use crate::OpsError;

/// Research doc: "Endpoint: POST https://slack.com/api/chat.postMessage".
pub const SLACK_POST_MESSAGE_URL: &str = "https://slack.com/api/chat.postMessage";

/// Message categories, routed per spec Section 8 (see [`SlackRouter::route`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MessageKind {
    /// Fills, position opens/closes, per-trade one-liners.
    Trading,
    /// Halts, drawdown approaches, divergence, outages, disputes.
    Alert,
    /// Interactive items requiring a human (approve/reject).
    Review,
    /// Daily digest, weekly calibration report, monthly review.
    Digest,
    /// Cost tracking, stale-signal warnings, heartbeat anomalies, infra.
    Ops,
}

impl MessageKind {
    /// Every routable kind; the router refuses to construct unless all of
    /// these can be routed to a configured channel with a known id.
    pub const ALL: [MessageKind; 5] = [
        MessageKind::Trading,
        MessageKind::Alert,
        MessageKind::Review,
        MessageKind::Digest,
        MessageKind::Ops,
    ];
}

/// Posting transport. Implementations surface HTTP-level outcomes only; the
/// Slack `{ok, error}` envelope is interpreted by [`SlackRouter`], so mocks
/// script envelopes without any HTTP machinery.
#[async_trait]
pub trait SlackTransport: Send + Sync {
    /// POST the JSON `body` to `chat.postMessage` authorized as `token`,
    /// returning the parsed response JSON.
    async fn post_message(&self, token: &str, body: Value) -> Result<Value, OpsError>;
}

/// Real transport over reqwest (rustls).
///
/// HTTP 429 is surfaced as [`OpsError::RateLimited`] with `Retry-After`
/// parsed into `retry_after_secs`. It deliberately does NOT sleep or retry:
/// the core event loop is deterministic with injected time, so a hidden wait
/// inside the transport would be an unauditable stall — the caller decides
/// when (and whether) to retry, honoring `Retry-After` exactly (research:
/// "queue and drain rather than drop" is runner policy, not transport
/// behavior).
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, OpsError> {
        Ok(ReqwestTransport {
            client: crate::http_client()?,
        })
    }
}

#[async_trait]
impl SlackTransport for ReqwestTransport {
    async fn post_message(&self, token: &str, body: Value) -> Result<Value, OpsError> {
        // Serialize ourselves so the Content-type matches the research doc
        // exactly ("application/json; charset=utf-8") regardless of reqwest
        // helper behavior. Token goes ONLY in the Authorization header (never
        // body or query string, per the research doc).
        let payload = serde_json::to_vec(&body).map_err(|e| OpsError::Transport {
            reason: e.to_string(),
        })?;
        let response = self
            .client
            .post(SLACK_POST_MESSAGE_URL)
            .bearer_auth(token)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            )
            .body(payload)
            .send()
            .await
            .map_err(crate::transport_error)?;

        let status = response.status();
        if status.as_u16() == 429 {
            let retry_after_secs = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok());
            return Err(OpsError::RateLimited { retry_after_secs });
        }
        if !status.is_success() {
            return Err(OpsError::Http {
                status: status.as_u16(),
            });
        }
        response
            .json::<Value>()
            .await
            .map_err(crate::transport_error)
    }
}

/// Proof of a delivered Slack message.
///
/// AUDIT CONTRACT (spec Section 8: "every Slack message is also an audit
/// row"): the caller of [`SlackRouter::send`] MUST persist an audit row from
/// this value. Auditing lives upstream (the ledger layer owns I5 writes;
/// this crate has no database dependency). `response_ts` plus `channel_id`
/// is the message's identity for later `chat.update` / dedupe (research:
/// "persist `(channel, ts)`").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMessage {
    pub channel_id: String,
    pub text: String,
    pub response_ts: Option<String>,
}

/// Channel-routed Slack sender (spec Section 8 routing table).
///
/// Fail-closed construction: every [`MessageKind`] must route to a channel
/// that is BOTH listed in `[slack].channels` AND present in the channel-id
/// map from [`crate::Secrets`]. A router that could fail to deliver an Alert
/// at runtime must not exist.
pub struct SlackRouter {
    bot_token: String,
    channel_ids: BTreeMap<String, String>,
    transport: Box<dyn SlackTransport>,
}

impl SlackRouter {
    /// `channel_ids` is channel-name -> Slack channel id (`C…`), normally
    /// `Secrets::slack_channel_ids().clone()`. `bot_token` is the `xoxb-`
    /// token from `Secrets::require(ENV_SLACK_BOT_TOKEN)`.
    pub fn new(
        config: &SlackConfig,
        channel_ids: BTreeMap<String, String>,
        bot_token: String,
        transport: Box<dyn SlackTransport>,
    ) -> Result<Self, OpsError> {
        for kind in MessageKind::ALL {
            let name = Self::route(kind);
            if !config.channels.iter().any(|c| c == name) {
                return Err(OpsError::Config {
                    reason: format!(
                        "slack.channels must include {name:?} (route target for {kind:?} messages)"
                    ),
                });
            }
        }
        for name in &config.channels {
            if !channel_ids.contains_key(name) {
                // The missing thing is literally the env var holding the id.
                return Err(OpsError::MissingSecret {
                    name: slack_channel_env_var(name),
                });
            }
        }
        Ok(SlackRouter {
            bot_token,
            channel_ids,
            transport,
        })
    }

    /// Spec Section 8 routing: Trading -> #fortuna-trading, Alert ->
    /// #fortuna-alerts, Review -> #fortuna-review, Digest -> #fortuna-digest,
    /// Ops -> #fortuna-ops (config uses the short names).
    pub fn route(kind: MessageKind) -> &'static str {
        match kind {
            MessageKind::Trading => "trading",
            MessageKind::Alert => "alerts",
            MessageKind::Review => "review",
            MessageKind::Digest => "digest",
            MessageKind::Ops => "ops",
        }
    }

    /// Post `text` to the channel routed for `kind`.
    ///
    /// Returns [`SentMessage`]; the caller MUST write an audit row from it
    /// (see [`SentMessage`] — auditing lives upstream). Slack `ok: false`
    /// envelopes (HTTP 200) become [`OpsError::Slack`]; a response without a
    /// boolean `ok: true` fails closed the same way. Callers should keep
    /// `text` <= 4,000 chars (research recommendation; Slack truncates at
    /// 40,000).
    pub async fn send(&self, kind: MessageKind, text: &str) -> Result<SentMessage, OpsError> {
        let name = Self::route(kind);
        // Unreachable after construction validation, but stay fail-closed.
        let channel_id = self
            .channel_ids
            .get(name)
            .ok_or_else(|| OpsError::MissingSecret {
                name: slack_channel_env_var(name),
            })?;

        let body = json!({ "channel": channel_id, "text": text });
        let response = self.transport.post_message(&self.bot_token, body).await?;

        if response.get("ok").and_then(Value::as_bool) != Some(true) {
            let code = response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown_error")
                .to_string();
            return Err(OpsError::Slack { code });
        }

        let response_ts = response
            .get("ts")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(SentMessage {
            channel_id: channel_id.clone(),
            text: text.to_string(),
            response_ts,
        })
    }
}

/// Build the Block Kit `blocks` array for an approval prompt — the exact
/// section+actions shape from the research doc ("Block Kit minimal
/// interactive message"). Needed by Phases 2/3; no interactivity listener
/// exists yet. Pass the result as `blocks` alongside a fallback `text`.
///
/// Both buttons carry the same `value` (the correlation id, e.g.
/// `halt:<ULID>`): approve-vs-reject is distinguished by `action_id` in the
/// `block_actions` payload, and downstream dedupe keys on the `value` ULID +
/// `action_ts` (research "Failure semantics"). `value` doubles as the
/// `block_id`, which is unique per approval as the research doc requires.
///
/// Limits the caller must respect (research limits table): section text
/// <= 3,000 chars; `action_id` <= 255; `value` <= 2,000 AND <= 255 because it
/// serves as `block_id`. Button labels are fixed `Approve` / `Reject`
/// (`plain_text`, well under the 75-char cap) with the documented
/// primary/danger styles.
pub fn approval_message(
    text: &str,
    approve_action_id: &str,
    reject_action_id: &str,
    value: &str,
) -> Value {
    json!([
        {
            "type": "section",
            "text": { "type": "mrkdwn", "text": text }
        },
        {
            "type": "actions",
            "block_id": value,
            "elements": [
                {
                    "type": "button",
                    "text": { "type": "plain_text", "text": "Approve" },
                    "style": "primary",
                    "action_id": approve_action_id,
                    "value": value
                },
                {
                    "type": "button",
                    "text": { "type": "plain_text", "text": "Reject" },
                    "style": "danger",
                    "action_id": reject_action_id,
                    "value": value
                }
            ]
        }
    ])
}
