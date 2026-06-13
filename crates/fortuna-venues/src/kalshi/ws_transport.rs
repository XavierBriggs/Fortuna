//! Concrete tokio-tungstenite WS transport for Kalshi (T4.2 item 2(i) — the live
//! IO edge that drives [`super::dial::run_dial`]).
//!
//! [`KalshiWsTransport`] does the signed-handshake `connect_async` and yields a
//! [`KalshiWsConn`] over the live `WebSocketStream`. Every DECISION the transport
//! routes through is unit-tested WITHOUT a socket:
//! - [`classify_ws_error`] maps the RECORDED venue evidence
//!   (`fixtures/kalshi/README.md`, 2026-06-13: "Connection reset without closing
//!   handshake", then an HTTP 502) into the dial's [`DisconnectCause`]s, so the
//!   live socket redials IDENTICALLY to the mock the dial was gated against;
//! - [`dispatch`] routes each incoming websocket message (text / ping / pong /
//!   close / error) to the pump's next step;
//! - [`KalshiWsTransport::signed_request`] builds the GET handshake on
//!   `/trade-api/ws/v2` carrying the three KALSHI-ACCESS-* headers (the WS
//!   handshake signs exactly like a REST GET — research §S11).
//!
//! The only untested seam is the live socket ROUND-TRIP itself (`connect_async`
//! and the `WebSocketStream` send/recv): per "no live socket in tests" it is
//! exercised first in the operator's recording session. The keep-alive logic it
//! drives ([`super::dial::KeepAlive`]) is unit-tested in the dial.

use super::auth::KalshiSigner;
use super::dial::{DisconnectCause, KeepAlive, KeepAliveAction, Sleeper, WsConn, WsTransport};
use async_trait::async_trait;
use fortuna_core::clock::Clock;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::{Bytes, Error as WsError, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

/// Production WS endpoint (research host table).
pub const KALSHI_WS_PROD_URL: &str = "wss://external-api-ws.kalshi.com/trade-api/ws/v2";
/// Demo WS endpoint — the only bootable target until live clearance.
pub const KALSHI_WS_DEMO_URL: &str = "wss://external-api-ws.demo.kalshi.co/trade-api/ws/v2";
/// The path the handshake signs over. Research §S11 (verbatim): the signed
/// message is `timestamp + "GET" + "/trade-api/ws/v2"`.
const WS_SIGN_PATH: &str = "/trade-api/ws/v2";

/// Map a tungstenite error into the dial's [`DisconnectCause`] so the redial loop
/// reacts identically whether the failure is a mid-stream reset, a refused
/// (re)connect, or a plain transport fault.
pub fn classify_ws_error(err: &WsError) -> DisconnectCause {
    match err {
        // The recorded evidence's first failure: a mid-stream TCP reset with no
        // WS close handshake.
        WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake) => {
            DisconnectCause::ResetWithoutClose
        }
        // The recorded evidence's second failure: an HTTP response (e.g. 502 Bad
        // Gateway) instead of the 101 upgrade on (re)connect.
        WsError::Http(resp) => DisconnectCause::ConnectHttpError {
            status: resp.status().as_u16(),
        },
        // Everything else — a clean close, IO, TLS, capacity, or other protocol
        // error — is a transport-level end that simply redials.
        _ => DisconnectCause::Transport,
    }
}

/// The pump's reaction to one item read from the socket. Factored out of the
/// recv loop so the routing is unit-tested without a live stream.
#[derive(Debug, PartialEq, Eq)]
enum RecvStep {
    /// A text frame to hand back to the session pump.
    Frame(String),
    /// A clean server close (or stream end) — the loop redials.
    Closed,
    /// The connection was lost; redial with this cause.
    Lost(DisconnectCause),
    /// A pong: refresh [`KeepAlive`] liveness, then keep reading.
    GotPong,
    /// A ping from the server: echo a pong with this payload, then keep reading.
    RespondPing(Vec<u8>),
    /// A frame type FORTUNA does not consume (binary / raw): keep reading.
    Ignore,
}

/// Route one `stream.next()` item. `None` is a stream end (clean close).
fn dispatch(item: Option<Result<Message, WsError>>) -> RecvStep {
    match item {
        None => RecvStep::Closed,
        Some(Err(e)) => RecvStep::Lost(classify_ws_error(&e)),
        Some(Ok(Message::Text(t))) => RecvStep::Frame(t.as_str().to_string()),
        Some(Ok(Message::Close(_))) => RecvStep::Closed,
        Some(Ok(Message::Pong(_))) => RecvStep::GotPong,
        Some(Ok(Message::Ping(p))) => RecvStep::RespondPing(p.to_vec()),
        Some(Ok(Message::Binary(_))) | Some(Ok(Message::Frame(_))) => RecvStep::Ignore,
    }
}

