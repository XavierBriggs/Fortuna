# Phase-1 Review Area b — Subsystem Inventory (evidence)

Scope: FORTUNA Rust workspace `/Users/xavierbriggs/fortuna-wt-ws3`, 17 crates under `crates/`.
Method: `rg -n` + targeted `Read`. READ-ONLY; no code or docs modified.
Convention below: **as-built** = code; **as-intended** = spec/doc-comment. Both cited when they differ.

---

## 0. Codename resolution (confirmed)

The mission framing names Iris / Mercury / Atlas / Artemis / Nemesis / Nike as "subsystems."
**They are NOT FORTUNA subsystems.** Per `docs/spec.md:17` (Relationship to Olympus):

> "No Olympus crate is taken as a dependency. The salvage policy is harvest, not link… Atlas/Nike/Artemis's primary value is their documented failure modes and venue knowledge, not their code… Artemis's provider-trait pattern and Hermes/Athena context and memory patterns are reused as designs, reimplemented here."

So Atlas/Nike/Artemis/Hermes/Athena are **prior Olympus lineage projects** harvested for *designs*. Mercury is named only as the Postgres-familiarity precedent (`spec.md:31`); ITHACA is a backup host (`spec.md:342`). None appear as crates (workspace has exactly 17 `fortuna-*` crates, `Cargo.toml:3-21`) or as code symbols.

The only two real domain names in the code:
- **Aeolus** — external proprietary signal source; `aeolus_eval` strategy (`spec.md:335`, §6.3). Wired as a `Source` adapter (see §3 + §5 below).
- **Kinetics** — the perps domain (`spec.md:294`, §5.15). Realized as `InstrumentKind`/`PerpPrice`/`GatedPerpOrder` (see §4 below).

Doc-comments in code reinforce this: e.g. `mech_structural` is annotated "Atlas/Nike lineage" (`spec.md:333`) — lineage, not dependency.

---

## 1. Subsystem table

Layer key: L0 deterministic core, L1 belief/memory (Postgres), L2 cognition, L3 ops; + extras (tools/binaries).

