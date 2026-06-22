> ARCHIVED 2026-06-22. Superseded by ARCHITECTURE.md. Kept for provenance; not source of truth.

# FORTUNA — End-to-End System Overview

> Demo-ready walkthrough of the whole system and each layer, grounded in the
> current code (main). The authoritative design is `docs/spec.md` (v0.9); this
> doc is the **narrative** companion — how a signal becomes a trade and how the
> harness keeps the model from ever touching money directly. Component reference
> lives in `docs/architecture.md`; the constitution is `CLAUDE.md`.

## 1. What FORTUNA is

A **model-driven autonomous trading system** in Rust. A language model *proposes*
trade ideas; a deterministic harness decides whether, how much, when, and how to
execute. The architecture is built so the model can **never** mutate external
state — sizing, timing, order type, and execution belong to the harness (I6).

- **16 crates** under `crates/`. Money is integer `Cents` (i64); `f64` appears
  only for probabilities inside cognition. The core event loop is single-threaded
  and deterministic, with all time from an injected `Clock` trait (no
  `SystemTime::now()` in the core) — so any run is **byte-exact replayable** and
  testable under DST.
- **Trading posture:** paper / sim / shadow / demo with mock funds. Live capital
  requires an explicit **operator promotion (I7)** — the rails are built; the
  human is never simulated.

## 2. The seven invariants (the constitution; `crates/fortuna-invariants/`, protected)

| # | Invariant | How it's enforced in code |
|---|---|---|
| **I1** | Universal gate — every order passes the same deterministic pipeline; the model cannot bypass it | `GatedOrder`/`GatedPerpOrder` are **sealed types**: private fields, `pub(crate)` `assemble()` constructor only in `fortuna-gates`, **no `Deserialize`**. `Venue::place()` accepts only the sealed type ⇒ compile-time I1. |
| **I2** | Drawdown halts with **human** re-arm (no auto-resume) | `DrawdownMonitor` (`fortuna-state`) sets a sticky `HaltScope::Global`; only `GatePipeline::rearm()` (CLI/operator) clears it — equity recovery, time, day-roll, config-reload never do. |
| **I3** | Runaway detection — **dual** token-buckets per venue **and** per market; breach is a halt, not a throttle | `fortuna-gates/rate.rs` buckets; a breach calls `set_halt()` (sticky) — refill never un-halts. |
| **I4** | Out-of-band kill switch — must not depend on cognition, event loop, Postgres, or any LLM | `fortuna-killswitch` is a **standalone binary** depending only on `fortuna-core` + `fortuna-venues` + `fortuna-gates` (no sqlx/ledger/cognition — guarded by a cargo-metadata test). Flat-file fsync'd journal; durable `KILLSWITCH_REVOKED` sentinel. |
| **I5** | Append-only audit — replay any decision; never deleted/updated in place | INSERT-only repos **and** Postgres triggers `fortuna_refuse_mutation` / `fortuna_beliefs_guard` reject UPDATE/DELETE. "Corrections" are superseding rows. Scoped exception (C1): a belief's four scoring columns are set **once** post-resolution. |
| **I6** | Propose-only model — zero tools that mutate external state | `fortuna-cognition` has **no dependency** on `fortuna-venues`/`-exec`/`-state`/`-runner`. `MindOutput` is exactly `{beliefs, proposals, journal, cost_cents}`; `#[serde(deny_unknown_fields)]` rejects any smuggled `contracts`/`order_type`/`tool_calls`. |
| **I7** | Promotion gates — no live capital without forward validation; no model swap without shadow comparison | Stage order `Sim < Paper < LiveMin < Scaled`; promotion records are operator-only and must be contiguous from Sim; model swap is **recommendation-only**. |

Each invariant is an **executable test** in the protected crate (13 files:
`i1_universal_gate`, `i2_drawdown_human_rearm`, `i3_runaway_halt`,
`i4_killswitch_independence`, `i4_killswitch_revocation`, `i5_audit_append_only`,
`i6_propose_only_mind`, `i6_persona_propose_only`, `i7_promotion_gates`, plus
`perp_i1..i4`). `scripts/check-protected-invariants.sh` blocks any weakening of an
existing assertion (additions only).

