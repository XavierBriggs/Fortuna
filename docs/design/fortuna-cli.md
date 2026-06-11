# FORTUNA Operator CLI — Design Specification

Status: design v1.1. Operator-directed 2026-06-11 (night). Command names
start/stop per operator preference (supersedes up/down). Adversarial critique
COMPLETE and folded — the AMENDMENTS section below is BINDING and overrides
the body where they conflict. v1 scope is FOUR new commands (mode and
db migrate-status are CUT). Implementer runs the Section 10 checklist plus the
amendment checks as iteration 0 BEFORE building; independent gate verifies.

## AMENDMENTS (adversarial critique, 2026-06-11 — binding, override the body)

A1. T4.1 SHUTDOWN CONTRACT (was Blocker 1 — the body's Section 1.3 claim
    "SIGTERM is its shutdown signal" was an unverified assumption, NOT a
    citation; BUILD_PLAN named no signal). Now contracted in BUILD_PLAN T4.1:
    SIGTERM handler == graceful shutdown, smoke/DST-asserted (SIGTERM ->
    cancel working orders + final audit row). `stop` MUST NOT ship before
    that assertion exists, and `stop` declares success only after confirming
    the shutdown audit line in the daemon log — process exit alone is not
    success.
A2. RECORDER COLLISION (was Blocker 2): a recorder is ALREADY running,
    started manually, no pidfile (live invocation: fortuna-recorder
    --interval-secs 30 --bracket-series KXBTC15M,KXBTC,KXBTCD, cwd = repo
    root). `start` MUST check for an unmanaged fortuna-recorder process
    (pgrep -f) and REFUSE with a one-time migration instruction ("stop the
    manual recorder, then re-run") — never adopt, never double-spawn (two
    appenders can tear JSONL lines in the sacred B0 dataset). Recorder
    invocation (interval 30s, series KXBTC15M,KXBTC,KXBTCD, ABSOLUTE
    out-dir) is pinned in config; spawn cwd specified as repo root.
A3. PID VALIDATION + ATOMIC CLAIM: pidfiles store PID + process name; every
    read validates via `ps -p <pid> -o comm=` containing the expected binary
    before trusting or signaling (macOS PID reuse). `start` claims the
    pidfile with OpenOptions::create_new (O_EXCL) BEFORE spawning; on
    EEXIST, validate-then-decide.
A4. DETACH MECHANICS: CommandExt::process_group(0) (stable std — no nix
    dep), stdin(Stdio::null()), APPEND-mode log redirection (never truncate;
    crash backtraces survive restarts). `logs -f` = exec tail -n50 -f.
A5. RUNTIME DIR: data/runtime/ (gitignored), NOT /tmp (macOS wipes /tmp on
    reboot — crash forensics must survive).
A6. CUTS: `mode` and `db migrate-status` commands and the
    --allow-pending-migrations flag are OUT of v1. Reason: fortuna_ledger::
    connect() AUTO-MIGRATES unconditionally (ledger/src/lib.rs:62-68) — a
    naive migrate-status would mutate schema, and a pre-flight refusal the
    boot path overrides is theater. `status` prints one line "config on
    disk: venue/mode (daemon may differ until restart)" instead of `mode`.
    Status Section 3 (metrics-endpoint poll) is DEFERRED until ROTA lands.
A7. STOP SEMANTICS: on daemon timeout, STILL proceed to the recorder; write
    a <component>.stopping marker so status shows "stopping since T";
    timeout warning text: "daemon is cancelling working orders — do NOT
    kill -9; watch fortuna logs daemon; if the venue is unreachable use
    fortuna kill"; default --timeout-secs 60 (>= the realistic cancel
    budget; the body's 30 contradicted its own <60s claim).
A8. ORPHAN-ORDER OWNERSHIP (stated, not built here): daemon crash between
    start and stop leaves venue orders to the EXISTING owners — dead-man
    monitor (spec line 352), order TTL policy (spec 5.4), boot
    reconciliation at next start. `start` prints active halts when DB is
    reachable (I2 visibility); `status` prints the age of the most recent
    audit row (a stale age + live pidfile = crash tell).
A9. TEST PLAN (replaces the body's easy list): pidfile liveness via spawned
    sleep (live), written-then-killed PID (stale), name-mismatch PID
    (reuse); start refusal on an unmanaged recorder-named process; atomic
    claim race (two starts, one wins); append-mode redirect; pin the
    behavior change that `status` without DATABASE_URL exits 0 (today it
    exits 1, main.rs:142-144); and the A1 SIGTERM assertion as a ship gate.
A10. I5 FRAMING: the daemon's own boot/final audit rows (mandatory,
    halt-on-failure) are the I5 record; CLI lifecycle rows are advisory
    attribution only — that is WHY best-effort is acceptable and `stop` is
    never blockable by a dead DB.

