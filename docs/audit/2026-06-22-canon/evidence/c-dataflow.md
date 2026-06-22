# Phase-1 Area c — Data flow (end-to-end trace)

Authoritative spec: `docs/spec.md` §4 (data-flow paragraph, line 78), §5.8 (cognition
loops, lines 212-218), §5.2/5.3/5.4 (venue/gate/exec), §5.5/5.9/5.10 (belief/Mind/calibration).
All citations are `path:line` from `rg -n` against the live tree. READ-ONLY review;
no code was modified.

Spec data-flow sentence under audit (spec.md:78):
> "venue/signal data enters the core event bus and is persisted point-in-time. The
> context assembler builds a budgeted context ... The model emits beliefs and proposals.
> The calibration layer adjusts probabilities. The decision engine (deterministic)
> compares calibrated beliefs to live prices, derives candidate orders with sizes, and
> submits them to the gate pipeline. Gated orders execute via the order manager. Fills
> update state. The daily loop reconciles outcomes against beliefs ... logged to the
> audit store at each step."

---

## Ordered data-flow table

| # | Hop | Code site (function/type) — `path:line` | Persisted to (fortuna-ledger table / repo) |
|---|-----|------------------------------------------|--------------------------------------------|
| 1 | Signal ingest (Source trait → normalized envelope + dedup) | `Source` trait `crates/fortuna-cognition/src/signals.rs:161`; `SignalEnvelope` (source,kind,received_at,payload,content_hash) `signals.rs:169`; `normalize_and_dedup()` `signals.rs:231`; content hash `signals.rs:200` | `signals` table via `SignalsRepo::insert()` `crates/fortuna-ledger/src/repos.rs:954` (append-only) |
| 2 | Venue market-data ingest (markets/books → `Market`) | `Venue::markets` `crates/fortuna-venues/src/lib.rs:113`; `Venue::book` `lib.rs:114`; `Market` type `crates/fortuna-venues/src/types.rs:40` | `price_snapshots` (point-in-time book snapshots; migration `20260619000001_price_snapshots_market_at_unique.sql`) |
| 3 | Context assembler (deterministic, budgeted; manifest+hash) | `assemble_context()` `crates/fortuna-cognition/src/context.rs:136`; `ContextManifest` `context.rs:115`; manifest hash stamped into belief provenance `crates/fortuna-cognition/src/beliefs.rs:90` | manifest hash carried in `beliefs.provenance` JSON (replayability per spec 5.7) |
| 4 | Belief creation (Mind output) | `Mind` trait + `decide()` `crates/fortuna-cognition/src/mind.rs:162-164`; `MindOutput { beliefs, proposals, journal, cost_cents }` `mind.rs:117`; `BeliefDraft` `beliefs.rs:73` | `beliefs` table via `BeliefsRepo::insert()` `crates/fortuna-ledger/src/repos.rs:1210` (immutable content; `fortuna_refuse_mutation`) |
| 5 | Calibration (p_raw → p; isotonic / shrinkage) | isotonic PAV `crates/fortuna-scoring/src/pav.rs:34`; `calibration_curve()` `crates/fortuna-cognition/src/beliefs.rs:223`; scope params fetch `crates/fortuna-live/src/compose.rs:66` | `calibration_params` table (per model_id/strategy/category/kind; config-recorded per spec 5.10) |
| 6 | Decision engine / comparator (belief vs price → candidate) | `compare_beliefs_to_markets()` → `EdgeCandidate` `crates/fortuna-cognition/src/cycle.rs:131`; fair_cents = floor(calibrated_p*100) `cycle.rs:156`; `EdgeCandidate`→`Proposal` (legs+fair_value) `crates/fortuna-runner/src/synthesis.rs:272,285` | not persisted (unsized intermediate); proposal audited at runner `runner.rs:1098` |
| 7 | Sizing (harness, NOT model) | `Runner::handle_proposal` sizing: affordability + haircut-Kelly `crates/fortuna-runner/src/runner.rs:1109-1185` (Kelly `kelly_contracts`, fraction × calibration quality, fail-closed to 0) | `sizing` audit row `runner.rs:1159` |
| 8 | **Gate pipeline (the split)** — `CandidateOrder` → sealed `GatedOrder` | `CandidateOrder` built `runner.rs:1214`; `Runner::evaluate_gates` → `GatePipeline::evaluate` `runner.rs:1226,1571`; pipeline checks 1-11 + seal `crates/fortuna-gates/src/pipeline.rs:227-258`; `GatedOrder::assemble` (only constructor) `pipeline.rs:255` | one `GateCheckRecord` per check → `audit` table (`gate_decision`) `runner.rs:1227-1233` |
| 9 | Order manager / execution (sealed order → venue) | `OrderManager::submit / submit_grouped / submit_group_concurrent` take `GatedOrder` only `crates/fortuna-exec/src/manager.rs:301,309,440`; `venue.place(order)` `manager.rs:371,514`; `Venue::place(GatedOrder)` `crates/fortuna-venues/src/lib.rs:115` | `intent_events` table via `PgIntentJournal::append()` `crates/fortuna-ledger/src/intent_journal.rs:42` (state machine: created→submitted→acked→filled) |
| 10 | State update (fill → positions/reservations) | `OrderManager::ingest_fill` `crates/fortuna-exec/src/manager.rs:593`; `PositionBook::apply_fill` `crates/fortuna-state/src/positions.rs:164`; runner `drain_fills` `runner.rs:1573` | `intent_events` (`FillApplied`); positions are in-memory, reconciled vs venue truth (spec 5.4) |
| 11 | Settlement (notices → entries / position resolution) | runner `process_settlements` `runner.rs:1061`; `PositionBook::apply_settlement` `crates/fortuna-state/src/positions.rs:255`; `SettlementsRepo::insert_entry` `crates/fortuna-ledger/src/repos.rs:334` | `settlement_entries` table (append-only; pending→posted→confirmed as new rows) |
| 12 | Scoring (resolve beliefs: status/outcome/brier/clv_bps; CLV/Brier) | scoring assembly `crates/fortuna-scoring/src/scorecard.rs:136`; resolve loop `crates/fortuna-live/src/daemon.rs:4909-5022`; `score_bracket` (Brier) `daemon.rs:4924`; `compute_clv_bps` `daemon.rs:5008`; set-once writer `BeliefsRepo::resolve_and_score` `crates/fortuna-ledger/src/repos.rs:1309` (`UPDATE beliefs SET status,outcome,brier,clv_bps WHERE outcome IS NULL`) | `beliefs` 4 scoring columns (I5 scoped exception, `fortuna_beliefs_guard`); `scorecards` table via `ScorecardsRepo::insert_scorecard` `repos.rs:2949` |
| — | Audit log (every step) | `AuditWriter::append` `crates/fortuna-ledger/src/audit.rs:54`; runner `self.audit(...)` throughout (`cognition`, `proposal`, `sizing`, `gate_decision`, `order`, `ttl_cancel`) | `audit` table (append-only, monthly-partitioned, `audit_append_only` trigger; I5) |

