# Signal contract design note — what a forecast vendor publishes, what FORTUNA subscribes to

Status: DESIGN THINKING ONLY (operator-requested 2026-06-10: "best schema and
contract for signals coming in… what would the aeolus subscribe to"). No build
is authorized by this note. Spec 5.11 governs; this note proposes the layer the
spec leaves open and the changes go through a spec touch-up if adopted.

## 1. What already exists (and is right)

Three layers are built and tested today:

| layer | type | contract |
|---|---|---|
| Transport | `Source` trait (fortuna-cognition/src/signals.rs) | poll/push adapter -> `RawSignal`; deliberately dumb |
| Normalization | `SignalEnvelope` | `{signal_id, source, kind, received_at, payload, content_hash}`; append-only store; dedup on (source, content_hash); `received_at` is the point-in-time authority |
| Per-source payload | `AeolusEnvelope` (reconciliation.rs) | strict `deny_unknown_fields`: `{station, target_date, run_at, brackets[{event_hint, p}]}` -> zero-capital `BeliefDraft`s |

The generic plumbing needs nothing: opaque payload + provenance + dedup +
point-in-time + data-not-instructions is exactly what spec 5.11 demands, and
the source registry (trust tiers, domain tags, per-source belief attribution)
already carries the trust model.

**The gap is one level down:** `payload` is opaque, so every new probabilistic
vendor needs a bespoke typed contract + mapper (a second AeolusEnvelope, a
third…). The thing worth designing once is the reusable CLAIM contract — the
shape a calibrated-forecast producer emits so that onboarding vendor N+1 is
registry config, not new Rust.

## 2. Proposal: `prob_claims/v1` payload contract

One versioned payload `kind` that any probabilistic vendor (Aeolus is instance
zero) can emit inside the existing envelope:

```jsonc
{
  "schema": "prob_claims/v1",          // strict per major version (deny unknown fields)
  "producer": "aeolus",                 // must match a source_registry row
  "producer_version": "sar-semos-2026-05", // model/version string, opaque to us
  "run_id": "01JXJ...",                 // producer's run identity; dedup + supersede key
  "issued_at": "2026-06-11T10:00:00Z",  // producer clock (advisory; received_at stays authoritative)
  "claims": [
    {
      "event_key": "wx.station-day-tmax:KNYC:2026-06-12",  // namespaced subject (see §3)
      "horizon": "2026-06-13T04:00:00Z", // when the claim resolves / stops being about the future
      "valid_until": "2026-06-11T22:00:00Z", // producer-declared staleness bound (freshness policy may tighten, never loosen)
      "outcome": {                       // exactly ONE of:
        "binary":      { "p": 0.55 },
        "categorical": { "bins": [ {"label": "t60", "p": 0.18}, {"label": "t65", "p": 0.55}, {"label": "t70", "p": 0.27} ] },
        "scalar":      { "quantiles": [ {"q": 0.1, "v": 61.2}, {"q": 0.5, "v": 65.0}, {"q": 0.9, "v": 69.1} ], "unit": "degF" }
      },
      "market_hints": [ {"venue": "kalshi", "ticker": "HIGHNY-26JUN12-T65"} ]  // ADVISORY ONLY
    }
  ],
  "calibration_note": { "resolved_n": 11174, "window": "rolling-90d" }  // producer-claimed; never trusted, only compared
}
```

Design decisions baked in:

- **Categorical generalizes brackets; scalar covers the next class.** Aeolus's
  temperature brackets are a categorical claim. A funding-rate or CPI-print
  forecaster emits scalar quantiles. Binary is the degenerate case the
  comparator consumes directly. One contract, three outcome shapes, no
  vendor-specific Rust.
- **`event_key` binds to OUR event model, by namespace.** The producer asserts
  a subject in a registered namespace ("wx.station-day-tmax"); the market-back
  matcher (spec 5.12) owns the mapping to venue tickers. `market_hints` exist
  because vendors often know the ticker — but they are hints into matching,
  never bindings (a vendor must not be able to point a belief at the wrong
  market).
- **Supersede-not-mutate.** (producer, event_key, horizon) identifies a claim
  lineage; a newer `run_id` supersedes older claims for context assembly and
  the comparator, while the store stays append-only (replays see what was
  current at trigger time). This is the freshness policy (spec 5.5) applied at
  the signal layer, and it is why `run_id` is required.
- **Producer-claimed calibration is data, not authority.** `calibration_note`
  is recorded and compared against OUR per-source scoring (Brier/CLV by belief
  attribution); divergence between a vendor's self-report and our measurement
  is itself a trust-tier input. Trust tiers and quality feeding sizing remain
  100% FORTUNA-computed.
- **Strictness matches Aeolus precedent:** unknown fields reject per major
  version; evolution is additive within a version only for OPTIONAL fields;
  anything load-bearing bumps the major (`prob_claims/v2`).

## 3. The subscription model ("what would the Aeolus subscribe to")

Two directions, deliberately asymmetric:

**FORTUNA subscribes to producers** — a subscription is a source_registry row,
not code: `{source_id, transport, kinds: ["prob_claims/v1"], namespaces:
["wx.*"], trust_tier, max_staleness}`. The trigger engine's existing "new
Aeolus run" rule generalizes to "new claim run in subscribed namespace";
aeolus_eval becomes "the wx.* claim scorer" with zero contract changes when a
second weather vendor appears. Onboarding vendor N+1 = one registry row + one
transport credential. (The "afternoon of work" rule from 5.11 becomes "an
hour".)

**Producers subscribe to FORTUNA's scorecards** — the reverse feed, and the
reason this contract is worth getting right: every claim FORTUNA ingests gets
scored against resolution (Brier vs market-implied baseline, CLV vs benchmark
snapshot — the aeolus_eval machinery, which is already vendor-agnostic in
spirit). Exporting `{run_id, event_key, your_p, market_p_at_receipt, resolved,
brier, clv}` per claim is the feedback product a forecast vendor wants and
cannot compute alone (they lack the market legs). For the Aeolus-as-API pivot
evaluation this is the demo: "publish claims, receive calibration-vs-market
scorecards." v2 scope; named now so the schema keeps the fields it needs
(run_id round-trips).

**Transports** (all behind `Source`, unchanged): file drop (today's
fixtures/aeolus path), polling REST, webhook push. Remote push gets a
per-source shared-secret signature header when it arrives — which
authenticates the SENDER and changes nothing about trust: payloads stay
untrusted data (5.11 data-not-instructions), the blast radius stays bounded by
I6 + gates.

## 4. What this deliberately does not do

- No new invariant surface: claims produce zero-capital beliefs unless a
  strategy proposes and the gates pass — identical to today's Aeolus path.
- No streaming/bus transport, no per-claim acks, no vendor SLAs — pull/push
  batch envelopes are sufficient at our trigger cadence.
- No silent migration: `AeolusEnvelope` remains the working contract until the
  Aeolus exporter speaks `prob_claims/v1`; that switch is a contract
  negotiation with a recorded export on both sides (GAPS Aeolus item),
  never an adapt-in-place.

## 5. If adopted (sequencing sketch, NOT authorized)

1. Spec touch-up (5.11 gains the claim-contract subsection; v0.9 bump).
2. `prob_claims/v1` types + mapper (categorical + binary first; scalar with
   the first scalar consumer), property tests over outcome-shape validation
   (probs sum, quantile monotonicity), strict-parse tests from spec text.
3. Registry-driven namespace subscription in the trigger engine.
4. Aeolus exporter speaks v1 (operator-side; contract negotiation).
5. Scorecard export (v2, after a second consumer or the API-product decision).
