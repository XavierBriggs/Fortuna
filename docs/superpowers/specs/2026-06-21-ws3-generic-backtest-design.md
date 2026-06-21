# WS3 — Generic Backtest Subsystem (Design Spec)

**Status:** design (decision-grade v1) · **Date:** 2026-06-21 · **Authority:** `docs/spec.md` (v0.9) > `CLAUDE.md` > the milestone spec (`2026-06-19-loop-close-and-provable-demo-design.md`) > this. Invariants I1–I7 are absolute.

**Goal:** A reusable, hardened backtest subsystem that replays *any* historical source through the **already-proven WS1/WS2 scoring** and produces an *honest, overfitting-deflated* GO/NO-GO — so FORTUNA can seed a real track record and prove edge with integrity, not with one flattering number.

**Architecture:** A new decoupled `fortuna-backtest` crate (a `HistoricalSource` trait → generic, portably-serialized records → an idempotent/deterministic replay harness → the same `fortuna-scoring` rules + the same ledger write path) guarded by four integrity gates (G-PIT, G-DEAD, G-PARITY, G-TRUTH). The only source-coupled code is `AeolusArchiveSource`.

**Tech stack:** Rust 2021, `serde`/`thiserror`, `sqlx` (ledger writes), `rusqlite`/streaming for the Aeolus adapter; replay through `fortuna-scoring` (pure). Money is `Cents`/`i64`; probabilities `f64` in scoring only; all time via the injected `Clock`.

---

## 1. Context & north-star

- **Sequencing (milestone spec §3):** *prove the machinery correct before real (messy) data flows through it.* WS1→WS2 built + tested live scoring on seeded data; **WS3 ingests real history through the already-proven machinery**; WS4 cuts the demo.
- **The brief's spine (Alexandria):** a lone backtest is a *hypothesis to attack*; **data-leakage is the #1 killer**. Integrity must be **caught here, never inherited** from a source. The four gates are the falsifiable defenses.
- **FORTUNA's thesis (scoring-architecture spec §0):** *"we forecast event probabilities better than the market and prove it through process gates — calibration + CLV — not PnL."* The deflation therefore lands on the **forecasting edge**, not on PnL.

## 2. The three locked decisions (brainstorm 2026-06-21)

- **D1 — Scope:** the **full** subsystem: contracts + harness + all four gates **including** the complete DSR/PBO/CSCV deflation, the multi-config sweep it requires, and a **research-grounding pass** (do not hand-roll the overfitting math).
- **D2 — Alexandria interaction:** **fortuna-backtest is self-contained Rust; FORTUNA owns all four gates; sources are `impl HistoricalSource`.** Records get a **portable, language-neutral serialization** so a source can be in-process Rust (`AeolusArchiveSource`) *or* an external export (a future Alexandria dump). Integrity **never depends on trusting a source**. Synergy: Alexandria's keep-the-dead settlement recorder *is* the G-DEAD universe manifest — the source that makes the no-survivorship check enforceable is the one already being built. (A shared cross-language spec is the right *long-term* answer but YAGNI until a second producer runs backtests; D2 does not preclude it.)
- **D3 — G-TRUTH target:** **deflate the metric that backs the claim** — the **forecasting edge**, with **Brier-skill (beats-baseline margin) PRIMARY** and **CLV corroborating-only** (never creating a GO; market-price benchmark ≠ ground truth), via **PBO/CSCV** (metric-agnostic) + a **best-of-N** correction (**Hansen SPA preferred** over White RC — RC is contaminated by poor configs) on the *selected* config, with the trial count = the **joint scope × config grid**. The paper-trade Sharpe is reported **DSR-deflated as walled-off context**, never the headline. The sweep varies the knobs where overfitting actually lives: **calibration-window / recal-method / scope / GO-threshold**.

## 3. Architecture & decoupling

**New crate `fortuna-backtest`** — same decoupling discipline as `fortuna-scoring`:
- Depends on `fortuna-scoring` (replay through the *same* rules → G-PARITY by construction) and `fortuna-ledger` (write the replayed rows). Knows **nothing** about weather/Kalshi/Aeolus.
- A grep gate keeps `crates/fortuna-backtest/src/` free of source-name literals (`"aeolus"`, `"meteorologist"`, `"kalshi"`, …), mutation-proven like fortuna-scoring's.
- The **A7 decoupling guard** excludes only the adapter (`AeolusArchiveSource`), as it already excludes `aeolus_venue.rs`.

## 4. The Source/Record contracts

