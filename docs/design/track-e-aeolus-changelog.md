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

## F8 — propose-only belief emission (binary brackets + scalar μ/σ fan) (this commit)

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
