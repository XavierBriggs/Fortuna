# Runbook: ingestion operations (news-aggregation / weather signals)

**Who this is for:** the operator turning on, watching, or recovering the
news-aggregation ingestion subsystem (`crates/fortuna-sources` + the
`crates/fortuna-live` daemon seam).
**When to read it:** BEFORE enabling ingestion for the first time — it is
opt-in, fail-closed, and the source-quarantine re-arm is a different path from
the trading-halt re-arm most operators know.
**Status:** accurate as of the Track-D OBS-1 slice (2026-06-13).

The one-sentence version: **merged code ingests nothing until you opt in**, and
when it is on it is off the money path — a persist failure or a quarantined
source cannot affect trading.

Related: [halt-and-rearm.md](halt-and-rearm.md) ·
[key-rotation-and-secrets.md](key-rotation-and-secrets.md) ·
[troubleshooting.md](troubleshooting.md) ·
[../design/ingestion-observability-contract.md](../design/ingestion-observability-contract.md)

---

## Purpose

Ingestion polls external sources (NWS alerts/AFD, the NWS CLI daily-extreme
grader, RSS feeds, the BLS macro calendar, and the Aeolus forecast vendor),
runs the Layer-1 structural validator on every fetched item, and writes the
accepted signals to the append-only signals store. It runs as its own IO loop
([`run_ingestion_loop`](../../crates/fortuna-live/src/ingestion.rs)) alongside
`drive()` — INDEPENDENT of the deterministic trading cycle. No model is anywhere
on the path (enforced at config validation, not by convention).

## Default state

OFF. The `[ingestion]` config section defaults to absent / `enabled = false`,
and the daemon spawns the loop ONLY when `[ingestion].enabled = true`. With the
section absent the daemon is byte-unchanged (`daemon_smoke` proves it). There is
nothing to disable on a fresh install; there is something to *enable*.

## Enabling live ingestion

Four operator prerequisites, all fail-closed — skip one and the factory or boot
refuses rather than running degraded.