`HistoricalSource` yields **streams** of generic records (never loads the full archive at once) plus a universe manifest:

```
trait HistoricalSource {
    fn beliefs(&self)   -> impl Stream<Item = Result<HistoricalBelief>>;
    fn outcomes(&self)  -> impl Stream<Item = Result<HistoricalOutcome>>;
    fn snapshots(&self) -> impl Stream<Item = Result<HistoricalSnapshot>>;
    fn trades(&self)    -> impl Stream<Item = Result<HistoricalTrade>>;   // optional, paper-only
    fn universe_manifest(&self) -> Result<UniverseManifest>;              // the G-DEAD manifest
}
```

**Records (generic; portably serializable):**
- `HistoricalBelief { provenance{producer_type, producer_id, mind_id?, mind_version?, strategy_id, category, scope}, payload: Binary{p} | Scalar{quantiles}, event_linkage, available_at, decided_at }`
- `HistoricalOutcome { event_linkage, outcome, resolved_at, resolution_source }`
- `HistoricalSnapshot { market, price: Cents, at }`  (CLV benchmark)
- `HistoricalTrade { …, orders: 0 }`  (paper-only; read-only — never a real order)
- `UniverseManifest`: the **engaged set** — every event/market the producer *could/did* form a belief on, **including dead/voided/delisted/NO-resolved**.

**Bitemporal timestamp invariant (documented on the trait — the silent-leak guard):**
- `available_at` is **knowledge time** (`fetched_at` — when we *learned* it), **never** event/observed/`target_date` time. Mapping `available_at` to an event/target instant is the one mapping that silently breaks no-look-ahead.
- `decided_at` is when the decision was made.
- Post-resolution quantities (outcomes, realized scores) carry `available_at = resolution time` — they can *label*, never *decide*.

