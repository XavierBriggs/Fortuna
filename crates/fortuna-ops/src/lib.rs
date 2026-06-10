//! fortuna-ops: operations-layer pieces (spec Section 8). T0.9 subset:
//! whole-shape config loader + env-only secrets, Slack client with channel
//! routing, and the dead-man pinger pieces.
//!
//! NOT in this crate slice: the operator CLI and the standalone kill switch
//! (owned elsewhere; `fortuna-killswitch` is its own crate per I4).
//! T1.5 adds: metrics registry + exposition renderer, the read-only
//! dashboard server, the daily digest composer, and the accounting export. Re-arm and
//! kill-reversal are deliberately NOT exposed over Slack (spec Section 8:
//! CLI-only; a compromised Slack token must not be able to un-halt a halted
//! system). This crate only ever SENDS to Slack — it has no inbound
//! interactivity surface yet.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

mod config;
pub mod dashboard;
mod deadman;
pub mod digest;
pub mod export;
pub mod metrics;
mod slack;

pub use config::{
    slack_channel_env_var, CognitionConfig, DeadmanConfig, FortunaConfig, Secrets, SizingConfig,
    SlackConfig, ENV_DATABASE_URL, ENV_DEADMAN_URL, ENV_SLACK_BOT_TOKEN, ENV_SLACK_CHANNEL_PREFIX,
};
pub use deadman::{DeadmanPinger, PingTransport, ReqwestPing};
pub use slack::{
    approval_message, MessageKind, ReqwestTransport, SentMessage, SlackRouter, SlackTransport,
    SLACK_POST_MESSAGE_URL,
};

use thiserror::Error;

/// Operations-layer errors. Variants never carry secret VALUES: secrets are
/// referenced by environment-variable NAME only, and URLs are stripped from
/// transport errors (the dead-man ping URL is itself a secret).
#[derive(Debug, Error)]
pub enum OpsError {
    /// Config failed to parse or validate. Fail-closed: an invalid config
    /// never yields a partially-usable value.
    #[error("invalid ops config: {reason}")]
    Config { reason: String },
    /// A required secret was absent from (or empty in) the environment.
    /// `name` is the environment-variable name, never the value.
    #[error("missing required secret: {name}")]
    MissingSecret { name: String },
    /// The accounting export refused or failed (write-once discipline).
    #[error("accounting export: {reason}")]
    Export { reason: String },
    /// Slack replied (typically HTTP 200) with `{"ok": false, "error": code}`
    /// — per the research doc, "check `ok`, not the status code".
    #[error("slack api error: {code}")]
    Slack { code: String },
    /// HTTP 429. `retry_after_secs` is parsed from the `Retry-After` header
    /// (integer seconds) when present. This crate NEVER sleeps or retries
    /// internally: the core is deterministic with injected time, so hidden
    /// waits are forbidden — the caller owns the backoff policy.
    #[error("rate limited (retry-after seconds: {retry_after_secs:?})")]
    RateLimited { retry_after_secs: Option<u64> },
    /// Unexpected non-2xx, non-429 HTTP status.
    #[error("unexpected http status: {status}")]
    Http { status: u16 },
    /// Network / serialization-level transport failure. reqwest errors are
    /// passed through [`transport_error`] which strips URLs before they can
    /// land in logs.
    #[error("transport failure: {reason}")]
    Transport { reason: String },
    /// Filesystem error (config file loading).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convert a reqwest error into [`OpsError::Transport`], stripping the URL:
/// reqwest's `Display` includes the request URL, and the dead-man ping URL
/// (and any future secret-bearing URL) must never leak into logs or audit
/// payloads.
pub(crate) fn transport_error(err: reqwest::Error) -> OpsError {
    OpsError::Transport {
        reason: err.without_url().to_string(),
    }
}

/// Shared HTTP client construction. The 10 s timeout implements the research
/// doc's "short client timeout + at-most-once send" posture for
/// `chat.postMessage` (no idempotency key exists; a long ambiguous wait
/// widens the double-post window).
pub(crate) fn http_client() -> Result<reqwest::Client, OpsError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(transport_error)
}
