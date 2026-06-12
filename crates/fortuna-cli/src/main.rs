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
//!   fortuna start  [--foreground] [--config-path <path>]
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
    foreground: bool,
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
        foreground: false,
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
            "--foreground" => args.foreground = true,
            other if args.command.is_empty() => args.command = other.to_string(),
            other => args.positional.push(other.to_string()),
        }
        i += 1;
    }
    if args.command.is_empty() {
        bail!(
            "usage: fortuna <status|halt|rearm|kill|config check|logs|start> \
             [scope|component] [--reason ..] [--operator ..] [--journal ..] \
             [--flatten] [--config-path ..] [-f] [--foreground]"
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
        "start" => start_cmd(&args),
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

/// What an EXISTING pidfile means to `start` (A3 validate-then-decide).
#[derive(Debug, PartialEq, Eq)]
enum ExistingPidfile {
    /// Parsed, alive, and the live process's comm contains the claimed
    /// name — genuinely ours and running.
    Running { pid: i64 },
    /// Dead pid, name mismatch (PID reuse), or unparseable junk: safe to
    /// remove and reclaim.
    Stale,
    /// EMPTY file: another `start` has claimed it and not yet written the
    /// pid — contended, never steal it.
    MidClaim,
}

/// Classify pidfile CONTENT against an injectable comm lookup (the real
/// lookup is [`comm_of`]; tests inject fakes — process liveness is not
/// deterministic from a unit test).
fn classify_existing(
    content: &str,
    comm_lookup: &dyn Fn(i64) -> Option<String>,
) -> ExistingPidfile {
    if content.trim().is_empty() {
        return ExistingPidfile::MidClaim;
    }
    let mut lines = content.lines();
    let pid = lines.next().and_then(|l| l.trim().parse::<i64>().ok());
    let name = lines.next().map(str::trim).filter(|n| !n.is_empty());
    let (pid, name) = match (pid, name) {
        (Some(pid), Some(name)) => (pid, name),
        _ => return ExistingPidfile::Stale,
    };
    match comm_lookup(pid) {
        Some(comm) if comm.contains(name) => ExistingPidfile::Running { pid },
        _ => ExistingPidfile::Stale,
    }
}

/// A3 atomic claim: O_EXCL create. Exactly one concurrent `start` wins;
/// the losers see EEXIST and go through validate-then-decide.
fn claim_pidfile(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// A4: APPEND-mode log redirection — never truncate; crash backtraces
/// survive restarts.
fn open_log_append(dir: &Path, component: &str) -> Result<std::fs::File> {
    let path = log_path(dir, component);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating log dir {}", parent.display()))?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening log {} for append", path.display()))
}

/// Claim-then-spawn (A3 order: the pidfile is claimed BEFORE the process
/// exists; the pid is written into the claimed file after). Detach per A4:
/// own process group, no stdin, append-redirected stdio. Clears any stale
/// A7 stopping marker so a restart never reads as "stopping". On spawn
/// failure the claim is released.
fn spawn_component(
    dir: &Path,
    component: &str,
    claimed_name: &str,
    mut cmd: std::process::Command,
) -> Result<(i64, std::process::Child)> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating runtime dir {}", dir.display()))?;
    let pidpath = pidfile_path(dir, component);
    let mut claimed = claim_pidfile(&pidpath)
        .with_context(|| format!("claiming pidfile {}", pidpath.display()))?;
    let _ = std::fs::remove_file(dir.join(format!("{component}.stopping")));
    let result = (|| {
        let log = open_log_append(dir, component)?;
        let log_err = log.try_clone().context("cloning log handle for stderr")?;
        use std::os::unix::process::CommandExt;
        cmd.stdin(std::process::Stdio::null())
            .stdout(log)
            .stderr(log_err)
            .process_group(0);
        let child = cmd
            .spawn()
            .with_context(|| format!("spawning {component}"))?;
        let pid = i64::from(child.id());
        use std::io::Write;
        write!(claimed, "{pid}\n{claimed_name}\n")
            .with_context(|| format!("writing pidfile {}", pidpath.display()))?;
        Ok((pid, child))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&pidpath);
    }
    result
}

