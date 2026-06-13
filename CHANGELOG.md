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

- **`SimRunner::inject_perp_tick`** (slice 4b, `fortuna-runner`, additive): the perp
  INGESTION seam. `EventPayload::PerpTick` has no producer in the deterministic
  `tick()` loop (which sources only `BookSnapshot`s), so the perp strategies would
  be inert in the daemon. This publishes an `EventOrigin::External` `PerpTick` onto
  the bus for the next `tick()` to dispatch through its EXISTING `new_events` read —
  so `tick()` itself is UNTOUCHED (the record/replay determinism contract and every
  existing DST recording are unaffected; the full DST corpus re-ran green to prove
  it). A Sim-soak test drives the REAL `funding_forecast` through a runner tick: it
  produces a scalar belief BECAUSE it saw an injected `PerpTick`, and nothing
  without one. The same seam carries the live kinetics feed
  (`KineticsPerpObservation` → `inject_perp_tick`).
- **`KineticsPerpObservation`** (slice 4a, `fortuna-venues::kinetics::perp_observation`,
  additive): the venue-side half of a `PerpTick`, built VERBATIM from a WS `ticker`
  frame — `MarketId` + `PerpMarks` (venue settlement; no conservative mark) +
  `FundingObservation` (rate→`Decimal` estimate, `next_funding_time`, reference
  price, capture `obs_at`). The venue crate stays BUS-FREE: this returns perp-domain
  components, and the producer (a later sub-slice) adds the `venue` id to make the
  bus event. 4 tests (synthetic exact-mapping + field-swap guards, recorded-frame
  re-derivation, malformed→`Err`). The foundation for the PerpTick producer —
  WITHOUT a producer the perp strategies are inert (slice-4 architectural finding,
  see GAPS). NEXT: the scripted PerpTick source (Sim soak) + daemon registration.
