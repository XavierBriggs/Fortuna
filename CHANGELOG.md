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

_Owned by Tracks A / C / E — see their entries (Track A's section is below)._

## Track A — venue / exec / recovery

Prior to this log (gated, on main): M3 rearm notices; T4.2 (i) Kalshi WS dial
slices 1-2 + 4-5 + concrete transport (see `docs/reviews/t42-wsdial-gate-2026-06-13.md`,
`t42-redial-gate-2026-06-13.md`, `m3-rearm-gate-2026-06-13.md`).

### 2026-06-13 — T4.5 ROTA deferred panels: validation + slice plan (no code)

**What.** Validation-only iteration for T4.5 (deferred ROTA trading-side panels): a
code-explorer map of rota.rs/views.rs/ledger + the design §5 contracts, recorded as
fit-validation notes in `docs/design/rota-dashboard.md` §10 ("T4.5 validation").

**Findings.** Three pieces are BUILDABLE-NOW (the R5 audit pool they were deferred
behind is live): (e) audit-recents — `/gates.recent_rejections` is clean (`gate_reject`
audit kind, payload `{intent,check,reason}`), `/settlement.recent_watchdog` has a
two-path sink nuance to resolve first; (a) discovery joins (tradability/edges +
shadow-triage); (b) gate-verdict badge (low value). Two are BLOCKED and ledgered as
operator/verifier asks in GAPS: (c) WS gap/resync counters need the operator-run live
dial wired into `drive()`; (d) the full §5 money model needs an operator/design call to
surface the mark-loop `AccountView` via a SimRunner accessor. Ownership confirmed: these
are track-A trading-side surfaces (the cognition panel + §9 presentation are track-B).

**Next.** Build order: (e) /gates.recent_rejections → (e) settlement → (a) joins → (b)
badge, each with a populated-path `#[sqlx::test]` (the T4.5 TEST RULE).

**Battery.** Docs-only (no `.rs` touched) — the code battery is unchanged from the green
`fbbf861` state this session; `cargo fmt --check` clean. No code, no new tests.

### 2026-06-13 — fix: scope kinetics-DTO suite past track-C's basis fixture (main was red)

**What.** `kinetics_dto.rs`'s `every_fixture_parses_into_its_typed_dto` exhaustively
globs `fixtures/kinetics-perps/`; track-C's slice-3b commit (`2c17295`) added the
cross-venue basis composite `paired_cycle_btc_perp_vs_kxbtc.json` there (perp +
co-recorded KXBTC bracket, for `perp_event_basis`) — not a kinetics endpoint DTO, so
the exhaustive test failed `UNCLASSIFIED`. Added a documented `NON_KINETICS_FIXTURES`
exclusion (skip that one stem before the counter).

**Why.** This failed on **main** (pre-existing, confirmed against the main worktree —
the verifier's disk-deferred merge battery missed it), so `cargo test --workspace` was
red for every track. Correct scoping, not a weakening: every real kinetics fixture is
still classified + parsed + counted, `seen == table.len()` still exhaustive
(code-reviewer confirmed). GAPS-ledgered; the cleaner fix (relocate the basis fixture
out of the kinetics dir) is a track-C/verifier follow-up.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (0 failed); run-dst.sh 200 (0 violations). code-reviewer ACCEPT.

### 2026-06-13 — T4.2 (iii) Cluster 2 tail: recorded 409→AlreadyExists — `1e96d20`

**What.** One round-trip test in `kalshi_recorded_roundtrip.rs`:
`recorded_place_duplicate_client_order_id_resolves_to_already_exists`. `place()`
over the operator-recorded duplicate-409 fixture (nested
`{"error":{"code":"order_already_exists",...}}`) → resolve-by-coid GET →
`VenueError::AlreadyExists{existing}`.

**Why.** Closes clearance item 7. The 409→AlreadyExists routing was covered
synthetically (`kalshi_adapter.rs`) with a PLACEHOLDER code; this drives the real
nested wire body that placeholder awaited — idempotent place, never a false success.

**No vacuous re-tests.** Items 5 (unauth GET /markets) + 12 (legacy
`/portfolio/orders` write family) are closed by CITED existing coverage, not new
tests: `markets()` round-trips ×5 in `kalshi_adapter.rs` (the unauth distinction is
a venue property, not mock-exercisable); the adapter writes via
`/portfolio/events/orders` exclusively (item 16) and the legacy body is DTO-identical
to v2. Clearance tally now PASSes 5, 7, 12; the 2(iii) checklist is done bar the
operator-run live WS handshake.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1325 passed, 0 failed); run-dst.sh 200 (0 invariant violations).
code-reviewer ACCEPT (sound, no issues). Protected crate untouched.

### 2026-06-13 — T4.2 (iv) kill-switch LIVE `freeze --venue kalshi` wiring — `7f69b81`

**What.** `crates/fortuna-killswitch` `main.rs` gains the live Kalshi freeze path
(replacing the stub): read the switch's own env creds → `load_kalshi_creds` (new in
`lib.rs`, pure, fail-closed) → `KalshiSigner` → `ReqwestKalshiTransport` →
`KalshiVenue` → `freeze_cancel_and_report_positions` on a self-spun current-thread
tokio runtime, with `RealClock`. New `tests/kalshi_live_wiring.rs` (9 tests).