| Crate | Layer | Responsibility (doc-comment cite) | Primary INPUTS | Primary OUTPUTS | Boundary contract (type / trait @ path:line) |
|---|---|---|---|---|---|
| **fortuna-core** | L0 | Shared vocabulary: ids, money, market view, perp types, clock, event bus. Lowest crate; everything depends on it. | (none — pure types) | `Cents`, `PerpPrice`, `MarketView`, `InstrumentKind`, `BusEvent`, `Clock` | `Cents` `money.rs:35`; `PerpPrice` `perp.rs:78`; `InstrumentKind` `perp.rs:68`; `MarketView` `market.rs:29`; `BusEvent` `bus.rs:115` / `EventPayload` `bus.rs:69`; `Clock` trait `clock.rs:163` |
| **fortuna-gates** | L0 | Deterministic pre-trade gate pipeline (I1). Constructs the sealed `GatedOrder`/`GatedPerpOrder`. Owns halt + rate-bucket state. | `CandidateOrder` + `GateInputs` (`pipeline.rs:162`) | `GateOutcome { gated: Result<GatedOrder,_> }` (`pipeline.rs:227`) | **gate entry** `Pipeline::evaluate()` `pipeline.rs:227`; **sealed** `GatedOrder` `order.rs:20` (priv fields, `pub(crate) fn assemble` `order.rs:37`); `GatedPerpOrder` `perp.rs:120` (`assemble` `perp.rs:137`) |
| **fortuna-venues** | L0 | Venue adapters + the `Venue` trait seam. Live Kalshi, Sim, Polymarket stub. Owns `Market`/`OrderBook`/`Fill` data types. | `GatedOrder` (place); HTTP/WS from venue | `VenueOrderId`, `Fill`, `OrderBook`, `Market`, `SettlementNotice` | **`Venue` trait** `lib.rs:94`; `async fn place(&self, order: GatedOrder)` `lib.rs:115`; `Market` `types.rs` (no `instrument_kind` field — see §6.4) |
| **fortuna-exec** | L0 | Intent journal + `OrderManager` state machine; journal-before-network; TTL sweep; fill dedup; flatten planner (§5.4). | `GatedOrder`, `Fill`, `Venue` trait | `IntentRecord`, `SubmitOutcome`, `BootReport` | `OrderManager<J: IntentJournal>` `manager.rs:219`; `submit()` `manager.rs:301` → calls `venue.place(order)` `manager.rs:371` |
| **fortuna-state** | L0 | Positions, account views, reservation ledger (§5.14), drawdown monitor (I2 detection). Derived state; rebuilt at boot. | `Fill`, `GatedOrder`, `SettlementNotice` | `PositionBook`, `AccountView`, `ReservationLedger`, drawdown verdict | `PositionBook` `positions.rs:143`; `AccountView` `accounts.rs:43`; `ReservationLedger` `reservations.rs:44` |
| **fortuna-ledger** | L1 | All Postgres persistence + repos; append-only audit (I5); scoring jobs (Brier/CLV). Migrations in `./migrations`. | audit rows, intents, beliefs, signals, calibration params | repo query results; `Scorecard` (from fortuna-scoring) | `BeliefsRepo` `repos.rs:1198`; `CalibrationParamsRepo` `repos.rs:1763`; `ScorecardsRepo` `repos.rs:2935`; `AuditWriter` `audit.rs:38`; `connect()` `lib.rs:68` |
| **fortuna-scoring** | L1 (pure) | Proper-scoring-rule math over immutable claims (Brier, CLV, Murphy, PAV, PIT, DM). Deps = serde+thiserror ONLY (acyclic). | outcome + forecast `f64` | `Scorecard` | `Scorecard` `scorecard.rs:81` (pure; `Cargo.toml` deps = serde, thiserror only) |
| **fortuna-cognition** | L2 | Model-agnostic mind, structured belief/proposal schema (I6), context assembler (§5.7), calibration (§5.10), signals + `Source` trait (§5.11). | `AssembledContext`, `RawSignal` | `MindOutput` (`BeliefDraft`, `ProposalDraft`), `SignalEnvelope`, `CalibrationParams` | **`Mind`** `mind.rs:162` (`decide` `:164`); **`Source`** `signals.rs:161`; `BeliefDraft` `beliefs.rs:73`; `ProposalDraft` `mind.rs:70`; `ScalarBeliefDraft` `scalar_beliefs.rs:32`; `AssembledContext` `context.rs:128` |
| **fortuna-sources** | L2 | Concrete ingest adapters that impl cognition's `Source`: Aeolus, NWS, RSS, calendar, + scheduler. "Deliberately dumb: fetch, retry, emit." | venue/RSS/REST/Aeolus HTTP | `RawSignal` (via `Source::fetch`) | impls `fortuna_cognition::signals::Source` (e.g. `aeolus.rs:68`); re-exports `lib.rs:23` |
| **fortuna-ops** | L3 | Config loader (env-only secrets), Slack routing (outbound only), dead-man pinger, metrics, read-only ROTA dashboard (axum). | `FortunaConfig` TOML+env; repo reads | Slack msgs, dashboard HTTP, metrics, accounting export | `FortunaConfig` `config.rs:36`; `SlackRouter`; ROTA `ScorecardQuery` `rota.rs` (read-only on ledger) |
| **fortuna-killswitch** | L0 / out-of-band (I4) | Standalone freeze-and-cancel + perp flatten. Functions when everything else (incl. Postgres) is dead. Flat-file journal. | live `Venue`, `Clock`, perp position/price | `KillReport`, `KILLSWITCH_REVOKED` sentinel file | `freeze_and_cancel()` `lib.rs:90`; `write_revocation()` `lib.rs:225`; `is_revoked()` `lib.rs:258`. Deps: core+venues+gates ONLY (NO sqlx/ledger/cognition) |
| **fortuna-runner** | L0 wiring | The deterministic single-threaded decision cycle. Defines the `Strategy` trait + `CoreHandle`; iterates strategies, sizes candidates, calls gates → exec → venue. | `BusEvent`, strategy proposals, beliefs, calibration | `TickReport`, `ShutdownReport`, pending beliefs/fills/settlements | **`Strategy` trait** `lib.rs:187`; `SimRunner<V,J>` `runner.rs:162`; `tick()` `runner.rs:888`; gate call `runner.rs` (`gates.evaluate`) |
| **fortuna-live** | binary | The live daemon: boot-validate config → PgIntentJournal + PgAuditSink → SimRunner → run_loop → graceful shutdown. Consumes the kill sentinel (I4). | config, Postgres, venue creds, edges | segment metrics, audit rows, persisted beliefs/fills, Slack | `run_loop()` `run_loop.rs:118`; **`RevocationHaltPoller<P>`** `run_loop.rs:85` (reads `is_revoked` `run_loop.rs:96`); `main.rs` |
| **fortuna-paper** | venue impl | Paper-fill engine for promotion gates (I7 / §11). Implements `Venue` identically to live. Market data pushed in. | `OrderBook`, `PublicTrade`, `GatedOrder` | `Fill`, `VenueOrderId`, positions | `PaperVenue` `lib.rs:99` (`impl Venue` `lib.rs:581`); `PaperLiveVenue` `paper_live.rs:23` (`impl Venue` `paper_live.rs:125`) |
| **fortuna-cli** | binary | Operator CLI: `status`, re-arm, kill-reversal (CLI-only per §8). Reads config-on-disk raw for the venue line. | CLI args, config, ledger | console output, operator actions | depends core/gates/ledger/ops; raw `toml::Value` read for `status` (deviation noted in its `Cargo.toml`) |
| **fortuna-recorder** | tool | B0 perishable-data recorder: captures public Kalshi perp/bracket endpoints to JSONL. Standalone; **zero fortuna deps**. | public Kalshi HTTP | `<out>/<date>/<stream>.jsonl` | `capture_row()` `lib.rs:87`; `top_of_book()` `lib.rs:58`. Deps: reqwest/tokio/anyhow/serde_json only |
| **fortuna-invariants** | tests | Executable encodings of I1–I7 (protected dir). | (compiles other crates) | test pass/fail | e.g. `perp_i1_sealed_order.rs`, `perp_i4_flatten_seal.rs`, `i7_promotion_gates.rs`; deps core+gates only |

