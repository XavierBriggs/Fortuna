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
    [2026-06-12: the contracted assertions now EXIST —
    `signal_with_working_orders_cancels_them_and_audits` and
    `daemon_smoke_boot_ticks_signal_shutdown`
    (crates/fortuna-live/tests/daemon_smoke.rs) fire the same stop channel
    main's SIGTERM handler fires and assert cancel-working-orders + the
    final audit row, atop `shutdown_cancels_acked_working_orders_and_audits`
    (crates/fortuna-live/tests/shutdown.rs).]
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

The binary `fortuna` already exists at `crates/fortuna-cli/src/main.rs`. The
ORIGINAL pre-T4.4 command set this design extended was:

```
fortuna status
fortuna halt   <scope> --reason "..." --operator <name>
fortuna rearm  <scope> --reason "..." --operator <name>
fortuna kill   [--flatten] --journal <path>
```

AS BUILT (post-T4.4), the binary now also ships `config check`, `logs`,
`start`, and `stop` (the usage banner is in the `parse_args` `bail!`, and the
module doc-comment header lists all eight); see §3 for the reconciled
inventory.

The arg parser is hand-rolled (the `while i < raw.len()` loop in `parse_args`).
There is no clap anywhere in the workspace. Deps: fortuna-core, fortuna-gates,
fortuna-ledger, fortuna-ops (added by this work, for `FortunaConfig`), anyhow,
tokio, serde_json, toml.

**Design rule: extend this binary. Do not create a second binary.**

### 1.2 Kill-switch independence (I4)

`crates/fortuna-killswitch/Cargo.toml` states the structural rule (no
fortuna-ledger, sqlx, Postgres, cognition, event loop); the invariant suite
asserts the dependency graph; `scripts/killswitch-test.sh` proves Postgres-free
via `env -u DATABASE_URL`. The `kill` command (in `kill()`,
`crates/fortuna-cli/src/main.rs`) uses the correct pattern: it execs the
standalone `fortuna-killswitch` binary (`Command::new("fortuna-killswitch")`,
with a `cargo run -p fortuna-killswitch` dev fallback when the installed binary
is absent), never a library call. `--flatten` selects the killswitch's `report`
verb; otherwise it invokes `freeze`. Nothing in this design may add
fortuna-killswitch to fortuna-cli's dependency tree.

### 1.2a Standalone kill-switch verbs (the binary `fortuna kill` triggers)

`crates/fortuna-killswitch/src/main.rs` is the I4 switch and is invoked
directly by the operator, out-of-band, as well as via `fortuna kill`. Its
verbs (each takes `--journal <path>`; `freeze` also accepts `--venue`,
default `kalshi`):

| Verb | Behavior |
|---|---|
| `freeze` | Cancel every open order at the venue (live: kalshi only; fail-closed on missing `FORTUNA_KILLSWITCH_KALSHI_*` creds), report positions, then WRITE the durable I4 revocation sentinel (`KILLSWITCH_REVOKED`, sibling of the journal) so the runtime refuses future order placement until cleared. |
| `flatten-perps` | Cancel-all + reduce-only IOC closes on the Kinetics perp venue, each close a gated order through the real perp gate (spec 5.15); also writes the revocation sentinel. `--venue` is ignored (kinetics is the only perp venue). |
| `clear-revocation` | **NEW (spec Section 8 — kill-switch reversal is CLI-only).** REMOVES the `KILLSWITCH_REVOKED` sentinel so a subsequent daemon RESTART boots un-revoked. Touches NO venue and reads NO creds — the operator's out-of-band un-revoke. Idempotent (a missing sentinel is success). This is the I4 reversal counterpart to `halt`/`rearm`'s I2 path: clearing the sentinel re-arms order-placing CAPABILITY, but the daemon resumes only on restart (I2: no automatic resumption). |
| `self-test` | Exercise the full freeze machinery against an in-process sim venue (the monthly-test path, I4). |
| `report` | NOT wired — prints guidance to use `freeze` (which reports positions after cancelling) and exits non-zero. |

The re-arm + kill-reversal being CLI/operator-only is deliberate (spec
Section 8): a compromised Slack token must not be able to un-halt a halted
system or clear a revocation. See `docs/runbooks/halt-and-rearm.md` for the
end-to-end re-arm sequence (`clear-revocation` + `fortuna rearm` + restart).

### 1.3 T4.1 daemon (the lifecycle subject)

BUILD_PLAN Phase 4 T4.1 specifies fortuna-live: config load, repos +
AuditWriter, tick loop, mind, dead-man pinger, halt poll <=500ms, graceful
shutdown (cancel working orders, final audit row). SIGTERM is its shutdown
signal; the CLI's `stop` manages it by SIGTERM only (AS BUILT: `send_sigterm`
shells out `kill -15` — `nix` is not a workspace dep and `Child::kill` is
SIGKILL). Per A1, `stop` confirms the daemon's clean-shutdown line
(`"fortuna-live: clean shutdown"`, `DAEMON_SHUTDOWN_MARKER`) in the log AFTER
the signal before declaring success — process exit alone is not success.

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

