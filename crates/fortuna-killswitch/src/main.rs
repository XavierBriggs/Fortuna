//! The standalone kill-switch BINARY (I4). Runs with everything else dead:
//! no Postgres, no main runtime, no Slack required.
//!
//! Usage:
//!   fortuna-killswitch freeze --journal <path> [--venue kalshi]
//!   fortuna-killswitch report --journal <path> [--venue kalshi]
//!   fortuna-killswitch self-test --journal <path>
//!
//! `self-test` exercises the full freeze machinery against an in-process
//! sim venue (the monthly-test path, spec I4); live venue adapters plug in
//! at T1.1 with their own credential set (env: FORTUNA_KILLSWITCH_*).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

use fortuna_core::clock::{Clock, RealClock, SimClock, UtcTimestamp};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::GatePipeline;
use fortuna_killswitch::{
    clear_revocation, freeze_cancel_and_report_positions, freeze_cancel_perp_and_flatten,
    load_gate_config, load_kalshi_creds, load_kinetics_creds, revocation_path, write_revocation,
};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::kalshi::client::{KalshiTransport, ReqwestKalshiTransport};
use fortuna_venues::kalshi::{KalshiSigner, KalshiVenue};
use fortuna_venues::kinetics::adapter::KineticsAdapter;
use fortuna_venues::kinetics::client::KineticsClient;
use fortuna_venues::sim::{FaultConfig, PlaceOrder, SimVenue};
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut journal: Option<PathBuf> = None;
    let mut venue_name = "kalshi".to_string();
    let mut command: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--journal" => {
                i += 1;
                journal = args.get(i).map(PathBuf::from);
            }
            "--venue" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    venue_name = v.clone();
                }
            }
            c if command.is_none() && !c.starts_with('-') => command = Some(c.to_string()),
            other => {
                eprintln!("unknown arg {other:?}");
                return usage();
            }
        }
        i += 1;
    }
    let Some(command) = command else {
        return usage();
    };
    let Some(journal) = journal else {
        eprintln!("--journal <path> is required (the switch's own flat-file state)");
        return usage();
    };

    match command.as_str() {
        "self-test" => self_test(&journal),
        // I4 RE-ARM PREREQUISITE (spec Section 8: kill-switch reversal is
        // CLI-only). Clear the durable kill sentinel so a subsequent runtime
        // RESTART boots un-revoked. Deliberately touches NO venue and reads NO
        // creds — it is the operator's out-of-band un-revoke, not a trade. The
        // halt the running daemon already applied is itself restart-gated (I2:
        // "no automatic resumption"), so order-placing capability returns only
        // after the operator both clears this AND restarts the daemon.
        "clear-revocation" => clear_revocation_cmd(&journal),
        // PERP FLATTEN (spec 5.15): cancel-all + reduce-only IOC closes on the
        // Kinetics perp venue, through the real perp gate (its OWN cred pair +
        // gate config, env-only, fail-closed). `--venue` is ignored: kinetics is
        // the only perp venue.
        "flatten-perps" => flatten_perps_kinetics(&journal),
        "freeze" => match venue_name.as_str() {
            "kalshi" => freeze_kalshi(&journal),
            other => {
                // Only kalshi is wired (built against recorded fixtures; never
                // invented). Failing LOUDLY beats pretending.
                eprintln!(
                    "no live freeze adapter for venue {other:?} (only `kalshi` is wired); \
                     run `fortuna-killswitch self-test --journal <path>` to exercise the \
                     machinery over the sim venue"
                );
                ExitCode::from(3)
            }
        },
        "report" => {
            // Report-only (open orders + positions WITHOUT cancelling) has no
            // library path yet: the switch's job is to STOP risk, and `freeze`
            // already reports positions after cancelling. A report-only verb is a
            // small future addition (ledgered GAPS).
            eprintln!(
                "`report` (positions without cancelling) is not wired; use `freeze` — \
                 the kill-switch default, which cancels every open order and then \
                 reports positions"
            );
            ExitCode::from(3)
        }
        other => {
            eprintln!("unknown command {other:?}");
            usage()
        }
    }
}

