//! fortuna-recorder — the B0 perishable-data capture loop (operator
//! amendment A, confirmed 2026-06-11: "FIRST and standalone — it ships and
//! runs even if the rest slips").
//!
//! Captures, on a fixed cadence, from PUBLIC unauthenticated endpoints
//! (verified public in the archived perps OpenAPI spec and by the Phase A
//! live captures; no credentials are read, no orders are possible):
//!   - /margin/markets                      (active set, marks, OI)
//!   - /margin/markets/{t}/orderbook        (one per active perp, verbatim)
//!   - /margin/funding_rates/estimate       (intraday funding estimates)
//!   - /margin/risk_parameters              (hourly; changes rarely)
//!   - event API /markets?series_ticker=…   (bracket quotes, e.g. KXBTC15M)
//!
//! Every row carries the same cycle_id so perp books and bracket quotes
//! pair by capture sweep. Bodies are stored VERBATIM; top-of-book numbers
//! are derived companions (fortuna_recorder::top_of_book). Output:
//! <out>/<YYYY-MM-DD>/<stream>.jsonl, one JSON row per line.
//!
//! This is an IO-edge TOOL like the fixture recorder: it signs nothing,
//! uses wall-clock time (live capture timestamps ARE the data), and treats
//! every fetch failure as a recorded row (status 0 + error text) — a gap
//! in perishable data is itself information.
//!
//! Run: cargo run -p fortuna-recorder -- [--once] [--interval-secs 30]
//!      [--out-dir data/perishable] [--bracket-series KXBTC15M]

use anyhow::{Context, Result};
use fortuna_recorder::{capture_row, day_dir, top_of_book};
use serde_json::Value;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PERPS_HOST: &str = "https://api.elections.kalshi.com";
const API_ROOT: &str = "/trade-api/v2";
const LEG_PACE: Duration = Duration::from_millis(150);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct Config {
    once: bool,
    interval: Duration,
    out_dir: String,
    bracket_series: Vec<String>,
}

fn parse_args() -> Result<Config> {
    let mut cfg = Config {
        once: false,
        interval: Duration::from_secs(30),
        out_dir: "data/perishable".to_string(),
        bracket_series: vec!["KXBTC15M".to_string()],
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--once" => cfg.once = true,
            "--interval-secs" => {
                let v = args.next().context("--interval-secs needs a value")?;
                cfg.interval = Duration::from_secs(v.parse().context("interval not a number")?);
            }
            "--out-dir" => cfg.out_dir = args.next().context("--out-dir needs a value")?,
            "--bracket-series" => {
                let v = args.next().context("--bracket-series needs a value")?;
                cfg.bracket_series = v.split(',').map(|s| s.trim().to_string()).collect();
            }
            other => anyhow::bail!("unknown flag {other}"),
        }
    }
    Ok(cfg)
}

struct Recorder {
    http: reqwest::Client,
    out_dir: String,
}

impl Recorder {
    fn append(&self, stream: &str, row: &Value) -> Result<()> {
        let at = row
            .get("captured_at_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let dir = Path::new(&self.out_dir).join(day_dir(at));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{stream}.jsonl"));
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let line = serde_json::to_string(row)?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    /// Fetch one URL and persist the row. Failures become rows (status 0),
    /// never aborts — a recording gap is data. Returns the body on 200.
    async fn capture(
        &self,
        cycle_id: u64,
        stream: &str,
        key: &str,
        url: &str,
        derive_book: bool,
    ) -> Option<String> {
        tokio::time::sleep(LEG_PACE).await;
        let at = now_ms();
        let (status, body) = match self.http.get(url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                match resp.text().await {
                    Ok(t) => (status, t),
                    Err(e) => (0, format!("body read error: {e}")),
                }
            }
            Err(e) => (0, format!("request error: {e}")),
        };
        let derived = if derive_book && status == 200 {
            top_of_book(&body).map(|t| {
                serde_json::json!({
                    "best_bid_tenthousandths": t.best_bid_tenthousandths,
                    "best_ask_tenthousandths": t.best_ask_tenthousandths,
                    "spread_tenthousandths": t.spread_tenthousandths,
                })
            })
        } else {
            None
        };
        let row = capture_row(cycle_id, at, stream, key, status, &body, derived);
        if let Err(e) = self.append(stream, &row) {
            eprintln!("[recorder] WRITE FAILURE {stream}/{key}: {e:#}");
        }
        (status == 200).then_some(body)
    }
}