**Why.** The machinery (`4e3a484`) was proven over a real `KalshiVenue` via a mock
transport; this is the binary actually wiring the production transport so the
operator can run a real demo freeze (the 27-item clearance is now signed on main).

**I4 (held, proven executably).** `i4_killswitch_independence` stays GREEN: `tokio`
is NOT in the structural forbidden set and is already transitive via
`fortuna-venues` (the direct dep adds zero packages); a self-spun one-shot reactor
for the HTTP cancels is not the daemon event loop; the sim `self-test` path is
byte-unchanged (operational layer) and the behavioral layer passes. "tokio for IO at
the edges."

**Fail-closed + secret-safe.** All three `FORTUNA_KILLSWITCH_KALSHI_*` env vars are
required (base URL never defaulted — prod vs demo must be explicit); a missing/blank
value or unreadable/empty PEM refuses before any venue call (exit 4). `KalshiCreds`
has a hand-written redacting `Debug` (mutation-tested); errors name only the env var
/ path, never key material.

**Operator dep (GAPS).** New env var `FORTUNA_KILLSWITCH_KALSHI_BASE_URL` (added to
`.env.example`); requested operator.md addition via GAPS. The live exercise itself is
operator-run.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (143 bins, 1324 passed, 0 failed, `i4_killswitch_independence` ok);
run-dst.sh 200 (4 corpus + 200 seeds, 0 invariant violations). code-reviewer ACCEPT
(1 must-fix [dead `RealClock.now()`] + 1 should-fix [exit-code assert] folded).
Protected crate untouched.

### 2026-06-13 — T4.2 (v) A2: Slack Socket Mode envelope loop — `f52ee66`

**What.** `crates/fortuna-ops/src/socket.rs` gains the ack-first listener LOOP
over a mockable `SlackSocketTransport`/`SlackSocketConn` (mirrors the Kalshi WS
dial seam `kalshi::dial`). `run_socket_loop`: connect → pump (ack → dedup →
dispatch) → redial. New `tests/socket_loop.rs` (12 tests) + 5 inline units.

**Why.** A1 was the pure decision logic; the loop is what actually receives,
acks, dedups, and survives reconnects against a recorded/mock transport — the
production-shaped listener minus the live socket (slice B).

**Safety teeth.** ack-FIRST before any sink touch (the 3s deadline; proven by a
shared ack-vs-sink ordering log); bounded envelope-id dedup ring — a
durably-handled envelope is suppressed but a `SinkError`-failed halt is left
UNrecorded so a Slack redelivery RE-ATTEMPTS it (code-reviewer should-fix folded
+ regression-tested); `SocketDial` capped-exponential reconnect surviving
transport loss AND the `disconnect`/refresh_requested lifecycle WITHOUT
escalating on planned refreshes; cancel watch (prompt mid-pump + mid-backoff).
I2 preserved end-to-end (a re-arm on the socket is acked but REFUSED). Untrusted
data: malformed frames skipped, no panic, no ack.

