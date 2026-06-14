# Runbook: troubleshooting

**Who this is for:** the operator at 3am with a daemon that won't boot, a
halt that won't clear, or a battery that won't run.
**When to read it:** symptom-first; find your symptom below.
**Status:** accurate as of commit `334612d` (2026-06-12). Every entry here is
a failure mode that actually occurred (ledger/verdict citations inline).

All commands run from the repo root. Times are UTC.

Related: [soak-start.md](soak-start.md) ·
[halt-and-rearm.md](halt-and-rearm.md) ·
[kill-switch-drill.md](kill-switch-drill.md)

---

## 1. Daemon won't boot: `environment rejected …`

Boot is fail-closed on the env contract
([crates/fortuna-live/src/boot.rs](../../crates/fortuna-live/src/boot.rs)).
The error names the offending VARIABLE, never its value:

- `required env var FORTUNA_SLACK_CHANNEL_OPS is not set` — the var is
  missing. Fix `.env`, then reload: `set -a && source .env && set +a`.
- `env var ANTHROPIC_API_KEY holds a placeholder value (contains "replace");
  refusing to boot` — a half-edited `.env`. The placeholder marks are
  `replace`, `changeme`, `your-`, `your_`, `<`, `user:password`, and empty
  (boot.rs `PLACEHOLDER_MARKS` — added precisely because half-configured
  states reached disk in the 2026-06-11 incident).
- `ANTHROPIC_API_KEY is absent and [cognition] allow_stub_mind = false:
  booting would silently run the stub mind…`
  ([crates/fortuna-live/src/main.rs](../../crates/fortuna-live/src/main.rs))
  — either set the key or opt into the stub explicitly in
  `config/fortuna.toml`.

Remember the daemon also loads `.env` itself from the cwd (main.rs
`dotenvy::from_path(".env")`) — running it from the wrong directory loses
that fallback.

## 2. Daemon won't boot: `config rejected` / `boot check failed`

[boot.rs](../../crates/fortuna-live/src/boot.rs) refusals are precise:

- `missing [daemon] section …` / `missing [cognition] section …` — your
  `config/fortuna.toml` predates the current example; re-diff against
  [config/fortuna.example.toml](../../config/fortuna.example.toml).
- The stage-specific Kalshi boot gate (boot.rs): `venue=kalshi requires
  stage=paper …` (kalshi at stage=sim — a mis-wiring), or `venue=kalshi
  stage=LiveMin/Scaled is refused: promotion past Paper needs the
  forward-validation gate (I7)`, or `venue = "kalshi", stage = "paper" requires
  a [kalshi] section with a non-empty series list`. `kalshi`+`paper`+`[kalshi].series`
  BOOTS (the demo composition); see [demo-flip.md](demo-flip.md).
- `halt_poll_ms = … violates the <=500ms halt-poll pin (ASSUMPTIONS)` — the
  halt rail's reaction bound is non-negotiable.
- `venue = "sim" requires a [sim] section with non-empty bracket_sets` —
  the sim daemon needs a market world.

Cheap pre-check that catches all of these without starting anything:
`./target/release/fortuna config check`.

## 3. Daemon won't boot: `postgres connect + migrate`

The error context is literally `postgres connect + migrate` (main.rs).
Check Postgres is up and `DATABASE_URL` points at it:

```
psql "$DATABASE_URL" -c "select 1"
```

**Pending migrations are NOT a refusal:** `fortuna_ledger::connect()`
auto-migrates unconditionally on every boot
([crates/fortuna-ledger/src/lib.rs](../../crates/fortuna-ledger/src/lib.rs),
`MIGRATOR.run`). This is also why the CLI has no `db migrate-status` command
— "a pre-flight refusal the boot path overrides is theater"
([docs/design/fortuna-cli.md](../design/fortuna-cli.md), amendment A6). A
migration FAILURE surfaces in this same boot error.

## 4. `cargo test` fails: `permission denied to create database`

The operator `.env`'s `DATABASE_URL` uses the `fortuna_app` role, which
cannot `CREATE DATABASE` — and `sqlx::test` creates a scratch database per
test. Documented in the t41-completion-gate verdict (Commands note 1:
"`cargo test --workspace` with the operator .env DATABASE_URL (fortuna_app)
fails environmentally: 'permission denied to create database'") and
GATE-FINDINGS-LATEST item 5. An exported shell `DATABASE_URL` OUTRANKS the
repo's `.cargo/config.toml` dev default (.cargo/config.toml [env] force=false default; T4.4 battery
environment note). Fix — run batteries without it:

```
env -u DATABASE_URL cargo test --workspace
```

(or grant `CREATEDB` to a dedicated test role — operator decision). No
operator-DB writes occur in this failure mode; the denial happens first.

## 5. Halt won't clear after `fortuna rearm`

It is not supposed to. Re-arm is RESTART-GATED: the running daemon never
auto-clears a halt; the re-arm takes effect at the next daemon restart
(operator-decisions-2026-06-12.md item 3 + ASSUMPTIONS.md T4.1 rearm entry; pinned by
`a_running_daemon_never_auto_clears_a_halt_on_rearm_only_a_restart_does` in
[crates/fortuna-live/tests/run_loop.rs](../../crates/fortuna-live/tests/run_loop.rs)).
ROTA will keep showing SYSTEM HALTED while `fortuna status` shows
`halts: none` — that exact divergence means "restart pending". Full
sequence and state table: [halt-and-rearm.md](halt-and-rearm.md).