/// LIVE `freeze --venue kalshi` (I4): cancel every open order at Kalshi using the
/// switch's OWN credential pair (env-only, SEPARATE from the runtime), then report
/// positions. FAIL-CLOSED — incomplete credentials refuse before any venue call.
/// The async `ReqwestKalshiTransport` is driven on a SELF-SPUN current-thread
/// tokio runtime: a one-shot reactor for the HTTP cancels with NO dependence on
/// the daemon event loop / Postgres / cognition (I4 holds — tokio is not in the
/// i4 forbidden set, and is already transitive via fortuna-venues). The first
/// live exercise is operator-run after the (now-signed) 27-item paper clearance.
fn freeze_kalshi(journal: &Path) -> ExitCode {
    let creds = match load_kalshi_creds(
        env_nonempty("FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID"),
        env_nonempty("FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH"),
        env_nonempty("FORTUNA_KILLSWITCH_KALSHI_BASE_URL"),
    ) {
        Ok(c) => c,
        Err(reason) => {
            eprintln!("kill-switch kalshi freeze REFUSED (fail-closed): {reason}");
            return ExitCode::from(4);
        }
    };

    let signer = match KalshiSigner::new(&creds.private_key_pem, creds.api_key_id) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kalshi signer construction failed: {e}");
            return ExitCode::from(4);
        }
    };
    // Live signing needs real wall time (the venue validates timestamp freshness);
    // RealClock is the legal source out here in binary-land.
    let clock: Arc<dyn Clock> = Arc::new(RealClock);
    let transport = match ReqwestKalshiTransport::new(
        &creds.base_url,
        signer,
        Arc::clone(&clock),
        Duration::from_secs(10),
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("kalshi transport construction failed: {e}");
            return ExitCode::from(4);
        }
    };
    let venue_id = match VenueId::new("kalshi") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("venue id construction failed: {e}");
            return ExitCode::from(1);
        }
    };
    // Empty series: a freeze touches only open_orders + cancel (no market sync).
    let venue = match KalshiVenue::new(
        venue_id,
        Arc::new(transport) as Arc<dyn KalshiTransport>,
        Arc::clone(&clock),
        vec![],
    ) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("kalshi venue construction failed: {e}");
            return ExitCode::from(4);
        }
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("kill-switch runtime build failed: {e}");
            return ExitCode::from(1);
        }
    };
    let result = runtime.block_on(freeze_cancel_and_report_positions(
        &venue,
        clock.as_ref(),
        journal,
    ));
    match result {
        Ok(report) => {
            // I4 SECOND HALF: a kill REVOKES as well as cancels. Write the
            // durable kill sentinel (sibling of the journal) so the runtime
            // refuses FUTURE order placement until the operator clears it
            // out-of-band. A revocation-write failure is LOUD but does NOT undo
            // the (successful) cancels — surface it and exit non-zero so the
            // operator re-runs / clears manually rather than believing the
            // capability was revoked when it was not (fail-closed reporting).
            let rev_path = revocation_path(journal);
            if let Err(e) = write_revocation(&rev_path, clock.as_ref(), "freeze_and_cancel") {
                eprintln!(
                    "FREEZE cancelled orders but the I4 revocation sentinel WRITE FAILED \
                     ({}): {e} — order-placing capability is NOT revoked; write {} by hand \
                     or re-run the freeze",
                    rev_path.display(),
                    rev_path.display()
                );
                return ExitCode::from(6);
            }
            let _ = journal_revocation(journal, clock.as_ref(), "freeze_and_cancel");
            println!(
                "freeze OK (kalshi): cancelled {}/{} orders, {} failed; {} open positions reported; \
                 order-placing capability REVOKED ({}); journal at {}",
                report.orders_cancelled,
                report.orders_seen,
                report.orders_cancel_failed,
                report.positions_seen,
                rev_path.display(),
                journal.display()
            );
            if report.orders_cancel_failed > 0 {
                eprintln!(
                    "WARNING: {} order(s) could not be confirmed cancelled — reconcile \
                     manually (re-running the switch is always safe)",
                    report.orders_cancel_failed
                );
                return ExitCode::from(5);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FREEZE FAILED (kalshi): {e}");
            ExitCode::from(1)
        }
    }
}