**Notes.** `SlackEnvelope.envelope_id` is now `#[serde(default)]` (hello/disconnect
protocol frames carry none). Two faithful Slack-vs-Kalshi differences ledgered for
B: no client subscribe step; no app-level keep-alive (B's real tokio-tungstenite
transport must set a WS ping/pong timeout so a half-open socket surfaces as a recv
error). ZERO new fortuna-ops dep.

**Remaining (GAPS).** B (operator-gated) = daemon wiring (HaltRequestSink → gate
halt path; EphemeralSender → SlackRouter) + real WSS transport + `[slack.socket_mode]`
config + `FORTUNA_SLACK_APP_TOKEN` + operator-run live.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (134 bins, 1209 passed, 0 failed); run-dst.sh 200 (4 corpus + 200
seeds, 0 invariant violations; ingest_dst 5/5; daemon_smoke 15/15). code-reviewer
ACCEPT (1 should-fix folded). Protected crate untouched.

### 2026-06-13 — T4.2 (v) A1: Slack Socket listener decision logic — `ca5082d`

**What.** New `crates/fortuna-ops/src/socket.rs` (+14 tests) — the Slack inbound
interactivity DECISION LOGIC (built to docs/research/ops/slack-api-2026-06-09).
`dispatch_envelope` routes block_actions / slash to handlers.

**Safety teeth.** I2 re-arm REFUSED (no halt path; `HaltRequestSink` exposes only
`request_halt` — code-reviewer confirmed airtight); allow-list (fail-closed empty;
absent user = no) + optional team restriction (WrongTeam); halt-only routing to
an injected sink (NOT the I4 killswitch); untrusted-data (action_id ENUM-matched,
reason bounded 500c opaque, panic-free indexing).

**Dep-clean.** Injected `HaltRequestSink`/`EphemeralSender` traits → ZERO new
fortuna-ops dep, no fortuna-runner/gates import.

**Remaining (GAPS).** A2 = the ack-first envelope loop + WS transport mock
(dedup/reconnect); B = daemon wiring + real WSS (tokio-tungstenite) + config +
`FORTUNA_SLACK_APP_TOKEN` + operator-run live.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (133
targets, 0 failed); run-dst.sh 200 (0 violations; daemon_smoke 15/15).
code-reviewer ACCEPT (2 must-fixes folded). Protected crate untouched.

### 2026-06-13 — T4.2 (iv) kill-switch Kalshi freeze machinery — `4e3a484`

**What.** `crates/fortuna-killswitch/tests/kalshi_freeze.rs` (1 test; test-only) —
proves the I4 freeze-and-cancel works over the REAL `KalshiVenue` adapter via a
mock transport (no live socket): open_orders → cancel each (DELETE + reconcile
GET → canceled) → KillReport(2 cancelled, 0 failed); 5 transport calls; the
flat-file journal records the freeze.

**I4.** Mock + `block_on` (no tokio runtime); `fortuna-venues` already a killswitch
dep → ZERO new crate → `i4_killswitch_independence` invariant test verified GREEN.

**Remaining (next slice, ledgered GAPS).** The live `freeze --venue kalshi` wiring
(FORTUNA_KILLSWITCH_* creds + ReqwestKalshiTransport on a current-thread tokio
runtime — I4 analysis flagged for verifier); live exercise operator-run after
clearance.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (132
targets, 0 failed, incl. i4_killswitch_independence); run-dst.sh 200 (0 violations;
daemon_smoke 15/15). code-reviewer ACCEPT. Protected crate untouched.

### 2026-06-13 — T4.2 (iii) Cluster 2/3: Kalshi auth-401 routing — `fe86cb5`

**What.** +1 parametric test in `kalshi_recorded_roundtrip.rs`: each recorded 401
auth-gateway body (bad-sig / unknown-key / missing-header / skew) → `balance()` →
`VenueError::Rejected` with the venue code surfaced; two needles use the `code=`
prefix so the auth path also proves G1 structured extraction discriminately.

**Verdicts.** Clearance item 3 → PASS; item 2 adapter-mapping half (skew 401 →
`header_timestamp_expired` → Rejected). code-reviewer ACCEPT. Battery green (131
targets, 0 failed; run-dst.sh 200 0-violations; daemon_smoke 15/15).

