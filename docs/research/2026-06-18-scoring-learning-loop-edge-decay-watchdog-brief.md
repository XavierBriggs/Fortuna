# Research Brief — Scoring System, Learning Loop & the Edge Decay Watchdog

**Created:** 2026-06-18 · **For:** a future deep-research + design session (hand this whole file to a fresh session).
**Authority:** `docs/spec.md` (v0.9) > `CLAUDE.md` > this. Constitution invariants I1–I7 are absolute.
**Methodology:** use the `deep-research-protocol` skill — multi-perspective, evidence-graded (cite `file:line`), adversarial red-team, calibrated confidence. **Verify against the code; do NOT inherit conclusions** (including this brief's — re-derive from source).

---

## 0. Why this exists

FORTUNA's entire legitimacy claim is: *it forecasts event probabilities better than the market and proves it through process gates (calibration + CLV), not PnL.* The scoring harness is that proof machinery — "a probability that hasn't been scored is a guess with a decimal point." A 2026-06-18 parallel-read assessment (7 agents, grounded) found the **sizing-safety half is rigorous** but the **proof/diagnostic half has real gaps**. This brief asks a future session to (A) deeply map the whole scoring system, (B) map the closed learning loop, and (C) design the **Edge Decay Watchdog** — the component that defends the edge against decay over time. The name is **config-driven** (see §C.0).

This is NOT an implementation order. It is a research + decision-grade design task. Produce a design doc + prioritized gap analysis; implementation is a later, separately-approved step (the operator chose "finish the close-the-loop work first, decide on scoring hardening after").

---

## 1. Grounded starting state (verify, then go deeper)

Reference syllabus: "Probabilistic Forecasting & Scoring Rules" (Module 3) — proper scores, calibration vs resolution (Murphy), sharpness-subject-to-calibration (Gneiting), CRPS/PIT/EMOS, CLV, Kelly↔calibration. Treat each section as a checklist against the code.

**What is IMPLEMENTED (confirmed, with anchors):**
- Proper scores: `BrierRule` (binary) + `CrpsPinballRule` (scalar, discretized CRPS via pinball) — `crates/fortuna-cognition/src/scoring.rs:238-379`. Belief-quality scoring (Brier/CRPS) is strictly walled off from trade-outcome scoring (PnL → `trade_scores`); **no win-rate/accuracy ever grades the belief model** (the syllabus's #1 warning — passed).
- Calibration: Platt (`calibration.rs:81-141`), isotonic PAV (`:190-252`), shrinkage-toward-market (`:260-263`, `w=min(n/50,1)`), extremization (`:270-276`, `k=1.0` default = identity), `calibration_quality` (`:347-363`), 10-bucket calibration curve (`beliefs.rs:223-255`).
- **Kelly ↔ calibration link is fully implemented** (`cycle.rs:196-202` `haircut_kelly_fraction = base × quality`, NaN→0 fail-closed; cold scope ⇒ Mind call skipped ⇒ zero size, `synthesis.rs:168-171`; "calibrate before you size" enforced). This is the load-bearing safety chain and it holds.
- Forward/OOS: scored on resolved outcomes only; `FULL_AUTONOMY_N=50` gate (`calibration.rs:29`); I7 operator-only promotion (`i7_promotion_gates.rs`); `propose_promotion` requires Brier-beats-baselines + positive CLV (`review.rs:166-182`).
- Per-producer scoring: Track-E personas via `resolved_persona_stats` (`repos.rs:1305-1337`); **Phase-C tasks D3/D4 extend this** (meteorologist scored parallel to Aeolus).
- CLV machinery: schema `beliefs.clv_bps` (`migrations/20260609000001_initial.sql:73`), computation `events.rs:314-339` (bps vs latest liquid pre-benchmark snapshot), set-once persistence (`repos.rs:1214-1242`), shadow-model use (`shadow.rs:135-136`).

**Confirmed GAPS (the research should validate, deepen, and prioritize):**
1. **CLV is never computed in the LIVE path (biggest).** `resolve_and_score_weather_beliefs` passes `None` for `clv_bps` (`daemon.rs:~4726`); `resolve_and_score_funding_beliefs` never calls `clv_bps()`; the `price_snapshots` table (`migrations:204-219`) is **never populated** (no snapshot collection scheduled); `events.benchmark_at` is unused. → The *fast, market-relative* gate (a spec GO criterion, spec 5.5) is dark in the demo.
2. **Murphy decomposition not implemented.** Only a single Brier mean + an ad-hoc 2-factor quality number. No Reliability − Resolution + Uncertainty split → cannot prove edge comes from *resolution* (beating base rate) vs noise; cannot detect the base-rate-forecaster trap. Gneiting "sharpness subject to calibration" not explicitly computed.
3. **PIT histogram missing.** The curve bins by claimed-p, not by `F(x_realized)` rank → cannot diagnose ensemble over/under-dispersion for the scalar (weather/funding) forecasts. (EMOS is absent **by design** — it lives in the separate Aeolus system; FORTUNA consumes Aeolus quantiles as immutable beliefs.)
4. **Reliability diagram not surfaced.** Data exists (`CalibrationBucket`) but isn't `Serialize`; no ROTA endpoint exposes the buckets; only aggregate Brier/CLV are returned; no confidence bands / thin-bin handling. (Rendering is the parallel UI-overhaul track's contract — coordinate with `docs/superpowers/specs/2026-06-18-operator-ui-overhaul-design.md`.)
5. **Log score + categorical Brier** not implemented (extensible `impl ScoringRule`; lower demo value).
6. **Per-source attribution gap:** Aeolus stamps `model_id` but not `persona_id`, so Aeolus-vs-synthesis can blur in one `ScopeKey` during weekly review (`daemon.rs:~5062-5071`).

---

## 2. Research Track A — Map the full scoring system

Deliverable: a precise, code-grounded map of every scoring path end-to-end, plus a Module-3 conformance scorecard.

Questions:
- For EACH proper score (Brier, CRPS-pinball, and any log/categorical): where computed, where persisted (`beliefs.brier`, `scalar_beliefs` → `belief_scores`), where consumed (calibration, sizing, promotion). Is each *strictly* proper as implemented? Any boundary/clamp that breaks properness?
- Trace a single belief's full scoring lifecycle: draft → persist → resolve (outcome set) → score (Brier/CRPS) → CLV → calibration fit → sizing haircut → promotion eligibility. Where is each step, and where does the chain currently break (esp. CLV)?
- Murphy decomposition: design how to compute Reliability/Resolution/Uncertainty from the existing `CalibrationBucket` data; what extra data (base rate per scope) is needed; how to surface it.
- PIT: design the histogram for the scalar quantile beliefs (funding/weather) — `u = F(x_realized)` from the persisted quantile fan; uniformity test; how to store/surface.
- Anti-pattern audit (adversarial): grep the whole tree for any place a non-proper metric (accuracy/win-rate/MAE-on-probabilities) could leak into belief grading or model selection. Confirm the belief/trade scoring wall is airtight.

---

## 3. Research Track B — Map the learning loop

The loop: **signal → belief (p) → proper score (vs outcome) + CLV (vs market) → calibration fit (Platt/isotonic, forward, n≥50) → sizing haircut (Kelly × quality) → resolve → re-score → re-calibrate (versioned) → promote/demote (I7, operator).**

Questions:
- Where does each arrow live in code? Which arrows are CLOSED (Phase C wired: belief persist, fill, settlement→PnL, calibration persist B1, calibration→synthesis B3, per-producer D4) vs still OPEN (CLV-live, demotion, automated re-fit cadence)?
- Feedback dynamics: how fast does the loop close per category (weather daily, funding 8h, world-forward)? What's the data-volume-to-trust latency (Brier needs many; CLV is faster — quantify)?
- Versioning: `calibration_params` is versioned (B1). How does a new calibration version propagate to live sizing (B3 refresh)? Is there a stale-params risk window?
- Out-of-sample integrity: prove (or disprove) that no in-sample calibration can leak. Where could the forward-only discipline silently break?

---

## 4. Research Track C — Design the **Edge Decay Watchdog**

### C.0 — Name is CONFIG (operator directive)
The component's name/identity is a **configuration value**, not a hardcoded literal (FORTUNA decoupling rule: identities/labels are data). Design it so the display name + the watchdog's scope identity come from config, e.g.:
```toml
[edge_decay_watchdog]              # the SECTION key can stay generic
name = "Edge Decay Watchdog"       # operator-facing label — CONFIG, not a literal in code
enabled = false                    # opt-in, default off (byte-identical when absent)
# ... thresholds below
```
No `if name == "edge decay watchdog"` branches; the spine references it by role, never by literal name. The decoupling guard (`i_decoupling_spine.rs`) must stay green.

### C.1 — What it is
The **reverse of the I7 promotion gate**: a continuous monitor that re-validates each live/paper strategy's edge is still **live** (CLV trend) and **calibrated** (Brier/reliability/PIT trend), and *defends* by triggering re-calibration, demotion (reduce/zero sizing), shadow-swap, or an operator alert when **decay** is detected. Two distinct decays to defend against:
1. **Calibration decay / drift** — the model's probability calibration degrades as the world shifts (distribution shift). Detect via reliability-diagram drift / rising Brier-reliability term / PIT going non-uniform over a rolling window.
2. **Edge decay / alpha erosion** — the tradeable edge erodes as the market adapts or competition arrives (an edge half-life). Detect via CLV trend going flat/negative over a rolling window even while calibration holds.

### C.2 — Research questions
- Literature (deep-research): concept-drift detection (ADWIN, DDM, Page-Hinkley, CUSUM), alpha-decay / strategy-half-life estimation, online calibration monitoring, change-point detection. Which transfer to a low-frequency event-market forecaster with thin per-bin data?
- Detection design: rolling windows vs change-point; what statistic per decay type (CLV slope + CI; reliability-error trend; PIT KS-distance trend; Brier vs base-rate-forecaster control). How to be robust to thin data (the recurring constraint) — when is "decay" signal vs noise?
- Defense ladder (graded response, NOT a single kill): e.g. (a) re-fit calibration now (don't wait for the weekly cadence) → (b) widen the shrinkage-toward-market / lower the Kelly quality → (c) demote (reduce envelope) → (d) shadow-swap to a challenger model → (e) operator NO-GO alert. Map each rung to existing machinery (calibration refit B1/B3, sizing haircut, I7 demotion, `shadow.rs` incumbent/challenger).
- Constitution fit: I2 (drawdown halt is human-rearm — the watchdog must NOT auto-resume); I6 (propose-only — the watchdog proposes demotion/recs, the harness/operator acts on capital steps per I7); I5 (its findings are append-only audit rows); I4 (must not entangle the kill-switch). Determinism: all detection from the injected `Clock` + persisted scores (replayable).
- Telemetry: per-strategy/producer decay state (healthy / drifting / decaying / demoted) as a named metric family (extend A7's `belief_scores{producer}` etc.) + a ROTA panel + the chain-view.

### C.3 — Deliverable
A `docs/superpowers/specs/`-style design doc: the watchdog's detection statistics, the graded defense ladder mapped to existing gates, the config schema (name + thresholds), the constitution/decoupling analysis, the telemetry, and a phased implementation plan with TDD test ideas (mutation-proof: a synthetic decaying-CLV / drifting-calibration series must trip the right rung; a stable series must not).

---

## 5. Kickoff prompt (paste into the fresh session)

> Use the `deep-research-protocol` skill. Repo: `/Users/xavierbriggs/fortuna`. Read `docs/research/2026-06-18-scoring-learning-loop-edge-decay-watchdog-brief.md` in full — it is your brief. Verify its grounded findings against the code (cite `file:line`; do not inherit its conclusions). Produce three decision-grade artifacts: (A) a full scoring-system map + Module-3 conformance scorecard; (B) a learning-loop map (closed vs open arrows, feedback latency per category); (C) a design doc for the **Edge Decay Watchdog** (name + thresholds are CONFIG, opt-in default-off, decoupling-guard-clean) — detection statistics for calibration-drift AND edge-decay, a graded defense ladder mapped to existing gates (calibration refit, Kelly haircut, I7 demotion, shadow-swap, operator alert), constitution analysis (I2/I4/I5/I6/I7), telemetry, and a phased TDD plan. Respect the seven invariants; the protected `crates/fortuna-invariants/` is read-only context. Deliver a prioritized recommendation: which scoring-proof gaps (CLV-live #1, Murphy decomposition, PIT, reliability-diagram serialization) to close first, with cost/value for the demo and the $50k/mo north star.

---

## 6. Pointers
- Scoring: `crates/fortuna-cognition/src/{scoring,calibration,beliefs,review,events,shadow,cycle}.rs`.
- Live scoring: `crates/fortuna-live/src/daemon.rs` (`resolve_and_score_weather_beliefs`, `resolve_and_score_funding_beliefs`, `run_weekly_review`, `persist_daily_calibration`).
- Ledger: `crates/fortuna-ledger/src/repos.rs` (BeliefsRepo, ScalarBeliefsRepo, BeliefScoresRepo, CalibrationParamsRepo, resolved_stats, resolved_persona_stats), `migrations/`.
- Sizing/promotion: `crates/fortuna-runner/src/synthesis.rs`, `crates/fortuna-cognition/src/{cycle,review}.rs`, `crates/fortuna-invariants/tests/i7_promotion_gates.rs`.
- ROTA/telemetry: `crates/fortuna-ops/src/{rota,metrics}.rs`; the decoupling guard `crates/fortuna-invariants/tests/i_decoupling_spine.rs`.
- Spec sections: 5.5 (belief ledger), 5.8 (weekly review/GO-NO-GO), 5.10 (calibration), 5.14 (sizing), I7 (promotion).
- UI contract coordination: `docs/superpowers/specs/2026-06-18-operator-ui-overhaul-design.md` (parallel track; consumes the reliability-diagram + chain-view contracts).
