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
- **Layer 3 — empirical scoring.** BUILT + LIVE. Cognition-side F9
  (`aeolus_reliability::score_reliability`) scores each Aeolus belief (Brier +
  CRPS) against the independent realized NWS temperature that Track-D's grader
  (`nws_cli_realized`, see Grading below) supplies; the daemon's daily resolver
  (`resolve_and_score_weather_beliefs`) fires it once per UTC day. The loop is
  closed — the ROTA V4 scorecard reads the per-`(model, scope)` reliability.

## Adapter table

| SourceKind | feed | signal kind | claimed-time fn | status / notes |
|---|---|---|---|---|
| `Nws` | `alerts` | NWS active alerts | `nws_claimed_time` | built (D4) |
| `Nws` | `afd` | Area Forecast Discussions | `nws_claimed_time` | built (D4); firehose — watch `dropped_over_volume` |
| `Nws` | `climate` | `nws.cli` (raw productText) | `nws_climate_claimed_time` | built (F2); the settlement record, two-hop; the grader `nws_cli_realized` extracts the realized high/low (see Grading) |
| `Rss` | — | `rss.item` | `rss_claimed_time` | built (D5); also the GDELT interim (`format=rss`) |
| `Calendar` | `schedule` | scheduled release (iCal) | `calendar_claimed_time` | built (D6); BLS macro |
| `Calendar` | `latest` | printed release (RSS) | `calendar_claimed_time` | built (D6); BLS macro |
| `Aeolus` | — | `aeolus.forecast` (raw envelope) | `aeolus_claimed_time` | built (F3); `x-api-key`, env-only secret |
| `Gdelt` | — | — | — | DEFERRED (D7); refused with the `rss` fallback hint |
| `Scrape` / `Mcp` | — | — | — | not buildable in Phase A (later phases) |

Signal `kind` constants are exported from the crate root (e.g.
`NWS_CLI_KIND`, `RSS_ITEM_KIND`, `AEOLUS_FORECAST_KIND`,
`RELEASE_SCHEDULED_KIND` / `RELEASE_PRINTED_KIND`).

## Grading — the resolution half (F2 grader; closes Layer 3)

`nws_cli_realized(product_text, station) -> Option<RealizedExtreme>` (in
`nws_climate.rs`) extracts the OFFICIAL daily high/low °F from an `nws.cli`
product — the independent resolution value an Aeolus weather belief is scored
against. FAIL-LOUD: `None` on any ambiguity (a jammed column `7676`, a missing
`MM`, an absent line, an inverted high<low, an unparseable date) — never a
fabricated temperature (spec 5.12). `RealizedExtreme` exposes `high_f`/`low_f`
(i64 °F); the bridge picks `high_f` for TMAX, `low_f` for TMIN. This is the
SOURCE-side half of the closed weather loop:

```
Aeolus forecast → match (F7) → belief (F8) → trade
                        nws.cli → nws_cli_realized (F2) → F9 score_reliability
                                  (None ⇒ belief stays OPEN)   → resolve_and_score
```

The F9 scorer + the resolution bridge are cognition-side (Track E); the daily
`drive()` trigger is Track A. The grader is the one piece Track D owns here.

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
(`Arc<RwLock<IngestionTelemetry>>`, OBS-2b — "one writer, many readers"), and the
reader is merged into `RotaState` (OBS-2c, DONE) so the V1/V2/V3 ROTA boards
project the live snapshot. The observability chain is COMPLETE end-to-end.
Contract: [ingestion-observability-contract.md](ingestion-observability-contract.md).

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
- **F4b** — release-aware cadence (consume `next_run_at` + the GEFS release
  pattern instead of static event windows).
- **F10** — Aeolus `source_registry` row + dossier finalization + v1→v2 fixture
  migration.
- **Slack routing of ingestion alerts** — quarantines are counted/logged now; a
  router can be passed into `IngestionWiring` later.

DONE since this map was first written (no longer deferred): the **F2 grader**
(`nws_cli_realized`, Track D); the **F5–F9** Aeolus belief pipeline + the
resolution **bridge** (Track E); the daily `drive()` **trigger** (Track A).
The weather scientific-method loop is closed and live — it lights up the moment
CLI ingestion is enabled (`[sources.nws_climate]` + `[ingestion] enabled`, on the
seeded `source_registry` row).

## Pointers

- Aeolus wire contract: [aeolus-fortuna-source-contract.md](aeolus-fortuna-source-contract.md)
  (rev 3, reconciled with the producer handoff).
- Telemetry + ROTA views contract: [ingestion-observability-contract.md](ingestion-observability-contract.md).
- Source dossiers (Layer 0): `docs/research/sources/` (per-source `dossier.md`;
  `TEMPLATE.md` is the template).
- Operator runbook: [../runbooks/ingestion-ops.md](../runbooks/ingestion-ops.md).
