# Track E — Aeolus weather→belief pipeline (F5–F9): changelog

Track-owned changelog (newest first) for the Aeolus deterministic weather pipeline,
reassigned C → E (operator-directed 2026-06-14). Separate from
`track-e-changelog.md` (the persona work) to keep the two features' histories — and
their merges — independent. Every entry = one gate-clean slice with its commit, what
landed, and how it was verified. Authoritative contract:
`docs/design/aeolus-fortuna-source-contract.md` (rev 3, wire schema `aeolus.forecast/v2`).

Branch `track-e-aeolus` off current `main` (838d7ed). New DISJOINT `aeolus_*.rs`
modules in `fortuna-cognition`; reuses the pinned `persona_beliefs::{normal_cdf,
prob_at_least}`, the binary `BeliefDraft`, the scalar `scoring`/`scalar_beliefs`
foundation, and the NWS grader; does NOT touch C's perp files or fortuna-runner. The
composition entry point is handed to Track A (daemon wiring). Convention: tests-first,
FULL workspace battery as the commit gate.

---

## Weather scoring bridge — resolve+score Aeolus beliefs vs the NWS grade (CLOSE THE LOOP) (this commit)

The F9 reliability LOOP, closed: every open Aeolus weather belief now resolves + scores against the
INDEPENDENT realized NWS temperature once its window closes. The standalone resolver
`resolve_and_score_weather_beliefs(pool, now, score_id_base)` in `crates/fortuna-live/src/daemon.rs`
mirrors `resolve_and_score_funding_beliefs`. Now WIRED LIVE into `drive()`'s daily boundary by Track A
(`0ad3f3f`/`349881d`, GATE ACCEPT — `daemon.rs:2523`, gated on `weather_source` ⟺ venue=kalshi), so the
loop runs on each UTC day. Per DUE open belief it:
1. routes to ITS NWS CLI product by the forecast's grading station (`cli_serves_station` matches the
   AWIPS id `CLI{nws_station_id}` as a whole token — never a substring),
2. grades the realized daily high/low from the product's raw text via the Track-D grader
   `nws_cli_realized` (NEVER Aeolus — the V4 self-grading caution),
3. binary brackets: Brier the belief's OWN persisted `p` against the realized `ge`/`lt` outcome
   (`aeolus_resolve::score_bracket`) and `resolve_and_score`,
4. the scalar μ/σ belief: CRPS its persisted quantile fan vs the realized value + one `crps_pinball`
   `belief_scores` row.