Operator requirement (verbatim intent): "good maintainability and
manageability, like a CLI — nice CLI meaning easy to start and stop the whole
system if possible." Zero bloat; every command earns its place.

## 1. Codebase findings (cited evidence)

### 1.1 Existing CLI

The binary `fortuna` already exists at `crates/fortuna-cli/src/main.rs`. Its
commands as of this writing (lines 8-12):

```
fortuna status
fortuna halt   <scope> --reason "..." --operator <name>
fortuna rearm  <scope> --reason "..." --operator <name>
fortuna kill   [--flatten] --journal <path>
```

The arg parser is hand-rolled (`while i < raw.len()` loop, lines 63-89). There
is no clap anywhere in the workspace. Deps: fortuna-core, fortuna-gates,
fortuna-ledger, anyhow, tokio, serde_json.

**Design rule: extend this binary. Do not create a second binary.**

### 1.2 Kill-switch independence (I4)

`crates/fortuna-killswitch/Cargo.toml` lines 8-9 state the structural rule (no
fortuna-ledger, sqlx, Postgres, cognition, event loop); the invariant suite
asserts the dependency graph; `scripts/killswitch-test.sh` line 10 proves
Postgres-free via `env -u DATABASE_URL`. The existing `kill` command (main.rs
lines 109-138) already uses the correct pattern: subprocess exec, never a
library call. Nothing in this design may add fortuna-killswitch to
fortuna-cli's dependency tree.

### 1.3 T4.1 daemon (the lifecycle subject)

BUILD_PLAN Phase 4 T4.1 specifies fortuna-live: config load, repos +
AuditWriter, tick loop, mind, dead-man pinger, halt poll <=500ms, graceful
shutdown (cancel working orders, final audit row). SIGTERM is its shutdown
signal; the CLI's `stop` manages it by SIGTERM only.

### 1.4 Recorder

`crates/fortuna-recorder/` runs as a free-standing background process; restart
cmd documented at data/recorder.log line 1; no pidfile/launchd yet (T5.B0
note). The CLI manages it with the same pidfile pattern as the daemon.

### 1.5 Parser style

All three extant binaries hand-roll `while i < args.len()` parsing. Follow the
idiom. No clap.

### 1.6 Operator-action audit pattern

main.rs lines 189-216: state-changing commands write a durable halts row plus
`audit.append("halt", Some(&operator), ...)`. Lifecycle commands follow the
pattern where the DB is reachable and degrade gracefully where not.

### 1.7 Config validation

`FortunaConfig::load_file` (crates/fortuna-ops/src/config.rs:140) validates the
whole shape; errors are typed `OpsError::Config`.

### 1.8 Migrations

`fortuna_ledger::MIGRATOR` (static sqlx Migrator, ledger/src/lib.rs:60).

## 2. Supervision / lifecycle decision

**Pidfile + SIGTERM; foreground available for debugging; no launchd in v1.**

`start` starts fortuna-live and fortuna-recorder as detached background processes,
PIDs written under `FORTUNA_RUNTIME_DIR` (default `/tmp/fortuna-pids/`),
stdout/stderr redirected to `FORTUNA_RUNTIME_DIR/logs/<component>.log`. `stop`
reads pidfiles, sends SIGTERM (daemon first, then recorder), waits up to
`--timeout-secs` (default 30) for clean exit; on timeout it prints a warning
and exits non-zero — it NEVER sends SIGKILL, because the daemon's shutdown path
(cancel working orders, final audit row) must complete. `status` reads pidfiles
+ `kill -0`.

Why not launchd: XML config outside the repo, launchctl as a second tool, logs
diverted to the unified log away from the audit trail, unload/load cycles on
every rebuild. The pidfile approach works identically under `cargo run` and
release builds, needs zero system config, and a launchd plist that simply
invokes `fortuna start` is the later upgrade path with no CLI changes. Why not
tmux/foreground-only: the operator needs their terminal back; `--foreground`
remains for debugging. I2/I4 compatibility: nothing auto-restarts, re-arm stays
CLI-only, kill switch stays a separate exec'd binary.

## 3. Command inventory

All changes land in `crates/fortuna-cli/` only.

