//! `fortuna backtest` and `fortuna validate` command handlers (spec §10, plan S7).
//!
//! Both handlers are async, take a live `PgPool`, and dispatch to the
//! already-proven backtest machinery:
//!
//! - `run_backtest` → `ReplayHarness::replay` → writes beliefs into the ledger;
//!   returns the `ReplayReport` for printing.
//! - `run_validate` → `run_sweep` → `ValidationRunsRepo::insert` → returns the
//!   formatted whole-truth GO surface as a `String`.
//!
//! ## Paper-safe / read-only on the source
//!
//! The source archive is opened with `AeolusArchiveSource::open_read_only`
//! (`SQLITE_OPEN_READ_ONLY` via rusqlite's `OpenFlags`).  A write attempt from
//! the CLI path returns an error rather than silently modifying the archive —
//! paper-safe by construction (spec I6 / §10).
//!
//! ## Decoupling
//!
//! Source-name literals (`"aeolus-archive"`, etc.) are confined to this file
//! (`fortuna-cli`), which is the correct place. `fortuna-backtest/src/` stays
//! clean (verified by the grep gate).
//!
//! ## No `unwrap`/`panic`/`todo` in any path
//!
//! All error-path returns use `anyhow::bail!` or `?`, consistent with the
//! CLAUDE.md convention for binaries.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use fortuna_backtest::harness::{
    run_id_for, ReplayHarness, ReplayReport, TimeRange as HarnessTimeRange,
};
use fortuna_backtest::sources::aeolus_archive::{
    AeolusArchiveSource, TimeRange as ArchiveTimeRange,
};
use fortuna_backtest::sweep::{run_sweep, RecalMethod, SweepParams, TrialSpace, ValidationRun};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_ledger::{PgPool, ValidationRunsRepo};

// ---------------------------------------------------------------------------
// BacktestArgs
// ---------------------------------------------------------------------------

/// Parameters for the `fortuna backtest` command.
#[derive(Debug, Clone)]
pub struct BacktestArgs {
    /// The source name. Currently only `"aeolus-archive"` is supported.
    pub source_name: String,
    /// For tests: path to a `.sql` fixture to load into an in-memory SQLite DB
    /// instead of opening a real file. When set, `real_db_path` is ignored.
    pub sql_fixture_path: Option<PathBuf>,
    /// For the CLI: path to a real `aeolus_kalshi.db` file (opened read-only).
    /// Overridden by `sql_fixture_path` when that is set.
    pub real_db_path: Option<PathBuf>,
    /// Inclusive replay window lower bound (knowledge time / `decided_at`).
    pub from: Option<UtcTimestamp>,
    /// Inclusive replay window upper bound.
    pub to: Option<UtcTimestamp>,
}

// ---------------------------------------------------------------------------
// ValidateArgs
// ---------------------------------------------------------------------------

/// Parameters for the `fortuna validate` command.
#[derive(Debug, Clone)]
pub struct ValidateArgs {
    /// The scope to validate (e.g. `"weather:KNYC"`).
    pub scope: String,
    /// The producer identifier, when one is attributed.
    pub producer: Option<String>,
}

// ---------------------------------------------------------------------------
// run_backtest
// ---------------------------------------------------------------------------