DESIGN DECISION (calibration-safe, divergence from the handoff sketch's `score_reliability(&fc, …)`):
score the PERSISTED `p`/quantiles off the belief row, NOT a re-parsed `aeolus.forecast` signal — so
reliability scores exactly what FORTUNA believed. Today `p == p_raw`, so this is numerically identical
to the μ/σ re-derivation; once a weather-calibration layer makes `p ≠ p_raw`, grading the persisted `p`
is the correct behavior. This mirrors the funding resolver's reconstruct-from-the-belief-row pattern.
A grade the bridge cannot place (missing/ambiguous/jammed CLI, unparseable hint, unknown variable) ⇒
the belief stays OPEN, never fabricated (spec 5.12).

New surface (all additive, invariants untouched):
- `crates/fortuna-cognition/src/aeolus_resolve.rs` (pure): `cli_serves_station`, `parse_bracket_hint`,
  `realized_f_for`, `score_bracket`; reuses `aeolus_reliability::bracket_outcome` (made `pub` — one
  outcome rule, no drift) + `beliefs::brier_score`.
- F8 `aeolus_beliefs::provenance` now stamps `nws_station_id` (the grading station) so the resolver
  routes off the persisted row alone — never by re-parsing the source forecast.
- `fortuna_ledger::BeliefsRepo::open_aeolus_weather_due(now_iso, limit)` + `OpenWeatherBelief` (the
  work queue; mirrors `ScalarBeliefsRepo::unresolved_due`).

Verified: 8 cognition unit tests (`aeolus_resolve`), 2 ledger tests (`open_weather_due`: filters +
limit/order), 4 live integration tests (`weather_resolve`: happy 14 brackets + 1 scalar resolve vs the
recorded Troutdale 91°F grade, idempotent re-run, unroutable station stays OPEN, jammed CLI grades to
None ⇒ OPEN), and the upgraded `aeolus_e2e` (the `realized = 88.0` stub replaced by the real F2 grader
over the recorded Troutdale CLI). Full workspace battery + DST green (see commit). Seams ledgered in
GAPS: the missing recorded NYC CLI (`CLINYC`) fixture, multi-station CLI, weather-belief CLV,
negative-threshold hints, the bounded CLI scan.

Shared-doc touches: GAPS.md (bridge handoff → DONE + new sub-seams). MERGED as `341340e` (GATE ACCEPT);
the `drive()` wiring was then completed by Track A (`0ad3f3f`/`349881d`), so this loop is CLOSED + LIVE.
The only remaining weather item is the recorded NYC `CLINYC` CLI fixture (operator capture; non-blocking —
a missing product leaves the belief OPEN, never fabricated).

## F7 bucket-matching — Aeolus μ/σ → Kalshi tradeable buckets (Track-E side) (this commit)

Closes the F7 venue impedance (raised by Track-A on real demo data): Aeolus emits a cumulative
ge-ladder, Kalshi trades 2°-inclusive in-range buckets + two tails — a literal `ge{N}→≥N` 1:1 yields
~0 edges. Contract ALIGNED with Track-A and committed: `docs/design/aeolus-kalshi-bucket-matching.md`.

New `crates/fortuna-cognition/src/aeolus_buckets.rs` (the Track-E half):
- The seam types `WeatherBucket { market_key, kind }` + `BucketKind { InRange{lo,hi} | GreaterEq{M} |
  LessEq{M} }` (Track-A discovers the live day-set and constructs these).
- `aeolus_bucket_beliefs(&AeolusForecast, &[WeatherBucket]) -> Vec<BeliefDraft>`: one propose-only
  belief per discovered bucket, `p == p_raw =` the μ/σ bucket probability via the F6 helpers — a
  bucket is a DIFFERENCE of the cumulative ladder: `InRange{lo,hi}` = `bracket_range_prob(lo, hi+1)` =
  `ge(lo)−ge(hi+1)`, `GreaterEq{M}` = `ge(M)`, `LessEq{M}` = `lt(M+1)`. `event_id = aeolus:{market_key}`
  so Track-A's edge is `Direct` 1:1; provenance + bucket-bounds-in-evidence stamped (I6 propose-only).
- `score_bucket_briers(...)` — the F9 per-kind Brier extension (`InRange` ⟺ `lo ≤ realized ≤ hi`, etc.).

THE INVARIANT: for the recorded complete KXHIGHNY 2026-06-13 day-set (`≤86 | [87,88] | … | ≥95`) the
per-bucket p's **telescope to 1.0** (`[1−ge87]+[ge87−ge89]+…+ge95 = 1`) — VALIDATED in the e2e to
1e-9, with the in-the-money `[87,88]` bucket carrying real mass (~0.40, μ≈87.3). 3 tests (sum-to-1 +
1:1 mapping; per-kind Brier vs a realized high — exactly one bucket resolves true; propose-only key
set + empty input). F8's ge-ladder beliefs STAY as the reliability/cross-check vehicle — not the
tradeable path.

Built directly, tests-first. Verified: `cargo test -p fortuna-cognition --test aeolus_buckets` 3/3;
full workspace clippy + test (see commit). Track-A builds the venue half (discovery → `WeatherBucket[]`
→ `Direct` edges → `drive()` world-forward wiring) against the recorded Kalshi fixture, mutation-proven.

Shared-doc touches: none (new contract doc + module).
## F10 — v1↔v2 schema dispatch + registry/dossier (this commit)

Operator-directed residual (2026-06-14). Three parts:

- **v1→v2 fixture migration (the build):** `aeolus_forecast::parse_versioned(body) ->
  AeolusEnvelopeVersion` routes a raw Aeolus envelope by its OPTIONAL `schema` field (contract §9):
  `schema` ABSENT ⇒ **V1** (the legacy `reconciliation::AeolusEnvelope` — the `aeolus_eval` T2.7
  fixture path, kept alive and NOT weakened); `schema == "aeolus.forecast/v2"` ⇒ the strict **V2**
  parse (reusing F6's `RawEnvelope`+`validate`); any other schema ⇒ `UnknownSchema` (the §8
  co-evolution tripwire, never silently treated as v1). `V2` is boxed (it's ~4× the v1 envelope;
  the `Box` auto-derefs for the accessors). 3 dispatch tests (v1 sample → V1, recorded v2 → V2,
  bogus schema → error); the v1 `aeolus_eval`/`reconciliation` suite (4 tests) is UNTOUCHED and
  green. Additive: `reconciliation.rs` (the v1 parser) is not modified.
- **Layer-0 dossier:** already complete (`docs/research/sources/aeolus/dossier.md`, tier-7 SOBER,
  authored 2026-06-13) — states the MEASURED reality (contract §1/§5): μ commodity (~4–8% worse
  than raw GEFS), edge over raw is calibration, market edge unproven; admit HIGH on authenticity,
  MODEST empirical tier, Layer 3 earns it. No change needed; confirmed accurate vs the captured
  fixtures. (BUILD_PLAN F10 box ticked.)
- **source_registry row:** a ledgered OPERATOR SEED action (per the dossier Decision + GAPS) — the
  exact admission values (source_id `aeolus`, tier `7`, domain `weather`, `auth_env =
  AEOLUS_API_TOKEN`, https-only host pinned [stable host TBD — currently an ephemeral tunnel],
  resolution-eligible NO) are recorded in GAPS for the operator to apply when D9 wires sources.

Verified: `cargo test -p fortuna-cognition --test aeolus_forecast` 21/21 + `--test reconciliation`
4/4 (aeolus_eval untouched); full workspace clippy + test (see commit).

Shared-doc touches: none (the dossier is Track-E-owned + pre-existing).

## F4b — release-aware cadence (Aeolus scheduler consumes next_run_at) (commit ef3ddb0)

Operator-directed refinement (2026-06-14): Aeolus refinements F4b + F10 reassigned to track-e. This
touches Track-D's `crates/fortuna-sources` (operator-authorized for this slice), built so it CANNOT
change any other source's cadence.

F4b: the D9 ingestion scheduler schedules Aeolus's next poll JUST AFTER the advertised next forecast
run instead of a blind steady interval (contract §3.4 — arrive right after Aeolus publishes). Three
small changes, all OPT-IN:
- `aeolus.rs`: `aeolus_next_run_at(signal) -> Option<UtcTimestamp>` — an exact mirror of
  `aeolus_claimed_time` reading `payload["next_run_at"]` (wrong-kind / missing / unparseable → None).
- `scheduler.rs`: a `ReleaseHintFn` per-source fn-pointer (analogue of `ClaimedTimeFn`), an opt-in
  `release_hint: Option<ReleaseHintFn>` on `Registered` (init None — `register()` signature
  UNCHANGED, so its ~10 callers are untouched), `set_release_hint(id, hint)`, and the pure
  `release_aware_due_ms`: poll at `next_run_at + 90s lead`, clamped to `[now+30s, now+2·base]` (a
  past/imminent hint → poll soon; an absurd hint → cap at ~2 steady intervals). The tick reads the
  hint BEFORE the signal is moved and tracks the max next-run; the `next_due` `None` arm is
  BYTE-IDENTICAL to the pre-F4b `interval_at` code — VERIFIED in the diff, so a source without a hint
  is completely unchanged.
- `factory.rs`: `build_adapter` also yields the per-source release hint (aeolus → `Some`, all others
  `None`); `build_scheduler` calls `set_release_hint` only when present.

Subagent-built tests-first; main-loop read + verified the scheduler tick diff (the byte-identical
fallback is the load-bearing safety property). +12 tests (the clamp band, the extractor, an opt-in
release-cadence scheduler test, and `source_without_release_hint_keeps_exact_steady_cadence`). 131
fortuna-sources tests pass (0 regressed) + 5 DST. Verified: full workspace clippy + test (see commit).

NOTE on the sibling items: **F10's Layer-0 dossier already exists and is complete**
(`docs/research/sources/aeolus/dossier.md`, tier-7 sober, authored 2026-06-13) — the remaining F10
`source_registry` row is an operator seed action (ledgered). **E.3/E.5 are DONE** — the persona
runner-loop + scoring-scope merged into main via `persona-live-integration`; no new Track-E build
(operator-confirmed 2026-06-14).

## e2e — the assignment GATE: recorded forecast → persisted, scored bracket belief (commit ec2300a) — PIPELINE COMPLETE

New `crates/fortuna-ledger/tests/aeolus_e2e.rs` (`#[sqlx::test]`): the whole F5–F9 chain on the
RECORDED fixture, persisted to the real ledger. recorded forecast → F6 strict parse + μ/σ→p → F5
dedup (a duplicate run collapses to one) → F7 world-forward match (14 events; grading station "NYC"
≠ "KNYC") → F8 propose-only beliefs (14 binary brackets persisted to `beliefs` via `EventsRepo` +
`BeliefsRepo`; the scalar μ/σ fan persisted to `scalar_beliefs`) → F9 Brier+CRPS vs a RECORDED
realized high (88°F) → each binary belief `resolve_and_score`'d, the scalar CRPS persisted to
`belief_scores`.

THE GATE asserted: the borderline ge87 belief is **`status == "resolved"`** with `outcome == Some(1)`
(88 ≥ 87) and a persisted brier — a SCORED bracket belief, not merely parsed (the assignment's "a
pipeline that parses but never scores a bracket belief is NOT done"). **Calibration validated, not
asserted:** the persisted `p` equals the pinned μ/σ math (`bracket_prob_ge(87,μ,σ)`) to 1e-12 — the
same number F6 pinned to 6.9e-8 of Aeolus's own recorded p — and `brier == (p−1)²`. The scalar CRPS
landed under `crps_pinball` (the Layer-3 / ROTA §9.1 scorecard feed). Built directly, tests-first;
1/1 green on the live DB (no new sqlx queries — existing repos only).

**The Aeolus weather→belief pipeline (F5–F9 + e2e) is COMPLETE**: parse → dedup → match →
propose-only beliefs → reliability scoring, end-to-end, validated against recorded Aeolus + a
realized temperature. Two seams remain (ledgered, not Track-E-cognition): the live-Kalshi-market
intersection (F7, venue/Track-A) and the NWS-CLI productText→°F grader (F9's realized input, F2/
Track-D). The composition entry point (drive these on the live loop) is handed to Track A.

## F9 — Layer-3 empirical reliability scoring (Brier + CRPS vs realized) (commit d673c74)

New `crates/fortuna-cognition/src/aeolus_reliability.rs`: `score_reliability(&AeolusForecast,
realized_f) -> AeolusReliability`. THE LOOP (contract §5 Layer 3): FORTUNA INDEPENDENTLY re-scores
every Aeolus belief at settlement against the realized temperature (the independent NWS grader, NOT
Aeolus — V4 self-grading caution). Per `(model, scope)` = `{model_id:"aeolus", model_version,
station, variable, target_date}` (mirrors the F8 provenance the ROTA scorecard groups by):

- **Binary Brier** per `ge`/`lt` bracket: outcome = realized integer high satisfies the bracket
  (`ge t ⟺ realized ≥ t`), `brier = brier_score(p_fortuna, outcome)` (the belief's own μ/σ
  probability, recomputed via the F6 helpers — the SAME math F8 emitted).
- **Scalar CRPS** of F8's μ/σ quantile fan vs the realized value via the pinned `CrpsPinballRule`
  (the SAME scalar object F8 emits). `crps: Option<f64>` (None only on a post-parse-impossible
  scoring error — never a panic).

SEAM (GAPS): the realized value is an INPUT; extracting the official daily high/low from the NWS-CLI
`productText` (the F2 grader) is a source-side concern not yet in cognition — F9 takes the graded
value, the e2e supplies a RECORDED one. Pure + deterministic; no Clock::now.

Built directly (the scoring reuses `brier_score` + `CrpsPinballRule` + the F6/F8 building blocks),
tests-first. 4 tests against the recorded fixture (μ≈87.35/σ≈1.90): a realized high of 88 satisfies
ge81..88 (8 true) and not ge89..94 (6 false); each brier is exactly `(p−outcome)²`; confident+correct
tails (ge81/ge94) score Brier <1e-3; the integer boundary (realized=87 ⟹ ge87 true, ge88 false); a
colder realized (80) flips all outcomes and grows the CRPS. Verified: `cargo test -p fortuna-cognition
--test aeolus_reliability` 4/4; full workspace clippy + test (see commit).

Shared-doc touches: none (new file only).

## F8 — propose-only belief emission (binary brackets + scalar μ/σ fan) (commit 142d762)

New `crates/fortuna-cognition/src/aeolus_beliefs.rs`: `emit_aeolus_beliefs(&AeolusForecast) ->
AeolusBeliefs { binary: Vec<BeliefDraft>, scalar: ScalarBeliefDraft, skipped_in_bracket }`. The
PROPOSE-ONLY producer step (I6 — beliefs only; no order/size/price/side on any output; the harness
owns sizing/gating/execution):

- **Binary bracket drafts** — one per `ge`/`lt` bracket, `event_id = aeolus:{event_hint}`, `p =
  p_raw =` FORTUNA's OWN μ/σ probability via the F6 helpers (`bracket_prob_ge`/`bracket_prob_lt`,
  the −0.5 correction inside them — F8 passes the raw integer threshold, no double-correction). NO
  calibration here (downstream layer; `p == p_raw`). `horizon = resolution.settles_after`. Evidence
  carries the cross-check (`p_aeolus`/`p_fortuna`/`divergence`) + skill as DATA; provenance carries
  `{model_id:"aeolus", station, variable, target_date, run_at, model_version}`. `in_bracket`
  brackets skipped + counted (a single threshold can't define a range).
- **One scalar draft** — the μ/σ distribution as a PINNED 7-point standard-normal quantile fan
  (`v = μ + σ·z`, fixed `(q,z)` table — no probit/erf-inverse, replay-byte-stable; σ>0 ⇒ strictly
  increasing ⇒ always `validate`s), `unit="degF"`, `event_key = aeolus:{station}:{variable}:
  {target_date}` — F9's CRPS vehicle.

Subagent-built tests-first; main-loop read + verified + feature-dev:code-reviewer. The reviewer's
one "Critical" (producer stamping `provenance` vs the `BeliefDraft` "harness-stamps" doc) was
VERIFIED a false alarm: both deterministic-producer precedents stamp provenance at the producer
(`persona_beliefs` → `{persona_id,…}`, `reconciliation`'s v1 Aeolus mapper → `{model_id:"aeolus",…}`)
and the scoring layer KEYS on it (`resolved_persona_stats` reads `provenance->>'persona_id'`), so F9
REQUIRES F8 to stamp `provenance->>'model_id'`; the doc refers to the LLM-mind proposal path. Kept
as-is. 7 tests (fixture: 14 ge drafts with `p == bracket_prob_ge`, `p==p_raw`, propose-only key set
`{event_id,p,p_raw,horizon,evidence,provenance}`, evidence cross-check, scalar fan validates + q=0.5≈μ,
lt path complementary). Verified: `cargo test -p fortuna-cognition --test aeolus_beliefs` 7/7; full
workspace clippy + test (see commit).

Shared-doc touches: none (new file only).

## F7 — world-forward match (forecast → predicted weather market-family) (commit efcaffe)

New `crates/fortuna-cognition/src/aeolus_match.rs`: `match_forecast(&AeolusForecast) ->
WeatherMarketFamily` synthesizes the temperature-bracket events a forecast predicts (spec §5.12
world-forward discovery). One `WeatherEvent` per bracket, keyed `aeolus:{event_hint}` (the v1
namespace), carrying threshold/comparison + Aeolus's own bracket p (the F8 cross-check); the family
carries the forecast identity, `model_version`, and the RESOLUTION declaration (grading
`nws_station_id` + authority + `settles_after`) so every synthesized event is SCOREABLE (§5.12). Pure
+ deterministic; bracket order preserved. SEAM (GAPS): intersecting the synthesized family with the
LIVE Kalshi book (does this bracket trade now?) is a venue-discovery concern, not this cognition
transform — F7 produces the forecast side.

Test against the RECORDED fixture: 14 events keyed `aeolus:knyc-2026-06-13-tmax-ge81…ge94`,
thresholds 81..94 in order, and the grading station resolved to "NYC" (DISTINCT from the Aeolus
station "KNYC" — taken from `resolution.*`, never inferred). Built directly (small, pure),
tests-first. Verified: `cargo test -p fortuna-cognition --test aeolus_match` green; `fmt` + `clippy
--workspace --all-targets -D warnings` clean; full workspace test green.

Shared-doc touches: none (new file only).

## F5 — identity-tuple dedup (newest run per slot) (commit d78c335)

New `crates/fortuna-cognition/src/aeolus_dedup.rs`: `dedup_forecasts(Vec<AeolusForecast>) ->
Vec<AeolusForecast>` collapses forecasts to one per `(station, variable, target_date)` slot — the
newest `run_at` wins, so a re-issued GEFS run never double-counts; a same-`run_at` correction
resolves to the later-received envelope (it supersedes, contract §3 ETag rule). Pure + deterministic:
a first-seen-ordered `Vec` (not a map), so the output is a pure function of the input (no clock, no
iteration-order surprises, no panic). Operates on F6's typed `AeolusForecast` via its `identity()`
surface. 5 tests (newest-wins-either-order, same-run revision supersedes, distinct slots survive in
order, empty/single identity, many-runs-collapse-irrespective-of-arrival). Built directly (small,
pure), tests-first. Verified: `cargo test -p fortuna-cognition --test aeolus_dedup` 5/5; `fmt` +
`clippy --workspace --all-targets -D warnings` clean; full workspace test fully green.

Shared-doc touches: none (new file only).

## F6 — strict v2 parser + μ/σ→bracket-p (the deterministic foundation) (commit 7f451bc)

New `crates/fortuna-cognition/src/aeolus_forecast.rs`. Two pure, replay-deterministic
pieces:

- **The μ/σ→p backbone** — `bracket_prob_ge`/`bracket_prob_lt`/`bracket_range_prob`,
  reusing the PINNED in-repo erf (`persona_beliefs::{normal_cdf, prob_at_least}`, A&S
  7.1.26 — not platform `libm`, so byte-identical replay holds, contract §7/I5). Kalshi
  brackets are integer degrees, so a `ge t` bracket is `P(high ≥ t) = P(T ≥ t − 0.5)`
  (a half-degree continuity correction). `bracket_range_prob` subtracts UNCLAMPED `ge`
  values before the final clamp (exact in the distribution body). Every result clamped
  into `(ε, 1−ε)`; `None` on σ≤0/non-finite.
- **The strict envelope parser** — `parse_envelope` / `parse_response` with
  `deny_unknown_fields` on every struct + renamed enums (so `family`/`units`/`variable`/
  `comparison`/`authority` drift is a hard parse error). Semantic `validate()`: pin
  `schema == "aeolus.forecast/v2"`, reject σ≤0/non-finite (handles NaN/+∞), require ≥1
  bracket + non-empty `event_hint`, and CLAMP each `brackets[].p` into `[1e-6, 1−1e-6]`
  (clamp-not-reject, rev-3). Skill fields (`crps`/`crpss_vs_raw`/`n_scored`) are nullable
  (`crpss_vs_raw` ships `null`). `AeolusForecast` wraps the validated envelope (private
  fields + accessors) and exposes the identity tuple `(station, variable, target_date,
  run_at)` for the F5 dedup slice.

**THE GATE — calibration VALIDATED, not asserted:** the test parses the RECORDED fixture
`fixtures/sources/aeolus/knyc_tmax.json` and checks FORTUNA's `bracket_prob_ge` against
all 14 recorded bracket p-values. **Max abs delta = 6.868e-8** across all 14 — the A&S
erf-approximation residual (contract §7's ~1e-7..1e-12 class), NOT a formula error (a
missing −0.5 would miss by ~0.1, e.g. ge87 → 0.572 vs the recorded 0.672). The `+00:00`
(non-`Z`) `run_at` parses via `UtcTimestamp::parse_iso8601` (chrono RFC3339).

Built by a subagent tests-first; main-loop read + verified the module + re-ran the gate.
18 tests (fixture-validation + σ≤0 / wrong-units / unknown-field / wrong-schema reject +
clamp + nullable-skill + identity-tuple). Verified: `cargo test -p fortuna-cognition
--test aeolus_forecast` 18/18; `fmt` + `clippy --workspace --all-targets -D warnings`
clean; full workspace test FULLY green (160 suites, 0 failures — Track C's `kinetics_dto`
fix has landed on main).

Shared-doc touches: none (new files only).