Dependency DAG (from each crate's `Cargo.toml`, no cycles): core ← {gates, venues, state, exec, ledger, cognition, scoring, ops, killswitch, paper, runner, sources}; live binds nearly all. `fortuna-live → fortuna-killswitch` is a one-way sentinel-consumer edge that does NOT touch killswitch's own dep graph (preserving I4).

---

## 2. The trait seams (the "three swappable edges", spec Principle 10 `spec.md:36`)

| Seam | Trait def @ path:line | Method signature (load-bearing) | Impls | Consumed @ |
|---|---|---|---|---|
| **Venue** | `fortuna-venues/src/lib.rs:94` | `async fn place(&self, order: GatedOrder) -> Result<VenueOrderId, VenueError>` `:115` | Kalshi (`kalshi/adapter.rs`), Sim (`sim.rs`), Polymarket stub (`polymarket/mod.rs`), PaperVenue/PaperLiveVenue (`fortuna-paper`) | `fortuna-exec/src/manager.rs:371` (sole prod submit); killswitch cancel/flatten |
| **Mind** (= spec "Provider trait, Artemis pattern", `spec.md:222`) | `fortuna-cognition/src/mind.rs:162` | `async fn decide(&self, ctx: &AssembledContext) -> Result<MindOutput, MindError>` `:164` | `StubMind` `mind.rs:218`, `AnthropicMind<T>` `mind.rs:461` (impl `:792`/`:814`); factory `mind_from_env` `mind.rs:855` | `fortuna-cognition/src/cycle.rs` (`DecisionCycle::run`); synthesis strategy |
| **Strategy** | `fortuna-runner/src/lib.rs:187` | `async fn on_event(&mut self, ev:&BusEvent, core:&CoreHandle) -> Result<Vec<Proposal>,_>` + `drain_beliefs/drain_scalar_beliefs/drain_degrades/edge_count` | `SynthesisStrategy` (`synthesis.rs`), `MechStructural`, `MechExtremes`, `FundingForecast`, `PerpEventBasis`(V1/V2), `SynthEvents` (`synth_events.rs`) | `fortuna-runner/src/runner.rs` (registration-order iteration) |
| **Source** | `fortuna-cognition/src/signals.rs:161` | `fn id(&self)->&str; async fn fetch(&mut self) -> Result<Vec<RawSignal>, SignalError>` | `AeolusSource<T>` `fortuna-sources/src/aeolus.rs:68`, NWS, RSS, calendar (all in fortuna-sources), Scripted (tests) | scheduler/normalizer in fortuna-sources + ingestion in fortuna-live |
| **Clock** | `fortuna-core/src/clock.rs:163` | `fn now(&self) -> UtcTimestamp` | `RealClock` `:170`, `SimClock` `:216` | `EventBus` (`bus.rs`), gate inputs, exec timestamps — injected everywhere (no `SystemTime::now` in core) |
| **GatedOrder** (sealed type, not a trait — the I1 seam) | `fortuna-gates/src/order.rs:20` | priv fields; only `pub(crate) fn assemble` `:37`; `Serialize` only, `Deserialize` FORBIDDEN (`order.rs:9`) | constructed solely by `Pipeline::evaluate` `pipeline.rs:227` | `Venue::place`, `OrderManager::submit` |

**Belief/Proposal flow (I6):** `MindOutput { beliefs: Vec<BeliefDraft>, proposals: Vec<ProposalDraft>, journal, cost_cents }` (`mind.rs:117`, `#[serde(deny_unknown_fields)]`). Beliefs are calibrated then compared to markets to derive candidates; the model's own `ProposalDraft`s are COUNTED-then-DISCARDED (`cycle.rs:727`) — sizing/timing/order-type belong to the harness. This is the as-built witness of I6 (propose-only).

---

## 3. Aeolus mapping (concrete code)

- **Source adapter:** `AeolusSource<T: FetchTransport>` — `fortuna-sources/src/aeolus.rs:40`; `impl Source` `:68`; `fetch` `:73`. Imports the seam from cognition: `use fortuna_cognition::signals::{RawSignal, SignalError, Source};` `aeolus.rs:26`.
- **Signal kind constant:** `AEOLUS_FORECAST_KIND = "aeolus.forecast"` `aeolus.rs:35`.
- **Contract / dossier (as-intended):** doc-comment `aeolus.rs:1-20` → `docs/design/aeolus-fortuna-source-contract.md`, `docs/research/sources/aeolus/dossier.md`. Adapter is deliberately dumb: fetch `/v2/forecasts`, split `{"forecasts":[...]}` wrapper, emit one `RawSignal` per envelope UNTOUCHED. No strict-parse / validate / dedup / trust-weight (those are downstream).
- **Forecast → belief:** `fortuna-cognition/src/aeolus_beliefs.rs` turns reconciled envelopes into `BeliefDraft` (binary) + `ScalarBeliefDraft` (quantile fan). `aeolus_claimed_time` (`aeolus.rs`) supplies the point-in-time `run_at`/`init_time` for the scheduler's future-dated check.
- **Strategy:** `aeolus_eval` is the spec's signal-under-evaluation strategy (`spec.md:335`, zero capital, scored not traded). As-built it is realized as the cognition belief-scoring path (Aeolus forecasts → beliefs → scored), not a dedicated `AeolusEvalStrategy` struct; trading-strategy materialization gates on the Phase-2 verdict (`spec.md:398-399`). **Open question** below.

---

## 4. Kinetics (perps) mapping (concrete code)

- **`InstrumentKind { BinaryEvent, Perp }`** — `fortuna-core/src/perp.rs:68` (`#[serde(rename_all="snake_case")]`).
- **`PerpPrice(i64)`** — `fortuna-core/src/perp.rs:78`; integer ten-thousandths ($0.0001 tick), checked arithmetic; doc-comment `perp.rs:1-11`: "A `PerpPrice` must never carry an event-contract price nor vice versa — the separation is type-level, not convention." `Cents` stays the event-contract money type (`money.rs:35`).
- **`GatedPerpOrder`** — `fortuna-gates/src/perp.rs:120` (sealed, `assemble` `:137`, carries `PerpPrice` + `reduce_only`). Parallel to `GatedOrder`.
- **Perp lifecycle types** in `fortuna-core/src/perp.rs`: `PerpPosition` (no settlement lifecycle), `MarginAccountView`, `PerpMarks`, `FundingAccrual`/`FundingObservation`. `EventPayload::PerpTick { marks, funding }` `bus.rs:69`.
- **Perp strategies:** `FundingForecast`, `PerpEventBasis`/`V2` (`fortuna-runner/src/perp_event_basis*.rs`); `ScalarBeliefDraft` egress (`scalar_beliefs.rs:32`).
- **Kill-switch perp flatten:** `GatedPerpOrder` consumed by killswitch flatten path (reduce-only IOC), invariant-pinned by `perp_i4_flatten_seal.rs`.

---

## 5. Blurry / leaky boundaries

Severity is this reviewer's judgment; all are evidence-cited.

### 5.1 LOW — `Source` trait lives in fortuna-cognition, not fortuna-sources
**As-built:** the abstract `Source` trait + `RawSignal`/`SignalEnvelope`/`SignalError` are defined in `fortuna-cognition/src/signals.rs:161`, and `fortuna-sources` (the crate that holds the *concrete* adapters) depends on cognition to get the trait (`aeolus.rs:26`; `fortuna-sources/Cargo.toml` lists `fortuna-cognition`).
**Why it's blurry:** the "ingest layer" responsibility is split across two crates — the seam (trait + envelope + normalizer + trigger engine) in cognition, the adapters in sources. A reader expecting all of §5.11 in `fortuna-sources` will not find the contract there. Not a leak (sources → cognition is a legal L2-internal edge, and cognition does NOT depend back on sources), but the crate split does not mirror the layer boundary cleanly.

### 5.2 LOW — `MarketView` (a cognition-facing prefilter type) lives in fortuna-core
**As-built:** `MarketView` is in `fortuna-core/src/market.rs:29`. The doc-comment (`market.rs:8-26`) is explicit and defends it: it was *moved* from `fortuna-cognition::discovery` into core specifically so `fortuna-venues` can return it from `WeatherMarketSource::day_set` WITHOUT a `venues → cognition` edge that would break I4. So this is an intentional, invariant-preserving placement — but it does mean a cognition-shaped type now sits in the lowest core crate. Flagged as design tension, not a defect; the rationale is sound and documented.

### 5.3 MEDIUM (as-built vs as-intended) — `InstrumentKind` is defined and serde-tested but NOT threaded into any production type
**As-intended:** `spec.md:5` (and §5.15) — "InstrumentKind {BinaryEvent, Perp} threaded through **markets, positions, and gates**."
**As-built:** `InstrumentKind` (`perp.rs:68`) is referenced in production code **nowhere**. The only references are its own definition and the serde round-trip test (`fortuna-core/tests/perp.rs:24-56`). It is NOT a field on `MarketView` (`market.rs:29`, no such field), NOT on the venues `Market` (`fortuna-venues/src/types.rs`, no `instrument_kind`), and not consulted in `fortuna-gates` or `fortuna-state`.
**What actually enforces the binary-vs-perp split:** *type-level separation* — distinct sealed orders (`GatedOrder`+`Cents` vs `GatedPerpOrder`+`PerpPrice`) and distinct gate/strategy paths. The spec's own perp.rs doc-comment endorses type-level separation (`perp.rs:10-11`), which arguably *supersedes* a runtime `kind` discriminant. So the mechanism is sound, but the spec's wording ("threaded through markets/positions/gates") overstates the as-built: `InstrumentKind` is effectively a dead enum at present. **Recommend** either threading it as documented or recording in GAPS that type-separation is the chosen mechanism and the enum is vestigial.

### 5.4 LOW — naming: spec "Provider trait" vs code `Mind`
The spec (`spec.md:222`) calls the model seam the "Provider trait (Artemis pattern)"; the code names it `Mind` (`mind.rs:162`). Pure rename, but a reader grepping for `Provider` finds nothing. Worth a one-line alias note in docs.

### 5.5 (Checked, NOT a leak) — cognition/sources do not touch execution
Confirmed by dependency graph + import scan: `fortuna-cognition` and `fortuna-sources` import none of `fortuna_exec`/`fortuna_venues`/`fortuna_gates`/`fortuna_runner`/`fortuna_live`. I6 schema enforcement (`deny_unknown_fields` on `MindOutput`/`BeliefDraft`/`ProposalDraft`) plus the discard-proposals path (`cycle.rs:727`) keep the L2→L0 boundary clean. The one cognition-touching-core type (`MarketView`) is in core, not the other direction.

### 5.6 (Checked, NOT duplication) — paper vs venues, exec vs state
`fortuna-paper` does not duplicate `fortuna-venues`; it is another `Venue` impl (the paper-fill engine, §11/I7) and depends on venues for the trait. `fortuna-exec` owns the intent/order state machine; `fortuna-state` owns positions/accounts/reservations — disjoint. No two crates write fills or run gates. The only borderline overlap is the kill-switch flatten planner vs exec's flatten planner, but they are deliberately separate: exec's planner is gate-routed; the kill-switch path is planner-EXEMPT for I4 independence (`spec.md:5`, perp flatten seal).

---

## 6. Open questions (uncertain — not guessed)

1. **`aeolus_eval` as a named Strategy struct?** The spec frames `aeolus_eval` as a Section-6 strategy (`spec.md:335`), but as-built it appears realized through the cognition belief-scoring pipeline rather than a `Strategy`-trait impl named `AeolusEval`. I did not find an `aeolus_eval`/`AeolusEval` strategy struct. Whether a dedicated strategy is intended for Phase 3 promotion (`spec.md:399`) or whether the belief-scoring path IS the implementation is unconfirmed.
2. **`synth_events` strategy maturity.** `fortuna-runner/src/synth_events.rs` exists; I did not audit whether it is a live `Strategy` impl or a paper/Phase-3 stub (`spec.md:399` lists `synth_events` paper for Phase 3).
3. **Is `InstrumentKind` intended to be threaded later, or is type-separation the final design?** (See 5.3.) Needs an operator/spec call.

---

## 7. Citation index (most load-bearing)

- Codename resolution: `docs/spec.md:17`, `:31`, `:333`, `:342`
- Venue trait/place: `fortuna-venues/src/lib.rs:94`, `:115`
- GatedOrder seal: `fortuna-gates/src/order.rs:20`, `:37`, `:9`; gate entry `fortuna-gates/src/pipeline.rs:227`
- GatedPerpOrder: `fortuna-gates/src/perp.rs:120`, `:137`
- Mind: `fortuna-cognition/src/mind.rs:162`, `:164`, `:855`
- Belief/Proposal: `fortuna-cognition/src/mind.rs:70`,`:117`; `beliefs.rs:73`; `scalar_beliefs.rs:32`; discard `cycle.rs:727`
- Strategy: `fortuna-runner/src/lib.rs:187`
- Source: `fortuna-cognition/src/signals.rs:161`; Aeolus `fortuna-sources/src/aeolus.rs:40`,`:68`,`:26`,`:35`
- Clock: `fortuna-core/src/clock.rs:163`,`:170`,`:216`
- BusEvent: `fortuna-core/src/bus.rs:115`,`:69`
- Kinetics: `fortuna-core/src/perp.rs:68`(InstrumentKind, dead),`:78`(PerpPrice); `fortuna-core/src/money.rs:35`(Cents)
- Killswitch I4 seam: `fortuna-killswitch/src/lib.rs:90`,`:225`,`:258`; poller `fortuna-live/src/run_loop.rs:85`,`:96`,`:118`
- Repos/audit: `fortuna-ledger/src/repos.rs:1198`,`:1763`,`:2935`; `audit.rs:38`; `lib.rs:68`
- Scoring purity: `fortuna-scoring/src/scorecard.rs:81`
- Config: `fortuna-ops/src/config.rs:36`
