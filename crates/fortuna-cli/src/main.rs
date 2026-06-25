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
//!   fortuna start  [paper-demo] [--foreground] [--config-path <path>]
//!   fortuna stop   [--timeout-secs N]
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
use fortuna_cli::backtest_cmd;
use fortuna_cli::doctor as doctor_mod;
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
    timeout_secs: Option<String>,
    flatten: bool,
    follow: bool,
    foreground: bool,
    // S7: backtest / validate flags
    from: Option<String>,
    to: Option<String>,
    scope: Option<String>,
    producer: Option<String>,
    archive: Option<String>,
    // W3: doctor flag
    offline: bool,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        command: String::new(),
        positional: Vec::new(),
        reason: None,
        operator: None,
        journal: None,
        config_path: None,
        timeout_secs: None,
        flatten: false,
        follow: false,
        foreground: false,
        from: None,
        to: None,
        scope: None,
        producer: None,
        archive: None,
        offline: false,
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
            "--timeout-secs" => {
                i += 1;
                args.timeout_secs = raw.get(i).cloned();
            }
            "--flatten" => args.flatten = true,
            "-f" | "--follow" => args.follow = true,
            "--foreground" => args.foreground = true,
            "--offline" => args.offline = true,
            "--from" => {
                i += 1;
                args.from = raw.get(i).cloned();
            }
            "--to" => {
                i += 1;
                args.to = raw.get(i).cloned();
            }
            "--scope" => {
                i += 1;
                args.scope = raw.get(i).cloned();
            }
            "--producer" => {
                i += 1;
                args.producer = raw.get(i).cloned();
            }
            "--archive" => {
                i += 1;
                args.archive = raw.get(i).cloned();
            }
            other if args.command.is_empty() => args.command = other.to_string(),
            other => args.positional.push(other.to_string()),
        }
        i += 1;
    }
    if args.command.is_empty() {
        bail!(
            "usage: fortuna <status|halt|rearm|kill|config check|logs|start|stop|\
             backtest|validate|doctor> \
             [scope|component|paper-demo] [--reason ..] [--operator ..] \
             [--journal ..] [--flatten] [--config-path ..] [-f] [--foreground] \
             [--timeout-secs N] [--from <date>] [--to <date>] [--scope <scope>] \
             [--producer <name>] [--archive <path>] [--offline]"
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
        "stop" => stop_cmd(&args),
        "doctor" => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio runtime")?;
            runtime.block_on(doctor_cmd(&args))
        }
        "halt" | "rearm" | "backtest" | "validate" => {
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

/// A2 (orphaned minor F-2): lifecycle paths anchor to the REPO ROOT
/// derived from the config path — the parent of its `config/` directory —
/// never the invoker's cwd. A `fortuna start` from the wrong directory
/// must not re-anchor data/ paths (a cwd-relative recorder out-dir would
/// silently fork the B0 dataset). A config kept outside a `config/` dir
/// anchors to the file's own directory; a relative path resolves against
/// the cwd first (identical to today for repo-root invocations).
fn repo_root_from_config(config_path: &Path) -> PathBuf {
    let cwd = || std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let abs = if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        cwd().join(config_path)
    };
    let parent = abs.parent().map(Path::to_path_buf).unwrap_or_else(cwd);
    if parent.file_name().map(|n| n == "config").unwrap_or(false) {
        parent.parent().map(Path::to_path_buf).unwrap_or_else(cwd)
    } else {
        parent
    }
}

/// Runtime state directory (A5): pidfiles + redirected logs, anchored to
/// the repo root (F-2). data/ is gitignored and survives reboots, unlike
/// /tmp on macOS. The env override always wins (operator pin + tests).
fn runtime_dir(root: &Path) -> PathBuf {
    std::env::var_os("FORTUNA_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("data/runtime"))
}

/// The resolved config path (--config-path or the default), shared by
/// every lifecycle command so they all derive the SAME root.
fn resolved_config_path(args: &Args) -> String {
    args.config_path
        .clone()
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string())
}

fn pidfile_path(dir: &Path, component: &str) -> PathBuf {
    dir.join(format!("{component}.pid"))
}

fn log_path(dir: &Path, component: &str) -> PathBuf {
    dir.join("logs").join(format!("{component}.log"))
}

/// `ps -p <pid> -o stat= -o comm=` — one call answers liveness and identity
/// (A3: macOS reuses PIDs; a live pid is trusted only if its command path
/// contains the name the pidfile claims). None = not running. A ZOMBIE
/// (stat Z*: exited, unreaped by its parent) reads as not running — it is
/// not signalable work, and `stop` must see its exit as an exit.
fn comm_of(pid: i64) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat=", "-o", "comm="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.trim();
    let mut parts = line.split_whitespace();
    let stat = parts.next()?;
    if stat.starts_with('Z') {
        return None;
    }
    let comm = parts.collect::<Vec<_>>().join(" ");
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
    // A2 + F-2: the out-dir must be ABSOLUTE and anchored to the CONFIG-
    // derived repo root, never the invoker's cwd — a relative path under a
    // wrong cwd would silently fork the sacred B0 dataset.
    let out_path = PathBuf::from(&out_dir);
    let abs_out = if out_path.is_absolute() {
        out_path
    } else {
        repo_root_from_config(Path::new(config_path)).join(out_path)
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

/// `fortuna start [paper-demo] [--foreground] [--config-path <p>]`
/// (design Section 5 as amended, W4): config check -> already-running
/// (idempotent exit 0) -> A2 recorder-collision refusal -> claim + spawn
/// missing components -> A8 halt visibility + best-effort lifecycle audit row.
///
/// ## paper-demo sub-mode (W4)
///
/// `fortuna start paper-demo` spawns the daemon under `execution_mode =
/// "paper_ledger"` (set in the config, enforced by `validate_bootable`); no
/// real-venue order is ever placed (the paper-live safety wall,
/// `i_paper_live_no_real_order`). The F11 pointer-write
/// (`data/runtime/current-demo-db-url`) is written by the **daemon** on boot
/// (after its Postgres pool connects) — not by this CLI — so the pointer
/// always reflects the URL the daemon actually booted with.
///
/// The mode is a CONFIG discipline: set `execution_mode = "paper_ledger"` in
/// `[runtime]` before running this. When the `paper-demo` positional is present
/// this command HARD-ASSERTS that mode ([`assert_paper_demo_safe`]) and FAILS
/// LOUDLY (non-zero exit) if the resolved config is not paper-safe — so an
/// operator can never footgun a non-paper "demo". Everything else (spawn) is
/// identical to a normal `start`.
/// W6a (Phase B Adv): when `fortuna start paper-demo` is invoked, HARD-ASSERT
/// the resolved config is paper-safe — `[runtime].execution_mode == "paper_ledger"`
/// — and FAIL LOUDLY otherwise so an operator can never footgun a non-paper
/// "demo". Reads `[runtime].execution_mode` as a raw `toml::Value` (the same
/// pattern `config_on_disk` uses; `[runtime]` is the daemon's section, not
/// `FortunaConfig`'s, and the cli has no runtime dep on fortuna-live). Fail
/// closed: a missing `[runtime]`/`execution_mode`, or an unreadable/unparseable
/// config, refuses. Pure (path-only) so it is unit-tested without a daemon.
fn assert_paper_demo_safe(config_path: &str) -> Result<()> {
    const REQUIRED: &str = "paper_ledger";
    let text = std::fs::read_to_string(config_path).with_context(|| {
        format!(
            "refusing `start paper-demo`: cannot read config {config_path} to verify \
             execution_mode == {REQUIRED:?}"
        )
    })?;
    let value: toml::Value = toml::from_str(&text).with_context(|| {
        format!("refusing `start paper-demo`: cannot parse config {config_path}")
    })?;
    let mode = value
        .get("runtime")
        .and_then(|r| r.get("execution_mode"))
        .and_then(|v| v.as_str());
    match mode {
        Some(REQUIRED) => Ok(()),
        Some(other) => bail!(
            "refusing `start paper-demo`: [runtime].execution_mode is {other:?}, but \
             paper-demo requires {REQUIRED:?} (live data + LOCAL paper execution; no \
             real-venue order). Set execution_mode = \"{REQUIRED}\" in {config_path}, \
             or run plain `fortuna start` for a non-demo mode."
        ),
        None => bail!(
            "refusing `start paper-demo`: {config_path} has no [runtime].execution_mode. \
             paper-demo requires an explicit execution_mode = \"{REQUIRED}\" (fail closed: \
             a demo never assumes paper)."
        ),
    }
}

fn start_cmd(args: &Args) -> Result<()> {
    let config_path = resolved_config_path(args);
    fortuna_ops::FortunaConfig::load_file(&config_path)
        .with_context(|| format!("config check failed for {config_path}"))?;
    // W6a: `fortuna start paper-demo` must be paper-safe by HARD ASSERTION (the
    // positional was previously unread — a doc-only alias). Fail loudly if the
    // resolved config is not execution_mode = "paper_ledger".
    if args.positional.first().map(String::as_str) == Some("paper-demo") {
        assert_paper_demo_safe(&config_path)?;
    }
    let root = repo_root_from_config(Path::new(&config_path));

    if args.foreground {
        // Debugging mode: the daemon owns the terminal. No pidfile, no
        // recorder, no detach — exec replaces this process entirely.
        let bin = resolve_component_binary("fortuna-live");
        println!("foreground: exec {} {config_path}", bin.display());
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&bin).arg(&config_path).exec();
        bail!("exec {} failed: {err}", bin.display());
    }

    let dir = runtime_dir(&root);
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
        let (bin_name, mut cmd) = match *component {
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
        // A2/F-2: spawn cwd pinned to the repo root, never the invoker's
        // cwd (the daemon's own .env load and any remaining relative path
        // resolve identically to a repo-root launch).
        cmd.current_dir(&root);
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

/// The daemon's graceful-exit marker (fortuna-live/src/main.rs prints it to
/// stderr on the clean path; `start`'s redirect lands it in the daemon log).
/// A1: `stop` succeeds only when this appears in the log AFTER the signal.
const DAEMON_SHUTDOWN_MARKER: &str = "fortuna-live: clean shutdown";

/// SIGTERM via shell-out (`nix` is not a workspace dep; std's Child::kill
/// is SIGKILL and is never acceptable here — GAPS T4.4 records the call).
fn send_sigterm(pid: i64) -> Result<()> {
    let status = std::process::Command::new("kill")
        .args(["-15", &pid.to_string()])
        .status()
        .context("running kill -15")?;
    if !status.success() {
        bail!("kill -15 {pid} exited with {status}");
    }
    Ok(())
}

/// Does `needle` appear in `path` at or after byte `offset`? The log is
/// APPEND-mode across runs (A4), so only bytes written after the signal
/// count — a previous run's marker must never satisfy A1.
fn log_contains_after(path: &Path, offset: u64, needle: &str) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return false;
    }
    String::from_utf8_lossy(&buf).contains(needle)
}

/// `fortuna stop [--timeout-secs N]` (design Section 5 + A1/A7/A10):
/// SIGTERM daemon then recorder, never SIGKILL, idempotent. Daemon success
/// requires the clean-shutdown line in the log AFTER the signal (A1 —
/// process exit alone is not success). Timeout leaves the process, the
/// pidfile, and the A7 stopping marker, warns, and STILL proceeds to the
/// recorder. The lifecycle audit row is best-effort: a dead DB can never
/// block a shutdown.
fn stop_cmd(args: &Args) -> Result<()> {
    let timeout_secs: i64 = match &args.timeout_secs {
        None => 60,
        Some(raw) => raw
            .parse()
            .with_context(|| format!("--timeout-secs {raw:?} is not a number"))?,
    };
    let dir = runtime_dir(&repo_root_from_config(Path::new(&resolved_config_path(
        args,
    ))));
    let clock = RealClock;
    let mut warnings = 0usize;
    let mut stopped: Vec<(&str, i64)> = Vec::new();
    for component in COMPONENTS {
        let pidpath = pidfile_path(&dir, component);
        let content = match std::fs::read_to_string(&pidpath) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("{component}: already stopped");
                continue;
            }
            Err(e) => {
                return Err(e).with_context(|| format!("reading pidfile {}", pidpath.display()))
            }
            Ok(content) => content,
        };
        let pid = match classify_existing(&content, &comm_of) {
            ExistingPidfile::Stale => {
                std::fs::remove_file(&pidpath)
                    .with_context(|| format!("removing stale pidfile {}", pidpath.display()))?;
                println!("{component}: already stopped (stale pidfile removed)");
                continue;
            }
            ExistingPidfile::MidClaim => {
                eprintln!(
                    "fortuna: {component} pidfile {} is claimed but empty — a start \
                     appears to be in progress; re-run stop after it finishes",
                    pidpath.display()
                );
                warnings += 1;
                continue;
            }
            ExistingPidfile::Running { pid } => pid,
        };
        // A7: marker first, so status shows "stopping since T" throughout.
        let marker = dir.join(format!("{component}.stopping"));
        std::fs::write(&marker, b"")
            .with_context(|| format!("writing stopping marker {}", marker.display()))?;
        // A1: capture the append-log offset BEFORE the signal.
        let logfile = log_path(&dir, component);
        let log_offset = std::fs::metadata(&logfile).map(|m| m.len()).unwrap_or(0);
        send_sigterm(pid)?;
        let deadline = clock
            .now()
            .epoch_millis()
            .saturating_add(timeout_secs * 1000);
        let exited = loop {
            if comm_of(pid).is_none() {
                break true;
            }
            if clock.now().epoch_millis() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        if !exited {
            // A7 verbatim guidance; the process, pidfile, and marker stay
            // for the operator. NEVER SIGKILL — the daemon is cancelling
            // working orders.
            eprintln!(
                "fortuna: {component} (pid {pid}) did not exit within {timeout_secs}s — \
                 daemon is cancelling working orders — do NOT kill -9; watch \
                 `fortuna logs daemon`; if the venue is unreachable use `fortuna kill`"
            );
            warnings += 1;
            continue;
        }
        std::fs::remove_file(&pidpath)
            .with_context(|| format!("removing pidfile {}", pidpath.display()))?;
        let _ = std::fs::remove_file(&marker);
        if component == "daemon" {
            if log_contains_after(&logfile, log_offset, DAEMON_SHUTDOWN_MARKER) {
                println!("daemon: stopped (clean shutdown confirmed in the log)");
            } else {
                // A1: exit alone is not success.
                eprintln!(
                    "fortuna: daemon (pid {pid}) exited but no shutdown line \
                     ({DAEMON_SHUTDOWN_MARKER:?}) appeared in {} after the signal — \
                     crash-style exit? Check `fortuna logs daemon` and the audit trail",
                    logfile.display()
                );
                warnings += 1;
            }
        } else {
            println!("{component}: stopped");
        }
        stopped.push((component, pid));
    }
    // A10: advisory attribution only — a dead DB never blocks a shutdown.
    if !stopped.is_empty() {
        match std::env::var("DATABASE_URL") {
            Err(_) => println!("db: DATABASE_URL not set — lifecycle audit skipped"),
            Ok(url) => {
                let bounded = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("tokio runtime")
                    .map(|rt| {
                        rt.block_on(async {
                            tokio::time::timeout(
                                std::time::Duration::from_secs(STATUS_DB_TIMEOUT_SECS),
                                stop_db_section(&url, &stopped),
                            )
                            .await
                        })
                    });
                match bounded {
                    Ok(Ok(Ok(()))) => {}
                    Ok(Ok(Err(e))) => println!("db: unavailable — {e:#} (stop unaffected)"),
                    Ok(Err(_)) => println!(
                        "db: unavailable — no response within {STATUS_DB_TIMEOUT_SECS}s \
                         (stop unaffected)"
                    ),
                    Err(e) => println!("db: unavailable — {e:#} (stop unaffected)"),
                }
            }
        }
    }
    if warnings > 0 {
        bail!("stop completed with {warnings} warning(s) — see above");
    }
    Ok(())
}

