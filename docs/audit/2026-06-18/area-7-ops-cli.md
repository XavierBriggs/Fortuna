# Area 7 — Operational readiness, demo CLI & observability

## Summary

The CLI entrypoint (`scripts/demo-launch.sh` → `fortuna start`) is a real, working multi-step
shell script — not a single clean `fortuna start paper-demo` command. It does its job but
requires operator setup (`.env`, creds, `config/fortuna.toml`, sqlx, a prior build) and is
six steps wide. ROTA is rich but has a critical safety gap: **execution_mode /
order_mutation_enabled are not surfaced in the Health panel** — the operator has no single-view
confirmation that orders are disabled for the paper-on-live run. WS reconnect and keep-alive
logic is correct and well-tested. Two live failures are confirmed in the daemon log: a DB
permission error on `funding_rates_historical` and a recurring dead-man ping failure, both
unblocked from the demo loop but noisy and confusing to an operator watching the chain.

---

## Findings

| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
|---|---|---|---|---|---|---|---|
| P1 | BLOCKS | `execution_mode` / `order_mutation_enabled` never reach the ROTA Health panel for the paper-on-live runner | `crates/fortuna-live/src/daemon.rs:1420-1465` (`paper_data_rota_views` constructs the health JSON), `crates/fortuna-ops/src/rota.rs:2107-2113` (JS renderer reads `halt_active`, `ticks_total`, `dead_man`, `venues` — no order-mutation key). No `order_mutation_enabled` field is emitted or consumed. | A viewer of the demo dashboard cannot tell at a glance whether orders are actually suppressed. If config is misconfigured (e.g. `orders_enabled = true`), ROTA shows nothing alarming. | `paper_data_rota_views` was written to emit the venue-agnostic health subset and was never extended with the ExecPolicy state. `ExecPolicy.order_mutation` is a daemon-internal type, not yet wired into the snapshot. | Add `order_mutation_enabled: bool` (from `runner.exec_policy.order_mutation` or a new `runner.order_mutation_enabled()` accessor) and `execution_mode: String` to the `health` JSON object in `paper_data_rota_views` and to the Sim variant (`views_from`). Add a corresponding `kv("order-mutation", …)` pill in the JS `health(j)` renderer. | Add an integration test asserting that a `PaperLive` daemon's `rota_views` health block contains `order_mutation_enabled: false` when the runtime section sets `orders_enabled = false`. |
| P1 | BLOCKS | `funding_rates_historical` INSERT fails with `permission denied` in the live demo DB | `data/runtime/logs/daemon.log:32`: `"funding poll errored: composition error: funding_rates_historical insert KXBCHPERP @ 2026-06-15T20:00:00.000Z: error returned from database: permission denied for table funding_rates_historical"` | The funding poller is silently crippled: no funding rates accrue. The §9.2 perps ROTA panel shows empty/unavailable data even though the poller shows "ACTIVE" on every restart. An operator reading ROTA cannot distinguish "running but blocked" from "not yet started". | `fortuna_app` user lacks GRANT on `funding_rates_historical` in `fortuna_demo` (the table likely exists but only the migration role owns it). The daemon does not halt on this failure — it counts errors and continues — but the failure recurs every poll. | Run `GRANT INSERT, SELECT ON funding_rates_historical TO fortuna_app;` in `fortuna_demo`. Add this to the demo DB setup runbook. | DST scenario: funding poller receives a `permission denied` response; assert the error counter increments and an alert fires, but the daemon loop continues. |
| P1 | BLOCKS | No `fortuna start paper-demo` command; demo startup requires 6+ operator steps | `scripts/demo-launch.sh` (the real entry point, 190 lines). The CLI command list in `crates/fortuna-cli/src/main.rs:8-17` is `status | halt | rearm | kill | config check | logs | start | stop` — no `paper-demo` verb, no `doctor` verb. `demo-launch.sh` calls `fortuna config check`, a market refresh script, optional `sqlx migrate run`, a kill-switch clear prompt, `fortuna start`, and then `caffeinate`. | A demo viewer or fresh operator cannot boot the system without reading the runbook. A misconfigured step silently diverges. The `fortuna start` command does not validate or print the paper-on-live mode on startup. | The `paper-demo` composition path was never promoted into the CLI as a named subcommand. It lives only in the shell script. | Add a `fortuna doctor` command that validates all prerequisites (`.env`, creds, DB reachable, kill-switch wired, release binaries present) and prints a clear PASS/FAIL. Separately, a `fortuna start paper-demo` alias (or at minimum a `--mode paper-demo` flag) that sets `FORTUNA_DEMO_DATABASE_URL`, runs the readiness gates, and launches in one command. | Integration test: `fortuna doctor` exits non-zero and names the missing dependency when `.env` is absent. |
| P2 | SERVES | Dead-man ping failure observed in live log, but cause is transient network — heartbeat recovers | `data/runtime/logs/daemon.log:153`: `"fortuna-live: dead-man ping FAILED: transport failure: error sending request"`. Immediately before: perp-tick poll timeout at line 152. One occurrence in the log; subsequent lines (157+) show the heartbeat re-arming normally. | One failed heartbeat is not an outage, but the Health panel shows `dead_man_last_ping_age_secs: null` for PaperLive (hardcoded null, `daemon.rs:1433`). An operator watching ROTA sees "—" regardless of whether the heartbeat is healthy. | `paper_data_rota_views` hardcodes `"dead_man_last_ping_age_secs": serde_json::Value::Null` (`daemon.rs:1433`) because the dead-man state lives in the Sim daemon path, not the paper-live path. The dead-man task IS running (the log shows repeated "armed" messages), but its last-ping age is never plumbed through. | Expose the dead-man `last_ping_age_secs` from the paper-live composition (the spawned deadman task already tracks it; thread it through the ActiveRunner like `counters()`) and emit it in `paper_data_rota_views`. | Assert: after 1 heartbeat interval, the snapshot's `health.dead_man_last_ping_age_secs` is non-null. |
| P2 | SERVES | `data/runtime/current-demo-db-url` pointer is stale and will mislead | `data/runtime/current-demo-db-url` contains `postgres://localhost/fortuna_demo_paper_green_20260617044732`. The AUDITOR-BRIEF says this file is stale; confirmed: that DB name embeds a datestamp from 2026-06-17. | Any script or operator action that reads this file will point at a long-gone DB. If the file is ever used programmatically (e.g. a future `fortuna doctor` or CI script), it silently diverges from the real `fortuna_demo`. | The file was presumably written by a prior demo session and never updated. No code in the current working tree writes or reads it (MISSING reader: searching `crates/` and `scripts/` shows no consumer). | Remove `data/runtime/current-demo-db-url` from the tree or add it to `.gitignore`. If the pointer is useful for debugging, let `demo-launch.sh` overwrite it on every boot. | |
| P2 | SERVES | perp-tick producer hitting a real Kinetics URL and timing out repeatedly in the demo | `data/runtime/logs/daemon.log:85`: `"perp_tick_producer: public GET /margin/markets/KXBTCPERP: error sending request"`, and line 152: `"operation perp_tick_producer: public GET /margin/markets/KXBTCPERP timed out"`. | The perp strategies are opt-in (`[perp_event_basis]` / `[perp_event_basis_v2]` present in config), so the producer fires against the real Kinetics external API. Repeated timeouts generate noise in the daemon log and inflate the venue API error counter, which ROTA's health panel reads (`c.venue_api_errors`) and shows "errors" in red. An operator sees a red venue pill for a non-fatal demo-period network hiccup. | The perp-tick producer uses the same `venue_api_errors` counter as the main trading path. If the external Kinetics URL is intermittently unreachable (demonstrated by the log), the health panel degrades even though no capital is at risk. | Add a separate `perp_api_errors` counter that does not fold into the main `venue_api_errors` used by the Health panel, OR display the healthy trading-path error count separately from the perp feed. Add a ROTA note that perp feed errors are non-fatal. | |
| P2 | SERVES | WS reconnect implemented but the live TLS dial is not yet wired (GAPS item) | `crates/fortuna-venues/src/kalshi/dial.rs:1-18` (module doc): "The async transport, the signed handshake, and the ping/pong keep-alive timer that DRIVE this state machine are the next 2(i) slice (ledgered in GAPS); nothing here opens a socket." `WsTransport` trait exists with a mock; no production `tokio-tungstenite` impl is present. | `WsDial` / `KeepAlive` / `pump_session` / `run_dial` are fully unit-tested and correct, but the actual live Kalshi WS market-data feed is not connected. The paper-on-live path uses REST polling (`tick()` → `venue.markets()` + book GETs at `tick_interval_ms`). No WS stream feeds the demo today. | Deliberate phased delivery — the decision logic was written first, the transport is a ledgered next slice. | Wire `ReqwestKalshiWsTransport` (the tokio-tungstenite impl) and call `run_dial` from the live boot path. Until then, note in the demo runbook that market data arrives via REST polling, not WS. | Integration test: `run_dial` with the real TLS transport against the Kalshi demo WS host (fixture-replayable). |
| P2 | SERVES | ROTA Health panel shows `stage: "paper"` hardcoded for PaperLive; no `execution_mode` or `orders_enabled` confirmation | `daemon.rs:1423`: `"stage": "paper"` is a string literal in `paper_data_rota_views` | See P1 finding above. Beyond the stage string, no indication whether this is `live_data_only`, `paper_ledger`, or `demo_orders`. | Same root cause as the P1 order-mutation finding. | Same fix — add `execution_mode` to the health JSON. | |
| P3 | BLOAT-cut | `scripts/demo-launch.sh` optional `sqlx migrate run` prints nothing on success | `scripts/demo-launch.sh:131`: `sqlx migrate run ... >/dev/null`. Success is invisible; an operator may not know whether migrations ran. | Minor UX: an operator watching the boot sequence cannot distinguish "migrations applied" from "sqlx not found; relying on daemon boot migration" (which does print). | The stdout suppression was presumably done to avoid noise. | Print a single confirmation line (e.g. `[demo] migrations applied`) on success, matching the pattern of the other `echo "[demo] …"` lines. | |
| P3 | PARK | No `[gates.rate."paper-live"]` applied check in demo-launch config validation | `config/fortuna.example.toml` has both `[gates.rate.kalshi]` and `[gates.rate."paper-live"]`; `fortuna config check` validates the config shape but does not assert that the rate-limit section for the running venue is present. | If an operator forgets `[gates.rate."paper-live"]`, the gate silently defaults. This is a P3 because the default exists and the gate is functional; it is a config completeness gap. | `FortunaConfig::validate_bootable` checks venues but not per-venue rate sections. | Add a boot validation that the active venue has a `[gates.rate.<venue>]` entry, warning (not refusing) if absent. | |

