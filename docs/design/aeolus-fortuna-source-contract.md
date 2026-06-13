# Aeolus → FORTUNA Source Contract

Wire schema: `aeolus.forecast/v2`.
Contract document: rev 3 (2026-06-13) — reconciled with the Aeolus producer
handoff (`olympus/aeolus/docs/fortuna-integration-handoff.md`, 2026-06-12) +
the Aeolus build design (`aeolus-forecast-v2-api-design.md`). Operator-approved
reconciliation 2026-06-13.
Status: DRAFT for operator approval. Conforms to docs/spec.md v0.9
§5.10/§5.11/§5.12 and the four-layer trust framework in
docs/superpowers/specs/2026-06-12-news-aggregation-design.md §4.4. Supersedes
the minimal v1 envelope (fixtures/aeolus/sample_envelope.json) — migration in
§9. The spec wins on conflict; extensions are flagged in §10.

> **Why the wire schema stays `v2`.** Even the rev-3 producer reconciliation
> does not add/remove/retype an emitted field — it narrows what Aeolus
> currently emits (tmax/tmin only) and corrects FORTUNA-side assumptions (auth
> header, trust framing, `p` handling). Per the co-evolution rule (§8), only a
> change to an emitted field bumps the schema string. The Aeolus build target
> (§2, §3, Appendix) now matches what Aeolus actually ships.

### Changelog (rev 3, reconciled with the Aeolus producer handoff)

The Aeolus team's handoff corrected several rev-2 assumptions. The contract now
matches the producer reality so the handshake works on first contact:

1. **Auth is `x-api-key`, not Bearer (§3.1, §3, Appendix).** Aeolus authenticates
   with an `x-api-key` header (Enterprise-tier key, no daily cap), not
   `Authorization: Bearer`. The F1 substrate is a GENERIC header injector, so
   this is a header-name change, not rework. Operator decision: accept
   `x-api-key` (zero Aeolus work).
2. **Variables are `tmax`/`tmin` ONLY (§2, §3, §3.3).** Aeolus has no predictive
   degree-day model (observed-only), so `hdd`/`cdd` are NOT emitted. The rev-2
   enum widening is WALKED BACK: do not build `hdd`/`cdd` handling yet. The
   "consume everything" intent stands as a PRINCIPLE — DD lands as a gated
   addendum if/when Aeolus builds the forecast model.
3. **Trust framing sobered (§1, §5).** Aeolus's point forecast (μ) is commodity
   — empirically ~4–8% WORSE than the raw GEFS mean; its edge over raw is
   CALIBRATION (σ), and its edge over the MARKET (what FORTUNA trades) is parity
   at best, historically negative. `crpss_vs_raw > 0` means "better-calibrated
   than raw," NOT "beats NOAA" and NOT "beats the market." Admit HIGH on Layer 0
   (operator-owned, authenticated); set a MODEST empirical tier and let Layer 3
   earn it. The contract no longer asserts an unmeasured edge.
4. **`brackets[].p`: clamp-not-reject (§2, §5).** Aeolus pre-clamps to
   `[1e-6, 1−1e-6]` (no 0/1). FORTUNA clamps as defense-in-depth rather than
   hard-rejecting `p∈{0,1}`.
5. **`skill.*` is nullable initially (§2).** `crpss_vs_raw` ships `null` until
   the Aeolus fast-follow scorer lands — treat `null` as "no live skill claim,"
   never gate fast-triggers on it. `n_scored` is the windowed (~30) count.
6. **Latest GEFS run per station-day (§3, §11).** The endpoint returns the
   latest run per `(station, target_date)`, not every historical run; backfill
   retention beyond ~90 days is an open Aeolus-side item.
7. **`next_run_at` IS consumed for release-aware cadence (kept from rev 2 §3.4).**

### Changelog (rev 2, from Track-D implementation review)

The adapter substrate (crates/fortuna-sources) now exists through D4; this
revision made the contract slot into it without hand-waving. Concretely:

1. **Auth is a named substrate prerequisite (§3.1, new).** The shared
   `FetchClient` had no auth path (it set only `User-Agent`, because
   NWS needs no key). A per-source auth header is now a listed track-D
   substrate addition that lands BEFORE `AeolusSource`.
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
6. **`next_run_at` is consumed for release-aware cadence (§2, §3.4).**

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

Aeolus is a CRPS-validated probabilistic temperature-forecast system. It is a
PROPRIETARY, operator-owned signal input. This contract wires it into FORTUNA's
weather domain.

