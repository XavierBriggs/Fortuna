# FORTUNA: System Design Specification

**Model-driven autonomous trading system. The model is the mind; FORTUNA is everything that makes the mind safe, stateful, and accountable.**

Version 0.8 (build-ready draft). June 9, 2026. Changes from 0.7 (verification-loop fixes): stale CLV-vs-close references corrected to benchmark snapshots, benchmark_at added to events, belief status enum completed, kill-switch flatten exempted from the planner (I4 independence), per-event decision-cycle serialization with trigger debounce (5.8), one-working-order-per-(strategy, market, side) rule (5.4), reservations rebuilt at boot as derived state (5.14), shadow-mode cost budget and paired-context rule (Section 11), taker paper-fill realism (Section 11), daily-loss definition pinned in config, Slack delivery failure escalates via dead-man path. Changes from 0.6 (review-loop fixes): capital hierarchy with allocation-vs-caps separation and reservation ledger (5.14), account views and conservative marking (5.14), multi-leg IntentGroup policy, execution policy with order TTL/re-quote, and flatten planner (5.4), same-event exposure cap moved to v1 and internal netting check added (5.3), beliefs FK to events (5.5), CLV benchmark-snapshot definition (5.5), belief freshness policy (5.5), triage scoring (5.8), replayability requirement (5.7), source registry with trust tiers (5.11), world-forward resolution-source requirement (5.12), exposure accounting fix for pending/disputed (5.13), stranded-state watchdogs (5.13), fee schedules as versioned config with per-fill reconciliation (5.2), re-arm and kill-reversal CLI-only (Section 8), housekeeping (BusEvent rename, day boundary 00:00 UTC, restore drill, stale strings). Changes from 0.5: settlement and event lifecycle reference model added (5.13), settlements/discrepancies tables and lifecycle metrics added (Sections 7, 8). Changes from 0.4: Postgres from day one (kill switch stays dependency-free), intent journal and crash recovery (5.4), clock abstraction and deterministic simulation testing, paper-fill realism rule (Section 11), dead-man heartbeat and accounting export (Section 8), abstract source interface (5.11), mech_extremes model veto confirmed for v1 with counterfactual scoring, Section 13 questions resolved. Changes from 0.3: canonical event model and discovery loops added (5.12), Slack replaces Telegram with channel routing (Section 8). Changes from 0.2: system renamed FORTUNA (was MINERVA), venue-agnosticism elevated to Principle 10, signal ingestion subsystem added (5.11). Changes from 0.1: greenfield salvage policy, in-house core decision finalized, Aeolus reclassified as signal-under-evaluation.

---

## 1. Purpose and thesis

FORTUNA is a single-operator autonomous trading system in which a frontier LLM (currently Claude Fable 5, interchangeable by design) performs synthesis and decision proposal, and a deterministic Rust harness owns state, execution, risk, and accountability. The thesis, validated by the foundation research: model capability is a rising commodity; durable edge lives in the harness (context assembly, memory, calibration post-processing, fee-aware execution, capacity-tier positioning) and in proprietary signal inputs (Aeolus and successors).

What FORTUNA is: an operating system for beliefs and trades. Signals come in, beliefs update, proposals are gated, orders execute, outcomes are reconciled, lessons persist.

What FORTUNA is not: a product, a backtesting playground for LLM decisions (forward-only validation, per research finding on contamination), or a high-frequency system. Decision cadence is minutes to days, never microseconds.

Relationship to Olympus: FORTUNA is greenfield. No Olympus crate is taken as a dependency. The salvage policy is harvest, not link: specific battle-tested functions (Kalshi auth/signing, fee math, ticker parsing, canonical event matching per the teams.toml approach, settlement-mechanics handling) are copied into fresh fortuna crates, reviewed, and owned; everything else is rewritten. Atlas/Nike/Artemis's primary value is their documented failure modes and venue knowledge, not their code. Aeolus continues to run as an external signal producer; FORTUNA consumes its output (Section 6). Artemis's provider-trait pattern and Hermes/Athena context and memory patterns are reused as designs, reimplemented here.

---

## 2. Design principles