**Canonical `event_linkage` (pinned now, not later):** beliefs, outcomes, and snapshots all join on `event_linkage`. It is a **canonical cross-producer join key** with an explicit namespace. We have already drawn blood on its failure mode (the `event_id` namespace mismatch that gave persona beliefs `CLV None`). The contract defines the namespace (and, where two producers' native keys differ, the documented reconciliation) so that a future Alexandria `market_id` and Aeolus's linkage resolve to the *same* key — otherwise the as-of join silently drops rows.

**Portable serialization:** every record `serde`-serializes to **JSONL** as the canonical, human-auditable, diff-able contract (Arrow/Parquet an optional scale path later). This file format **is** the FORTUNA↔Alexandria boundary; a Python platform emits JSONL, FORTUNA's harness consumes it identically to the in-process adapter.

## 5. The four integrity gates

Each gate is enforced **FORTUNA-side** and ships an **executable test** (and a mutation that reds it).

### G-PIT — no look-ahead, enforced at JOIN time, per decision
- The replay assembles each decision's inputs via an **as-of join** keyed on `event_linkage`; `as_of(t)` is a **required parameter (no default)**.
- **Rule (documented):** a record enters a decision's context iff **`available_at < decided_at` (STRICT)**. Strict `<` is the conservative tie-break against coarse/same-bucket timestamps (a daily forecast where `available_at` and `decided_at` land in the same day must not admit same-bucket future info). The CLV-entry snapshot is the latest with `at < decided_at`.
- A record with `available_at >= decided_at` is a leak → **rejected/quarantined + counted**, never silently joined.
- **Executable gates:** (a) a planted look-ahead record (`available_at` after `decided_at`) is excluded + flagged; (b) a **BVA test at exact equality** (`available_at == decided_at` is *excluded*); a mutation relaxing the comparison to `<=` reds (b).

### G-DEAD — no survivorship, via the engaged-set manifest
- The harness scores against the source's full `universe_manifest()` (the **engaged set**: markets the producer could/did form beliefs on). It may **not** filter to survivors or favorable resolutions.
- **Load-bearing clause:** voided + NO-resolved markets are **present in the scored set**. (A producer that simply never forecast a market is *not* survivorship; one that forecast it then dropped the loser *is* — so "coverage == manifest" alone must not false-positive on legitimately-un-forecast markets; the voided/NO-present check is the real catch.)
- **Executable gate:** scored coverage == manifest (zero silent drops) AND voided/NO-resolved present; a "drop the dead/losers" mutation reds it. (Alexandria's keep-the-dead recorder emits this manifest.)

### G-PARITY — backtest scores identically to live
- Replay calls the **identical** `fortuna-scoring` rules **and** ledger write path as live; the only deltas are `provenance.source="historical-import"` + preserved original timestamps.
- **Target:** byte-identical scorecards modulo the source label — **conditioned on** zero hidden non-determinism in the scoring path (no wall-clock `now()`, no unseeded RNG, no map-iteration-order dependence; `fortuna-scoring` is pure and WS2 proved deterministic, RNG-free Wald bands). If any non-determinism surfaces, the claim weakens to "semantically identical within tolerance" (still acceptable) — but the default target is byte-identity.
- **Executable gate:** the same record set scored via the live path vs the backtest path yields byte-identical scorecards modulo the label (extends WS2's `scorecard_parity_seam` to the full replay); any divergence reds it.

### G-TRUTH — forecasting-edge deflation (the honest GO/NO-GO)
- The **sweep** varies the trial space {calibration-window, recal-method, scope, GO-threshold}; each config's **out-of-sample** forecasting edge is computed. **Brier-skill (beats-baseline margin) is the PRIMARY gated metric** (consistent with WS2's "Brier is the sole GO gate"); **CLV is a corroborating axis only** — reported with its own deflation, it may strengthen the read but **never creates a GO** (CLV's benchmark is a market price, not ground truth). The trial count is the **joint scope × config grid** (no cross-scope snooping).
- **PBO via purged + embargoed CSCV** (the #1 lie-prevention): plain combinatorial in/out splits on **overlapping labels** (Aeolus same-day/same-station brackets share information) *understate* overfitting — the OOS leaks and PBO looks artificially good. So splits are **purged** (drop train observations whose label window overlaps a test observation) and **embargoed** (a buffer around each test fold). PBO = P(the in-sample-selected config degrades below median OOS).
- **Best-of-N** correction (White Reality Check / Hansen SPA / deflated p-value) on the *selected* config's edge, accounting for the `N` trials.
- **Effective-N / MinTRL guard, as a verdict STATE:** post-purge the *independent* sample shrinks; the report carries `effective_n`. The verdict ∈ **{GO, NO-GO, INSUFFICIENT-EVIDENCE}** — if `effective_n < MinTRL` (the minimum track-record length that supports the CSCV partition count), the verdict is **INSUFFICIENT-EVIDENCE, never GO**. "We don't have enough independent data to say" is part of the whole truth (and aligns with WS2's existing `n<min_n → Insufficient`).
- **DSR** on the paper-trade Sharpe is reported as **walled-off supporting context** (PnL is not the edge claim).
- The GO surface reports `{selected_config, n_trials, family_n_trials, effective_n, brier_edge, brier_pbo, brier_spa_p, clv_edge, clv_pbo, clv_spa_p, sharpe_dsr, verdict}` — **never a single flattering number** (Brier is the gated headline; CLV columns are corroborating).
- **Executable gates:** (a) a deliberately-overfit sweep (many configs, one lucky winner) yields high PBO / non-significant deflated edge → **NO-GO**; (b) a **purged-CSCV-correctness test on a known-overlap fixture** — construct known overlap, confirm purging removes it, and a *no-purge* mutation measurably *understates* PBO (this is the test that proves the purging actually bites); (c) `effective_n < MinTRL` → INSUFFICIENT-EVIDENCE.

## 6. The replay harness

- **Streaming:** consumes the source's record streams; never materializes the full archive (the Aeolus `aeolus_kalshi.db` is 17.8 GB).
- **As-of join per decision:** the G-PIT enforcement point (§5).
- **Same scoring + same ledger path:** G-PARITY (§5). Writes append-only (I5), every row stamped `provenance.source="historical-import"` with **original timestamps preserved** — it never masquerades as a live decision.
- **Idempotent:** content-hash / `ON CONFLICT DO NOTHING` so a re-run is a no-op (re-runnable).
- **Deterministic:** all time via the injected `Clock`; no wall-clock. Same source + same range → same ledger.

## 7. The sweep + G-TRUTH machinery

- A **sweep driver** enumerates the trial space, runs the forecasting pipeline per config on the resolved set, and computes each config's OOS edge.
- The **deflation module** (`fortuna-scoring` extension or a `fortuna-backtest` sub-module — purity-preserving, pure math): purged+embargoed CSCV → PBO; best-of-N correction; DSR; effective-N/MinTRL. Grounded by the research pass (§8) before implementation.
- **Persistence:** a new append-only `validation_runs` table `{run_id, scope, producer, trial_space, n_trials, family_n_trials, selected_config, brier_edge, brier_pbo, brier_spa_p, clv_edge, clv_pbo, clv_spa_p, effective_n, mintrl_ok, sharpe_dsr, verdict, computed_at}` (Brier = gated; CLV = corroborating; `family_n_trials` = the joint scope×config grid), protected by the `fortuna_refuse_mutation` trigger (the `scorecards` pattern; I5). The WS2 scorecard contract is extended with the deflated view.

## 8. Research-grounding pass (FIRST, before implementation)

Like WS2's scoring-math report, a deep-research report grounds the overfitting toolkit *before* any code: **Bailey–López de Prado** (Deflated Sharpe Ratio; PBO/CSCV; purging & embargo; MinTRL), **White 2000** (Reality Check), **Hansen 2005** (SPA test), and the interaction with Diebold–Mariano (already in WS2). Deliverable: `docs/research/2026-06-21-ws3-backtest-overfitting-grounding.md` — exact formulas, the purge/embargo procedure, the effective-N/MinTRL derivation, and what to ADOPT vs simplify. The plan's deflation slices reference it; nothing is hand-rolled from memory.

## 9. `AeolusArchiveSource` (the only coupled code)

- Streams + cleans + maps `aeolus.db` + `aeolus_kalshi.db` → generic records + the manifest.
- **The post-resolution-leak trap (the adapter's #1 G-PIT risk):** Aeolus was built to *trade*, not as a bitemporal research archive, so it mixes pre-decision and post-resolution data.
  - `bracket_probability_log` is the **belief source**; its timestamp must be the **forecast-issuance** instant (knowledge time) → `available_at`.
  - `scorecards` carry CRPS/PIT/absolute_error — **post-resolution** quantities. The scalar belief must be the **predicted distribution at issuance** (issuance-knowable), **never** a realized score; the realized scores/PIT are **outcome-side only** (`available_at` = resolution time, label-never-decide), and FORTUNA **recomputes** them through its own scoring (this is also what keeps G-PARITY honest — we never import Aeolus's precomputed scores).
  - The adapter must **verify against the real schema** that the issuance instant actually exists and is not a backfilled/target timestamp.
- `market_resolutions` → outcomes (incl. voided); `snapshot_quotes` → snapshots; `shadow_intents` → paper trades (`orders=0` preserved).
- Exact schema-mapping (station-code normalization, bracket-threshold parsing, single-contract Kalshi de-vig) is firmed in the **plan**, against the real schemas.

## 10. CLI

- `fortuna backtest <source> [--from --to]` — idempotent replay into the ledger.
- `fortuna validate --scope … --producer …` — the sweep → deflated GO surface (writes `validation_runs`).
- Both repeatable; read-only on the source; paper-safe (no real orders, ever).

## 11. Testing & DST

- **Per-gate executable tests** (§5): look-ahead caught + BVA equality; dead/voided present + coverage==manifest; parity byte-identity; overfit→NO-GO + purged-CSCV-bites + INSUFFICIENT-on-thin-N.
- **DST scenarios (new):** re-run idempotency; partial-replay recovery (crash mid-stream → resume → no dupes, no gaps); clock-injection determinism.
- The crate stays pure (grep gate); the Aeolus adapter is the only coupled code.

## 12. Invariant safety

- **I5:** all replayed rows + `validation_runs` are append-only; the `fortuna_refuse_mutation` trigger guards the new table. Historical rows never masquerade as live (source-stamped, original timestamps).
- **I6:** the backtest runs no live model — it replays already-recorded beliefs and recomputes their scores deterministically. No new model authority or external-state mutation is introduced.
- **I7:** the deflated GO surface **recommends**; promotion to live capital remains an operator action. `crates/fortuna-invariants/` is not weakened (additions only).
- **A7 decoupling guard:** extended to exclude `AeolusArchiveSource` as an adapter; the core crate has no source literals.

## 13. Deferrals / open questions (for the plan)

- Exact `aeolus*.db` → record schema mapping (firmed against real schemas in the plan).
- Whether the deflation math lives in `fortuna-scoring` (reusable, pure) or a `fortuna-backtest` sub-module — decided at plan time by what keeps the dependency graph clean (lean: pure deflation in `fortuna-scoring`, orchestration in `fortuna-backtest`).
- Shared cross-language Source/Record *standard* with Alexandria — deferred until a second producer justifies the standardization (D2).
- Synthesis-belief backtest resolution path — inherits WS1's resolution; revisit if the demo needs synthesis history.