**Be precise about what the edge is (producer's own assessment, handoff §5).**
Aeolus's POINT forecast (μ) is commodity — empirically ~4–8% *worse* than the
raw GEFS ensemble mean. Its edge over raw is CALIBRATION (the σ is well-tuned),
which is real. Its edge over the MARKET — which is what FORTUNA actually trades
against — is parity at best, historically negative. So Aeolus is admitted HIGH
on Layer 0 (it is the operator's own authenticated system) but its EMPIRICAL
trust is modest and unproven against the market; Layer 3 earns the tier (§5).
The contract deliberately does not assert a market edge the loop has not
measured.

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
    "crpss_vs_raw": null,
    "n_scored": 30,
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
| `variable` | enum `tmax` \| `tmin` | the forecast daily extreme. **v2 emits tmax/tmin ONLY** — Aeolus has no predictive degree-day model (handoff §2). `hdd`/`cdd` are a future gated addendum (§3.3), NOT built yet. FORTUNA keys events by variable. Part of the identity tuple. |
| `units` | enum `degF` | guards against a silent °C drift; FORTUNA asserts it. |
| `target_date` | YYYY-MM-DD (station-local) | the forecast day. Part of the identity tuple. |
| `run_at` | UTC ISO8601 | when Aeolus produced this run = the forecast `init_time`, NOT the API response time — POINT-IN-TIME authority for the belief's evidence and freshness (§5.11 received_at honesty). Part of the identity tuple. |
| `next_run_at` | UTC ISO8601 | when the next run is expected. The D9 scheduler CONSUMES this for release-aware cadence (§3.4) — it schedules the next poll around it. Absence is legal (the scheduler falls back to the configured base_interval / release pattern). |
| `valid_until` | UTC ISO8601 | after this, the forecast is stale (the market is resolving); FORTUNA stops trusting it for new triggers (freshness, §5.5 — enforced FORTUNA-side, not in the adapter). |
| `distribution.family` | enum `normal` | the predictive family. v2 supports `normal` only; richer families = a v3 extension (§10). |
| `distribution.mu` | f64, in `units` | predictive mean. THE primary signal. |
| `distribution.sigma` | f64 > 0, in `units` | predictive std-dev. FORTUNA rejects σ ≤ 0 (degenerate). |
| `distribution.model_version` | string | e.g. `sar-semos-v1`; rides into belief provenance; a model change is a visible event. |
| `skill.crps` | f64 ≥ 0, nullable | recent CRPS for this station/variable. TELEMETRY — excluded from the dedup ETag (§3). |
| `skill.crpss_vs_raw` | f64, **nullable** | skill score vs the RAW GEFS ensemble (>0 = better CALIBRATED than raw — NOT "beats NOAA," NOT "beats the market"; see §1/§5). **Ships `null` until the Aeolus fast-follow scorer lands — treat `null` as "no live skill claim," never gate fast-triggers on it.** TELEMETRY. |
| `skill.n_scored` | int ≥ 0, nullable | windowed scored-day count (~30, NOT the example's 11174); a skill claim on n=3 is not a skill claim. TELEMETRY. |
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
| `brackets[].p` | f64 in `[1e-6, 1−1e-6]` | Aeolus's own probability for the bracket — a CROSS-CHECK against FORTUNA's μ/σ-derived p; a large divergence (threshold set ABOVE the ~1e-12 erf delta, §7) is a data-quality alarm, not silently averaged. Aeolus PRE-CLAMPS to `[1e-6, 1−1e-6]` (no 0/1); FORTUNA **clamps-not-rejects** as defense-in-depth (a stray 0/1 is clamped, not a hard parse failure). |

## 3. The endpoint (the Aeolus-team build target)

Aeolus exposes ONE read-only endpoint. It is a pull (FORTUNA polls); no
push/webhook in v2.

```
GET /v2/forecasts?station={id}&variable={tmax|tmin}&from={date}&to={date}
Auth: x-api-key header (Aeolus Enterprise-tier key, no daily cap; FORTUNA holds
      it in env only — AEOLUS_API_TOKEN — never in repo/config/logs).
Host: pinned in FORTUNA's source_registry row; https-only.
Response: 200 → { "forecasts": [ <v2 envelope>, ... ] }   (latest GEFS run per
                (station, target_date) in range — NOT every historical run)
          304 → unchanged (FORTUNA sends If-None-Match; Aeolus returns a
                forecast-scoped ETag — steady-state polling is near-free)
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

### 3.1 FORTUNA-side substrate prerequisite — auth (the F1 task)

The shared `FetchClient` (crates/fortuna-sources, D2) currently supports
host-pinning, https-only, conditional GET, redirect re-validation, and the
per-host politeness budget — but it sets only a `User-Agent` header and has NO
auth-header path (NWS, the first source, needs no key). Aeolus authenticates
with **`x-api-key`** (rev 3). Therefore, BEFORE `AeolusSource` can be built, the
substrate gains a small, GENERIC per-source auth-header capability:

- a per-source optional header injector (an arbitrary header allowlist) — for
  Aeolus that is `x-api-key: <token>`; the design is header-name-agnostic so
  Bearer or any future scheme drops in with no rework;
- the token sourced from an env var named in config (`AEOLUS_API_TOKEN`) —
  never in config, repo, logs, or audit payloads (house rule + §5.11);
- redacted everywhere it could surface (error strings, debug, telemetry) —
  covered by a redaction test.

This is the **F1** task — a track-D substrate item, not Aeolus's work, listed so
the dependency is explicit and ordered (F1 blocks F3, the `AeolusSource`).

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

### 3.3 Variable classes — principle vs. producer reality (rev 3)

The operator decision is "consume everything Aeolus produces." The producer
reality (handoff §2) is: **Aeolus emits `tmax` and `tmin` only.** It has no
predictive degree-day model — degree-days are observed-only — so `hdd`/`cdd`
are NOT on the wire, and FORTUNA must **not** build enum handling for them yet
(it would be dead code modelling a variable that does not exist).

So the standing rule reconciles to: consume every variable Aeolus ACTUALLY
emits (today tmax/tmin); each NEW class arrives as a small gated addendum, not a
silent widening. When Aeolus builds a degree-day FORECAST model, that addendum
must spell out two pieces that are not just a new enum value:

- **Distribution.** A degree-day is a derived, accumulated quantity; its
  predictive distribution is generally NOT a single-day `normal` — the addendum
  declares its own `distribution.family` or supplies per-day components FORTUNA
  composes.
- **Resolution.** Degree-days settle against an ACCUMULATION of official obs
  over the period, so `resolution.authority` gains accumulation variants and the
  §3.2 grader must expose the accumulated series, not just the daily extreme.

Until then: tmax/tmin, single-day `normal`, daily-extreme resolution.

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

NEW FORTUNA-side work (co-evolved with this contract, gated — the F6 task):
extend the strict `AeolusEnvelope`/parse in reconciliation.rs from v1
(station/target_date/run_at/brackets[{event_hint,p}]) to v2 (the full §2 shape),
with the μ/σ → threshold-probability helper (§7 determinism), σ>0 / units==degF
validation, nullable `skill.*`, and `brackets[].p` CLAMP-not-reject. v1 fixtures
migrate per §9.

## 5. Trust-framework fit (the closed loop — this is the elegant part)

- **Layer 0 (admission):** Aeolus is an operator-owned, authenticated source —
  admitted HIGH on AUTHENTICITY (the operator's own system; the `x-api-key`
  proves it; ToS = N/A internal). But Layer 0 authenticity is NOT empirical
  edge: the INITIAL trust tier is MODEST, not maxed (handoff §5 — μ is
  commodity, the market edge is unproven). Let Layer 3 earn the tier. The
  dossier (docs/research/sources/aeolus/dossier.md) must state the measured
  reality, not assert an edge the loop has not yet seen.
- **Layer 1 (structural) — TWO complementary checks in two places (rev 2):**
  - **1a, scheduler (D9):** the generic `StructuralValidator` —
    future-dated reject (vs `run_at`), stale-republication flag, per-tick
    volume envelope. Aeolus gets this like every source.
  - **1b, cognition (reconciliation.rs):** the strict schema/semantic parse —
    σ≤0, units≠degF, unknown fields all refuse; `brackets[].p` is CLAMPED to
    `[1e-6, 1−1e-6]` (not hard-rejected, rev 3). This is SCHEMA conformance,
    which §4.4 names as part of Layer 1; it is NOT the same thing as 1a.
- **Layer 2 (corroboration):** for FAST triggers, weather corroborates Aeolus
  against the raw-NWS / observed source. Aeolus does NOT auto-clear the
  fast-trigger tier on a self-reported skill claim — `crpss_vs_raw` only means
  "better calibrated than raw," it does not prove a market edge (§1), and it
  ships `null` at first anyway. Treat NWS as a genuine corroboration input, not
  a mere divergence alarm, until Layer 3 has measured Aeolus.
- **Layer 3 (empirical):** THE LOOP — and the ONLY place Aeolus's real value is
  established. Aeolus SELF-REPORTS skill (`skill.*`, possibly `null`); FORTUNA
  INDEPENDENTLY re-scores every Aeolus-sourced belief by Brier/CLV at settlement
  against the declared resolution source (the §3.2 observed grader). Trust
  attribution compares FORTUNA's MEASURED skill to Aeolus's self-report —
  agreement reinforces, a gap (claimed-but-not-observed) is a flagged anomaly.
  Honest expectation (handoff §5): Aeolus converges to a MODEST Layer-3
  contribution until it ingests a better input (ECMWF) or proves out on a
  curated station subset. Self-graded caution (V4): the resolution authority is
  NWS, NOT Aeolus — beliefs are graded by an independent source, not the
  forecaster. Clean.

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

## 11. Resolved decisions and remaining open questions

Resolved by operator, 2026-06-13 (rev 3 reconciliation with the producer):

- **Auth — `x-api-key`.** Accept the Aeolus default header (zero Aeolus work);
  the F1 substrate is a generic header injector so Bearer remains a drop-in if
  ever needed (§3.1).
- **Variables — tmax/tmin only now; consume-everything is the principle.**
  Aeolus emits no degree-days (no DD forecast model); FORTUNA does NOT build
  hdd/cdd handling until a gated addendum lands (§3.3).
- **Trust — modest on admission, Layer-3-earned.** Admit HIGH on authenticity
  (Layer 0) but a MODEST empirical tier; the dossier states measured reality,
  not an unproven edge (§1, §5).
- **`crpss_vs_raw: null` handling — confirmed.** Null = "no live skill claim";
  fast-triggers do NOT depend on it (§2, §5 Layer 2).
- **`brackets[].p` — clamp-not-reject** to `[1e-6, 1−1e-6]` (§2, §5 Layer 1b).
- **Backfill — date-range query assumed; latest run per station-day.** The
  endpoint returns the latest GEFS run per `(station, target_date)` over
  `from`/`to`; FORTUNA builds against the date-range query for backtest/
  calibration warm-start.
- **Smart, release-aware cadence (§3.4).** The scheduler consumes `next_run_at`
  + a per-source release pattern (GEFS for Aeolus, the macro calendar for
  BLS/Fed/FRED), with triggers/hooks able to fire a poll early. Built into D9.
- **Bracket cross-check divergence threshold** — set ABOVE the ~1e-12
  Aeolus-vs-pinned-erf delta (§7), not at zero.

Remaining open (genuinely undecided; none block the adapter):

- Aeolus's historical RETENTION horizon beyond ~90 days — Aeolus-side decision.
- Degree-day distribution/resolution specifics (§3.3) — settled with the Aeolus
  team if/when Aeolus builds a DD forecast model and the hdd/cdd class is wired.
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
> (Rev 3: this now matches what Aeolus has already specified in its handoff —
> kept as the confirmed agreement, not a fresh ask.)
>
> `GET /v2/forecasts?station={id}&variable={tmax|tmin}&from={YYYY-MM-DD}&to={YYYY-MM-DD}`,
> `x-api-key` auth (Aeolus's default), returning `{ "forecasts": [ <envelope>,
> ... ] }` (latest run per station-day) where each envelope is the v2 object
> specified in §2. Source the fields from your existing tables:
> `distribution.{mu,sigma,model_version}` from `forecast_log`;
> `skill.{crps,crpss_vs_raw,n_scored,window_days}` from `scorecards` (trailing
> window per station/variable; `crpss_vs_raw` MAY be `null` until the scorer
> lands); `resolution.nws_station_id` from `station_config.nws_station_id`;
> `run_at` = the forecast `init_time` (NOT the response time). Emit a
> deterministic ETag over the FORECAST content
> — the identity tuple `(station, target_date, variable, run_at)` plus
> `distribution`, `resolution`, and `brackets` — but EXCLUDING the `skill`
> block and `next_run_at`, so a skill recompute on an unchanged forecast still
> returns 304. Never fabricate a skill number — emit the real `n_scored` even
> when small. Bump the `schema` string to `aeolus.forecast/v3` for any breaking
> field change; never silently alter v2. No secrets or PII in the payload. One
> real captured response becomes FORTUNA's test fixture, so the first response
> you serve is the contract — get the field names and types exactly per §2.
