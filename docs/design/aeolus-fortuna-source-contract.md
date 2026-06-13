# Aeolus → FORTUNA Source Contract

Wire schema: `aeolus.forecast/v2`.
Contract document: rev 2 (2026-06-13) — Track-D review integrated.
Status: DRAFT for operator approval. Conforms to docs/spec.md v0.9
§5.10/§5.11/§5.12 and the four-layer trust framework in
docs/superpowers/specs/2026-06-12-news-aggregation-design.md §4.4. Supersedes
the minimal v1 envelope (fixtures/aeolus/sample_envelope.json) — migration in
§9. The spec wins on conflict; extensions are flagged in §10.

> **Why the wire schema stays `v2` while the document is rev 2.** The rev-2
> changes are FORTUNA-side, process, and dependency clarifications — they do
> NOT add, remove, or retype any field Aeolus emits. The Aeolus build target
> (§2, §3, Appendix) is unchanged from the prior draft. Per the co-evolution
> rule (§8), only a change to an emitted field bumps the wire schema string.

### Changelog (rev 2, from Track-D implementation review)

The adapter substrate (crates/fortuna-sources) now exists through D4; this
revision makes the contract slot into it without hand-waving. Concretely:

1. **Auth is a named substrate prerequisite (§3.1, new).** The shared
   `FetchClient` had no Authorization path (it set only `User-Agent`, because
   NWS needs no key). Bearer auth is now a listed track-D substrate addition
   that lands BEFORE `AeolusSource`.
