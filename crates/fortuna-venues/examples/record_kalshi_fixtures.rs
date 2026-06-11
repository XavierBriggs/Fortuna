//! Kalshi DEMO-environment fixture recorder (operator-authorized session,
//! 2026-06-10: "do the kalshi demo fixture recording").
//!
//! Records the operator fixtures demanded by the 27-item checklist in
//! docs/research/venue/kalshi-api-2026-06-10/research.md §Uncertainties into
//! fixtures/kalshi/ as `<area>__<case>.json` (verbatim response body) plus a
//! sibling `.meta.json` (method, path, status, sanitized request body, note).
//!
//! SAFETY RAILS:
//! - Demo hosts are HARDCODED. Reads ONLY the demo credential env vars
//!   (KALSHI_API_DEMO_KEY_ID, KALSHI_DEMO_PRIVATE_KEY_PATH); the production
//!   variable names are never referenced. Demo keys do not work on prod and
//!   vice versa (research §2), so a mixed-up key fails closed.
//! - No secret material is ever printed or written into fixtures/meta:
//!   request headers are not recorded at all.
//! - Every order this tool places is tracked and canceled in the cleanup
//!   stage (mock funds regardless).
//!
//! This is an IO-edge capture TOOL, not core code: it signs live requests,
//! so it uses wall-clock time directly (the injected-Clock rule governs the
//! deterministic core, not one-shot operator tooling — see CLAUDE.md).
//!
//! Run: `cargo run -p fortuna-venues --example record_kalshi_fixtures`
//! (with the two demo env vars set; see fixtures/kalshi/README.md).

use anyhow::{bail, Context, Result};
use fortuna_venues::kalshi::auth::{KalshiSigner, HEADER_KEY, HEADER_SIGNATURE, HEADER_TIMESTAMP};
use fortuna_venues::kalshi::ws::subscribe_orderbook_cmd;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REST_HOST: &str = "https://external-api.demo.kalshi.co";
const REST_HOST_ALT: &str = "https://demo-api.kalshi.co";
const WS_HOST: &str = "wss://external-api-ws.demo.kalshi.co";
const API_ROOT: &str = "/trade-api/v2";
const WS_PATH: &str = "/trade-api/ws/v2";
const FIXTURE_DIR: &str = "fixtures/kalshi";
const PACE: Duration = Duration::from_millis(350);
const ORDER_PACE: Duration = Duration::from_millis(1000);
const WS_CAPTURE_SECS: u64 = 90;
const WS_MAX_FRAMES: usize = 5000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// RFC-4122-shaped v4 id from the process CSPRNG (client_order_id values).
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

/// Cents -> FixedPointDollars string ("0.0100"). Order prices are 1..=99c.
fn dollars(cents: i64) -> String {
    format!("{}.{:02}00", cents / 100, cents % 100)
}

/// Parse "0.5600" / "1.00" style dollars strings (or bare ints) to cents.
fn to_cents(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    let s = v.as_str()?;
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    let whole: i64 = whole.parse().ok()?;
    let frac2: i64 = if frac.is_empty() {
        0
    } else {
        let two: String = frac.chars().chain(std::iter::repeat('0')).take(2).collect();
        two.parse().ok()?
    };
    Some(whole * 100 + frac2)
}

#[derive(Clone, Copy)]
enum AuthMode {
    Signed,
    SignedSkewMs(i64),
    BadSignature,
    UnknownKeyId,
    MissingSignature,
    Unauthenticated,
}

impl AuthMode {
    fn label(self) -> &'static str {
        match self {
            AuthMode::Signed => "signed",
            AuthMode::SignedSkewMs(_) => "signed-skewed",
            AuthMode::BadSignature => "bad-signature",
            AuthMode::UnknownKeyId => "unknown-key-id",
            AuthMode::MissingSignature => "missing-signature-header",
            AuthMode::Unauthenticated => "unauthenticated",
        }
    }
}

