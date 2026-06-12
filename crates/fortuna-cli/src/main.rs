//! `fortuna` — the operator CLI (spec Section 8; the I2 re-arm path).
//!
//! Drawdown-halt re-arm and kill-switch reversal are CLI-ONLY by design:
//! Slack may request, the CLI confirms; a compromised Slack token must not
//! be able to un-halt a halted system.
//!
//! Commands:
//!   fortuna status
//!   fortuna halt   <global|strategy:<id>|venue:<id>> --reason "..." --operator <name>
//!   fortuna rearm  <global|strategy:<id>|venue:<id>> --reason "..." --operator <name>
//!   fortuna kill   [--flatten] --journal <path>
//!   fortuna config check [--config-path <path>]
//!   fortuna logs   <daemon|recorder> [-f]
//!
//! halt/rearm write durable halt_events + an audit row; the running system
//! restores flags from the fold at boot and observes operator events via its
//! halt-poll (runner, T0.10). `kill` execs the STANDALONE fortuna-killswitch
//! binary — this CLI is a trigger, never a substitute for it.
//!
//! T4.4 lifecycle layer (design docs/design/fortuna-cli.md, amendments
//! binding): `status` always prints the process-health section from
//! name-validated pidfiles under FORTUNA_RUNTIME_DIR (default data/runtime/,
//! A5) and DEGRADES without a database — no DATABASE_URL or an unreachable
//! Pg never hides process health (A9 pins exit 0). `start`/`stop` are later
//! slices; `stop` ships only against T4.1's asserted SIGTERM contract (A1).
//!
//! Binaries may use anyhow (conventions); the no-unwrap rule still holds.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

use anyhow::{bail, Context, Result};
use fortuna_core::clock::{Clock, RealClock, UtcTimestamp};
use fortuna_ledger::{parse_halt_scope, AuditWriter, HaltsRepo};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The two managed components (design Section 3). `start`/`stop`/`status`/
/// `logs` all key off these names; the pidfile and log paths derive from them.
const COMPONENTS: [&str; 2] = ["daemon", "recorder"];

/// Default config path; `--config-path` overrides (committed shape is
/// config/fortuna.example.toml — the real file is operator-local).
const DEFAULT_CONFIG_PATH: &str = "config/fortuna.toml";

/// Upper bound on the degradable db section of `status`: a Pg outage must
/// not stall the operator's view of process health (sqlx's own pool timeout
/// is 30s — far too slow for a status command).
const STATUS_DB_TIMEOUT_SECS: u64 = 5;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fortuna: {e:#}");
            ExitCode::from(1)
        }
    }
}

struct Args {
    command: String,
    positional: Vec<String>,
    reason: Option<String>,
    operator: Option<String>,
    journal: Option<String>,
    config_path: Option<String>,
    flatten: bool,
    follow: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        command: String::new(),
        positional: Vec::new(),
        reason: None,
        operator: None,
        journal: None,
        config_path: None,
        flatten: false,
        follow: false,
    };
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--reason" => {
                i += 1;
                args.reason = raw.get(i).cloned();
            }
            "--operator" => {
                i += 1;
                args.operator = raw.get(i).cloned();
            }
            "--journal" => {
                i += 1;
                args.journal = raw.get(i).cloned();
            }
            "--config-path" => {
                i += 1;
                args.config_path = raw.get(i).cloned();
            }
            "--flatten" => args.flatten = true,
            "-f" | "--follow" => args.follow = true,
            other if args.command.is_empty() => args.command = other.to_string(),
            other => args.positional.push(other.to_string()),
        }
        i += 1;
    }
    if args.command.is_empty() {
        bail!(
            "usage: fortuna <status|halt|rearm|kill|config check|logs> [scope|component] \
             [--reason ..] [--operator ..] [--journal ..] [--flatten] \
             [--config-path ..] [-f]"
        );
    }
    Ok(args)
}

fn run() -> Result<()> {
    let args = parse_args()?;
    match args.command.as_str() {
        "kill" => kill(&args),
        "config" => config_cmd(&args),
        "logs" => logs_cmd(&args),
        "status" => status_cmd(&args),
        "halt" | "rearm" => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio runtime")?;
            runtime.block_on(db_command(&args))
        }
        other => bail!("unknown command {other:?}"),
    }
}

// --------------------------------------------------------------- T4.4 helpers

/// Runtime state directory (A5): pidfiles + redirected logs. data/ is
/// gitignored and survives reboots, unlike /tmp on macOS.
fn runtime_dir() -> PathBuf {
    std::env::var_os("FORTUNA_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/runtime"))
}

fn pidfile_path(dir: &Path, component: &str) -> PathBuf {
    dir.join(format!("{component}.pid"))
}

fn log_path(dir: &Path, component: &str) -> PathBuf {
    dir.join("logs").join(format!("{component}.log"))
}