> AS-BUILT note: the default runtime dir is `data/runtime/` (A5), NOT
> `/tmp/...`; the `stop` default timeout is 60s (A7), NOT 30; liveness is via
> `ps -p <pid> -o stat= -o comm=` (name-validated, zombie-aware), NOT a bare
> `kill -0`. Operator walkthroughs: `docs/runbooks/demo-bringup.md` (start the
> stack) and `docs/runbooks/halt-and-rearm.md` (stop / halt / re-arm).

`start` starts fortuna-live and fortuna-recorder as detached background processes,
PIDs written under `FORTUNA_RUNTIME_DIR` (default `data/runtime/`, A5; anchored
to the config-derived repo root, F-2), stdout/stderr redirected (append-mode, A4)
to `FORTUNA_RUNTIME_DIR/logs/<component>.log`. `stop` reads pidfiles, sends
SIGTERM (daemon first, then recorder), waits up to `--timeout-secs` (default 60,
A7) for clean exit; on timeout it prints the A7 warning and exits non-zero — it
NEVER sends SIGKILL, because the daemon's shutdown path (cancel working orders,
final audit row) must complete. `status` reads pidfiles and validates each PID's
liveness + identity.

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

> **Reconciled to the shipped binary (2026-06-12).** The table below is the
> AS-BUILT inventory after the AMENDMENTS were applied. `mode` and
> `db migrate-status` were CUT (A6) and never built; `--allow-pending-migrations`
> does not exist; `status` gained a DB + audit-age section, NOT a metrics-endpoint
> poll (deferred, A6). Both `start` and `stop` are operationalized in
> `docs/runbooks/demo-bringup.md` (bring-up) and `docs/runbooks/halt-and-rearm.md`
> (halt/re-arm lifecycle).