struct Captured {
    status: u16,
    json: Option<Value>,
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
    #[allow(clippy::too_many_arguments)]
    async fn capture(
        &mut self,
        name: &str,
        method: reqwest::Method,
        host: &str,
        path_q: &str,
        body: Option<Value>,
        auth: AuthMode,
        note: &str,
    ) -> Result<Captured> {
        tokio::time::sleep(PACE).await;
        let url = format!("{host}{path_q}");
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
                let h = self
                    .signer
                    .sign(method.as_str(), "/trade-api/v2/not-the-real-path", ts)?;
                for (k, v) in h.as_header_pairs() {
                    req = req.header(k, v);
                }
            }
            AuthMode::UnknownKeyId => {
                let h = self.signer.sign(method.as_str(), path_q, ts)?;
                req = req
                    .header(HEADER_KEY, "00000000-0000-0000-0000-000000000000")
                    .header(HEADER_SIGNATURE, h.signature_b64.as_str())
                    .header(HEADER_TIMESTAMP, h.timestamp_ms.as_str());
            }
            AuthMode::MissingSignature => {
                let h = self.signer.sign(method.as_str(), path_q, ts)?;
                req = req
                    .header(HEADER_KEY, h.api_key_id.as_str())
                    .header(HEADER_TIMESTAMP, h.timestamp_ms.as_str());
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
            "host": host,
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

    /// Place a V2 order and track it for cleanup. Returns (capture, order_id).
    async fn place_v2(
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
                REST_HOST,
                &format!("{API_ROOT}/portfolio/events/orders"),
                Some(body),
                AuthMode::Signed,
                note,
            )
            .await?;
        let id = cap
            .json
            .as_ref()
            .and_then(|j| j.get("order_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if let Some(ref oid) = id {
            self.placed.push((oid.clone(), false));
        }
        Ok((cap, id))
    }

    async fn cancel_v2(&mut self, name: &str, order_id: &str, note: &str) -> Result<Captured> {
        tokio::time::sleep(ORDER_PACE).await;
        let cap = self
            .capture(
                name,
                reqwest::Method::DELETE,
                REST_HOST,
                &format!("{API_ROOT}/portfolio/events/orders/{order_id}"),
                None,
                AuthMode::Signed,
                note,
            )
            .await?;
        if cap.status == 200 {
            for (oid, done) in &mut self.placed {
                if oid == order_id {
                    *done = true;
                }
            }
        }
        Ok(cap)
    }
}

/// Liquidity scoring over GET /markets results: prefer two-sided books,
/// then highest volume; (liquid_market, soonest_closing_two_sided).
fn pick_markets(list: &Value) -> (Option<Value>, Option<Value>) {
    let arr = match list.get("markets").and_then(|m| m.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => return (None, None),
    };
    let bid = |m: &Value| {
        m.get("yes_bid_dollars")
            .or_else(|| m.get("yes_bid"))
            .and_then(to_cents)
            .unwrap_or(0)
    };
    let ask = |m: &Value| {
        m.get("yes_ask_dollars")
            .or_else(|| m.get("yes_ask"))
            .and_then(to_cents)
            .unwrap_or(0)
    };
    let vol = |m: &Value| {
        m.get("volume")
            .or_else(|| m.get("volume_fp"))
            .and_then(to_cents)
            .unwrap_or(0)
    };
    let two_sided: Vec<&Value> = arr
        .iter()
        .filter(|m| bid(m) > 0 && ask(m) > 0 && ask(m) < 100)
        .collect();
    let liquid = two_sided
        .iter()
        .max_by_key(|m| vol(m))
        .copied()
        .or_else(|| arr.first())
        .cloned();
    let soonest = two_sided
        .iter()
        .filter_map(|m| {
            m.get("close_time")
                .and_then(|c| c.as_str())
                .map(|c| (c.to_string(), *m))
        })
        .min_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, m)| m.clone());
    (liquid, soonest)
}

