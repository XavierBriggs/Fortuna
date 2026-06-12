//! Kinetics perps DEMO-environment fixture recorder (operator-authorized
//! session, 2026-06-11: "run the Kinetics perps demo fixture-recording
//! session"; mock funds only).
//!
//! Records the operator fixtures demanded by
//! docs/research/venue/kinetics-perps-2026-06-10/research.md §12 (items 1-10,
//! 12-14, 16; 11/15/17 are PROD/post-fee and 18 is human outreach) into
//! fixtures/kinetics-perps/ as `<area>__<case>.json` (verbatim response body)
//! plus a sibling `.meta.json` (method, path, status, sanitized request body,
//! note) — the same conventions as fixtures/kalshi/.
//!
//! SAFETY RAILS:
//! - Demo hosts are HARDCODED and asserted (`.demo.kalshi.co`). Reads ONLY
//!   the demo credential env vars (KALSHI_API_DEMO_KEY_ID,
//!   KALSHI_DEMO_PRIVATE_KEY_PATH); the production and kill-switch variable
//!   names are never referenced. Demo keys do not work on prod and vice
//!   versa, so a mixed-up key fails closed.
//! - No secret material is ever printed or written into fixtures/meta:
//!   request headers are not recorded at all.
//! - Sizes are 1-2 contracts of the smallest perp (~$6-13 notional, mock
//!   funds). Every order this tool places is tracked and canceled in the
//!   cleanup stage EXCEPT the deliberate §12-item-10 funding position
//!   (1 contract, left open so a later session can capture the funding
//!   tick); its details land in the session manifest.
//! - DEGRADED MODE: if the demo account is not margin-enabled
//!   (GET /margin/enabled -> false / private reads 403), the recorder does
//!   NOT thrash the blocked surface: it captures the auth probes, the public
//!   surfaces, the WS handshake, and exactly ONE blocked-evidence probe per
//!   private item family, expanding a family only if its probe succeeds.
//!
//! This is an IO-edge capture TOOL, not core code: it signs live requests,
//! so it uses wall-clock time directly (the injected-Clock rule governs the
//! deterministic core, not one-shot operator tooling — see CLAUDE.md).
//!
//! Run from the repo root:
//! `cargo run -p fortuna-venues --example record_kinetics_fixtures`

use anyhow::{bail, Context, Result};
use fortuna_venues::kalshi::auth::KalshiSigner;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Demo REST host (research §8a/§9). NEVER a prod host.
const REST_HOST: &str = "https://external-api.demo.kalshi.co";
/// Dedicated demo margin WS host (research §8b). NEVER a prod host.
const WS_HOST: &str = "wss://external-api-margin-ws.demo.kalshi.co";
const API_ROOT: &str = "/trade-api/v2";
/// Margin WS URL path; also the primary signing-path candidate (§12 item 2).
const WS_PATH_MARGIN: &str = "/trade-api/ws/v2/margin";
/// Fallback signing-path candidate (the event-API string), per research §8b.
const WS_PATH_EVENT: &str = "/trade-api/ws/v2";
const FIXTURE_DIR: &str = "fixtures/kinetics-perps";
const PACE: Duration = Duration::from_millis(350);
const ORDER_PACE: Duration = Duration::from_millis(1000);
const PUBLIC_WS_CAPTURE_SECS: u64 = 75;
const WS_MAX_FRAMES: usize = 5000;
/// Drain window for private WS events after each order action.
const PUMP: Duration = Duration::from_millis(1800);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Refuse to touch anything that is not unambiguously the demo environment.
fn assert_demo_host(host: &str) -> Result<()> {
    if !host.contains(".demo.kalshi.co") {
        bail!("SAFETY ABORT: host {host} is not a .demo.kalshi.co host");
    }
    Ok(())
}

/// RFC-4122-shaped v4 id from the process CSPRNG (client_order_id /
/// client_transfer_id values).
fn uuid4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Perp prices tick in 0.0001 dollars (research §3): one "tick unit" here is
/// 1e-4 dollars. Format integer tick units as a FixedPointDollars string.
fn ticks_to_dollars(ticks: i64) -> String {
    format!("{}.{:04}", ticks / 10_000, ticks % 10_000)
}

/// Parse a FixedPointDollars string ("6.2590", "5000.0000") into 1e-4-dollar
/// tick units. Tolerates 0-6 decimals (truncates past 4).
fn dollars_to_ticks(s: &str) -> Option<i64> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let whole: i64 = whole.parse().ok()?;
    let frac4: String = frac.chars().chain(std::iter::repeat('0')).take(4).collect();
    let frac4: i64 = frac4.parse().ok()?;
    Some(whole * 10_000 + frac4)
}

#[derive(Clone, Copy)]
enum AuthMode {
    Signed,
    SignedSkewMs(i64),
    BadSignature,
    Unauthenticated,
}

impl AuthMode {
    fn label(self) -> &'static str {
        match self {
            AuthMode::Signed => "signed",
            AuthMode::SignedSkewMs(_) => "signed-skewed",
            AuthMode::BadSignature => "bad-signature",
            AuthMode::Unauthenticated => "unauthenticated",
        }
    }
}

struct Captured {
    status: u16,
    json: Option<Value>,
}