## 3. The end-to-end flow (signal → trade → settlement)

```
External feeds (NWS, RSS, calendar, Kinetics perps, Aeolus forecast)
   │
   ▼  fortuna-sources / fortuna-cognition
[1] INGESTION   Source → RawSignal → IngestionScheduler.tick()
                hard gate: future-dated / republished(UNIQUE source+content_hash) / over-volume
                → SignalEnvelope (immutable; received_at is the sole temporal authority)
   │
   ▼  fortuna-cognition
[2] TRIGGER     TriggerEngine: declarative rules (NewSignalKind, KeywordMatch,
                PriceBeliefDivergence, MarketOpen); per-event serialization + debounce
                → Fire | CoalescedInFlight | CoalescedDebounce
   │
   ▼
[3] CONTEXT     assemble_context(): verify every item hash, filter point-in-time
                (≤ trigger_at), sort by section priority, pack within char budget
                → AssembledContext (rendered prompt + manifest + manifest_hash)
   │
   ▼  I6 boundary — the ONLY place the model is consulted
[4] MIND        Mind.decide(ctx) → MindOutput {beliefs[], proposals[], journal?, cost_cents}
                StubMind (DST/no-key) | AnthropicMind (Synthesis/Mid/Triage tiers)
                budget checked BEFORE the call; deny_unknown_fields; p ∈ (0,1)
   │
   ├─ beliefs ──▶ [5] CALIBRATION: raw p → calibrated p (Platt/isotonic, or shrink to
   │              market prior when N<50); deterministic, no mid-cycle learning
   │                   │
   │                   ▼ fortuna-cognition comparator
   │              [6] COMPARE: calibrated beliefs ⋈ market_event_edges ⋈ live quotes
   │                   → EdgeCandidate[] (unsized; two-sided: buy YES/NO when fair > ask + floor)
   │                   │
   │                   ▼ fortuna-state sizing (Kelly is an INPUT, not the decision)
   │              [7] SIZE: kelly_contracts(calibrated_p, price, fraction × calibration_quality,
   │                   headroom, cap) → integer contracts (floored, capped, fail-closed on NaN)
   │
   └─ proposals ─▶ (same derive→size path; the model's max_price_cents is only a hint)
   │
   ▼  fortuna-gates — I1
[8] GATE        GatePipeline.evaluate(): 10 ordered, fail-closed checks
                1 halts · 2 capital · 3 position caps · 4 price band · 5 size ·
                6 fee-adjusted edge floor · 7 dual token-bucket (I3) · 8 idempotency ·
                9 same-event exposure · 10 internal netting
                every check writes a GateCheckRecord (pass or reject) → audit (I5)
                → GatedOrder (sealed) | GateRejection
   │
   ▼  fortuna-exec
[9] EXECUTE     IntentJournal.append(Created) BEFORE any network call → reserve capital
                → Venue.place(GatedOrder) → Acked → FillApplied (dedup by fill_id)
                state machine recovered deterministically from the journal on restart
   │
   ▼  fortuna-state
[10] STATE      PositionBook.apply_fill() (checked Cents arithmetic; YES/NO never netted)
                AccountView rebuilt (settled/committed/floating/deployable)
                DrawdownMonitor.check() → halt on breach (I2)
   │
   ▼  fortuna-venues + fortuna-ledger
[11] SETTLE     settlements_since → Position.apply_settlement; corrections are NEW
                superseding rows; discrepancies opened, never silently fixed (I5)
```

Every step is deterministic and produces append-only audit rows sufficient to
**replay the entire decision lineage** after the fact.

## 4. Each layer, in depth

### 4.1 Ingestion (`fortuna-sources`, `fortuna-cognition/signals.rs`)
- `Source` trait → `RawSignal {kind, payload(JSON), received_at}`. Adapters:
  `NwsSource`, `RssSource`, `CalendarSource`, `AeolusSource`.