---

## Trace / narrative

### Demo entrypoint

The operator-facing entry point is `scripts/demo-launch.sh` (`scripts/demo-launch.sh:1-189`). It
performs in order: (1) option parsing, (2) config path + DB URL resolution with a demo-DB
override chain (`*/fortuna` → `*/fortuna_demo`), (3) `scripts/refresh-demo-markets.sh` call,
(4) `fortuna config check`, (5) optional `sqlx migrate run`, (6) kill-switch sentinel check with
a typed confirmation prompt, (7) `fortuna start`, (8) pid-file validation, (9) `caffeinate` for
the daemon PID, (10) 5-second wait + `fortuna status` + pid liveness check.

The `fortuna` CLI (`crates/fortuna-cli/src/main.rs:8-17`) offers exactly these verbs:
`status`, `halt`, `rearm`, `kill`, `config check`, `logs`, `start`, `stop`. There is no `doctor`,
no `paper-demo`, no `check-ready`. The claim in the session evidence is confirmed.

The `fortuna start` command (`main.rs:470-595`) performs a config check, validates pidfiles
(A3 atomic claim), refuses if an unmanaged `fortuna-recorder` exists, spawns `fortuna-live`
and `fortuna-recorder`, and optionally writes a lifecycle audit row. It does **not** print the
execution mode or paper-vs-live confirmation during boot.