| Command | Behavior | DB required |
|---|---|---|
| `fortuna start [--foreground] [--allow-pending-migrations]` | config check -> migration check -> refuse if already running -> start daemon + recorder w/ pidfiles + log redirect -> audit "lifecycle/start" (best-effort) | migrations check only |
| `fortuna stop [--timeout-secs N]` | SIGTERM daemon then recorder; wait; never SIGKILL; idempotent; audit best-effort, never blocks shutdown | no |
| `fortuna status` | Section 1 always (pidfile + kill -0, <200ms); Section 2 if DB (active halts, recent audit); Section 3 if daemon metrics endpoint reachable (last tick age, dead-man) — extends existing status arm (main.rs:152-176), existing DB queries unchanged | degradable |
| `fortuna halt / rearm / kill` | EXISTING, unchanged | as today |
| `fortuna logs <component> [-f]` | tail `FORTUNA_RUNTIME_DIR/logs/<component>.log`; components: daemon, recorder | no |
| `fortuna db migrate-status` | list applied/pending via MIGRATOR; exit 0 only if none pending | yes |
| `fortuna config check [--config-path <p>]` | `FortunaConfig::load_file` validation only; starts nothing | no |
| `fortuna mode` | read-only print of venue/stage/mode from config + reminder that changes are edit + `stop && start` | no |

Six new commands; four existing (one extended).

## 4. Explicitly NOT in v1

- Hot config reload / mode toggle (auditability: running state must be
  reconstructible from config-at-boot; restart is the clean audit boundary;
  fail-closed startup stays simple). Mode changes = edit config, `fortuna stop
  && fortuna start` (<60s even with working orders).
- Remote management / multi-host; ROTA data duplication beyond terse status;
  kill-switch wrapping; launchd plists (documented upgrade path); automatic
  crash restarts (conflicts with I2's no-automatic-resumption posture — the
  operator decides what a crash means).

## 5. Data flows

`start`: config check (fail->exit 1) -> migration check (pending->exit 1 unless
flag) -> already-running check (idempotent exit 0) -> spawn daemon w/ pidfile +
log redirect -> spawn recorder likewise -> best-effort audit row -> print pids.

`stop`: per component: read pidfile -> not running? "already stopped" ->
running? SIGTERM, wait <=N s -> clean? remove pidfile : warn + exit 1 ->
best-effort audit row.

`status`: always pidfile+kill -0 section; then DB section (active halts, recent
halt audit) if DATABASE_URL; then metrics section (tick age, dead-man) if the
daemon's GET endpoint answers.

## 6. Dependency changes

Add `fortuna-ops = { path = "../fortuna-ops" }` to fortuna-cli (for
FortunaConfig). Pidfiles via std::fs; spawning via std::process::Command;
SIGTERM via the `nix` crate IF already transitive, else shell-out to
`kill -15 <pid>` with a GAPS entry (std's Child::kill is SIGKILL — never use
it here). No other new deps.

## 7. Audit and safety rules

1. `start`/`stop` write best-effort `"lifecycle"` audit rows (action, pids,
   `$USER` as actor) when DB reachable; failures warn, never block.
2. `start` refuses on config-check failure or pending migrations (unless flag).
3. `stop` never SIGKILLs; timeout => exit 1, process left for the operator.
4. `stop` idempotent; `mode` strictly read-only.
5. Existing halt/rearm/kill semantics untouched.

## 8. Mode-change policy (justification)

Audit (I5): a restart creates the clean boundary — boot reconciliation records
config at startup; hot-swap would need re-validation, venue reconnection, open-
order reconciliation, and atomic strategy swap mid-trade for zero benefit
tonight. Demo flip stays: `stop` -> edit config -> `config check` -> `start`.

## 9. Testing plan

New `crates/fortuna-cli/tests/cli_integration.rs`, written BEFORE
implementation: config_check_rejects_bad_toml; config_check_accepts_example;
status_no_processes (empty runtime dir => "stopped", exit 0); stop_idempotent;
mode_reads_config (prints venue, modifies nothing); db_migrate_status_no_db
(informative non-zero). The start->status->stop smoke is a documented manual
runbook check (real process forking is timing-flaky in CI); if T4.1's DST
smoke composition allows, assert the shutdown audit row appears in the daemon
log before exit.

## 10. Implementer validation checklist (run BEFORE building)

1. `cargo build -p fortuna-cli` succeeds at HEAD.
2. `grep -r clap crates/*/Cargo.toml` is empty (house style holds).
3. fortuna-killswitch is never a fortuna-cli dependency (Cargo.toml +
   `cargo tree -p fortuna-cli`); `kill` stays subprocess-exec (main.rs:116,122).
