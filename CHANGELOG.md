# Changelog

This is the FORTUNA project changelog. It follows [Keep a Changelog](https://keepachangelog.com/)
style. Each build track maintains its own **subsystem subsection** under
`## [Unreleased]`, so concurrent edits touch distinct sections and rarely
collide; the verifier reconciles the subsections on merge. Dates are UTC. One
concise bullet per logical change; newest-relevant first.

## [Unreleased]

### Ingestion & data sources (fortuna-sources, Track D)

The news-aggregation / weather-signal ingestion subsystem (`crates/fortuna-sources`)
and its daemon seam (`crates/fortuna-live` `ingestion.rs` / `boot.rs`). Off by
default — merged code activates zero ingestion until an operator opts in (see
`docs/runbooks/ingestion-ops.md`). No model is anywhere on the ingestion path.

#### Added

- Fail-closed `[sources.<id>]` config (`SourceConfig` / `SourceKind`): unknown
  kinds/fields, non-https URLs, and anything not runnable in Phase A are hard
  errors, never defaults (D1).
- `FetchClient` HTTP substrate: SSRF-safe host pin (`HostPin`), https-only,
  conditional GET (ETag / If-Modified-Since → 304 ⇒ empty), and a GCRA
  politeness rate-limit (D2).
- Layer-1 `StructuralValidator` (refuse future-dated / republished / over-volume
  per tick) plus the Layer-0 dossier template (D3).
- `NwsSource` adapter — NWS active alerts (`feed = "alerts"`) and Area Forecast
  Discussions (`feed = "afd"`), emitting `nws.*` signals, with dossier and real
  fixtures (D4).
- `RssSource` adapter — any RSS/Atom via feed-rs, emitting `rss.item`; Fed/SEC
  dossiers (D5).
- `CalendarSource` adapter — BLS macro release schedule (`feed = "schedule"`,
  iCalendar) and latest-numbers RSS (`feed = "latest"`) (D6).
- Layer-2 corroboration (`corroborate`) — near-duplicate clustering that
  collapses syndication so one wire story carried by many outlets is one origin;
  built as a standalone pass, not yet wired into the live ingestion tick (D8).
- `IngestionScheduler` — the validator-WIRED ingest core: per-source cadence,
  the live Layer-1 hard gate (refuse-and-quarantine on the path), per-source
  `Health` machine with operator-only `rearm`, deterministic capped exponential
  backoff, and `SourceMetrics` (D9).
- Config-driven `build_scheduler` factory plus the daemon `[ingestion]` seam
  (default-off; the trading daemon is byte-unchanged when the section is absent)
  (D10).
- **Phase A merged to main @ `f31aaa8`** (NWS + RSS + Calendar; GDELT deferred).
- Generic per-source auth header (`auth_header` / `auth_env`): `x-api-key` and
  any scheme drop in by name; the secret is env-only and redacted (F1).
- `NwsClimateSource` adapter (`feed = "climate"`) — the NWS CLI
  (Climatological Report–Daily) two-hop grader, the official daily max/min
  settlement record; emits `nws.cli` carrying the raw productText (F2).
- `AeolusSource` adapter (kind `aeolus`) — the operator-owned probabilistic
  temperature-forecast vendor; `x-api-key` auth, env-only secret; emits
  `aeolus.forecast` (the raw envelope, untouched) with real live-endpoint
  fixtures (F3).
- Climate grader wired into the factory — scheduler-validated and reachable
  through config (F4).
- OBS-1 ingestion telemetry data surface (`IngestionTelemetry`): per-source
  `SourceTelemetry`, process-wide `FunnelCounts`, and a bounded (256), newest-
  first `recent` feed of redacted `SignalRecord`s — the observability
  contract §2 snapshot.
- OBS-2a funnel loop-stages — `IngestionCore` / `IngestionWiring` now fill the
  funnel's `normalized` / `deduped` / `persisted` / `persist_failures` stages and
  expose `telemetry(now)`, so the funnel is complete end to end (those stages
  read 0 in OBS-1). The `Arc<RwLock>` publish that exposes the snapshot to ROTA
  is OBS-2b (deferred).
- OBS-3 `SourceTelemetry.domain_tags` — populated from the `source_registry`
  admission via a new `domain_of` resolver on `build_scheduler` (parallel to
  `tier_of`), so the per-source telemetry carries its domain (weather|macro|…).
  No more empty placeholder fields in the telemetry surface.
- OBS-2b telemetry publish — `run_ingestion_loop` now publishes the snapshot into
  a shared `IngestionTelemetryHandle` (`Arc<RwLock<IngestionTelemetry>>`) each
  tick ("one writer, many readers", §2); `IngestionTelemetry` derives `Default`
  for the empty pre-first-tick state. The daemon creates the handle (inert when
  ingestion is off) and logs the final funnel at shutdown. The ROTA read endpoint
  (OBS-2c) is track B's harness.
- Design docs: `docs/design/aeolus-fortuna-source-contract.md` (rev 3,
  reconciled with the Aeolus producer handoff) and
  `docs/design/ingestion-observability-contract.md` (telemetry + ROTA-views
  contract for track-B).

#### Fixed

- Unified the URL parser across the fetch path — the host pin is now built from
  the same WHATWG `url` parser (`reqwest::Url` / `url::Url::parse().host_str()`)
  the HTTP client and redirect handling use, removing the hand-rolled
  `host_of_https` (see Security).

#### Security

- **Critical SSRF "parser-differential" fixed at root cause before merge** — a
  mismatch between a hand-rolled host extractor and the HTTP client's WHATWG URL
  parser was eliminated by deleting `host_of_https` and unifying on one parser;
  cleared by 29 adversarial vectors. The injection surface (ingestion) treats all
  fetched content as untrusted data, never instructions (spec 5.11).
- Per-source auth secrets are env-only (resolved by the binary, never the lib),
  marked sensitive (`HeaderValue::set_sensitive`) so the `http` crate prints
  `Sensitive`, and elided as `<redacted>` in manual `Debug` — never in config,
  repo, logs, or audit payloads.

#### Deferred

- D7 `GdeltSource` — external IP rate-limit; interim is `rss` against GDELT's
  `format=rss`.
- OBS-2 — the loop-side funnel stages (`normalized` / `deduped` / `persisted`)
  and the `Arc<RwLock>` snapshot publish (fortuna-live); OBS-3 — `domain_tags`
  from the registry.
- F4b — release-aware cadence (consume `next_run_at` + the GEFS release pattern).
- F10 — Aeolus `source_registry` row + dossier finalization + v1→v2 fixture
  migration.
- F5–F9 — these are cognition (Track C), not Track D: F5 dedup, F6 the strict
  v2 μ/σ→p parser, F7 world-forward match, F8 belief→calibration→gates→sizing,
  F9 the Layer-3 `source_reliability` scoring that V4 of the ROTA scorecard
  depends on (until then V4 shows "insufficient data").

### Trading core, venues & exec

_Owned by Tracks A / C / E — see their entries (Track A's section is below)._

## Track A — venue / exec / recovery

Prior to this log (gated, on main): M3 rearm notices; T4.2 (i) Kalshi WS dial
slices 1-2 + 4-5 + concrete transport (see `docs/reviews/t42-wsdial-gate-2026-06-13.md`,
`t42-redial-gate-2026-06-13.md`, `m3-rearm-gate-2026-06-13.md`).

### 2026-06-13 — T4.2 (ii) book-driven recorded-stream replay into PaperVenue — `fc5bd64`

**What.** New integration test `crates/fortuna-runner/tests/recorded_replay.rs`
(7 tests; test-only, no production change). Drives the production replay seam
`KalshiWsParser -> BookAssembler -> fortuna_paper::feed_stream_event ->
PaperVenue` over the operator-recorded Kalshi WS fixtures
(`fixtures/kalshi/ws__orderbook_trade_{yes,noleg}.jsonl`) and composes both
mechanical strategies (`mech_structural`, `mech_extremes`) over the replayed book.

**Why.** Queue item 2(ii): exercise the venue/exec/paper path against the
RECORDED fixtures "as if live," not doc-derived/synthetic frames.

**Asserts.** Gapless, fully-typed parse of both fixtures (0 trade frames); the
EXACT assembled book inside PaperVenue (yes 47×3 / 52×2; noleg 47×3 / 48×1,
including a transient empty book that replays clean); book-only replay yields NO
fills (a resting maker order is untouched); both strategies consume the recorded
book and abstain correctly, with liveness controls proving each fires on a
qualifying input.

**Fixture-blocked (ledgered in GAPS, never fabricated).** (1) Trade-through
replay — no public trade frame was recorded (quiet market); paper maker fills are
trade-driven (spec 11). (2) Structural-arb replay — a single-market recording
cannot complete a bracket; needs a multi-market bracket fixture.

**Battery.** `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D
warnings`; `cargo test --workspace` (126 targets, 0 failed); `scripts/run-dst.sh
200` (4 corpus + 200 seeds, 0 invariant violations; daemon_smoke 15/15;
ingest_dst 5/5). code-reviewer pass folded in. Protected crate untouched.

**Shared docs.** No architecture/runbook change warranted (test-only; the replay
seam and strategies are unchanged production code). BUILD_PLAN T4.2 progress
noted (box stays unticked — slices iii–v remain); queue item 2(ii) marked done.