/// `ps -p <pid> -o comm=` — one call answers both liveness and identity
/// (A3: macOS reuses PIDs; a live pid is trusted only if its command path
/// contains the name the pidfile claims). None = not running.
fn comm_of(pid: i64) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let comm = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if comm.is_empty() {
        None
    } else {
        Some(comm)
    }
}

/// ISO8601 of a stop marker's mtime (A7: "stopping since T"). File mtime is
/// recorded state, not a clock read — the Clock-injection rule is untouched.
fn stopping_since(marker: &Path) -> Option<String> {
    let modified = std::fs::metadata(marker).ok()?.modified().ok()?;
    let millis = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()?;
    UtcTimestamp::from_epoch_millis(millis)
        .ok()
        .map(|t| t.to_iso8601())
}

/// One status line per component. Every distrust path reads as "stopped":
/// a stale pidfile must never be reported (or later signaled) as a live
/// process.
fn process_state_line(dir: &Path, component: &str) -> String {
    let pidpath = pidfile_path(dir, component);
    let raw = match std::fs::read_to_string(&pidpath) {
        Err(_) => return "stopped".to_string(),
        Ok(raw) => raw,
    };
    let mut lines = raw.lines();
    let pid = lines.next().and_then(|l| l.trim().parse::<i64>().ok());
    let name = lines.next().map(|l| l.trim().to_string());
    let (pid, name) = match (pid, name) {
        (Some(pid), Some(name)) if !name.is_empty() => (pid, name),
        _ => {
            return format!(
                "stopped (stale pidfile {}: unparseable — expected \"<pid>\\n<name>\")",
                pidpath.display()
            )
        }
    };
    match comm_of(pid) {
        None => format!("stopped (stale pidfile: pid {pid} not running)"),
        Some(comm) if !comm.contains(&name) => format!(
            "stopped (stale pidfile: pid {pid} name mismatch — \
             ps reports {comm:?}, pidfile claims {name:?})"
        ),
        Some(_) => match stopping_since(&dir.join(format!("{component}.stopping"))) {
            Some(since) => format!("stopping since {since} (pid {pid})"),
            None => format!("running (pid {pid})"),
        },
    }
}

/// `fortuna config check [--config-path <p>]`: whole-shape validation via
/// fortuna-ops, starts nothing, mutates nothing.
fn config_cmd(args: &Args) -> Result<()> {
    match args.positional.first().map(String::as_str) {
        Some("check") => {}
        _ => bail!("usage: fortuna config check [--config-path <path>]"),
    }
    let path = args
        .config_path
        .clone()
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    fortuna_ops::FortunaConfig::load_file(&path)
        .with_context(|| format!("config check failed for {path}"))?;
    println!("config OK: {path}");
    Ok(())
}

/// `fortuna logs <component> [-f]`: tail the redirected log (A4 — `start`
/// owns the redirection; neither binary has a --log-file flag). exec
/// replaces this process so Ctrl-C lands on tail directly.
fn logs_cmd(args: &Args) -> Result<()> {
    let component = args
        .positional
        .first()
        .context("usage: fortuna logs <daemon|recorder> [-f]")?;
    if !COMPONENTS.contains(&component.as_str()) {
        bail!("unknown component {component:?} — components: daemon, recorder");
    }
    let path = log_path(&runtime_dir(), component);
    if !path.is_file() {
        bail!(
            "no log file at {} (has `fortuna start` run?)",
            path.display()
        );
    }
    let mut cmd = std::process::Command::new("tail");
    cmd.arg("-n50");
    if args.follow {
        cmd.arg("-f");
    }
    cmd.arg(&path);
    use std::os::unix::process::CommandExt;
    let err = cmd.exec(); // only returns on failure
    bail!("exec tail failed: {err}");
}

/// The A6 one-liner: venue (and mode, once config grows one) as configured
/// ON DISK — the running daemon may differ until restart. Raw toml::Value
/// read because [daemon] is the daemon's section, not FortunaConfig's.
fn config_on_disk(path: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let value: toml::Value = toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let daemon = value
        .get("daemon")
        .ok_or_else(|| format!("{path}: no [daemon] section"))?;
    let venue = daemon
        .get("venue")
        .and_then(|v| v.as_str())
        .unwrap_or("unset");
    let mut line = format!("venue={venue}");
    if let Some(mode) = daemon.get("mode").and_then(|v| v.as_str()) {
        line.push_str(&format!(" mode={mode}"));
    }
    Ok(line)
}

