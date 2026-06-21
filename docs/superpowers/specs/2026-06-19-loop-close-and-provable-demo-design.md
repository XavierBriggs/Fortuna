# Loop-Close & Provable Demo — Design Spec

**Status:** design (brainstormed 2026-06-19). **North-star reference:** `docs/superpowers/specs/2026-06-19-scoring-and-validation-architecture.md` (the scoring/validation architecture; this spec is the buildable milestone derived from it). **Supersedes** the remaining Phase C tasks (D4, E1–E6) by folding them into this milestone.

> **For agentic workers:** the implementation plan derived from this spec is executed via subagent-driven-development (implementer→verifier per task, DST + invariants every task, final whole-branch review). This spec is the design, not the plan.

## 0. Goal (one sentence)

Make the FORTUNA learning loop **measurably close for every producer**, make the GO/NO-GO gate **tell the whole truth**, and surface a **provable demo** backed by a **real March→June track record** — built on a **generic, repeatable, decoupled backtest capability** — so the system can look forward, collect data, and promote-and-grow.

## 1. Why this supersedes "finish Phase C as written"

The V&V loop on the architecture doc proved the original remaining Phase C tasks rest on hidden gaps, so finishing them as written would ship a demo that *looks* done but is half-blind:

- **Persona beliefs are never scored.** Only two resolvers exist — `resolve_and_score_weather_beliefs` (daemon.rs:4637, Aeolus-only: `open_aeolus_weather_due` filters `provenance->>'model_id'='aeolus'`, repos.rs:1362) and `resolve_and_score_funding_beliefs` (daemon.rs:4378). Meteorologist beliefs (`provenance.persona_id`, event_id `{region_key}#{suffix}`) match neither → the Aeolus-vs-meteorologist head-to-head accrues **zero** scored data. (D4 as briefed would ship per-producer queries that return empty forever.)
- **CLV is dark.** `events.rs::clv_bps` exists but is never called live; `price_snapshots`' only writer (`SnapshotsRepo::insert`, repos.rs:791) has no live caller.
- **`go_nogo` omits half its own gate.** `go_nogo` (review.rs:189–308) gates only volume + CLV>0 + expectancy>0 + fee/PnL ratio — **no Brier-beats-baseline gate, no Resolution gate** — i.e. the calibration half of spec §11 is silently missing.

"Closing the loop" therefore *means* the scoring/validation machinery. This milestone builds the **"close + make it provable"** scope chosen by the operator.

## 2. Scope line (operator decision 2026-06-19)

**In this milestone (must-have for an honest, provable close):**

| Gap | What |
|---|---|
| G1 | CLV live — wire the capturer (write `price_snapshots`) + call existing `clv_bps` in the resolver |
| G2 | Unified `PredictiveKind` dispatch + **build** persona/synthesis binary resolution |
| G4 | Per-producer / per-mind keying (`provenance.producer` uniform; `(mind_id, mind_version)`) |
| — | Extend `go_nogo` with the **Brier-beats-baseline** gate (spec §11) |
| G3 | Murphy decomposition (REL/RES/UNC) + reliability-diagram serialization |
| G6 | PIT histogram (scalar) |
| G5 | RPS for bracket ladders + Log score (additive `impl ScoringRule`; Brier stays the gate) |
| — | **Generic backtest subsystem** (WS3) + real-history ingest |
| — | Demo surface (E1–E6) + scorecard serialization contract |

**Deferred to named follow-on milestones (own spec→plan→build, same rails):** G7 (`k_unc` sizing factor), G8 (Edge Decay Watchdog).

## 3. Architecture — four workstreams

The sequencing rule: **prove the machinery correct before real (messy) data flows through it.** WS1→WS2 build + test the live scoring with seeded data; WS3 ingests real history through the *already-proven* machinery; WS4 cuts the demo.

### WS1 — Live spine (the loop honestly closes for every producer)
Closes G1 (CLV-live), G2 (unified dispatch + persona/synthesis resolution), G4 (per-producer keying), and the `go_nogo` Brier-gate extension. **Consumes D4.**