### 2026-06-13 — T4.2 (iii) Cluster 2: Kalshi exec round-trips — `811e383`

**What.** `crates/fortuna-venues/tests/kalshi_recorded_roundtrip.rs` (4 tests;
test-only) — transport round-trips driving place/cancel/fills through a scripted
`MockKalshiTransport` over the operator-recorded response bodies.

**Asserts.** place()→recorded 201→VenueOrderId; place()→recorded nested 400→
Rejected with the venue code structure-carried (G1 e2e); the cancel STALE-READ
RACE (F16)→Timeout, never a false success off the lagged reconcile GET;
fills_since round-trips the recorded fills (taker yes/52c/fee 2c, coid resolved
via GET order).

**Verdicts.** Clearance items 6, 8-routing, 15, 19-roundtrip → PASS. REMAINING C2:
409-dup-resolve routing, unauth GET, legacy order family; then Cluster 3.

**Ledgered.** Cancel-hardening follow-up (poll-until-terminal + recancel-404-as-
canceled) — safe today (Timeout → caller reconciles); see GAPS.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (131
targets, 0 failed); run-dst.sh 200 (0 violations; daemon_smoke 15/15).
code-reviewer ACCEPT. Protected crate untouched.

### 2026-06-13 — G1 fix: Kalshi error_reason nested-object extraction — `b2087fc`

**What.** `crates/fortuna-venues/src/kalshi/dto.rs` — `error_reason` now
structure-extracts the nested `{"error":{"code","message","details"}}` body
(`KalshiErrorBody.error: Option<serde_json::Value>`), the commonest recorded 4xx
shape (17/19). The 429 string shape and the flat shape are unchanged.

**Why.** Closes gap **G1** that the 2(iii) Cluster-1 clearance exposed — the
venue's error code now reaches diagnostics structured (`code=order_already_exists;
...`) instead of a raw-JSON dump. Diagnostic quality; HTTP-status routing was
already correct. Zero blast radius (dto.rs-internal).

**Tests.** TDD red-first: new `error_reason_extracts_the_nested_error_object`
(kalshi_dto.rs); `recorded_nested_4xx_...` tightened to require the `code=` prefix.
The 3 pre-existing error_reason tests unchanged + green.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (130
targets, 0 failed); run-dst.sh 200 (0 violations; daemon_smoke 15/15).
code-reviewer ACCEPT. Protected crate untouched.

### 2026-06-13 — T4.2 (iii) Cluster 1: Kalshi paper-clearance — `f7206a4`

**What.** `crates/fortuna-venues/tests/kalshi_recorded.rs` (18 tests; test-only) —
the FIRST tests to load the operator-recorded `fixtures/kalshi/` bodies (every
prior adapter test used doc-derived samples), asserting the adapter parses the
real wire per the README findings. Plus the 27-item clearance record
`docs/design/track-a-kalshi-paper-clearance.md` (operator-signed gate; UNSIGNED).

**Why.** Queue 2(iii): an executable, operator-signable clearance that the adapter
handles the wire the venue ACTUALLY sent — `venue=kalshi` stays boot-refused until
signed.

**Verdicts.** Cluster 1 PASS: 1,7,8,9,10,13,14,16,17,18,20,21. PENDING: Cluster 2
(transport round-trips), Cluster 3 (auth-skew, WS live handshake). UNCOVERABLE
(re-capture): demo/prod parity, STP maker mode, cursor stability/expired,
settlement units, populated series fee fields, maintenance-window status.

**Adapter gaps EXPOSED (ledgered GAPS, not fixed here).** G1 nested error body not
structure-extracted (diagnostic quality; routing correct). G2 no exchange-status
DTO/method (halt rails). Both resolve before promotion.

**Battery.** fmt; clippy --workspace --all-targets; cargo test --workspace (127
targets, 0 failed); run-dst.sh 200 (4 corpus + 200 seeds, 0 violations;
daemon_smoke 15/15). code-reviewer pass folded in (C1 doc path; C2 legacy-family
label). Protected crate untouched.

### 2026-06-13 — T4.2 (ii) book-driven recorded-stream replay into PaperVenue — `e6dd7ec`

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