/// `fortuna status`, degradable by design (Section 3): process health
/// ALWAYS prints; the db section prints when DATABASE_URL is set and Pg
/// answers within the bound; nothing about the database can fail the
/// command (A9 pins exit 0 without DATABASE_URL; I5 visibility is the
/// daemon's own audit rows, not this read path).
fn status_cmd(args: &Args) -> Result<()> {
    let dir = runtime_dir();
    println!("processes:");
    for component in COMPONENTS {
        println!("  {component}: {}", process_state_line(&dir, component));
    }
    let config_path = args
        .config_path
        .clone()
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    match config_on_disk(&config_path) {
        Ok(line) => println!("config on disk: {line} (daemon may differ until restart)"),
        Err(reason) => println!("config on disk: unavailable ({reason})"),
    }
    match std::env::var("DATABASE_URL") {
        Err(_) => {
            println!("db: DATABASE_URL not set — halts/audit sections skipped");
            Ok(())
        }
        Ok(url) => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio runtime")?;
            let bounded = runtime.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(STATUS_DB_TIMEOUT_SECS),
                    status_db_section(&url),
                )
                .await
            });
            match bounded {
                Ok(Ok(())) => {}
                Ok(Err(e)) => println!("db: unavailable — {e:#}"),
                Err(_) => {
                    println!("db: unavailable — no response within {STATUS_DB_TIMEOUT_SECS}s")
                }
            }
            Ok(())
        }
    }
}

/// The pre-T4.4 status body, queries unchanged (design checklist item 9).
async fn status_db_section(url: &str) -> Result<()> {
    let pool = fortuna_ledger::connect(url).await?;
    let halts = HaltsRepo::new(pool.clone());
    let clock = RealClock;
    let now = clock.now();
    let active = halts.active().await?;
    if active.is_empty() {
        println!("halts: none");
    } else {
        println!("halts ({}):", active.len());
        for (scope, reason) in active {
            println!("  {} — {reason}", fortuna_ledger::halt_scope_string(&scope));
        }
    }
    let audit = AuditWriter::new(
        pool,
        std::sync::Arc::new(RealClock),
        now.epoch_millis() as u64,
    );
    for kind in ["halt", "gate_decision", "order"] {
        let rows = audit.recent(kind, 3).await?;
        if !rows.is_empty() {
            println!("recent {kind}:");
            for r in rows {
                println!("  {} {}", r.at, r.payload);
            }
        }
    }
    Ok(())
}

/// Trigger the STANDALONE kill switch. This must keep working with Postgres
/// down, so it never touches the database — it execs the independent binary.
fn kill(args: &Args) -> Result<()> {
    let journal = args
        .journal
        .clone()
        .unwrap_or_else(|| "/tmp/fortuna-killswitch.jsonl".to_string());
    let action = if args.flatten { "report" } else { "freeze" };
    eprintln!("fortuna: invoking standalone kill switch ({action}, journal {journal})");
    let status = std::process::Command::new("fortuna-killswitch")
        .args([action, "--journal", &journal])
        .status()
        .or_else(|_| {
            // Dev fallback: through cargo when the installed binary is absent.
            std::process::Command::new(env!("CARGO"))
                .args([
                    "run",
                    "-q",
                    "-p",
                    "fortuna-killswitch",
                    "--",
                    action,
                    "--journal",
                    &journal,
                ])
                .status()
        })
        .context("spawning fortuna-killswitch")?;
    if !status.success() {
        bail!("kill switch exited with {status}");
    }
    Ok(())
}

async fn db_command(args: &Args) -> Result<()> {
    let url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL is required for halt/rearm (kill and the read commands work without it)",
    )?;
    let pool = fortuna_ledger::connect(&url).await?;
    let halts = HaltsRepo::new(pool.clone());
    let clock = RealClock;
    let now = clock.now();

    match args.command.as_str() {
        "halt" | "rearm" => {
            let scope_raw = args
                .positional
                .first()
                .context("scope required: global | strategy:<id> | venue:<id>")?;
            let scope = parse_halt_scope(scope_raw)
                .with_context(|| format!("unparseable scope {scope_raw:?}"))?;
            let reason = args.reason.clone().context("--reason is required")?;
            let operator = args
                .operator
                .clone()
                .context("--operator <name> is required (operator actions are attributed)")?;
            let audit = AuditWriter::new(
                pool.clone(),
                std::sync::Arc::new(RealClock),
                now.epoch_millis() as u64,
            );
            if args.command == "halt" {
                halts.record_set(&scope, &reason, &operator, now).await?;
                audit
                    .append(
                        "halt",
                        Some(&operator),
                        None,
                        serde_json::json!({"action": "set", "scope": scope_raw, "reason": reason}),
                    )
                    .await?;
                println!(
                    "halt set on {scope_raw}; the runner enforces it within its poll interval"
                );
            } else {
                // I2: THE human re-arm path. Out-of-band by construction.
                halts.record_rearm(&scope, &reason, &operator, now).await?;
                audit
                    .append(
                        "halt",
                        Some(&operator),
                        None,
                        serde_json::json!({"action": "rearm", "scope": scope_raw, "reason": reason}),
                    )
                    .await?;
                println!("re-armed {scope_raw} (operator: {operator})");
            }
            Ok(())
        }
        _ => bail!("unreachable"),
    }
}