/// Component binary resolution: FORTUNA_BIN_DIR first (operator pin + test
/// seam), then siblings of this executable (standard cargo target layout),
/// then PATH.
fn resolve_component_binary(bin_name: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os("FORTUNA_BIN_DIR") {
        let candidate = PathBuf::from(dir).join(bin_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(bin_name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(bin_name)
}

/// The recorder invocation (A2: pinned — interval 30s, the three bracket
/// series, ABSOLUTE out-dir). An optional `[recorder]` table in the config
/// overrides; the defaults are the recorded live invocation verbatim.
fn recorder_invocation(config_path: &str) -> Result<Vec<String>> {
    let mut interval_secs: i64 = 30;
    let mut bracket_series = "KXBTC15M,KXBTC,KXBTCD".to_string();
    let mut out_dir = "data/perishable".to_string();
    if let Ok(text) = std::fs::read_to_string(config_path) {
        if let Ok(value) = toml::from_str::<toml::Value>(&text) {
            if let Some(rec) = value.get("recorder") {
                if let Some(v) = rec.get("interval_secs").and_then(|v| v.as_integer()) {
                    interval_secs = v;
                }
                if let Some(v) = rec.get("bracket_series").and_then(|v| v.as_str()) {
                    bracket_series = v.to_string();
                }
                if let Some(v) = rec.get("out_dir").and_then(|v| v.as_str()) {
                    out_dir = v.to_string();
                }
            }
        }
    }
    // A2: the out-dir must be ABSOLUTE — a relative path under a wrong cwd
    // would silently fork the sacred B0 dataset.
    let out_path = PathBuf::from(&out_dir);
    let abs_out = if out_path.is_absolute() {
        out_path
    } else {
        std::env::current_dir()
            .context("resolving cwd for the recorder out-dir")?
            .join(out_path)
    };
    Ok(vec![
        "--interval-secs".to_string(),
        interval_secs.to_string(),
        "--bracket-series".to_string(),
        bracket_series,
        "--out-dir".to_string(),
        abs_out.display().to_string(),
    ])
}

/// A2: every fortuna-recorder process NOT accounted for by the managed
/// pidfile. pgrep failing to run is a refusal, not a pass — fail closed.
fn unmanaged_recorder_pids(managed: Option<i64>) -> Result<Vec<i64>> {
    let out = std::process::Command::new("pgrep")
        .args(["-f", "fortuna-recorder"])
        .output()
        .context("running pgrep (required for the recorder-collision check)")?;
    // pgrep exits 1 with empty output when nothing matches.
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text
        .lines()
        .filter_map(|l| l.trim().parse::<i64>().ok())
        .filter(|pid| Some(*pid) != managed)
        .collect())
}

/// `fortuna start [--foreground] [--config-path <p>]` (design Section 5 as
/// amended): config check -> already-running (idempotent exit 0) -> A2
/// recorder-collision refusal -> claim + spawn missing components -> A8
/// halt visibility + best-effort lifecycle audit row. No migration
/// pre-flight (A6: the daemon's boot connect auto-migrates; a refusal the
/// boot path overrides is theater).
fn start_cmd(args: &Args) -> Result<()> {
    let config_path = args
        .config_path
        .clone()
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());
    fortuna_ops::FortunaConfig::load_file(&config_path)
        .with_context(|| format!("config check failed for {config_path}"))?;

    if args.foreground {
        // Debugging mode: the daemon owns the terminal. No pidfile, no
        // recorder, no detach — exec replaces this process entirely.
        let bin = resolve_component_binary("fortuna-live");
        println!("foreground: exec {} {config_path}", bin.display());
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&bin).arg(&config_path).exec();
        bail!("exec {} failed: {err}", bin.display());
    }

    let dir = runtime_dir();
    let mut running: Vec<(&str, i64)> = Vec::new();
    let mut to_start: Vec<&str> = Vec::new();
    for component in COMPONENTS {
        let pidpath = pidfile_path(&dir, component);
        match std::fs::read_to_string(&pidpath) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => to_start.push(component),
            Err(e) => {
                return Err(e).with_context(|| format!("reading pidfile {}", pidpath.display()))
            }
            Ok(content) => match classify_existing(&content, &comm_of) {
                ExistingPidfile::Running { pid } => {
                    println!("{component}: already running (pid {pid})");
                    running.push((component, pid));
                }
                ExistingPidfile::Stale => {
                    std::fs::remove_file(&pidpath)
                        .with_context(|| format!("removing stale pidfile {}", pidpath.display()))?;
                    to_start.push(component);
                }
                ExistingPidfile::MidClaim => bail!(
                    "another start appears to be in progress (pidfile {} is claimed \
                     but empty); re-run shortly, or remove it if you are certain",
                    pidpath.display()
                ),
            },
        }
    }
    if to_start.is_empty() {
        println!("already running — nothing to start");
        return Ok(());
    }

    // A2: never adopt, never double-spawn. Two appenders can tear JSONL
    // lines in the B0 dataset, so an unmanaged recorder refuses the WHOLE
    // start (conservative: even a daemon-only spawn) until the operator
    // migrates.
    let managed_recorder = running
        .iter()
        .find(|(c, _)| *c == "recorder")
        .map(|(_, pid)| *pid);
    let unmanaged = unmanaged_recorder_pids(managed_recorder)?;
    if !unmanaged.is_empty() {
        bail!(
            "refusing to start: unmanaged fortuna-recorder process(es) {unmanaged:?} \
             (two appenders can tear JSONL lines in the B0 dataset). One-time \
             migration: stop the manual recorder, then re-run `fortuna start` — \
             the recorder runs managed (pidfile + log redirect) from then on"
        );
    }

    let mut started: Vec<(&str, i64)> = Vec::new();
    for component in &to_start {
        let (bin_name, cmd) = match *component {
            "daemon" => {
                let bin = resolve_component_binary("fortuna-live");
                let mut cmd = std::process::Command::new(&bin);
                cmd.arg(&config_path);
                ("fortuna-live", cmd)
            }
            _ => {
                let bin = resolve_component_binary("fortuna-recorder");
                let mut cmd = std::process::Command::new(&bin);
                cmd.args(recorder_invocation(&config_path)?);
                ("fortuna-recorder", cmd)
            }
        };
        let (pid, child) = spawn_component(&dir, component, bin_name, cmd)?;
        // The child is detached (own process group, redirected stdio);
        // dropping the handle does not signal it.
        drop(child);
        println!("started {component} (pid {pid})");
        started.push((component, pid));
    }

    // A8 + A10: halt visibility and the lifecycle audit row are advisory
    // attribution — best-effort, bounded, never a start blocker (the
    // daemon's own boot audit row is the I5 record).
    match std::env::var("DATABASE_URL") {
        Err(_) => {
            println!("db: DATABASE_URL not set — halt visibility and lifecycle audit skipped")
        }
        Ok(url) => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio runtime")?;
            let all: Vec<(&str, i64)> = running.iter().chain(started.iter()).copied().collect();
            let bounded = runtime.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(STATUS_DB_TIMEOUT_SECS),
                    start_db_section(&url, &all),
                )
                .await
            });
            match bounded {
                Ok(Ok(())) => {}
                Ok(Err(e)) => println!("db: unavailable — {e:#} (start unaffected)"),
                Err(_) => println!(
                    "db: unavailable — no response within {STATUS_DB_TIMEOUT_SECS}s \
                     (start unaffected)"
                ),
            }
        }
    }
    Ok(())
}