/// The concrete Kalshi WS transport: a signed-handshake `connect_async` producing
/// a [`KalshiWsConn`] over the live `WebSocketStream`. Construct once; the redial
/// loop calls `connect` for each (re)dial. Holds an injected `Clock` (the
/// handshake timestamp + keep-alive `now`) and `Sleeper` (the keep-alive tick) —
/// no direct wall-clock read.
pub struct KalshiWsTransport {
    signer: KalshiSigner,
    ws_url: String,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    ping_interval: Duration,
    pong_deadline: Duration,
    keepalive_tick: Duration,
}

impl KalshiWsTransport {
    /// Defaults: ping every 10s, declare a silent socket dead after 30s with no
    /// pong, poll the deadline every 5s.
    pub fn new(
        signer: KalshiSigner,
        ws_url: String,
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> KalshiWsTransport {
        KalshiWsTransport {
            signer,
            ws_url,
            clock,
            sleeper,
            ping_interval: Duration::from_secs(10),
            pong_deadline: Duration::from_secs(30),
            keepalive_tick: Duration::from_secs(5),
        }
    }

    /// Build the signed WS upgrade request: a GET on the WS path carrying the
    /// three KALSHI-ACCESS-* auth headers. tungstenite adds the standard upgrade
    /// headers (Sec-WebSocket-Key/Version, Upgrade, Connection).
    fn signed_request(&self, now_ms: i64) -> Result<Request<()>, DisconnectCause> {
        let headers = self
            .signer
            .sign("GET", WS_SIGN_PATH, now_ms)
            .map_err(|_| DisconnectCause::Transport)?;
        let mut builder = Request::builder().method("GET").uri(&self.ws_url);
        for (name, value) in headers.as_header_pairs() {
            builder = builder.header(name, value);
        }
        builder.body(()).map_err(|_| DisconnectCause::Transport)
    }
}

#[async_trait]
impl WsTransport for KalshiWsTransport {
    async fn connect(&self) -> Result<Box<dyn WsConn>, DisconnectCause> {
        let now_ms = self.clock.now().epoch_millis();
        let request = self.signed_request(now_ms)?;
        let (stream, _response) = connect_async(request)
            .await
            .map_err(|e| classify_ws_error(&e))?;
        Ok(Box::new(KalshiWsConn {
            stream,
            clock: self.clock.clone(),
            sleeper: self.sleeper.clone(),
            keepalive: KeepAlive::new(self.ping_interval, self.pong_deadline, now_ms),
            keepalive_tick: self.keepalive_tick,
        }))
    }
}

/// A live Kalshi WS connection adapting `WebSocketStream` to [`WsConn`]: text
/// frames pass through to the pump, server pings are echoed, pongs refresh
/// [`KeepAlive`], and the keep-alive deadline (or any transport error) ends the
/// session with the matching [`DisconnectCause`]. The socket round-trip is the
/// operator-exercised seam; its dispatch + liveness decisions are unit-tested.
struct KalshiWsConn {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    keepalive: KeepAlive,
    keepalive_tick: Duration,
}

#[async_trait]
impl WsConn for KalshiWsConn {
    async fn send(&mut self, cmd: &Value) -> Result<(), DisconnectCause> {
        self.stream
            .send(Message::Text(cmd.to_string().into()))
            .await
            .map_err(|e| classify_ws_error(&e))
    }

