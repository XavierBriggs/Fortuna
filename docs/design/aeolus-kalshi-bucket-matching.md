# Aeolus ↔ Kalshi bucket-matching contract (v1)

Status: ALIGNED 2026-06-14 (Track-E ↔ Track-A). The seam that makes Aeolus weather
forecasts tradeable on Kalshi's daily temperature buckets. Conforms to
`aeolus-fortuna-source-contract.md` §1 (Aeolus owns the forecast; FORTUNA owns the
market reasoning) and the F6 μ/σ→p backbone (`aeolus_forecast.rs`).

## 1. The impedance + the resolution

Aeolus emits a CUMULATIVE ge-ladder (`ge81..ge94` = `P(high ≥ N)`); Kalshi's daily set
is IN-RANGE 2°-inclusive buckets + two tails. A literal `ge{N}→≥N` 1:1 only hits the
tail, yielding ~0 edges on a forecast that tops at ge94. The resolution: a Kalshi bucket
is a DIFFERENCE of the cumulative ladder, which F6 already computes — `P(high ∈ [lo,hi])
= ge(lo) − ge(hi+1) = bracket_range_prob(lo, hi+1, μ, σ)`. So FORTUNA emits one belief
PER DISCOVERED BUCKET, mapping `Direct` 1:1 onto the tradeable markets. Forecast brackets
MIRROR venue brackets — no overlap, no double-counting, proper per-market sizing.

## 2. The seam type — Track-A → Track-E

Track-A constructs these from the live Kalshi book (one per discovered active market):

```rust
pub struct WeatherBucket {
    /// Kalshi market identity (the raw ticker). Track-E stamps the belief's
    /// event_id = "aeolus:{market_key}", so the edge is event↔market 1:1.
    pub market_key: String,
    pub kind: BucketKind,
}
pub enum BucketKind {
    /// Integer daily high ∈ [lo_f, hi_f] INCLUSIVE (Kalshi "87° to 88°" ⇒ {87,88}).
    InRange   { lo_f: i64, hi_f: i64 },
    /// High ≥ threshold_f (upper tail, "95° or above").
    GreaterEq { threshold_f: i64 },
    /// High ≤ threshold_f (lower tail, "86° or below").
    LessEq    { threshold_f: i64 },
}
```

### Track-A's venue→kind derivation (from the real Kalshi strike fields)

The recorded `KXHIGHNY` day-set (`fixtures/kalshi/…`, 2026-06-13) proves the wire shape:

| ticker | strike_type | floor | cap | subtitle | → BucketKind |
|---|---|---|---|---|---|
| `T87` | less | – | 87 | "86° or below" | `LessEq{86}` |
| `B87.5` | between | 87 | 88 | "87° to 88°" | `InRange{87,88}` |
| `B89.5` | between | 89 | 90 | "89° to 90°" | `InRange{89,90}` |
| `B91.5` | between | 91 | 92 | "91° to 92°" | `InRange{91,92}` |
| `B93.5` | between | 93 | 94 | "93° to 94°" | `InRange{93,94}` |
| `T94` | greater | 94 | – | "95° or above" | `GreaterEq{95}` |

- `between(floor=F, cap=C)` → `InRange{ lo_f: F, hi_f: C }`
- `greater(floor=F)` → `GreaterEq{ threshold_f: F+1 }`  (`>F` = `≥F+1`)
- `less(cap=C)` → `LessEq{ threshold_f: C−1 }`  (`<C` = `≤C−1`)

(Track-A owns this derivation + the `KalshiMarket` DTO extension carrying
`strike_type`/`floor_strike`/`cap_strike`, additive, fixture-proven.)

## 3. Track-E entry point

```rust
pub fn aeolus_bucket_beliefs(fc: &AeolusForecast, buckets: &[WeatherBucket]) -> Vec<BeliefDraft>
```

One propose-only `BeliefDraft` per bucket, ORDER-PRESERVING. `p == p_raw` (no calibration
— downstream, I6). `event_id = "aeolus:{market_key}"`. `horizon = resolution.settles_after`.
`provenance = {model_id:"aeolus", station, variable, target_date, run_at, model_version}`.
`evidence` carries (DATA) the bucket `kind` + bounds, `p_fortuna`, and skill — so F9/ROTA
have the bounds structurally without a new field on the shared `BeliefDraft`.

### The p per kind (F6's pinned helpers — the −0.5 correction lives inside them)

| kind | p |
|---|---|
| `InRange{lo, hi}` | `bracket_range_prob(lo, hi+1, μ, σ)` = `ge(lo) − ge(hi+1)` |
| `GreaterEq{M}` | `bracket_prob_ge(M, μ, σ)` |
| `LessEq{M}` | `bracket_prob_lt(M+1, μ, σ)` = `1 − ge(M+1)` |

## 4. Invariants (the contract's teeth)

1. **Partition ⇒ sums to 1.** For a complete, non-overlapping day-set
   (`≤M | [M+1,M+2] | … | ≥K`) the p's TELESCOPE to 1.0 (within clamp ε):
   `[1−ge87] + [ge87−ge89] + [ge89−ge91] + [ge91−ge93] + [ge93−ge95] + ge95 = 1` ✓.
   **Track-A owns passing the complete active day-set** for the forecast's `target_date`;
   Track-E computes each bucket independently (it does not enforce the partition, but the
   e2e proves the sum on the demo set).
2. **1:1 Direct edges.** Each returned belief maps `Direct` to exactly one market
   (`event_id` ↔ `market_key`) — no overlap, the whole point over the ge-ladder.
3. **Replay-deterministic** (pinned erf, §7); empty `buckets` ⇒ empty `Vec`; never panics.

## 5. F9 reliability (Track-E, per-kind outcome)

`score_bucket_reliability(&AeolusForecast, &[WeatherBucket], realized_f)` scores each bucket
belief by Brier — outcome = `lo ≤ realized ≤ hi` / `realized ≥ M` / `realized ≤ M` per kind;
CRPS stays on the μ/σ fan. (A small extension to F9, taking the same `WeatherBucket[]`.)
F8's ge-ladder beliefs STAY as the reliability/cross-check vehicle — they are not the
tradeable path.

## 6. Ownership

- **Track-A (venue):** the `KalshiMarket` strike-field DTO; station→Kalshi-series map
  (`KNYC`+`tmax` → `KXHIGHNY`, grounded; other cities added only as each NWS↔Kalshi pairing
  is confirmed, never guessed); live bucket discovery → `WeatherBucket[]` (complete active
  day-set); the entry-point call; the `Direct` edges; the `drive()` world-forward wiring.
  RECORDED Kalshi data only.
- **Track-E (cognition):** `WeatherBucket`/`BucketKind` (the seam types), `aeolus_bucket_beliefs`,
  the F9 bucket-outcome extension, and the recorded e2e (sum-to-1 + maps onto the demo set).

## 7. Conventions / notes

- `market_key` = the RAW Kalshi ticker (e.g. `event_id = "aeolus:KXHIGHNY-26JUN13-B87.5"`);
  the edge is `{ market: "KXHIGHNY-26JUN13-B87.5", venue: "kalshi", event_id:
  "aeolus:KXHIGHNY-26JUN13-B87.5", mapping: Direct }` — no re-derivation.
- The recorded June-13 set is `status="determined"` (settled) — STRUCTURALLY COMPLETE, ideal
  for the sum-to-1 e2e; in production Track-A builds buckets from ACTIVE markets only (the open
  day).
- A bucket far in the tail → clamped tiny p (still a valid belief); the partition's tails carry it.