/// LIVE `flatten-perps` (spec 5.15, T5.B8, I4): cancel every open Kinetics perp
/// order + close each non-flat position with a REDUCE-ONLY IOC that crosses the
/// live book — every close a SEALED `GatedPerpOrder` through the real perp gate.
/// FAIL-CLOSED: the gate config AND the switch's OWN kinetics credential pair
/// load + validate BEFORE any venue call; a miss refuses (exit 4). One-shot
/// current-thread runtime: NO daemon event loop / Postgres / cognition (I4).
fn flatten_perps_kinetics(journal: &Path) -> ExitCode {
    // 1. Gate config (env-only, fail-closed) -> the SEAL (GatePipeline).
    let gate_config = match load_gate_config(env_nonempty("FORTUNA_KILLSWITCH_GATE_CONFIG_PATH")) {
        Ok(c) => c,
        Err(reason) => {
            eprintln!("kill-switch flatten-perps REFUSED (fail-closed): {reason}");
            return ExitCode::from(4);
        }
    };
    let mut gates = match GatePipeline::new(gate_config) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("kill-switch gate pipeline construction failed: {e}");
            return ExitCode::from(4);
        }
    };

    // 2. The switch's OWN Kinetics cred pair (env-only, SEPARATE from the runtime).
    let creds = match load_kinetics_creds(
        env_nonempty("FORTUNA_KILLSWITCH_KINETICS_API_KEY_ID"),
        env_nonempty("FORTUNA_KILLSWITCH_KINETICS_PRIVATE_KEY_PATH"),
        env_nonempty("FORTUNA_KILLSWITCH_KINETICS_BASE_URL"),
    ) {
        Ok(c) => c,
        Err(reason) => {
            eprintln!("kill-switch flatten-perps REFUSED (fail-closed): {reason}");
            return ExitCode::from(4);
        }
    };
    // Slippage to cross the book (bps), default 50. A bad value falls back to the
    // default rather than failing — but if it exceeds the gate price-band, each
    // close self-rejects PriceSanity (counted, never a wrong fill).
    let slippage_bps = env_nonempty("FORTUNA_KILLSWITCH_KINETICS_SLIPPAGE_BPS")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50);

    let signer = match KalshiSigner::new(&creds.private_key_pem, creds.api_key_id) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kinetics signer construction failed: {e}");
            return ExitCode::from(4);
        }
    };
    // Live signing needs real wall time (RealClock is the legal source out here).
    let clock: Arc<dyn Clock> = Arc::new(RealClock);
    let transport = match ReqwestKalshiTransport::new(
        &creds.base_url,
        signer,
        Arc::clone(&clock),
        Duration::from_secs(10),
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("kinetics transport construction failed: {e}");
            return ExitCode::from(4);
        }
    };
    let venue_id = match VenueId::new("kinetics") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("venue id construction failed: {e}");
            return ExitCode::from(1);
        }
    };
    let adapter = KineticsAdapter::new(KineticsClient::new(
        Arc::new(transport) as Arc<dyn KalshiTransport>
    ));

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("kill-switch runtime build failed: {e}");
            return ExitCode::from(1);
        }
    };
    let result = runtime.block_on(freeze_cancel_perp_and_flatten(
        &adapter,
        &mut gates,
        &venue_id,
        slippage_bps,
        clock.as_ref(),
        journal,
    ));
    match result {
        Ok(report) => {
            // I4 SECOND HALF (mirrors the kalshi freeze): a kill REVOKES as well
            // as flattens. Write the durable kill sentinel so the runtime refuses
            // FUTURE order placement until the operator clears it out-of-band.
            let rev_path = revocation_path(journal);
            if let Err(e) = write_revocation(&rev_path, clock.as_ref(), "flatten_perps") {
                eprintln!(
                    "FLATTEN cancelled/closed perps but the I4 revocation sentinel WRITE FAILED \
                     ({}): {e} — order-placing capability is NOT revoked; write {} by hand \
                     or re-run the flatten",
                    rev_path.display(),
                    rev_path.display()
                );
                return ExitCode::from(6);
            }
            let _ = journal_revocation(journal, clock.as_ref(), "flatten_perps");
            println!(
                "flatten-perps OK (kinetics): cancelled {}/{} orders ({} failed); {} positions seen, \
                 {} closes placed, {} failed, {} skipped (no price), {} gate-rejected; \
                 order-placing capability REVOKED ({}); journal at {}",
                report.orders_cancelled,
                report.orders_seen,
                report.orders_cancel_failed,
                report.positions_seen,
                report.flatten_orders_placed,
                report.flatten_orders_failed,
                report.flatten_orders_skipped_no_price,
                report.flatten_orders_rejected_by_gate,
                rev_path.display(),
                journal.display()
            );
            // Exit 5 if ANYTHING was left un-resolved — a failed cancel OR a
            // position not confirmed flat (skipped / place-failed / gate-rejected)
            // — so the operator reconciles. Re-running is always safe (idempotent).
            if report.orders_cancel_failed > 0
                || report.flatten_orders_skipped_no_price > 0
                || report.flatten_orders_failed > 0
                || report.flatten_orders_rejected_by_gate > 0
            {
                eprintln!(
                    "WARNING: not every order/position was confirmed resolved — reconcile \
                     manually (re-running the switch is always safe)"
                );
                return ExitCode::from(5);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FLATTEN-PERPS FAILED (kinetics): {e}");
            ExitCode::from(1)
        }
    }
}