1. **Seed `source_registry` rows.** Each source needs a trust tier (from its
   Layer-0 dossier under `docs/research/sources/<id>/dossier.md`). An enabled
   source with no registry tier is refused by `build_scheduler` ("admit it
   first"). Built dossiers/tiers: `nws`, `rss_fed_press`, `rss_sec_edgar`,
   `calendar_bls`, `nws_climate` (tier 10), `aeolus` (tier 7).

2. **Add `[sources.<id>]` config rows** in `fortuna.toml`. Required keys for an
   enabled source: `kind`, `url` (https only), `base_interval`,
   `rate_budget_per_min`. `feed` is required for `nws` (`alerts` | `afd` |
   `climate`) and `calendar` (`schedule` | `latest`). For Aeolus, also set
   `auth_header = "x-api-key"` and `auth_env = "AEOLUS_API_TOKEN"` (the env-var
   NAME, never the secret). Example:

   ```toml
   [sources.nws_alerts]
   kind = "nws"
   feed = "alerts"
   url = "https://api.weather.gov/alerts/active?area=TX"
   base_interval = "10m"
   rate_budget_per_min = 30

   [sources.aeolus_knyc]
   kind = "aeolus"
   url = "https://forecasts.aeolus.internal/v2/forecasts?station=KNYC&variable=tmax"
   base_interval = "6h"
   rate_budget_per_min = 6
   auth_header = "x-api-key"
   auth_env = "AEOLUS_API_TOKEN"
   ```

3. **Set the secret env var(s).** For Aeolus, export `AEOLUS_API_TOKEN`. The
   library never reads env; the daemon resolves the `auth_env` NAME to its value
   (`|name| std::env::var(name).ok()`). A named `auth_env` that does not resolve
   is a hard error — an authenticated source never silently fetches
   unauthenticated. Secrets stay env-only (never in config, repo, logs, or audit
   payloads); see [key-rotation-and-secrets.md](key-rotation-and-secrets.md).

4. **Set `[ingestion] enabled = true`** plus its knobs:

   ```toml
   [ingestion]
   enabled = true
   tick_ms = 5000           # loop polling granularity; scheduler skips not-due sources
   trigger_floor = 5        # tier >= floor may wake a decision cycle
   volume_envelope = 512    # per-tick accepted-item cap (AFD-firehose containment)
   user_agent = "FORTUNA-ingest (ops@example.com)"
   ```

   `enabled` and `user_agent` are required; `tick_ms` (5000), `trigger_floor`
   (5), and `volume_envelope` (512) default if omitted. The section uses
   `deny_unknown_fields` — a typo is a refusal, not a silent ignore.

## Reading health & telemetry

The scheduler exposes a live snapshot via `telemetry(generated_at) ->
IngestionTelemetry` (the OBS-1 surface, observability-contract §2). It is a pure
projection on the injected `Clock` — no wall-time, no secrets. It carries:

- **per-source `SourceTelemetry`** — `health`
  (`healthy` | `degraded` | `quarantined`), trust tier, last poll / last success
  / next due, and the `SourceMetrics` counters: `polls`, `empty_polls` (the 304
  proxy), `fetch_errors`, `accepted`, `dropped_future`, `dropped_republished`,
  `dropped_over_volume`, `quarantines`, `rearms`, plus a redacted `last_error`;
- **process-wide `FunnelCounts`** — `fetched`, `validated_accepted`,
  `validated_dropped` (the loop-side `normalized` / `deduped` / `persisted`
  stages are OBS-2, still 0 in this snapshot);
- **`recent`** — a bounded (256) newest-first feed of redacted `SignalRecord`s
  (status `accepted` | `dropped:future` | `dropped:republished` |
  `dropped:over_volume`); payload summaries are truncated and quoted, never
  interpreted (spec 5.11).

ROTA views (observability-contract §4): **V1 Live Signal Feed, V2 Sources
Health, V3 Ingest Funnel** read this in-memory snapshot and are live as soon as
track-B wires them. **V4 Source/Vendor Scorecard** depends on the cognition
Layer-3 `source_reliability` table (a weekly trust-attribution job, F9) and
shows "insufficient data (n=…)" until those beliefs settle.

## Quarantine & recovery

A source that fails `quarantine_after` consecutive fetches goes `Quarantined`.
This is LOUD (it raises an Ops alert and increments `quarantines`) and there is
NO automatic resume — a quarantined source is never polled again until an
operator clears it. Backoff before quarantine is deterministic capped
exponential; this honors the spirit of invariant I2's human re-arm.

To recover, after diagnosing the cause, call `rearm(source_id)` on the scheduler
(routed through the daemon's ops surface / `IngestionWiring::rearm`). Re-arm sets
the source healthy, zeroes its consecutive-failure count, makes it due again, and
increments `rearms`. It emits no "Recovered" alert (re-arm is the operator
action, not an auto-recovery).

**This is NOT the trading-halt re-arm.** Two distinct paths:

| | Source quarantine re-arm (here) | Trading halt re-arm |
|---|---|---|
| Scope | One ingestion source | The trading daemon / strategy / venue |
| Invariant | I2 *in spirit* (loud, operator-only, no auto-resume) | I2 proper (drawdown/runaway/audit halt) |
| Action | `rearm(source_id)` on the scheduler | `fortuna rearm <…>` CLI verb |
| Effect | Source pollable again immediately | Records re-arm; takes effect only on daemon RESTART |
| Runbook | this file | [halt-and-rearm.md](halt-and-rearm.md) |

Do not reach for the `fortuna rearm` CLI to clear a quarantined source, and do
not expect an ingestion re-arm to clear a trading halt.

## Safety properties

- **SSRF host-pin + https-only.** Every fetch is pinned to the configured URL's
  host via `HostPin`, built from the same WHATWG URL parser the HTTP client uses
  (the parser-differential SSRF was fixed at root cause); non-https URLs are
  refused at config parse.
- **Secret redaction / env-only.** Auth secrets are resolved from env by the
  binary, marked sensitive, and redacted in `Debug`/logs.
- **No model in the path.** Phase A forbids model extraction on any enabled
  source; the ingestion path is deterministic adapters + validators only.
- **Off the money path.** Ingestion is its own IO loop, independent of the
  trading cycle. A persist failure is counted (`persist_failures`), not fatal;
  a quarantined source cannot affect gates, sizing, or execution.
- **Append-only.** Accepted signals go to the append-only signals store; the
  authoritative dedup is the ledger's `UNIQUE(source, content_hash)`.

## Known limits

- **GDELT deferred (D7).** No `gdelt` adapter yet — an enabled `gdelt` source is
  refused with a hint to use `rss` against GDELT's `format=rss` as the interim.
- **AFD is a firehose.** The NWS Area Forecast Discussions feed is high-volume;
  watch `dropped_over_volume` and tune `volume_envelope` (per-tick accepted cap)
  rather than removing the gate.
- **Slack routing of ingestion alerts is pending.** Quarantines are counted and
  logged now; wiring the Ops Slack route for ingestion alerts is a deferred
  step. Until then, watch the telemetry snapshot / V2 Sources Health.
