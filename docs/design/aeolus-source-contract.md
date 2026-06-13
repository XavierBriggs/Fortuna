# Aeolus → FORTUNA Source Contract (v2)

Status: DRAFT for operator approval, 2026-06-13. Conforms to docs/spec.md
v0.9 §5.10/§5.11/§5.12 and the four-layer trust framework in
docs/superpowers/specs/2026-06-12-news-aggregation-design.md §4.4. Supersedes
the minimal v1 envelope (fixtures/aeolus/sample_envelope.json) — migration in
§9. The spec wins on conflict; extensions are flagged in §10.

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

### Field semantics (every field is load-bearing or it is cut)

| field | type | meaning / FORTUNA use |
|---|---|---|
| `schema` | string const `aeolus.forecast/v2` | version pin; FORTUNA rejects any other value (forces lockstep upgrades). |
| `station` | string | Aeolus station id (e.g. KNYC). |
| `nws_station_id` | string | the OFFICIAL station the bracket resolves against; usually == station, declared explicitly so FORTUNA never infers it. |
| `variable` | enum `tmax` \| `tmin` | which daily extreme (Aeolus mirrors both). FORTUNA keys events by it. |
| `units` | enum `degF` | guards against a silent °C drift; FORTUNA asserts it. |
| `target_date` | YYYY-MM-DD (station-local) | the forecast day. |
| `run_at` | UTC ISO8601 | when Aeolus produced this run — POINT-IN-TIME authority for the belief's evidence and freshness (§5.11 received_at honesty). |
| `next_run_at` | UTC ISO8601 | when the next run is expected — the source scheduler's re-poll hint. |
| `valid_until` | UTC ISO8601 | after this, the forecast is stale (the market is resolving); FORTUNA stops trusting it for new triggers. |
| `distribution.family` | enum `normal` | the predictive family. v2 supports `normal` only; richer families = a v3 extension (§10). |
| `distribution.mu` | f64, in `units` | predictive mean. THE primary signal. |
| `distribution.sigma` | f64 > 0, in `units` | predictive std-dev. FORTUNA rejects σ ≤ 0 (degenerate). |
| `distribution.model_version` | string | e.g. `sar-semos-v1`; rides into belief provenance; a model change is a visible event. |
| `skill.crps` | f64 ≥ 0 | recent CRPS for this station/variable. |
| `skill.crpss_vs_raw` | f64 | skill score vs raw NOAA (>0 = beats raw). FORTUNA's Layer-3 trust weighting reads this. |
| `skill.n_scored` | int ≥ 0 | sample size behind the skill numbers (a skill claim on n=3 is not a skill claim). |
| `skill.window_days` | int | the trailing window for the skill stats. |
| `skill.as_of` | UTC ISO8601 | when the skill stats were computed. |
| `resolution.authority` | enum `nws_observed_high` \| `nws_observed_low` | how the event settles — REQUIRED (§5.12: every event declares a resolution source). |
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
GET /v2/forecasts?station={id}&variable={tmax|tmin}&from={date}&to={date}
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
4. **Deterministic ETag** over the envelope content so 304s work and FORTUNA's
   dedup (§4) is stable.
5. **No secrets, no PII** in the payload. Station ids and forecasts only.
6. **Stable schema string** — bump to `aeolus.forecast/v3` for ANY breaking
   field change; never silently add/remove fields under v2 (FORTUNA's strict
   parser will hard-fail, which is the intended tripwire, but a version bump is
   the courteous form).

## 4. The FORTUNA side — ingestion path

`AeolusSource` (a track-D `fortuna-sources` adapter) is a thin wrapper:

```
AeolusSource::fetch() -> Vec<RawSignal>
  GET the endpoint (FetchClient: host-pinned, https-only, conditional GET,
      politeness budget) -> for each v2 envelope ->
  RawSignal { kind: "aeolus.forecast", payload: <the envelope JSON>,
              received_at: <clock now> }
```

Then the EXISTING cognition pipeline takes over, unchanged:
- `normalize_and_dedup`: source_registry check (fail-closed allowlist) -> the
  SignalEnvelope `{source, type, received_at, payload, content_hash}` -> dedup
  on (source, content_hash). The content_hash makes a re-emitted identical
  forecast a no-op; a revised forecast (new μ/σ) is a new signal.
- The signal lands in the append-only `signals` table.
- World-forward discovery (§5.12) synthesizes/【matches】 the weather event from
  the signal, declaring the resolution source from `resolution.*`.
- Synthesis forms the belief: from μ/σ it computes `p_raw` for the market's
  threshold; FORTUNA's calibration layer (§5.10) produces `p`; the
  `brackets[].p` cross-check rides in evidence.
