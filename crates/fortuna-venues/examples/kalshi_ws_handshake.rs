//! Live Kalshi WS handshake exercise (T4.2 item 2(i), the operator-run first-live
//! seam). DEMO ONLY, READ ONLY. Drives the REAL production transport
//! ([`KalshiWsTransport`]) and session pump ([`pump_session`]) against the live
//! Kalshi DEMO websocket: a signed `connect_async` handshake, an `orderbook_delta`
//! subscribe to a currently-open demo market, and a bounded read of streamed book
//! frames — then a clean disconnect. It places NO orders and performs NO writes;
//! the only REST call is a read-only `GET /markets` to pick an open ticker.
//!
//! Credentials are DEMO-only (refuse loudly if absent) and are NEVER printed:
//!   KALSHI_API_DEMO_KEY_ID         — demo API key id
//!   KALSHI_DEMO_PRIVATE_KEY_PATH   — path to the demo RSA private key (PEM)
//!   KALSHI_WS_SECS (optional)      — read window in seconds (default 12)
//!
//! Run (creds live in the main checkout's gitignored .env):
//!   set -a; . /Users/xavierbriggs/fortuna/.env; set +a; \
//!     cargo run -p fortuna-venues --example kalshi_ws_handshake
//!
//! The endpoints are HARD-CODED to demo ([`KALSHI_DEMO_BASE_URL`] /
//! [`KALSHI_WS_DEMO_URL`]); this binary cannot target production.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use fortuna_core::clock::RealClock;
use fortuna_venues::kalshi::auth::KalshiSigner;
use fortuna_venues::kalshi::client::{
    KalshiTransport, ReqwestKalshiTransport, KALSHI_DEMO_BASE_URL,
};
use fortuna_venues::kalshi::dial::{pump_session, TokioSleeper, WsTransport};
use fortuna_venues::kalshi::ws::KalshiWsEvent;
use fortuna_venues::kalshi::ws_transport::{KalshiWsTransport, KALSHI_WS_DEMO_URL};
use fortuna_venues::stream::StreamEvent;