- `IngestionScheduler.tick(now)` polls each due source with **per-source
  isolation** (health: Healthy/Degraded/Quarantined; exponential backoff;
  quarantine clears only via operator `rearm()` — the I2 spirit). A
  `StructuralValidator` on the live path enforces three hard drops:
  **future-dated**, **republished** (`UNIQUE(source, content_hash)` via
  `DedupIndex`), **over-volume**. Telemetry (`IngestionTelemetry`/`FunnelCounts`)
  feeds the ROTA V1–V3 boards.
- `normalize_and_dedup()` checks the `SourceRegistry` allowlist (fail-closed) and
  emits an immutable `SignalEnvelope` (ULID id; SHA-256 content hash). **Untrusted
  data (spec 5.11):** payloads are hashed, stored, pattern-matched — never executed
  and never treated as instructions.

### 4.2 Trigger (`fortuna-cognition/signals.rs`)
- `TriggerEngine` applies declarative `TriggerRule`s and **serializes one decision
  per event**: `request_cycle()` returns `Fire` only when no cycle is in flight and
  the debounce window has passed; otherwise it coalesces. A news burst becomes one
  decision — the cost-control valve.

### 4.3 Context (`fortuna-cognition/context.rs`)
- `assemble_context()` is deterministic and manifest-audited: every offered item's
  claimed hash is **recomputed** (mismatch is refused — replayability is sacred),
  items after `trigger_at` are excluded (point-in-time), the rest are packed by
  `SectionKind` priority within a char budget. Output carries a `manifest_hash`
  that lands in belief provenance. Item bodies render inside delimited
  `<context-item>` blocks — quoted data, never prose instructions.

### 4.4 The Mind — I6 boundary (`fortuna-cognition/mind.rs`)
- `Mind::decide(AssembledContext) → MindOutput`. Implementations: `StubMind`
  (scripted, for DST + no-key boot) and `AnthropicMind` over `ReqwestMindTransport`
  with three tiers — **Synthesis** (deep), **Mid** (daily reconciliation/reviews),
  **Triage** (cheap pre-frontier gate) — sharing the `[cognition]` per-cycle/daily
  `CostBudget` (checked **before** each call; recorded after; rolls at 00:00 UTC).
- `MindOutput = {beliefs[], proposals[], journal?, cost_cents}`. `ProposalDraft`'s
  entire surface is `{market, side, max_price_cents, thesis, belief_ref, urgency}`
  — **no sizing/timing/order-type fields exist**, and `deny_unknown_fields` rejects
  any attempt to smuggle them. The model emits beliefs (a probability + evidence +
  horizon) and *unsized* proposals; the harness owns everything else.

### 4.5 Beliefs, calibration, scoring (`fortuna-cognition/beliefs.rs`, `calibration.rs`, `scoring.rs`)
- A `BeliefDraft` is validated (`p, p_raw ∈ (0,1)`), provenance-stamped by the
  harness, and persisted **immutably**; updates are superseding rows. Post-
  resolution a scoring job sets the four scoring columns once (`status, outcome,
  brier, clv_bps`) — the C1/I5 scoped exception.
- Calibration is per `(model_id, strategy, category)`: Platt/isotonic when
  `N ≥ 50`, conservative **shrinkage toward the market prior** when low-data, fully
  to 0.5 when a scope is unwired (no edge priced). Scalar claims use a
  `PredictiveDistribution {Binary, Categorical, Scalar}` with a swappable
  `ScoringRule` (`CrpsPinballRule`); `scalar_beliefs`/`belief_scores` persist them.
- Two resolve+score loops run at the daily boundary:
  `resolve_and_score_funding_beliefs` and `resolve_and_score_weather_beliefs`
  (the Aeolus weather loop, closing F9 `aeolus_reliability::score_reliability`
  against realized NWS temperatures).

### 4.6 Gates — I1/I2/I3 (`fortuna-gates`)
- `GatedOrder` (and `GatedPerpOrder`): private fields, `pub(crate) assemble()`,
  no `Deserialize`, no `From` — **unconstructible outside the crate**.