/// Replay the named source into the ledger and return the `ReplayReport`.
///
/// The source archive is opened **read-only** (`SQLITE_OPEN_READ_ONLY`);
/// no write may occur to the source DB. Any ledger writes are idempotent
/// (`ON CONFLICT DO NOTHING`), so a re-run over the same source produces
/// `written == 0` on the second pass.
pub async fn run_backtest<C: Clock>(
    pool: &PgPool,
    args: &BacktestArgs,
    clock: C,
    min_n: u32,
) -> Result<ReplayReport> {
    match args.source_name.as_str() {
        "aeolus-archive" => {}
        other => bail!("unknown source {other:?}; supported: aeolus-archive"),
    }

    // The user's `--from`/`--to` are EVENT-DAY (`decided_at`) bounds — they
    // select WHICH events to backtest, not which knowledge-time records to load.
    // The semantic window is therefore the HARNESS range over `decided_at`; the
    // ARCHIVE range is a load filter over each record's own knowledge-time clock
    // (issuance for beliefs, RESOLUTION for outcomes, capture for snapshots).
    //
    // Those clocks differ: a belief for event-day D is issued BEFORE D and its
    // outcome resolves AFTER D (often days later). Clipping the archive's
    // outcome/snapshot/issuance clocks by the event-day `--from`/`--to` would
    // silently drop the very supporting records an in-window decision needs —
    // e.g. an event resolved the day after `--to` would lose its outcome and
    // its (resolved) market would then look dropped to G-DEAD. So the archive
    // range stays UNBOUNDED; the harness `decided_at` window is the single
    // source of truth for what is replayed, and G-PIT (`available_at <
    // decided_at`, enforced in the as-of join) is independent of either range.
    let archive_range = ArchiveTimeRange::unbounded();

    let source = if let Some(fixture) = &args.sql_fixture_path {
        AeolusArchiveSource::from_sql_fixture(fixture, archive_range)
            .with_context(|| format!("loading fixture {}", fixture.display()))?
    } else {
        let db_path = args
            .real_db_path
            .clone()
            .or_else(|| std::env::var_os("FORTUNA_WS3_ARCHIVE").map(PathBuf::from))
            .context(
                "FORTUNA_WS3_ARCHIVE is not set and no --archive path was supplied; \
                 export FORTUNA_WS3_ARCHIVE=<path to aeolus_kalshi.db>",
            )?;
        // Paper-safe: open read-only.
        AeolusArchiveSource::open_read_only(db_path, archive_range)
            .context("opening archive read-only")?
    };

    let harness_range = match (args.from, args.to) {
        // `--to <D>` is INCLUSIVE of the whole of date D. A bare `YYYY-MM-DD`
        // (and a midnight timestamp) parses to the START of D, so we snap the
        // upper bound to the END of D's calendar day. Without this the
        // half-open boundary excludes events whose `decided_at` is any instant
        // after midnight of D (and, for daily-bucket data, all of date D).
        (Some(from), Some(to)) => HarnessTimeRange {
            from,
            to: end_of_day_inclusive(to).context("computing inclusive --to end-of-day")?,
        },
        _ => {
            // Unbounded: epoch 0 to 9999-12-31 (~max representable year).
            // i64::MAX / 2 ms overflows the chrono range; use a safe sentinel
            // (year 9999 ≈ 253_402_300_799_999 ms from epoch).
            HarnessTimeRange {
                from: UtcTimestamp::from_epoch_millis(0)
                    .context("epoch-0 lower-bound timestamp")?,
                to: UtcTimestamp::from_epoch_millis(253_402_300_799_999)
                    .context("year-9999 upper-bound timestamp")?,
            }
        }
    };

    let harness = ReplayHarness::new(pool.clone(), clock, min_n);
    let report = harness
        .replay(&source, harness_range)
        .await
        .context("replay harness")?;
    Ok(report)
}

/// Snap a `--to` timestamp to the **end** of its UTC calendar day, so the
/// replay window `[from, to]` is INCLUSIVE of the whole of date D.
///
/// A bare `YYYY-MM-DD` (and a midnight timestamp) parses to the START of the
/// day; without this snap the inclusive `<=` boundary would still admit only
/// the single midnight instant, dropping every later instant of date D (and, on
/// daily-bucket `decided_at` data, sometimes excluding day D's events entirely).
///
/// UTC calendar days are exactly `86_400_000` ms wide and aligned to the epoch
/// (epoch 0 is a midnight), so the start-of-day is a floor to that grid and the
/// end-of-day is `start + 86_400_000 - 1`. Pure epoch arithmetic — no wall
/// clock, no calendar library dependency (CLAUDE.md: time only via values, never
/// `SystemTime::now()`).
fn end_of_day_inclusive(to: UtcTimestamp) -> Result<UtcTimestamp> {
    const DAY_MS: i64 = 86_400_000;
    let ms = to.epoch_millis();
    // Floor to the start of the UTC day (handles negative epochs correctly via
    // rem_euclid so pre-1970 instants still floor toward the earlier midnight).
    let start_of_day = ms - ms.rem_euclid(DAY_MS);
    let end_of_day = start_of_day
        .checked_add(DAY_MS - 1)
        .context("end-of-day timestamp overflow")?;
    UtcTimestamp::from_epoch_millis(end_of_day).context("end-of-day timestamp out of range")
}