/// How many open demo markets to subscribe to (small — this is a connectivity
/// proof, not a load test).
const MAX_TICKERS: usize = 3;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // ---- demo credentials ONLY; refuse loudly if absent (never printed) ----
    let key_id = std::env::var("KALSHI_API_DEMO_KEY_ID")
        .context("KALSHI_API_DEMO_KEY_ID not set (demo credentials required; see the .env in the main checkout)")?;
    let key_path = std::env::var("KALSHI_DEMO_PRIVATE_KEY_PATH")
        .context("KALSHI_DEMO_PRIVATE_KEY_PATH not set (demo credentials required)")?;
    let read_secs: u64 = std::env::var("KALSHI_WS_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);

    // Defensive: this binary is demo-only. Refuse if the compiled endpoints are
    // not the demo hosts (guards against a future edit pointing at prod).
    if !KALSHI_DEMO_BASE_URL.contains("demo") || !KALSHI_WS_DEMO_URL.contains("demo") {
        bail!("refusing to run: endpoints are not the demo hosts");
    }

    let pem = std::fs::read_to_string(&key_path)
        .with_context(|| format!("reading demo private key at {key_path}"))?;
    // Two signers from one PEM read (REST transport + WS transport each own one);
    // the PEM is dropped immediately after.
    let signer_rest = KalshiSigner::new(&pem, key_id.clone()).context("building REST signer")?;
    let signer_ws = KalshiSigner::new(&pem, key_id).context("building WS signer")?;
    drop(pem);

    let clock = Arc::new(RealClock);
    println!("Kalshi DEMO WS handshake exercise (READ-ONLY, no orders)");
    println!("  REST: {KALSHI_DEMO_BASE_URL}");
    println!("  WS:   {KALSHI_WS_DEMO_URL}");

    // ---- 1) read-only REST: pick currently-open demo markets to subscribe to ----
    let rest = ReqwestKalshiTransport::new(
        KALSHI_DEMO_BASE_URL,
        signer_rest,
        clock.clone(),
        Duration::from_secs(10),
    )
    .context("building demo REST transport")?;
    let tickers = open_demo_tickers(&rest).await;
    match &tickers {
        Ok(t) if !t.is_empty() => println!("  open demo markets to subscribe: {t:?}"),
        Ok(_) => println!("  (no open demo markets found via GET /markets — handshake only)"),
        Err(e) => println!("  (GET /markets failed: {e}; handshake only)"),
    }
    let tickers: Vec<String> = tickers.unwrap_or_default();
    let ticker_refs: Vec<&str> = tickers.iter().map(String::as_str).collect();

    // ---- 2) the SIGNED WS HANDSHAKE (the live seam) ----
    let ws = KalshiWsTransport::new(
        signer_ws,
        KALSHI_WS_DEMO_URL.to_string(),
        clock,
        Arc::new(TokioSleeper),
    );
    print!("  WS signed handshake (connect_async)... ");
    let mut conn = match ws.connect().await {
        Ok(conn) => {
            println!("OK — 101 upgrade, authenticated");
            conn
        }
        Err(cause) => {
            println!("FAILED: {cause:?}");
            bail!("WS handshake failed: {cause:?}");
        }
    };

    if ticker_refs.is_empty() {
        println!("  no tickers to subscribe; handshake proven, disconnecting.");
        return Ok(());
    }

    // ---- 3) subscribe + bounded read of live book frames via the REAL pump ----
    println!("  subscribing to orderbook_delta and reading for {read_secs}s...");
    let mut sub_id = 0u64;
    let mut tally = FrameTally::default();
    let pump = pump_session(conn.as_mut(), &ticker_refs, &mut sub_id, |ev| {
        tally.record(ev)
    });
    match tokio::time::timeout(Duration::from_secs(read_secs), pump).await {
        Ok(cause) => println!("  session ended early: {cause:?}"),
        Err(_) => println!("  read window elapsed; closing."),
    }
    drop(conn); // clean disconnect

    println!("\n  SUMMARY (demo, read-only):");
    println!(
        "    handshake OK | subscribed={} snapshots={} deltas={} seq_gaps={} errors={} other={}",
        tally.subscribed, tally.snapshots, tally.deltas, tally.seq_gaps, tally.errors, tally.other
    );
    if tally.snapshots == 0 && tally.deltas == 0 {
        println!("    NOTE: no book frames in the window (quiet demo market is normal).");
    }
    Ok(())
}

/// Redacted tally of streamed events — no payloads, just kinds/counts.
#[derive(Default)]
struct FrameTally {
    subscribed: u32,
    snapshots: u32,
    deltas: u32,
    seq_gaps: u32,
    errors: u32,
    other: u32,
}

impl FrameTally {
    fn record(&mut self, ev: KalshiWsEvent) {
        match ev {
            KalshiWsEvent::Subscribed { .. } => self.subscribed += 1,
            KalshiWsEvent::Stream(StreamEvent::BookSnapshot { .. }) => self.snapshots += 1,
            KalshiWsEvent::Stream(StreamEvent::BookDelta { .. }) => self.deltas += 1,
            KalshiWsEvent::SeqGap { .. } => self.seq_gaps += 1,
            KalshiWsEvent::Error { .. } => self.errors += 1,
            _ => self.other += 1,
        }
    }
}

/// Read-only `GET /markets?limit=...&status=open` on the demo host; return up to
/// [`MAX_TICKERS`] open market tickers. Best-effort: any non-200 or parse miss
/// yields an empty list (the caller falls back to a handshake-only run).
async fn open_demo_tickers(rest: &ReqwestKalshiTransport) -> Result<Vec<String>> {
    let (status, body) = rest
        .request("GET", "/markets", Some("limit=100&status=open"), None)
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    if !(200..300).contains(&status) {
        bail!("GET /markets returned HTTP {status}");
    }
    let tickers = body
        .get("markets")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("ticker").and_then(|t| t.as_str()))
                .take(MAX_TICKERS)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(tickers)
}
