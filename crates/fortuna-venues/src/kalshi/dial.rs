//! Kalshi websocket DIAL layer (T4.2 item 2(i)): the redial-with-resubscribe
//! and seq-gap-resync DECISION logic for the live socket.
//!
//! Scope discipline (paired with [`super::ws`], the message layer): this module
//! owns the *survival* decisions — when a connection is lost or refused, how
//! long to back off before redialing, and that a reconnect ALWAYS re-subscribes
//! (a reconnect never assumes a surviving subscription; the book is rebuilt from
//! a fresh snapshot). It is a PURE state machine so the behavior the recorded
//! venue evidence demands — survive a mid-stream "Connection reset without
//! closing handshake" and then an HTTP 502 on the reconnect
//! (`fixtures/kalshi/README.md`, 2026-06-13 entry) — is unit-tested with NO live
//! socket. The async transport, the signed handshake, and the ping/pong
//! keep-alive timer that DRIVE this state machine are the next 2(i) slice
//! (ledgered in GAPS); nothing here opens a socket.

use std::time::Duration;

/// Why a websocket connection ended or failed to establish. Every cause leads to
/// a redial; the variant is carried for ops visibility (metrics / logging).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectCause {
    /// "Connection reset without closing handshake" — a mid-stream TCP reset
    /// with no WS close frame (`fixtures/kalshi/README.md`, 2026-06-13 evidence,
    /// stream 1).
    ResetWithoutClose,
    /// The (re)connect attempt got an HTTP response instead of the 101 upgrade —
    /// e.g. 502 Bad Gateway from the demo WS host (same evidence). Carries the
    /// status for visibility.
    ConnectHttpError { status: u16 },
    /// The keep-alive (ping/pong) deadline passed with no pong: the socket is
    /// silently dead and must be torn down and redialed.
    KeepAliveTimeout,
    /// Any other transport-level failure (TLS, connect timeout, protocol error).
    Transport,
}

/// The action the dial driver should take next. PURE decisions, so the dial's
/// survival behavior is unit-tested against the recorded venue evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialAction {
    /// Freshly (re)connected: send the subscribe command for ALL tracked
    /// tickers. A reconnect NEVER assumes a surviving subscription — every book
    /// is re-baselined from a fresh snapshot.
    Subscribe,
    /// Connection lost or refused: wait `backoff`, then redial.
    Redial { backoff: Duration },
    /// A sequence gap tore a book (`super::ws::KalshiWsEvent::SeqGap`):
    /// resubscribe to obtain a fresh snapshot before trusting that market again.
    Resync,
}

/// The dial's redial state: capped-exponential backoff over consecutive
/// connection failures, reset on a clean connect. Deterministic (no jitter) so a
/// single-connection market-data dial stays replay-friendly and unit-testable.
#[derive(Debug, Clone)]
pub struct WsDial {
    consecutive_failures: u32,
    base: Duration,
    cap: Duration,
}

impl WsDial {
    /// Defaults grounded in the recorded evidence: a 502 host blip clears in
    /// seconds, so start at 500ms and cap at 30s. A market-data socket retries
    /// INDEFINITELY — a persistent outage surfaces via the venue error counter
    /// and the health view, not a silent give-up.
    pub fn new() -> WsDial {
        WsDial::with_backoff(Duration::from_millis(500), Duration::from_secs(30))
    }

    /// Construct with explicit backoff bounds (tests pin the schedule).
    pub fn with_backoff(base: Duration, cap: Duration) -> WsDial {
        WsDial {
            consecutive_failures: 0,
            base,
            cap,
        }
    }

    /// A connection was established. Reset the backoff and (re)subscribe — a
    /// reconnect always re-baselines (no stale subscription assumed).
    pub fn on_connected(&mut self) -> DialAction {
        self.consecutive_failures = 0;
        DialAction::Subscribe
    }

