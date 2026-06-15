# Runbook: halt and re-arm (I2)

**Who this is for:** the operator responding to a halt (drawdown, runaway,
audit failure, or a manual halt), or running a halt drill.
**When to read it:** BEFORE the first halt drill of the soak — the re-arm
semantics are not what most people expect.
**Status:** accurate as of 2026-06-13.

The one-sentence version: **a re-arm does NOT resume a running daemon.**
Resumption requires a restart. That is invariant I2 ("Drawdown halts with
human re-arm … No automatic resumption") read conservatively — the
adjudication is the ASSUMPTIONS.md entry "T4.1 — daemon halt re-arm is
RESTART-GATED", pinned by the executable test
`a_running_daemon_never_auto_clears_a_halt_on_rearm_only_a_restart_does`
([crates/fortuna-live/tests/run_loop.rs](../../crates/fortuna-live/tests/run_loop.rs)).

All commands run from the repo root with `DATABASE_URL` exported (`halt` and
`rearm` require it; [crates/fortuna-cli/src/main.rs](../../crates/fortuna-cli/src/main.rs)).
Build the CLI first if needed: `cargo build --release -p fortuna-cli`.

Related: [demo-bringup.md](demo-bringup.md) ·
[soak-start.md](soak-start.md) ·
[kill-switch-drill.md](kill-switch-drill.md) ·
[troubleshooting.md](troubleshooting.md)

---

## Command syntax (as built)

From the CLI header
([crates/fortuna-cli/src/main.rs](../../crates/fortuna-cli/src/main.rs)):

```
fortuna halt   <global|strategy:<id>|venue:<id>> --reason "..." --operator <name>
fortuna rearm  <global|strategy:<id>|venue:<id>> --reason "..." --operator <name>
fortuna status
```

`--reason` and `--operator` are both REQUIRED — operator actions are
attributed (the CLI refuses without them). Both verbs write a durable
`halt_events` row plus an audit row (I5). Halt/re-arm are CLI-ONLY by
design: Slack may request a halt, but no re-arm verb exists over Slack — a
compromised Slack token must not be able to un-halt the system (CLI header
comment; I2).

### A kill-switch revocation is NOT cleared by `rearm`

A drawdown/runaway/manual halt is cleared by `rearm` (this runbook). A
**kill-switch revocation** is a SEPARATE, durable halt and is NOT: after a live
`fortuna kill` / standalone `freeze`/`flatten-perps`, the switch leaves a
`KILLSWITCH_REVOKED` sentinel file that holds a global halt for as long as it
exists — across restarts (the daemon's halt poller checks it before every tick).
`rearm` does NOT remove that sentinel. Clearing it is operator-only, out-of-band,
and CLI-only: `fortuna-killswitch clear-revocation --journal <path>`, THEN re-arm
(if any other halt is also standing) and restart. The full flow lives in
[kill-switch-drill.md](kill-switch-drill.md) §5. So if the daemon stays halted
after a clean `rearm` + restart and the cause was a kill, the sentinel is still
present — clear it first.

## The correct full sequence

```
halt  ->  investigate  ->  rearm  ->  fortuna stop  ->  fortuna start
```

### 1. Halt

**OPERATOR-JUDGMENT** — this stops new trading. Precondition: you intend it
(an incident, or a scheduled drill).

```
./target/release/fortuna halt global --reason "drill: first soak halt drill" --operator xavier
```

Success looks like:

```
halt set on global; the runner enforces it within its poll interval
```

The running daemon applies it within `halt_poll_ms` (pinned ≤ 500ms,
[config/fortuna.example.toml](../../config/fortuna.example.toml) `[daemon]`).
Gate-driven halts (drawdown I2, runaway I3, audit-append failure) set the
same flag without your help; in those cases start at step 2.

### 2. Investigate

Do not touch `rearm` until the cause is understood and written down.

```
./target/release/fortuna status
```

Read the `recent halt:` audit rows and the ROTA Audit tail. A standing halt
audits exactly once across segment boundaries (test
`a_standing_halt_audits_exactly_once_across_segment_boundaries`,
run_loop.rs) — one row per cause, not a flood.

### 3. Re-arm

**OPERATOR-JUDGMENT** — this is THE human re-arm path (I2). Precondition:
the cause is diagnosed and the `--reason` records the disposition, not just
"rearm".

```
./target/release/fortuna rearm global --reason "drill complete; cause: manual drill halt" --operator xavier
```

Output: `re-armed global (operator: xavier)`.

**Known gap (open as of `334612d`):** this output does NOT yet say "pending
daemon restart", and ROTA has no re-arm-pending notice — the M3 finding
(docs/reviews/GATE-FINDINGS-LATEST.md item 3; still open per the soak-go
verdict's findings). Until that lands, this runbook is the notice.

### 4. Restart the daemon — re-arm takes effect ONLY here

**OPERATOR-JUDGMENT** — a restart re-fires the weekly/monthly reviews
(fire-on-boot pattern; expected, per the soak-go verdict's Info finding) and
counts as a soak restart in the soak log.

Managed lifecycle:

```
./target/release/fortuna stop
./target/release/fortuna start
```

Foreground daemon: Ctrl-C (or SIGTERM the pid), confirm the
`fortuna-live: clean shutdown` line, then start again per
[soak-start.md](soak-start.md) step 4. The boot fold reads the
`halt_events` set→rearm history and comes up clear (ASSUMPTIONS.md T4.1
entry).

## What ROTA shows in each state

ROTA's halt indicator comes from the RUNNING daemon's gate state
([crates/fortuna-live/src/views.rs](../../crates/fortuna-live/src/views.rs)
`runner.active_halt()`), not from the database. `fortuna status` reads the
database fold. They diverge by design between re-arm and restart:

| State | ROTA (`/rota`) | `fortuna status` |
|---|---|---|
| Running, clear | Health: `halt` pill **clear** | `halts: none` |
| Halted | Full-screen red **SYSTEM HALTED** takeover + `HALTED` pill + reason ([crates/fortuna-ops/src/rota.rs](../../crates/fortuna-ops/src/rota.rs), `#halt` overlay) | `halts (1): global — <reason>` |
| Re-armed, NOT yet restarted | **still SYSTEM HALTED** — the running daemon never auto-clears (restart-gated) | `halts: none` |
| Restarted after re-arm | clear | `halts: none` |

If ROTA says HALTED and `fortuna status` says none, that is not a bug — it
means a re-arm is pending a restart. Restart.

## When to stop and escalate

- A halt with no `halt_events` cause row → I5 problem; treat as an incident,
  do not re-arm.
- The halt CLEARS without your restart → that would violate the
  restart-gated pin; STOP everything and record it (the invariant test in
  run_loop.rs says this cannot happen — if it did, the implementation is
  wrong).
- Venue unreachable while the daemon holds working orders → this runbook
  does not cover it; see [kill-switch-drill.md](kill-switch-drill.md)
  ("when to use the real kill").
- Daemon still halted after a clean `rearm` + restart, and the prior event was
  a kill → a `KILLSWITCH_REVOKED` sentinel is still present; `rearm` does not
  clear it. Run `fortuna-killswitch clear-revocation --journal <path>`, then
  restart ([kill-switch-drill.md](kill-switch-drill.md) §5).