2. **Dedup no longer mis-fires on skill refreshes (§3, §4 sharpened).** The
   ETag is scoped to forecast identity + distribution, EXCLUDING volatile
   `skill.*`; FORTUNA's belief identity keys on `(station, target_date,
   variable, run_at)`. A skill recompute on an unchanged forecast now 304s and
   supersedes-in-place instead of spawning a spurious "new forecast" signal.
3. **Layer 1 is disambiguated (§5).** Schema/semantic validation (the strict
   parse) runs in cognition; the structural validator (future-dated, stale-
   republication, volume envelope) runs in the D9 scheduler. Aeolus signals
   get BOTH, like every other source — they are different checks in different
   places, not one thing.
4. **The grader the loop depends on is named as a build dependency (§3.2,
   new).** `resolution.authority = nws_observed_high` requires a registered
   NWS *observed-daily-high* source. D4's `NwsSource` covers alerts + forecast
   discussions, NOT observations — so the closed Layer-3 loop needs an
   observations feed that must exist and be in `source_registry` before any
   Aeolus weather belief can be scored (§5.12 forbids unscoreable beliefs).
5. **Replay determinism for μ/σ→p (§7).** The normal CDF needs an `erf`; a
   platform `libm` erf can vary, breaking byte-identical replay (I5). A pinned
   deterministic approximation is now required for the helper.
6. **`next_run_at` is explicitly an OPTIONAL scheduler hint (§2, §11).** The
   D9 scheduler drives cadence from config; consuming `next_run_at` for
   dynamic per-source cadence is a deferred capability, not a v2 dependency.

## 1. Purpose and the division of expertise

Aeolus is a CRPS-validated probabilistic temperature-forecast system that beats
raw NOAA at most stations. It is a PROPRIETARY signal input — the exact
"durable edge" docs/spec.md §1 names. This contract wires it into FORTUNA's
weather domain as a high-trust source.

The division is clean and non-overlapping, and it is the whole design:

- **Aeolus owns the forecast.** It emits the calibrated predictive
  distribution (μ, σ) per station/date with its own skill metadata. It does
  NOT need to know which Kalshi markets exist, how they are sized, or whether
  FORTUNA trades them.
- **FORTUNA owns the market reasoning.** It maps the distribution to whatever
  Kalshi temperature brackets it discovers (any threshold, not a fixed list),
  forms beliefs, applies its OWN calibration layer (§5.10), sizes, gates, and
  scores outcomes back against Aeolus's forecast for trust attribution.

Consequence: **Aeolus emits the DISTRIBUTION, not just point bracket
probabilities.** Point probabilities for a fixed bracket list (the v1 envelope)
under-serve FORTUNA — it can only reason about the brackets Aeolus pre-chose.
From (μ, σ) FORTUNA computes P(temp ≥ t) for ANY threshold t a Kalshi market
defines: `P = 1 - Φ((t − μ)/σ)`. The convenience brackets remain (a cross-check),
but μ/σ is the load-bearing payload.

## 2. The wire contract — the Aeolus forecast envelope (v2)

One JSON object per (station, target_date, forecast variable). FORTUNA parses
this STRICTLY (`deny_unknown_fields`) — an unknown field is a hard parse error,
on purpose, so contract drift surfaces immediately (§8).

```json
{
  "schema": "aeolus.forecast/v2",
  "station": "KNYC",
  "nws_station_id": "KNYC",
  "variable": "tmax",
  "units": "degF",
  "target_date": "2026-06-12",
  "run_at": "2026-06-11T10:00:00.000Z",
  "next_run_at": "2026-06-11T16:00:00.000Z",
  "valid_until": "2026-06-12T00:00:00.000Z",

  "distribution": {
    "family": "normal",
    "mu": 64.3,
    "sigma": 3.1,
    "model_version": "sar-semos-v1"
  },

  "skill": {
    "crps": 1.145,
    "crpss_vs_raw": 0.12,
    "n_scored": 11174,
    "window_days": 30,
    "as_of": "2026-06-11T00:00:00.000Z"
  },

  "resolution": {
    "authority": "nws_observed_high",
    "nws_station_id": "KNYC",
    "settles_after": "2026-06-13T00:00:00.000Z",
    "note": "official NWS daily maximum for the station, in degF"
  },

  "brackets": [
    { "event_hint": "highny-2026-06-12-t60", "threshold_f": 60, "comparison": "ge", "p": 0.92 },
    { "event_hint": "highny-2026-06-12-t65", "threshold_f": 65, "comparison": "ge", "p": 0.41 },
    { "event_hint": "highny-2026-06-12-t70", "threshold_f": 70, "comparison": "ge", "p": 0.08 }
  ]
}
```

### The forecast-identity tuple (load-bearing for dedup, rev 2)

`(station, target_date, variable, run_at)` uniquely identifies one forecast
run. It is the dedup key (§3, §4): two envelopes sharing it ARE the same
forecast even if their `skill.*` telemetry differs. Nothing in the envelope may
make two same-identity forecasts hash differently except `distribution`
(a corrected μ/σ at the same `run_at` is a genuine revision, see §3 ETag rule).

### Field semantics (every field is load-bearing or it is cut)

| field | type | meaning / FORTUNA use |
|---|---|---|
| `schema` | string const `aeolus.forecast/v2` | version pin; FORTUNA rejects any other value (forces lockstep upgrades). |
| `station` | string | Aeolus station id (e.g. KNYC). Part of the identity tuple. |
| `nws_station_id` | string | the OFFICIAL station the bracket resolves against; usually == station, declared explicitly so FORTUNA never infers it. |
| `variable` | enum `tmax` \| `tmin` \| `hdd` \| `cdd` (open; see §3.3) | the forecast variable. tmax/tmin are the daily extremes; hdd/cdd are heating/cooling degree-days (energy/degree-day market families). FORTUNA consumes EVERY variable Aeolus exposes (operator decision, rev 2). Degree-day variables carry their own resolution model and possibly a non-`normal` family — settled per §3.3. FORTUNA keys events by variable. Part of the identity tuple. |
| `units` | enum `degF` | guards against a silent °C drift; FORTUNA asserts it. |
| `target_date` | YYYY-MM-DD (station-local) | the forecast day. Part of the identity tuple. |
| `run_at` | UTC ISO8601 | when Aeolus produced this run = the forecast `init_time`, NOT the API response time — POINT-IN-TIME authority for the belief's evidence and freshness (§5.11 received_at honesty). Part of the identity tuple. |
| `next_run_at` | UTC ISO8601 | when the next run is expected. The D9 scheduler CONSUMES this for release-aware cadence (§3.4) — it schedules the next poll around it. Absence is legal (the scheduler falls back to the configured base_interval / release pattern). |
| `valid_until` | UTC ISO8601 | after this, the forecast is stale (the market is resolving); FORTUNA stops trusting it for new triggers (freshness, §5.5 — enforced FORTUNA-side, not in the adapter). |
| `distribution.family` | enum `normal` | the predictive family. v2 supports `normal` only; richer families = a v3 extension (§10). |
| `distribution.mu` | f64, in `units` | predictive mean. THE primary signal. |
| `distribution.sigma` | f64 > 0, in `units` | predictive std-dev. FORTUNA rejects σ ≤ 0 (degenerate). |
| `distribution.model_version` | string | e.g. `sar-semos-v1`; rides into belief provenance; a model change is a visible event. |
| `skill.crps` | f64 ≥ 0 | recent CRPS for this station/variable. TELEMETRY — excluded from the dedup ETag (§3). |
| `skill.crpss_vs_raw` | f64 | skill score vs raw NOAA (>0 = beats raw). FORTUNA's Layer-3 trust weighting reads this. TELEMETRY. |
| `skill.n_scored` | int ≥ 0 | sample size behind the skill numbers (a skill claim on n=3 is not a skill claim). TELEMETRY. |
| `skill.window_days` | int | the trailing window for the skill stats. TELEMETRY. |
| `skill.as_of` | UTC ISO8601 | when the skill stats were computed. TELEMETRY — changes without the forecast changing; see §3 ETag rule. |
| `resolution.authority` | enum `nws_observed_high` \| `nws_observed_low` | how the event settles — REQUIRED (§5.12). Requires a registered grader source (§3.2). |
| `resolution.nws_station_id` | string | the grading station. |
| `resolution.settles_after` | UTC ISO8601 | earliest the official obs is final. |
| `resolution.note` | string | human description; not parsed for logic. |
| `brackets[]` | array, ≥1 | convenience pre-computed probabilities + the threshold definitions FORTUNA needs to map to markets. |
| `brackets[].event_hint` | non-empty string | stable id; `aeolus:{event_hint}` is FORTUNA's event-id namespace (unchanged from v1). |
| `brackets[].threshold_f` | i64 (°F) | the threshold; integer °F (Kalshi temp brackets are integer-degree). |
| `brackets[].comparison` | enum `ge` \| `lt` \| `in_bracket` | how the threshold reads. |
| `brackets[].p` | f64 in (0,1) | Aeolus's own probability for the bracket — a CROSS-CHECK against FORTUNA's μ/σ-derived p; a large divergence is a data-quality alarm, not silently averaged. p in {0,1} is rejected (certainty is schema-invalid, matching FORTUNA's belief rule §5.5). |

## 3. The endpoint (the Aeolus-team build target)

Aeolus exposes ONE read-only endpoint. It is a pull (FORTUNA polls); no
push/webhook in v2.

```
GET /v2/forecasts?station={id}&variable={tmax|tmin|hdd|cdd}&from={date}&to={date}
Auth: Bearer token (Aeolus-issued API key; FORTUNA holds it in env only —
      never in repo/config/logs).
