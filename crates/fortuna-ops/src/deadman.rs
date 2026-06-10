//! Dead-man pinger pieces (spec Section 8: "FORTUNA pings an external
//! monitor (off-ITHACA) every minute; missed pings alert via the monitor's
//! own channel. The system cannot report its own death; liveness detection
//! must live outside it.")
//!
//! This module owns cadence math and the ping transport; the LOOP belongs to
//! the runner (T0.10): `if pinger.due(clock.now()) { pinger.ping().await?;
//! pinger.record_ping(clock.now()); }`. Ping failures surface as typed
//! errors for the runner to escalate — the dead-man channel is also the
//! escalation path for Slack delivery failures, so errors here must never be
//! swallowed.
//!
//! All time is injected: `due`/`record_ping` take `now` parameters, never
//! read wall time.

use async_trait::async_trait;
use fortuna_core::clock::UtcTimestamp;

use crate::config::DeadmanConfig;
use crate::OpsError;

/// GET-ping transport. 2xx = ok.
#[async_trait]
pub trait PingTransport: Send + Sync {
    async fn ping(&self, url: &str) -> Result<(), OpsError>;
}

/// Real transport: GET the monitor URL; any 2xx is success, anything else is
/// a typed error (no internal retries — the runner owns escalation policy).
pub struct ReqwestPing {
    client: reqwest::Client,
}

impl ReqwestPing {
    pub fn new() -> Result<Self, OpsError> {
        Ok(ReqwestPing {
            client: crate::http_client()?,
        })
    }
}

#[async_trait]
impl PingTransport for ReqwestPing {
    async fn ping(&self, url: &str) -> Result<(), OpsError> {
        // transport_error strips URLs: the ping URL is a secret.
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(crate::transport_error)?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(OpsError::Http {
                status: status.as_u16(),
            })
        }
    }
}

/// Cadence state + transport for the dead-man heartbeat.
///
/// No `Debug` impl on purpose: the held URL is a secret.
pub struct DeadmanPinger {
    url: String,
    interval_secs: u64,
    last_ping: Option<UtcTimestamp>,
    transport: Box<dyn PingTransport>,
}

impl DeadmanPinger {
    /// `url` comes from `Secrets::require(ENV_DEADMAN_URL)`; the interval
    /// from `[deadman]`. A zero interval is rejected (it would degenerate
    /// `due` to "always", hammering the monitor).
    pub fn new(
        url: String,
        config: &DeadmanConfig,
        transport: Box<dyn PingTransport>,
    ) -> Result<Self, OpsError> {
        if config.ping_interval_secs == 0 {
            return Err(OpsError::Config {
                reason: "deadman.ping_interval_secs must be >= 1".to_string(),
            });
        }
        Ok(DeadmanPinger {
            url,
            interval_secs: config.ping_interval_secs,
            last_ping: None,
            transport,
        })
    }

    /// True when a ping should be sent at `now`: on the first call (never
    /// pinged), and thereafter once a FULL interval has elapsed since the
    /// last [`DeadmanPinger::record_ping`] (flooring sub-second remainders,
    /// so `due` flips exactly at the boundary, never early).
    ///
    /// If `now` is EARLIER than the last recorded ping (a clock anomaly that
    /// injected monotone clocks should make impossible), this returns false:
    /// the failure direction is "stop pinging so the external monitor
    /// alerts a human", never "keep feeding the monitor and mask the
    /// defect".
    pub fn due(&self, now: UtcTimestamp) -> bool {
        let Some(last) = self.last_ping else {
            return true;
        };
        let Ok(elapsed_millis) =
            u64::try_from(now.epoch_millis().saturating_sub(last.epoch_millis()))
        else {
            return false; // backwards time
        };
        elapsed_millis / 1_000 >= self.interval_secs
    }

    /// Record a successful ping at `now`; the next `due` is one interval out.
    pub fn record_ping(&mut self, now: UtcTimestamp) {
        self.last_ping = Some(now);
    }

    /// Fire one ping at the configured URL. Errors are surfaced typed and
    /// untouched for the runner to escalate (spec Section 8).
    pub async fn ping(&self) -> Result<(), OpsError> {
        self.transport.ping(&self.url).await
    }

    pub fn interval_secs(&self) -> u64 {
        self.interval_secs
    }

    pub fn last_ping(&self) -> Option<UtcTimestamp> {
        self.last_ping
    }
}