## 6. Slack messages not arriving

Slack delivery failure is COUNTED, never fatal, and never silent: every
routed alert lands as an audit row whether or not Slack accepted it; a send
failure appends `[SLACK SEND FAILED: …]` to that row, and the run's total
surfaces at shutdown as `N Slack alert send(s) failed over this run`
([crates/fortuna-live/src/daemon.rs](../../crates/fortuna-live/src/daemon.rs),
`route_alerts` + the shutdown summary). So: the audit trail is complete even
when the channel is dark. Diagnose with the ROTA Audit tail (grep for
`SLACK SEND FAILED`), then check the bot token / channel ids in `.env`. A
token change requires a daemon restart (env is read at boot —
[key-rotation-and-secrets.md](key-rotation-and-secrets.md)).

## 7. Dead-man pages, or `dead-man ping FAILED` in the log

Two distinct signals:

- `fortuna-live: dead-man ping FAILED: …` in the daemon log — the daemon is
  ALIVE but cannot reach the monitor (main.rs logs it; the heartbeat task
  keeps trying). Check network/URL.
- The monitor PAGES you — the daemon went silent. Silence is the
  escalation of record by design ("the system cannot report its own death",
  [.env.example](../../.env.example)). Check, in order: process alive
  (`./target/release/fortuna status` — a live pidfile with a stale
  "most recent audit row" age is the crash tell, CLI A8), daemon log, then
  Postgres.

Also remember: the FIRST ping arms the monitor (GAPS.md
"Operator-blocked: credentials") — a daemon started briefly and stopped will
page you later. That page is real, not spurious.

## 8. `halt-state poll FAILING — halt rail blind` Ops alert

The daemon could not poll halt state and is trading on last-known state; it
alerts once per failing→ok transition, not per segment
([daemon.rs](../../crates/fortuna-live/src/daemon.rs)). Treat as an
incident: check Postgres first. If you cannot restore the halt rail
promptly, stop the daemon (SIGTERM — graceful) rather than run blind.

## 9. Where the logs live

- Managed lifecycle (`fortuna start`): `data/runtime/logs/daemon.log` and
  `data/runtime/logs/recorder.log`, APPEND-mode across restarts (crash
  backtraces survive); `FORTUNA_RUNTIME_DIR` overrides the base dir
  ([crates/fortuna-cli/src/main.rs](../../crates/fortuna-cli/src/main.rs)).
  Tail: `./target/release/fortuna logs daemon -f`.
- Foreground daemon (the soak-start contract): stderr of your terminal —
  there is no managed log, and `fortuna stop` will honestly warn it cannot
  confirm the clean-shutdown line for such a daemon (ASSUMPTIONS.md, T4.4
  slice 3).
- The audit TRAIL (I5) is in Postgres, not in any log file; the logs are
  convenience, the audit rows are the record.

## 10. `cargo` hangs: `Blocking waiting for file lock on build directory`

Cargo serializes builds that share a target dir. In this repo that is by
design in two places: parallel agent sessions share gate builds through ONE
`CARGO_TARGET_DIR=/tmp/fortuna-gate-target` ("cargo's lock serializes
concurrent gate builds; therefore GATES RUN ONE AT A TIME, queued" —
[docs/design/orchestration.md](../design/orchestration.md) §4b), and any two
commands in the same checkout contend on `target/`. The lock is not a hang:
wait for the other build, or check `ps | grep cargo` to see who holds it.
Do NOT delete lock files of a live build. Related hazard with history:
this machine has hit ENOSPC twice from concurrent battery builds
(GAPS.md ENOSPC incident entry, 2026-06-12: ~8GB per track battery) — if a
build dies with ENOSPC, free space (`cargo clean` of YOUR OWN target only;
never `data/perishable`, never another session's target) and rebuild.

## 11. `fortuna start` refuses or warns

- `refusing to start: unmanaged fortuna-recorder process(es) […]` — by
  design (two appenders can tear JSONL lines in the B0 dataset). Stop the
  manual recorder once, re-run `fortuna start`; managed from then on (CLI
  A2).
- `another start appears to be in progress (pidfile … is claimed but
  empty)` — a concurrent `start` is mid-claim; re-run shortly. Only remove
  the pidfile if you are certain no start is running (CLI message text).
- `stopped (stale pidfile: …)` lines in `fortuna status` — informational;
  `start`/`stop` clean stale pidfiles themselves.

## When to stop and escalate

- Any failure that smells like an invariant (a halt clearing on its own, an
  audit row missing for an action that happened, an order without a gate
  decision) → STOP, do not work around it; record in GAPS.md and treat the
  implementation as wrong until proven otherwise (CLAUDE.md, protected
  directory doctrine).
- Repeated ENOSPC → operator disk decision (the 35GB main-checkout target
  is cleaned only at operator-approved idle windows, orchestration.md §4b).