Host: pinned in FORTUNA's source_registry row; https-only.
Response: 200 → { "forecasts": [ <v2 envelope>, ... ] }   (one per station/date/variable in range)
          304 → unchanged (FORTUNA sends If-None-Match / If-Modified-Since;
                Aeolus returns ETag — steady-state polling is near-free)
          4xx/5xx → typed error body { "error": { "code", "message" } }
```

Requirements on Aeolus's side, in priority order:
1. **The distribution is mandatory and exact** — μ/σ straight from `forecast_log`
   (the production SAR-SEMOS state), not a re-derivation. model_version from the
   same row.
2. **Skill from `scorecards`** — the trailing-window CRPS / CRPSS-vs-raw and
   n_scored for that station/variable. If a station has too few scored days,
   emit `skill` with the real (small) `n_scored` — never fabricate a skill
   number; FORTUNA down-weights low-n sources by design.
3. **run_at = the forecast init_time**, not the API response time — point-in-time
   honesty is load-bearing (a belief's evidence timestamp must be when the
   forecast was actually made).
4. **Deterministic ETag scoped to the forecast, NOT the telemetry (rev 2).**
   The ETag is computed over the forecast-identity tuple + `distribution` +
   `resolution` + `brackets` — i.e. everything that defines the forecast —
   but EXCLUDING `skill.*` and `next_run_at`. Consequence: when Aeolus
   recomputes skill stats for an unchanged forecast, the ETag is unchanged and
   FORTUNA's conditional GET returns 304 (no re-ingest, no spurious signal). A
   corrected μ/σ at the same `run_at` DOES change the ETag — that is a real
   revision and should supersede.
5. **No secrets, no PII** in the payload. Station ids and forecasts only.
6. **Stable schema string** — bump to `aeolus.forecast/v3` for ANY breaking
   field change; never silently add/remove fields under v2 (FORTUNA's strict
   parser will hard-fail, which is the intended tripwire, but a version bump is
   the courteous form).

### 3.1 FORTUNA-side substrate prerequisite — auth (rev 2, new)

The shared `FetchClient` (crates/fortuna-sources, D2) currently supports
host-pinning, https-only, conditional GET, redirect re-validation, and the
per-host politeness budget — but it sets only a `User-Agent` header and has NO
Authorization path (NWS, the first source, needs no key). Aeolus needs Bearer
auth. Therefore, BEFORE `AeolusSource` can be built, the substrate gains a
small, generic per-source auth-header capability:

- a per-source optional `Authorization: Bearer <token>` (and, generically, an
  arbitrary header allowlist) injected by the transport;
- the token sourced from an env var named in config (e.g.
  `AEOLUS_API_TOKEN`) — never in config, repo, logs, or audit payloads
  (house rule + §5.11);
- redacted everywhere it could surface (error strings, debug, telemetry).

This is a track-D substrate task (a "D-series" item), not Aeolus's work. It is
listed here so the dependency is explicit and ordered.

### 3.2 FORTUNA-side build dependency — the resolution grader (rev 2, new)

`resolution.authority = nws_observed_high` is only meaningful if FORTUNA can
fetch the OFFICIAL observed daily maximum to score the belief. That is a
different NWS endpoint than D4's `NwsSource` covers (which fetches active
ALERTS and forecast DISCUSSIONS, not station observations). The closed Layer-3
loop (§5) therefore depends on:

- an NWS observed-daily-extreme feed (e.g. `/stations/{id}/observations` or the
  CF6 climate product), as a new `NwsFeed::Observations` variant or a sibling
  adapter;
- that feed registered in `source_registry` so it is a VALID resolution source
  per §5.12 (watchlist events declare their resolution source from the
  registry; an event with no checkable resolution source is `unscoreable` and
  excluded — so without this, Aeolus weather beliefs cannot be created).

This dependency is named so it is sequenced before (or with) `AeolusSource`,
not discovered late. It is also independently useful: the observed-high feed is
the grader for ANY weather belief, Aeolus-sourced or not.

### 3.3 Variable classes — consume everything (rev 2, operator decision)

FORTUNA consumes every variable Aeolus produces, not just tmax/tmin. tmax/tmin
ship in v2 as-specified. Degree-day variables (`hdd`/`cdd`) and any future
class are accepted, with two pieces that MUST be settled per-class with the
Aeolus team before that class goes live (they are not just a new enum value):

- **Distribution.** A degree-day is a derived, truncated/accumulated quantity;
  its predictive distribution is generally NOT a single-day `normal`. Each
  non-temperature variable declares its own `distribution.family` (e.g. a
  gamma or an accumulation model), or supplies the per-day `normal` components
  FORTUNA composes. This is the only place v2's "normal only" relaxes, and it
  relaxes per-variable, explicitly.
- **Resolution.** Degree-days settle against an ACCUMULATION of official obs
  over the period, not a single daily extreme — so `resolution.authority`
  gains accumulation variants and the §3.2 grader must expose the accumulated
  series, not just the daily high.

Net: "consume everything" is the standing decision; each new variable class is
a small, gated contract addendum (distribution + resolution spelled out), never
a silent widening.

### 3.4 Smart, release-aware cadence (rev 2, operator decision)

The scheduler does NOT poll on a dumb fixed interval. It is release-time-aware
and consumes every timing hint available:

- **`next_run_at` is consumed** (no longer merely optional): after a fetch, the
  source's next poll is scheduled around the advertised next run, so FORTUNA
  arrives just after Aeolus publishes rather than polling blind.
- **Known release schedules drive event windows.** For Aeolus this is the GEFS
  cadence (~6h per station); for the macro sources it is the BLS/Fed/FRED
  calendar; the scheduler boosts to tight polling in the window around each
  expected publish and idles otherwise (the §4.2 event-window mechanism of the
  news-aggregation design, generalized so every source can declare its release
  pattern — static schedule, payload hint, or calendar-derived).
- **Triggers and hooks.** A `release_scheduled`/`release_printed` signal (or a
  future webhook/push, the design's Phase-D source class) can FIRE a poll
  rather than wait for the next tick — so a known 12:30Z CPI print or a pushed
  "new Aeolus run" wakes ingestion immediately.

This is D9 (the ingestion scheduler) territory; the contract records the
expectation so the scheduler is built to consume `next_run_at` + a per-source
release pattern from day one, not retrofit it.

## 4. The FORTUNA side — ingestion path

`AeolusSource` (a track-D `fortuna-sources` adapter) is a thin wrapper:

```
AeolusSource::fetch() -> Vec<RawSignal>
  GET the endpoint (FetchClient: host-pinned, https-only, conditional GET,
      politeness budget, Bearer auth per §3.1) -> for each v2 envelope ->
  RawSignal { kind: "aeolus.forecast", payload: <the envelope JSON>,
              received_at: <clock now> }
