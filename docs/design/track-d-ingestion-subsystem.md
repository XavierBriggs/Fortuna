# Track-D ingestion subsystem — the map

The first thing a Track-D session reads. A living index of the
news-aggregation / weather-signal ingestion subsystem: where the code is, how
data flows, what each trust layer's status is, and what is deferred.

## Overview

Track D builds the ingestion edge: dumb, deterministic adapters that fetch from
external sources (NWS, RSS, the BLS calendar, the Aeolus forecast vendor),
validate every item structurally and emit signal
envelopes to the append-only signals store — which is the seam to everything
downstream. It is an IO-edge crate like `fortuna-venues`: everything here
fetches and emits; nothing here decides. **No model is anywhere on the ingestion
path** (enforced at config validation, spec 5.11). The subsystem is opt-in and
default-off in the daemon; see [../runbooks/ingestion-ops.md](../runbooks/ingestion-ops.md)
to enable it.

## Crate & modules (`crates/fortuna-sources`)

| module | role |
|---|---|
| `config` | `SourceKind` / `SourceConfig` / `SourcesConfig`: fail-closed parse of the `[sources.<id>]` TOML tables. |
| `fetch` | `FetchClient<ReqwestFetchTransport>` HTTP substrate: `HostPin` SSRF pin, https-only, conditional GET (304⇒empty), GCRA politeness limiter, `with_auth_header` (sensitive/redacted). |
| `validate` | Layer-1 `StructuralValidator` — `Verdict::{Accept, RejectFuture, RejectRepublished, RejectOverVolume}` per tick. |
| `corroborate` | Layer-2 near-duplicate clustering (`corroborate`) — collapses syndication to one origin. |
| `nws` | `NwsSource` — active alerts + Area Forecast Discussions. |
| `nws_climate` | `NwsClimateSource` — the NWS CLI daily-extreme two-hop grader (the settlement record). |
| `rss` | `RssSource` — any RSS/Atom via feed-rs. |
| `calendar` | `CalendarSource` — BLS iCal schedule + latest-numbers RSS. |
| `aeolus` | `AeolusSource` — the operator-owned probabilistic temperature-forecast vendor. |
| `scheduler` | `IngestionScheduler` — drives the adapters, runs the Layer-1 hard gate, tracks health + telemetry. |
| `factory` | `build_scheduler` — maps `(SourceKind, feed)` → adapter; fail-closed composition. |
| `error` | `SourcesError`. |

The daemon seam lives in `crates/fortuna-live`: `ingestion.rs` (`IngestionCore`,
`IngestionWiring`, `build_ingestion_wiring`, `run_ingestion_loop`) and `boot.rs`
(the `[ingestion]` config section).

## End-to-end data flow

```
fortuna.toml [sources.<id>]  ──parse──▶  SourcesConfig
                                            │
source_registry (trust tiers) ─────────────┤
                                            ▼
                              factory::build_scheduler   (fail-closed:
                                            │             tier required,
                                            │             auth resolved or refuse)
                                            ▼
                          scheduler.register(id, source, schedule,
                                             claimed_time, validator_config)
                                            │
                                  ┌──── tick(now) ────┐         (deterministic core;
                                  ▼                   ▼          injected Clock, no sleep)
                         Layer-1 validate        per-source health
                         (Accept / Reject*)      (Healthy/Degraded/Quarantined)
                                  │
                    accepted ─────┘
                                  ▼
        IngestionCore: normalize_and_dedup  (registry re-check + authoritative
                                  │           UNIQUE(source, content_hash) dedup)
                                  ▼
        IngestionWiring.tick_and_persist  ──▶  SignalsRepo (append-only)
                                  │                  └─ persist failure: COUNTED, non-fatal
                                  └──▶ Ops alert per quarantine (Slack routing deferred)
```

The async run-loop is `run_ingestion_loop`: it reads the injected `Clock`,
sleeps on real time at the IO edge, and exits on a stop signal. It is its own
loop, independent of the trading `drive()` cycle — off the money path.

## Four-layer trust framework (design §4.4) — status

- **Layer 0 — admission / dossiers.** Research-grounded dossiers under
  `docs/research/sources/<id>/dossier.md` assign a trust tier. BUILT for `nws`,
  `rss_fed_press`, `rss_sec_edgar`, `calendar_bls`, `nws_climate` (tier 10),
  `aeolus` (tier 7). Tiers are seeded into `source_registry`; the factory
  refuses an enabled source with no tier.
- **Layer 1 — structural validation.** BUILT + WIRED in the scheduler — the D9
  hard gate; future-dated / republished / over-volume items are
  refused-and-recorded on the live path (refusal is mutation-proven).
- **Layer 2 — corroboration.** BUILT as a standalone pass (`corroborate()` in
  `corroborate.rs`, near-duplicate clustering) but NOT YET WIRED into the live
  `IngestionCore` tick — the live path dedups via `normalize_and_dedup`'s
  authoritative `UNIQUE(source, content_hash)` index. Wiring corroboration into
  the tick is a follow-up.
- **Layer 3 — empirical scoring.** NOT built here — it is cognition-side (the
  `source_reliability` table + a weekly trust-attribution job, F9). The ROTA V4
  scorecard shows "insufficient data" until then.

