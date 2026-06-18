# Phase C — Close the Loop (Demo-Paper-Ready v1) — Design

**Date:** 2026-06-18 · **Status:** brainstorm (pending operator approval) · **Authority:** `docs/spec.md` > `CLAUDE.md` > this. Builds on the audit `docs/audit/2026-06-18/` (esp. `MVP-CLOSURE-PLAN.md`).

## 1. Goal

One cohesive change that **closes the loop end-to-end for every strategy** and turns on the demo so it **accrues trustworthy validation data** — on the clean post-Phase-B baseline. Target: `fortuna start paper-demo` runs the full chain (signal → belief → proposal → gate → paper-fill → settlement → realized PnL → score → calibration), all arms, no real order possible, visible in one view — on the path to a **$50k/month validated PnL run-rate**.

It also **proves the intelligence thesis** (the moat): a Mind persona authoring beliefs that are *scored head-to-head with the quant model and the market, and traded* — demonstrated on weather (the working substrate), with the discovery/persona **machinery built generically** so macro/econ/politics are later config, not new code.

## 2. Design principles (non-negotiable for this plan)

These constrain every task. A task that violates one is wrong.

### 2.1 Decoupling — no hardcoding on weather / Aeolus / Kalshi
The audit (Area 5) found the engine clean *except* a weather/Kalshi seam in discovery. Phase C builds **generic** machinery; weather/Aeolus/Kalshi are the **first plug-in instances**, carried as data/config, never branched on in the spine.
- **Spine is domain/venue/producer-agnostic.** Settlement → PnL → trade-scoring → calibration are keyed by `(venue_id, market_id, producer, category, rule)` as *data*. No `if weather` / `if kalshi` / `if aeolus` literals in `fortuna-gates`/`-exec`/`-state`/`-ledger` spine paths or the scoring/calibration code.
- **Belief producers are pluggable.** `aeolus`, `meteorologist`, (future) `macro-economist` are just `producer` strings; the belief/scoring/calibration layer already keys per-producer. `synthesis` prices *any* belief→edge with no producer-specific code.
- **Discovery/seeding is venue- & domain-neutral.** Replace the weather-/Kalshi-specific F7 seam (`WeatherMarketSource` in the `kalshi::` namespace returning `Vec<KalshiMarket>`; `KalshiMarket` DTOs leaking into `fortuna-live` at `aeolus_venue.rs:42`/`daemon.rs:2276,2360`; hardcoded `venue:"kalshi"` at `aeolus_venue.rs:165`) with a generic **`MarketCatalog`/`MarketView`** discovery that takes a series/category from config. Adding a macro series or a Polymarket series is config, not code.
- **Persona machinery is generic.** Charter + schema + runner are domain-agnostic; `meteorologist` is one config row.
- **Testable:** a guard test asserts the spine crates contain no `"weather"`/`"kalshi"`/`"aeolus"` string literals in money/gate/scoring paths (allowed only in config, adapter, and producer-instance modules).

### 2.2 Idempotency — safe under retry, restart, and replay
Every write in the loop is idempotent; re-running the resolve/settle/score loop or restarting the daemon must **never double-count PnL or duplicate orders/fills/beliefs/events**.
- **Orders:** `ClientOrderId` (ULID) is the idempotency key; resubmission with the same key never creates a second order (preserve the existing manager/paper-venue behavior; add a test).
- **Fills:** each fill carries a stable `fill_id`; `FillsRepo` insert is `ON CONFLICT (fill_id) DO NOTHING`. Replaying the journal does not double-insert.
- **Settlement:** set-once per resolved market (unique key; the `realized_value IS NULL` / unique-constraint pattern already used for scalar resolution). Re-running the resolver is a no-op.
- **Beliefs:** dedup by `(producer, event, horizon/window)` — one scored unit per window (this *is* the fix for the funding oversampling: score per distinct window, not per tick).
- **Events:** robust dedup (the discovery/seeding fix) — no duplicate canonical events; a DB-level guard, not just the heuristic.
- **Calibration:** versioned upsert per scope (monotonic `version`); applying is idempotent.
- **PnL:** realized PnL is a deterministic function of persisted settled fills — **reconstructable from events, identically, on every run** (this is also the replay requirement).
- **Testable:** run the full resolve→settle→score cycle **twice** → byte-identical ledger state; replay a recorded session → identical PnL.