    async fn recv(&mut self) -> Result<Option<String>, DisconnectCause> {
        loop {
            tokio::select! {
                item = self.stream.next() => match dispatch(item) {
                    RecvStep::Frame(text) => return Ok(Some(text)),
                    RecvStep::Closed => return Ok(None),
                    RecvStep::Lost(cause) => return Err(cause),
                    RecvStep::GotPong => {
                        self.keepalive.on_pong(self.clock.now().epoch_millis());
                    }
                    RecvStep::RespondPing(payload) => {
                        if self
                            .stream
                            .send(Message::Pong(payload.into()))
                            .await
                            .is_err()
                        {
                            return Err(DisconnectCause::Transport);
                        }
                    }
                    RecvStep::Ignore => {}
                },
                // The keep-alive tick: ping on the interval, or declare the
                // half-open socket dead. Sleep through the INJECTED sleeper — no
                // direct wall-clock read in the loop.
                _ = self.sleeper.sleep(self.keepalive_tick) => {
                    match self.keepalive.poll(self.clock.now().epoch_millis()) {
                        KeepAliveAction::SendPing => {
                            if self.stream.send(Message::Ping(Bytes::new())).await.is_err() {
                                return Err(DisconnectCause::Transport);
                            }
                        }
                        KeepAliveAction::Dead => return Err(DisconnectCause::KeepAliveTimeout),
                        KeepAliveAction::Idle => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fortuna_core::clock::RealClock;
    use rsa::pkcs8::EncodePrivateKey;
    use std::sync::OnceLock;
    use tokio_tungstenite::tungstenite::http::Response;

    #[test]
    fn a_mid_stream_reset_classifies_to_the_recorded_reset_cause() {
        let err = WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake);
        assert_eq!(classify_ws_error(&err), DisconnectCause::ResetWithoutClose);
    }

    #[test]
    fn an_http_502_on_connect_classifies_to_the_recorded_502_cause() {
        let resp: Response<Option<Vec<u8>>> = Response::builder().status(502).body(None).unwrap();
        let err = WsError::Http(Box::new(resp));
        assert_eq!(
            classify_ws_error(&err),
            DisconnectCause::ConnectHttpError { status: 502 }
        );
    }

    #[test]
    fn io_and_close_errors_classify_to_a_plain_transport_redial() {
        let io = WsError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        ));
        assert_eq!(classify_ws_error(&io), DisconnectCause::Transport);
        assert_eq!(
            classify_ws_error(&WsError::ConnectionClosed),
            DisconnectCause::Transport
        );
    }

    #[test]
    fn dispatch_routes_each_incoming_message_kind() {
        // Stream end / close -> redial; text -> a frame for the pump.
        assert_eq!(dispatch(None), RecvStep::Closed);
        assert_eq!(
            dispatch(Some(Ok(Message::Text("hello".into())))),
            RecvStep::Frame("hello".to_string())
        );
        assert_eq!(dispatch(Some(Ok(Message::Close(None)))), RecvStep::Closed);
        // Pong refreshes liveness; a server ping is echoed; an error is classified.
        assert_eq!(
            dispatch(Some(Ok(Message::Pong(Bytes::new())))),
            RecvStep::GotPong
        );
        assert_eq!(
            dispatch(Some(Ok(Message::Ping(Bytes::from_static(b"ka"))))),
            RecvStep::RespondPing(b"ka".to_vec())
        );
        assert_eq!(
            dispatch(Some(Err(WsError::Protocol(
                ProtocolError::ResetWithoutClosingHandshake
            )))),
            RecvStep::Lost(DisconnectCause::ResetWithoutClose)
        );
        // Binary frames are not consumed.
        assert_eq!(
            dispatch(Some(Ok(Message::Binary(Bytes::new())))),
            RecvStep::Ignore
        );
    }

    fn test_transport() -> KalshiWsTransport {
        // One shared 2048-bit key per test binary (keygen is the slow part).
        static KEY: OnceLock<rsa::RsaPrivateKey> = OnceLock::new();
        let key = KEY.get_or_init(|| {
            rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).expect("test RSA keygen")
        });
        let pem = key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .expect("pkcs8 pem");
        let signer = KalshiSigner::new(&pem, "test-key-id".to_string()).expect("signer");
        KalshiWsTransport::new(
            signer,
            KALSHI_WS_DEMO_URL.to_string(),
            Arc::new(RealClock),
            Arc::new(crate::kalshi::dial::TokioSleeper),
        )
    }

    #[test]
    fn signed_request_is_a_get_on_the_ws_path_with_the_three_auth_headers() {
        let t = test_transport();
        let req = t
            .signed_request(1_700_000_000_000)
            .expect("a signed handshake request");
        assert_eq!(req.method(), "GET");
        assert!(
            req.uri().to_string().contains("/trade-api/ws/v2"),
            "uri carries the WS path: {}",
            req.uri()
        );
        let h = req.headers();
        assert_eq!(
            h.get(crate::kalshi::auth::HEADER_KEY)
                .unwrap()
                .to_str()
                .unwrap(),
            "test-key-id"
        );
        assert_eq!(
            h.get(crate::kalshi::auth::HEADER_TIMESTAMP)
                .unwrap()
                .to_str()
                .unwrap(),
            "1700000000000"
        );
        assert!(
            !h.get(crate::kalshi::auth::HEADER_SIGNATURE)
                .unwrap()
                .is_empty(),
            "the RSA-PSS signature header is present"
        );
    }
}