impl Captured {
    fn str_field(&self, key: &str) -> Option<String> {
        self.json
            .as_ref()
            .and_then(|j| j.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

struct Recorder {
    http: reqwest::Client,
    signer: KalshiSigner,
    out: PathBuf,
    summary: Vec<(String, String)>,
    /// (order_id, already_terminal) for cleanup.
    placed: Vec<(String, bool)>,
}

impl Recorder {
    fn note(&mut self, name: &str, msg: String) {
        println!("  [{name}] {msg}");
        self.summary.push((name.to_string(), msg));
    }

    fn write_fixture(&self, name: &str, body: &str, meta: &Value) -> Result<()> {
        let f = self.out.join(format!("{name}.json"));
        std::fs::write(&f, body).with_context(|| format!("writing {}", f.display()))?;
        let m = self.out.join(format!("{name}.meta.json"));
        let pretty = serde_json::to_string_pretty(meta).context("meta to_string")?;
        std::fs::write(&m, pretty).with_context(|| format!("writing {}", m.display()))?;
        Ok(())
    }

    /// One captured REST call -> fixture + meta + summary row.
    async fn capture(
        &mut self,
        name: &str,
        method: reqwest::Method,
        path_q: &str,
        body: Option<Value>,
        auth: AuthMode,
        note: &str,
    ) -> Result<Captured> {
        tokio::time::sleep(PACE).await;
        assert_demo_host(REST_HOST)?;
        let url = format!("{REST_HOST}{path_q}");
        let mut req = self.http.request(method.clone(), &url);
        let ts = now_ms();
        match auth {
            AuthMode::Signed => {
                let h = self.signer.sign(method.as_str(), path_q, ts)?;
                for (k, v) in h.as_header_pairs() {
                    req = req.header(k, v);
                }
            }
            AuthMode::SignedSkewMs(skew) => {
                let h = self.signer.sign(method.as_str(), path_q, ts + skew)?;
                for (k, v) in h.as_header_pairs() {
                    req = req.header(k, v);
                }
            }
            AuthMode::BadSignature => {
                // Valid-format signature over the WRONG message.
                let h = self.signer.sign(
                    method.as_str(),
                    "/trade-api/v2/margin/not-the-real-path",
                    ts,
                )?;
                for (k, v) in h.as_header_pairs() {
                    req = req.header(k, v);
                }
            }
            AuthMode::Unauthenticated => {}
        }
        if let Some(ref b) = body {
            req = req.json(b);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("{name}: request to {url}"))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.with_context(|| format!("{name}: body"))?;
        let parsed: Option<Value> = serde_json::from_str(&text).ok();
        let meta = json!({
            "recorded_at_epoch_ms": ts,
            "environment": "demo",
            "host": REST_HOST,
            "method": method.as_str(),
            "path": path_q,
            "status": status,
            "auth": auth.label(),
            "request_body": body,
            "note": note,
        });
        self.write_fixture(name, &text, &meta)?;
        self.note(name, format!("HTTP {status}"));
        Ok(Captured {
            status,
            json: parsed,
        })
    }

    /// Place a margin order and track it for cleanup. Returns (capture, id).
    async fn place(
        &mut self,
        name: &str,
        body: Value,
        note: &str,
    ) -> Result<(Captured, Option<String>)> {
        tokio::time::sleep(ORDER_PACE).await;
        let cap = self
            .capture(
                name,
                reqwest::Method::POST,
                &format!("{API_ROOT}/margin/orders"),
                Some(body),
                AuthMode::Signed,
                note,
            )
            .await?;
        let id = cap.str_field("order_id");
        if let Some(ref oid) = id {
            if cap.status == 201 {
                self.placed.push((oid.clone(), false));
            }
        }
        Ok((cap, id))
    }

    async fn cancel(&mut self, name: &str, order_id: &str, note: &str) -> Result<Captured> {
        tokio::time::sleep(ORDER_PACE).await;
        let cap = self
            .capture(
                name,
                reqwest::Method::DELETE,
                &format!("{API_ROOT}/margin/orders/{order_id}"),
                None,
                AuthMode::Signed,
                note,
            )
            .await?;
        if cap.status == 200 {
            self.mark_done(order_id);
        }
        Ok(cap)
    }

    fn mark_done(&mut self, order_id: &str) {
        for (oid, done) in &mut self.placed {
            if oid == order_id {
                *done = true;
            }
        }
    }
}

/// One live margin-WS connection with verbatim-frame capture.
struct WsSession {
    stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    frames: Vec<String>,
    pings: u32,
    first_ping_payload: Option<String>,
    http_status: u16,
    signed_path: &'static str,
}

impl WsSession {
    /// Connect to the margin WS URL signing `signed_path` for the handshake.
    /// On an HTTP rejection returns Ok(Err((status, body))) so the caller can
    /// capture the evidence and try the fallback signing path.
    async fn connect(
        signer: &KalshiSigner,
        signed_path: &'static str,
    ) -> Result<std::result::Result<WsSession, (u16, String)>> {
        assert_demo_host(WS_HOST)?;
        let url = format!("{WS_HOST}{WS_PATH_MARGIN}");
        let mut req = url.into_client_request().context("building ws request")?;
        let ts = now_ms();
        let h = signer.sign("GET", signed_path, ts)?;
        for (k, v) in h.as_header_pairs() {
            req.headers_mut().insert(
                k,
                v.parse()
                    .map_err(|_| anyhow::anyhow!("header value unparseable"))?,
            );
        }
        match tokio_tungstenite::connect_async(req).await {
            Ok((stream, resp)) => Ok(Ok(WsSession {
                stream,
                frames: Vec::new(),
                pings: 0,
                first_ping_payload: None,
                http_status: resp.status().as_u16(),
                signed_path,
            })),
            Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
                let status = resp.status().as_u16();
                let body = resp
                    .body()
                    .as_ref()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default();
                Ok(Err((status, body)))
            }
            Err(e) => Err(e).context("ws connect"),
        }
    }

    async fn send(&mut self, cmd: &Value) -> Result<()> {
        self.stream
            .send(Message::text(cmd.to_string()))
            .await
            .context("ws send")
    }

    /// Drain frames for up to `window`, recording text frames verbatim.
    async fn pump(&mut self, window: Duration) -> Result<()> {
        let deadline = Instant::now() + window;
        while Instant::now() < deadline && self.frames.len() < WS_MAX_FRAMES {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(remaining, self.stream.next()).await {
                Err(_) => break,
                Ok(None) => break,
                Ok(Some(frame)) => match frame.context("ws frame")? {
                    Message::Text(t) => self.frames.push(t.as_str().to_string()),
                    Message::Ping(p) => {
                        self.pings += 1;
                        self.first_ping_payload
                            .get_or_insert_with(|| String::from_utf8_lossy(&p).into_owned());
                    }
                    Message::Close(c) => {
                        self.frames.push(format!(
                            "{{\"__close_frame\": {:?}}}",
                            c.map(|c| c.to_string())
                        ));
                        break;
                    }
                    _ => {}
                },
            }
        }
        Ok(())
    }

    /// True if any captured frame's `type` field equals `t`.
    fn saw_type(&self, t: &str) -> bool {
        self.frames.iter().any(|f| {
            serde_json::from_str::<Value>(f)
                .ok()
                .and_then(|j| j.get("type").and_then(|v| v.as_str()).map(|s| s == t))
                .unwrap_or(false)
        })
    }

    /// Close the socket and write the .jsonl + .meta.json fixture pair.
    async fn finish(mut self, r: &mut Recorder, name: &str, cmds: &[Value], note: &str) {
        let _ = self.stream.close(None).await;
        let body = self.frames.join("\n");
        let meta = json!({
            "recorded_at_epoch_ms": now_ms(),
            "environment": "demo",
            "host": WS_HOST,
            "url_path": WS_PATH_MARGIN,
            "signed_path": self.signed_path,
            "http_status": self.http_status,
            "subscribe_cmds": cmds,
            "frames_captured": self.frames.len(),
            "pings_observed": self.pings,
            "first_ping_payload": self.first_ping_payload,
            "format": "one verbatim text frame per line (.jsonl)",
            "note": note,
        });
        let f = r.out.join(format!("{name}.jsonl"));
        if let Err(e) = std::fs::write(&f, body) {
            r.note(name, format!("FAILED writing frames: {e:#}"));
            return;
        }
        match serde_json::to_string_pretty(&meta) {
            Ok(pretty) => {
                if let Err(e) = std::fs::write(r.out.join(format!("{name}.meta.json")), pretty) {
                    r.note(name, format!("FAILED writing meta: {e:#}"));
                    return;
                }
            }
            Err(e) => {
                r.note(name, format!("FAILED meta json: {e:#}"));
                return;
            }
        }
        r.note(
            name,
            format!(
                "ws 101={} frames={} pings={}",
                self.http_status,
                self.frames.len(),
                self.pings
            ),
        );
    }
}

/// Connect with the documented-presumed margin signing path, falling back to
/// the event-API path per research §12 item 2; HTTP rejections are captured
/// as fixtures.
async fn ws_connect_with_fallback(r: &mut Recorder, label: &str) -> Result<WsSession> {
    match WsSession::connect(&r.signer, WS_PATH_MARGIN).await? {
        Ok(s) => {
            r.note(
                label,
                format!(
                    "handshake OK signing {WS_PATH_MARGIN} (status {})",
                    s.http_status
                ),
            );
            Ok(s)
        }
        Err((status, body)) => {
            let meta = json!({
                "recorded_at_epoch_ms": now_ms(),
                "environment": "demo",
                "host": WS_HOST,
                "url_path": WS_PATH_MARGIN,
                "signed_path": WS_PATH_MARGIN,
                "status": status,
                "note": "item 2: handshake REJECTED signing /trade-api/ws/v2/margin; \
                         body below; falling back to the event-API signing path",
            });
            let name = format!("{label}_margin_path_rejected");
            r.write_fixture(&name, &body, &meta)?;
            r.note(
                &name,
                format!("HTTP {status} — trying fallback signing path"),
            );
            match WsSession::connect(&r.signer, WS_PATH_EVENT).await? {
                Ok(s) => {
                    r.note(
                        label,
                        format!(
                            "handshake OK signing FALLBACK {WS_PATH_EVENT} (status {})",
                            s.http_status
                        ),
                    );
                    Ok(s)
                }
                Err((status2, body2)) => {
                    let meta2 = json!({
                        "recorded_at_epoch_ms": now_ms(),
                        "environment": "demo",
                        "host": WS_HOST,
                        "url_path": WS_PATH_MARGIN,
                        "signed_path": WS_PATH_EVENT,
                        "status": status2,
                        "note": "item 2: handshake rejected on BOTH signing paths",
                    });
                    let name2 = format!("{label}_event_path_rejected");
                    r.write_fixture(&name2, &body2, &meta2)?;
                    bail!(
                        "margin WS handshake rejected on both signing paths ({status}/{status2})"
                    );
                }
            }
        }
    }
}

/// (best_bid_ticks, best_ask_ticks, first/last summary) from a REST
/// orderbook response — computed defensively by min/max because the sort
/// order is the §11.1 conflict this session is meant to settle.
fn book_extremes(book: &Value) -> (Option<i64>, Option<i64>, String) {
    let ob = book.get("orderbook").unwrap_or(book);
    let levels = |key: &str| -> Vec<i64> {
        ob.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|lvl| {
                        lvl.get(0)
                            .and_then(|p| p.as_str())
                            .and_then(dollars_to_ticks)
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let bids = levels("bids");
    let asks = levels("asks");
    let order_note = format!(
        "bids n={} first={:?} last={:?}; asks n={} first={:?} last={:?}",
        bids.len(),
        bids.first(),
        bids.last(),
        asks.len(),
        asks.first(),
        asks.last()
    );
    (
        bids.iter().max().copied(),
        asks.iter().min().copied(),
        order_note,
    )
}

/// Pick the session market: prefer the BTC perp (smallest notional, most
/// liquid), else the first active market.
fn pick_market(list: &Value) -> Option<String> {
    let arr = list.get("markets").and_then(|m| m.as_array())?;
    let active = |m: &Value| m.get("status").and_then(|s| s.as_str()) == Some("active");
    let ticker = |m: &Value| m.get("ticker").and_then(|t| t.as_str()).map(str::to_string);
    arr.iter()
        .find(|m| {
            active(m)
                && ticker(m)
                    .map(|t| t.starts_with("KXBTCPERP"))
                    .unwrap_or(false)
        })
        .or_else(|| arr.iter().find(|m| active(m)))
        .and_then(ticker)
}

fn gtc_order(ticker: &str, side: &str, count: &str, price_ticks: i64, post_only: bool) -> Value {
    json!({
        "ticker": ticker,
        "client_order_id": uuid4(),
        "side": side,
        "count": count,
        "price": ticks_to_dollars(price_ticks),
        "time_in_force": "good_till_canceled",
        "self_trade_prevention_type": "taker_at_cross",
        "post_only": post_only,
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // ---- demo credentials only; refuse loudly if absent ----
    let key_id = std::env::var("KALSHI_API_DEMO_KEY_ID")
        .context("KALSHI_API_DEMO_KEY_ID not set (demo credentials required; see .env)")?;
    let key_path = std::env::var("KALSHI_DEMO_PRIVATE_KEY_PATH")
        .context("KALSHI_DEMO_PRIVATE_KEY_PATH not set (demo credentials required; see .env)")?;
    let pem = std::fs::read_to_string(&key_path)
        .with_context(|| format!("reading demo private key at {key_path}"))?;
    let signer = KalshiSigner::new(&pem, key_id)?;
    drop(pem);
    assert_demo_host(REST_HOST)?;
    assert_demo_host(WS_HOST)?;

    let out = Path::new(FIXTURE_DIR).to_path_buf();
    std::fs::create_dir_all(&out).context("creating fixtures/kinetics-perps")?;
    if !Path::new("crates").is_dir() {
        bail!("run from the repo root (fixtures/kinetics-perps must resolve)");
    }

    println!("Kinetics perps DEMO fixture recorder — hosts: {REST_HOST} / {WS_HOST}");
    println!("Output: {FIXTURE_DIR}/  (verbatim bodies + .meta.json; no headers recorded)\n");

    let mut rec = Recorder {
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .context("building http client")?,
        signer,
        out,
        summary: Vec::new(),
        placed: Vec::new(),
    };
    let r = &mut rec;
    let get = reqwest::Method::GET;
    let post = reqwest::Method::POST;
    let put = reqwest::Method::PUT;
    let delete = reqwest::Method::DELETE;

    // ============ item 1: auth round-trip + 401 bodies + skew ============
    let enabled = r
        .capture(
            "auth__margin_enabled_ok",
            get.clone(),
            &format!("{API_ROOT}/margin/enabled"),
            None,
            AuthMode::Signed,
            "item 1: happy-path signed GET /margin/enabled (event-API signing recipe)",
        )
        .await?;
    if enabled.status != 200 {
        bail!(
            "signed GET /margin/enabled returned HTTP {} — aborting before any further \
             stages (credentials or signing are wrong; nothing else is meaningful)",
            enabled.status
        );
    }
    let enabled_flag = enabled
        .json
        .as_ref()
        .and_then(|j| j.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let bal = r
        .capture(
            "auth__margin_balance",
            get.clone(),
            &format!("{API_ROOT}/margin/balance"),
            None,
            AuthMode::Signed,
            "item 1: signed GET /margin/balance (5-token cost variant); in a \
             non-margin-enabled account this is the 403 enablement-gate evidence",
        )
        .await?;
    if bal.status == 401 {
        bail!(
            "signed GET /margin/balance returned 401 after /margin/enabled accepted the \
             same signature — aborting (fail-closed on inconsistent happy-path auth)"
        );
    }
    let margin_enabled = enabled_flag && bal.status == 200;
    if !margin_enabled {
        r.note(
            "session",
            format!(
                "MARGIN NOT ENABLED for this demo account (enabled={enabled_flag}, \
                 balance HTTP {}) — DEGRADED capture: auth probes, public surfaces, \
                 WS handshake, one blocked-evidence probe per private item family. \
                 Order lifecycle and the item-10 funding position are BLOCKED \
                 (operator action: enable margin/perps on the demo account).",
                bal.status
            ),
        );
    }
    let settled_cents: i64 = bal
        .json
        .as_ref()
        .and_then(|j| j.get("settled_funds"))
        .and_then(|v| v.as_str())
        .and_then(dollars_to_ticks)
        .map(|t| t / 100)
        .unwrap_or(0);

    for (name, skew) in [
        ("auth__skew_minus5s", -5_000i64),
        ("auth__skew_plus5s", 5_000),
        ("auth__skew_minus30s", -30_000),
        ("auth__skew_plus30s", 30_000),
        ("auth__skew_minus5min", -300_000),
        ("auth__skew_plus5min", 300_000),
    ] {
        let _ = r
            .capture(
                name,
                get.clone(),
                &format!("{API_ROOT}/margin/balance"),
                None,
                AuthMode::SignedSkewMs(skew),
                "item 1: timestamp-skew probe (event API tolerated ±5s, rejected ±30s); \
                 skew rejections must present as 401 regardless of margin enablement",
            )
            .await;
    }
    let _ = r
        .capture(
            "auth__bad_signature",
            get.clone(),
            &format!("{API_ROOT}/margin/balance"),
            None,
            AuthMode::BadSignature,
            "item 1: valid-format signature over the wrong message — 401 body",
        )
        .await;

    // ============ public surfaces + item 8 orderbook ordering ============
    let markets = r
        .capture(
            "markets__list",
            get.clone(),
            &format!("{API_ROOT}/margin/markets"),
            None,
            AuthMode::Unauthenticated,
            "demo margin market catalog, unauthenticated (research says public)",
        )
        .await?;
    let ticker = markets
        .json
        .as_ref()
        .and_then(pick_market)
        .context("no active margin market found — cannot continue")?;
    r.note("markets__list", format!("session market = {ticker}"));

    let _ = r
        .capture(
            "markets__single",
            get.clone(),
            &format!("{API_ROOT}/margin/markets/{ticker}"),
            None,
            AuthMode::Unauthenticated,
            "single market object (tick_size / leverage_estimates / mark prices)",
        )
        .await;
    for (name, path, auth, note) in [
        (
            "exchange__status",
            format!("{API_ROOT}/margin/exchange/status"),
            AuthMode::Unauthenticated,
            "margin exchange status shape",
        ),
        (
            "risk__parameters",
            format!("{API_ROOT}/margin/risk_parameters"),
            AuthMode::Unauthenticated,
            "item 9 support: liquidation thresholds + IM multiplier map (IM=1.3xMM check)",
        ),
        (
            "account__limits_perps",
            format!("{API_ROOT}/account/limits/perps"),
            AuthMode::Signed,
            "perps rate-limit tier + read/write buckets (enablement-gating evidence \
             if 4xx)",
        ),
        (
            "fees__tiers",
            format!("{API_ROOT}/margin/fee_tiers"),
            AuthMode::Signed,
            "maker/taker rate maps (promo period; prod re-check is item 11/17, skipped)",
        ),
        (
            "risk__notional_limit",
            format!("{API_ROOT}/margin/notional_risk_limit"),
            AuthMode::Signed,
            "per-user notional caps (default value undocumented)",
        ),
        (
            "funding__rates_estimate",
            format!("{API_ROOT}/margin/funding_rates/estimate?ticker={ticker}"),
            AuthMode::Unauthenticated,
            "in-progress funding period estimate for the session market",
        ),
        (
            "funding__rates_historical",
            format!("{API_ROOT}/margin/funding_rates/historical?ticker={ticker}&limit=5"),
            AuthMode::Unauthenticated,
            "recent funding-rate history for the session market",
        ),
        (
            "funding__history_no_params",
            format!("{API_ROOT}/margin/funding_history"),
            AuthMode::Signed,
            "item 10 baseline probe: no params — first run revealed an UNDOCUMENTED \
             required start_date query argument (bare-msg 400)",
        ),
        (
            "funding__history_baseline",
            {
                // Dynamic window: funding posts at 04/12/20 UTC; a hardcoded
                // end_date went stale the moment the UTC date rolled (caught
                // when the 2026-06-12T04:00Z payment fell outside the window).
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("epoch")
                    .as_secs() as i64;
                let day = 86_400;
                let fmt = |t: i64| {
                    let days = t / day;
                    // civil date from days since epoch (Howard Hinnant algorithm)
                    let (mut y, mut doe) = (1970 + 4 * (days / 1461), days % 1461);
                    while doe >= if y % 4 == 0 { 366 } else { 365 } {
                        doe -= if y % 4 == 0 { 366 } else { 365 };
                        y += 1;
                    }
                    let leap = y % 4 == 0;
                    let ml = [
                        31,
                        if leap { 29 } else { 28 },
                        31,
                        30,
                        31,
                        30,
                        31,
                        31,
                        30,
                        31,
                        30,
                        31,
                    ];
                    let mut m = 0usize;
                    while doe >= ml[m] {
                        doe -= ml[m];
                        m += 1;
                    }
                    format!("{y:04}-{:02}-{:02}", m + 1, doe + 1)
                };
                format!(
                    "{API_ROOT}/margin/funding_history?start_date={}&end_date={}",
                    fmt(now - 7 * day),
                    fmt(now + day)
                )
            },
            AuthMode::Signed,
            "item 10 baseline: own funding payments BEFORE holding through a funding \
             time; start_date AND end_date are both REQUIRED (undocumented); ISO dates \
             accepted",
        ),
    ] {
        let _ = r.capture(name, get.clone(), &path, None, auth, note).await;
    }

    let book = r
        .capture(
            "orderbook__depth0",
            get.clone(),
            &format!("{API_ROOT}/margin/markets/{ticker}/orderbook?depth=0"),
            None,
            AuthMode::Unauthenticated,
            "item 8: full-depth book — settles the §11.1 sort-order conflict",
        )
        .await?;
    let (best_bid, best_ask, order_note) =
        book.json
            .as_ref()
            .map(book_extremes)
            .unwrap_or((None, None, "no body".to_string()));
    r.note("orderbook__depth0", order_note);
    let _ = r
        .capture(
            "orderbook__depth5",
            get.clone(),
            &format!("{API_ROOT}/margin/markets/{ticker}/orderbook?depth=5"),
            None,
            AuthMode::Unauthenticated,
            "item 8: depth=5 — which END of the array the 5 levels come from",
        )
        .await;
    let _ = r
        .capture(
            "orderbook__agg_010",
            get.clone(),
            &format!(
                "{API_ROOT}/margin/markets/{ticker}/orderbook?depth=5&aggregation_tick_size=0.10"
            ),
            None,
            AuthMode::Unauthenticated,
            "item 8: aggregation_tick_size=0.10 bucketing",
        )
        .await;
    if let (Some(bb), Some(ba)) = (best_bid, best_ask) {
        r.note(
            "session",
            format!(
                "best_bid={} best_ask={}",
                ticks_to_dollars(bb),
                ticks_to_dollars(ba)
            ),
        );
    }

    // ============ item 2: margin WS handshake + market-data capture ============
    let cmd_book = json!({
        "id": 1,
        "cmd": "subscribe",
        "params": {"channels": ["orderbook_delta", "trade"], "market_tickers": [ticker]},
    });
    let cmd_ticker = json!({
        "id": 2,
        "cmd": "subscribe",
        "params": {"channels": ["ticker"], "market_tickers": [ticker], "send_initial_snapshot": true},
    });
    match ws_connect_with_fallback(r, "ws__public").await {
        Ok(mut ws) => {
            ws.send(&cmd_book).await?;
            ws.send(&cmd_ticker).await?;
            ws.pump(Duration::from_secs(PUBLIC_WS_CAPTURE_SECS)).await?;
            ws.finish(
                r,
                "ws__public_orderbook_ticker",
                &[cmd_book, cmd_ticker],
                "item 2: subscribed acks, orderbook snapshot+delta with seq, ticker with \
                 funding_rate/mark prices, heartbeat pings",
            )
            .await;
        }
        Err(e) => r.note("ws__public_orderbook_ticker", format!("FAILED: {e:#}")),
    }

    // ============ private WS session (items 3/12 events; in degraded mode the
    // private-channel subscribe ack/error frames are themselves the fixture) ============
    let cmd_private = json!({
        "id": 1,
        "cmd": "subscribe",
        "params": {"channels": ["user_orders", "fill", "order_group_updates"]},
    });
    let mut private_ws = match ws_connect_with_fallback(r, "ws__private").await {
        Ok(mut ws) => {
            ws.send(&cmd_private).await?;
            ws.pump(PUMP).await?;
            Some(ws)
        }
        Err(e) => {
            r.note("ws__private_lifecycle", format!("connect FAILED: {e:#}"));
            None
        }
    };
    // Pump helper used between REST actions (drains user_orders/fill events).
    macro_rules! pump_ws {
        () => {
            if let Some(ws) = private_ws.as_mut() {
                let _ = ws.pump(PUMP).await;
            }
        };
    }

    let mut funding_position: Option<Value> = None;

    if margin_enabled {
        let best_bid =
            best_bid.context("empty bid side on the session market — cannot run order stages")?;
        let best_ask =
            best_ask.context("empty ask side on the session market — cannot run order stages")?;
        // Far-from-touch resting bid: 15% below best bid. Always inside the
        // price band (floor = min(80% of best bid, 1000 ticks below) <= 85%).
        let far_bid = best_bid * 85 / 100;

        // ===== items 3+4: order lifecycle, duplicate, freed-after-cancel =====
        let ord_a_body = gtc_order(&ticker, "bid", "1", far_bid, false);
        let (ord_a, ord_a_id) = r
            .place(
                "orders__create_gtc",
                ord_a_body.clone(),
                "item 3: GTC limit, post_only=false, 1 contract, 15% below touch",
            )
            .await?;
        pump_ws!();
        if ord_a.status == 201 {
            let _ = r
                .place(
                    "orders__duplicate_client_order_id",
                    ord_a_body.clone(),
                    "item 4: exact resubmission — expect 409; capture the code string",
                )
                .await;
        }
        if let Some(oid) = ord_a_id.clone() {
            let _ = r
                .capture(
                    "orders__get_after_create",
                    get.clone(),
                    &format!("{API_ROOT}/margin/orders/{oid}"),
                    None,
                    AuthMode::Signed,
                    "item 3: MarginOrder read surface (NO status field — derive from counts)",
                )
                .await;
            let _ = r
                .cancel(
                    "orders__cancel",
                    &oid,
                    "item 3: happy-path cancel (reduced_by)",
                )
                .await;
            pump_ws!();
            let _ = r
                .capture(
                    "orders__get_after_cancel",
                    get.clone(),
                    &format!("{API_ROOT}/margin/orders/{oid}"),
                    None,
                    AuthMode::Signed,
                    "item 3: read surface after cancel (last_update_reason transition; \
                     event API had a stale-read race here — compare timestamps)",
                )
                .await;
            let _ = r
                .place(
                    "orders__reuse_canceled_client_id",
                    ord_a_body,
                    "item 4: does a CANCELED order's client_order_id free up? \
                     (event API: NO, 409 forever)",
                )
                .await;
            pump_ws!();
        }

        // ord_b: post_only resting; amend both kinds. Count 2 so the amend-
        // decrease (2 -> 1) is observable; rests 15% below touch, no fill risk.
        let (ord_b, ord_b_id) = r
            .place(
                "orders__create_post_only",
                gtc_order(&ticker, "bid", "2", far_bid + 1, true),
                "item 3: GTC limit, post_only=true, resting far below touch",
            )
            .await?;
        pump_ws!();
        if let (201, Some(oid)) = (ord_b.status, ord_b_id.clone()) {
            let amend_dec = r
                .capture(
                    "orders__amend_decrease",
                    post.clone(),
                    &format!("{API_ROOT}/margin/orders/{oid}/amend"),
                    Some(json!({
                        "ticker": ticker,
                        "side": "bid",
                        "price": ticks_to_dollars(far_bid + 1),
                        "count": "1",
                    })),
                    AuthMode::Signed,
                    "item 3: amend SAME price, count 2->1 — the queue-KEEPING decrease",
                )
                .await;
            pump_ws!();
            let after_dec_id = amend_dec
                .ok()
                .and_then(|c| c.str_field("order_id"))
                .unwrap_or_else(|| oid.clone());
            if after_dec_id != oid {
                r.placed.push((after_dec_id.clone(), false));
                r.mark_done(&oid);
            }
            let amend_px = r
                .capture(
                    "orders__amend_price",
                    post.clone(),
                    &format!("{API_ROOT}/margin/orders/{after_dec_id}/amend"),
                    Some(json!({
                        "ticker": ticker,
                        "side": "bid",
                        "price": ticks_to_dollars(far_bid - 50),
                        "count": "1",
                    })),
                    AuthMode::Signed,
                    "item 3: amend price change — the queue-LOSING amendment",
                )
                .await;
            pump_ws!();
            let after_px_id = amend_px
                .ok()
                .and_then(|c| c.str_field("order_id"))
                .unwrap_or_else(|| after_dec_id.clone());
            if after_px_id != after_dec_id {
                r.placed.push((after_px_id.clone(), false));
                r.mark_done(&after_dec_id);
            }
            let _ = r
                .capture(
                    "orders__get_after_amend",
                    get.clone(),
                    &format!("{API_ROOT}/margin/orders/{after_px_id}"),
                    None,
                    AuthMode::Signed,
                    "item 3: last_update_reason after Amend",
                )
                .await;
            let _ = r
                .cancel(
                    "orders__cancel_after_amend",
                    &after_px_id,
                    "item 3: cleanup of ord_b",
                )
                .await;
            pump_ws!();
        }

        // ord_c: the dedicated /decrease endpoint (reduce_by).
        let (ord_c, ord_c_id) = r
            .place(
                "orders__create_for_decrease",
                gtc_order(&ticker, "bid", "2", far_bid, false),
                "item 3: GTC 2 contracts for the /decrease probe",
            )
            .await?;
        pump_ws!();
        if let (201, Some(oid)) = (ord_c.status, ord_c_id) {
            let _ = r
                .capture(
                    "orders__decrease_reduce_by",
                    post.clone(),
                    &format!("{API_ROOT}/margin/orders/{oid}/decrease"),
                    Some(json!({"reduce_by": "1"})),
                    AuthMode::Signed,
                    "item 3: POST /decrease reduce_by=1 (XOR reduce_to)",
                )
                .await;
            pump_ws!();
            let _ = r
                .capture(
                    "orders__get_after_decrease",
                    get.clone(),
                    &format!("{API_ROOT}/margin/orders/{oid}"),
                    None,
                    AuthMode::Signed,
                    "item 3: last_update_reason=Decrease expected",
                )
                .await;
            let _ = r
                .cancel(
                    "orders__cancel_after_decrease",
                    &oid,
                    "item 3: cleanup of ord_c",
                )
                .await;
            pump_ws!();
        }

        // ===== item 10: the deliberate funding position (LEFT OPEN) =====
        let funding_body = json!({
            "ticker": ticker,
            "client_order_id": uuid4(),
            "side": "bid",
            "count": "1",
            "price": ticks_to_dollars(best_ask + 200),
            "time_in_force": "immediate_or_cancel",
            "self_trade_prevention_type": "taker_at_cross",
        });
        let (funding, _) = r
            .place(
                "orders__funding_position_ioc",
                funding_body,
                "item 10 SETUP: IOC buy 1 contract, crossing — position LEFT OPEN \
                 deliberately for the 04/12/20 UTC funding capture (later session). \
                 Also item 3: average_fill_price/average_fee_paid presence on fills.",
            )
            .await?;
        pump_ws!();
        let fill_count = funding.str_field("fill_count").unwrap_or_default();
        let avg_price = funding.str_field("average_fill_price").unwrap_or_default();
        let avg_fee = funding.str_field("average_fee_paid").unwrap_or_default();
        r.note(
            "orders__funding_position_ioc",
            format!(
                "OPEN FUNDING POSITION: {ticker} long fill_count={fill_count} \
                 avg_price={avg_price} avg_fee={avg_fee} — DO NOT CLOSE"
            ),
        );
        funding_position = Some(json!({
            "ticker": ticker,
            "side": "long",
            "contracts": fill_count,
            "average_fill_price": avg_price,
            "average_fee_paid": avg_fee,
            "purpose": "research §12 item 10 — hold across a 04/12/20 UTC funding time; \
                        capture GET /margin/funding_history in a later session; DO NOT CLOSE",
        }));
        let _ = r
            .capture(
                "fills__after_open",
                get.clone(),
                &format!("{API_ROOT}/margin/fills?ticker={ticker}&limit=10"),
                None,
                AuthMode::Signed,
                "item 3: MarginFill shape (entry_price, realized_pnl, fees) after the IOC fill",
            )
            .await;

        // ===== item 9: positions / balance / risk with an open position =====
        let _ = r
            .capture(
                "positions__open",
                get.clone(),
                &format!("{API_ROOT}/margin/positions"),
                None,
                AuthMode::Signed,
                "item 9: signed position, entry_price, margin_used, roe",
            )
            .await;
        let _ = r
            .capture(
                "balance__compute_available",
                get.clone(),
                &format!("{API_ROOT}/margin/balance?compute_available_balance=true"),
                None,
                AuthMode::Signed,
                "item 9: 50-token balance variant with available-balance computation",
            )
            .await;
        let _ = r
            .capture(
                "risk__account",
                get.clone(),
                &format!("{API_ROOT}/margin/risk"),
                None,
                AuthMode::Signed,
                "item 9: account+position leverage, liquidation prices — IM=1.3xMM and \
                 margin-ratio-direction check data (vs risk__parameters)",
            )
            .await;

        // ===== item 5: reduce_only with GTC (expect rejection) =====
        let (ro, ro_id) = r
            .place(
                "orders__reduce_only_gtc",
                json!({
                    "ticker": ticker,
                    "client_order_id": uuid4(),
                    "side": "ask",
                    "count": "1",
                    "price": ticks_to_dollars(best_ask + 400),
                    "time_in_force": "good_till_canceled",
                    "self_trade_prevention_type": "taker_at_cross",
                    "reduce_only": true,
                }),
                "item 5: reduce_only + GTC — spec says rejected unless IOC/FOK; exact body",
            )
            .await?;
        if ro.status == 201 {
            // Research contradicted: it was accepted. It rests far above touch
            // (no fill risk to the funding position); cancel it immediately.
            if let Some(oid) = ro_id {
                let _ = r
                    .cancel(
                        "orders__reduce_only_gtc_cancel",
                        &oid,
                        "item 5 CONTRADICTION cleanup: reduce_only GTC was ACCEPTED — \
                         canceled immediately to protect the funding position",
                    )
                    .await;
            }
        }
        pump_ws!();

        // ===== item 6: insufficient margin =====
        // Size the order so required margin far exceeds the mock balance:
        // notional ~= 20x settled funds (plus a floor in case balance parse failed).
        let huge_count = (settled_cents * 2_000 / far_bid.max(1)) + 10_000;
        let _ = r
            .place(
                "orders__insufficient_margin",
                json!({
                    "ticker": ticker,
                    "client_order_id": uuid4(),
                    "side": "bid",
                    "count": format!("{huge_count}"),
                    "price": ticks_to_dollars(far_bid),
                    "time_in_force": "good_till_canceled",
                    "self_trade_prevention_type": "taker_at_cross",
                }),
                "item 6: order whose margin requirement exceeds the demo balance — \
                 exact 400 code/message (may surface as notional_risk_limit instead)",
            )
            .await;

        // ===== item 7: price band + off-tick =====
        let _ = r
            .place(
                "orders__price_band_violation",
                gtc_order(&ticker, "bid", "1", best_bid / 2, false),
                "item 7: bid at 50% of best bid — below the band floor \
                 min(80% of bb, 1000 ticks below bb); exact error body",
            )
            .await;
        let off_tick_price = format!("{}5", ticks_to_dollars(far_bid));
        let _ = r
            .place(
                "orders__off_tick_price",
                json!({
                    "ticker": ticker,
                    "client_order_id": uuid4(),
                    "side": "bid",
                    "count": "1",
                    "price": off_tick_price,
                    "time_in_force": "good_till_canceled",
                    "self_trade_prevention_type": "taker_at_cross",
                }),
                "item 7: 5-decimal price (off the 0.0001 tick grid) — exact error body",
            )
            .await;
        pump_ws!();

        // ===== item 12: order groups as runaway rail =====
        let group = r
            .capture(
                "groups__create",
                post.clone(),
                &format!("{API_ROOT}/margin/order_groups/create"),
                Some(json!({"contracts_limit": 10})),
                AuthMode::Signed,
                "item 12: create group, 10-contract rolling-15s limit",
            )
            .await?;
        let group_id = group.str_field("order_group_id");
        let _ = r
            .capture(
                "groups__list",
                get.clone(),
                &format!("{API_ROOT}/margin/order_groups"),
                None,
                AuthMode::Signed,
                "item 12: list groups",
            )
            .await;
        if let Some(gid) = group_id {
            let _ = r
                .capture(
                    "groups__get",
                    get.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}"),
                    None,
                    AuthMode::Signed,
                    "item 12: group detail (is_auto_cancel_enabled, orders)",
                )
                .await;
            let mut grouped = gtc_order(&ticker, "bid", "1", far_bid, false);
            grouped["order_group_id"] = json!(gid);
            let (g_ord, g_ord_id) = r
                .place(
                    "orders__create_in_group",
                    grouped,
                    "item 12: GTC order attached to the group",
                )
                .await?;
            pump_ws!();
            let _ = r
                .capture(
                    "groups__update_limit",
                    put.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}/limit"),
                    Some(json!({"contracts_limit": 5})),
                    AuthMode::Signed,
                    "item 12: PUT limit update (would trigger if new limit already exceeded)",
                )
                .await;
            let _ = r
                .capture(
                    "groups__trigger",
                    put.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}/trigger"),
                    Some(json!({})),
                    AuthMode::Signed,
                    "item 12: trigger — cancels grouped orders, blocks new ones until reset",
                )
                .await;
            pump_ws!();
            if let (201, Some(oid)) = (g_ord.status, g_ord_id) {
                let after = r
                    .capture(
                        "orders__get_after_group_trigger",
                        get.clone(),
                        &format!("{API_ROOT}/margin/orders/{oid}"),
                        None,
                        AuthMode::Signed,
                        "item 12: grouped order state after trigger (last_update_reason)",
                    )
                    .await;
                // The trigger should have canceled it; mark done if it shows
                // no remaining contracts so cleanup does not re-cancel.
                if let Ok(cap) = after {
                    let remaining = cap
                        .json
                        .as_ref()
                        .map(|j| j.get("order").unwrap_or(j))
                        .and_then(|j| j.get("remaining_count"))
                        .and_then(|v| v.as_str())
                        .and_then(dollars_to_ticks);
                    if remaining == Some(0) {
                        r.mark_done(&oid);
                    }
                }
            }
            let _ = r
                .capture(
                    "groups__reset",
                    put.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}/reset"),
                    Some(json!({})),
                    AuthMode::Signed,
                    "item 12: reset after trigger — re-allows orders",
                )
                .await;
            pump_ws!();
            let _ = r
                .capture(
                    "groups__get_after_reset",
                    get.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}"),
                    None,
                    AuthMode::Signed,
                    "item 12: group state after reset",
                )
                .await;
            let _ = r
                .capture(
                    "groups__delete",
                    delete.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}"),
                    None,
                    AuthMode::Signed,
                    "item 12: delete group (cleanup; also cancels members)",
                )
                .await;
        } else {
            r.note(
                "groups__create",
                "no order_group_id in response — group stages skipped".into(),
            );
        }

        // ===== item 13: subaccount transfer idempotency =====
        let _ = r
            .capture(
                "subaccounts__create_nobody",
                post.clone(),
                &format!("{API_ROOT}/portfolio/margin/subaccounts"),
                None,
                AuthMode::Signed,
                "item 13: body-less POST (OpenAPI declares no requestBody) — first run \
                 showed the API requires a JSON content type anyway (flat 400)",
            )
            .await;
        let sub = r
            .capture(
                "subaccounts__create",
                post.clone(),
                &format!("{API_ROOT}/portfolio/margin/subaccounts"),
                Some(json!({})),
                AuthMode::Signed,
                "item 13: create a margin subaccount (empty JSON body)",
            )
            .await?;
        let sub_n = sub
            .json
            .as_ref()
            .and_then(|j| j.get("subaccount_number"))
            .and_then(|v| v.as_i64())
            .unwrap_or(1);
        let transfer_body = json!({
            "client_transfer_id": uuid4(),
            "from_subaccount": 0,
            "to_subaccount": sub_n,
            "amount_cents": 1,
        });
        let first = r
            .capture(
                "subaccounts__transfer_first",
                post.clone(),
                &format!("{API_ROOT}/portfolio/margin/subaccounts/transfer"),
                Some(transfer_body.clone()),
                AuthMode::Signed,
                "item 13: 1-cent transfer primary -> subaccount",
            )
            .await?;
        let _ = r
            .capture(
                "subaccounts__transfer_duplicate",
                post.clone(),
                &format!("{API_ROOT}/portfolio/margin/subaccounts/transfer"),
                Some(transfer_body),
                AuthMode::Signed,
                "item 13: SAME client_transfer_id resubmitted — idempotency behavior",
            )
            .await;
        if first.status == 200 {
            let _ = r
                .capture(
                    "subaccounts__transfer_back",
                    post.clone(),
                    &format!("{API_ROOT}/portfolio/margin/subaccounts/transfer"),
                    Some(json!({
                        "client_transfer_id": uuid4(),
                        "from_subaccount": sub_n,
                        "to_subaccount": 0,
                        "amount_cents": 1,
                    })),
                    AuthMode::Signed,
                    "item 13 cleanup: return the cent to the primary subaccount",
                )
                .await;
        }
    } else {
        // ============ DEGRADED MODE: one blocked-evidence probe per private
        // item family; expand a family only if its probe succeeds. ============
        if let Some(bb) = best_bid {
            let far_bid = bb * 85 / 100;
            let (cap, oid) = r
                .place(
                    "orders__create_gtc_blocked",
                    gtc_order(&ticker, "bid", "1", far_bid, false),
                    "items 3-7 blocked-evidence probe: order create against a \
                     non-margin-enabled account — exact gate error body",
                )
                .await?;
            if cap.status == 201 {
                r.note(
                    "orders__create_gtc_blocked",
                    "CONTRADICTION: create succeeded despite enabled=false — canceling".into(),
                );
                if let Some(oid) = oid {
                    let _ = r
                        .cancel(
                            "orders__create_gtc_blocked_cancel",
                            &oid,
                            "immediate cancel of the contradiction probe",
                        )
                        .await;
                }
            }
        }
        let _ = r
            .capture(
                "positions__blocked",
                get.clone(),
                &format!("{API_ROOT}/margin/positions"),
                None,
                AuthMode::Signed,
                "items 9/10 blocked-evidence probe: positions read while not margin-enabled",
            )
            .await;
        let group = r
            .capture(
                "groups__create",
                post.clone(),
                &format!("{API_ROOT}/margin/order_groups/create"),
                Some(json!({"contracts_limit": 10})),
                AuthMode::Signed,
                "item 12 probe: group create while not margin-enabled",
            )
            .await?;
        if let Some(gid) = group.str_field("order_group_id") {
            // Groups work without enablement: capture the full rail minus
            // member orders (those are blocked).
            let _ = r
                .capture(
                    "groups__get",
                    get.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}"),
                    None,
                    AuthMode::Signed,
                    "item 12: group detail (no member orders possible in degraded mode)",
                )
                .await;
            let _ = r
                .capture(
                    "groups__trigger",
                    put.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}/trigger"),
                    Some(json!({})),
                    AuthMode::Signed,
                    "item 12: trigger (empty group)",
                )
                .await;
            pump_ws!();
            let _ = r
                .capture(
                    "groups__reset",
                    put.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}/reset"),
                    Some(json!({})),
                    AuthMode::Signed,
                    "item 12: reset after trigger",
                )
                .await;
            let _ = r
                .capture(
                    "groups__delete",
                    delete.clone(),
                    &format!("{API_ROOT}/margin/order_groups/{gid}"),
                    None,
                    AuthMode::Signed,
                    "item 12: delete group (cleanup)",
                )
                .await;
        }
        let _ = r
            .capture(
                "subaccounts__create_nobody",
                post.clone(),
                &format!("{API_ROOT}/portfolio/margin/subaccounts"),
                None,
                AuthMode::Signed,
                "item 13: body-less POST (OpenAPI declares no requestBody) — the API \
                 requires a JSON content type anyway (flat 400 invalid_content_type)",
            )
            .await;
        let sub = r
            .capture(
                "subaccounts__create",
                post.clone(),
                &format!("{API_ROOT}/portfolio/margin/subaccounts"),
                Some(json!({})),
                AuthMode::Signed,
                "item 13 probe: subaccount create (empty JSON body) while not \
                 margin-enabled",
            )
            .await?;
        if sub.status == 201 {
            let sub_n = sub
                .json
                .as_ref()
                .and_then(|j| j.get("subaccount_number"))
                .and_then(|v| v.as_i64())
                .unwrap_or(1);
            let transfer_body = json!({
                "client_transfer_id": uuid4(),
                "from_subaccount": 0,
                "to_subaccount": sub_n,
                "amount_cents": 1,
            });
            let first = r
                .capture(
                    "subaccounts__transfer_first",
                    post.clone(),
                    &format!("{API_ROOT}/portfolio/margin/subaccounts/transfer"),
                    Some(transfer_body.clone()),
                    AuthMode::Signed,
                    "item 13: 1-cent transfer primary -> subaccount",
                )
                .await?;
            let _ = r
                .capture(
                    "subaccounts__transfer_duplicate",
                    post.clone(),
                    &format!("{API_ROOT}/portfolio/margin/subaccounts/transfer"),
                    Some(transfer_body),
                    AuthMode::Signed,
                    "item 13: SAME client_transfer_id resubmitted — idempotency behavior",
                )
                .await;
            if first.status == 200 {
                let _ = r
                    .capture(
                        "subaccounts__transfer_back",
                        post.clone(),
                        &format!("{API_ROOT}/portfolio/margin/subaccounts/transfer"),
                        Some(json!({
                            "client_transfer_id": uuid4(),
                            "from_subaccount": sub_n,
                            "to_subaccount": 0,
                            "amount_cents": 1,
                        })),
                        AuthMode::Signed,
                        "item 13 cleanup: return the cent to the primary subaccount",
                    )
                    .await;
            }
        }
    }

    // ============ item 16: intra-exchange transfer (expected 4xx; both modes) ============
    let _ = r
        .capture(
            "transfer__intra_exchange",
            post.clone(),
            &format!("{API_ROOT}/portfolio/intra_exchange_instance_transfer"),
            Some(json!({
                "source": "event_contract",
                "destination": "margined",
                // Default 100 centicents = 1 cent. Overridable so a first run can
                // FUND the margin subaccount for the order lifecycle of a second
                // run (the rail proved live on demo: 200 + transfer_id).
                "amount": std::env::var("KINETICS_FUND_CENTICENTS")
                    .ok()
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(100),
            })),
            AuthMode::Signed,
            "item 16: documented 'currently not available' — capture the 4xx body \
             (amount is in CENTICENTS; 100 = 1 cent; KINETICS_FUND_CENTICENTS overrides)",
        )
        .await;

    // ============ item 14: GET /margin/orders status-filter probes ============
    let list_all = r
        .capture(
            "orders__list_all",
            get.clone(),
            &format!("{API_ROOT}/margin/orders?limit=10"),
            None,
            AuthMode::Signed,
            "item 14: unfiltered list (probe; filters run only if this is 200)",
        )
        .await?;
    if list_all.status == 200 {
        for (name, q) in [
            ("orders__filter_resting", "?status=resting&limit=10"),
            ("orders__filter_canceled", "?status=canceled&limit=10"),
            ("orders__filter_executed", "?status=executed&limit=10"),
            ("orders__filter_open", "?status=open&limit=10"),
            (
                "orders__filter_garbage",
                "?status=not-a-real-status&limit=10",
            ),
        ] {
            let _ = r
                .capture(
                    name,
                    get.clone(),
                    &format!("{API_ROOT}/margin/orders{q}"),
                    None,
                    AuthMode::Signed,
                    "item 14: status-filter vocabulary probe (MarginOrder has no status \
                     field; an error body may enumerate accepted values)",
                )
                .await;
        }
    }

    // ============ cleanup: cancel anything still tracked as live ============
    // The item-10 funding position is a POSITION (its IOC order is terminal),
    // so it is never in this list — it stays open by design.
    let leftovers: Vec<String> = r
        .placed
        .iter()
        .filter(|(_, done)| !done)
        .map(|(id, _)| id.clone())
        .collect();
    for (i, oid) in leftovers.iter().enumerate() {
        let _ = r
            .cancel(
                &format!("cleanup__leftover_{i}"),
                oid,
                "end-of-session cancel of any order still tracked live \
                 (terminal-state cancels are themselves useful fixtures)",
            )
            .await;
    }
    if margin_enabled {
        let _ = r
            .capture(
                "orders__final_resting",
                get.clone(),
                &format!("{API_ROOT}/margin/orders?status=resting&limit=100"),
                None,
                AuthMode::Signed,
                "cleanup verification: expect ZERO resting orders at session end",
            )
            .await;
        let _ = r
            .capture(
                "positions__final",
                get.clone(),
                &format!("{API_ROOT}/margin/positions"),
                None,
                AuthMode::Signed,
                "cleanup verification: expect EXACTLY the 1-contract funding position",
            )
            .await;
    }

    // ============ close private WS + manifest ============
    if let Some(mut ws) = private_ws.take() {
        let _ = ws.pump(Duration::from_secs(5)).await;
        let saw = format!(
            "saw user_orders={} fill={} order_group_updates={}",
            ws.saw_type("user_orders"),
            ws.saw_type("fill"),
            ws.saw_type("order_group_updates")
        );
        r.note("ws__private_lifecycle", saw);
        ws.finish(
            r,
            "ws__private_lifecycle",
            &[cmd_private],
            "items 3+12: user_orders/fill/order_group_updates frames captured across \
             the order-lifecycle stage (or the private-subscribe ack/error frames in \
             degraded mode)",
        )
        .await;
    }

    let manifest = json!({
        "recorded_at_epoch_ms": now_ms(),
        "environment": "demo",
        "tool": "examples/record_kinetics_fixtures.rs",
        "session_market": ticker,
        "margin_enabled": margin_enabled,
        "open_funding_position": funding_position,
        "results": r.summary.iter().map(|(n, s)| json!({"stage": n, "result": s})).collect::<Vec<_>>(),
    });
    let pretty = serde_json::to_string_pretty(&manifest).context("manifest")?;
    std::fs::write(r.out.join("session__manifest.meta.json"), pretty)?;

    println!("\n==== session summary ({} rows) ====", r.summary.len());
    for (n, s) in &r.summary {
        println!("{n:45} {s}");
    }
    Ok(())
}