### 2.3 Good telemetry — the loop is observable, not a black box
Every stage emits metrics; the operator can *see* the loop work and diagnose where it stalls.
- **Per-stage counters/gauges/latencies:** signals ingested; beliefs authored (by `producer`); edges minted; proposals (by `strategy`); gate decisions (accept/reject **by check**); fills (by `venue`,`market`); settlements; realized PnL (by `strategy`); belief scores Brier/CLV (by `producer`); calibration state (by scope); mind budget spend; data freshness/staleness.
- **Sliceable by dimension** — `producer` / `strategy` / `venue` / `category` are metric **labels** (data, not hardcoded series), consistent with §2.1.
- **The ROTA chain-view** surfaces the full per-market chain + the safety state (`execution_mode`, `order_mutation_enabled`, book freshness) — the audit's missing P1.
- **Structured logs + Prometheus** via the existing `MetricsRegistry`; degrade-events and stale-data are visible (no silent failures — F1 of the constitution).
- **Testable:** the demo emits the key series; the chain-view renders a market's full chain; a stalled stage is visible as a flat/zero metric.

### 2.4 The constitution (unchanged, must stay green)
I1 universal sealed gate · I2 drawdown halt · I3 dual rate-limit · I4 standalone kill switch · I5 append-only audit · I6 propose-only/unsized model · I7 no live capital without promotion. **No real-order path constructible in paper.** No `panic!/unwrap/expect/f64` in money paths. All time via `Clock`.

## 3. The closed loop (shared spine) — what Phase C wires

```
live venue data ─► signal/tick ─► belief(producer) or edge ─► proposal (unsized, I6)
  ─► veto ─► 10-gate pipeline ─► sealed GatedOrder ─► PaperLiveVenue (LOCAL paper fill, idempotent)
  ─► [WIRE] settlement on resolution (set-once) ─► realized PnL ─► [WIRE] trade score
  belief side: belief ─► resolution/grading ─► Brier/CLV (per producer) ─► [WIRE] calibration persist ─► sizing
  all ─► append-only audit + [WIRE] durable recording ─► telemetry + ROTA chain-view
```
**[WIRE] = the four "computed-but-never-persisted" gaps** the audit found (settlement, fills, calibration, recording), each built **idempotent + telemetered + venue/producer-agnostic**.

## 4. Per-strategy flows (all close; all gather data)

| Arm | Belief/edge source | Phase-C wiring | Data it accrues |
|---|---|---|---|
| `mech_extremes` | market microstructure (≥90¢ fade) | settlement + scoring (shared) | realized PnL, fee drag, expectancy, fill realism |
| `mech_structural` | bracket-ladder arb | shared + **generic live-ticker discovery** | arb frequency + realized PnL (or honest "no arb") |
| `synthesis` (model) | **beliefs** — weather via Aeolus *and* meteorologist | F0 calibration-persist + settlement + scoring | model-vs-market edge, model-arm realized PnL |
| `perp_event_basis_v2` | Kinetics perp tick → per-bin q_j/EV | shared settlement + scoring | basis fills + realized PnL when EV clears |
| `funding_forecast` (data-only) | Kinetics funding tick → scalar fan | **scoring fix: dedup by window** (idempotency §2.2) | honest funding-forecast skill vs baselines |

## 5. The intelligence arm — meteorologist parallel to Aeolus

Two independent belief producers for each weather event, **scored separately** (the proof):
- **Aeolus** = quant (GEFS μ/σ → bracket CDF).
- **meteorologist** = Mind persona reading the qualitative NWS **AFD** (forecaster narrative), **alerts**, **CLI** obs (+ Aeolus + market as context), emitting a structured `outcomes[].p` / `regime` / `confidence` / `key_risk` per its schema → a `domain_analysis` + belief `producer="meteorologist"`.