```

The adapter is DUMB (spec 5.11): it fetches, parses the transport JSON into one
`RawSignal` per envelope, and emits the raw envelope as the payload untouched.
It does NOT do the strict schema parse, the σ>0/units checks, dedup, or trust
weighting — those are downstream (§5). The one piece of Aeolus-shape knowledge
the adapter exposes (mirroring D4's `nws_claimed_time`) is the
forecast-identity tuple + claimed time (`run_at`), so the D9 scheduler can run
the structural validator and the normalizer can dedup on identity.

Then the EXISTING cognition pipeline takes over:
- **D9 scheduler structural validation (Layer 1a):** the `StructuralValidator`
  runs the future-dated check (against `run_at`), stale-republication flag, and
  per-tick volume envelope — same as every source.
- **`normalize_and_dedup`:** source_registry check (fail-closed allowlist) ->
  the SignalEnvelope `{source, type, received_at, payload, content_hash}` ->
  dedup. Dedup is on the forecast-identity tuple `(station, target_date,
  variable, run_at)` (rev 2) — NOT a hash of the whole payload — so a skill-only
  refresh does not masquerade as a new forecast (and §3.4's ETag means it
  rarely even reaches here).
- The signal lands in the append-only `signals` table.
- **World-forward discovery (§5.12)** synthesizes / matches the weather event
  from the signal, declaring the resolution source from `resolution.*` — which
  must resolve to the registered grader of §3.2.
- **Synthesis (Layer 1b — strict parse):** the strict `AeolusEnvelope` parse
  (`reconciliation.rs`, already `deny_unknown_fields`) validates the full v2
  shape — σ>0, units==degF, p∈(0,1), no unknown fields — and is where a
  malformed run is refused. From μ/σ it computes `p_raw` for the market's
  threshold; FORTUNA's calibration layer (§5.10) produces `p`; the
  `brackets[].p` cross-check rides in evidence.
- The mapped `BeliefDraft` keeps the v1 namespace: `event_id =
  aeolus:{event_hint}`, with `evidence` carrying the Aeolus ref + skill, and
  harness-stamped `provenance` (model_id, station, run_at, model_version).
  Belief identity keys on `(event, run_at)`, so a corrected same-`run_at`
  forecast supersedes rather than duplicates.

NEW FORTUNA-side work (co-evolved with this contract, gated): extend the strict
`AeolusEnvelope`/parse in reconciliation.rs from v1 (station/target_date/run_at/
brackets[{event_hint,p}]) to v2 (the full §2 shape), with the μ/σ → threshold-
probability helper (§7 determinism) and σ>0 / p∈(0,1) / units==degF validation.
v1 fixtures migrate per §9.

## 5. Trust-framework fit (the closed loop — this is the elegant part)

- **Layer 0 (admission):** Aeolus is an operator-owned, authenticated source —
  admitted at HIGH initial tier with its dossier (authenticity = the operator's
  own system, the bearer token proves it, ToS = N/A internal). Dossier lives at
  docs/research/sources/aeolus/dossier.md per the Layer-0 template.
- **Layer 1 (structural) — TWO complementary checks in two places (rev 2):**
  - **1a, scheduler (D9):** the generic `StructuralValidator` —
    future-dated reject (vs `run_at`), stale-republication flag, per-tick
    volume envelope. Aeolus gets this like every source.
  - **1b, cognition (reconciliation.rs):** the strict schema/semantic parse —
    σ≤0, units≠degF, p∈{0,1}, unknown fields all refuse. This is SCHEMA
    conformance, which §4.4 names as part of Layer 1; it is NOT the same thing
    as 1a, and the contract should not equate them.
- **Layer 2 (corroboration):** for FAST triggers, weather can corroborate
  Aeolus against raw NWS (the §3.2 grader / a separate registry source) — but
  since Aeolus's `crpss_vs_raw` already proves it beats raw, Aeolus alone clears
  the fast-trigger tier; NWS is the divergence alarm, not a gate.
- **Layer 3 (empirical):** THE LOOP. Aeolus SELF-REPORTS its skill (`skill.*`);
  FORTUNA INDEPENDENTLY re-scores every Aeolus-sourced belief by Brier/CLV at
  settlement against the declared resolution source (the §3.2 observed-high
  grader). FORTUNA's trust attribution compares its own measured skill to
  Aeolus's self-reported skill — agreement reinforces trust; a gap (Aeolus
  claims skill FORTUNA doesn't observe) is a flagged anomaly. Self-graded
  caution (V4): the resolution authority is NWS, NOT Aeolus, so these beliefs
  are NOT self-graded — the grader is independent of the forecaster. Clean.

## 6. Security / abuse posture

Trusted, operator-owned source — but still DATA, never instructions (§5.11):
- The forecast payload reaches the model only inside the context assembler's
  delimited data blocks; I6 (propose-only) and I1 (gates) bound it regardless.
- Host pinned in source_registry; https-only; bearer token env-only and
  redacted everywhere (§3.1).
- The numbers are validated structurally (§2, §5 Layer 1b) before they can move
  capital; a poisoned/buggy Aeolus run (σ absurd, μ off-planet) fails the strict
  parse or is caught by the bracket-vs-distribution cross-check, not silently
  traded.

## 7. Determinism / money discipline

- μ/σ are f64 (forecast space — probabilities are f64 in cognition, allowed).
  The moment a probability becomes a SIZE, it crosses into integer cents in the
  harness — Aeolus never touches money.
- `threshold_f` is integer °F; the μ/σ→p computation is pure and replayable from
  the stored signal (the raw envelope is persisted, so a belief replays
  byte-identically).
- **Replay-deterministic normal CDF (rev 2).** `P = 1 - Φ((t−μ)/σ)` needs an
  `erf`/`erfc`. A platform `libm` erf is NOT guaranteed bit-identical across
  toolchains/architectures, which would break I5 byte-identical replay. The
  μ/σ→p helper MUST use a pinned, in-repo deterministic approximation (a fixed
  rational/poly erf with documented max error), not the system math library.
  A DST/property test pins a table of (μ, σ, t) → p vectors so a drift fails
  loud.
- received_at from the injected Clock; no wall-time in the ingestion path.

## 8. Co-evolution discipline

The contract version (`schema` string) and FORTUNA's strict parser change
TOGETHER, in one gated change, or not at all. The `deny_unknown_fields`
strictness is the feature: it makes a unilateral Aeolus change fail loud in
FORTUNA's gate rather than silently corrupt beliefs.

**The trade is explicit (rev 2):** strictness buys "no silent drift" at the
cost of "even an ADDITIVE field requires a lockstep deploy." For a proprietary,
correctness-critical, operator-owned edge that is the right trade; for a
third-party feed it would not be. We choose strict here deliberately, and the
existing `AeolusEnvelope` (already `deny_unknown_fields`) sets the precedent.

Process: a contract change is a versioned design-doc edit -> FORTUNA parser
update + fixture re-record -> gated -> Aeolus deploys the matching version.
Neither side ships a breaking change alone.

## 9. Migration from v1

v1 (the committed sample_envelope.json: station/target_date/run_at/brackets
[{event_hint,p}]) is a strict SUBSET of v2's brackets minus the distribution/
skill/resolution. Migration: (a) keep the v1 parser working behind
`schema` absent => treat as v1 (back-compat for the aeolus_eval fixture test);
(b) add the v2 parser for `schema == "aeolus.forecast/v2"`; (c) re-record
fixtures/aeolus/sample_envelope.json as a real v2 response once Aeolus exposes
the endpoint; (d) the AeolusSource adapter emits only v2. The T2.7
aeolus_eval test stays green throughout (it pins v1; do not weaken it).

## 10. Spec deltas (for GAPS.md at implementation time)

1. Distribution-emitting source (μ/σ) — §5.11 is agnostic to payload shape;
   this enriches it. Conservative: the raw envelope is persisted, the μ/σ→p
   computation is FORTUNA-side, replayable, and uses a pinned deterministic erf
   (§7).
2. Source self-reporting skill — new; cross-checked by FORTUNA's own scoring,
   never trusted blind (Layer 3). Skill is telemetry, excluded from the dedup
   ETag (§3).
3. `normal` family only in v2 — richer predictive families (mixtures, skew) are
   a v3 extension when a station needs it.
4. Per-source auth header in the fetch substrate (§3.1) — generic capability,
   secret env-only, redacted; not Aeolus-specific.
5. Forecast-identity dedup (`station, target_date, variable, run_at`) instead of
   whole-payload hashing for this source (§4) — so volatile telemetry does not
   spawn signals.
6. Resolution grader dependency (§3.2): an NWS observed-daily-extreme source
   must exist and be registered before Aeolus weather beliefs are scoreable.

## 11. Resolved decisions (rev 2) and remaining open questions

Resolved by operator, 2026-06-13:

- **Variable classes — consume everything.** FORTUNA ingests every variable
  Aeolus exposes (tmax/tmin now; hdd/cdd and beyond as added), each new class a
  small gated addendum specifying its distribution + resolution (§3.3). NOT
  deferred to Phase B.
- **Backfill — assume date-range query.** The endpoint serves historical runs
  over `from`/`to`, not just the latest, so FORTUNA can warm-start backtest /
  calibration. Aeolus's retention horizon is the only open sub-question (how far
  back the history goes); the QUERY ability is assumed and built against.
- **Smart, release-aware cadence (§3.4).** The scheduler consumes `next_run_at`
  and a per-source release pattern (GEFS cadence for Aeolus, the macro calendar
  for BLS/Fed/FRED), with triggers/hooks able to fire a poll on a known release
  or a push. Built into D9 from the start.

Remaining open (genuinely undecided; none block the adapter):

- Aeolus's historical RETENTION horizon (how many days/years of runs the
  backfill endpoint serves) — Aeolus-side decision.
- Degree-day distribution/resolution specifics (§3.3) — settled with the Aeolus
  team when the hdd/cdd class is first wired.
- The persisted "domain-analysis" layer (a meteorologist/economist persona that
  reasons over the ingested signals and emits a reusable analysis many beliefs
  reference) is a COGNITION/Mind feature, NOT part of this source contract or
  Track D — tracked in its own design note. Named here only so the boundary is
  explicit: this contract delivers the raw forecast faithfully; the expert
  reasoning over it lives in Mind.

---

## APPENDIX — PROMPT FOR THE AEOLUS TEAM (separable; hand this over)

> FORTUNA needs a read-only forecast endpoint from Aeolus. Build:
>
> `GET /v2/forecasts?station={id}&variable={tmax|tmin}&from={YYYY-MM-DD}&to={YYYY-MM-DD}`,
> Bearer-auth, returning `{ "forecasts": [ <envelope>, ... ] }` where each
> envelope is the v2 object specified in §2 of this document. Source the
> fields from your existing tables: `distribution.{mu,sigma,model_version}`
> from `forecast_log`; `skill.{crps,crpss_vs_raw,n_scored,window_days}` from
> `scorecards` (trailing window per station/variable); `resolution.nws_station_id`
> from `station_config.nws_station_id`; `run_at` = the forecast `init_time`
> (NOT the response time). Emit a deterministic ETag over the FORECAST content
> — the identity tuple `(station, target_date, variable, run_at)` plus
> `distribution`, `resolution`, and `brackets` — but EXCLUDING the `skill`
> block and `next_run_at`, so a skill recompute on an unchanged forecast still
> returns 304. Never fabricate a skill number — emit the real `n_scored` even
> when small. Bump the `schema` string to `aeolus.forecast/v3` for any breaking
> field change; never silently alter v2. No secrets or PII in the payload. One
> real captured response becomes FORTUNA's test fixture, so the first response
> you serve is the contract — get the field names and types exactly per §2.