- **`perp_event_basis` STRATEGY** (slice 3b-strategy, `fortuna-runner::perp_event_basis`,
  additive): the propose-only, mechanical, Sim-stage bracket trader. On a `PerpTick`
  it rebuilds bin probabilities from `core.books` (YES mid `(bid_or_0 + ask_or_0)/2`
  — an absent quote counts as the 0c floor, so the live `0 bid / Nc ask` far tails
  keep their `ask/2` mass and the strategy reproduces the kernel's validated basis),
  calls `compute_basis`, and proposes ONE maker-only (`Urgency::Passive`) UNSIZED
  `Cents` leg (I6 — no qty; the harness sizes) on the bin containing the perp
  forecast, gated by the fee-trap (`fair = limit + premium`, clamped ≤99). It holds
  its OWN bracket catalog (`MarketId → BracketStrike`); no `fortuna_venues::Market`
  widening (live catalog-population is the slice-4 daemon concern). 14 mutation-pinned
  unit/e2e tests + a DST oracle that independently recomputes the verdict in lockstep
  with `bin_prob`. VALIDATED on live DEMO data: the committed e2e (cycle …753775,
  basis −$55.53) + a fresh independent cycle (…754035, basis +$55.08), both with
  perp/ladder agreement <0.1%.
- **`perp_event_basis` basis kernel** (slices 3 + 3b, `fortuna-cognition::basis`):
  the deterministic forecast-quality basis signal — `bracket_implied_median` (a
  **KXBTC** price-level bracket ladder's YES bid/ask → normalized probabilities →
  0.5-crossing interpolation) + `compute_basis` (perp mark − implied median,
  gated past the assumed-fee floor). Slice 3b refined the kernel to the REAL
  3-strike-type ladder grounded in the live capture: a `BracketStrike` enum
  {`Between`{floor,cap}, `Greater`{floor}, `Less`{cap}} with `BracketBin{kind,
  prob}`; a 0.5 crossing landing in an OPEN tail returns `None` (no finite width
  to interpolate — conservative, no fabricated point). The kernel now has ZERO
  money-type touch: `compute_basis` takes the perp mark as caller-supplied `f64`
  BTC-dollars (the per-contract→BTC ×10000 boundary is the caller's), so it is
  pure f64-cognition. The implied-median reduction (`sum_p`) is taken over the
  SORTED bins, so the median is a pure function of the ladder MULTISET,
  independent of caller input order (a DST-found float-determinism wrinkle: a
  non-associative input-order sum could flip the 0.5 crossing at an exact
  cum==0.5-at-a-bin-boundary tie). 14 mutation-pinned synthetic tests + a NEW
  real-data e2e (`basis_live_fixture.rs`) on the committed paired cycle — implied
  median $63,961.53 vs perp $63,906.00 → basis −$55.53 (two independent price
  sources agree <0.1%). The composite fixture lives in `fixtures/perp-basis/`
  (a recorder-DERIVED perp+ladder pair for the basis/cognition layer, NOT a
  single Kinetics DTO capture — kept OUT of `fixtures/kinetics-perps/` so the
  venue DTO-coverage tripwire `every_fixture_parses_into_its_typed_dto`, which
  requires every fixture there to classify, is not tripped; operator-directed
  location, the tripwire's "every DTO fixture accounted for" guarantee intact).
  The bracket-TRADER strategy (the sized `Cents` bracket-leg trade) stays
  fixture-gated.
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

- Daemon composition (slice 4): register `funding_forecast` + `perp_event_basis`
  into the Sim runner and populate the latter's bracket catalog from the live
  Kalshi market list (coordinate with track A — `daemon.rs`). F5–F9 (Aeolus
  weather → belief) build on the scalar foundation. Marked pending, not done.
  (Slices 1–2 + the slice-3/3b basis kernel + the perp_event_basis STRATEGY are
  DONE; the `KalshiMarket` floor/cap DTO is NOT needed — the strategy holds its
  own catalog.)

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

_Owned by Tracks A / B / C / E — see their entries below._

## Track A — venue / exec / recovery

Prior to this log (gated, on main): M3 rearm notices; T4.2 (i) Kalshi WS dial
slices 1-2 + 4-5 + concrete transport (see `docs/reviews/t42-wsdial-gate-2026-06-13.md`,
`t42-redial-gate-2026-06-13.md`, `m3-rearm-gate-2026-06-13.md`).

### 2026-06-13 — F16a: Kalshi cancel-reconcile hardened via the order list

**Changed.** `KalshiVenue::cancel` (`crates/fortuna-venues/src/kalshi/adapter.rs`).
On a DELETE-200 ack whose single reconcile GET reads stale `Resting`/`Unknown`
(the recorded F16/F3 race — DELETE acked `reduced_by:"1.00"`, GET ~360ms later
still `resting`), cancel() now reconciles ONCE against the order LIST
(`GET /portfolio/orders`, new `cancel_reconcile_status_via_list`) — the
authoritative terminal surface — and maps: list `Canceled`→`Ok(())`,
`Executed`→`Rejected` (fills via `fills_since`), still-stale/absent/list-error→
`Timeout` (the safe fallback). A genuinely-canceled order that read stale now
resolves to a definite `Ok` instead of a false `Timeout`. The first-DELETE-404 →
`NotFound` path is unchanged (no ack ⇒ claim nothing).

**Why the order list, not recancel-404.** `fixtures/kalshi/README.md` finding-16
suggested "treat recancel-404 as proof-of-canceled"; the fixtures REFUTE it — the
404 bodies for already-canceled, already-EXECUTED, and never-existed orders are
byte-identical (`orders__cancel_already_canceled` == `_executed` == `_unknown_id`),
so that heuristic would MASK A FILL. The list status is the safe discriminator
(`portfolio__orders_list` carries the same id `canceled` and other ids `executed`).
README finding-16 annotated with this correction.

**Tests** (verbatim recorded bodies; no fabrication): stale→list-canceled→Ok;
stale→list-EXECUTED→Rejected (safety headline, **mutation-proven** — flip the
Executed arm to `Ok(())` ⇒ that test reds); stale→absent→Timeout; the two existing
stale tests extended to the 3-call flow (Timeout preserved, not weakened). Full
`fortuna-venues` suite green; protected crate untouched; no new dep, no constructor
change. **Deferred (F16b, GAPS):** the full multi-attempt bounded-backoff poll —
needs an injected Sleeper + a recorded multi-stale fixture (never fabricated).

### 2026-06-13 — T4.5 slice: gate-verdict badge (/api/rota/v1/build) — `7ed3138`

**What.** New `/api/rota/v1/build` endpoint exposing the LATEST gate verdict
parsed from the verifier's `docs/reviews/*.md` — the local operator console's
build-health badge (design §7 cut it from v1 for "no parser"; T4.5 re-includes
it). New `RotaState.reviews_dir` capability (mirrors `perishable_dir`; main.rs
wires `docs/reviews`; a deployed daemon lacks `docs/` → "unknown"). `parse_verdict_token`
finds `verdict:` anywhere in a line (line-start AND mid-line `Base: … Verdict: X`
headers) and validates the ACCEPT*/BLOCK vocabulary (no prose false-positives);
`latest_gate_verdict` picks the newest-by-mtime `.md` carrying a verdict (the
rolling GATE-FINDINGS bus + verdict-less files skipped); bounded 8KB read; no-panic.

**Tests.** Parser units over every real format (+ mid-line, ACCEPT-WITH-CONDITIONS,
a prose-guard) + a deterministic populated-path scanner test (`File::set_modified`)
+ endpoint + degraded. code-reviewer ACCEPT (1 should-fix folded: the mid-line miss).

**Correction.** The iteration-14 validation note over-claimed the *discovery joins*
(a) as BUILDABLE-NOW; per design §4/§12 they are deferred (queries unwritten,
triage-recall not-in-v1) and discovery observability is track-B's — corrected in
GAPS/queue/§10. So the buildable track-A T4.5 surface is now COMPLETE: audit-recents
(gates+settlement) + this badge. Remaining: (c) WS counters, (d) money model — both
operator/verifier-blocked (GAPS).

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1391 passed, 0 failed); run-dst.sh 200 (0 violations). (One run hit a
transient sqlx-test temp-DB-name collision in the pre-existing cognition test — a
known parallel-`#[sqlx::test]` flake, not this slice; green on re-run.)

### 2026-06-13 — T4.5 slice: /settlement.recent_watchdog_events — `9558d56`

**What.** Second T4.5 build slice (design §5), mirroring the gates slice.
`view_settlement` (rota.rs) merges `recent_watchdog_events` when the R5 pool is
present: the audit `watchdog` rows (sub-kinds settlement_overdue / dispute_freeze /
orphaned_position) → `{audit_id, at, kind (the sub-kind), market_ref}`, newest-first.
New `recent_watchdog_events_page` (runtime sqlx, `payload->>'kind'` text-extract).
No verdict filter — every watchdog row is an event.

**Honest/degraded.** Daemon-shaped "settlement" base view preserved (`views_from`
untouched — the fortuna-live views test still asserts the array is absent there); no
pool → explicit `available:false`; errors neutral. The bus `settlement_overdue` event
is a separate kind; the audit table carries `watchdog`.

**Tests (populated-path).** Seed all 3 watchdog sub-kinds + a non-watchdog row; assert
only the watchdog rows surface newest-first with the full §5 shape (first/middle/last
pinned), foreign kind excluded; available-but-empty; degraded-no-pool. code-reviewer
ACCEPT (clean faithful mirror).

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1387 passed, 0 failed); run-dst.sh 200 (0 violations).

### 2026-06-13 — T4.5 slice: /gates.recent_rejections (audit-recents) — `59fa594`

**What.** First T4.5 build slice (design §5). `view_gates` (rota.rs) now merges a
`recent_rejections` sub-surface when the R5 read pool is present: recent per-check
gate REJECTIONS from the audit `gate_decision` trail, mapped to `{audit_id, at,
check, reason, intent_ref}`, newest-first. New `recent_gate_rejections_page`
extracts fields as TEXT in SQL (`payload->>'check'` etc.) — runtime sqlx, the
`audit_tail_page` precedent (off the sqlx-offline cache).

**Why.** The first of the BUILDABLE-NOW T4.5 pieces (the audit pool it was deferred
behind is live). Surfaces *why* orders were gate-rejected for the operator.

**Honest/degraded.** The daemon-shaped "gates" base view is preserved (`views_from`
untouched). No pool → explicit `available:false`, never fabricated; errors log
neutral detail, never raw sqlx. The bus `gate_reject` event is a separate kind
(live stream); the audit table carries `gate_decision`.

**Tests (populated-path, T4.5 TEST RULE).** Seed real `gate_decision` rows (2
Rejects + a Pass + the foreign `gate_reject` kind); assert only the 2 Rejects
surface newest-first with the full §5 shape, Pass+foreign excluded; available-but-
empty when no rejects; degraded-no-pool unavailable. code-reviewer ACCEPT.

**Battery.** fmt --check; clippy --workspace --all-targets -D warnings; cargo test
--workspace (1384 passed, 0 failed); run-dst.sh 200 (0 violations).

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
- **Domain Analyses board** (`GET /api/rota/v1/analyses`, mission item 1 / track-E
  §20.2 "the whole process") — the analysis-artifact ledger newest-first: which
  persona (`id@version`) analysed which `region_key`, when, at what cost (dollars
  via the `cents` flag), the `content_hash` replay anchor (8-char prefix), and the
  supersession status, with an `{analyses, open, cost_cents}` summary. A fortuna-ops
  runtime-sqlx query (audit-tail pattern). UNTRUSTED-DATA BOUNDARY: this view renders
  STRUCTURAL METADATA ONLY — the `findings` / `signal_manifest` JSONB (untrusted
  model/signal output) are not selected or exposed; the per-artifact expander (where
  the esc/JSON-encode discipline applies) is a §20.2 follow-on (GAPS).
- **Forecasts scorecard** (`GET /api/rota/v1/forecasts`, track-C §9.1 "the outcomes
  of the whole process") — the scalar-forecast calibration headline: per (producer,
  scoring rule) the mean score (CRPS, lower=better) over RESOLVED forecasts, the
  resolved count, and the unit, with a `{producers, rules, scored}` summary. A
  `scalar_beliefs ⋈ belief_scores` runtime-sqlx aggregate (audit-tail pattern).
  SCORE METADATA ONLY — the untrusted `quantiles`/`provenance` JSONB are not selected
  or exposed; the recent-forecast feed + `coverage_bps` + sparkline are §9.1 follow-
  ons (GAPS). Degrades honest-`unavailable` until track-C's daemon persist (slice 4)
  writes the tables — never a fabricated score.
- **Working Orders board** (`GET /api/rota/v1/working_orders`, mission item 3 "trades
  being executed" — the live side) — the intents currently resting at the venue
  (submitted / acked / partially-filled, not yet terminal): market, side, action,
  limit (dollars), qty, filled, status, submitted-at, with a `{working}` summary. A
  `views_from` board shaped daemon-side from `runner.manager().intents()` filtered by
  `IntentStatus::is_working()` (the same ROTA seam as Strategy P&L; a pure panic-free
  read — daemon snapshot byte-unchanged, daemon_smoke 15/15). Empty when nothing rests
  (honest). With Recent Fills + Strategy P&L, mission item 3 (trades) is substantially
  covered; unrealized PnL remains the mark-loop gap.
- **Persona Scorecard board** (`GET /api/rota/v1/persona_scores`, track-E §20.1
  outcomes half — now unblocked by the merged persona runtime) — per persona, the
  calibration of its resolved beliefs: n_resolved, mean Brier (lower=better), mean
  CLV bps (higher=better), aggregated from the `beliefs` table grouped by
  `provenance->>'persona_id'`, with an honest `evaluating (n/60)` verdict. A pure
  AVG/COUNT projection — the §11 PROMOTABLE/RETIRE verdict + the raw/market baselines
  + calibration_quality are NOT computed in ROTA (unpersisted / cognition logic;
  omitted, never faked). Completes the Personas board's two halves (registry +
  scorecard). Honest-`unavailable` until the persona runner is daemon-wired.
- **Telemetry board** (`GET /api/rota/v1/telemetry`, mission item 6 "the Prometheus
  stack on the console") — the metric series the daemon exports (the same
  `MetricsRegistry` the `/metrics` exposition is rendered from), grouped by subsystem
  (ingest/gate/exec/state/venue/killswitch/cognition/…), one row per series with its
  type + integer value. R2-clean: the daemon shapes it via the new
  `MetricsRegistry::telemetry_board` (an additive `views["telemetry"]` key, daemon
  snapshot byte-stable) and ROTA serves it via `read_view` — the handler never parses
  Prometheus text. Completes the operator's single-pane-of-glass across all six
  mission areas (cognition, pipeline, trades, discovery, DB, telemetry).
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