Pipeline: trigger (fresh AFD/cadence) → context assembly (point-in-time, scoped to the event) → Mind call (persona **charter** + schema; fix the `main.rs:474` charter-injection bug so the persona's own method is used, not the synthesis charter) → persist analysis + belief → score on CLI resolution (per producer) → `synthesis` prices the calibrated belief vs market → paper trade. Over weeks the per-producer calibration answers: *does the Mind beat the model and the market?*

Design knob: the meteorologist **sees** the Aeolus number but is instructed to form its own view; per-producer scoring reveals whether it adds signal vs echoes the model.

## 6. Clean weather-market discovery (generic, "easy")

Generalize discovery so pointing at a series/category **auto-discovers markets → events → mappings → beliefs** with no manual `psql`, via the venue-neutral `MarketCatalog` (§2.1). Folds in the Area-9 fixes as the **generic machinery** (weather is the first instance, macro later):
- Fix the world-forward **unscoreable trap** (`discovery.rs:689` exact-match vs prose `resolution_source`) so discovered events become scoreable.
- Robust **event dedup** (DB-guard, not just heuristic) — idempotency §2.2.
- **Controlled category vocabulary** (no free-form `macro`/`Macro/Fed`/`x`).
- **Market-back event-matching** (`daemon.rs:2010` empty-`existing_events` stub) so re-discovery maps to existing events instead of minting duplicates.
- **Live-ticker refresh** so dated brackets never go stale (kills the expired-JUN16 problem).

## 7. Demo surface

- **`fortuna start paper-demo`** — one command: **fresh clean DB**, live venue reads, `execution_mode=live_data_only`/`paper_ledger` with **no order-mutation path constructible**, all arms in paper, telemetry on. A `fortuna doctor` readiness check (DB, migrations, creds, mode, GRANTs).
- **Fresh restart** is part of the command (clean slate for honest data; stop any prior daemon, provision a fresh DB, apply migrations + GRANTs).
- **ROTA chain-view** (§2.3) — mode/order-mutation/freshness + the full per-market chain.

## 8. Build order (one cohesive plan, internal sequencing)

All ship together as Phase C, but built spine-out so each step is testable:
1. **Generic spine** — settlement, fills, recording persistence + trade scoring (idempotent, telemetered, agnostic). Closes the mechanical + perp arms immediately. Fix funding-score window-dedup.
2. **F0 calibration-persist** (paper stage) — wakes `synthesis`; weather belief grading → Brier/CLV → calibration.
3. **Generic discovery/seeding** (§6) — venue-neutral `MarketCatalog`, the Area-9 fixes, easy weather discovery.
4. **Meteorologist persona** (§5) — charter fix, registry seed, enable; parallel-scored on weather.
5. **Demo surface** (§7) — `paper-demo` CLI, `doctor`, fresh restart, ROTA chain-view.

## 9. Out of scope / deferred
- Macro/econ/politics **domains** (the persona *machinery* is built generic here; those domains are config + a research pass later).
- The P2 legibility file-splits (daemon.rs/repos.rs/rota.rs), dual-mode collapse, `AnthropicVetoMind` (audit §12 — separate effort).
- Real/live capital (I7), demo-orders mode.

## 10. Testing strategy
- **TDD** per task; **DST** scenarios for settlement/PnL/calibration (seeded, deterministic, replayable).
- **Idempotency tests** (§2.2): double-run cycle = identical state; replay = identical PnL; duplicate fill/belief/event/order rejected.
- **Decoupling guard** (§2.1): no domain/venue literals in spine paths; a second producer (`meteorologist`) and the per-producer scoring exercised; a venue-neutral catalog test.
- **Per-strategy golden flows**: fixed inputs → stable proposal/gate/fill/settle/score records for each arm.
- **Telemetry test** (§2.3): key series emitted; chain-view renders.
- Invariants (I1–I7) stay green; `fmt` + `clippy -D warnings` + full workspace + `scripts/run-dst.sh`.

## 11. Definition of done — "ready to gather data"
On a fresh DB via `fortuna start paper-demo`: **every arm's loop closes through to persisted realized PnL and/or per-producer scored beliefs**, idempotently, observable in one ROTA chain-view, with no real order constructible — the meteorologist authoring weather beliefs scored head-to-head with Aeolus + the market. The system is accruing the PnL/calibration/CLV data that feeds the north star, with discovery/persona machinery generic enough that macro is the next config, not the next rewrite.

## 12. Risks / open questions
- **F0 / I7 boundary:** auto-persisting calibration in `paper` must be unambiguously "not capital promotion."
- **Discovery generalization** touches the F7 seam the live demo currently relies on — must stay green through the refactor (DST + the existing weather path as a regression).
- **Meteorologist quality cold-start:** needs resolved-belief accrual before its calibration is meaningful (weeks); telemetry must show "accruing, not yet calibrated" honestly.
- **Aeolus feed fragility** (ephemeral tunnel) — a stable source is an operator prerequisite for a multi-week soak.