- `GatePipeline.evaluate()` runs ten ordered checks fail-closed (first rejection
  stops), emitting a `GateCheckRecord` per check whether pass or reject. Halts
  (`HaltFlags` global/strategy/venue) block at check 1; dual token-buckets (per
  venue + per market) are check 7 and a breach becomes a sticky halt.

### 4.7 Execution (`fortuna-exec`)
- Append-only `IntentJournal`: `Created` is journaled **before** any network call;
  lifecycle `Created → Submitted → Acked → PartiallyFilled → Filled | Cancelled |
  Rejected | BootClosed`. `OrderManager::recover()` folds the journal to rebuild
  the exact state machine — identical whether fresh or crash-restarted. One working
  order per `(strategy, market, side)`; fills idempotent by `fill_id`, client order
  ids derived from intent id (at-least-once delivery safe). The flatten planner
  lives here (the *normal* path; the kill switch's emergency flatten is planner-free).

### 4.8 State (`fortuna-state`)
- `Position` carries per-side `Lot {qty, cost_basis}`, `realized_pnl`, `fees_paid`,
  and a lifecycle (`Open | ResolutionPending | Disputed`). YES and NO are **never
  netted** (a $1 settlement identity arbs depend on). `AccountView` =
  settled/committed/floating/deployable + exposure-in-limbo. `ReservationLedger`
  holds per-strategy capital envelopes, rebuilt at boot from open intents so a
  crash can't leak a reservation. `DrawdownMonitor` enforces I2.
- **Sizing** (`sizing.rs`): `kelly_contracts()` computes the Kelly fraction in f64,
  converts **once** to integer ppm, then does widened integer math — no float
  arithmetic on money. Fraction = base × calibration_quality (NaN/out-of-range ⇒ 0,
  fail-closed).

### 4.9 Money (`fortuna-core/money.rs`)
- `Cents(i64)` newtype with `checked_add/sub/mul/neg/abs/sum` returning
  `Result<_, MoneyError>` (overflow is an error, never a panic/wrap). Decimal only
  at venue boundaries with **rounding always against us** (`from_dollars_floor` for
  costs, `from_dollars_ceil` for proceeds/fees).

### 4.10 Venues (`fortuna-venues`, `fortuna-paper`)
- `Venue` trait: `place(GatedOrder)`, `markets`, `book`, `cancel`, `positions`,
  `fills_since`, `settlements_since`, `account`, `fee_model`. Adapters:
  - **`kalshi/`** (event contracts) — RSA-PSS signing, V2 REST DTOs, series-scoped
    catalog, cancel-then-confirm (works around a venue body bug), 409→lookup-by-
    client-id. *Sim-development clearance only* until the operator fixture checklist
    is signed off.
  - **`kinetics/`** (perps) — creds-less public market/funding-estimate reads;
    `GatedPerpOrder`; reduce-only requires IOC/FOK (GTC refused before the wire);
    liquidation fills are a **distinct class** the caller must handle.
  - **`SimVenue`** — the DST workhorse: one canonical YES book (NO mirrored), with
    eight **seeded** fault classes (api_error, place_timeout_but_placed, reject,
    ack_delay, drop_fill, dup_fill, cancel_timeout_{not_}cancelled) rolled in a
    fixed order so same seed ⇒ identical behavior.
  - **`PaperVenue`** (`fortuna-paper`) — honest GO/NO-GO numbers via the same
    `Venue` interface. `apply_public_trade()` fills a resting maker **only when the
    market trades strictly THROUGH the limit, never at touch** (spec 11), with a
    configurable quantity haircut. This is mutation-pinned against a **real recorded
    public print** (`fixtures/kalshi/trades__public_recorded.json`): a 3¢ print
    fills a 4¢ buy (through), never a 3¢ buy (touch).
- `FeeModel`/`ScheduleFeeModel`: versioned schedules, three formula types
  (quadratic / flat-bps / tiered), category multipliers, rounding always against us;
  the live adapters reconcile modeled vs charged fees and open a discrepancy on drift.

### 4.11 Ledger (`fortuna-ledger`, Postgres via sqlx)
- Five migrations (`initial`, `discovery`, `personas`, `scalar_beliefs`,
  `funding_rates_historical`). Append-only tables (audit, beliefs,
  market_event_edges, intent_events, fills, settlement_entries, discrepancies,
  reservation_events, halt_events, price_snapshots, signals, …) are protected by
  the `fortuna_refuse_mutation` trigger; `beliefs` by the stricter
  `fortuna_beliefs_guard` (content immutable; only the four scoring columns may be
  set once from NULL). **`AuditWriter::append()` is the I5 contract**: an `Err`
  means *no audit ⇒ trading halts* (the runner wires append-failure to a global
  halt; the DST asserts it). The dashboard uses a separate `connect_readonly_pool`
  so a read can never queue against the audit writer.

### 4.12 Safety rails (`fortuna-killswitch`, I4)
- A **standalone binary**, structurally independent of Postgres/cognition/event-loop
  (a cargo-metadata invariant test enforces the forbidden-dependency list). Actions:
  - `freeze_and_cancel` — cancel every open event-contract order; touch no
    positions (position exits are operator venue-UI/CLI flows).
  - `freeze_cancel_and_report_positions` — the above + report open positions.
  - `freeze_cancel_perp_and_flatten` (spec 5.15) — cancel every perp order, then
    close each non-flat position with a **reduce-only IOC that is itself a sealed
    `GatedPerpOrder`** (the switch is a *consumer* of the gate seal, never a
    constructor — I1 holds even on the emergency path); planner-free, fail-closed.
  - Every action appends a JSON line to a **flat fsync'd journal**; a kill writes a
    durable `KILLSWITCH_REVOKED` sentinel. In the daemon, `RevocationHaltPoller`
    wraps the Postgres halt poller and reports a global halt whenever the sentinel
    is present (a `std::fs::exists` check, no DB) — so a revoked switch **survives
    restart** and only `clear-revocation` + restart re-arms.
  - CLI verbs: `self-test`, `freeze [--venue kalshi]`, `flatten-perps`,
    `clear-revocation`, `report`; typed exit codes (0 ok, 4 fail-closed, 5
    incomplete, 6 revocation-write-failed, …).

### 4.13 The live daemon (`fortuna-live`)
- `drive()` is the deterministic, single-threaded loop. Wall time enters **once**
  (`RealClock` at boot, threaded as an `Arc<SimClock>`); `RealCadence` advances the
  sim clock by slept wall-ms — everything inside reads the injected clock. It runs
  in **segments**; each segment polls halts every ≤500 ms (dedup-audited), ticks the
  runner on its interval, and between segments drains/persists beliefs and refreshes
  the synthesis edge set. Opt-in wirings (default-off ⇒ byte-unchanged daemon):
  the **live PerpTick producer** (`run_perp_tick_producer`, creds-less public
  Kinetics GETs, gated on `[perp_event_basis_v2]`) drained at each segment head via
  `perp_tick_rx → inject_perp_tick`; the funding poller; world-forward discovery;
  personas. At the **daily boundary** it runs reconciliation (reads the day, writes
  the journal, places **no** orders — I6), the digest, and the resolve+score loops;
  **weekly/monthly reviews** emit GO/NO-GO **recommendations only** (I7). Boot is
  fail-closed: `validate_bootable` + `resolve_kalshi_demo_creds` (credential IO
  isolated at the binary edge, PEM wrapped in `Secret`, errors name the var/path
  never the value) + the IO-free `build_kalshi_demo_transport`.

### 4.14 Observability (`fortuna-ops`)
- A deterministic `MetricsRegistry` (integer counters/gauges; Prometheus 0.0.4
  exposition; a `telemetry_board` view). **ROTA** is a read-only operator console on
  the metrics listener (`/api/rota/v1/*`, all GET — a route test pins 405 on every
  mutating method), capability-optional (renders "unavailable", never 500, when a
  pool is absent), reading pre-shaped views + its own read-only-pool queries. The
  **Slack router** is send-only, fail-closed at construction (every message kind
  must map to a configured channel), and every outbound message also writes an audit
  row. A **dead-man pinger** heartbeats an external monitor on injected time.

### 4.15 The proof system (`fortuna-invariants`, DST, the battery)
- **Invariant tests** (13 files, additions-only, protected): I1–I7 + `perp_i1..i4`
  encode the constitution as executable contracts (e.g. property tests over
  thousands of seeds that every order outcome carries a coherent gate trail; that a
  drawdown breach locks until operator re-arm; that the kill-switch sentinel halts
  and survives restart; that smuggled sizing fields are schema-rejected; that
  promotion needs contiguous operator records).
- **DST** — 10 harnesses (`dst` core, `synthesis_dst`, `settlement_dst`, `perp_dst`,
  `funding_forecast_dst`, `perp_event_basis_dst`, `persona_dst`,
  `persona_orchestrator_dst`, `ingest_dst`, `daemon_smoke`) run **seeded,
  reproducible** scenarios with fault injection, assert byte-identical replay, and
  several enforce **per-arm hit accounting** (the suite fails if a code arm never
  fires). A red seed prints itself; `scripts/replay.sh --seed <N>` reproduces it;
  the fix's seed is pinned forever in `crates/fortuna-core/dst-corpus/` (6 seed
  files, 14 pinned seeds today).