- **Producer-agnostic weather resolution** (the D4a brief, `.git/sdd/task-D4a-brief.md`): stamp uniform grading keys (`nws_station_id`, `variable`, `target_date`, `producer`) on persona weather beliefs; generalize `open_aeolus_weather_due` → `open_weather_bracket_due` (select by grading-keys-present, **no producer literal**); generalize the resolver's `aeolus:`-prefix hint derivation to any producer. The realized-temperature lookup (NWS CLI via `nws_cli_realized`) is already producer-agnostic.
- **Unified dispatch (D-4):** one `score_resolved_beliefs(scope)` that selects the rule by `PredictiveKind` (data) and calls `rule.score(...)`, replacing the two hand-rolled forks; **build resolution for persona + synthesis binary beliefs** (today unresolved).
- **CLV-live (G1):** wire a live writer of `SnapshotsRepo::insert` (sample the daemon's existing book poll — do NOT add a second poller); call the existing `events.rs::clv_bps` + `SnapshotsRepo::latest_liquid_before` (repos.rs:830) in the resolver; persist via `resolve_and_score`. (Reuse — the selection algorithm already exists.)
- **Per-producer keying (G4):** `provenance.producer` uniform on both binary emit paths; `resolved_stats_for_producer`, `scores_for_producer`; `calibration_for_scope(..., producer: Option<&str>)`; synthesis prices the best-calibrated producer **deterministically** (quality DESC, producer-id ASC tie-break — producers are DATA, never `if producer=="aeolus"`).
- **`go_nogo` extension:** add the Brier-beats-market-baseline gate (spec §11, synthesis scopes only) so the verdict stops silently omitting the calibration half.

### WS2 — Proof layer (make it provable + visible)
Closes G3, G6, G5, and defines the **scorecard serialization contract** (committed early to unblock the UI session).

- **Murphy decomposition (G3):** REL/RES/UNC from the resolved + calibrated set per scope; `Serialize`; a ROTA/HTTP scorecard endpoint.
- **Reliability diagram (G3):** serialize the calibration curve (bins + counts + bands).
- **PIT histogram (G6):** bin `F(x_realized)` for scalar producers (Aeolus already computes PIT in `scorecards` — reuse the definition).
- **RPS + Log (G5):** two new `impl ScoringRule` (`RpsRule` for ordinal bracket ladders, `LogScoreRule`); additive — Brier stays the GO gate; recorded as data (`belief_scores.rule_id`).
- **Contract-first:** the scorecard serialization shape (per `(strategy, category, producer, mind)` scope) is committed **before** its compute lands, so the UI session builds against it in parallel.

### WS3 — Generic backtest subsystem (real history, zero Aeolus coupling)
A reusable, repeatable, hardened **system capability** — not a demo throwaway, not an Aeolus importer.

- **`fortuna-backtest` crate (new):** a `HistoricalSource` trait yielding **generic** records — `HistoricalBelief { provenance, p, kind, event linkage }`, `HistoricalOutcome`, `HistoricalSnapshot`, optional `HistoricalTrade`. The harness knows nothing about weather/Kalshi/Aeolus.
- **The harness** replays any source through the **same WS1/WS2 scoring** (so backtest and live score identically), writing into the FORTUNA ledger: **append-only, idempotent (re-runnable), deterministic (injected `Clock`)**, every row stamped `provenance.source="historical-import"` with **original timestamps** preserved (I5 honesty — never masquerades as a live decision).
- **`AeolusArchiveSource` (the only Aeolus-coupled code):** a source-adapter (producer-instance, like `aeolus_venue.rs`) that streams + cleans + maps `aeolus.db` (`scorecards`→scalar record + PIT, `bracket_probability_log`→binary beliefs) and `aeolus_kalshi.db` (`market_resolutions`→outcomes, `snapshot_quotes`→price snapshots, `shadow_intents`→paper trades). Streams (never loads 17.8 GB at once). The A7 decoupling guard excludes it as an adapter.
- **CLI:** `fortuna backtest <source> [--from --to]` — repeatable, idempotent.
- **Reusability:** a future producer/venue = one new `impl HistoricalSource`; nothing else changes.

### WS4 — Demo surface (E1–E6) + the demo cut
- **E1** `fortuna doctor` — readiness (DB, migrations, env/creds, mode-safe, GRANTs, source reachable).
- **E2** `fortuna start paper-demo` — fresh DB, `execution_mode="paper_ledger"` (paper fills, **no real-venue order**; the `i_paper_live_no_real_order` wall holds), pointer-write (F11). **The forward-collection entry point.**
- **E3** ROTA chain-view + safety pills + reasoning drill-in — emits `execution_mode`/`order_mutation_enabled`/book-freshness; renders per-event chain (signals→beliefs-by-producer-with-rationale→proposal→gate→fill→settle→score) + the head-to-head + reliability/Murphy/GO-NO-GO. **Data + serialization is ours (shared with WS2's contract); the rich render is the UI session's via the contract.**
- **E4** dead-man ping reconnect + alert.
- **E5** demo runbook + Aeolus stable-source note; CHANGELOG.
- **E6** operator `rearm` CLI verb (`gates.rearm()` exists, halt.rs) — clears a human-cleared halt out-of-band (I2), refuses if the kill-switch sentinel is present (I4).

## 4. Key design decisions

- **D-A (import honesty, I5):** historical rows enter the append-only ledger stamped `provenance.source="historical-import"`, original timestamps preserved — the audit log stays truthful about live vs imported. The backtest never fabricates live decisions.
- **D-B (backtest decoupling):** the harness is generic spine; the only domain-coupled code is the `HistoricalSource` adapter (producer-instance). The A7 guard stays green; `fortuna-backtest` does not enter the killswitch dep graph (I4).
- **D-C (forward-first / promote-and-grow):** the live loop is the engine; the backtest is the seed. §11 promotion runs on the **forward/out-of-sample clock**. The historical replay seeds calibration and *proves the surfaces on real data*, but counts toward a promotion verdict **only** if it meets §11's strict no-lookahead / paired-context bar; otherwise it is labeled backtest-evidence, distinct from the live forward window. (Honesty guardrail — V&V must pressure-test this.)
- **D-D (contract-first UI):** the scorecard serialization shape is committed before its compute, so the parallel UI session is never blocked on us. Each handoff is flagged in the plan.
- **D-E (live + on-cadence proof):** Murphy/PIT/reliability/GO-NO-GO recompute on the daemon cadence as first-class capabilities (not offline tooling), built iteratively through the implementer→verifier loop.
- **D-F (config vs spec reconciliation):** the shipped config diverges from spec §11 (`min_paper_days_mechanical=14` vs 30; `max_fee_pnl_ratio=0.5` vs 0.35; `min_resolved_beliefs_synthesis=100` vs 60 — the last is *stricter*, fine). The demo config (E2) uses the **spec** values; divergences recorded in GAPS.md.

## 5. Data flow

```
LIVE:      signal → belief(p_raw, producer) → calibrate(per-producer) → size(Kelly×quality) →
           gate(11 checks) → paper fill → settle(NWS CLI / venue) → resolve_and_score(rule by PredictiveKind) →
           Brier/RPS/Log/CRPS + CLV(live snapshots) → Murphy/PIT/reliability(cadence) → go_nogo → promotion(§11, operator)
BACKTEST:  HistoricalSource → generic records → ledger(append-only, source=import, orig ts) →
           [SAME resolve_and_score + Murphy/PIT/CLV] → scorecard
DEMO:      fortuna backtest aeolus-archive  (seed real history)
           fortuna start paper-demo         (live forward, paper_ledger, collect→promote)
           ROTA chain-view / UI endpoint    (per scope/producer/mind)
```

## 6. Invariants & decoupling guarantees

- **I1** universal gate unchanged. **I5** append-only — both the live `resolve_and_score` set-once (app-layer WHERE `outcome IS NULL` + rows_affected guard) and the backtest import (source-stamped, idempotent) preserve it; new scoring columns are additive. **I6** propose-only — the model emits no order/size/price; the harness scores + sizes. **I7** promotion is operator-only; demotion (if any) is the spec stage step-down. **I4** killswitch independence — `fortuna-backtest` and the proof layer live in cognition/live/ops, outside the killswitch graph (`i4_killswitch_independence.rs`).
- **Decoupling (A7):** producer/domain/venue are DATA. New code keeps the `ScoringRule` dispatch keyed on `PredictiveKind`; the best-producer selection sorts by quality (no name literal); the backtest harness carries no domain literal (only its adapter does). A targeted test asserts the generalized resolver queue carries no producer literal.
- **Protected invariants:** additions-only; baseline `dadd28a`. Any apparent need to weaken one → STOP, record in GAPS "Disputed invariant tests".

## 7. Worked example (end-to-end, once built)

**Part 1 — Seed the real track record (WS3).** `fortuna backtest aeolus-archive --from 2026-03-14 --to 2026-06-18` streams the two SQLite DBs, maps each day to generic records (`bracket_probability_log`→beliefs `producer="aeolus"`, `source="historical-import"`; `market_resolutions`→outcomes; `scorecards`→scalar record + PIT; `snapshot_quotes`→price snapshots; `shadow_intents`→paper trades, preserving `orders=0`), and replays them through the same scoring the live loop uses. Idempotent. Real March→June track record now in FORTUNA's ledger.

**Part 2 — The proof surfaces (WS2).** `fortuna scorecard --scope weather:KNYC:tmax --producer aeolus` →
```
n=88 (forward/OOS)  Brier 0.171 (baseline 0.214 ✓)  Log 0.508  CRPS 1.146  PIT ▁▂▅▇▆▅▂▁
Murphy: REL .004 RES .061 UNC .214 (informative)  CLV +83bps (T-1h, de-vigged)
Reliability near-diagonal  GO ✓ (CLV>0, Brier beats baseline, fee/PnL .22<.35, n≥60)
```
Every number is computed by live, on-cadence code — the same functions that score tomorrow.

**Part 3 — One live day (WS1+WS4).** `fortuna start paper-demo` (paper_ledger; paper fills only, no real order). Tuesday 2026-06-23: discovery tags `KXHIGHTNY-26JUN23-B87.5`; the CLV capturer snapshots at the tagging instant. Two producers author on the same bracket — `aeolus` p=0.38, `meteorologist` p=0.31 with a logged rationale. Each calibrates against its **own** curve; synthesis prices the better-calibrated (Aeolus today, by quality DESC — data-driven). Sizing → 7 @ 11¢; the 11-check gate passes → paper fill; the capturer keeps marking T-6h/T-1h. Wednesday: NWS CLI posts KNYC high 86°F → bracket NO; `resolve_and_score` writes outcome + Brier for **both** producers and CLV = de-vig(T-1h)−de-vig(entry) = +64 bps (market drifted toward our view pre-settlement — edge confirmed even on a losing contract). Calibration re-fits forward (n→89). Loop closed *and extended*.

**Part 4 — Operator view (E3).** The chain panel renders signals→beliefs(by producer + rationale)→synthesis(priced aeolus: Brier 0.171<meteo 0.196)→proposal→gate→fill→settle(Brier+CLV), plus the aeolus-vs-meteorologist-vs-synthesis head-to-head, reliability diagram, and GO/NO-GO chip. The UI session renders the same serialized data richly.

**Part 5 — Promote & grow.** The seeded history proves the machinery; the **live forward window** drives promotion. The §11 gate reads `INSUFFICIENT_DATA (accruing)` until the forward clock clears, then flips to **GO**; the operator (never the model) promotes Paper→Live-minimum. A future producer plugs into the same keyed scoring + the same backtest harness (one new `HistoricalSource`). That is the "grow."

## 8. Testing (every slice carries three layers)

TDD per slice (failing test from this spec first). **Each slice ships all three test layers where applicable — a slice is not done with only unit tests:**
- **Unit / property:** scoring logic (Brier/RPS/Log/CRPS/Murphy/PIT), the resolver-queue selection, Kelly/calibration math, the backtest mapping — property tests where the logic is total.
- **Integration:** the slice wired through its real neighbors against Postgres — e.g. a persona belief flows author→resolve→score→`resolved_stats_for_producer`; the CLV capturer writes `price_snapshots` and the resolver reads them back; `go_nogo` consumes real scored rows. Uses the `#[sqlx::test]` ledger harness.
- **Live-smoke / data:** the slice exercised against the live/daemon path or real data — the daemon smoke test (`daemon_smoke.rs`) extended per slice; `fortuna backtest` over a SMALL real archive slice produces non-empty scored rows; `fortuna start paper-demo` boots and the `i_paper_live_no_real_order` wall holds; the scorecard endpoint returns real serialized data.

Cross-cutting: DST corpus for anything touching orders/state/recovery; the full invariant suite (esp. i4 dep-graph + the A7 decoupling guard) on any task touching crate deps or moving types. Backtest also: golden replay (same input → byte-identical scores), idempotency (re-run is a no-op), import-honesty (every row source-stamped). Each slice runs the **implementer→verifier loop**; the milestone ends with a **final whole-branch review** (a single reviewer, not a fan-out workflow — token-disciplined). `cargo fmt`, `clippy -D warnings`, full suite, DST all green before any checkbox ticks.

## 9. Open questions (for the operator)

1. **Backtest counts toward promotion? — RESOLVED (conservative default, operator may override).** The historical replay is **evidence + calibration seed only**; the §11 promotion verdict runs on the **live-forward clock**. Backtest-scored beliefs are stamped `source="historical-import"` and are **excluded from the promotion volume/metric count by default** (a `forward_only` filter on the go_nogo inputs). They may be opted IN to the count later only if the replay provably meets §11's no-lookahead / paired-context bar — an explicit operator action, not the default. This keeps a green GO from ever resting on backtested data.
2. **Synthesis binary beliefs:** do they need their own resolution path, or is the meteorologist head-to-head the only binary-producer comparison the demo needs? (Investigation: synthesis provenance shape is currently undefined.) Scope WS1's "synthesis resolution" accordingly.
3. **Data cleaning rules:** the exact `aeolus_kalshi.db` → FORTUNA mapping (station-code normalization, bracket-threshold parsing, de-vig for single-contract Kalshi) is firmed up in the WS3 plan against the real schemas.
4. **E3 render split:** confirm the boundary — built-in ROTA text view (ours) vs the UI session's rich frontend — to avoid double-build.

## 10. Follow-on milestones (not this spec)

- **G7 — `k_unc` estimation-uncertainty shrinkage** (sizing factor; Baker–McHale / Chu–Wu–Swartz).
- **G8 — Edge Decay Watchdog** (config-named; needs G1–G6 as inputs).
