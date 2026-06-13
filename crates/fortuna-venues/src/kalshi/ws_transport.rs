//! Concrete tokio-tungstenite WS transport for Kalshi (T4.2 item 2(i) — the live
//! IO edge that drives [`super::dial::run_dial`]).
//!
//! Landing now: [`classify_ws_error`], the tungstenite-error -> [`DisconnectCause`]
//! map. It is the TESTABLE heart of the concrete transport — it routes the
//! RECORDED venue evidence (`fixtures/kalshi/README.md`, 2026-06-13:
//! "Connection reset without closing handshake", then an HTTP 502 on the
//! reconnect) into the exact causes the dial state machine was unit-tested
//! against, so the live socket redials identically to the mock.
//!
//! DEFERRED (operator-exercised — "no live socket in tests"): the
//! `KalshiWsTransport: WsTransport` impl that does the signed-handshake
//! `connect_async` (reusing [`super::auth::KalshiSigner`] over GET
//! `/trade-api/ws/v2`, timestamp from an injected clock) and the
//! `WsConn` adapter over the `WebSocketStream` (whose send/recv error arms call
//! `classify_ws_error`). Its first LIVE exercise is the operator's recording
//! session; see GAPS.

use super::dial::DisconnectCause;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tokio_tungstenite::tungstenite::Error as WsError;

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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::http::Response;

    #[test]
    fn a_mid_stream_reset_classifies_to_the_recorded_reset_cause() {
        // The recorded evidence's first failure: a TCP reset with no close frame.
        let err = WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake);
        assert_eq!(classify_ws_error(&err), DisconnectCause::ResetWithoutClose);
    }

    #[test]
    fn an_http_502_on_connect_classifies_to_the_recorded_502_cause() {
        // The recorded evidence's second failure: a 502 instead of the 101 upgrade.
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
}