| Command | Behavior | DB required |
|---|---|---|
| `fortuna start [--foreground] [--config-path <p>]` | config check -> refuse if already running (idempotent exit 0) -> A2 unmanaged-recorder refusal -> claim + spawn daemon + recorder w/ pidfiles + append-log redirect -> A8 active-halt visibility + best-effort "lifecycle/start" audit. `--foreground` exec-replaces with `fortuna-live` (no pidfile, no recorder). NO migration pre-flight (A6: the daemon's boot connect auto-migrates) | no (halt visibility + audit are best-effort) |
| `fortuna stop [--timeout-secs N]` | SIGTERM daemon then recorder; wait (default 60s, A7); never SIGKILL; idempotent; daemon success requires the clean-shutdown line in the log AFTER the signal (A1); timeout leaves process + pidfile + `.stopping` marker and warns; audit best-effort, never blocks shutdown | no |
| `fortuna status` | process-health section ALWAYS (name-validated pidfiles, exit 0 even without a DB, A9); "config on disk" line; then a DB section if `DATABASE_URL` is set and Pg answers within 5s (active halts, recent halt/gate/order rows, A8 audit-age crash-tell). NO metrics-endpoint poll — deferred (A6) | degradable |
| `fortuna halt / rearm / kill` | EXISTING. `halt`/`rearm` write a durable halt + audit row (rearm prints the restart-required notice, M3); `kill` execs the standalone `fortuna-killswitch` (see §1.2) | halt/rearm yes; kill no |
| `fortuna logs <component> [-f]` | exec `tail -n50 [-f] <log>` on the redirected `FORTUNA_RUNTIME_DIR/logs/<component>.log`; components: daemon, recorder | no |
| `fortuna config check [--config-path <p>]` | `FortunaConfig::load_file` validation only; starts nothing, mutates nothing | no |
| ~~`fortuna db migrate-status`~~ | **CUT (A6), never built.** The daemon's boot `fortuna_ledger::connect()` auto-migrates unconditionally, so a status command would either mutate schema or be theater. | — |
| ~~`fortuna mode`~~ | **CUT (A6), never built.** Replaced by the `status` "config on disk: venue/mode (daemon may differ until restart)" line. Mode changes = edit config, then `stop && start`. | — |

Four new commands (start, stop, logs, config check); `status` extended;
halt/rearm/kill unchanged.

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

> AS BUILT: there is no migration pre-flight (A6) and no metrics section
> (deferred, A6). The flows below are corrected to the shipped binary.

`start`: config check (fail->exit 1) -> per-component already-running check
(all running -> idempotent exit 0; stale pidfiles removed; mid-claim -> bail) ->
A2 unmanaged-recorder refusal -> claim pidfile (O_EXCL) + spawn daemon w/ append
log redirect -> spawn recorder likewise -> A8 active-halt visibility +
best-effort lifecycle audit row -> print pids.

`stop`: per component: read pidfile -> not running / stale? "already stopped"
-> running? write `.stopping` marker, SIGTERM, wait <=N s -> clean? remove
pidfile (+ for the daemon, confirm the A1 shutdown line) : warn (NEVER SIGKILL),
leave process + pidfile + marker -> best-effort audit row; any warning -> exit
non-zero.

`status`: always the process-health section (name-validated pidfiles); a
"config on disk" line; then a DB section (active halts; recent halt/gate/order
rows; A8 audit-age crash-tell) if DATABASE_URL is set and Pg answers within 5s.
No metrics section (deferred).

## 6. Dependency changes

Add `fortuna-ops = { path = "../fortuna-ops" }` to fortuna-cli (for
FortunaConfig). Pidfiles via std::fs; spawning via std::process::Command.
AS BUILT: `nix` is NOT a workspace dep, so SIGTERM is the shell-out
`kill -15 <pid>` (`send_sigterm`); std's `Child::kill` is SIGKILL and is never
used here. `toml` is also a dep (recorder invocation override + the
`config on disk` line read raw `toml::Value`).

## 7. Audit and safety rules

1. `start`/`stop` write best-effort `"lifecycle"` audit rows (action, pids,
   `$USER` as actor) when DB reachable; failures warn, never block.
2. `start` refuses on config-check failure, on a mid-claim pidfile, or on an
   unmanaged recorder (A2). No migration pre-flight (A6 — cut).
3. `stop` never SIGKILLs; timeout => exit non-zero, process left for the operator.
4. `stop` idempotent. (`mode` was cut, A6; `status`'s config-on-disk line is the
   read-only replacement.)
5. Existing halt/rearm/kill semantics untouched.

## 8. Mode-change policy (justification)

Audit (I5): a restart creates the clean boundary — boot reconciliation records
config at startup; hot-swap would need re-validation, venue reconnection, open-
order reconciliation, and atomic strategy swap mid-trade for zero benefit
tonight. Demo flip stays: `stop` -> edit config -> `config check` -> `start`.

## 9. Testing plan

AS BUILT — `crates/fortuna-cli/tests/cli_integration.rs` exists and is the
A9 plan, NOT the easy list once sketched here (no `mode`/`db migrate-status`
tests — those commands were cut). It drives the real binary with a temp
runtime dir and stub component binaries, covering: `config_check_*`
(accepts example, rejects bad TOML, missing file fails);
`status_*` (no processes exits 0; live pidfile reads running; dead-pid /
name-mismatch / malformed pidfiles read stale; stopping marker shows
"stopping since"; DB-unreachable still exits 0); `logs_*` (unknown / missing
component, missing file, prints last 50); `start_*` (config check first;
refuses on an unmanaged recorder; idempotent when all running; foreground
exec + exit propagation; mid-claim pidfile is contended not stale);
`stop_*` (idempotent; graceful daemon confirms the log line and cleans up;
exit without the shutdown line is NOT success; pre-existing marker lines in
the append log are ignored; timeout warns/proceeds/leaves state; never
signals a name-mismatched pid; recorder needs no log line); and
`usage_names_new_commands`. The A3/A4 primitives (claim race, append-mode
redirect, pidfile classification, zombie liveness, log-offset) are unit-tested
inline in `main.rs`. The start->status->stop smoke against REAL release
binaries stays a manual runbook check (§13; real process forking is timing-flaky
in CI). The A1 SIGTERM contract is asserted in `fortuna-live`'s own DST/smoke
tests (see the AMENDMENT A1 bracket), the ship gate for `stop`.

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
| crates/fortuna-cli/src/main.rs | 4 new command arms (start, stop, logs, config check); status extended; usage string updated |
| crates/fortuna-cli/tests/cli_integration.rs | NEW — Section 9 tests, written first |
| GAPS.md / ASSUMPTIONS.md | SIGTERM/MIGRATOR fallbacks; runtime-dir choice |

No changes to killswitch, ops, ledger, invariants, or any other crate.

## 13. Manual smoke runbook (Section 9: the start->status->stop check)

Implementer-recorded with the build (2026-06-12). Real-binary forking is
deliberately NOT in CI; the operator (or verifier) runs this once per
release box, from the repo root, with .env sourced:

1. `cargo build --release -p fortuna-cli -p fortuna-live -p fortuna-recorder`
2. STOP the manual recorder first (A2 will refuse otherwise — expected).
3. `FORTUNA_BIN_DIR=target/release target/release/fortuna start`
   — expect: `started daemon (pid …)`, `started recorder (pid …)`,
   active-halts print, no error. data/runtime/{daemon,recorder}.pid exist.
4. `target/release/fortuna status` — both `running (pid …)`; config line.
5. `target/release/fortuna logs daemon` — boot lines, no truncation.
6. `target/release/fortuna stop` — daemon SIGTERM, "clean shutdown
   confirmed in the log" (A1), recorder stopped, pidfiles gone, exit 0.
7. `target/release/fortuna stop` again — both "already stopped", exit 0.
8. Restart the manual recorder if the managed lifecycle is not yet adopted.