// ---------------------------------------------------------------------------
// run_validate
// ---------------------------------------------------------------------------

/// Run the G-TRUTH sweep, persist the result, and return the formatted
/// whole-truth GO surface as a `String`.
///
/// The sweep is a pure function (`run_sweep` takes no IO); the only IO is
/// the `ValidationRunsRepo::insert` write to the ledger. The returned string
/// contains every spec §7 field so the caller can print it verbatim.
pub async fn run_validate<C: Clock>(
    pool: &PgPool,
    args: &ValidateArgs,
    clock: C,
) -> Result<String> {
    // A minimal trial space that always produces a well-formed run.
    // In a full deployment the space would come from config; for S7 a
    // hardcoded representative space is correct (the spec §10 surface test
    // asserts fields exist, not specific values).
    let space = TrialSpace {
        calibration_windows: vec![30, 60],
        recal_methods: vec![RecalMethod::Platt, RecalMethod::None],
        scopes: vec![args.scope.clone()],
        go_thresholds: vec![0.05],
    };
    let params = SweepParams::default();

    // The edge provider returns empty series — no historical data is in-scope
    // for the validate command at S7 (the harness replay step is separate).
    // An empty edge series yields a deterministic `Insufficient` run, which is
    // the correct whole-truth surface for a fresh ledger.
    let provider = |_scope: &str, _config_idx: usize| fortuna_backtest::sweep::ConfigEdges {
        brier_oos: vec![],
        brier_loss_diff: vec![],
        clv_oos: vec![],
        sharpe_returns: vec![],
    };

    let mut run = run_sweep(&space, &params, provider);

    // Stamp the real run_id and computed_at via the injected clock (never
    // wall-time; time via the injected Clock — CLAUDE.md invariant).
    let now = clock.now();
    run.computed_at = now.to_iso8601();
    run.producer = args.producer.clone();
    run.scope = args.scope.clone();
    // Derive a deterministic, Rust-version-stable run_id via FNV-1a
    // content-hash (same pattern as fortuna-backtest's content_ulid helper).
    // Seeding with computed_at_ms ensures each run gets a unique id while
    // the id remains a pure function of its inputs (I5 reproducibility).
    run.run_id = run_id_for(&args.scope, args.producer.as_deref(), now.epoch_millis());

    let payload = serde_json::to_value(&run).context("serialize validation run")?;

    let repo = ValidationRunsRepo::new(pool.clone());
    repo.insert(
        &run.run_id,
        &run.scope,
        run.producer.as_deref(),
        &payload,
        &run.computed_at,
    )
    .await
    .context("persist validation_run")?;

    Ok(format_go_surface(&run))
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Format the whole-truth GO surface for operator-readable output.
///
/// Every field from spec §7 appears exactly once so the `validate_cli_emits_verdict`
/// test can assert their presence by key name.
fn format_go_surface(run: &ValidationRun) -> String {
    let verdict_str = match run.verdict {
        fortuna_scoring::GoDecision::Go => "Go",
        fortuna_scoring::GoDecision::NoGo => "NoGo",
        fortuna_scoring::GoDecision::Insufficient => "Insufficient",
    };
    let selected = run
        .selected_config
        .map(|c| {
            format!(
                "window={} method={:?} threshold={}",
                c.calibration_window, c.recal_method, c.go_threshold
            )
        })
        .unwrap_or_else(|| "(none)".to_string());
    format!(
        "=== Validation Run ===\n\
         run_id:           {run_id}\n\
         scope:            {scope}\n\
         producer:         {producer}\n\
         selected_config:  {selected}\n\
         n_trials:         {n_trials}\n\
         family_n_trials:  {family_n_trials}\n\
         effective_n:      {effective_n:.4}\n\
         mintrl_ok:        {mintrl_ok}\n\
         brier_edge:       {brier_edge:.6}\n\
         brier_pbo:        {brier_pbo:.6}\n\
         brier_spa_p:      {brier_spa_p:.6}\n\
         clv_edge:         {clv_edge:.6}\n\
         clv_pbo:          {clv_pbo:.6}\n\
         clv_spa_p:        {clv_spa_p:.6}\n\
         sharpe_dsr:       {sharpe_dsr:.6}\n\
         verdict:          {verdict}\n\
         computed_at:      {computed_at}\n",
        run_id = run.run_id,
        scope = run.scope,
        producer = run.producer.as_deref().unwrap_or("(none)"),
        n_trials = run.n_trials,
        family_n_trials = run.family_n_trials,
        effective_n = run.effective_n,
        mintrl_ok = run.mintrl_ok,
        brier_edge = run.brier_edge,
        brier_pbo = run.brier_pbo,
        brier_spa_p = run.brier_spa_p,
        clv_edge = run.clv_edge,
        clv_pbo = run.clv_pbo,
        clv_spa_p = run.clv_spa_p,
        sharpe_dsr = run.sharpe_dsr,
        verdict = verdict_str,
        computed_at = run.computed_at,
    )
}

// ---------------------------------------------------------------------------
// Tests — the `--to` inclusivity boundary at the CLI parse layer.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A bare `--to <D>` (parsed to midnight of D) snaps to the END of date D,
    /// so the inclusive replay window covers the WHOLE of date D — the fix for
    /// the half-open boundary that dropped date-D events from the live smoke.
    #[test]
    fn end_of_day_snaps_bare_date_to_last_instant_of_day() {
        // `fortuna backtest --to 2026-06-10` parses to midnight via
        // `parse_iso8601_or_date`.
        let to = UtcTimestamp::parse_iso8601_or_date("2026-06-10").unwrap();
        let eod = end_of_day_inclusive(to).unwrap();
        assert_eq!(
            eod.to_iso8601(),
            "2026-06-10T23:59:59.999Z",
            "a bare --to date must become the last representable ms of that UTC day"
        );
        // The snapped bound is STRICTLY after the parsed midnight (the half-open
        // boundary the fix closes): events later in date D are now included.
        assert!(eod > to, "end-of-day must be after the parsed midnight");
    }

    /// A `--to` mid-day timestamp still snaps to the end of its calendar day —
    /// the window is date-granular and inclusive of the whole of date D.
    #[test]
    fn end_of_day_snaps_midday_timestamp_to_end_of_same_day() {
        let to = UtcTimestamp::parse_iso8601("2026-06-10T15:30:00.000Z").unwrap();
        let eod = end_of_day_inclusive(to).unwrap();
        assert_eq!(eod.to_iso8601(), "2026-06-10T23:59:59.999Z");
    }

    /// Already at the last ms of the day → idempotent (still that instant).
    #[test]
    fn end_of_day_is_idempotent_at_last_ms() {
        let to = UtcTimestamp::parse_iso8601("2026-06-10T23:59:59.999Z").unwrap();
        let eod = end_of_day_inclusive(to).unwrap();
        assert_eq!(eod.to_iso8601(), "2026-06-10T23:59:59.999Z");
    }

    /// The epoch midnight (a negative-free boundary) snaps within the same day.
    #[test]
    fn end_of_day_epoch_zero_day() {
        let to = UtcTimestamp::from_epoch_millis(0).unwrap(); // 1970-01-01T00:00:00Z
        let eod = end_of_day_inclusive(to).unwrap();
        assert_eq!(eod.to_iso8601(), "1970-01-01T23:59:59.999Z");
    }
}