- The mapped `BeliefDraft` keeps the v1 namespace: `event_id =
  aeolus:{event_hint}`, with `evidence` carrying the Aeolus ref + skill, and
  harness-stamped `provenance` (model_id, station, run_at, model_version).

NEW FORTUNA-side work (co-evolved with this contract, gated): extend the
strict `AeolusEnvelope`/parse in reconciliation.rs from v1 (station/target_date/
run_at/brackets[{event_hint,p}]) to v2 (the full §2 shape), with the μ/σ →
threshold-probability helper and σ>0 / p∈(0,1) / units==degF validation. v1
fixtures migrate per §9.

## 5. Trust-framework fit (the closed loop — this is the elegant part)

- **Layer 0 (admission):** Aeolus is an operator-owned, authenticated source —
  admitted at HIGH initial tier with its dossier (authenticity = the operator's
  own system, the bearer token proves it, ToS = N/A internal).
- **Layer 1 (structural):** the strict v2 parse IS the structural validator —
  σ≤0, units≠degF, p∈{0,1}, unknown fields all refuse-and-quarantine.
- **Layer 2 (corroboration):** for FAST triggers, weather can corroborate Aeolus
  against raw NWS (a separate registry source) — but since Aeolus's `crpss_vs_raw`
  already proves it beats raw, Aeolus alone clears the fast-trigger tier; NWS is
  the divergence alarm, not a gate.
- **Layer 3 (empirical):** THE LOOP. Aeolus SELF-REPORTS its skill (`skill.*`);
  FORTUNA INDEPENDENTLY re-scores every Aeolus-sourced belief by Brier/CLV at
  settlement against the declared resolution source. FORTUNA's trust attribution
  compares its own measured skill to Aeolus's self-reported skill — agreement
  reinforces trust; a gap (Aeolus claims skill FORTUNA doesn't observe) is a
  flagged anomaly. Self-graded caution (V4): the resolution authority is NWS,
  NOT Aeolus, so these beliefs are NOT self-graded — the grader is independent
  of the forecaster. Clean.

## 6. Security / abuse posture

Trusted, operator-owned source — but still DATA, never instructions (§5.11):
- The forecast payload reaches the model only inside the context assembler's
  delimited data blocks; I6 (propose-only) and I1 (gates) bound it regardless.
- Host pinned in source_registry; https-only; bearer token env-only.
- The numbers are validated structurally (§2) before they can move capital;
  a poisoned/buggy Aeolus run (σ absurd, μ off-planet) fails Layer-1 validation
  or is caught by the bracket-vs-distribution cross-check, not silently traded.

## 7. Determinism / money discipline

- μ/σ are f64 (forecast space — probabilities are f64 in cognition, allowed).
  The moment a probability becomes a SIZE, it crosses into integer cents in the
  harness — Aeolus never touches money.
- `threshold_f` is integer °F; the μ/σ→p computation is pure and replayable from
  the stored signal (the raw envelope is persisted, so a belief replays
  byte-identically).
- received_at from the injected Clock; no wall-time in the ingestion path.

## 8. Co-evolution discipline

The contract version (`schema` string) and FORTUNA's strict parser change
TOGETHER, in one gated change, or not at all. The `deny_unknown_fields`
strictness is the feature: it makes a unilateral Aeolus change fail loud in
FORTUNA's gate rather than silently corrupt beliefs. Process: a contract change
is a versioned design-doc edit -> FORTUNA parser update + fixture re-record ->
gated -> Aeolus deploys the matching version. Neither side ships a breaking
change alone.

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
   computation is FORTUNA-side and replayable.
2. Source self-reporting skill — new; cross-checked by FORTUNA's own scoring,
   never trusted blind (Layer 3).
3. `normal` family only in v2 — richer predictive families (mixtures, skew) are
   a v3 extension when a station needs it.

## 11. Open questions (deferred; not blocking the adapter)

- Whether FORTUNA should also consume Aeolus's degree_days for the
  energy/degree-day market families (a second variable class) — Phase B.
- Backfill: should the endpoint serve historical runs (for FORTUNA's
  backtest/calibration warm-start) or only the latest? v2 assumes a date range;
  Aeolus decides retention.
- Rate/cadence: Aeolus runs on the GEFS cadence (~6h); FORTUNA polls at
  `next_run_at`. Confirm Aeolus's real publish cadence per station.

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
> (NOT the response time). Emit a deterministic ETag over envelope content so
> conditional GETs return 304 when unchanged. Never fabricate a skill number —
> emit the real `n_scored` even when small. Bump the `schema` string to
> `aeolus.forecast/v3` for any breaking field change; never silently alter v2.
> No secrets or PII in the payload. One real captured response becomes
> FORTUNA's test fixture, so the first response you serve is the contract — get
> the field names and types exactly per §2.