1. **The model proposes, the harness disposes.** No code path exists by which model output reaches a venue without passing the deterministic gate pipeline.
2. **Deterministic core, probabilistic shell.** Everything that touches money is deterministic, replayable Rust. Everything probabilistic produces artifacts (beliefs, proposals, journals) that the core consumes.
3. **Beliefs are first-class.** The model's primary output is structured beliefs, not trades. Trades are derived by deterministic comparison of beliefs to prices. Every belief is scored against reality whether or not it was traded. The belief ledger is the proof-of-edge mechanism.
4. **Forward-only validation.** LLM-decision performance is only ever measured on data the system encountered live (paper or real). Backtests validate deterministic components only.
5. **Memory over fine-tuning.** The system improves by distilling reconciled lessons back into future context (FinMem/FinCon pattern; Athena MEMORY.md pattern). Model weights are never assumed to carry system state.
6. **Model-agnostic by contract.** The cognition layer speaks one structured proposal/belief schema over a provider trait. Swapping models is a config change plus a mandatory shadow period.
7. **Fee-aware by construction.** Every proposal is evaluated net of modeled fees before gating. Maker-first execution. Fee/PnL ratio is a tracked per-strategy metric. (Alpha Arena's losers died of fees; this bug class is excluded by design.)
8. **Degrade gracefully to mechanical.** If the model, provider, or cognition layer is down, mechanical strategies and risk management continue unaffected. The system makes money without the brain; the brain is upside.
9. **Boring persistence, simple hardening.** Postgres from day one (operator-familiar via Mercury; concurrent dashboard readers, partitioned append stores, analytical scoring queries; the migration avoided is worth more than the simplicity deferred), TOML config, integer-cent arithmetic via rust_decimal, Slack alerts, tokio async. Olympus conventions throughout. Exception: the standalone kill-switch process keeps self-contained flat-file/SQLite state because it must function when nothing else is alive, including Postgres. No component earns complexity until something measurably breaks without it.
10. **Venue-agnostic by contract, symmetric with model-agnosticism.** The durable core is the invariant middle (gates, belief ledger, memory, calibration, audit). Three swappable edges surround it: minds (Mind trait), venues (Venue trait), strategies (Strategy trait). Capital rungs are traversed by adding or swapping edges, never by rewriting the middle. Prediction markets are the launch venue class, not a structural commitment; the equities rung is an adapter plus a fee model, not a fork.

---

## 3. Non-negotiable invariants

These are absolute. Any change requires a written rationale in the audit log and a version bump of this document. Modeled on SEC 15c3-5 / MiFID II Art. 17 control requirements (the post-Knight-Capital spec), applied voluntarily.

- **I1. Universal gate.** Every order, regardless of origin (model proposal, mechanical strategy, manual CLI), passes the same deterministic pre-trade gate pipeline. The model cannot bypass, modify, disable, or be consulted by the gates. Gates are config-driven (TOML), hot-reloadable only by the operator.
- **I2. Drawdown halts with human re-arm.** Per-strategy and global max-drawdown thresholds. Breach flattens or freezes per policy and sets a halt flag that only a human can clear, out-of-band. No automatic resumption.
- **I3. Runaway detection.** Dual token-bucket rate limits (burst plus sustained) per venue and per market on order submissions. Breach is a halt, not a throttle. Duplicate-order detection via client-order-id idempotency.
- **I4. Out-of-band kill switch.** A kill path (Slack command plus a local CLI) that flattens or freezes all positions and revokes order-placing capability. It must not depend on the cognition runtime, the event loop, or any LLM provider being healthy. Tested monthly.
- **I5. Append-only audit log.** Every model call (prompt hash, context manifest, model id, cost), every belief, every proposal, every gate decision (pass/modify/reject plus reason), every order, every fill, every config change. Sufficient to replay any decision after the fact. Never deleted, never updated in place.
- **I6. Propose-only model interface.** The model emits structured proposals and beliefs into a queue. Sizing, timing, order type, and execution belong to the harness. The model has zero tools that mutate external state.
- **I7. Promotion gates.** No strategy touches live capital without passing its forward validation gate (Section 11). No model version replaces another in live decision flow without a shadow-mode comparison period. No capital scale-up without continued forward performance.

---

## 4. Architecture overview

```
                        ┌─────────────────────────────────────────────┐
                        │  L3 OPERATIONS                              │
                        │  metrics, dashboards, Slack, kill switch,│
                        │  cost tracking, config, audit query tools   │
                        └─────────▲──────────────────────▲────────────┘
                                  │                      │
┌─────────────────────────────────┴───┐   ┌──────────────┴───────────────────┐
│  L2 COGNITION (model-agnostic)      │   │  L0 DETERMINISTIC CORE (Rust)    │
│                                     │   │                                  │
│  context assembler                  │   │  event bus (single-threaded,     │
│  decision cycle (proposals/beliefs) │   │   deterministic ordering)        │
│  reconciliation loops (D/W/M)       │──▶│  gate pipeline (I1..I3)          │
│  calibration post-processor         │   │  order manager / execution       │
│  provider trait (Fable 5 / any)     │   │  position + account state        │
└──────────────▲──────────────────────┘   │  venue adapters (Kalshi, PM,     │
               │                          │   ForecastEx, broker, CEX)       │
┌──────────────┴──────────────────────┐   └──────────────▲───────────────────┘
│  L1 BELIEF AND MEMORY (Postgres)    │                  │
│                                     │           market data, fills
│  belief ledger     trade journal    │                  │
│  memory store      audit log        │            ┌─────┴─────┐
│  scoring jobs (Brier/CLV)           │            │  venues   │
└─────────────────────────────────────┘            └───────────┘
```

Data flow, one full cycle: venue/signal data enters the core event bus and is persisted point-in-time. The context assembler builds a budgeted context from L1 plus live state. The model emits beliefs and proposals. The calibration layer adjusts probabilities. The decision engine (deterministic) compares calibrated beliefs to live prices, derives candidate orders with sizes, and submits them to the gate pipeline. Gated orders execute via the order manager. Fills update state. The daily loop reconciles outcomes against beliefs and writes journal entries and distilled lessons back into memory. Everything is logged to the audit store at each step.

---

## 5. Component specifications

### 5.1 Core runtime and event bus (L0)

Single-threaded deterministic message bus for all trading-relevant events (NautilusTrader / LMAX disruptor pattern), with tokio runtimes for network IO feeding into it. Deterministic event ordering is what makes replay and audit meaningful.

Build decision (finalized v0.2): **in-house core, greenfield.** Three reasons. First, forward-only validation (Principle 4) removes the main reason to adopt NautilusTrader: its high-fidelity backtest engine. FORTUNA needs deterministic replay of gates and execution, not nanosecond matching simulation. Second, the engine surface actually required at Rung 0 is small: event loop, order state machine, gate pipeline, state reconciliation. Third, the hard part (prediction-market adapters, settlement metadata, oracle-delay handling) does not exist in Nautilus and would be written in-house regardless. Nautilus's deterministic single-threaded bus and replay discipline are adopted as patterns. Two supporting disciplines: a `Clock` trait injected everywhere (no scattered SystemTime::now(); a sim clock drives replay and tests), and deterministic simulation testing as the primary test methodology: a sim venue adapter plus seeded fault injection (delayed acks, dropped fills, dup messages, mid-cycle crashes) runs the full core through thousands of randomized failure scenarios per CI run (TigerBeetle VOPR / Antithesis pattern; turmoil or madsim class tooling). Documented revisit trigger: the equities rung (post-$25k), where Nautilus's broker integrations carry real value; the alternative at that point is a simple in-house Alpaca/IBKR REST adapter, decided then.

Crate layout (workspace):

```
fortuna/
  crates/
    fortuna-core        # event bus, clock, ids, replay
    fortuna-gates       # gate pipeline (I1..I3)
    fortuna-exec        # order manager, fee models, idempotency
    fortuna-state       # positions, balances, reconciliation vs venue
    fortuna-venues      # adapter trait + kalshi/, polymarket/, forecastex/, ...
    fortuna-ledger      # belief ledger, journal, memory, audit (Postgres)
    fortuna-cognition   # context assembler, loops, provider trait, schemas
    fortuna-ops         # metrics, slack, kill switch, CLI
  config/
    fortuna.toml        # limits, venues, strategies, model tiers
```

### 5.2 Venue adapter trait

```rust
#[async_trait]
pub trait Venue: Send + Sync {
    fn id(&self) -> VenueId;
    async fn markets(&self, filter: MarketFilter) -> Result<Vec<Market>>;
    async fn book(&self, market: &MarketId) -> Result<OrderBook>;     // L1/L2 as available
    async fn place(&self, order: GatedOrder) -> Result<VenueOrderId>; // takes GatedOrder only
    async fn cancel(&self, id: &VenueOrderId) -> Result<()>;
    async fn positions(&self) -> Result<Vec<VenuePosition>>;
    async fn fills_since(&self, cursor: Cursor) -> Result<Vec<Fill>>;
    fn fee_model(&self) -> &dyn FeeModel;                              // per-venue formulae
}
```

Type-level enforcement of I1: `place` accepts only `GatedOrder`, a type constructible solely by the gate pipeline. Fee models per venue: Kalshi taker 0.07 x p x (1-p) with maker discounts and category multipliers; Polymarket Intl mostly zero with 0.0625 formula on fee-enabled markets; Polymarket US flat 10bp taker. Settlement metadata (oracle type, resolution source, expected lag) is part of `Market`, because oracle-delay artifacts must be excluded from edge scans (per the NBA microstructure finding). Fee schedules are data, not code: a versioned fee config per venue (formula type: quadratic p(1-p) coefficient | flat bps | tiered; coefficients; maker/taker variants; category multipliers; effective_date) interpreted by one fee engine, so a venue fee change is a config change with an audit row. Every fill reconciles charged fee against modeled fee; mismatch writes a discrepancy (the config is only trusted because it is continuously verified).

### 5.3 Gate pipeline (fortuna-gates)

Ordered, fail-closed checks. Each emits an audit record with verdict and reason.

1. Halt flags (global, per-strategy, per-venue) clear.
2. Account capital threshold: order cost plus open exposure within configured aggregate limit (integer cents).
3. Per-market and per-strategy position caps.
4. Price sanity: limit price within configured band of mid/last; reject crossing beyond max slippage.
5. Size sanity: min/max contract counts; notional cap per order.
6. Fee-adjusted edge floor: modeled net edge must exceed configured minimum (e.g., reject if expected value net of fees < threshold per strategy).
7. Rate limits: dual token bucket per venue and per market (I3).
8. Duplicate/idempotency check on client order id.
9. Same-event exposure cap (v1): aggregate worst-case exposure per canonical event across all markets and venues, computed via market_event_edges. Launch strategies (brackets, cross-venue) are intrinsically correlated, so this ships in v1; broader cross-event correlation modeling is v2.
10. Internal netting check: reject or cancel-and-replace any order that would cross FORTUNA's own resting order on the same market. Strategies remain independent deciders; the shared gate layer deduplicates execution (no self-crossing, no internal fee burn).

Config example:

```toml
[gates.global]
max_total_exposure_cents = 800_000        # 80% of account
max_daily_loss_cents     = 50_000         # drawdown halt: realized + conservative-mark unrealized, day = 00:00 UTC
[gates.per_strategy.mech_extremes]
max_exposure_cents = 200_000
max_order_notional_cents = 10_000
min_net_edge_bps = 150
[gates.rate.kalshi]
burst = 5
sustained_per_min = 20
```

### 5.4 Order manager and execution

Maker-first policy: default resting limit orders at or inside the configured edge price; escalation to taker only when (a) the strategy declares time sensitivity and (b) net edge after taker fees still clears the floor.

**Intent journal and crash recovery.** Every order intent is persisted with a state machine (created -> submitted -> acked -> partially_filled -> filled | cancelled | rejected) BEFORE any network call. Client order ids are derived deterministically from intent ids, so resubmission after a crash is idempotent by construction. Boot sequence: no strategy wakes until reconciliation completes: fetch venue open orders and fills since last cursor, match against the intent journal, adopt orphans (venue orders with no journal entry are cancelled and alerted), advance stuck intents, and only then release the event loop. Delivery semantics are at-least-once plus idempotent dedup; exactly-once over a network is designed around, not assumed. Periodic state reconciliation: venue positions/balances are authoritative; divergence beyond tolerance raises an alert and freezes the affected strategy. Partial-fill handling: remaining quantity re-evaluated against current gate state, not blindly chased.

**Multi-leg execution (IntentGroup).** Multi-leg strategies submit legs as one IntentGroup with a declared completion policy: maximum unhedged notional and maximum leg-open duration. Partial completion beyond either bound triggers a deterministic complete-or-unwind decision: taker-complete if net edge after taker fees still clears the floor, otherwise unwind. Group-level reconciliation; unwind costs are logged as execution-loss attribution per strategy (this is where the documented 62% combinatorial-arb failure mode is either survived or measured).

**Execution policy.** Entry style (passive maker, stepped re-quote, taker escalation) is selected deterministically from urgency, book liquidity, and edge-decay rate. Every resting order carries a TTL; any belief update or relevant signal touching the order's event forces cancel-and-re-quote. One working order per (strategy, market, side) unless the strategy explicitly declares laddering; a refreshed belief re-quotes the existing order rather than stacking a new one. v1 is TTL plus re-quote; a smarter entry system (queue-aware, decay-aware) evolves behind the same policy interface.

**Flatten planner.** Any flatten request through the main runtime (halt policy, operator) first computes an estimated book-walk cost. Default action is freeze-and-cancel; flatten executes only with operator confirmation displaying the estimate, or automatically when the estimate is within a configured bound. Panic-flattening a thin book is a self-inflicted loss the system refuses to take silently. Exemption by design: the standalone kill-switch process cannot depend on the planner (I4 independence); its default action is freeze-and-cancel, and emergency flatten through it is best-effort without cost estimation, an accepted emergency cost.

### 5.5 Belief ledger (the heart of L1)

The model's primary artifact. Schema (Postgres; shown abbreviated):

```sql
CREATE TABLE beliefs (
  belief_id     TEXT PRIMARY KEY,          -- ulid
  created_at    TEXT NOT NULL,             -- ISO8601 UTC
  event_id      TEXT NOT NULL,             -- FK -> events (5.12); beliefs attach to events only
                                           -- market linkage flows through market_event_edges
  p             REAL NOT NULL,             -- model probability, post-calibration
  p_raw         REAL NOT NULL,             -- pre-calibration
  horizon       TEXT NOT NULL,             -- resolution timestamp
  evidence      TEXT NOT NULL,             -- JSON: [{source, ref, weight_note}]
  provenance    TEXT NOT NULL,             -- JSON: {model_id, prompt_hash, context_manifest_hash, cost_cents}
  supersedes    TEXT,                      -- prior belief_id this updates
  status        TEXT NOT NULL DEFAULT 'open',  -- open|resolved|superseded|abandoned
  outcome       INTEGER,                   -- 0/1 when resolved
  brier         REAL,                      -- filled by scoring job
  clv_bps       REAL                       -- vs benchmark snapshot (see CLV definition below)
);
```

Rules: beliefs are immutable; updates create a new row with `supersedes`. A scoring job resolves outcomes (from venue settlement or registry resolution sources) and computes Brier per belief and rolling calibration curves per (strategy, category, model_id, source). This yields the edge-attribution matrix: where the model is calibrated, where it is noise, and which evidence sources carry weight.

**CLV definition (benchmark snapshots, not settlement).** Settlement prices converge mechanically toward $0.99 as resolution becomes certain, so CLV is never measured at settlement. Each event carries benchmark_at = event start time when known, else expected_resolution_at. Markets linked to the event get scheduled price snapshots (T-24h, T-1h, T-5m before benchmark_at, plus at every FORTUNA trade). CLV = entry price vs the latest liquid pre-benchmark snapshot, subject to a minimum-liquidity filter (stale or one-sided books produce no CLV rather than fake CLV); post-event oracle-drift windows are excluded.

**Freshness policy.** Every belief has a category-configured maximum age. Refresh is required when age is exceeded, when a relevant signal arrives on the event, or inside the pre-benchmark window (refresh cadence tightens approaching event start, where staleness costs the most). Stale beliefs are excluded from the comparator until refreshed; a position held under a stale belief raises the stranded-state watchdog (5.13).

### 5.6 Trade journal and memory store

Three memory tiers (FinMem-inspired, Athena-style implementation):

- **Working memory:** assembled fresh each cycle (positions, open beliefs, today's plan, live prices). Not persisted as memory; it is a view.
- **Episodic memory:** daily reconciliation outputs. One journal entry per trading day: every closed trade reconciled against its originating belief and thesis (right for the right reason / right for the wrong reason / wrong thesis / wrong execution), realized vs expected fees, notable misses (beliefs that were correct but untraded), and tomorrow's plan.
- **Semantic memory:** distilled lessons promoted from episodic entries during weekly review (e.g., "NWS discussion updates before 06Z systematically lead Kalshi high-temp markets in winter"; "my synthesis beliefs on politics categories are uncalibrated, stand down"). Bounded list, each lesson carries provenance (which journal entries support it) and a review date. Lessons decay: unconfirmed lessons are demoted at monthly review.

Semantic memory and the current plan are injected into every decision-cycle context. This is the entire learning loop; no fine-tuning.

### 5.7 Context assembler

Deterministic, budgeted context packing per cycle type. Sections in priority order: system charter and constraints summary; current account/position state; open beliefs relevant to the trigger; live market snapshot (prices, books, fees for candidate markets); fresh signals (Aeolus output, news items with timestamps and sources, venue announcements); semantic memory lessons; recent episodic excerpts if relevant. Every context build emits a manifest (list of item ids and hashes) into the audit log; the manifest hash lives in belief provenance, making any decision reconstructable.

Replayability requirement: every context section must be either an immutable stored item (referenced by id and hash in the manifest) or deterministically recomputable from the audit log at the manifest timestamp; computed views (position state, account views) are snapshotted into the manifest where recomputation would be expensive. Point-in-time discipline: only data timestamped before the cycle trigger enters context. Anonymization mode (strip entity identifiers) is available for any retrospective evaluation, per the Glasserman/Lin distraction-effect finding, but retrospective evaluation of model decisions is out of scope for validation regardless (Principle 4).

### 5.8 Cognition loops

- **Fast loop (continuous, no frontier model):** core scans books and signals; mechanical strategies act directly through gates; triggers are raised for the decision cycle (price diverged from an open belief by > X, new Aeolus run, scheduled market open, news webhook).
- **Decision cycle (trigger-driven, minutes-scale):** per-event serialization: at most one decision cycle in flight per canonical event, with a debounce window that coalesces triggers arriving during or shortly after a cycle (a news burst is one decision, not five). Tiered model usage. A cheap model triages the trigger (worth frontier attention or not). Triage is itself scored: every triage decision is logged, and a fixed daily sample of declined triggers runs the full frontier cycle in shadow, with resulting beliefs scored normally. This yields triage recall (missed-opportunity rate) and precision against a random-triage baseline, per triage model_id, making the triage model swappable on evidence like everything else. Fable 5 receives assembled context and emits belief updates and proposals. Calibration layer adjusts p. Decision engine derives orders (sizing below) and submits to gates.
- **Daily reconciliation (00:00 UTC; these markets do not close, so the day boundary is defined, not discovered):** the model reads the day's fills, open positions, and originating beliefs; writes the journal entry and tomorrow's plan. No orders are placed from this loop.
- **Weekly review:** calibration audit per strategy/category (auto-generated charts plus model commentary), lesson promotion to semantic memory, strategy GO/NO-GO recommendations against gate thresholds (recommendations only; promotion is a human action, I7).
- **Monthly review:** capital allocation across strategies, model-version evaluation (shadow results), fee/PnL and cost-of-cognition audit, kill-switch test, Postgres backup restore drill.

### 5.9 Model interface (fortuna-cognition)

Provider trait (Artemis pattern):

```rust
#[async_trait]
pub trait Mind: Send + Sync {
    fn id(&self) -> ModelId;                       // e.g. "claude-fable-5"
    async fn decide(&self, ctx: AssembledContext) -> Result<MindOutput>;
}
pub struct MindOutput {
    pub beliefs: Vec<BeliefDraft>,                  // structured, schema-validated
    pub proposals: Vec<ProposalDraft>,              // market, side, max_price, thesis, belief_ref, urgency
    pub journal: Option<JournalDraft>,              // reconciliation cycles only
    pub cost_cents: i64,
}
```

Structured output enforced via tool-use/JSON schema; any schema-invalid output is rejected and logged, never repaired silently. Per-cycle and per-day cost budgets in config; budget breach degrades to mechanical-only and alerts. Tiering: triage model (cheap), synthesis model (Fable 5), both behind `Mind`.

**Sizing is not the model's job.** The decision engine sizes deterministically per the capital hierarchy (5.14): fractional Kelly (default 0.25) on calibrated edge, drawing from the strategy's envelope via reservation, haircut by category calibration quality, capped by gate limits. The model's `urgency` may select execution style within policy, never size.

### 5.10 Calibration layer

Per (model_id, strategy, category): Platt scaling or isotonic regression fit on resolved beliefs from the forward record only; extremization parameter where the weekly audit supports it. Until a category has N >= 50 resolved beliefs, a conservative shrinkage-toward-market prior applies (low-data categories get little autonomous weight). This layer is deterministic code with versioned parameters; parameter updates are config changes recorded in audit.

### 5.11 Signal ingestion subsystem

All non-venue-execution data enters through one funnel: per-source ingest adapters (deliberately dumb: fetch, retry, emit), a normalizer producing a common envelope {source, type, received_at, payload, content_hash} with dedup, and the append-only signals store. Source classes at launch: venue metadata feeds (new markets, settlement notices), Aeolus runs, news/text (RSS, webhooks, NWS discussions), and a macro/event calendar. Two rules govern the layer. Point-in-time: received_at is authoritative, contexts are assembled as-of trigger time, and no signal is ever updated in place, which is what makes decisions replayable. Data-not-instructions: all ingested text is treated as untrusted content; the prompt-injection blast radius is bounded by I6 and the gates regardless. The trigger engine sits on top as the cost-control valve: declarative rules (price-belief divergence, new Aeolus run, market open, keyword webhooks) plus cheap-model triage decide what wakes the frontier model; everything else lands and sleeps until a context assembler reads it. Sources are curated: a source registry (allowlist) with per-source trust tier and domain tags governs what may be ingested at all; trust tiers feed evidence weighting and are themselves updated by per-source belief attribution (a source whose evidence correlates with bad beliefs is demoted on the record). All adapters implement one abstract `Source` trait (poll or push, returns envelopes); anything reachable by any means (RSS, REST, webhook, MCP plumbing, scraper, file drop) hides behind it, so acquiring a new source never touches the core. Adding a source must remain an afternoon of work; any ingestion adapter that wants to be clever is doing the normalizer's or trigger engine's job.

### 5.12 Event model and discovery

**Canonical events.** Beliefs attach to events; markets are projections of events onto venues. Two tables formalize what the belief ledger, cross-venue strategies, and gate check 9 already assume. `events`: event_id, canonical statement, resolution criteria, resolution source, horizon, benchmark_at (event start when known, else expected resolution; anchors CLV snapshots per 5.5), category, status. `market_event_edges`: market_id, event_id, mapping type (direct | negation | bracket-component | conditional-on), confidence, proposed_by (model_id or operator), confirmed_by, created_at. Relational tables with edges, no graph database (Principle 9). Edge confidence tiers gate usage: strategies declare the minimum tier they accept; cross-venue and multi-leg strategies require human-confirmed edges, because a wrong equivalence edge converts an arbitrage into an unhedged position (resolution-criteria divergence is the UMA-style failure mode). The LLM proposes edges; deterministic checks (resolution source match, horizon match) score them; #fortuna-review confirms the high-stakes ones.

**Market-back discovery (primary, daily loop).** Poll venue catalogs for new listings; deterministic prefilter (category allowlist, volume floor, resolution-clarity heuristics, category calibration record); cheap-tier model normalizes survivors into canonical events, matching against the existing events table before creating new rows; edges proposed with confidence; tradability score persisted. New trigger type: "market matched to event with open belief" wakes the decision cycle, which is how the system arrives early on fresh listings.

**World-forward discovery (secondary, budget-capped).** A low-frequency loop synthesizes candidate events from the signals store and writes beliefs attached to watchlist events that have no market edges yet. Watchlist events must declare a resolution source from the source registry at creation; events without a checkable resolution source are marked unscoreable and excluded from calibration and watchlist counts (no beliefs nobody can grade). These cost no capital, are scored against their declared resolution sources regardless (so world-forward calibration is measured before it is trusted), and pre-position the system: when market-back later finds a matching market, a thesis already exists. Hard daily cost cap; this loop is the first thing throttled under budget pressure.

### 5.13 Settlement and event lifecycle (canonical reference model)

This section is the official record of how events, markets, positions, settlements, and beliefs move through their lives. It is normative documentation: design discussions, metrics, and audits use these state names and dispositions. Implementation may simplify internally so long as observable behavior matches this model. Governing principle: settlement is asynchronous and adversarial; FORTUNA never assumes it, only reconciles it. Two truths coexist deliberately: venue truth governs money, canonical event criteria govern beliefs, and divergence between them is recorded, never reconciled away.

**Event lifecycle.**
created -> active -> resolution_pending -> resolved_provisional -> resolved_final
Terminal alternative: dead (voided | source_lost | mutated), reachable from any pre-final state. resolved_provisional may excurse to disputed and return to resolved_final or reversed. Events are created by discovery or operator; resolution_pending begins when the underlying occurs or horizon passes.

**Market lifecycle (venue projection of an event).**
listed -> trading -> (halted <->) expired -> determined -> settled
Terminal alternative: voided (refund path). determined may be reversed by venue correction and re-determined; reversals are new entries superseding old, never edits.

**Position lifecycle.**
opening (intents in flight) -> open -> resolution_pending -> settling -> settled
Alternatives: voided_refunded; disputed (frozen: excluded from PnL and bankroll until final); settled -> reversed -> re-settled on venue correction.

**Belief disposition.** open -> resolved (scored: Brier vs canonical outcome, CLV vs benchmark snapshot per 5.5) | superseded (new belief row) | abandoned (event died: excluded from calibration entirely, scored neither right nor wrong, because a voided market is the world breaking the question, not the model being wrong).

**Settlement entries.** pending -> posted -> confirmed (reconciled against venue balance) | reversed (superseded_by reference). Capital rule: bankroll and account views per 5.14. Exposure accounting: positions in resolution_pending or disputed states REMAIN in exposure at worst-case value (reversal risk is real risk, and excluding them would free gate headroom to re-risk the same event) while being excluded from bankroll until settled.

**Watchdogs.** Settlement-overdue: expected_resolution_at + grace -> alert. Dispute monitor: venue dispute flags and oracle proposal windows are ingested as signals. Divergence detector: venue outcome vs canonical criteria mismatch writes a settlement_divergence on the market_event_edge; PnL follows venue truth, the belief scores against canonical truth, and the edge's confidence takes a documented hit that tightens which strategies may use that market family.

**Stranded-state watchdogs (no orphans).** Every open position and open belief must always have a defined next processor. Watchdogs enforce it: an open position with no fresh open belief and no mechanical owner is an orphan (alert, forced exit evaluation); an event past its horizon but unresolved triggers the resolution watchdog; a belief stale beyond policy (5.5) blocks the comparator and flags any held position. Stranded states are surfaced and dispositioned, never discovered by accident.

**Discrepancy rule (no silent corrections).** Any mismatch between FORTUNA's books and venue truth (missed fill, unseen payout, balance drift) writes an explicit discrepancy record resolved only by a matching entry, an adjustment with reason, or operator escalation. Corrections are new append-only entries referencing the error.

**Lifecycle metrics (derived from lifecycle records).** Settlement lag distribution per venue and category; overdue count and age; void rate per category; dispute rate and dispute duration; settlement reversal count; divergence count per edge family; discrepancy open count and aging; capital-in-limbo (pending settlement value over time); belief abandonment rate. These feed the weekly review and are first-class inputs to market tradability scoring (a category that voids or disputes often is measurably worse regardless of edge).

### 5.14 Capital hierarchy, account views, and sizing

**Allocation vs caps (the per-* untangling).** Capital allocation has exactly two tiers: total bankroll -> per-strategy envelopes, set at the monthly review and recorded in config. Sizing draws only from the strategy's envelope through a reservation ledger: each candidate order reserves capital at gate time and releases it on cancel or position close, so concurrent Kelly sizing cannot jointly over-commit an envelope or the account. Reservations are derived state: rebuilt at boot from open intents and positions during reconciliation, so a crash can never leak a reservation and permanently lock envelope capital. Everything else per-* is a risk CAP, not a capital source: per-venue, per-market, per-canonical-event (via edges), and per-category caps are gate constraints that block regardless of envelope headroom. Models receive no capital tier: minds are advisory, and model trust is expressed exclusively through calibration weight and shadow gating, never through bankroll.

**Account views (the running tally).** Four continuously reconciled numbers: settled (venue-confirmed cash), committed (resting order cost plus active reservations), floating (pending settlements at guaranteed minimum plus open positions at conservative mark), and total = settled + floating. Deployable = settled - committed. These are the dashboard headline and the inputs to every sizing decision.

**Marking policy.** Unrealized PnL and all halt math use conservative-side marks: bid for long exposure, ask for short. If the book is stale beyond a configured age or the spread exceeds a threshold, the position marks at the conservative bound and is flagged wide-mark. Mid-marking in thin books manufactures both false halts and hidden losses; FORTUNA prices its own positions pessimistically and is occasionally pleasantly surprised.

---

## 6. Strategy plugin interface

```rust
#[async_trait]
pub trait Strategy: Send + Sync {
    fn id(&self) -> StrategyId;
    fn kind(&self) -> StrategyKind;                 // Mechanical | Synthesis
    fn stage(&self) -> Stage;                       // Sim | Paper | LiveMin | Scaled (I7)
    async fn on_event(&mut self, ev: &BusEvent, core: &CoreHandle) -> Result<Vec<OrderIntent>>; // BusEvent: bus message, distinct from canonical events (5.12)
    fn metrics(&self) -> StrategyMetrics;           // pnl, fees, clv, exposure
}
```

Launch set (Rung 0):

1. **mech_structural** (Atlas/Nike lineage): YES/NO sum scans, bracket monotonicity, cross-platform divergence on equivalent events. Pure mechanical, no model. Serves as the non-LLM baseline for edge attribution and validates the execution path end to end.
2. **mech_extremes:** favorite-longshot fading at price extremes in sub-$100k-volume markets, maker-only, exploiting the documented retail longshot bias and the fee curve's reward for extreme prices. Ships in v1 WITH the model veto (decision: maximize brain involvement to truly test it). The veto is reduce-only (can suppress or shrink, never add or grow), and every veto is logged and counterfactually scored against the vetoed trade's observable outcome, so veto value-add is a measured quantity within ~60 days, not a belief.
3. **aeolus_eval (signal-under-evaluation, zero capital):** Aeolus has accumulated negative PnL on Kalshi and its forecasts have never been rigorously validated. It does not launch as a trading strategy. Instead, every Aeolus forecast is piped into the belief ledger and scored (Brier vs market-implied baseline, CLV vs benchmark snapshot) with no orders placed. The scoring record decides among four diagnoses, each with a different treatment: (a) forecasts not actually well-calibrated (fix or retire Aeolus), (b) forecasts good but no edge over market consensus, CLV ~ zero or negative (retire as a trading signal, keep as context evidence), (c) real edge eroded by fees/execution (fixable in the harness: maker-only, extreme-price preference), (d) mechanical defects of the lead-hour EMOS-defaults class (audit and re-run). Weather's daily resolution makes this the fastest-feedback category available, so aeolus_eval doubles as the validation vehicle for the entire belief pipeline in Phase 2. Promotion to a live synthesis strategy goes through the full Section 11 gates like everything else; its capacity ceiling (~$4.4M/month category volume) is accepted because its job is proof, not income.
4. **synth_events (paper-only initially):** low-attention, retail-dominated event markets where consensus is weak; pure information-synthesis beliefs. This is the strategy that scales with model improvement; it earns live capital only through the full gate sequence.

---

## 7. Data model summary (L1, Postgres)

Tables: `beliefs` (5.5), `events` and `market_event_edges` (5.12), `journal` (episodic entries, JSON body plus extracted fields), `lessons` (semantic memory with provenance and review dates), `audit` (append-only, typed records: model_call, gate_decision, order, fill, config_change, halt, killswitch_test), `orders`/`fills` (execution mirror), `markets` (point-in-time snapshots with settlement metadata), `signals` (Aeolus runs, news items, with received_at timestamps), `calibration_params` (versioned). One schema per concern (ledger, audit, market data); audit and signals tables partitioned monthly with cold archive. Add `intents` (order intent state machine, 5.4), `settlements` (entry lifecycle, 5.13), `discrepancies` (books-vs-venue mismatches with resolution disposition, 5.13), `price_snapshots` (CLV benchmark schedule, 5.5), `source_registry` (trust tiers and domain tags, 5.11), and `reservations` (envelope capital reservation ledger, 5.14). Nightly backups on ITHACA plus offsite copy.

---

## 8. Observability and operations (L3)

- **Metrics (OpenTelemetry, scraped to a local dashboard):** per-strategy PnL (realized/unrealized), fee/PnL ratio, CLV, rolling Brier and calibration curves, exposure by venue/category/underlying, gate rejection counts by reason, settlement lifecycle metrics (lag, overdue, void rate, dispute rate, reversals, divergences, discrepancy aging, capital-in-limbo per 5.13), order/fill latency, venue API error rates, model cost per day and per decision, context token usage, loop heartbeats, triage recall/precision per triage model, envelope reservation utilization, unwind cost attribution, wide-mark flag counts.
- **Alerts and messaging (Slack, channel-routed):** one bot, severity- and type-routed channels. #fortuna-trading: fills, position opens/closes, per-trade one-liners. #fortuna-alerts: halts, drawdown approaches (80% of limit), reconciliation divergence, venue/provider outages, settlement disputes; halts @-mention the operator. #fortuna-review: interactive items requiring a human (edge confirmations, promotion recommendations, lesson promotions), with approve/reject buttons. #fortuna-digest: daily morning digest, weekly calibration report, monthly review. #fortuna-ops: cost tracking, stale-signal warnings, heartbeat anomalies, infra. Routing is config-driven (TOML); every Slack message is also an audit row, and interactive responses (button presses) are authenticated operator actions logged with actor and timestamp. Channels are private; Slack delivery failures escalate through the dead-man monitor's channel. Two actions are deliberately NOT available via Slack: drawdown-halt re-arm and kill-switch reversal are CLI-only (Slack may request, the CLI confirms); a compromised Slack token must not be able to un-halt a halted system.
- **Kill switch:** Slack command (authenticated, allow-listed chat) and local CLI on ITHACA via Tailscale; both paths talk directly to a tiny standalone process holding venue credentials with cancel/flatten capability, independent of the main runtime (I4).
- **Dashboards:** "The Instrument" aesthetic carried over from Aeolus; positions and beliefs board, calibration board, ops board. Read-only web UI served on Tailscale only.
- **Dead-man heartbeat:** FORTUNA pings an external monitor (off-ITHACA) every minute; missed pings alert via the monitor's own channel. The system cannot report its own death; liveness detection must live outside it.
- **Accounting export:** nightly job exports fills, fees, settlements, and realized PnL per venue class to an immutable ledger file (tax treatment differs materially across event contracts, crypto, and equities; the export is the raw material, not tax advice).
- **Deployment:** primary on ITHACA (systemd units per layer); cognition layer can run anywhere (it is stateless between cycles by design); venue-credentialed processes only on ITHACA. Config in TOML, secrets in environment files outside the repo.

---

## 9. Failure modes and degraded states

| Failure | Behavior |
|---|---|
| LLM provider down / cost budget hit | Cognition pauses; mechanical strategies and risk management continue; alert |
| Venue API down | Affected venue frozen; resting orders cancel-on-reconnect policy per config; alert |
| State divergence vs venue | Strategy freeze, reconcile, human ack to resume |
| Schema-invalid model output | Reject, log, retry once with error feedback, then skip cycle |
| Oracle/settlement dispute on held market | Position flagged, excluded from PnL until resolution, alert (UMA-style risk) |
| Audit write failure | Trading halts (no audit, no trading) |
| Runaway detection trip | Venue halt, human re-arm (I2/I3) |
| ITHACA power/network loss | Resting orders are bounded by gate caps; kill switch reachable via cloud fallback path (small VPS mirror of the killswitch process, decision deferred, see Section 12) |

---

## 10. Security

Venue API keys scoped to trade-only where venues support it (no withdrawal scopes ever). Kill-switch process holds its own credential set. Tailscale-only admin surfaces. Prompt-injection posture: all external text (news, market titles, venue announcements) entering context is treated as data; the model's charter instructs it to never treat in-context content as instructions, and the propose-only interface (I6) bounds the blast radius regardless: the worst a poisoned context can cause is a bad proposal, which still faces gates, caps, and edge floors.

---

## 11. Validation pipeline and GO/NO-GO gates

Stages per strategy (I7). Promotion is a human decision against these thresholds; demotion is automatic on breach.

- **Sim:** deterministic components only (gates, sizing, fee models, adapters against recorded data) plus deterministic simulation testing: seeded fault-injection runs (venue faults, crashes mid-intent, duplicate messages) with invariant assertions. Exit: 100% gate test coverage, replay determinism verified, zero invariant violations across the randomized failure corpus.
- **Paper (>= 30 trading days for mechanical, >= 60 resolved beliefs for synthesis):** maker fills in paper count ONLY when the market trades through the limit price (not touches), with a configurable quantity haircut; touch-fill optimism is the classic paper-trading inflation and would corrupt every gate below; taker paper fills assume crossing the visible book at displayed depth, never mid. GO requires positive CLV (prediction markets) or positive expectancy net of modeled fees; Brier beating market-price baseline on the strategy's categories for synthesis strategies; fee/PnL ratio < 0.35; zero invariant violations.
- **Live-minimum (cap: $500 exposure per strategy):** >= 30 days. GO requires paper metrics holding within tolerance live (slippage-adjusted), reconciliation clean.
- **Scaled:** stepwise exposure increases (2x at a time) gated on rolling 30-day forward metrics. Any drawdown halt resets the step.

Model swaps: new model runs in shadow (full decision cycles, beliefs logged and scored, no orders) for >= 30 resolved beliefs per active category; promotion requires Brier/CLV >= incumbent. Shadow runs operate under their own cost budget and may sample cycles rather than shadowing every one, but only paired contexts (identical AssembledContext to the incumbent) count toward the comparison. Both models' belief ledgers make this a direct, fair comparison.

System-level kill criteria (be honest about NO-GO): if after 90 live days no strategy sustains positive CLV, the synthesis pipeline is shelved and only mechanical strategies run while the thesis is re-examined.

---

## 12. Build phases

- **Phase 0 (core skeleton):** fortuna-core, gates, exec, state, Kalshi adapter port, audit log, Slack, kill switch, config. Exit: mech_structural running in Sim with full replay.
- **Phase 1 (mechanical live path):** paper then live-min for mech_structural and mech_extremes. Exit: first GO promotion; execution path proven with real fills; dashboards live.
- **Phase 2 (belief pipeline):** ledger, context assembler, provider trait with Fable 5, decision cycle, daily reconciliation, scoring jobs, calibration layer. aeolus_eval running: every Aeolus forecast scored as a belief, zero capital. Exit: 60 resolved scored beliefs, calibration report generated, and an evidence-backed verdict on the four Aeolus diagnoses.
- **Phase 3 (the loop closes):** weekly/monthly loops, lesson promotion, Aeolus promotion-or-retirement decision per the Phase 2 verdict, market-back discovery loop with edge confirmation flow, world-forward watchlist loop (capped), synth_events paper, Polymarket US adapter, shadow-mode model comparison harness. Exit: full system operating on its own cadence with human review at promotion points only.

Deliberately deferred: equities/broker adapter (Rung 1, post-$25k), crypto basis module (Rung 2 parking), information-graph storage (only if context assembly measurably fails without it), cloud killswitch mirror (decide in Phase 1), multi-operator anything.

## 13. Open questions for design review

1. Resolved: per-strategy comparators with one shared sizing library (per-strategy auditability without duplicated sizing logic).
2. Resolved: yes. Size scales with resolved-belief count and calibration quality per category; research the fractional-Kelly-under-parameter-uncertainty literature to tune the haircut curve.
3. Resolved: veto ships in v1, reduce-only, counterfactually scored (Section 6).
4. Resolved: abstract Source trait over all acquisition methods (5.11); webhooks, polling, MCP plumbing, scrapers are interchangeable implementations.
5. Resolved: Postgres from day one (Principle 9); kill-switch process remains dependency-free.
6. Naming: resolved. FORTUNA is the system name; loops and components stay descriptive (the mythology budget is spent at the system level).