/// An env var, treated as ABSENT when unset or blank — empty env never counts as
/// a present credential (`load_kalshi_creds` is the durable fail-closed guard).
fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// `clear-revocation --journal <path>` (spec Section 8: kill-switch reversal is
/// CLI-only): remove the durable kill sentinel so a subsequent runtime RESTART
/// boots un-revoked. NO venue, NO creds — the operator's out-of-band un-revoke.
/// Idempotent (a missing sentinel is success). Journals + prints the action.
fn clear_revocation_cmd(journal: &Path) -> ExitCode {
    let rev_path = revocation_path(journal);
    match clear_revocation(&rev_path) {
        Ok(()) => {
            // RealClock is the legal time source out here in binary-land; the
            // journal append is best-effort (the clear itself already succeeded).
            let _ = journal_revocation_cleared(journal);
            println!(
                "revocation cleared: {} removed (idempotent). RESTART the daemon to re-arm \
                 order-placing capability (I2: no automatic resumption); journal at {}",
                rev_path.display(),
                journal.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!(
                "clear-revocation FAILED for {}: {e} — remove the file by hand to re-arm",
                rev_path.display()
            );
            ExitCode::from(1)
        }
    }
}

/// Append a `revocation_written` line to the switch's journal (best-effort: the
/// sentinel WRITE is the load-bearing act; this is the audit breadcrumb). Uses
/// `OpenOptions::append` directly — the lib's `journal_line` is private, and the
/// journal is the switch's own flat-file state main may append to.
fn journal_revocation(journal: &Path, clock: &dyn Clock, by: &str) -> std::io::Result<()> {
    append_journal_json(
        journal,
        &serde_json::json!({
            "event": "revocation_written",
            "by": by,
            "sentinel": revocation_path(journal).display().to_string(),
            "at": clock.now().to_iso8601(),
        }),
    )
}

/// Append a `revocation_cleared` line (best-effort breadcrumb; the file removal
/// is the load-bearing act).
fn journal_revocation_cleared(journal: &Path) -> std::io::Result<()> {
    append_journal_json(
        journal,
        &serde_json::json!({
            "event": "revocation_cleared",
            "sentinel": revocation_path(journal).display().to_string(),
            "at": RealClock.now().to_iso8601(),
        }),
    )
}

/// One JSON line appended + fsync'd to the journal (mirrors the lib's
/// `journal_line` IO style; serde_json::to_string on a built Value never fails,
/// so the only error is IO).
fn append_journal_json(journal: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal)?;
    let line = value.to_string();
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: fortuna-killswitch <freeze|flatten-perps|clear-revocation|report|self-test> --journal <path> [--venue kalshi]\n\
         \n\
         clear-revocation (spec Section 8): remove the durable I4 kill sentinel (KILLSWITCH_REVOKED,\n\
           sibling of --journal) so a daemon RESTART re-arms order-placing capability. No venue, no creds.\n\
         \n\
         flatten-perps (spec 5.15): cancel-all + reduce-only IOC closes on Kinetics. Requires (env-only, fail-closed):\n\
           FORTUNA_KILLSWITCH_GATE_CONFIG_PATH        (gate config TOML; validated before any venue call)\n\
           FORTUNA_KILLSWITCH_KINETICS_API_KEY_ID\n\
           FORTUNA_KILLSWITCH_KINETICS_PRIVATE_KEY_PATH\n\
           FORTUNA_KILLSWITCH_KINETICS_BASE_URL\n\
           FORTUNA_KILLSWITCH_KINETICS_SLIPPAGE_BPS   (optional; default 50)"
    );
    ExitCode::from(2)
}

