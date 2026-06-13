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

use super::ws::{subscribe_orderbook_cmd, KalshiWsEvent, KalshiWsParser};
use async_trait::async_trait;
use serde_json::Value;
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

/// One established websocket connection, abstracted so the session pump is
/// integration-tested with a SCRIPTED MOCK — no live socket. The signing TLS
/// dial that PRODUCES a `WsConn` is the redial-loop slice (next); this trait is
/// the seam between them.
#[async_trait]
pub trait WsConn: Send {
    /// Send a command frame (the subscribe). `Err(cause)` means the connection
    /// was lost mid-send.
    async fn send(&mut self, cmd: &Value) -> Result<(), DisconnectCause>;
    /// Receive the next text frame. `Ok(Some)` = a frame; `Ok(None)` = a clean
    /// server close; `Err(cause)` = the connection was lost (reset, etc.).
    async fn recv(&mut self) -> Result<Option<String>, DisconnectCause>;
}

/// Drive ONE connection's lifecycle after a successful connect: subscribe to
/// `tickers`, then pump frames through a fresh [`KalshiWsParser`] into
/// `on_event` until the connection ends. A sequence gap RESYNCS in place — the
/// torn book is re-baselined by resubscribing (and resetting the parser) without
/// dropping the connection. An unparseable frame is treated as a torn stream and
/// ends the session so the redial loop re-baselines. Returns the cause the
/// connection ended with, which the loop (next slice) feeds to [`WsDial`].
/// `next_sub_id` is threaded in so subscribe-command ids stay monotone across
/// reconnects.
pub async fn pump_session<C: WsConn + ?Sized>(
    conn: &mut C,
    tickers: &[&str],
    next_sub_id: &mut u64,
    mut on_event: impl FnMut(KalshiWsEvent),
) -> DisconnectCause {
    *next_sub_id += 1;
    if let Err(cause) = conn
        .send(&subscribe_orderbook_cmd(*next_sub_id, tickers))
        .await
    {
        return cause;
    }
    let mut parser = KalshiWsParser::new();
    loop {
        match conn.recv().await {
            Err(cause) => return cause,
            // A clean server close still requires the loop to redial.
            Ok(None) => return DisconnectCause::Transport,
            Ok(Some(text)) => match parser.parse_frame(&text) {
                Ok(KalshiWsEvent::SeqGap { sid, expected, got }) => {
                    on_event(KalshiWsEvent::SeqGap { sid, expected, got });
                    // Resync: a fresh subscribe re-baselines the torn book; reset
                    // the parser so the new snapshot seeds a clean sequence.
                    *next_sub_id += 1;
                    if let Err(cause) = conn
                        .send(&subscribe_orderbook_cmd(*next_sub_id, tickers))
                        .await
                    {
                        return cause;
                    }
                    parser = KalshiWsParser::new();
                }
                Ok(ev) => on_event(ev),
                // An unparseable frame means the stream's structure is lost: end
                // the session so the loop reconnects and re-baselines.
                Err(_) => return DisconnectCause::Transport,
            },
        }
    }
}

/// Opens connections (TLS upgrade + signed handshake). `Err(cause)` reports a
/// FAILED connect — e.g. `ConnectHttpError { status: 502 }` — which the redial
/// loop treats exactly like a lost connection. Mocked in tests; the live
/// tokio-tungstenite impl is the next slice.
#[async_trait]
pub trait WsTransport {
    async fn connect(&self) -> Result<Box<dyn WsConn>, DisconnectCause>;
}

