# Runbook: starting the Phase-4 EXIT soak

**Who this is for:** the operator starting (or restarting) the 7-day Sim soak.
**When to read it:** before the first start, and again at any restart during the
soak window.
**Status:** accurate as of 2026-06-13. The soak GO verdict is
[docs/reviews/soak-go-gate-2026-06-12.md](../reviews/soak-go-gate-2026-06-12.md)
("GO. The daemon at 8ea8a4d is fit to start the 7-day Phase-4 EXIT soak NOW").
The start is an operator action by design (BUILD_PLAN T4.1: outward-facing
secrets + a release build belong to the human).

All commands run from the repo root. Times are UTC.

Related: [halt-and-rearm.md](halt-and-rearm.md) ·
[troubleshooting.md](troubleshooting.md) ·
[kill-switch-drill.md](kill-switch-drill.md)

---

## 1. Build the release binaries

```
cargo build --release -p fortuna-live -p fortuna-cli
```

`fortuna-live` is the daemon the soak runs
([crates/fortuna-live/src/main.rs](../../crates/fortuna-live/src/main.rs));
`fortuna-cli` builds the `fortuna` operator binary used for `status`, `halt`,
`rearm`, `logs`, and `stop`
([crates/fortuna-cli/src/main.rs](../../crates/fortuna-cli/src/main.rs)).
As of `334612d` no release binaries exist in `target/release/` — this step is
required, not optional.

## 2. Put the config in place and check it

```
cp -n config/fortuna.example.toml config/fortuna.toml
./target/release/fortuna config check
```

Success looks like: `config OK: config/fortuna.toml`.

`config/fortuna.toml` is gitignored; the committed shape is
[config/fortuna.example.toml](../../config/fortuna.example.toml), which since
commit `304f746` carries everything the soak composition needs (the
`[review]` thresholds, the `synthesis_cents` envelope, and
`[gates.per_strategy.synthesis]`). `[daemon] venue` stays `"sim"` — the boot
check refuses `"kalshi"` until T4.2 fixture clearance
([crates/fortuna-live/src/boot.rs](../../crates/fortuna-live/src/boot.rs),
`validate_bootable`; see [demo-flip.md](demo-flip.md)).

## 3. Provision the environment — names only; values are the operator's

The daemon's env contract is `validate_env` in
[crates/fortuna-live/src/boot.rs](../../crates/fortuna-live/src/boot.rs). The
required variable NAMES (shape committed in
[.env.example](../../.env.example)):

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | Postgres (connect + auto-migrate) |
| `FORTUNA_SLACK_BOT_TOKEN` | Slack bot token |
| `FORTUNA_SLACK_CHANNEL_TRADING` | channel id |
| `FORTUNA_SLACK_CHANNEL_ALERTS` | channel id |
| `FORTUNA_SLACK_CHANNEL_REVIEW` | channel id |
| `FORTUNA_SLACK_CHANNEL_DIGEST` | channel id |
| `FORTUNA_SLACK_CHANNEL_OPS` | channel id |
| `FORTUNA_DEADMAN_URL` | external dead-man monitor ping URL |
| `ANTHROPIC_API_KEY` | optional — absent + `[cognition] allow_stub_mind = false` refuses boot (main.rs); absent + `true` runs the inert StubMind |

**OPERATOR-JUDGMENT** — provisioning real values. Preconditions: `.env` is
gitignored and `chmod 600`; no value is ever pasted into a config file, a doc,
or a log ([key-rotation-and-secrets.md](key-rotation-and-secrets.md)). Note
that empty values and placeholder spellings (`REPLACE`, `changeme`, `your-`,
`<`, `user:password`) refuse boot by design (`PLACEHOLDER_MARKS`, boot.rs) —
a half-edited `.env` cannot start the daemon.

Then load it into the shell:

```
set -a && source .env && set +a
```

## 4. Start the daemon

**OPERATOR-JUDGMENT** — this starts the soak clock, begins spending the
`[cognition]` budgets if `ANTHROPIC_API_KEY` is set, and the FIRST dead-man
ping ARMS the external monitor (GAPS.md "Operator-blocked: credentials":
arming it and then stopping produces a false "down" page). Precondition: you
intend a continuous multi-day run starting now.

```
./target/release/fortuna-live config/fortuna.toml
```

This is the gate-verified start contract
([soak-go-gate-2026-06-12.md](../reviews/soak-go-gate-2026-06-12.md),
"OPERATOR START COMMAND"). Note: `fortuna start` (the managed lifecycle) also
exists, but on this machine it will REFUSE while the operator's unmanaged
`fortuna-recorder` process is running (the A2 collision rule —
ASSUMPTIONS.md, T4.4 slice 2); the direct command above is the soak path
until the recorder is migrated to managed mode.

## 5. What healthy boot output looks like

These lines, in this order, on stderr (verbatim from
[crates/fortuna-live/src/main.rs](../../crates/fortuna-live/src/main.rs)):