### ROTA view coverage

ROTA has 26 routes (`rota.rs:63-94`). The `health` view is a snapshot passthrough
(`view_health` at line 114). For the PaperLive path, `paper_data_rota_views`
(`daemon.rs:1400-1466`) populates the `health`, `gates`, `streams`, `money`, and `settlement`
snapshot keys. The health key (`daemon.rs:1421-1438`) emits: `stage` (hardcoded "paper"),
`venue`, `halt_active`, `halt_reason`, `rearm_requires_restart`, `ticks_total`,
`last_tick_age_ms` (null), `fill_latency_*`, `dead_man_last_ping_age_secs` (null), and
`venues[]`. **Absent:** `execution_mode`, `order_mutation_enabled`, `orders_enabled`.

The ROTA JS health renderer (`rota.rs:2107-2113`) reads `halt_active`, `halt_reason`,
`rearm_requires_restart`, `ticks_total`, `fill_latency_*`, `dead_man_last_ping_age_secs`, and
`venues`. It emits no order-mutation pill. A running demo with `orders_enabled = false` looks
identical to one with `orders_enabled = true` from the dashboard.

The `streams` view (`daemon.rs:1445-1449`) emits only `venue_api_errors_total` and
`venues[{id}]` for PaperLive — no stream-health detail for WS (which is not live) and no
breakdown between trading-path errors and perp-feed errors.