- **The battery (definition of done):** `cargo fmt --all --check` +
  `clippy --workspace --all-targets -D warnings` + `cargo test --workspace` +
  `scripts/run-dst.sh` + `check-protected-invariants.sh`. Doctrine: **green is not
  verification** — a finding isn't trusted until a *mutation* (flip `<`→`<=`, drop a
  guard, `Some(rx)`→`None`) is shown to RED the suite.

## 5. Crate map

| Crate | Role |
|---|---|
| `fortuna-core` | Clock, `Cents`, ULIDs, deterministic bus + replay. No IO. |
| `fortuna-gates` | Sealed `GatedOrder`/`GatedPerpOrder`, the 10-check pipeline, halts, token-buckets (I1/I2/I3) |
| `fortuna-exec` | Append-only intent journal, IntentGroup, execution policy, flatten planner |
| `fortuna-state` | Positions, account views, marks, reservations, Kelly sizing, drawdown |
| `fortuna-venues` | `Venue` trait, fee model, sim venue (faults), `kalshi/`, `kinetics/` |
| `fortuna-paper` | Paper venue — trade-through realism (fills only through, never at touch) |
| `fortuna-ledger` | All Postgres: migrations, append-only tables + triggers, audit writer, repos |
| `fortuna-cognition` | Sources/ingestion, triggers, context, `Mind` (Stub/Anthropic), calibration, scoring, personas, discovery, aeolus |
| `fortuna-sources` | Signal source adapters (NWS, RSS, calendar, Aeolus) + the ingestion scheduler |
| `fortuna-runner` | `Strategy` trait, `Proposal`, the composed sim runner, perp_event_basis strategy |
| `fortuna-live` | The live daemon — `drive()` loop, composition, boot gate, the binary |
| `fortuna-ops` | ROTA dashboard, metrics, Slack routing, dead-man pinger |
| `fortuna-killswitch` | Standalone kill switch (I4) — freeze/flatten/revoke, flat-file journal |
| `fortuna-invariants` | **Protected.** Executable I1–I7 (+ perp) tests |
| `fortuna-recorder` | Provenanced fixture recording (secret-redacted) |
| `fortuna-cli` | Operator CLI (halts, etc.) |

## 6. Status

Code-complete and **battery-certified green** (fmt + clippy + `cargo test
--workspace` + DST: 14 corpus + 2000 random seeds, zero invariant violations).
Live capabilities: event-contract paper trading, perps data-collection (no perp
trading), the Aeolus weather reliability loop, the ROTA console, and the kill
switch. The only open items are **operator-only**: a live demo-boot with demo
creds, an `.env.example` host reconcile, and an optional broader recorder-e2e
fixture. Live capital remains gated behind the I7 promotion ladder.
