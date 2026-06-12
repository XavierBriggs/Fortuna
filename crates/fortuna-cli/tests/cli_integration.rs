//! T4.4 operator-CLI integration tests (design docs/design/fortuna-cli.md
//! Section 9 + amendment A9), written BEFORE the implementation per DoD.
//!
//! Slice 1 covers the read-only surfaces: `config check`, `logs`, and the
//! `status` process-health section (pidfile + name-validated PID per A3,
//! stopping marker per A7, config-on-disk line per A6) — including the
//! PINNED behavior change that `status` without DATABASE_URL exits 0
//! (A9; it previously exited 1). `start`/`stop` (real process forking,
//! SIGTERM) are later slices; their tests land with them.
//!
//! Every invocation pins FORTUNA_RUNTIME_DIR to a per-test temp dir and
//! strips DATABASE_URL (the workspace .cargo [env] dev default would
//! otherwise leak into every spawned child).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fortuna"))
}

/// Fresh per-test scratch dir (std-only; tempfile is not a workspace dep).
fn temp_dir(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fortuna-cli-it-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run the CLI with the runtime dir pinned and NO DATABASE_URL.
fn run_no_db(runtime_dir: &Path, args: &[&str]) -> Output {
    bin()
        .args(args)
        .env("FORTUNA_RUNTIME_DIR", runtime_dir)
        .env_remove("DATABASE_URL")
        .output()
        .unwrap()
}

fn stdout_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A3 pidfile format: first line PID, second line expected process name.
fn write_pidfile(dir: &Path, comp: &str, pid: u32, name: &str) {
    fs::write(dir.join(format!("{comp}.pid")), format!("{pid}\n{name}\n")).unwrap();
}

/// Keeps a helper process from outliving a failing test.
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A live process whose `ps -o comm=` output contains "sleep".
fn spawn_sleep() -> ChildGuard {
    ChildGuard(
        Command::new("sleep")
            .arg("300")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

fn example_config() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/fortuna.example.toml")
}

// ---------------------------------------------------------------- config check

#[test]
fn config_check_accepts_example() {
    let dir = temp_dir("cfg-ok");
    let example = example_config();
    let out = run_no_db(
        &dir,
        &[
            "config",
            "check",
            "--config-path",
            example.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "expected exit 0, stderr: {}",
        stderr_of(&out)
    );
    assert!(
        stdout_of(&out).contains("config OK"),
        "stdout: {}",
        stdout_of(&out)
    );
}

#[test]
fn config_check_rejects_bad_toml() {
    let dir = temp_dir("cfg-bad");
    let bad = dir.join("bad.toml");
    fs::write(&bad, "this is [ not toml = =").unwrap();
    let out = run_no_db(
        &dir,
        &["config", "check", "--config-path", bad.to_str().unwrap()],
    );
    assert!(!out.status.success(), "garbage TOML must fail config check");
    assert!(
        stderr_of(&out).contains("config check failed"),
        "stderr: {}",
        stderr_of(&out)
    );
}

#[test]
fn config_check_missing_file_fails() {
    let dir = temp_dir("cfg-missing");
    let absent = dir.join("absent.toml");
    let out = run_no_db(
        &dir,
        &["config", "check", "--config-path", absent.to_str().unwrap()],
    );
    assert!(
        !out.status.success(),
        "a missing config file must fail the check"
    );
}

// ---------------------------------------------------------------------- status

#[test]
fn status_no_processes_no_db_exits_zero() {
    // A9 pins the behavior CHANGE: status without DATABASE_URL exits 0
    // (process health is the always-available section).
    let dir = temp_dir("st-empty");
    let out = run_no_db(&dir, &["status"]);
    assert!(
        out.status.success(),
        "status without DATABASE_URL must exit 0 (A9), stderr: {}",
        stderr_of(&out)
    );
    let text = stdout_of(&out);
    assert!(text.contains("daemon: stopped"), "stdout: {text}");
    assert!(text.contains("recorder: stopped"), "stdout: {text}");
    assert!(text.contains("config on disk:"), "A6 line missing: {text}");
    assert!(text.contains("DATABASE_URL not set"), "stdout: {text}");
    // Process health renders BEFORE the db section (design Section 3).
    let proc_at = text.find("daemon:").unwrap();
    let db_at = text.find("DATABASE_URL not set").unwrap();
    assert!(
        proc_at < db_at,
        "process section must precede db section: {text}"
    );
}

#[test]
fn status_shows_live_pidfile_as_running() {
    let dir = temp_dir("st-live");
    let child = spawn_sleep();
    let pid = child.0.id();
    write_pidfile(&dir, "daemon", pid, "sleep");
    let out = run_no_db(&dir, &["status"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(
        text.contains(&format!("daemon: running (pid {pid})")),
        "stdout: {text}"
    );
}

#[test]
fn status_dead_pid_is_stale_not_running() {
    let dir = temp_dir("st-dead");
    let pid = {
        let mut child = spawn_sleep();
        let pid = child.0.id();
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        pid
    };
    write_pidfile(&dir, "daemon", pid, "sleep");
    let out = run_no_db(&dir, &["status"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.contains("daemon: stopped"), "stdout: {text}");
    assert!(
        text.contains("stale"),
        "a dead pid must read as stale: {text}"
    );
    assert!(!text.contains("daemon: running"), "stdout: {text}");
}

#[test]
fn status_name_mismatch_is_stale_not_running() {
    // A3: macOS reuses PIDs — a live pid whose comm does not contain the
    // pidfile's claimed name must NEVER be trusted (or signaled, later).
    let dir = temp_dir("st-mismatch");
    let child = spawn_sleep();
    let pid = child.0.id();
    write_pidfile(&dir, "daemon", pid, "fortuna-live");
    let out = run_no_db(&dir, &["status"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.contains("name mismatch"), "stdout: {text}");
    assert!(!text.contains("daemon: running"), "stdout: {text}");
}

#[test]
fn status_malformed_pidfile_is_stale() {
    let dir = temp_dir("st-malformed");
    fs::write(dir.join("daemon.pid"), "not-a-pid\n").unwrap();
    let out = run_no_db(&dir, &["status"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.contains("daemon: stopped"), "stdout: {text}");
    assert!(!text.contains("daemon: running"), "stdout: {text}");
}

#[test]
fn status_stopping_marker_shows_stopping_since() {
    // A7: stop writes <component>.stopping; status surfaces it while the
    // process is still draining.
    let dir = temp_dir("st-stopping");
    let child = spawn_sleep();
    let pid = child.0.id();
    write_pidfile(&dir, "daemon", pid, "sleep");
    fs::write(dir.join("daemon.stopping"), "").unwrap();
    let out = run_no_db(&dir, &["status"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.contains("daemon: stopping since"), "stdout: {text}");
}

#[test]
fn status_db_unreachable_still_exits_zero() {
    // "Degradable" (design Section 3): a Pg outage must not hide process
    // health from the operator. Port 9 on loopback refuses immediately.
    let dir = temp_dir("st-noPg");
    let out = bin()
        .args(["status"])
        .env("FORTUNA_RUNTIME_DIR", &dir)
        .env("DATABASE_URL", "postgres://127.0.0.1:9/fortuna_nope")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "status with an unreachable DB must still exit 0, stderr: {}",
        stderr_of(&out)
    );
    let text = stdout_of(&out);
    assert!(text.contains("daemon: stopped"), "stdout: {text}");
    assert!(text.contains("db: unavailable"), "stdout: {text}");
}

// ------------------------------------------------------------------------ logs

#[test]
fn logs_rejects_unknown_component() {
    let dir = temp_dir("logs-unknown");
    let out = run_no_db(&dir, &["logs", "frobnicator"]);
    assert!(!out.status.success());
    let err = stderr_of(&out);
    assert!(
        err.contains("daemon") && err.contains("recorder"),
        "error must name the valid components: {err}"
    );
}

#[test]
fn logs_requires_component() {
    let dir = temp_dir("logs-bare");
    let out = run_no_db(&dir, &["logs"]);
    assert!(!out.status.success());
}

#[test]
fn logs_missing_file_fails_informatively() {
    let dir = temp_dir("logs-missing");
    let out = run_no_db(&dir, &["logs", "daemon"]);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("no log file"),
        "stderr: {}",
        stderr_of(&out)
    );
}

#[test]
fn logs_prints_last_50_lines() {
    let dir = temp_dir("logs-tail");
    fs::create_dir_all(dir.join("logs")).unwrap();
    let body: String = (1..=60).map(|i| format!("L{i:04}\n")).collect();
    fs::write(dir.join("logs/daemon.log"), body).unwrap();
    let out = run_no_db(&dir, &["logs", "daemon"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let text = stdout_of(&out);
    assert!(text.contains("L0060"), "newest line must print: {text}");
    assert!(
        text.contains("L0011"),
        "50th-from-end line must print: {text}"
    );
    assert!(
        !text.contains("L0010"),
        "only the last 50 lines print: {text}"
    );
}

// ----------------------------------------------------------------------- start
//
// Slice 2 (A2/A3/A4). The SUCCESS spawn path is a manual runbook check by
// design (Section 9: real forking is timing-flaky in CI) — and this dev box
// intentionally hosts the operator's UNMANAGED recorder, so a clean spawn
// cannot run here at all. What IS deterministic everywhere: the config-check
// gate, the A2 refusal (a planted decoy guarantees a pgrep hit even on clean
// machines), all-running idempotency (exit 0 BEFORE the A2 check), and
// --foreground exec. Claim atomicity and append-mode redirection are unit
// tests in main.rs.

/// An executable stub script. Component stubs simulate the managed binaries
/// without booting real processes.
fn write_stub(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A bin dir holding fortuna-live + fortuna-recorder stubs (FORTUNA_BIN_DIR
/// resolution seam).
fn stub_bin_dir(case: &str) -> PathBuf {
    let dir = temp_dir(&format!("{case}-bin"));
    write_stub(&dir, "fortuna-live", "#!/bin/sh\nsleep 300\n");
    write_stub(&dir, "fortuna-recorder", "#!/bin/sh\nsleep 300\n");
    dir
}

fn run_start(runtime_dir: &Path, bin_dir: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["start"];
    args.extend_from_slice(extra);
    bin()
        .args(&args)
        .env("FORTUNA_RUNTIME_DIR", runtime_dir)
        .env("FORTUNA_BIN_DIR", bin_dir)
        .env_remove("DATABASE_URL")
        .output()
        .unwrap()
}

#[test]
fn start_fails_config_check_first() {
    let dir = temp_dir("start-badcfg");
    let bins = stub_bin_dir("start-badcfg");
    let absent = dir.join("absent.toml");
    let out = run_start(&dir, &bins, &["--config-path", absent.to_str().unwrap()]);
    assert!(!out.status.success(), "missing config must refuse start");
    assert!(
        stderr_of(&out).contains("config check failed"),
        "stderr: {}",
        stderr_of(&out)
    );
    assert!(
        !dir.join("daemon.pid").exists() && !dir.join("recorder.pid").exists(),
        "a refused start must claim no pidfiles"
    );
}

#[test]
fn start_refuses_on_unmanaged_recorder() {
    // A2: never adopt, never double-spawn — two appenders can tear the B0
    // JSONL dataset. The decoy's command line contains "fortuna-recorder"
    // (script path), guaranteeing a pgrep -f hit even on machines without
    // the operator's real recorder.
    let dir = temp_dir("start-refuse");
    let bins = stub_bin_dir("start-refuse");
    let decoy_dir = temp_dir("start-refuse-decoy");
    let decoy = write_stub(&decoy_dir, "fortuna-recorder", "#!/bin/sh\nsleep 300\n");
    let _decoy_proc = ChildGuard(Command::new(&decoy).spawn().unwrap());
    let example = example_config();
    let out = run_start(&dir, &bins, &["--config-path", example.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "unmanaged recorder must refuse start"
    );
    let err = stderr_of(&out);
    assert!(
        err.contains("stop the manual recorder, then re-run"),
        "the one-time migration instruction must print: {err}"
    );
    assert!(
        !dir.join("daemon.pid").exists() && !dir.join("recorder.pid").exists(),
        "the A2 refusal happens BEFORE any pidfile claim"
    );
}

#[test]
fn start_is_idempotent_when_all_running() {
    // Design Section 5: already-running -> exit 0. This must be decided
    // BEFORE the A2 check (a fully-managed system is not a collision).
    let dir = temp_dir("start-idem");
    let bins = stub_bin_dir("start-idem");
    let daemon = spawn_sleep();
    let recorder = spawn_sleep();
    write_pidfile(&dir, "daemon", daemon.0.id(), "sleep");
    write_pidfile(&dir, "recorder", recorder.0.id(), "sleep");
    let example = example_config();
    let out = run_start(&dir, &bins, &["--config-path", example.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "all-running start must exit 0 (idempotent), stderr: {}",
        stderr_of(&out)
    );
    assert!(
        stdout_of(&out).contains("already running"),
        "stdout: {}",
        stdout_of(&out)
    );
}

#[test]
fn start_foreground_execs_daemon_and_propagates_exit() {
    let dir = temp_dir("start-fg");
    let bins = temp_dir("start-fg-bin");
    write_stub(
        &bins,
        "fortuna-live",
        "#!/bin/sh\necho FOREGROUND_STUB ran with: \"$1\"\nexit 7\n",
    );
    let example = example_config();
    let out = run_start(
        &dir,
        &bins,
        &["--foreground", "--config-path", example.to_str().unwrap()],
    );
    assert_eq!(
        out.status.code(),
        Some(7),
        "exec must propagate the daemon's exit code; stderr: {}",
        stderr_of(&out)
    );
    let text = stdout_of(&out);
    assert!(text.contains("FOREGROUND_STUB"), "stdout: {text}");
    assert!(
        !dir.join("daemon.pid").exists(),
        "--foreground is a debugging mode: no pidfile"
    );
}

#[test]
fn start_mid_claim_pidfile_is_contended_not_stale() {
    // A3 validate-then-decide: an EMPTY claimed pidfile is another start
    // mid-claim, not stale junk — never steal it.
    let dir = temp_dir("start-midclaim");
    let bins = stub_bin_dir("start-midclaim");
    fs::write(dir.join("daemon.pid"), "").unwrap();
    let example = example_config();
    let out = run_start(&dir, &bins, &["--config-path", example.to_str().unwrap()]);
    assert!(!out.status.success(), "mid-claim must refuse, not steal");
    assert!(
        stderr_of(&out).contains("in progress"),
        "stderr: {}",
        stderr_of(&out)
    );
}

// ----------------------------------------------------------------------- usage

#[test]
fn usage_names_new_commands() {
    let dir = temp_dir("usage");
    let out = run_no_db(&dir, &[]);
    assert!(!out.status.success());
    let err = stderr_of(&out);
    assert!(err.contains("config"), "usage must name config: {err}");
    assert!(err.contains("logs"), "usage must name logs: {err}");
    assert!(err.contains("start"), "usage must name start: {err}");
}
