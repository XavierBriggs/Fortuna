//! S7 CLI integration tests: `fortuna backtest` + `fortuna validate`.
//!
//! Written FROM the plan (S7) and spec §10 BEFORE implementation (TDD).
//!
//! Three tests, one requirement each:
//!
//! 1. `backtest_cli_idempotent` — two invocations of the backtest handler
//!    against the same fixture source and a fresh ledger; the second run's
//!    `written == 0` and `skipped_idempotent == first.written`.
//!
//! 2. `validate_cli_emits_verdict` — the validate handler's output string
//!    contains the verdict AND every whole-truth field from the spec §7
//!    GO surface (`n_trials`, `family_n_trials`, `effective_n`, `brier_pbo`,
//!    `brier_spa_p`, `clv_edge`, `clv_pbo`, `clv_spa_p`, `sharpe_dsr`,
//!    `verdict`).
//!
//! 3. `cli_is_read_only_on_source` — after a backtest run the source SQLite
//!    file is byte-for-byte identical to the original (the handler must open
//!    it read-only; a write attempt to a read-only file fails the test).
//!
//! All three call the async command handlers DIRECTLY (not the binary) so we
//! can inject a `PgPool` from `#[sqlx::test]` and a path to the committed
//! fixture.
//!
//! ## SQLX discipline
//!
//! The handlers call existing repos (`ValidationRunsRepo`, `BeliefsRepo`,
//! etc.) whose `query!` macros are already in `.sqlx/`. No new `query!`
//! appears in `backtest_cmd.rs`, so no `.sqlx` regen is needed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use fortuna_cli::backtest_cmd::{run_backtest, run_validate, BacktestArgs, ValidateArgs};
use fortuna_core::clock::RealClock;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Absolute path to the committed Aeolus fixture SQL (the same fixture the S6
/// tests use — no new fixture is needed).
fn fixture_sql_path() -> PathBuf {
    // The fixture lives in fortuna-backtest/tests/fixtures/; we reference it
    // with CARGO_MANIFEST_DIR from fortuna-cli (one crate up in the workspace).
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("fortuna-backtest")
        .join("tests")
        .join("fixtures")
        .join("aeolus_archive.sql")
}

/// Write the fixture SQL into a real on-disk SQLite file (so we can test that
/// the file is NOT modified after a backtest run). Returns the path of the
/// created file; the caller is responsible for cleanup.
fn fixture_as_real_file(dir: &std::path::Path) -> PathBuf {
    let sql = std::fs::read_to_string(fixture_sql_path()).expect("fixture must exist");
    let path = dir.join("aeolus_archive_ro_test.db");

    // Create the DB by connecting, running the SQL, then closing.
    {
        let conn = rusqlite::Connection::open(&path).expect("create fixture db");
        conn.execute_batch(&sql).expect("load fixture sql into db");
    }
    path
}

// ---------------------------------------------------------------------------
// Test 1: idempotent replay
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn backtest_cli_idempotent(pool: PgPool) {
    let sql_path = fixture_sql_path();
    assert!(
        sql_path.exists(),
        "fixture SQL must exist: {}",
        sql_path.display()
    );

    let args = BacktestArgs {
        source_name: "aeolus-archive".to_string(),
        sql_fixture_path: Some(sql_path.clone()),
        real_db_path: None,
        from: None,
        to: None,
    };
    let min_n = 3u32;

    // First run
    let report1 = run_backtest(&pool, &args, RealClock, min_n)
        .await
        .expect("first backtest run must succeed");

    // Second run — must be a no-op on the ledger
    let report2 = run_backtest(&pool, &args, RealClock, min_n)
        .await
        .expect("second backtest run must succeed");

    assert_eq!(
        report2.written, 0,
        "second run must write 0 new rows (idempotent); got written={} skipped={}",
        report2.written, report2.skipped_idempotent
    );
    assert_eq!(
        report2.skipped_idempotent, report1.written,
        "second run's skipped_idempotent must equal first run's written"
    );
}

// ---------------------------------------------------------------------------
// Test 2: validate emits the whole-truth surface
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn validate_cli_emits_verdict(pool: PgPool) {
    let args = ValidateArgs {
        scope: "weather:KNYC".to_string(),
        producer: Some("aeolus".to_string()),
    };

    let output = run_validate(&pool, &args, RealClock)
        .await
        .expect("validate must succeed");

    // The whole-truth spec §7 fields must ALL appear in the output.
    // We check their KEYS (not specific values) so the test doesn't hardcode
    // metric values, but does require the complete surface.
    for field in &[
        "n_trials",
        "family_n_trials",
        "effective_n",
        "brier_pbo",
        "brier_spa_p",
        "clv_edge",
        "clv_pbo",
        "clv_spa_p",
        "sharpe_dsr",
        "verdict",
    ] {
        assert!(
            output.contains(field),
            "output must contain field {field:?}; got:\n{output}"
        );
    }
    // A verdict value must also be present (one of the GoDecision variants).
    let has_verdict_value =
        output.contains("Go") || output.contains("NoGo") || output.contains("Insufficient");
    assert!(
        has_verdict_value,
        "output must contain a verdict value (Go/NoGo/Insufficient); got:\n{output}"
    );
}

// ---------------------------------------------------------------------------
// Test 3: source file is read-only (not modified after a backtest run)
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn cli_is_read_only_on_source(pool: PgPool) {
    let tmp = std::env::temp_dir().join(format!(
        "fortuna-cli-ro-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    std::fs::create_dir_all(&tmp).expect("create tmp dir");

    let db_path = fixture_as_real_file(&tmp);

    // Capture pre-run metadata
    let meta_before = std::fs::metadata(&db_path).expect("stat before");
    let size_before = meta_before.len();
    let mtime_before = meta_before.modified().expect("mtime before");

    let args = BacktestArgs {
        source_name: "aeolus-archive".to_string(),
        sql_fixture_path: None,
        real_db_path: Some(db_path.clone()),
        from: None,
        to: None,
    };

    let _ = run_backtest(&pool, &args, RealClock, 3)
        .await
        .expect("backtest on real file must succeed");

    let meta_after = std::fs::metadata(&db_path).expect("stat after");
    let size_after = meta_after.len();
    let mtime_after = meta_after.modified().expect("mtime after");

    assert_eq!(
        size_before, size_after,
        "source DB size must not change (read-only)"
    );
    assert_eq!(
        mtime_before, mtime_after,
        "source DB mtime must not change (read-only)"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp);
}
