# Changelog

This is the FORTUNA project changelog. It follows [Keep a Changelog](https://keepachangelog.com/)
style. Each build track maintains its own **subsystem subsection** under
`## [Unreleased]`, so concurrent edits touch distinct sections and rarely
collide; the verifier reconciles the subsections on merge. Dates are UTC. One
concise bullet per logical change; newest-relevant first.

## [Unreleased]

### Cognition belief-pipeline & perps (fortuna-cognition / fortuna-ledger / fortuna-core, Track C)

The `prob_claims/v1` scalar-belief foundation + perp strategies (design
`docs/design/perp-strategies-and-scalar-claims.md`). Verifier-gated ACCEPT
(slices 1a + 1b + 2a + funding kernel) and MERGED to main @2809aea, 2026-06-13.
Slice 2b (`funding_forecast` producer) gated ACCEPT (dispersion-widening
mutation-proven) and MERGED to main @f949554, 2026-06-13.

#### Added

- **`perp_event_basis` basis kernel** (slice 3, `fortuna-cognition::basis`): the
  deterministic forecast-quality basis signal — `bracket_implied_median` (a
  KXBTC15M bracket ladder's YES bid/ask → normalized probabilities →
  0.5-crossing interpolation) + `compute_basis` (perp mark − implied median,
  gated past the assumed-fee floor). f64-cognition (never money); the bracket
  structure is grounded in the committed Kalshi research, only the test values
  are synthetic. 10 mutation-proven tests. The bracket-TRADER strategy + the
  real-orderbook e2e stay fixture-gated (operator-queue #4 + a `KalshiMarket`
  floor/cap DTO extension).
- **`funding_forecast` strategy** (slice 2b, `fortuna-runner`): a zero-capital
  scalar belief-producer — on a `PerpTick` it forecasts the next funding rate
  directly from the recorded venue estimate (`finalize_funding_rate(estimate)`;
  the estimate IS the running TWAP, never re-derived) and emits a
  `PredictiveDistribution::Scalar` quantile fan whose dispersion widens with
  time-remaining-in-window (a documented rung-0 model, CRPS-measured). Proposes
  NOTHING (I6). A live-data CRPS test scores a recorded estimate → forecast
  against a recorded realized rate; exact-window calibration is deferred to the
  operator-queued paired fixture (the test pins the gap executably, never
  fabricates). DST arm over tick/gap/window-roll/clamp chaos.
- **Perp-strategy seam** (slice 2a, additive): `EventPayload::PerpTick` + the
  `FundingObservation` type (`fortuna-core`), `ScalarBeliefDraft`
  (`fortuna-cognition::scalar_beliefs`), the `drain_scalar_beliefs()` default
  Strategy-trait method + the runner's `pending_scalar_beliefs` buffer
  (`fortuna-runner`) — the plumbing the `funding_forecast` strategy (2b) rides.
  Bus events replay byte-stable (the `Decimal` rate preserves scale). The binary
  `BeliefDraft` / `drain_beliefs` path is byte-unchanged.
- **Scalar belief type + swappable scoring** (`fortuna-cognition::scoring`,
  slice 1a): `PredictiveDistribution {Binary, Categorical, Scalar{quantiles,
  unit}}` + `RealizedOutcome` + the swappable `ScoringRule` trait; `BrierRule`
  + `CrpsPinballRule` (native CRPS = mean pinball / quantile loss); `ScoreError`;
  full `validate()` (strict-(0,1) binary p, categorical sum≈1, ≥2
  strictly-increasing non-crossing quantiles). Additive — the binary
  `BeliefDraft` path is byte-unchanged. 54 tests incl. a proper-scoring proptest.
- **Scalar-belief storage** (`fortuna-ledger`, slice 1b): append-only
  `scalar_beliefs` (immutable claim + one-time resolution; `producer`
  first-class for the ROTA scorecard) and `belief_scores` (rule-tagged
  `(belief_id, rule_id)` score, FK → `scalar_beliefs`, unique per rule);
  `ScalarBeliefsRepo` (exactly-once `resolve`, mirroring `resolve_and_score`) +
  `BeliefScoresRepo`. Migration `20260613000002_scalar_beliefs.sql` with
  append-only DB triggers. 7 live-PG tests.
- Deterministic funding-forecast kernel (`fortuna-core::perp`): `FundingWindow`
  (running TWAP of recorded premiums; premium-as-input never re-derived) +
  `finalize_funding_rate` (±2 % clamp, 0.01 % zero threshold). 13 tests.

#### Deferred

- perp_event_basis STRATEGY (slice 3b — the Cents bracket-leg trade + the
  KalshiMarket floor/cap DTO; the slice-3 basis kernel above is DONE+merged, the
  trade is fixture-gated), daemon composition (slice 4), and F5–F9 (Aeolus
  weather → belief) — all build on the scalar foundation above. Marked pending,
  not done. (Slices 1–2 + the slice-3 basis kernel are DONE + merged to main.)

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

### Domain-analysis personas (fortuna-cognition, Track E)

Persona analysts (meteorologist + macro economist) that reason over UNTRUSTED
signals and emit calibration-scored beliefs. Verifier-gated ACCEPT and MERGED to
main @2668291, 2026-06-13. No model action is ever execution — personas propose.

#### Added

- Persona belief consumption (`persona_beliefs`, E.4): the μ/σ→p backbone +
  artifact→`BeliefDraft` fan-out into the GATED belief pipeline (never orders —
  I6), plus the `SectionKind::DomainAnalysis` context section.
- Persona scoring + promote/retire (`persona_scoring`, E.5): calibration Brier vs
  both baselines (raw + market) + CLV; `propose_promotion` returns a
  RECOMMENDATION-ONLY `PersonaPromotionProposal` (the daemon never self-promotes —
  the I7 analog; a human acts on the proposal). Mutation-proven gate.
- The trusted/untrusted firewall (E.3a core): the persona's method rides the Mind
  `system_charter`; untrusted signals are assembled only as `<context-item>` data,
  never as instructions.
- End-to-end meteorologist proof + macro-economist generalization (one mechanism,
  two domains) + the persona-authoring operator runbook + a seeded persona-runner
  DST arm (budget throttle, signal absence, schema-invalid findings).

### Trading core, venues & exec

_Owned by Tracks A / C / E — see their entries. Not maintained here._

### ROTA observability console (fortuna-ops, Track B)

The read-only operator single pane of glass (`crates/fortuna-ops/src/rota.rs`,
`assets/rota/`). Mission 2: total observability. Read-only doctrine absolute (zero
mutating endpoints), gold-on-black, honest nulls; every board screenshot-verified
with real rows (archived under `docs/reviews/rota-visual/`). Live status matrix:
`docs/design/rota-observability.md`.

#### Added

- Local bringup harness (`crates/fortuna-ops/examples/rota_local.rs`): seeds a
  GUARDED throwaway Postgres (`ROTA_LOCAL_DATABASE_URL` only, never the operator's
  DB) + a representative snapshot, serves the console — the reusable screenshot
  rig. The 7 original boards (health/money/gates/cognition/settlement/streams/audit)
  screenshot-verified with real rows.
- Generic `boardTable` renderer for the D-contract `{title, columns, rows, summary}`
  envelope, with a data-driven `pill` column flag — reused by every ingestion board.
- **V2 Sources Health** (`GET /api/rota/v1/ingest_sources`) — per-source health /
  polls / accepted / drop-by-reason / 304-rate / quarantines; surfaces the
  AFD-firehose.
- **V1 Live Signal Feed** (`GET /api/rota/v1/ingest_feed`) — recent signals
  newest-first with their (redacted, esc()'d) data + accept/drop status pills.
- **V3 Ingest Funnel** (`GET /api/rota/v1/ingest_funnel`) — the pipeline as a stage
  table (fetched → validated → normalized → persisted) with retention % + drop-offs.
- **Discovery — Events board** (`GET /api/rota/v1/discovery`, mission item 4 "the
  canonical events we have, the markets under them") — the events ledger with each
  event's status + DISTINCT mapped-market count (a LEFT JOIN to
  `market_event_edges`, supersession-safe). A fortuna-ops runtime-sqlx query (the
  audit-tail pattern). Benchmark snapshots + per-event drill-in are follow-ons.
- **Database board** (`GET /api/rota/v1/db`, mission item 5 "honest visibility into
  the actual tables — counts") — an exact `COUNT(*)` sweep over every one of the 24
  ledger tables (incl. the `scalar_beliefs`/`belief_scores` scalar plane), busiest-
  first, with a `{tables, total_rows}` summary. The table
  names are query literals (UNION ALL, no interpolation — zero injection surface);
  a genuinely-empty table shows a real `0`, never an omitted row. A fortuna-ops
  runtime-sqlx query (the audit-tail pattern). NOTE (GAPS): exact COUNT is accurate
  at Sim scale — swap to `pg_class.reltuples` when `audit`/`signals` grow; per-table
  drill-in (recents / columns) is a follow-on.
- **Personas board** (`GET /api/rota/v1/personas`, mission item 1 "how beliefs are
  formed — the roster of analysts"; track-E §20.1 registry half) — every
  (persona_id, version) grouped by persona, newest version first, with domain, tier,
  lifecycle status (a `pill`: active→green, retired→dim), the method-file integrity
  hash (8-char prefix), the signal kinds it reads (`reads_signal_kinds` flattened),
  and effective date, plus a `{personas, versions, active}` summary. A fortuna-ops
  runtime-sqlx query (the audit-tail pattern); all columns are operator-authored
  config (not untrusted data). The §20.1 SCORECARD half (per-persona Brier/CLV/
  verdict) is data-blocked on track-E persona scoring — ROTA surfaces it when the
  data lands, never a fabricated score (GAPS).
- **Strategy P&L board** (`GET /api/rota/v1/strategies`, mission item 3 "realized
  PnL per strategy") — per-strategy realized PnL / fees / fills / open exposure,
  shaped daemon-side from `runner.digest_snapshot()` (the same attribution the
  daily digest uses, no runner change) in the `views_from` ROTA seam, served via
  `boardTable` with money columns as dollars. A losing strategy renders honestly
  (negative). Unrealized PnL stays the mark-loop gap; working orders
  (`runner.manager().intents()`) is the remaining trades follow-on.
- **Recent Fills board** (`GET /api/rota/v1/fills`, mission item 3 "trades being
  executed") — the executed trades from the durable `fills` ledger, newest-first
  (time/market/side/action/qty/price/fee/maker-taker). A runtime-sqlx query (the
  audit-tail pattern, no fortuna-live touch) + a new data-driven `cents` column
  flag on `boardTable` so money columns render as dollars. A fill carries no
  strategy/PnL (ledgered): per-strategy P&L (a views_from board) + working orders
  + the honest unrealized-PnL gap (no mark loop) are follow-ons.
- **OBS-2c — V1/V2/V3 now render LIVE daemon data.** `merge_ingest_views`
  (fortuna-live `views.rs`) shapes the daemon-published `IngestionTelemetryHandle`
  (track-D OBS-2b) into the three board envelopes each ROTA segment, merged at the
  snapshot-composition site (`main.rs`, non-blocking `try_read`). Honest gate: an
  unticked / ingestion-off telemetry merges nothing, so the boards stay degraded and
  the daemon snapshot is byte-unchanged (daemon_smoke 15/15). Unit-tested to produce
  the exact screenshot-verified envelopes; ROTA stays a pure snapshot reader
  (fortuna-ops gains no fortuna-sources dependency).
- Cognition board **belief lifecycle** — status distribution (open/resolved/
  superseded/abandoned) + the resolved beliefs' calibration outcome (mean Brier/CLV)
  via a real `GROUP BY`/`AVG` (runtime sqlx).
- Loop-file rule 6 — the operator doc-discipline directive (own docs + targeted
  shared-doc edits + this changelog; no staleness), part of DoD.

#### Deferred / blocked (ledgered in GAPS)

- **D V6** full belief→strategy→PnL — schema-blocked (no belief→trade link); ROTA
  surfaces the calibration edge proxy (CLV), never a fabricated dollar PnL.
- **C** `/forecasts`,`/perps` and **E** `/personas`,`/analyses`,`/persona_pipeline`
  — built as their tables/data land.