### WS reconnect

`WsDial` (`dial.rs:59-113`) implements capped-exponential backoff: base 500ms, cap 30s, reset
on clean connect. `KeepAlive` (`dial.rs:142-176`) pings every `ping_interval`, declares the
socket dead after `pong_deadline`. `pump_session` + `run_dial` (`dial.rs:202-329`) are fully
implemented state machines with scripted-mock unit tests covering the recorded venue evidence
(reset-without-close + 502 sequence, `dial.rs:426-494`). **However**, the production WS
transport impl (`WsTransport` trait) is not yet wired to a real TLS dial (`dial.rs:14`: "the
async transport … are the next 2(i) slice, ledgered in GAPS"). The demo currently uses REST
polling.

### Rate limits

Config at `config/fortuna.example.toml` defines `[gates.rate.kalshi]` (burst=5,
sustained=20/min) and `[gates.rate."paper-live"]` (identical values). Gate logic
(`crates/fortuna-exec/src/manager.rs`) enforces these as dual token buckets (I3). No retry
logic with exponential backoff is in the gates themselves — a rate-limit breach is a halt, not
a throttle, per spec I3. Confirmed no `f64` money paths in the gate code.

### Timeouts

HTTP timeout for Kalshi demo transport: 20s (`KALSHI_DEMO_HTTP_TIMEOUT_SECS`,
`daemon.rs:515`). Synthesis mind HTTP timeout: 30s (`SYNTH_MIND_TIMEOUT_SECS`,
`daemon.rs:187`). `fortuna status` DB timeout: 5s (`STATUS_DB_TIMEOUT_SECS`, `main.rs:57`).

### Daemon log evidence

Log file `/Users/xavierbriggs/fortuna/data/runtime/logs/daemon.log` confirmed these claims:
- "dead-man heartbeat armed" appears repeatedly (line 4, 14, 27, etc.) — heartbeat task IS
  running.
- "dead-man ping FAILED: transport failure" at line 153 — one observed failure, preceded by a
  perp-tick timeout at line 152. The claim in the session evidence is **confirmed**.
- "funding poll FETCH FAILED: … permission denied for table funding_rates_historical" at line 32
  — confirmed. The poller is ACTIVE (`daemon.log:17`) but its first insert failed with a DB
  permission error. This recurs.
- "perp-tick poll FETCH FAILED" at lines 85 and 152 — confirmed, external Kinetics URL errors.

### Stale pointer

`data/runtime/current-demo-db-url` contains `postgres://localhost/fortuna_demo_paper_green_20260617044732`.
This is stale — that database name appears nowhere in the running codebase. No file in
`crates/` or `scripts/` reads this file (confirmed by grep). The AUDITOR-BRIEF's instruction
to ignore it is confirmed as correct.

---

## Self-adversarial pass

**P1: order_mutation_enabled gap** — This is the most important finding. Counter-argument: the
ExecPolicy state is enforced deep in the exec manager and is architecturally sound; an operator
who reads the config already knows the mode. But the audit target is demo readiness, and a
demo viewer watching ROTA has no way to confirm safety state without reading the config file
— the operationally correct surface IS the dashboard. Severity P1 stands.

**P1: DB permission error** — Could this be a one-time transient rather than a persistent grant
gap? The log shows it on line 32 (an early boot) and the poller has been "ACTIVE" ever since,
suggesting it fires and hits the error on every poll cycle. The error message is
`permission denied for table`, not a transient connection error. Severity P1 stands.

**P1: no `paper-demo` command** — One could argue `scripts/demo-launch.sh` IS the single
entrypoint; the audit target says "`fortuna start paper-demo` = ONE command". The script exists,
which is better than nothing, but it is not a CLI verb. P1 is the right severity for a demo
readiness gate; a viewer seeing the system for the first time cannot discover this without
the runbook.

**P2: WS not wired** — This could be P1 if the demo depends on WS for market data. It does
not: the paper-on-live path uses REST polling, which is functional (the demo has been running).
P2 is correct — this is a known deferred slice, not a demo blocker today.

**Possible miss**: I did not read `docs/runbooks/halt-and-rearm.md` or `kill-switch-drill.md`
in detail. The demo-bringup runbook references both and they are the operator's procedure for
safety-drill verification. A separate audit of those docs against the CLI implementation
(especially the I2 restart-gate behavior and the revocation-file path consistency) would be
worthwhile.

**Possible miss**: ROTA's route-table test (referenced in the module doc: "the route-table test
asserts 405 on every other method") — I did not verify that this test exists and passes. If it
was written but never updated as routes were added, a POST path could exist silently.

---

## Open questions for the Lead

1. **Order-mutation visibility (P1):** Is the intent to surface `order_mutation_enabled` in ROTA
   before the demo, or is the runbook + config file considered sufficient? If the former, which
   team owns the daemon-snapshot plumbing (daemon.rs `paper_data_rota_views` is a fortuna-live
   concern; the JS renderer is ROTA)?

2. **`funding_rates_historical` grant (P1):** Has the `fortuna_app` user been granted on this
   table in `fortuna_demo`? The log says no. Is this a known open item or an oversight?

3. **WS dial gap:** The session evidence says "WsDial bounded backoff at dial.rs" as if WS is
   active. This audit confirms WsDial is implemented but the TLS transport is unbuilt. Should
   the demo runbook be updated to say "REST polling, not WS, for market data" to set operator
   expectations?

4. **`current-demo-db-url` pointer:** Should this file be removed from the working tree, or
   is it used by a tool outside this repo?

5. **`fortuna doctor`:** Is there a planned milestone for a readiness-check command? The demo
   bringup runbook (§0) has a manual checklist; a `doctor` command would enforce it
   programmatically.