    /// The connection was lost, or a connect attempt failed (the 502 case). Count
    /// it and redial after a capped-exponential backoff.
    pub fn on_connection_lost(&mut self, _cause: DisconnectCause) -> DialAction {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        DialAction::Redial {
            backoff: self.backoff(),
        }
    }

    /// The parser reported a sequence gap: the book is torn. Resync
    /// (resubscribe). This does NOT perturb the redial backoff — the socket is
    /// healthy; only the book baseline is stale.
    pub fn on_seq_gap(&mut self) -> DialAction {
        DialAction::Resync
    }

    /// Capped-exponential delay: `base * 2^(failures - 1)`, saturated at `cap`.
    /// `failures` is always >= 1 here (incremented before the call); the shift
    /// is bounded to keep the multiply from overflowing before the cap clamps.
    fn backoff(&self) -> Duration {
        let n = self.consecutive_failures.saturating_sub(1).min(20);
        let scaled = self.base.saturating_mul(2u32.saturating_pow(n));
        scaled.min(self.cap)
    }
}

impl Default for WsDial {
    fn default() -> WsDial {
        WsDial::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded venue evidence (fixtures/kalshi/README.md, 2026-06-13): a
    /// healthy connect, then a mid-stream reset-without-close, then a 502 on the
    /// reconnect, then a clean reconnect. The dial must survive it: subscribe on
    /// connect, back off and redial on the reset, back off MORE and redial on the
    /// 502, then re-subscribe on the recovered connection.
    #[test]
    fn dial_survives_the_recorded_reset_then_502_evidence() {
        let mut dial = WsDial::new();
        assert_eq!(dial.on_connected(), DialAction::Subscribe);
        assert_eq!(
            dial.on_connection_lost(DisconnectCause::ResetWithoutClose),
            DialAction::Redial {
                backoff: Duration::from_millis(500)
            },
            "first loss redials after the base backoff"
        );
        assert_eq!(
            dial.on_connection_lost(DisconnectCause::ConnectHttpError { status: 502 }),
            DialAction::Redial {
                backoff: Duration::from_millis(1000)
            },
            "the 502 on the reconnect backs off LONGER (exponential), not the base again"
        );
        assert_eq!(
            dial.on_connected(),
            DialAction::Subscribe,
            "the recovered connection re-subscribes (re-baseline)"
        );
        assert_eq!(
            dial.on_connection_lost(DisconnectCause::ResetWithoutClose),
            DialAction::Redial {
                backoff: Duration::from_millis(500)
            },
            "a clean connect RESET the backoff schedule"
        );
    }

    /// A seq gap resyncs (resubscribe) and must NOT perturb the connection-level
    /// redial backoff — the socket is healthy, only the book baseline is stale.
    #[test]
    fn a_seq_gap_resyncs_without_touching_the_redial_backoff() {
        let mut dial = WsDial::new();
        dial.on_connected();
        assert_eq!(dial.on_seq_gap(), DialAction::Resync);
        assert_eq!(dial.on_seq_gap(), DialAction::Resync);
        assert_eq!(
            dial.on_connection_lost(DisconnectCause::Transport),
            DialAction::Redial {
                backoff: Duration::from_millis(500)
            },
            "the redial backoff is unchanged by intervening resyncs"
        );
    }

    /// The backoff is capped — a long outage must not schedule an absurd delay.
    #[test]
    fn the_backoff_is_capped() {
        let mut dial = WsDial::with_backoff(Duration::from_millis(500), Duration::from_secs(30));
        let mut last = Duration::ZERO;
        for _ in 0..40 {
            if let DialAction::Redial { backoff } =
                dial.on_connection_lost(DisconnectCause::Transport)
            {
                last = backoff;
            } else {
                panic!("a connection loss always redials");
            }
        }
        assert_eq!(
            last,
            Duration::from_secs(30),
            "backoff saturates at the cap"
        );
    }
}