---

## Decision / Execution split (the critical question)

Per spec Principle 1 (spec.md:23) and I6 (spec.md:45): the model proposes; no code path
carries model output to a venue except through the deterministic gate pipeline, and orders
reach venues only as the sealed `GatedOrder` type.

### Where the split physically happens
`crates/fortuna-runner/src/runner.rs:1187-1283` (`handle_proposal`, Phase A):
- The model veto (if any) is **reduce-only**, audited, and runs strictly **before** the
  gates: "the gates never consult the model (I1); the model never sees the gates"
  (`runner.rs:1187-1190`). Sizing (`runner.rs:1109-1185`) is the harness's job (I6).
- A `CandidateOrder` (mutable plaintext struct, `pipeline.rs:39`) is built `runner.rs:1214`
  and handed to `evaluate_gates` `runner.rs:1226`.
- A leg becomes a `GatedOrder` **only** via `outcome.gated` (`runner.rs:1246-1282`); only the
  sealed `Ok(gated)` arm is pushed into `staged`. Rejections produce **no placeable artifact**.
- Only `Vec<GatedOrder>` crosses into Phase B and reaches the venue
  (`runner.rs:1311-1316` → `manager.submit_group_concurrent`).

### Is `GatedOrder` sealed? YES.
`crates/fortuna-gates/src/order.rs`:
- Struct has **all-private fields** (`order.rs:20-30`).
- The **only** constructor is `pub(crate) fn assemble` (`order.rs:37`), called solely at the
  end of the pipeline after every check passes (`pipeline.rs:255`). Comment: "THE ONLY
  CONSTRUCTOR. pub(crate): callable solely from the gate pipeline" (`order.rs:33-36`).
