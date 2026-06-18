# AMENDMENT — TRACK C — perp_event_basis v2 + funding_forecast scoring (slice-3b-v2)

**Hand this to track-C's loop. It is a SPEC amendment, not a new build order to start now.**
Operator-endorsed 2026-06-13. Bus entry: `docs/reviews/GATE-FINDINGS-LATEST.md` → LATEST,
"TRACK C — slice-3b-v2 SPEC LEDGERED". Binding spec: `docs/design/perp-strategies-and-scalar-claims.md`
**§3.3** (basis-v2) + **§2.6** (funding_forecast scoring) + **§5** (sequencing).

---

## What changed

Your endorsed perp amendments are now the BINDING design for the next rung of the
perp_event_basis TRADER and for funding_forecast's acceptance. They are written into the
design doc (§3.3, §2.6). Nothing is built yet — this ledgers the requirements so they are
firm when you reach them.

**RUNG-0 IS DONE AND UNTOUCHED.** The merged, demo-validated median-basis kernel
(`fortuna-cognition::basis`) + the propose-only strategy (`fortuna-runner::perp_event_basis`)
stay exactly as they are. v2 is ADDITIVE and the rung-0 path remains the FALLBACK when v2's
richer inputs are absent, stale, or incoherent. You are not rewriting rung-0; you are adding
a smarter rung on top of it.

## SEQUENCING — do not start yet

§5 recommends v2 **BEHIND the Kalshi demo-flip** in your queue. The demo-flip unblocks live
observability of the already-producing funding_forecast (the operator's "demo mode" goal);
v2 deepens a Sim-stage, propose-only, non-live-capital strategy (I7) whose rung-0 is already
merged, so it gates nothing live. **Do NOT start v2 until the demo-flip lands, unless the
operator explicitly reorders.** (The operator may reorder — watch the bus.) F5–F9 was moved
off you → track E (2026-06-14), so this is your perp queue, not an addition on top of F5–F9.

## The spec (read §3.3 / §2.6 in full before coding — this is the digest)

### §2.6 — funding_forecast scoring (do this FIRST when v2 starts; isolated, no strategy dep)
- **A2b:** the scalar `PredictiveDistribution::Scalar` carries exactly 7 quantiles
  `{0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95}`. Unit-test the produced q-vector pins to these.
- **A2d:** funding_forecast must BEAT baselines on the same resolved windows — above all
  **carry-forward** (the venue funding ESTIMATE projected flat to `next_funding_time`), plus
  last-realized and random-walk. Implement each baseline as a trivial scalar producer over the
  same ticks; score side-by-side via the existing `(belief_id, rule_id)` rows (§1.3). Test that
  the comparison is COMPUTED. If it can't beat carry-forward it stays DATA-ONLY (no promotion) —
  promotion is the operator's call on the measured result, never automatic (I7).

### §3.3 — basis-v2 (the build order; each slice gate-clean + full battery)
1. **§2.6** (above) — first.
2. **A3 + A6:** per-bracket fair prob `q_j = F(cap_j) − F(floor_j)` (open tails `1−F(floor)` /
   `F(cap)`), F = model settlement CDF on anchor **S₀ = the CF Benchmarks reference / BRTI**
   (`FundingObservation.reference_price`, NOT the perp mark — the perp's deviation from it is the
   funding signal), dispersion σ over horizon τ. Rung-0-of-v2 σ = realized perp-mark vol scaled
   by √τ, config-overridable. Bracket-IMPLIED σ is a DIAGNOSTIC only (never the pricing input —
   circular). `bracket_implied_median` is retained, demoted to a health metric (A10). A6 stale
   reference feed (Clock-measured age) → DISABLE that tick.
3. **A9:** ladder no-arb validation (implied cumulative non-decreasing, YES-sum ≈ 1, no crossed
   free-lock). Fail → disable basis trading on that ladder. A genuine lock is mech_structural's
   arb, not yours.
4. **A5:** horizon gating off τ = settlement − now (injected Clock): ≤4h direct / 4–48h
   vol-adjusted (σ∝√τ) / >48h DISABLED.
5. **A4 + A8:** per-bin EV gate replaces the scalar fee-trap:
   `EV_j = q_j − ask_j − fee − slippage − reserve − adverse_j > threshold` (ask = executable YES
   price, not mid; adverse_j = maker adverse-selection penalty). EV is the GO/no-go, NOT a size —
   the leg stays UNSIZED (I6). Multiple bins may clear → multiple unsized maker legs.
6. **A7:** MEASURE perp-vs-bracket informativeness (per-side spreads, depth, quote staleness);
   when the bracket bin is fresher/tighter/deeper, don't assume the perp leads — down-weight or
   veto. Unknown/stale → treat as NOT perp-favorable. **DATA CAVEAT:** per-level quote ages may
   not be on the current `OrderBook`/fixture — if so, record the gap in GAPS and gate on the
   recorder's bracket-vs-perp cycle freshness, treating missing age as stale (never fresh).
7. **A10:** emit full-CDF diagnostics (model `q_j` vector, implied-vs-model CDF, divergence,
   realized band-coverage) as named `MetricSample`s + in the proposal thesis/provenance.

## C / B split (do not build B's half)
A10's DISPLAY (rendering the fan / CDF / basis trail) is **track-B's ROTA §9.2**. You produce
the numbers; B paints them. Do not add ROTA views.

## Discipline (non-negotiable — the verifier gates each slice mutation-proven)
- Additive only; rung-0 untouched and kept as the fallback.
- I6: propose-only, UNSIZED maker legs — the harness sizes. EV/q_j are honest f64 edge claims
  the gates re-check; gaming them games our own risk math.
- I7: Sim stage, no auto-promotion.
- Money/forecast split: `q_j`, σ, EV, τ are f64 forecast-domain (cognition), NEVER money; the
  `Cents` leg pricing and the `PerpPrice` boundary are unchanged. No f64 touches a `Cents` price.
- No `panic!`/`unwrap!`/`expect` anywhere in the path. Every degenerate/stale/incoherent input
  degrades to "propose nothing" (mirror the kernel's `None`), never a crash or a fabricated number.
- Every veto (A5 >48h, A6 stale, A9 no-arb, A7 not-favorable) = propose nothing.
- Each slice: `cargo fmt --check`, clippy `-D warnings`, full test suite, `scripts/run-dst.sh`
  all green; new failure modes → DST corpus; GAPS/ASSUMPTIONS updated; tick the BUILD_PLAN box
  with a one-line note. Ledger your build response in GAPS (do NOT edit the bus).
- Tests from the spec text BEFORE implementation (property tests for the CDF/EV math; DST for
  anything touching τ/Clock).

## When you finish a slice
Push your branch; the verifier gates it on the MERGED tree, mutation-proven, slice-by-slice in
the §3.3 order. A BLOCK naming track-C on the bus preempts your queue.