fn active_perp_tickers(markets_body: &str) -> Vec<String> {
    let v: Value = match serde_json::from_str(markets_body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("markets")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|m| m.get("status").and_then(|s| s.as_str()) == Some("active"))
                .filter_map(|m| m.get("ticker").and_then(|t| t.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cfg = parse_args()?;
    let rec = Recorder {
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .context("building http client")?,
        out_dir: cfg.out_dir.clone(),
    };
    println!(
        "fortuna-recorder (B0): host={PERPS_HOST} out={} interval={:?} series={:?} once={}",
        cfg.out_dir, cfg.interval, cfg.bracket_series, cfg.once
    );

    let mut cycle_id: u64 = now_ms() as u64; // unique across restarts
    loop {
        let cycle_started = std::time::Instant::now();

        // 1. Perps market set (marks, OI; source of the active-ticker list).
        let markets = rec
            .capture(
                cycle_id,
                "perp_markets",
                "all",
                &format!("{PERPS_HOST}{API_ROOT}/margin/markets"),
                false,
            )
            .await;

        // 2. Orderbook per active perp, verbatim + derived top-of-book.
        let tickers = markets
            .as_deref()
            .map(active_perp_tickers)
            .unwrap_or_default();
        for t in &tickers {
            rec.capture(
                cycle_id,
                "perp_orderbook",
                t,
                &format!("{PERPS_HOST}{API_ROOT}/margin/markets/{t}/orderbook"),
                true,
            )
            .await;
        }

        // 3. Intraday funding estimate + mark price, PER ticker (the
        //    endpoint requires ?ticker — probed live 2026-06-11; the
        //    response also carries mark_price and next_funding_time).
        for t in &tickers {
            rec.capture(
                cycle_id,
                "funding_estimate",
                t,
                &format!("{PERPS_HOST}{API_ROOT}/margin/funding_rates/estimate?ticker={t}"),
                false,
            )
            .await;
        }

        // 4. Bracket quotes for each configured series (event API, public;
        //    yes_bid/yes_ask top-of-book rides in the market objects).
        //    min_close_ts=now keeps the live window + pre-created future
        //    windows and drops finalized noise; NO status filter because
        //    upcoming windows sit `initialized` (probed live 2026-06-11).
        //    Overnight-ET hours can legitimately have no live window —
        //    that gap is itself basis-strategy data.
        let now_s = now_ms() / 1000;
        for s in &cfg.bracket_series {
            rec.capture(
                cycle_id,
                "bracket_quotes",
                s,
                &format!(
                    "{PERPS_HOST}{API_ROOT}/markets?series_ticker={s}&min_close_ts={now_s}&limit=200"
                ),
                false,
            )
            .await;
        }

        // 5. Risk parameters: hourly-ish (rarely change, big body).
        if cycle_id.is_multiple_of(120) || cfg.once {
            rec.capture(
                cycle_id,
                "risk_parameters",
                "all",
                &format!("{PERPS_HOST}{API_ROOT}/margin/risk_parameters"),
                false,
            )
            .await;
        }

        println!(
            "[recorder] cycle {cycle_id}: {} perp books + {} series in {:?}",
            tickers.len(),
            cfg.bracket_series.len(),
            cycle_started.elapsed()
        );
        if cfg.once {
            return Ok(());
        }
        cycle_id = cycle_id.wrapping_add(1);
        tokio::time::sleep(cfg.interval.saturating_sub(cycle_started.elapsed())).await;
    }
}
