//! `fortuna doctor` — operator readiness checklist (spec W3 / WS4).
//!
//! Prints a `[ok]`/`[FAIL]` checklist and returns a `DoctorReport`; the
//! binary exits non-zero if `report.all_green == false`.
//!
//! ## Checks (in order)
//! 1. **db_reachable** — `SELECT 1`; proves the pool works.
//! 2. **migrations_applied** — no dirty rows in `_sqlx_migrations`.
//! 3. **env_creds** — required env vars present and non-empty (presence +
//!    length ONLY; values are NEVER printed).
//! 4. **mode_safe** — `[runtime].execution_mode == "paper_ledger"` and
//!    `[runtime].orders_enabled == false`; skipped when no config path.
//! 5. **grants** — the app role can `SELECT` from key tables.
//! 6. **source_reachable** — a read-only Kalshi/Aeolus HTTP probe; skipped
//!    when `opts.offline == true` (default in CI).
//!
//! The `run` function is pure-ish (the pool is injected; env is injected
//! via `DoctorOpts`). `main` calls it and handles printing + exit code.

use std::collections::BTreeMap;

use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single checklist row.
#[derive(Debug)]
pub struct Check {
    /// Short snake_case identifier used in tests.
    pub name: String,
    /// True = passed; false = failed.
    pub ok: bool,
    /// Human-readable detail (the failure reason, or an affirmation).
    pub detail: String,
}

/// The full doctor report.
#[derive(Debug)]
pub struct DoctorReport {
    /// True only when every check passed.
    pub all_green: bool,
    pub checks: Vec<Check>,
}