/// Best-effort `lifecycle` stop row (A10).
async fn stop_db_section(url: &str, components: &[(&str, i64)]) -> Result<()> {
    let pool = fortuna_ledger::connect(url).await?;
    let clock = RealClock;
    let now = clock.now();
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let audit = AuditWriter::new(
        pool,
        std::sync::Arc::new(RealClock),
        now.epoch_millis() as u64,
    );
    let mut payload = serde_json::json!({"action": "stop"});
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
    let root = repo_root_from_config(Path::new(&resolved_config_path(args)));
    let path = log_path(&runtime_dir(&root), component);
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
    let config_path = resolved_config_path(args);
    let dir = runtime_dir(&repo_root_from_config(Path::new(&config_path)));
    println!("processes:");
    for component in COMPONENTS {
        println!("  {component}: {}", process_state_line(&dir, component));
    }
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

/// A8 (orphaned minor F-1): the age of the MOST RECENT audit row — a
/// stale age beside a live daemon pidfile is the crash tell. Unparseable
/// timestamps degrade honestly; ages render in seconds under two
/// minutes, whole minutes beyond.
fn format_audit_age(now: UtcTimestamp, at: &str, kind: &str) -> String {
    match UtcTimestamp::parse_iso8601(at) {
        Err(_) => format!("most recent audit row: at unparseable ({at:?}, kind {kind})"),
        Ok(t) => {
            let secs = (now.epoch_millis() - t.epoch_millis()).max(0) / 1000;
            let age = if secs < 120 {
                format!("{secs}s ago")
            } else {
                format!("{}m ago", secs / 60)
            };
            format!("most recent audit row: {age} (kind {kind})")
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
    // A8 crash tell (F-1): the newest row of ANY kind. A stale age while
    // the process section shows a live daemon = the daemon stopped
    // writing — investigate before trusting anything above.
    match audit.latest_at().await? {
        Some(latest) => println!("{}", format_audit_age(now, &latest.at, &latest.kind)),
        None => println!("most recent audit row: none yet"),
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

/// `fortuna doctor [--offline] [--config-path <p>]`
///
/// Runs the readiness checklist and exits non-zero if any check is red.
/// `--offline` skips the network source-reachability probe (useful in CI).
async fn doctor_cmd(args: &Args) -> Result<()> {
    let url =
        std::env::var("DATABASE_URL").context("DATABASE_URL is required for fortuna doctor")?;
    let pool = fortuna_ledger::connect(&url).await?;

    // Snapshot the real process env for the cred check (values NEVER printed).
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();

    let opts = doctor_mod::DoctorOpts {
        env,
        offline: args.offline,
        config_path: args.config_path.clone(),
    };
    let report = doctor_mod::run(&pool, &opts).await;
    doctor_mod::print_report(&report);

    if report.all_green {
        Ok(())
    } else {
        // Non-zero exit; anyhow error message gives the operator the cue.
        bail!("doctor: one or more checks FAILED (see checklist above)");
    }
}

async fn db_command(args: &Args) -> Result<()> {
    let url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL is required for halt/rearm/backtest/validate \
         (kill and the read commands work without it)",
    )?;
    let pool = fortuna_ledger::connect(&url).await?;
    let halts = HaltsRepo::new(pool.clone());
    let clock = RealClock;
    let now = clock.now();

    match args.command.as_str() {
        "backtest" => {
            let source_name = args
                .positional
                .first()
                .cloned()
                .unwrap_or_else(|| "aeolus-archive".to_string());
            let from = parse_optional_ts(args.from.as_deref(), "--from")?;
            let to = parse_optional_ts(args.to.as_deref(), "--to")?;
            // `--archive` is the source path: a publish directory for `alexandria`,
            // else the `aeolus_kalshi.db` file (with the FORTUNA_WS3_ARCHIVE fallback).
            let (real_db_path, archive_dir) = if source_name == "alexandria" {
                (None, args.archive.as_ref().map(PathBuf::from))
            } else {
                (
                    args.archive
                        .as_ref()
                        .map(PathBuf::from)
                        .or_else(|| std::env::var_os("FORTUNA_WS3_ARCHIVE").map(PathBuf::from)),
                    None,
                )
            };
            let bt_args = backtest_cmd::BacktestArgs {
                source_name,
                sql_fixture_path: None,
                real_db_path,
                archive_dir,
                from,
                to,
            };
            // min_n=3 matches the WS2 default; a future config key can override.
            let report = backtest_cmd::run_backtest(&pool, &bt_args, clock, 3).await?;
            println!(
                "backtest complete: written={} skipped_idempotent={} look_ahead_rejected={}",
                report.written, report.skipped_idempotent, report.look_ahead_rejected
            );
            if let Some(card) = &report.scorecard {
                println!(
                    "parity scorecard: scope={} n={} verdict={:?}",
                    card.scope, card.n, card.go.decision
                );
            }
            Ok(())
        }
        "validate" => {
            let scope = args
                .scope
                .clone()
                .context("--scope <scope> is required for fortuna validate")?;
            let source_name = args
                .positional
                .first()
                .cloned()
                .unwrap_or_else(|| "aeolus-archive".to_string());
            // W7: the same source the `backtest` command uses supplies the real
            // replayed track record for the edge series. When absent, validate is
            // honestly `Insufficient`-by-construction (no track record in scope).
            // `--archive` is a publish directory for `alexandria`, else the
            // `aeolus_kalshi.db` file (with the FORTUNA_WS3_ARCHIVE fallback).
            let (archive_path, archive_dir) = if source_name == "alexandria" {
                (None, args.archive.as_ref().map(PathBuf::from))
            } else {
                (
                    args.archive
                        .as_ref()
                        .map(PathBuf::from)
                        .or_else(|| std::env::var_os("FORTUNA_WS3_ARCHIVE").map(PathBuf::from)),
                    None,
                )
            };
            let v_args = backtest_cmd::ValidateArgs {
                scope,
                producer: args.producer.clone(),
                source_name,
                sql_fixture_path: None,
                archive_path,
                archive_dir,
            };
            let output = backtest_cmd::run_validate(&pool, &v_args, clock).await?;
            println!("{output}");
            Ok(())
        }
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
                // I4 (E6): REFUSE the re-arm while the kill-switch revocation
                // sentinel stands (or its state is unverifiable). This runs
                // BEFORE record_rearm so a standing kill can never be re-armed
                // back into order-placing capability via the ledger path.
                rearm_revocation_precondition(&resolved_config_path(args))?;
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
                println!("{}", rearm_success_message(scope_raw, &operator));
            }
            Ok(())
        }
        _ => bail!("unreachable command in db_command"),
    }
}

/// Parse an optional ISO8601 / date string into a `UtcTimestamp`.
fn parse_optional_ts(raw: Option<&str>, flag: &str) -> Result<Option<UtcTimestamp>> {
    match raw {
        None => Ok(None),
        Some(s) => UtcTimestamp::parse_iso8601(s)
            .or_else(|_| UtcTimestamp::parse_iso8601_or_date(s))
            .map(Some)
            .with_context(|| format!("{flag} value {s:?} is not a valid ISO8601 date/timestamp")),
    }
}

/// I4 (E6) re-arm precondition: REFUSE the re-arm while a kill sentinel stands
/// (or while its state is unverifiable). Reads `[killswitch].revocation_file`
/// from the config on disk (raw `toml::Value` — the same pattern `config_on_disk`
/// uses, because `[killswitch]` is the daemon's section, not `FortunaConfig`'s)
/// and applies the THREE-WAY [`fortuna_killswitch::revocation_guard`]:
/// PRESENT → refuse (a standing kill blocks re-arm until cleared out-of-band, I4);
/// UNVERIFIABLE → refuse (a stat error, e.g. an unreadable parent dir — FAIL
/// CLOSED; this is why we cannot use `!is_revoked`, which collapses
/// unverifiable→false and would wrongly ALLOW); ABSENT → allow (the happy path).
///
/// Absent `[killswitch]` / no `revocation_file` ⇒ no sentinel is configured (the
/// daemon would not wrap a `RevocationHaltPoller` either) ⇒ allow. A config that
/// cannot be read/parsed is itself a refusal (fail closed — we cannot prove the
/// kill state). Pure (path-only): unit-tested without a database.
fn rearm_revocation_precondition(config_path: &str) -> Result<()> {
    let text = match std::fs::read_to_string(config_path) {
        Ok(t) => t,
        // Cannot read the config ⇒ cannot prove the kill state ⇒ fail closed.
        Err(e) => bail!(
            "refusing re-arm: cannot read config {config_path} to verify the kill \
             sentinel state ({e}) — the kill-switch revocation state is unverifiable"
        ),
    };
    let value: toml::Value = toml::from_str(&text)
        .with_context(|| format!("refusing re-arm: cannot parse config {config_path}"))?;
    let revocation_file = value
        .get("killswitch")
        .and_then(|k| k.get("revocation_file"))
        .and_then(|v| v.as_str());
    let Some(path) = revocation_file else {
        // No sentinel configured: nothing to guard (matches the daemon's
        // "absent => no RevocationHaltPoller wrap" — boot.rs).
        return Ok(());
    };
    match fortuna_killswitch::revocation_guard(Path::new(path)) {
        fortuna_killswitch::RevocationGuard::Allow => Ok(()),
        fortuna_killswitch::RevocationGuard::Refuse => bail!(
            "refusing re-arm: the kill-switch revocation sentinel is present or its \
             state is unverifiable — order-placing capability stays revoked. Clear \
             the sentinel out-of-band (fortuna-killswitch / clear the KILLSWITCH_REVOKED \
             file), confirm it is gone, then re-run the re-arm and restart the daemon (I4/I2)."
        ),
    }
}

/// The operator-facing line(s) printed after a successful re-arm. Pure so the
/// wording is unit-tested without a database. M3 (the re-arm notice): a re-arm
/// clears the durable halt in the ledger, but I2 is restart-gated — the RUNNING
/// daemon never auto-resumes, so the operator must be told to restart.
fn rearm_success_message(scope_raw: &str, operator: &str) -> String {
    format!(
        "re-armed {scope_raw} (operator: {operator})\n\
         halt cleared in the ledger; the RUNNING daemon resumes only on restart \
         — run: fortuna stop && fortuna start"
    )
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

    #[test]
    fn paper_demo_assert_accepts_paper_ledger() {
        // The only paper-safe mode for `fortuna start paper-demo`.
        let dir = scratch("paper-demo-ok");
        let config = dir.join("fortuna.toml");
        std::fs::write(
            &config,
            "[runtime]\nstage = \"paper\"\nexecution_mode = \"paper_ledger\"\norders_enabled = false\n",
        )
        .unwrap();
        assert_paper_demo_safe(config.to_str().unwrap())
            .expect("paper_ledger is the paper-safe demo mode");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paper_demo_assert_rejects_non_paper_mode() {
        // A `paper-demo` whose config resolves to ANY non-paper mode is a FOOTGUN
        // — it must FAIL LOUDLY (clear error, non-zero exit). (Mutation: drop the
        // assert and an operator could run a live "demo".)
        for bad in [
            "live_data_only",
            "demo_orders",
            "production_orders",
            "dry_run",
        ] {
            let dir = scratch(&format!("paper-demo-bad-{bad}"));
            let config = dir.join("fortuna.toml");
            std::fs::write(
                &config,
                format!("[runtime]\nstage = \"paper\"\nexecution_mode = \"{bad}\"\norders_enabled = false\n"),
            )
            .unwrap();
            assert!(
                assert_paper_demo_safe(config.to_str().unwrap()).is_err(),
                "mode {bad} must be refused for paper-demo"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn paper_demo_assert_rejects_non_paper_mode_messages() {
        let dir = scratch("paper-demo-bad-msg");
        let config = dir.join("fortuna.toml");
        std::fs::write(
            &config,
            "[runtime]\nstage = \"paper\"\nexecution_mode = \"production_orders\"\norders_enabled = false\n",
        )
        .unwrap();
        let err = assert_paper_demo_safe(config.to_str().unwrap())
            .expect_err("production_orders must be refused for paper-demo");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("paper_ledger") && msg.contains("production_orders"),
            "the refusal names the required and the offending mode: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paper_demo_assert_rejects_missing_runtime_section() {
        // No [runtime] / no execution_mode ⇒ a "demo" with no explicit paper
        // mode ⇒ refuse (fail closed; never assume paper).
        let dir = scratch("paper-demo-no-runtime");
        let config = dir.join("fortuna.toml");
        std::fs::write(&config, "[daemon]\nvenue = \"kalshi\"\n").unwrap();
        assert_paper_demo_safe(config.to_str().unwrap())
            .expect_err("absent [runtime].execution_mode must refuse paper-demo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rearm_precondition_refuses_when_killswitch_sentinel_present() {
        // I4 (E6): the operator re-arm path must REFUSE while a kill sentinel
        // stands. The precondition reads [killswitch].revocation_file and applies
        // the THREE-WAY guard — a present sentinel → Refuse → the re-arm errors
        // BEFORE record_rearm. (Mutation: drop the guard / negate is_revoked and
        // this reds — an absent-OR-present sentinel would both "pass".)
        let dir = scratch("rearm-sentinel-present");
        let sentinel = dir.join("KILLSWITCH_REVOKED");
        std::fs::write(&sentinel, b"{\"revoked_at\":\"x\"}\n").unwrap();
        let config = dir.join("fortuna.toml");
        std::fs::write(
            &config,
            format!(
                "[killswitch]\nrevocation_file = {:?}\n",
                sentinel.display().to_string()
            ),
        )
        .unwrap();

        let err = rearm_revocation_precondition(config.to_str().unwrap())
            .expect_err("a present kill sentinel must REFUSE the re-arm");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("kill") && (msg.contains("revoc") || msg.contains("sentinel")),
            "the refusal names the standing kill: {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rearm_precondition_refuses_when_sentinel_unreadable() {
        // The UNVERIFIABLE branch: the sentinel's parent dir is 0o000, so the
        // child cannot be stat'd. This exercises the try_exists/stat probe (NOT
        // is_revoked, which would report false and — if negated — ALLOW). The
        // precondition must FAIL CLOSED → Refuse.
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("rearm-sentinel-unreadable");
        let locked = dir.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        let sentinel = locked.join("KILLSWITCH_REVOKED");
        let config = dir.join("fortuna.toml");
        std::fs::write(
            &config,
            format!(
                "[killswitch]\nrevocation_file = {:?}\n",
                sentinel.display().to_string()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = rearm_revocation_precondition(config.to_str().unwrap());

        // Restore perms BEFORE asserting so cleanup always succeeds.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        let err = result.expect_err("an unverifiable sentinel must FAIL CLOSED → refuse");
        assert!(
            format!("{err:#}").to_lowercase().contains("kill"),
            "the refusal names the kill state: {err:#}"
        );
    }

    #[test]
    fn rearm_precondition_allows_when_sentinel_absent() {
        // The happy path: an absent sentinel under a readable parent ALLOWS the
        // re-arm (this is exactly the case `!is_revoked` would also pass — the
        // guard must not over-refuse the normal operator action).
        let dir = scratch("rearm-sentinel-absent");
        let sentinel = dir.join("KILLSWITCH_REVOKED");
        let config = dir.join("fortuna.toml");
        std::fs::write(
            &config,
            format!(
                "[killswitch]\nrevocation_file = {:?}\n",
                sentinel.display().to_string()
            ),
        )
        .unwrap();
        rearm_revocation_precondition(config.to_str().unwrap())
            .expect("an absent sentinel under a readable parent must ALLOW the re-arm");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rearm_precondition_allows_when_no_killswitch_section() {
        // No [killswitch] section ⇒ no sentinel configured ⇒ nothing to guard
        // (the daemon would not wrap a RevocationHaltPoller either). ALLOW.
        let dir = scratch("rearm-no-section");
        let config = dir.join("fortuna.toml");
        std::fs::write(&config, "[other]\nx = 1\n").unwrap();
        rearm_revocation_precondition(config.to_str().unwrap())
            .expect("absent [killswitch] config ⇒ no sentinel ⇒ allow");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rearm_message_tells_the_operator_to_restart() {
        // M3: a re-arm clears the durable ledger halt, but I2 is restart-gated —
        // the RUNNING daemon resumes ONLY on restart. The notice must say so and
        // give the exact command, or an operator who re-armed sees trading stay
        // halted with no explanation (the four-state divergence in
        // runbooks/halt-and-rearm.md).
        let msg = rearm_success_message("global", "xavier");
        assert!(
            msg.contains("re-armed global") && msg.contains("operator: xavier"),
            "keeps the scope + operator line: {msg}"
        );
        assert!(
            msg.to_lowercase().contains("restart"),
            "must tell the operator a restart is required: {msg}"
        );
        assert!(
            msg.contains("fortuna stop && fortuna start"),
            "must give the exact restart command: {msg}"
        );
    }

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
    fn zombie_child_reads_as_not_running() {
        // An exited-but-unreaped child is a zombie: ps still lists it
        // (stat Z), but it is not running and never signalable work.
        // `stop` polling such a pid must see an exit, not a hang.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = i64::from(child.id());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(comm_of(pid), None, "a zombie must read as not-running");
        let _ = child.wait();
    }

    #[test]
    fn log_contains_after_respects_the_offset() {
        // A1 + A4: append-mode logs accumulate runs; only bytes after the
        // pre-signal offset count as THIS shutdown's evidence.
        let dir = scratch("log-offset");
        let path = dir.join("daemon.log");
        let old = format!("{DAEMON_SHUTDOWN_MARKER} (previous run)\n");
        std::fs::write(&path, &old).unwrap();
        let offset = old.len() as u64;
        assert!(
            !log_contains_after(&path, offset, DAEMON_SHUTDOWN_MARKER),
            "a marker before the offset must not count"
        );
        assert!(
            log_contains_after(&path, 0, DAEMON_SHUTDOWN_MARKER),
            "offset 0 sees the whole file"
        );
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "{DAEMON_SHUTDOWN_MARKER} — ticks=3").unwrap();
        drop(f);
        assert!(
            log_contains_after(&path, offset, DAEMON_SHUTDOWN_MARKER),
            "a marker appended after the offset counts"
        );
        assert!(
            !log_contains_after(&dir.join("absent.log"), 0, DAEMON_SHUTDOWN_MARKER),
            "a missing log is never evidence"
        );
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

    // ---- orphaned minor F-1: the A8 audit-age crash-tell line ----

    #[test]
    fn audit_age_line_formats_age_and_kind() {
        // now = 2026-06-12T08:00:42Z, row at 08:00:00Z => 42s ago.
        let now = UtcTimestamp::parse_iso8601("2026-06-12T08:00:42.000Z").unwrap();
        let line = format_audit_age(now, "2026-06-12T08:00:00.000Z", "gate_decision");
        assert!(line.contains("42s ago"), "{line}");
        assert!(line.contains("gate_decision"), "{line}");
        // Older rows render in minutes for the operator's eye.
        let old = format_audit_age(now, "2026-06-12T07:48:42.000Z", "order");
        assert!(old.contains("12m ago"), "{old}");
        // An unparseable at column degrades honestly, never panics.
        let bad = format_audit_age(now, "not-a-time", "halt");
        assert!(bad.contains("unparseable"), "{bad}");
    }

    // ---- orphaned minor F-2: A2 spawn-cwd pinning (repo-root anchor) ----

    #[test]
    fn repo_root_derives_from_the_config_path() {
        // /x/repo/config/fortuna.toml => /x/repo (the config dir's parent).
        let root = repo_root_from_config(Path::new("/x/repo/config/fortuna.toml"));
        assert_eq!(root, PathBuf::from("/x/repo"));
        // A relative default resolves against the cwd, exactly as before
        // for the repo-root invocation; the FUNCTION just exposes it.
        let cwd = std::env::current_dir().unwrap();
        let rel = repo_root_from_config(Path::new("config/fortuna.toml"));
        assert_eq!(rel, cwd);
        // A bare filename (no config dir) falls back to the cwd — never
        // an empty or root path.
        let bare = repo_root_from_config(Path::new("fortuna.toml"));
        assert_eq!(bare, cwd);
    }

    #[test]
    fn recorder_out_dir_anchors_to_the_repo_root_not_the_cwd() {
        // A2: a cwd-relative out-dir from a wrong cwd would silently fork
        // the B0 dataset. The invocation anchors relative out-dirs to the
        // CONFIG-derived root.
        let dir = scratch("recorder-anchor");
        let config_dir = dir.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config = config_dir.join("fortuna.toml");
        std::fs::write(&config, "[recorder]\nout_dir = \"data/perishable\"\n").unwrap();
        let args = recorder_invocation(config.to_str().unwrap()).unwrap();
        let out_pos = args.iter().position(|a| a == "--out-dir").unwrap();
        let out = &args[out_pos + 1];
        assert!(
            out.starts_with(dir.to_str().unwrap()),
            "out-dir anchors to the config's repo root, got {out}"
        );
        assert!(out.ends_with("data/perishable"), "{out}");
    }
}