/// Active halts (I2 visibility at start, A8) + the best-effort `lifecycle`
/// audit row (A10: advisory attribution, `$USER` as actor).
async fn start_db_section(url: &str, components: &[(&str, i64)]) -> Result<()> {
    let pool = fortuna_ledger::connect(url).await?;
    let halts = HaltsRepo::new(pool.clone());
    let clock = RealClock;
    let now = clock.now();
    let active = halts.active().await?;
    if active.is_empty() {
        println!("active halts: none");
    } else {
        println!(
            "ACTIVE HALTS ({}) — the daemon will not trade until re-armed:",
            active.len()
        );
        for (scope, reason) in active {
            println!("  {} — {reason}", fortuna_ledger::halt_scope_string(&scope));
        }
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let audit = AuditWriter::new(
        pool,
        std::sync::Arc::new(RealClock),
        now.epoch_millis() as u64,
    );
    let mut payload = serde_json::json!({"action": "start"});
    for (component, pid) in components {
        payload[format!("{component}_pid")] = serde_json::json!(pid);
    }
    audit
        .append("lifecycle", Some(&user), None, payload)
        .await?;
    Ok(())
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    //! Unit tests for the A3/A4 primitives whose properties are not
    //! deterministically testable through the binary: claim atomicity
    //! (a race), append-mode redirection (never truncate), pidfile
    //! classification (injected comm lookup), and the claim-then-spawn
    //! sequence (pidfile content + stopping-marker clear).

    use super::*;

    fn scratch(case: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fortuna-cli-unit-{}-{case}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn classify_empty_is_mid_claim() {
        let lookup = |_: i64| -> Option<String> { panic!("must not look up a mid-claim") };
        assert_eq!(classify_existing("", &lookup), ExistingPidfile::MidClaim);
        assert_eq!(
            classify_existing("  \n", &lookup),
            ExistingPidfile::MidClaim
        );
    }

    #[test]
    fn classify_garbage_is_stale() {
        let lookup = |_: i64| -> Option<String> { Some("anything".to_string()) };
        assert_eq!(
            classify_existing("not-a-pid\n", &lookup),
            ExistingPidfile::Stale
        );
        assert_eq!(classify_existing("123\n", &lookup), ExistingPidfile::Stale);
        assert_eq!(
            classify_existing("123\n  \n", &lookup),
            ExistingPidfile::Stale
        );
    }

    #[test]
    fn classify_dead_pid_is_stale() {
        let lookup = |_: i64| -> Option<String> { None };
        assert_eq!(
            classify_existing("123\nfortuna-live\n", &lookup),
            ExistingPidfile::Stale
        );
    }

    #[test]
    fn classify_name_mismatch_is_stale() {
        // A3: PID reuse — alive, but it is not our process.
        let lookup = |_: i64| -> Option<String> { Some("/usr/bin/vim".to_string()) };
        assert_eq!(
            classify_existing("123\nfortuna-live\n", &lookup),
            ExistingPidfile::Stale
        );
    }

    #[test]
    fn classify_match_is_running() {
        let lookup = |pid: i64| -> Option<String> {
            assert_eq!(pid, 123);
            Some("/repo/target/release/fortuna-live".to_string())
        };
        assert_eq!(
            classify_existing("123\nfortuna-live\n", &lookup),
            ExistingPidfile::Running { pid: 123 }
        );
    }

    #[test]
    fn claim_pidfile_race_has_exactly_one_winner() {
        // A9: two starts, one wins. Raced wider than two for good measure.
        let dir = scratch("claim-race");
        let path = dir.join("daemon.pid");
        let winners: usize = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| s.spawn(|| claim_pidfile(&path).is_ok()))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(|&won| won)
                .count()
        });
        assert_eq!(winners, 1, "O_EXCL must admit exactly one claimant");
    }

    #[test]
    fn log_redirection_appends_and_never_truncates() {
        // A4: a restart must not erase the previous run's backtrace.
        use std::io::Write;
        let dir = scratch("append-log");
        let mut first = open_log_append(&dir, "daemon").unwrap();
        writeln!(first, "first-run line").unwrap();
        drop(first);
        let mut second = open_log_append(&dir, "daemon").unwrap();
        writeln!(second, "second-run line").unwrap();
        drop(second);
        let text = std::fs::read_to_string(log_path(&dir, "daemon")).unwrap();
        assert!(text.contains("first-run line"), "log was truncated: {text}");
        assert!(text.contains("second-run line"), "append failed: {text}");
    }

    #[test]
    fn spawn_component_writes_pidfile_and_clears_stopping_marker() {
        let dir = scratch("spawn");
        std::fs::write(dir.join("daemon.stopping"), "").unwrap();
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("300");
        let (pid, mut child) = spawn_component(&dir, "daemon", "sleep", cmd).unwrap();
        let content = std::fs::read_to_string(pidfile_path(&dir, "daemon")).unwrap();
        assert_eq!(content, format!("{pid}\nsleep\n"));
        assert!(
            !dir.join("daemon.stopping").exists(),
            "a fresh start must clear the stale A7 stopping marker"
        );
        // Test cleanup only — the real stop path never SIGKILLs.
        child.kill().unwrap();
        let _ = child.wait();
    }

    #[test]
    fn spawn_component_releases_claim_on_spawn_failure() {
        let dir = scratch("spawn-fail");
        let cmd = std::process::Command::new("/nonexistent/binary/for/this/test");
        let result = spawn_component(&dir, "daemon", "ghost", cmd);
        assert!(result.is_err());
        assert!(
            !pidfile_path(&dir, "daemon").exists(),
            "a failed spawn must release the pidfile claim"
        );
    }
}