/// The monthly test (I4): build a sim venue with live orders + positions,
/// freeze it, verify zero open orders remain, print the report.
fn self_test(journal: &std::path::Path) -> ExitCode {
    let start = match UtcTimestamp::from_epoch_millis(1_780_000_000_000) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("clock setup failed: {e}");
            return ExitCode::from(1);
        }
    };
    let clock = Arc::new(SimClock::new(start));
    let venue = match build_sim(clock.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("sim setup failed: {e}");
            return ExitCode::from(1);
        }
    };

    let result = futures::executor::block_on(freeze_cancel_and_report_positions(
        &venue,
        clock.as_ref(),
        journal,
    ));
    match result {
        Ok(report) => {
            let remaining = venue.resting_orders().len();
            if report.orders_cancelled != report.orders_seen || remaining != 0 {
                eprintln!(
                    "SELF-TEST FAILED: {} of {} cancelled, {remaining} remaining",
                    report.orders_cancelled, report.orders_seen
                );
                return ExitCode::from(1);
            }
            println!(
                "self-test OK: cancelled {}/{} orders, reported {} positions; journal at {}",
                report.orders_cancelled,
                report.orders_seen,
                report.positions_seen,
                journal.display()
            );
            // The wall clock exists out here in binary-land (RealClock is
            // the legal source); stamp the journal with a real-time marker.
            let _ = RealClock.now();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("SELF-TEST FAILED: {e}");
            ExitCode::from(1)
        }
    }
}

fn build_sim(clock: Arc<SimClock>) -> Result<SimVenue, Box<dyn std::error::Error>> {
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
        "#,
    )?;
    let venue = SimVenue::new(
        VenueId::new("sim")?,
        clock,
        ScheduleFeeModel::new(vec![s]).map_err(|e| std::io::Error::other(e.to_string()))?,
        FaultConfig::none(1),
        Cents::new(100_000),
    );
    let market = MarketId::new("KS-TEST")?;
    venue.add_market(Market {
        id: market.clone(),
        venue: VenueId::new("sim")?,
        title: "kill-switch self-test market".into(),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    });
    venue.set_book(
        &market,
        vec![PriceLevel {
            price: Cents::new(45),
            qty: Contracts::new(50),
        }],
        vec![PriceLevel {
            price: Cents::new(55),
            qty: Contracts::new(50),
        }],
    )?;
    // Live orders + a position for the freeze to deal with.
    venue.place_raw(PlaceOrder {
        market: market.clone(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(40),
        qty: Contracts::new(5),
        client_order_id: ClientOrderId::new("ks-resting-1")?,
    })?;
    venue.place_raw(PlaceOrder {
        market: market.clone(),
        side: Side::No,
        action: Action::Buy,
        limit_price: Cents::new(30),
        qty: Contracts::new(5),
        client_order_id: ClientOrderId::new("ks-resting-2")?,
    })?;
    venue.place_raw(PlaceOrder {
        market,
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(55),
        qty: Contracts::new(3),
        client_order_id: ClientOrderId::new("ks-filled-1")?,
    })?;
    Ok(venue)
}