/// THE WS dial loop: connect, pump one session, and on ANY end (lost connection
/// or refused connect) redial after [`WsDial`]'s capped-exponential backoff —
/// indefinitely, until `cancel` flips true. This is where the recorded venue
/// evidence is survived end-to-end: a mid-stream reset and then a 502 on the
/// reconnect both route through `on_connection_lost` and a backed-off redial. The
/// backoff sleep AND an in-flight pump are both cancellable (a stop never waits
/// out a backoff or a healthy stream). `on_event` receives every parsed frame.
pub async fn run_dial<T, F>(
    transport: &T,
    tickers: &[&str],
    mut dial: WsDial,
    mut cancel: tokio::sync::watch::Receiver<bool>,
    mut on_event: F,
) where
    T: WsTransport + ?Sized,
    F: FnMut(KalshiWsEvent),
{
    let mut sub_id = 0u64;
    loop {
        if *cancel.borrow() {
            return;
        }
        let backoff = match transport.connect().await {
            Ok(mut conn) => {
                // A fresh connection resets the backoff; the pump re-subscribes.
                let _ = dial.on_connected();
                // Pump until the connection ends — OR until cancelled mid-stream
                // (a stop never waits out a healthy socket).
                let cause = tokio::select! {
                    _ = cancel.changed() => return,
                    cause = pump_session(conn.as_mut(), tickers, &mut sub_id, &mut on_event) => cause,
                };
                redial_backoff(&mut dial, cause)
            }
            Err(cause) => redial_backoff(&mut dial, cause),
        };
        // Cancellable backoff: a stop wakes immediately instead of sleeping it out.
        tokio::select! {
            _ = cancel.changed() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
    }
}

/// The redial delay for a lost/refused connection. `on_connection_lost` always
/// yields `Redial`; the other arms are unreachable, but the venues no-panic rule
/// forbids `unreachable!()`, so they degrade to a zero delay (retry at once).
fn redial_backoff(dial: &mut WsDial, cause: DisconnectCause) -> Duration {
    match dial.on_connection_lost(cause) {
        DialAction::Redial { backoff } => backoff,
        DialAction::Subscribe | DialAction::Resync => Duration::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::StreamEvent;
    use std::collections::VecDeque;

    /// A scripted connection: `recv` replays a queued sequence of outcomes;
    /// `send` records the subscribe commands (or fails with a configured cause).
    struct MockWsConn {
        recvs: VecDeque<Result<Option<String>, DisconnectCause>>,
        sent: Vec<Value>,
        send_fails_with: Option<DisconnectCause>,
    }

    impl MockWsConn {
        fn new(recvs: Vec<Result<Option<String>, DisconnectCause>>) -> MockWsConn {
            MockWsConn {
                recvs: recvs.into(),
                sent: Vec::new(),
                send_fails_with: None,
            }
        }
    }

    #[async_trait]
    impl WsConn for MockWsConn {
        async fn send(&mut self, cmd: &Value) -> Result<(), DisconnectCause> {
            if let Some(cause) = &self.send_fails_with {
                return Err(cause.clone());
            }
            self.sent.push(cmd.clone());
            Ok(())
        }
        async fn recv(&mut self) -> Result<Option<String>, DisconnectCause> {
            self.recvs
                .pop_front()
                .unwrap_or(Err(DisconnectCause::Transport))
        }
    }

    const SUBSCRIBED: &str =
        r#"{"id":1,"type":"subscribed","msg":{"channel":"orderbook_delta","sid":2}}"#;
    const SNAPSHOT_SEQ2: &str = r#"{"type":"orderbook_snapshot","sid":2,"seq":2,"msg":{"market_ticker":"FED-23DEC-T3.00","yes_dollars_fp":[["0.0800","300.00"]],"no_dollars_fp":[["0.5400","20.00"]]}}"#;
    const DELTA_SEQ3: &str = r#"{"type":"orderbook_delta","sid":2,"seq":3,"msg":{"market_ticker":"FED-23DEC-T3.00","price_dollars":"0.960","delta_fp":"-54.00","side":"yes"}}"#;
    const DELTA_SEQ4_GAP: &str = r#"{"type":"orderbook_delta","sid":2,"seq":4,"msg":{"market_ticker":"FED-23DEC-T3.00","price_dollars":"0.960","delta_fp":"-54.00","side":"yes"}}"#;

    /// One scripted connect outcome: `Err(cause)` = a failed connect (the 502);
    /// `Ok(frames)` = a connection replaying those recv outcomes.
    type ScriptedConnect = Result<Vec<Result<Option<String>, DisconnectCause>>, DisconnectCause>;

    /// A scripted transport: each `connect()` pops one outcome.
    struct MockWsTransport {
        connects: std::sync::Mutex<VecDeque<ScriptedConnect>>,
        count: std::sync::atomic::AtomicUsize,
    }

    impl MockWsTransport {
        fn new(connects: Vec<ScriptedConnect>) -> MockWsTransport {
            MockWsTransport {
                connects: std::sync::Mutex::new(connects.into()),
                count: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn connect_count(&self) -> usize {
            self.count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl WsTransport for MockWsTransport {
        async fn connect(&self) -> Result<Box<dyn WsConn>, DisconnectCause> {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match self.connects.lock().unwrap().pop_front() {
                Some(Ok(frames)) => Ok(Box::new(MockWsConn::new(frames))),
                Some(Err(cause)) => Err(cause),
                // Exhausted: keep refusing (a real test cancels before here).
                None => Err(DisconnectCause::Transport),
            }
        }
    }

    #[tokio::test]
    async fn run_dial_survives_a_reset_then_a_502_and_recovers() {
        // The connect-level recorded evidence: a healthy connection that resets
        // mid-stream, then a 502 on the reconnect, then a recovered connection.
        // The loop must redial through BOTH failures and resubscribe on recovery.
        let transport = MockWsTransport::new(vec![
            Ok(vec![
                Ok(Some(SNAPSHOT_SEQ2.to_string())),
                Ok(Some(DELTA_SEQ3.to_string())),
                Err(DisconnectCause::ResetWithoutClose),
            ]),
            Err(DisconnectCause::ConnectHttpError { status: 502 }),
            Ok(vec![
                Ok(Some(SNAPSHOT_SEQ2.to_string())),
                Err(DisconnectCause::Transport),
            ]),
        ]);
        let (tx, rx) = tokio::sync::watch::channel(false);
        let mut snapshots = 0usize;
        let mut deltas = 0usize;
        // Zero backoff so the redials are instant.
        let dial = WsDial::with_backoff(Duration::ZERO, Duration::ZERO);
        run_dial(&transport, &["FED-23DEC-T3.00"], dial, rx, |ev| match ev {
            KalshiWsEvent::Stream(StreamEvent::BookSnapshot { .. }) => {
                snapshots += 1;
                // Stop once recovered — the SECOND snapshot is on connection #3.
                if snapshots == 2 {
                    let _ = tx.send(true);
                }
            }
            KalshiWsEvent::Stream(StreamEvent::BookDelta { .. }) => deltas += 1,
            _ => {}
        })
        .await;

        assert_eq!(
            transport.connect_count(),
            3,
            "redialed through the reset AND the 502"
        );
        assert_eq!(snapshots, 2, "the initial AND the recovery snapshot");
        assert_eq!(deltas, 1, "the single pre-reset delta");
    }

    #[tokio::test]
    async fn pump_subscribes_then_emits_parsed_events_until_the_connection_ends() {
        // A healthy connection: subscribe ack, snapshot, in-order delta, then a
        // mid-stream reset. The pump subscribes ONCE, emits each parsed event,
        // and returns the cause the connection ended with.
        let mut conn = MockWsConn::new(vec![
            Ok(Some(SUBSCRIBED.to_string())),
            Ok(Some(SNAPSHOT_SEQ2.to_string())),
            Ok(Some(DELTA_SEQ3.to_string())),
            Err(DisconnectCause::ResetWithoutClose),
        ]);
        let mut events = Vec::new();
        let mut id = 0;
        let cause = pump_session(&mut conn, &["FED-23DEC-T3.00"], &mut id, |ev| {
            events.push(ev)
        })
        .await;

        assert_eq!(cause, DisconnectCause::ResetWithoutClose);
        assert_eq!(
            conn.sent.len(),
            1,
            "subscribed exactly once on a clean stream"
        );
        assert_eq!(id, 1, "the subscribe id advanced once");
        assert!(matches!(
            events[0],
            KalshiWsEvent::Subscribed { sid: 2, .. }
        ));
        assert!(matches!(
            events[1],
            KalshiWsEvent::Stream(StreamEvent::BookSnapshot { .. })
        ));
        assert!(matches!(
            events[2],
            KalshiWsEvent::Stream(StreamEvent::BookDelta { .. })
        ));
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn pump_resubscribes_to_resync_on_a_sequence_gap() {
        // Snapshot at seq 2, then a delta at seq 4 — a gap (expected 3). The
        // pump surfaces the SeqGap AND resubscribes in place (re-baseline) WITHOUT
        // dropping the connection: two subscribe commands by the time it ends.
        let mut conn = MockWsConn::new(vec![
            Ok(Some(SNAPSHOT_SEQ2.to_string())),
            Ok(Some(DELTA_SEQ4_GAP.to_string())),
            Err(DisconnectCause::Transport),
        ]);
        let mut events = Vec::new();
        let mut id = 0;
        let cause = pump_session(&mut conn, &["FED-23DEC-T3.00"], &mut id, |ev| {
            events.push(ev)
        })
        .await;

        assert_eq!(cause, DisconnectCause::Transport);
        assert!(
            events.iter().any(|e| matches!(
                e,
                KalshiWsEvent::SeqGap {
                    expected: 3,
                    got: 4,
                    ..
                }
            )),
            "the gap is surfaced: {events:?}"
        );
        assert_eq!(
            conn.sent.len(),
            2,
            "initial subscribe + one resync subscribe"
        );
        assert_eq!(id, 2, "the resync advanced the subscribe id again");
    }

    #[tokio::test]
    async fn pump_returns_the_cause_on_clean_close_and_on_a_failed_subscribe() {
        // A clean server close ends the session (the loop redials).
        let mut closed = MockWsConn::new(vec![Ok(None)]);
        let mut id = 0;
        assert_eq!(
            pump_session(&mut closed, &["FED-23DEC-T3.00"], &mut id, |_| {}).await,
            DisconnectCause::Transport
        );
        assert_eq!(closed.sent.len(), 1, "subscribed before the close");

        // A subscribe that fails mid-send returns its cause and pumps nothing.
        let mut broken = MockWsConn::new(vec![]);
        broken.send_fails_with = Some(DisconnectCause::KeepAliveTimeout);
        let mut events = Vec::new();
        let mut id2 = 0;
        let cause = pump_session(&mut broken, &["FED-23DEC-T3.00"], &mut id2, |ev| {
            events.push(ev)
        })
        .await;
        assert_eq!(cause, DisconnectCause::KeepAliveTimeout);
        assert!(
            events.is_empty(),
            "no frames pumped after a failed subscribe"
        );
    }

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