fn jstr<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
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

    let out = Path::new(FIXTURE_DIR).to_path_buf();
    std::fs::create_dir_all(&out).context("creating fixtures/kalshi")?;
    if !Path::new("crates").is_dir() {
        bail!("run from the repo root (fixtures/kalshi must resolve)");
    }

    println!("Kalshi DEMO fixture recorder — hosts: {REST_HOST} / {REST_HOST_ALT} / {WS_HOST}");
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

    // ============ auth round-trip (checklist #1-#5) ============
    let bal = r
        .capture(
            "auth__balance_ok",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/portfolio/balance"),
            None,
            AuthMode::Signed,
            "checklist #1: happy-path signed GET balance; #19 balance units",
        )
        .await?;
    if bal.status != 200 {
        bail!(
            "signed balance call returned HTTP {} — aborting before any further stages \
             (credentials or signing are wrong; nothing else is meaningful)",
            bal.status
        );
    }
    let balance_cents = bal
        .json
        .as_ref()
        .and_then(|j| j.get("balance"))
        .and_then(to_cents)
        .unwrap_or(0);
    r.note("auth__balance_ok", format!("balance_cents={balance_cents}"));

    let _ = r
        .capture(
            "auth__balance_alt_host",
            get.clone(),
            REST_HOST_ALT,
            &format!("{API_ROOT}/portfolio/balance"),
            None,
            AuthMode::Signed,
            "checklist #4: signature path identical across both demo hosts",
        )
        .await;

    for (name, skew, note) in [
        ("auth__skew_minus5s", -5_000i64, "checklist #2"),
        ("auth__skew_plus5s", 5_000, "checklist #2"),
        ("auth__skew_minus30s", -30_000, "checklist #2"),
        ("auth__skew_minus5min", -300_000, "checklist #2"),
        ("auth__skew_plus5min", 300_000, "checklist #2"),
    ] {
        let _ = r
            .capture(
                name,
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/portfolio/balance"),
                None,
                AuthMode::SignedSkewMs(skew),
                note,
            )
            .await;
    }
    for (name, mode, note) in [
        (
            "auth__bad_signature",
            AuthMode::BadSignature,
            "checklist #3: valid-format signature over the wrong message",
        ),
        (
            "auth__unknown_key_id",
            AuthMode::UnknownKeyId,
            "checklist #3: all-zeros key id",
        ),
        (
            "auth__missing_signature_header",
            AuthMode::MissingSignature,
            "checklist #3: KALSHI-ACCESS-SIGNATURE omitted",
        ),
    ] {
        let _ = r
            .capture(
                name,
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/portfolio/balance"),
                None,
                mode,
                note,
            )
            .await;
    }

    // ============ markets + pagination (#5, #17, #18, #21) ============
    let _ = r
        .capture(
            "markets__unauth_list",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/markets?limit=2"),
            None,
            AuthMode::Unauthenticated,
            "checklist #5: unauthenticated GET /markets",
        )
        .await;

    let open = r
        .capture(
            "markets__status_open",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/markets?status=open&limit=200"),
            None,
            AuthMode::Signed,
            "checklist #21: status vocabulary (open filter); market-selection pool",
        )
        .await?;
    let (liquid, soonest) = open.json.as_ref().map(pick_markets).unwrap_or((None, None));
    let liquid = liquid.context("no open markets returned — cannot continue order stages")?;
    let ticker = jstr(&liquid, "ticker")
        .context("chosen market has no ticker")?
        .to_string();
    let series_ticker = jstr(&liquid, "series_ticker").map(str::to_string);
    let best_bid_cents = liquid
        .get("yes_bid_dollars")
        .or_else(|| liquid.get("yes_bid"))
        .and_then(to_cents)
        .unwrap_or(0);
    let price_structure = jstr(&liquid, "price_level_structure")
        .unwrap_or("(absent)")
        .to_string();
    let settle_ticker = soonest
        .as_ref()
        .and_then(|m| jstr(m, "ticker"))
        .map(str::to_string);
    r.note(
        "markets__status_open",
        format!(
            "chosen ticker={ticker} best_bid={best_bid_cents}c structure={price_structure} \
             settle_candidate={settle_ticker:?}"
        ),
    );

    for (name, q, note) in [
        (
            "markets__status_closed",
            "?status=closed&limit=5",
            "checklist #21",
        ),
        (
            "markets__status_settled",
            "?status=settled&limit=5",
            "checklist #21",
        ),
        (
            "markets__limit_over_max",
            "?limit=1001",
            "checklist #18: over-max limit — 400 or clamp?",
        ),
        (
            "markets__garbage_cursor",
            "?limit=5&cursor=garbage-cursor-fixture",
            "checklist #17: garbage cursor error body",
        ),
        ("markets__page1", "?limit=5", "checklist #17: cursor walk"),
    ] {
        let _ = r
            .capture(
                name,
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/markets{q}"),
                None,
                AuthMode::Signed,
                note,
            )
            .await;
    }
    // Cursor walk pages 2-3 from page1's cursor.
    let mut cursor: Option<String> = None;
    if let Ok(p1) = std::fs::read_to_string(r.out.join("markets__page1.json")) {
        cursor = serde_json::from_str::<Value>(&p1)
            .ok()
            .and_then(|j| jstr(&j, "cursor").map(str::to_string));
    }
    for page in ["markets__page2", "markets__page3"] {
        let Some(c) = cursor.filter(|c| !c.is_empty()) else {
            break;
        };
        let cap = r
            .capture(
                page,
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/markets?limit=5&cursor={c}"),
                None,
                AuthMode::Signed,
                "checklist #17: cursor walk",
            )
            .await;
        cursor = cap
            .ok()
            .and_then(|c| c.json)
            .and_then(|j| jstr(&j, "cursor").map(str::to_string));
    }
    let _ = r
        .capture(
            "markets__single_filter_lastpage",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/markets?tickers={ticker}"),
            None,
            AuthMode::Signed,
            "checklist #17: single-result page — absent vs empty cursor on last page",
        )
        .await;
    let _ = r
        .capture(
            "markets__unauth_single",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/markets/{ticker}"),
            None,
            AuthMode::Unauthenticated,
            "checklist #5: unauthenticated GET /markets/{ticker}",
        )
        .await;
    let _ = r
        .capture(
            "markets__single",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/markets/{ticker}"),
            None,
            AuthMode::Signed,
            "market object for the traded ticker (status/fee/structure fields)",
        )
        .await;

    // ============ orderbook / series / account / exchange (#16, #20, #22, #27) ============
    let _ = r
        .capture(
            "orderbook__base",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/markets/{ticker}/orderbook"),
            None,
            AuthMode::Signed,
            "checklist #20: REST orderbook — no_dollars leg pricing vs WS yes-scale",
        )
        .await;
    if let Some(ref st) = series_ticker {
        let _ = r
            .capture(
                "series__base",
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/series/{st}"),
                None,
                AuthMode::Signed,
                "checklist #22: fee_type/fee_multiplier for the traded series",
            )
            .await;
    }
    for (name, path, note) in [
        (
            "series__fee_changes",
            "/series/fee_changes",
            "scheduled fee changes surface",
        ),
        (
            "account__endpoint_costs",
            "/account/endpoint_costs",
            "checklist #16: actual token costs (checklist names this path)",
        ),
        (
            "account__limits",
            "/account/limits",
            "checklist #16: actual token costs (docs index names this path)",
        ),
        (
            "exchange__status",
            "/exchange/status",
            "checklist #27: current exchange status shape (maintenance window shape \
             still needs a real window)",
        ),
    ] {
        let _ = r
            .capture(
                name,
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}{path}"),
                None,
                AuthMode::Signed,
                note,
            )
            .await;
    }

    // ============ orders: maker, duplicate, cancel family (#6, #7, #14, #15) ============
    let maker_client_id = uuid4();
    let maker_body = json!({
        "ticker": ticker,
        "client_order_id": maker_client_id,
        "side": "bid",
        "count": "1.00",
        "price": "0.0100",
        "time_in_force": "good_till_canceled",
        "self_trade_prevention_type": "taker_at_cross",
        "post_only": false,
    });
    let (maker, maker_id) = r
        .place_v2(
            "orders__create_v2_maker",
            maker_body.clone(),
            "checklist #6: maker resting at 1c (far from touch)",
        )
        .await?;
    if maker.status == 201 {
        let _ = r
            .place_v2(
                "orders__duplicate_client_order_id",
                maker_body.clone(),
                "checklist #7: exact resubmission — capture the 409 code string",
            )
            .await;
    }
    if let Some(oid) = maker_id.clone() {
        let _ = r
            .capture(
                "orders__get_after_create",
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/portfolio/orders/{oid}"),
                None,
                AuthMode::Signed,
                "checklist #15: reconcile surface after create",
            )
            .await;
        let _ = r
            .cancel_v2(
                "orders__cancel_v2",
                &oid,
                "checklist #14: happy-path V2 cancel",
            )
            .await;
        let _ = r
            .capture(
                "orders__get_after_cancel",
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/portfolio/orders/{oid}"),
                None,
                AuthMode::Signed,
                "checklist #15: reconcile after cancel (changelog wrong-order caveat)",
            )
            .await;
        let _ = r
            .cancel_v2(
                "orders__cancel_already_canceled",
                &oid,
                "checklist #14: cancel of already-canceled order",
            )
            .await;
        let _ = r
            .place_v2(
                "orders__reuse_canceled_client_id",
                maker_body,
                "checklist #7: does a CANCELED order's client_order_id free up?",
            )
            .await;
    }
    let _ = r
        .cancel_v2(
            "orders__cancel_unknown_id",
            "00000000-0000-0000-0000-000000000000",
            "checklist #14: cancel unknown order id — 404 vs 200-with-zero",
        )
        .await;

    // ============ taker IOC fill -> fills/positions (#6, #19, #22) ============
    let (ioc, ioc_id) = r
        .place_v2(
            "orders__create_v2_taker_ioc",
            json!({
                "ticker": ticker,
                "client_order_id": uuid4(),
                "side": "bid",
                "count": "1.00",
                "price": "0.9900",
                "time_in_force": "immediate_or_cancel",
                "self_trade_prevention_type": "taker_at_cross",
            }),
            "checklist #6: IOC crossing — average_fill_price/average_fee_paid presence; \
             #22 fee math vs series fee fields",
        )
        .await?;
    if ioc.status == 201 {
        if let Some(oid) = ioc_id {
            let _ = r
                .cancel_v2(
                    "orders__cancel_executed",
                    &oid,
                    "checklist #14: cancel of an executed (IOC-terminal) order",
                )
                .await;
        }
    }
    let _ = r
        .capture(
            "fills__after_taker",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/portfolio/fills?limit=10"),
            None,
            AuthMode::Signed,
            "checklist #19: fill fee_cost dollars-string typing; #17 fills cursor",
        )
        .await;
    let _ = r
        .capture(
            "portfolio__positions",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/portfolio/positions"),
            None,
            AuthMode::Signed,
            "positions shape with a real (demo) position",
        )
        .await;
    let _ = r
        .capture(
            "portfolio__orders_list",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/portfolio/orders?limit=10"),
            None,
            AuthMode::Signed,
            "checklist #17: orders pagination page shape",
        )
        .await;

    // ============ rejection shapes (#8, #9, #13) ============
    let huge_count = balance_cents / 99 + 10_000;
    let _ = r
        .place_v2(
            "orders__insufficient_balance",
            json!({
                "ticker": ticker,
                "client_order_id": uuid4(),
                "side": "bid",
                "count": format!("{huge_count}.00"),
                "price": "0.9900",
                "time_in_force": "good_till_canceled",
                "self_trade_prevention_type": "taker_at_cross",
                "post_only": false,
            }),
            "checklist #8: exceeds demo balance — exact code/message",
        )
        .await;
    let _ = r
        .place_v2(
            "orders__invalid_price_structure",
            json!({
                "ticker": ticker,
                "client_order_id": uuid4(),
                "side": "bid",
                "count": "1.00",
                "price": "0.5150",
                "time_in_force": "good_till_canceled",
                "self_trade_prevention_type": "taker_at_cross",
                "post_only": true,
            }),
            "checklist #9: sub-cent price on this market's price_level_structure \
             (see markets__single fixture for the structure value)",
        )
        .await;
    let (numeric, numeric_id) = r
        .place_v2(
            "orders__numeric_field_types",
            json!({
                "ticker": ticker,
                "client_order_id": uuid4(),
                "side": "bid",
                "count": 1,
                "price": 0.01,
                "time_in_force": "good_till_canceled",
                "self_trade_prevention_type": "taker_at_cross",
            }),
            "checklist #13: count/price as JSON numbers instead of strings",
        )
        .await?;
    if numeric.status == 201 {
        if let Some(oid) = numeric_id {
            let _ = r
                .cancel_v2("cleanup__numeric_order", &oid, "cleanup of #13 probe")
                .await;
        }
    }

    // ============ post-only cross (#10) ============
    let (po, po_id) = r
        .place_v2(
            "orders__post_only_cross",
            json!({
                "ticker": ticker,
                "client_order_id": uuid4(),
                "side": "bid",
                "count": "1.00",
                "price": "0.9900",
                "time_in_force": "good_till_canceled",
                "self_trade_prevention_type": "taker_at_cross",
                "post_only": true,
            }),
            "checklist #10: post_only crossing the book — expect cancel + \
             last_update_reason=PostOnlyCrossCancel",
        )
        .await?;
    if let Some(oid) = po_id {
        let _ = r
            .capture(
                "orders__get_post_only",
                get.clone(),
                REST_HOST,
                &format!("{API_ROOT}/portfolio/orders/{oid}"),
                None,
                AuthMode::Signed,
                "checklist #10: last_update_reason on the post-only order",
            )
            .await;
        if po.status == 201 {
            let _ = r
                .cancel_v2("cleanup__post_only", &oid, "cleanup if it rested")
                .await;
        }
    }

    // ============ self-trade prevention (#11) ============
    if best_bid_cents > 0 && best_bid_cents < 97 {
        let stp_price = dollars(best_bid_cents + 1);
        let (_stp_rest, stp_rest_id) = r
            .place_v2(
                "orders__stp_setup",
                json!({
                    "ticker": ticker,
                    "client_order_id": uuid4(),
                    "side": "bid",
                    "count": "1.00",
                    "price": stp_price,
                    "time_in_force": "good_till_canceled",
                    "self_trade_prevention_type": "taker_at_cross",
                    "post_only": true,
                }),
                "checklist #11: resting bid one tick above best bid (becomes best)",
            )
            .await?;
        let _ = r
            .place_v2(
                "orders__stp_self_cross",
                json!({
                    "ticker": ticker,
                    "client_order_id": uuid4(),
                    "side": "ask",
                    "count": "1.00",
                    "price": stp_price,
                    "time_in_force": "immediate_or_cancel",
                    "self_trade_prevention_type": "taker_at_cross",
                }),
                "checklist #11: ask crossing our own best bid under taker_at_cross",
            )
            .await;
        if let Some(oid) = stp_rest_id {
            let _ = r
                .capture(
                    "orders__stp_resting_after",
                    get.clone(),
                    REST_HOST,
                    &format!("{API_ROOT}/portfolio/orders/{oid}"),
                    None,
                    AuthMode::Signed,
                    "checklist #11: resting order state after the STP event",
                )
                .await;
            let _ = r
                .cancel_v2("cleanup__stp_resting", &oid, "cleanup of STP probe")
                .await;
        }
    } else {
        r.note(
            "orders__stp_setup",
            format!("SKIPPED: best bid {best_bid_cents}c unusable for the STP probe"),
        );
    }

    // ============ legacy order family (#12, #16) ============
    let legacy = r
        .capture(
            "orders__legacy_create",
            post.clone(),
            REST_HOST,
            &format!("{API_ROOT}/portfolio/orders"),
            Some(json!({
                "ticker": ticker,
                "client_order_id": uuid4(),
                "action": "buy",
                "side": "yes",
                "count": 1,
                "yes_price": 1,
            })),
            AuthMode::Signed,
            "checklist #12: legacy create with integer-cent fields (10x token cost; \
             recorded once, never used by the adapter)",
        )
        .await?;
    let legacy_id = legacy
        .json
        .as_ref()
        .and_then(|j| j.get("order").and_then(|o| o.get("order_id")))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if let Some(oid) = legacy_id {
        let _ = r
            .capture(
                "orders__legacy_cancel",
                reqwest::Method::DELETE,
                REST_HOST,
                &format!("{API_ROOT}/portfolio/orders/{oid}"),
                None,
                AuthMode::Signed,
                "checklist #16: legacy DELETE (token cost dispute: 2 vs 20)",
            )
            .await;
    }

    // ============ settlement seeding + settlements page (#19) ============
    if let Some(ref st) = settle_ticker {
        let _ = r
            .place_v2(
                "orders__settlement_seed",
                json!({
                    "ticker": st,
                    "client_order_id": uuid4(),
                    "side": "bid",
                    "count": "1.00",
                    "price": "0.9900",
                    "time_in_force": "immediate_or_cancel",
                    "self_trade_prevention_type": "taker_at_cross",
                }),
                "position in the soonest-closing two-sided market so a settlement \
                 record exists for a later poll",
            )
            .await;
    }
    let _ = r
        .capture(
            "settlements__page",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/portfolio/settlements?limit=10"),
            None,
            AuthMode::Signed,
            "checklist #19: settlement cent-int units (likely empty on a fresh \
             account — re-poll after the seeded market closes)",
        )
        .await;

    // ============ websocket capture, both use_yes_price states (#23-#25) ============
    for (name, use_yes) in [
        ("ws__orderbook_trade_yes", true),
        ("ws__orderbook_trade_noleg", false),
    ] {
        if let Err(e) = ws_capture(r, name, &ticker, use_yes).await {
            r.note(name, format!("FAILED: {e:#}"));
        }
    }

    // ============ cleanup: cancel anything still tracked as live ============
    let leftovers: Vec<String> = r
        .placed
        .iter()
        .filter(|(_, done)| !done)
        .map(|(id, _)| id.clone())
        .collect();
    for (i, oid) in leftovers.iter().enumerate() {
        let _ = r
            .cancel_v2(
                &format!("cleanup__leftover_{i}"),
                oid,
                "end-of-session cancel of any order still tracked live \
                 (terminal-state cancels are no-ops and themselves useful fixtures)",
            )
            .await;
    }
    let _ = r
        .capture(
            "settlements__end_of_session",
            get.clone(),
            REST_HOST,
            &format!("{API_ROOT}/portfolio/settlements?limit=10"),
            None,
            AuthMode::Signed,
            "re-poll; if still empty, re-run after the seeded market closes: \
             see orders__settlement_seed.meta.json for the market",
        )
        .await;

    // ============ session manifest ============
    let manifest = json!({
        "recorded_at_epoch_ms": now_ms(),
        "environment": "demo",
        "tool": "examples/record_kalshi_fixtures.rs",
        "traded_ticker": ticker,
        "settlement_seed_ticker": settle_ticker,
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

/// One authenticated WS session: subscribe orderbook_delta+trade on `ticker`,
/// capture every text frame VERBATIM (one per line, replayable straight into
/// `KalshiWsParser`) for WS_CAPTURE_SECS; pings observed are counted in meta.
async fn ws_capture(r: &mut Recorder, name: &str, ticker: &str, use_yes_price: bool) -> Result<()> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;

    let url = format!("{WS_HOST}{WS_PATH}");
    let mut req = url
        .clone()
        .into_client_request()
        .context("building ws request")?;
    let ts = now_ms();
    let h = r.signer.sign("GET", WS_PATH, ts)?;
    for (k, v) in h.as_header_pairs() {
        req.headers_mut().insert(
            k,
            v.parse()
                .map_err(|_| anyhow::anyhow!("header value unparseable"))?,
        );
    }
    let (mut ws, resp) = tokio_tungstenite::connect_async(req)
        .await
        .context("ws connect")?;
    let http_status = resp.status().as_u16();

    // The crate's builder pins use_yes_price=true; the false-state session
    // (checklist #24) sends the same command shape with the flag flipped.
    let cmd = if use_yes_price {
        subscribe_orderbook_cmd(1, &[ticker])
    } else {
        json!({
            "id": 1,
            "cmd": "subscribe",
            "params": {
                "channels": ["orderbook_delta", "trade"],
                "market_tickers": [ticker],
                "use_yes_price": false,
            }
        })
    };
    ws.send(Message::text(cmd.to_string()))
        .await
        .context("ws subscribe send")?;

    let mut lines: Vec<String> = Vec::new();
    let mut pings = 0u32;
    let mut ping_payload: Option<String> = None;
    let deadline = Instant::now() + Duration::from_secs(WS_CAPTURE_SECS);
    while Instant::now() < deadline && lines.len() < WS_MAX_FRAMES {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, ws.next()).await {
            Err(_) => break,
            Ok(None) => break,
            Ok(Some(frame)) => match frame.context("ws frame")? {
                Message::Text(t) => lines.push(t.as_str().to_string()),
                Message::Ping(p) => {
                    pings += 1;
                    ping_payload.get_or_insert_with(|| String::from_utf8_lossy(&p).into_owned());
                }
                Message::Close(c) => {
                    lines.push(format!(
                        "{{\"__close_frame\": {:?}}}",
                        c.map(|c| c.to_string())
                    ));
                    break;
                }
                _ => {}
            },
        }
    }
    let _ = ws.close(None).await;

    let body = lines.join("\n");
    let meta = json!({
        "recorded_at_epoch_ms": ts,
        "environment": "demo",
        "host": WS_HOST,
        "path": WS_PATH,
        "http_status": http_status,
        "subscribe_cmd": cmd,
        "frames_captured": lines.len(),
        "pings_observed": pings,
        "first_ping_payload": ping_payload,
        "duration_secs": WS_CAPTURE_SECS,
        "format": "one verbatim text frame per line (.jsonl) — feed lines to KalshiWsParser",
        "note": "checklist #23-#25: signed handshake, subscribed/snapshot/delta/trade capture",
    });
    let f = r.out.join(format!("{name}.jsonl"));
    std::fs::write(&f, body).with_context(|| format!("writing {}", f.display()))?;
    let m = r.out.join(format!("{name}.meta.json"));
    std::fs::write(m, serde_json::to_string_pretty(&meta)?)?;
    r.note(
        name,
        format!("ws 101={http_status} frames={} pings={pings}", lines.len()),
    );
    Ok(())
}