```
fortuna-live: synthesis mind = AnthropicMind (live; model from [cognition])
fortuna-live: composed (venue=sim, markets from [sim], journal+audit in Postgres)
fortuna-live: metrics at http://127.0.0.1:9187
fortuna-live: dead-man heartbeat armed (pings the monitor every interval)
fortuna-live: Slack routing active (alerts -> #fortuna-alerts/#fortuna-ops)
```

With no `ANTHROPIC_API_KEY` the first line is instead
`fortuna-live: synthesis mind = StubMind (no ANTHROPIC_API_KEY; inert)`.

Failure looks like an immediate exit with one of: `environment rejected …`
(env contract), `config rejected` / `boot check failed` (config contract), or
`postgres connect + migrate` (database) — diagnose via
[troubleshooting.md](troubleshooting.md). A boot that runs but prints
`fortuna-live: ROTA read pool unavailable — audit tail degrades to empty` is
DEGRADED, not healthy: the dashboard's audit tail is blind. Investigate
Postgres before trusting the run.

## 6. Confirm ROTA and the metrics endpoint are live

With the daemon running:

```
curl -fsS http://127.0.0.1:9187/api/rota/v1/health
curl -fsS http://127.0.0.1:9187/metrics | head -n 5
```

Success: the first returns JSON containing `"halt_active": false`; the second
returns Prometheus text. Open `http://127.0.0.1:9187/rota` in a browser for
the operator console (routes in
[crates/fortuna-ops/src/rota.rs](../../crates/fortuna-ops/src/rota.rs)). Both
endpoints are GET-only by design. The bind address is `[daemon] metrics_bind`
(default `127.0.0.1:9187`,
[config/fortuna.example.toml](../../config/fortuna.example.toml)).

## 7. The ten soak-watch metrics

From the GO verdict
([soak-go-gate-2026-06-12.md](../reviews/soak-go-gate-2026-06-12.md),
"SOAK-WATCH METRICS"). Log each watch firing to `docs/reviews/soak-log.md`
(the file does not exist as of `334612d` — create it at the first firing; it
is the soak's log of record).

| # | Metric | Where it surfaces |
|---|---|---|
| 1 | Daemon uptime; restarts noted (each restart re-fires the weekly/monthly reviews — EXPECTED, per the verdict's Info finding) | ROTA Health panel (`ticks`); soak-log |
| 2 | Halt count — zero UNEXPLAINED; every `halt_events` row gets a cause | ROTA Health (`halt` pill) + Audit tail; [halt-and-rearm.md](halt-and-rearm.md) |
| 3 | Mind budget burn vs `[cognition]` daily/per-cycle budgets | ROTA Cognition panel (`spend today` vs `daily budget`) |
| 4 | Dead-man freshness (pings every `[deadman] ping_interval_secs` = 60s) | the external monitor itself; ROTA Health `dead-man` |
| 5 | Belief-persistence growth (`beliefs` table monotone under the synthesis arm) | ROTA Cognition (recent beliefs); Postgres `beliefs` |
| 6 | Exactly ONE `journal` row per UTC day (unique day index); reconciliation skip/failure audit rows are the honest-degrade signal | Postgres `journal`; ROTA Audit tail |
| 7 | `weekly_review` audit row + #digest summary once per Monday-aligned week (boot fire on day one, then 2026-06-15, -22, -29 for a soak started at the verdict date) | Slack #digest; ROTA Audit tail |
| 8 | `monthly_review` audit row only if the soak crosses 2026-07-01 (plus the boot fire) | Slack #digest/#ops; ROTA Audit tail |
| 9 | `[SLACK SEND FAILED:` audit rows + the shutdown summary row (`N Slack alert send(s) failed over this run`) | ROTA Audit tail; daemon log |
| 10 | Metrics endpoint reachable (GET-only) + halt-poll-failure alerts ABSENT | step 6 curls; absence of `halt-state poll FAILING` in #ops/audit |

## 8. Stopping (end of soak or for a deliberate restart)

The shutdown contract is SIGTERM == SIGINT == graceful: cancel working
orders, write the final audit row, exit (main.rs SIGTERM CONTRACT). For a
foreground daemon, Ctrl-C. For a managed daemon, `fortuna stop`. Success
looks like this line on stderr / in the daemon log:

```
fortuna-live: clean shutdown — ticks=… polls=… poll_failures=… halts_applied=… cancelled=… unacked=…
```

Never `kill -9` — the daemon is cancelling working orders on the way down
(the `fortuna stop` timeout warning text,
[crates/fortuna-cli/src/main.rs](../../crates/fortuna-cli/src/main.rs)).

## When to stop and escalate

- Any halt you cannot explain from the audit tail → STOP, follow
  [halt-and-rearm.md](halt-and-rearm.md); do not re-arm before the cause is
  written down.
- `halt-state poll FAILING — halt rail blind` Ops alert → the daemon is
  trading on last-known halt state; treat as an incident
  ([troubleshooting.md](troubleshooting.md)).
- Dead-man monitor pages → the daemon stopped beating; check process,
  then logs, then Postgres — in that order.