/// Options / injectable surface for `run`.
///
/// In production `main.rs` builds this from real env / config path. In tests
/// the env map is injected directly so the test controls which vars are
/// present without mutating the real process environment.
pub struct DoctorOpts {
    /// Injected env map (key → value). Production: snapshot of the real env.
    pub env: BTreeMap<String, String>,
    /// When true, skip the network source-reachability check (CI / offline).
    pub offline: bool,
    /// Optional path to `config/fortuna.toml`; skips mode-safe check if None.
    pub config_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Required env var names (mirrors boot.rs `validate_env`)
// ---------------------------------------------------------------------------

/// Env vars that must be present and non-empty for the daemon to boot.
/// This list mirrors `fortuna_live::boot::validate_env` — keep in sync.
const REQUIRED_ENV_VARS: &[&str] = &[
    "DATABASE_URL",
    "FORTUNA_SLACK_BOT_TOKEN",
    "FORTUNA_DEADMAN_URL",
    "FORTUNA_SLACK_CHANNEL_TRADING",
    "FORTUNA_SLACK_CHANNEL_ALERTS",
    "FORTUNA_SLACK_CHANNEL_REVIEW",
    "FORTUNA_SLACK_CHANNEL_DIGEST",
    "FORTUNA_SLACK_CHANNEL_OPS",
];

/// Tables the app role must be able to SELECT from.
const PROBE_TABLES: &[&str] = &["events", "beliefs", "audit", "fills", "settlement_entries"];

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

/// Run all readiness checks and return a `DoctorReport`.
///
/// # Design
/// - Never panics; all errors are folded into `Check { ok: false, detail }`.
/// - Never prints secret values — presence/length only.
/// - The pool is already connected when this is called; `db_reachable` is a
///   `SELECT 1` probe, not a reconnect.
pub async fn run(pool: &PgPool, opts: &DoctorOpts) -> DoctorReport {
    let mut checks: Vec<Check> = Vec::new();
    let mut all_green = true;

    // ------------------------------------------------------------------
    // 1. DB reachable
    // ------------------------------------------------------------------
    let db_ok = check_db_reachable(pool).await;
    if !db_ok.ok {
        all_green = false;
    }
    checks.push(db_ok);

    // ------------------------------------------------------------------
    // 2. Migrations applied
    // ------------------------------------------------------------------
    let mig_ok = check_migrations_applied(pool).await;
    if !mig_ok.ok {
        all_green = false;
    }
    checks.push(mig_ok);

    // ------------------------------------------------------------------
    // 3. Env / creds present (presence + length ONLY — never print values)
    // ------------------------------------------------------------------
    let cred_ok = check_env_creds(&opts.env);
    if !cred_ok.ok {
        all_green = false;
    }
    checks.push(cred_ok);

    // ------------------------------------------------------------------
    // 4. Mode-safe (skip when no config path is given)
    // ------------------------------------------------------------------
    if let Some(ref path) = opts.config_path {
        let mode_ok = check_mode_safe(path);
        if !mode_ok.ok {
            all_green = false;
        }
        checks.push(mode_ok);
    }

    // ------------------------------------------------------------------
    // 5. GRANTs — app role can SELECT probe tables
    // ------------------------------------------------------------------
    let grants_ok = check_grants(pool).await;
    if !grants_ok.ok {
        all_green = false;
    }
    checks.push(grants_ok);

    // ------------------------------------------------------------------
    // 6. Source reachable (skipped in offline / CI mode)
    // ------------------------------------------------------------------
    if !opts.offline {
        let src_ok = check_source_reachable().await;
        if !src_ok.ok {
            all_green = false;
        }
        checks.push(src_ok);
    }

    DoctorReport { all_green, checks }
}

// ---------------------------------------------------------------------------
// Print helper (used by main)
// ---------------------------------------------------------------------------

/// Print the report to stdout in checklist format.
pub fn print_report(report: &DoctorReport) {
    for check in &report.checks {
        let marker = if check.ok { "[ok]  " } else { "[FAIL]" };
        println!("{marker} {}: {}", check.name, check.detail);
    }
    if report.all_green {
        println!("\nfortuna doctor: all checks passed");
    } else {
        println!("\nfortuna doctor: one or more checks FAILED");
    }
}

// ---------------------------------------------------------------------------
// Individual check implementations
// ---------------------------------------------------------------------------

async fn check_db_reachable(pool: &PgPool) -> Check {
    let name = "db_reachable".to_string();
    match sqlx::query("SELECT 1").execute(pool).await {
        Ok(_) => Check {
            name,
            ok: true,
            detail: "postgres responded to SELECT 1".to_string(),
        },
        Err(e) => Check {
            name,
            ok: false,
            detail: format!("SELECT 1 failed: {e}"),
        },
    }
}

async fn check_migrations_applied(pool: &PgPool) -> Check {
    let name = "migrations_applied".to_string();
    // Check for dirty (failed) migrations.
    let dirty_result: Result<i64, _> =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = false")
            .fetch_one(pool)
            .await;
    match dirty_result {
        Err(e) => {
            return Check {
                name,
                ok: false,
                detail: format!("could not query _sqlx_migrations: {e}"),
            };
        }
        Ok(dirty) if dirty > 0 => {
            return Check {
                name,
                ok: false,
                detail: format!("{dirty} dirty (failed) migration(s) in _sqlx_migrations"),
            };
        }
        Ok(_) => {}
    }

    // Count applied migrations (success = true).
    let applied_result: Result<i64, _> =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = true")
            .fetch_one(pool)
            .await;
    match applied_result {
        Ok(n) if n > 0 => Check {
            name,
            ok: true,
            detail: format!("{n} migration(s) applied, none dirty"),
        },
        Ok(_) => Check {
            name,
            ok: false,
            detail: "no migrations have been applied (empty _sqlx_migrations)".to_string(),
        },
        Err(e) => Check {
            name,
            ok: false,
            detail: format!("could not count applied migrations: {e}"),
        },
    }
}

fn check_env_creds(env: &BTreeMap<String, String>) -> Check {
    let name = "env_creds".to_string();
    let mut missing: Vec<&str> = Vec::new();
    let mut empty: Vec<&str> = Vec::new();

    for var in REQUIRED_ENV_VARS {
        match env.get(*var) {
            None => missing.push(var),
            Some(v) if v.trim().is_empty() => empty.push(var),
            Some(_) => {} // present and non-empty — OK; value is NEVER inspected
        }
    }

    if missing.is_empty() && empty.is_empty() {
        Check {
            name,
            ok: true,
            detail: format!(
                "{} required env vars present and non-empty",
                REQUIRED_ENV_VARS.len()
            ),
        }
    } else {
        let mut parts: Vec<String> = Vec::new();
        if !missing.is_empty() {
            parts.push(format!("missing: {}", missing.join(", ")));
        }
        if !empty.is_empty() {
            parts.push(format!("empty: {}", empty.join(", ")));
        }
        Check {
            name,
            ok: false,
            // Report the VAR NAMES only — never the values.
            detail: parts.join("; "),
        }
    }
}

fn check_mode_safe(config_path: &str) -> Check {
    let name = "mode_safe".to_string();
    let text = match std::fs::read_to_string(config_path) {
        Ok(t) => t,
        Err(e) => {
            return Check {
                name,
                ok: false,
                detail: format!("could not read config {config_path:?}: {e}"),
            };
        }
    };
    let value: toml::Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            return Check {
                name,
                ok: false,
                detail: format!("could not parse config {config_path:?}: {e}"),
            };
        }
    };
    let runtime = match value.get("runtime") {
        Some(r) => r,
        None => {
            return Check {
                name,
                ok: false,
                detail: "no [runtime] section in config — cannot verify mode safety".to_string(),
            };
        }
    };
    let mode = runtime
        .get("execution_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let orders_enabled = runtime
        .get("orders_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true); // default to unsafe (fail closed)

    if mode == "paper_ledger" && !orders_enabled {
        Check {
            name,
            ok: true,
            detail: "execution_mode=paper_ledger, orders_enabled=false".to_string(),
        }
    } else {
        Check {
            name,
            ok: false,
            detail: format!(
                "NOT paper-safe: execution_mode={mode:?}, orders_enabled={orders_enabled}"
            ),
        }
    }
}

async fn check_grants(pool: &PgPool) -> Check {
    let name = "grants".to_string();
    let mut failed: Vec<String> = Vec::new();

    for table in PROBE_TABLES {
        // A minimal SELECT with a false WHERE so no rows are returned, but the
        // permission check fires. On permission denial sqlx returns an error.
        // SECURITY NOTE: `table` MUST remain a compile-time constant from PROBE_TABLES
        // (a &[&str] of hard-coded names) — the format! here is NOT parameterized user
        // input. Never allow a non-const table name to reach this format! call.
        let q = format!("SELECT 1 FROM {table} WHERE false");
        if let Err(e) = sqlx::query(&q).execute(pool).await {
            failed.push(format!("{table}: {e}"));
        }
    }

    if failed.is_empty() {
        Check {
            name,
            ok: true,
            detail: format!("SELECT on {} table(s) OK", PROBE_TABLES.len()),
        }
    } else {
        Check {
            name,
            ok: false,
            detail: format!("permission denied on: {}", failed.join("; ")),
        }
    }
}

async fn check_source_reachable() -> Check {
    let name = "source_reachable".to_string();
    // Kalshi public status endpoint (read-only, unauthenticated).
    let url = "https://api.elections.kalshi.com/trade-api/v2/exchange/status";
    match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Err(e) => Check {
            name,
            ok: false,
            detail: format!("could not build HTTP client: {e}"),
        },
        Ok(client) => match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 401 => {
                // 401 is acceptable: the endpoint exists and responded.
                Check {
                    name,
                    ok: true,
                    detail: format!("Kalshi status endpoint responded ({})", resp.status()),
                }
            }
            Ok(resp) => Check {
                name,
                ok: false,
                detail: format!("Kalshi status endpoint returned {}", resp.status()),
            },
            Err(e) => Check {
                name,
                ok: false,
                detail: format!("Kalshi status endpoint unreachable: {e}"),
            },
        },
    }
}