## Adapter table

| SourceKind | feed | signal kind | claimed-time fn | status / notes |
|---|---|---|---|---|
| `Nws` | `alerts` | NWS active alerts | `nws_claimed_time` | built (D4) |
| `Nws` | `afd` | Area Forecast Discussions | `nws_claimed_time` | built (D4); firehose — watch `dropped_over_volume` |
| `Nws` | `climate` | `nws.cli` (raw productText) | `nws_climate_claimed_time` | built (F2); the daily-extreme settlement record, two-hop |
| `Rss` | — | `rss.item` | `rss_claimed_time` | built (D5); also the GDELT interim (`format=rss`) |
| `Calendar` | `schedule` | scheduled release (iCal) | `calendar_claimed_time` | built (D6); BLS macro |
| `Calendar` | `latest` | printed release (RSS) | `calendar_claimed_time` | built (D6); BLS macro |
| `Aeolus` | — | `aeolus.forecast` (raw envelope) | `aeolus_claimed_time` | built (F3); `x-api-key`, env-only secret |
| `Gdelt` | — | — | — | DEFERRED (D7); refused with the `rss` fallback hint |
| `Scrape` / `Mcp` | — | — | — | not buildable in Phase A (later phases) |

Signal `kind` constants are exported from the crate root (e.g.
`NWS_CLI_KIND`, `RSS_ITEM_KIND`, `AEOLUS_FORECAST_KIND`,
`RELEASE_SCHEDULED_KIND` / `RELEASE_PRINTED_KIND`).

## Telemetry surface (OBS-1)

`IngestionScheduler::telemetry(generated_at) -> IngestionTelemetry` is the
observability data surface: per-source `SourceTelemetry` (health + the
`SourceMetrics` counters + timestamps + redacted `last_error`), process-wide
`FunnelCounts`, and a bounded-256 newest-first `recent` feed of redacted
`SignalRecord`s. A pure projection — `generated_at` is the injected Clock, never
wall-time; summaries/errors are redacted and truncated. The loop-side funnel
stages are filled by `IngestionWiring::telemetry` (OBS-2a: `normalized` /
`deduped` from the core, `persisted` / `persist_failures` from the wiring), and
`domain_tags` is populated from the registry admission (OBS-3). The loop
PUBLISHES each tick's snapshot into a shared `IngestionTelemetryHandle`
(`Arc<RwLock<IngestionTelemetry>>`, OBS-2b — "one writer, many readers"). The
remaining step is OBS-2c — track B wiring a reader clone into `RotaState` for the
V1/V2/V3 boards. Contract:
[ingestion-observability-contract.md](ingestion-observability-contract.md).

## Safety notes

- **SSRF.** `HostPin` pins every request (incl. redirects) to the configured
  host, built from the same WHATWG URL parser the HTTP client uses — the
  parser-differential SSRF was fixed at root cause (the hand-rolled
  `host_of_https` was deleted; unified on `reqwest::Url` / `url::Url::parse()`).
  https-only at config parse.
- **Secrets.** Auth secrets are env-only: the library never reads env; the
  binary resolves an `auth_env` NAME via `secret_resolver`. Values are marked
  sensitive (`HeaderValue::set_sensitive`) and elided as `<redacted>` in `Debug`.
  A named env that does not resolve is a hard error (no silent unauthenticated
  fetch); half-configured auth (header XOR env) is refused.
- **Untrusted data.** Everything fetched is data, never instructions (spec
  5.11). The telemetry feed quotes summaries; it never interprets them.

## Deferred / next

- **D7** `GdeltSource` — external IP rate-limit; interim = `rss` against
  `format=rss`.
- **OBS-2c** — track B wires a reader clone of the published
  `IngestionTelemetryHandle` into `RotaState` (fortuna-ops) for the V1/V2/V3
  boards; main.rs passes `ingest_telemetry.clone()` into the dashboard state.
  (OBS-2a funnel loop-stages, OBS-2b the publish, and OBS-3 registry
  `domain_tags` are DONE.)
- **F4b** — release-aware cadence (consume `next_run_at` + the GEFS release
  pattern instead of static event windows).
- **F10** — Aeolus `source_registry` row + dossier finalization + v1→v2 fixture
  migration.
- **F5–F9 (cognition, Track C — not Track D):** F5 dedup, F6 the strict v2
  μ/σ→p parser, F7 world-forward match, F8 belief→calibration→gates→sizing,
  F9 the Layer-3 `source_reliability` scoring.
- **Slack routing of ingestion alerts** — quarantines are counted/logged now; a
  router can be passed into `IngestionWiring` later.

## Pointers

- Aeolus wire contract: [aeolus-fortuna-source-contract.md](aeolus-fortuna-source-contract.md)
  (rev 3, reconciled with the producer handoff).
- Telemetry + ROTA views contract: [ingestion-observability-contract.md](ingestion-observability-contract.md).
- Source dossiers (Layer 0): `docs/research/sources/` (per-source `dossier.md`;
  `TEMPLATE.md` is the template).
- Operator runbook: [../runbooks/ingestion-ops.md](../runbooks/ingestion-ops.md).