4. fortuna-ops adds cleanly as a regular dep; no cycle (`cargo tree`).
5. `FortunaConfig::load_file` signature matches (ops/src/config.rs:140).
6. sqlx 0.8 MIGRATOR exposes a pending-status API; else fall back + GAPS.
7. `grep -r FORTUNA_RUNTIME_DIR crates/` empty (name free); default dir
   creatable via create_dir_all.
8. SIGTERM mechanism decided: `cargo tree -p fortuna-cli | grep nix`; if
   absent, shell-out `kill -15` + GAPS entry. NEVER Child::kill (SIGKILL).
9. Existing status DB queries (main.rs:154-175) byte-unchanged in the diff;
   process-health section inserted BEFORE them.
10. `audit.append` signature matches (ledger/src/audit.rs:47-52); lifecycle
    rows: `("lifecycle", Some(&user), None, json!({"action": ...}))`.
11. `--timeout-secs` conflicts with no existing flag (main.rs:52-89).
12. Recorder has no --log-file flag (recorder/src/main.rs:59-76); `start` owns
    the redirect; `logs recorder` reads the redirected path.

## 11. Fit-validation notes (implementer fills at iteration 0)

Recorded 2026-06-11 (implementer loop iteration 3; validation only).
Verdict: **BUILDABLE AS AMENDED** — all checklist items pass; A6's cuts
moot item 6; no misfit beyond what the amendments already resolve.

- 1 PASS: `cargo build -p fortuna-cli` exit 0 at HEAD.
- 2 PASS: zero `clap` across all crate manifests.
- 3 PASS: cli deps = core, gates, ledger only; killswitch absent from the
  tree; `kill` is subprocess exec (`Command::new("fortuna-killswitch")`,
  main.rs:116).
- 4 PASS: `cargo tree -p fortuna-cli --depth 1` = core/gates/ledger;
  fortuna-ops deps are core+gates (no cli), so adding ops is acyclic.
- 5 PASS: `pub fn load_file(path: impl AsRef<Path>) -> Result<Self,
  OpsError>` at ops/config.rs:140, exactly as cited.
- 6 N/A by amendment A6 (db migrate-status CUT from v1; MIGRATOR API
  question moot).
- 7 PASS: `FORTUNA_RUNTIME_DIR` 0 hits (name free). A5 runtime dir
  data/runtime/ is inside the gitignored data/ tree.
- 8 DECIDED: `nix` is NOT in Cargo.lock -> SIGTERM mechanism is the
  shell-out `kill -15 <pid>` fallback per this item's else-branch; GAPS
  entry to be written WITH the build (not yet — nothing built).
  Child::kill never used (SIGKILL).
- 9 PASS: existing status DB queries live at main.rs:~154-175 (halts
  print verified); byte-unchanged requirement noted for the build diff.
- 10 PASS: `pub async fn append(&self, kind: &str, ...)` begins at
  audit.rs:47 as cited.
- 11 PASS: existing flags are --flatten/--journal/--operator/--reason
  (+ bare --); --timeout-secs is free.
- 12 PASS: recorder flags are --bracket-series/--interval-secs/--once/
  --out-dir; NO --log-file, so `start` owns the redirect. A2 note: the
  ABSOLUTE out-dir pin is expressible through the existing --out-dir.
- MIGRATOR API call used: none (A6 cut).
- GAPS entries created: none yet — the SIGTERM shell-out entry lands with
  the build iteration per DoD.
- Bloat/misfit flags: body §3 table still lists `mode` + `db
  migrate-status` (6 new commands) and §2 names /tmp + 30s timeout — all
  overridden by A5/A6/A7 (4 commands, data/runtime/, 60s); build to the
  amendments. A1 ship-gate: `stop` cannot ship before T4.1's SIGTERM
  smoke assertion exists — T4.1 currently has only the boot layer, so
  CLI build order is config-check/status/logs/start first, stop LAST.

## 12. Files to create or modify

| File | Change |
|---|---|
| crates/fortuna-cli/Cargo.toml | + fortuna-ops (+ nix only if chosen) |
| crates/fortuna-cli/src/main.rs | 6 new command arms; status extended; usage string updated |
| crates/fortuna-cli/tests/cli_integration.rs | NEW — Section 9 tests, written first |
| GAPS.md / ASSUMPTIONS.md | SIGTERM/MIGRATOR fallbacks; runtime-dir choice |

No changes to killswitch, ops, ledger, invariants, or any other crate.