- `#[derive(Serialize)]` **only**, never `Deserialize` (`order.rs:19`); a Deserialize impl is
  documented as a forbidden constructor-bypass (`order.rs:8-10,35`). Confirmed no Deserialize
  impl exists for either sealed type.
- Type-level enforcement pinned by **compile-fail doc-tests** in
  `crates/fortuna-invariants/src/lib.rs:20-31` (forging `GatedOrder {}` and requiring
  `DeserializeOwned` both must fail to compile), plus a path-witness guarding against vacuous
  pass (`lib.rs:12-13`).

### Does `Venue::place` accept ONLY `GatedOrder`? YES.
Trait: `async fn place(&self, order: GatedOrder)` `crates/fortuna-venues/src/lib.rs:115`.
All production impls take `GatedOrder` exclusively:
- Kalshi `crates/fortuna-venues/src/kalshi/adapter.rs:571`
- Sim `crates/fortuna-venues/src/sim.rs:1033`
- Paper `crates/fortuna-paper/src/lib.rs:612`
- Paper-on-live `crates/fortuna-paper/src/paper_live.rs:153` (routes to `self.paper.place`)
- Polymarket-US stub `crates/fortuna-venues/src/polymarket/mod.rs:87` (fixture-gated refusal)
The exec manager's submit entry points (`manager.rs:301,309,440`) also accept `GatedOrder`
exclusively — there is no looser submit signature. Runtime invariant test `i1_universal_gate`
(`crates/fortuna-invariants/tests/i1_universal_gate.rs:166-241`) proves every venue-reaching
order carries a complete ALL-pass audit trail and rejections yield nothing placeable.

### Perp arm (spec 5.15) — same discipline, separate sealed type.
`GatedPerpOrder` `crates/fortuna-gates/src/perp.rs:120` has all-private fields; only constructor
is `pub(crate) fn assemble` `perp.rs:137`, called at end of `evaluate_perp` `perp.rs:364`;
Serialize-only (`perp.rs:117-119`); compile-fail pins at `fortuna-invariants/src/lib.rs:42-55`.
The kinetics adapter `place(&GatedPerpOrder, ...)` accepts only the sealed type
(`crates/fortuna-venues/src/kinetics/adapter.rs:111-113`); TIF/post_only are execution policy
(I6), not gate scope. The perp gate shares the same `GatePipeline` state — same halt flags and
I3 buckets (`perp.rs:1-34`, `pipeline.rs:180-187`) — so a breach on either arm halts both.

### Bypass search (test code, CLI, kill-switch, paper, recorder) — no leaks found.
- **CLI** (`crates/fortuna-cli/src/main.rs`): process lifecycle only (start/stop/logs/halt).
  No order-placement, no `place`, no `CandidateOrder`/`submit`. The spec I1 mention of "manual
  CLI" orders is *as-intended*; *as-built* there is no manual-order CLI surface to leak through.
