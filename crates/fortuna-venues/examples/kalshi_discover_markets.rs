//! READ-ONLY Kalshi DEMO market discovery (F7 venue seam). Finds the live
//! temperature / daily-high series so the Aeolus weather match can be GROUNDED
//! in a recorded fixture (never a fabricated ticker). DEMO ONLY, READ ONLY:
//! the only calls are `GET /series` (by category) and a paginated
//! `GET /markets?status=open` — no orders, no writes. Credentials are demo-only
//! (env, never printed).
//!
//! Run (creds in the main checkout's gitignored .env):
//!   set -a; . /Users/xavierbriggs/fortuna/.env; set +a; \
//!     cargo run -p fortuna-venues --example kalshi_discover_markets
//!
//! Output is market metadata only (ticker / title / series / category); it is
//! the raw material for a recorded fixture, captured by hand after review.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use fortuna_core::clock::RealClock;
use fortuna_venues::kalshi::auth::KalshiSigner;
use fortuna_venues::kalshi::client::{
    KalshiTransport, ReqwestKalshiTransport, KALSHI_DEMO_BASE_URL,
};

/// Substrings that flag a weather / temperature market (case-insensitive), in
/// the ticker or the title — the discovery filter (matching, not fabrication).
const TEMP_MARKS: [&str; 6] = ["high", "temp", "degree", "weather", "climate", "°"];

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let key_id = std::env::var("KALSHI_API_DEMO_KEY_ID")
        .context("KALSHI_API_DEMO_KEY_ID not set (demo credentials required; see the main .env)")?;
    let key_path = std::env::var("KALSHI_DEMO_PRIVATE_KEY_PATH")
        .context("KALSHI_DEMO_PRIVATE_KEY_PATH not set")?;
    if !KALSHI_DEMO_BASE_URL.contains("demo") {
        bail!("refusing to run: base url is not the demo host");
    }
    let pem = std::fs::read_to_string(&key_path)
        .with_context(|| format!("reading demo private key at {key_path}"))?;
    let signer = KalshiSigner::new(&pem, key_id).context("building demo signer")?;
    drop(pem);
    let rest = ReqwestKalshiTransport::new(
        KALSHI_DEMO_BASE_URL,
        signer,
        Arc::new(RealClock),
        Duration::from_secs(15),
    )
    .context("building demo REST transport")?;

    println!("Kalshi DEMO market discovery (READ-ONLY) — {KALSHI_DEMO_BASE_URL}\n");

    // --- 1) series listing, weather-ish categories first ---
    println!("== GET /series (by category) ==");
    for cat in ["Climate", "Weather", "Climate and Weather"] {
        let q = format!("category={}", cat.replace(' ', "%20"));
        match rest.request("GET", "/series", Some(&q), None).await {
            Ok((200, body)) => {
                let series = body.get("series").and_then(|s| s.as_array());
                let n = series.map(|a| a.len()).unwrap_or(0);
                println!("  category={cat:?}: {n} series");
                if let Some(arr) = series {
                    for s in arr.iter().take(25) {
                        println!(
                            "    {} | {} | category={}",
                            s.get("ticker").and_then(|v| v.as_str()).unwrap_or("?"),
                            s.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                            s.get("category").and_then(|v| v.as_str()).unwrap_or("")
                        );
                    }
                }
            }
            Ok((status, _)) => println!("  category={cat:?}: HTTP {status}"),
            Err(e) => println!("  category={cat:?}: error {e:?}"),
        }
    }

    // --- 2) paginate open markets; tally series prefixes + flag temp markets ---
    println!("\n== GET /markets?status=open (paginated) ==");
    let mut cursor = String::new();
    let mut total = 0usize;
    let mut by_series: BTreeMap<String, usize> = BTreeMap::new();
    let mut temp_hits: Vec<(String, String)> = Vec::new();
    for page in 0..40 {
        let mut q = "status=open&limit=1000".to_string();
        if !cursor.is_empty() {
            q.push_str(&format!("&cursor={cursor}"));
        }
        let (status, body) = rest
            .request("GET", "/markets", Some(&q), None)
            .await
            .map_err(|e| anyhow::anyhow!("GET /markets page {page}: {e:?}"))?;
        if status != 200 {
            bail!("GET /markets returned HTTP {status} on page {page}");
        }
        let markets = match body.get("markets").and_then(|m| m.as_array()) {
            Some(m) => m,
            None => break,
        };
        for m in markets {
            total += 1;
            let ticker = m.get("ticker").and_then(|v| v.as_str()).unwrap_or("");
            let title = m
                .get("title")
                .and_then(|v| v.as_str())
                .or_else(|| m.get("yes_sub_title").and_then(|v| v.as_str()))
                .unwrap_or("");
            // series prefix = ticker up to the first '-'
            let series = ticker.split('-').next().unwrap_or(ticker).to_string();
            *by_series.entry(series).or_default() += 1;
            let hay = format!("{ticker} {title}").to_ascii_lowercase();
            if TEMP_MARKS.iter().any(|m| hay.contains(m)) {
                temp_hits.push((ticker.to_string(), title.to_string()));
            }
        }
        cursor = body
            .get("cursor")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        if cursor.is_empty() {
            break;
        }
    }

    println!("  total open markets scanned: {total}");
    println!("  distinct series prefixes ({}):", by_series.len());
    for (s, n) in by_series.iter() {
        println!("    {s}  ({n})");
    }
    println!("\n  TEMPERATURE/WEATHER hits ({}):", temp_hits.len());
    if temp_hits.is_empty() {
        println!("    (NONE on demo right now — F7 venue grounding needs prod read-only or a later capture)");
    } else {
        for (t, title) in temp_hits.iter().take(60) {
            println!("    {t} | {title}");
        }
    }

    // --- 3) optional verbatim capture for a recorded fixture (read-only) ---
    // KALSHI_CAPTURE_SERIES=KXHIGHNY KALSHI_CAPTURE_OUT=fixtures/kalshi/markets__high_temp.json
    if let (Ok(series), Ok(out)) = (
        std::env::var("KALSHI_CAPTURE_SERIES"),
        std::env::var("KALSHI_CAPTURE_OUT"),
    ) {
        // No status filter: capture ALL statuses (incl. settled) so a recorded
        // forecast can align to the markets that existed for its target_date.
        let q = format!("series_ticker={series}&limit=1000");
        let (status, body) = rest
            .request("GET", "/markets", Some(&q), None)
            .await
            .map_err(|e| anyhow::anyhow!("capture GET /markets {series}: {e:?}"))?;
        if status != 200 {
            bail!("capture GET /markets {series} returned HTTP {status}");
        }
        let n = body
            .get("markets")
            .and_then(|m| m.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let pretty = serde_json::to_string_pretty(&body).context("serialize capture")?;
        std::fs::write(&out, pretty).with_context(|| format!("writing {out}"))?;
        println!(
            "\n  CAPTURED {n} market(s) for series {series} -> {out} (verbatim, market-data only)"
        );
    }
    Ok(())
}