- **Kill-switch (I4 exemption, intentional)** `crates/fortuna-killswitch/src/lib.rs`:
  - Event-contract path is **freeze-and-cancel only** — it constructs no event-contract orders
    ("placing requires a `GatedOrder` (I1)") (`lib.rs:10-14,149-156`).
  - The only **place-capable** path is PERP flatten `freeze_cancel_perp_and_flatten`
    (`lib.rs:434`): it builds a `PerpCandidateOrder` (`lib.rs:673`) and routes it through the
    **real perp gate** `gates.evaluate_perp(&candidate,&inputs).gated` (`lib.rs:697`), placing
    only if the gate seals it — "THE SEAL: a close is a GatedPerpOrder only if the gate builds
    it" (`lib.rs:696`). The switch is a **consumer** of the seal, not a bypass. It remains
    I4-independent (no Postgres/cognition/event-loop; flat-file journal) (`lib.rs:1-8`). This
    is the documented spec-5.4 emergency-flatten exemption (best-effort, no flatten-planner),
    **not** a gate bypass.
  - Note this nuance vs the spec-I4 wording ("flattens ... all positions"): event-contract
    positions are **not** auto-flattened by the switch (operator venue-UI/CLI exits them);
    only perp positions are flattened, and even those traverse the gate. This is a deliberate
    "construct no order outside the gate" choice, documented at `lib.rs:11-14`.
- **Paper-on-live** `crates/fortuna-paper/src/paper_live.rs`: reads live market data
  (`self.read`, `paper_live.rs:145-151`) but `place` routes to the in-memory paper engine
  (`paper_live.rs:153-155`) — orders never hit a real venue. Pinned by
  `crates/fortuna-invariants/tests/i_paper_live_no_real_order.rs` ("Paper-on-live must never
  place or cancel a real Kalshi order").
- **Recorder** `crates/fortuna-recorder/src/`: read-only market-data capture; no `place`,
  no gate, no submit (search returned nothing).
- **Test code**: every `venue.place(...)` in tests is fed a pipeline-produced/`gated_*`
  helper `GatedOrder`; no test forges one (it cannot — fields private, no Deserialize).

### Verdict
**The decision/execution split HOLDS.** It is enforced at the type level, not by convention:
`GatedOrder`/`GatedPerpOrder` have private fields, a single `pub(crate)` constructor invoked
only after the full fail-closed gate pipeline passes, Serialize-only (no Deserialize), and every
`Venue::place` (trait + all impls) and every exec-manager submit entry accepts only the sealed
type. The model's output is `ProposalDraft`/`BeliefDraft` (cognition only); sizing, timing,
order type, and execution are all harness-side; the model veto runs before and is invisible to
the gates. The single place-capable kill-switch path (perp flatten) consumes the seal rather
than bypassing it, and the paper-on-live composite executes against an in-memory engine. **No
leak found** in production code, CLI, kill-switch, paper, recorder, or test code.

---

## Open questions
- **Normal-path perp execution is not yet wired.** No production caller builds a
  `PerpCandidateOrder`/`GatedPerpOrder` for *opening* trades — searches across
  `fortuna-exec`, `fortuna-runner`, `fortuna-live` found none; the only production constructor
  is the kill-switch flatten. So the perp seal is currently exercised by the gate tests and the
  kill-switch only; the live perp *trading* loop appears unbuilt (consistent with Kinetics being
  an operator-directed extension, spec 5.15). Not a leak — flagged as a coverage/maturity gap.
- **Belief-scoring writer location vs spec phrasing.** The set-once scoring update lives in
  `fortuna-live/src/daemon.rs:4909-5022` (daemon resolve loop) calling
  `BeliefsRepo::resolve_and_score` (`repos.rs:1309`), rather than in a standalone
  `fortuna-scoring` job; `fortuna-scoring` provides the math (PAV, Brier, scorecard). Behavior
  matches spec 5.5 I5-exception (set-once where `outcome IS NULL`); the crate boundary differs
  from the "scoring job" mental model in spec 5.5. As-built vs as-intended: divergence is
  organizational, not behavioral.
- **`price_snapshots` write path not directly cited.** Hop 2's snapshot table exists (migration)
  and the CLV path reads snapshots, but the exact snapshot-insert repo fn was not located in
  this pass (the recorder/daemon likely writes it). Flagged for a follow-up cite.
