# Area 1 — Critical paths / the spine

## Summary

The paper-on-live spine has six paths to trace and four of them are wired end-to-end and provably in-flight in `fortuna_demo`. The two broken ones are load-bearing for the demo target: (1) **settlement never persists** — positions settle in-memory only, the `settlement_entries` table stays at zero rows, and PnL is ephemeral across restarts; (2) **calibration params are never written** — `run_weekly_review` calls `fit_platt` but the resulting `ScopeCalibration.fitted` object is logged and dropped, never inserted into `calibration_params`, so the synthesis arm sizes zero (it gets `CalibrationContext = None` at compose time). Both are documented as Phase-2 follow-ons or future slices in the code but block the demo's claimed "full loop". The dual-mode config (`[runtime].execution_mode` vs `[daemon].data_source/execution`) is coherent, not a competing path: boot.rs cross-validates the two sections and refuses ambiguous combos. The book path is REST-polled (confirmed at `runner.rs:827`), not WS-streamed — a known design choice, not a defect, but means stale-book risk under latency.

---

## Findings

| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
|---|---|---|---|---|---|---|---|
| P0 | BLOCKS | Settlement is in-memory only — `settlement_entries` table stays empty; realized PnL is lost on restart | `runner.rs:1918–2010` (apply_fresh_settlement): settlement goes to `SettlementLedger` (in-memory BTreeMap), not `SettlementsRepo`. `repos.rs:280–343` defines `SettlementsRepo.insert_entry` but it is **never called in production** (grep finds only test callers). `fortuna_demo` confirms: `settlement_entries = 0`. `daemon.rs:1451–1463`: explicit "Phase-2 follow-on" comment on the settlement panel. | PnL is not reconstructable across restarts. The full loop spec (`signal → … → settlement → score`) is incomplete. Post-settlement position PnL lives only in the runner's `PositionBook`. | `SettlementsRepo::insert_entry` exists and is correct; it is simply never wired into `apply_fresh_settlement` / `apply_void` / `apply_correction`. | In `apply_fresh_settlement` (and the correction/void branches), call `SettlementsRepo::insert_entry` after each `settlements.record_pending` / `settlements.advance`. The daemon already holds a pool via `PgIntentJournal`. | DST scenario: start runner → paper-fill a market → inject settlement notice → assert `settlement_entries` COUNT = 2 (pending + posted). Restart runner → reload from DB → assert settlement state recovers. |
| P0 | BLOCKS | `fit_platt` runs in `run_weekly_review` but fitted params are **never persisted** — `calibration_params = 0` in demo DB; synthesis sizes zero | `daemon.rs:4222–4234`: `run_weekly_review` calls `CalibrationParamsRepo::new(pool).latest(…)` to read a prior version, then at `cognition/src/review.rs:101` it calls `fit_platt` and wraps the result in `ScopeCalibration.fitted`. Back in `daemon.rs:3216–3231` the `WeeklyReview` result is only logged and routed to Slack — `CalibrationParamsRepo::insert` is **never called anywhere in production** (grep: only callers are `fortuna-ops/examples/rota_local.rs:342,357` and `fortuna-ops/tests/rota.rs:904`). At compose time (`daemon.rs:355–375`, `boot_paper_live_runner:875–895`) `calibration_for_scope` calls `CalibrationParamsRepo::latest` → returns `None` → `CalibrationContext = None`. `cycle.rs:76`: "Without one, beliefs shrink FULLY to the market prior and price no edge." Demo audit: 0 synthesis proposals, 5126 cognition degrades (4891 triage-budget-exhausted, 171 provider errors). | Synthesis strategy cannot size any trade — the Kelly numerator is always zero without calibration. Even when triage degrades clear, proposals will be sized to zero. The weekly review loop produces calibrated params in memory but silently discards them. | `run_weekly_review` returns `WeeklyReview` which carries `calibration: Vec<ScopeCalibration>`. Each `ScopeCalibration` has a `fitted: Option<CalibrationParams>`. The insert call is missing from the `drive()` block that handles the `Ok(wr)` branch (`daemon.rs:3216`). | After the `Ok(wr)` arm in `drive()`, iterate `wr.calibration`, and for each scope where `sc.fitted.is_some()`, call `CalibrationParamsRepo::new(pool.clone()).insert(…)`. Use `sc.fitted_version_would_be` as the version. | DST: run weekly review with N≥FULL_AUTONOMY_N resolved beliefs → assert `calibration_params` COUNT > 0 → re-compose runner → assert synthesis calibration context is `Some`. |
| P1 | BLOCKS | Fills not persisted to `fills` table — `FillsRepo` never called in production; fill audit entries in `audit` table are the only durable fill record | `runner.rs:1442–1508` (`drain_fills`): fills polled from venue, applied to position book, audited to `audit` table (`runner.rs:1493–1497`), published to bus. `FillsRepo` has `insert()` at `repos.rs:49` but the only production callers are in tests (`fortuna-ledger/tests/ledger.rs:290`). Demo DB: `fills = 0`, `audit` has 2 `kind='fill'` rows with full price/qty/fee/side/market payload. | Fill payloads in `audit` contain enough to reconstruct PnL manually (market, side, price, qty, fee), but the dedicated `fills` table exists for exactly this purpose and is wired in `rota.rs:1475` for ROTA health display. After a crash, PnL reconstruction requires parsing `audit` JSONB manually. The `fills` table is a cleaner, indexed, typed source of truth. | The `FillsRepo` repo and `fills` table schema exist; the wire call in `drain_fills` is missing. | In `drain_fills`, after `self.audit("fill", …)`, call `FillsRepo::new(pool).insert(venue, fill)`. The daemon holds the pool. | DST: fill a paper order → assert `fills` COUNT = 1. Assert idempotency: re-process same fill_id → COUNT stays 1. |
| P1 | BLOCKS | Bus recording (`recording_jsonl`) is ephemeral — `ShutdownReport` does not carry it; `main.rs` drops it silently; replay cannot be exercised on live runs | `runner.rs:90–96`: `ShutdownReport` has `cancelled, unacked, working` — **no `recording_jsonl`**. `runner.rs:98–104`: `RunnerReport` carries `recording_jsonl` but is only returned by `runner.report()`, which is called only in tests. `main.rs:973–982`: shutdown log is stats-only; no recording is persisted. `bus.rs:270–340`: `replay_verify` exists and is correct but has no production caller. The `manifest_hash` on proposals (`audit` payload) provides the decision anchor, but the market-snapshot inputs to that hash are not persisted (`market_snapshots = 0`, `price_snapshots = 0` in demo DB). | Golden path 6 (replay) is structurally incomplete in production. A decision can be identified by `manifest_hash` but the bus recording needed to re-run the deterministic sequence byte-identically is never saved. Decisions are auditable in the `audit` table (gate checks, fills) but not *replayable* in the spec-5.7 sense. | By design in the current phase (DST/sim replay is green; live replay is a later milestone). `recording_jsonl` is explicitly in `RunnerReport`, a DST-only artifact. | Persist `recording_jsonl` to a file or table at shutdown (or checkpoint per segment). Wire `runner.report()` into `main.rs` post-shutdown. Alternatively persist price snapshots so the manifest can be reconstructed. | DST existing: `replay_verify` is tested in `bus.rs`. Gap: no test proves live-run recordings survive a restart. |
| P2 | PARK | Dual-mode config (`[runtime].execution_mode` vs `[daemon].data_source/execution`) looks like two paths but is one coherent, cross-validated path | `boot.rs:580–656`: `validate_bootable` cross-checks `[runtime]` and `[daemon]` sections and refuses ambiguous combos with clear error messages. `boot.rs:697–742`: each `execution_mode` variant is matched to allowed `data_source/execution` pairs. `config/fortuna.example.toml:134–151`: both sections co-exist; the comment makes the semantics explicit. Boot tests at `boot.rs:956–1000` exercise the acceptance and rejection cases. | Not a defect — the design is intentional and well-validated. Only a reader unfamiliar with the design might mistake the two fields for competing paths. | None — the cross-validation logic at `validate_bootable` is correct. | None needed beyond confirming the tests pass. | Existing boot tests already cover this. |
| P2 | PARK | Book path is REST-polled per tick (`runner.rs:827`), not WS-streamed; no staleness guard for slow ticks | `runner.rs:827`: `self.venue.book(&market).await` in the tick loop — one HTTP GET per market per tick. `PaperLiveVenue` delegates `book()` to `KalshiReadClient.book()` (`paper_live.rs:149`). No "last-updated-at" staleness gate before strategies see the snapshot. At `tick_interval_ms = 1000` with N markets, this is N sequential GETs per second. | A slow HTTP response delays the entire tick; a partially stale book snapshot reaches strategies. Under Kalshi rate limits this risks 429s and venue_api_error counts rising. The `runner.counters().venue_api_errors` is tracked but no automatic halt triggers on sustained book-fetch failures. | REST polling is the paper-on-live Phase 1 design choice; WS streaming is Phase 2. | Add a timestamp to the `OrderBook` returned by the venue and a staleness check in the gate (or strategy) to suppress quotes older than N seconds. | DST: simulate a stalled book fetch; assert the proposal is suppressed and venue_api_errors increments. |
| P3 | BLOAT-cut | `market_snapshots` and `price_snapshots` tables exist in schema and are referenced in `rota.rs` health panel but have zero rows and `SnapshotsRepo` has no production callers | `repos.rs:755–812`: `SnapshotsRepo` with `insert` and `recent` methods. `rota.rs:1475–1477`: counts both tables in health SQL. `fortuna_demo`: `market_snapshots = 0`, `price_snapshots = 0`. No production caller of `SnapshotsRepo::insert` found. | Dead schema weight; ROTA health panel shows zeros with no explanation. If the intent is per-tick price recording for replay anchoring, missing the insert is a P0 replay gap (covered under the bus recording finding above). | Tables were created for the replay/audit substrate; the insert path was not wired. | Either wire `SnapshotsRepo::insert` in `drain_fills` or the tick loop (for the replay anchor use-case), or drop the tables until needed. | N/A until wired. |

---

## Trace / narrative

### Path 1 — market-data → normalized snapshot → strategy input

**WIRED end-to-end.**

`runner.rs:791–841`: each tick calls `venue.refresh_market_data_for_markets(&markets)` (step 0), then for each non-terminal market calls `venue.book(&market)` (step 1). The returned `OrderBook` is stored in `self.books` and published as `EventPayload::BookSnapshot` onto the `EventBus`. The bus is single-threaded and deterministic (`bus.rs:2`).

For the paper-on-live mode, `PaperLiveVenue.refresh_market_data_for_markets()` calls `KalshiReadClient.refresh_market_data_for_markets()` which GETs the live Kalshi REST endpoint per market (`paper_live.rs:138–143`). The book data then flows through `PaperLiveVenue.book()` → `KalshiReadClient.book()` → REST GET → parsed `OrderBook`. This path is exercised: `fortuna_demo` shows 6930 audit rows over the period 2026-06-16T03:54 to 2026-06-18T01:05.

**Gap**: books are REST-polled, not WS-streamed. No staleness guard exists before the `BookSnapshot` is published to strategies.

### Path 2 — decision → proposal

**WIRED but synthesis proposal count is zero in production.**

`runner.rs:843–870`: after the bus processes the `BookSnapshot`, strategies see new events via `on_event`. For the `synthesis` strategy (`SynthesisStrategy`), a fired trigger runs a `DecisionCycle`. The `CycleOutcome` carries `candidates` which become `EdgeCandidate`s → `Proposal`s → `handle_proposal` at `runner.rs:954`.

The `manifest_hash` is attached to every proposal audit entry (`runner.rs:974`). In `fortuna_demo`: 43 proposals from `mech_extremes`, **0 from synthesis**. The reason is the calibration chain break: without a `CalibrationContext`, beliefs shrink fully to the market prior and produce no edge candidates. Additionally, 5126 cognition degrades — 4891 triage budget-exhausted, 171 provider errors — confirm that synthesis is firing but unable to form beliefs due to budget exhaustion and API errors.

**Gap (P0)**: calibration params never persisted → synthesis proposals impossible.

### Path 3 — risk gate → sealed GatedOrder

**WIRED and correct.**

`runner.rs:918–1113`: after sizing (`cost_per_set`, envelope headroom), each leg is submitted as a `CandidateOrder` to `GatePipeline::evaluate`. The pipeline runs checks 1–10 in sequence (`pipeline.rs:216–247`). The only constructor of `GatedOrder` is `GatedOrder::assemble(candidate)` at `pipeline.rs:244`, called only after all checks pass — I1 invariant is structurally enforced. Gate decisions are audited per check (`runner.rs:1070–1106`).

`fortuna_demo` confirms: 230 `gate_decision` audit rows. Sample: check 1 (Halts), check 2 (Capital: "cost $45.79 + exposure $0.00 within $8000.00"), check 3 (PositionCaps), intent_id tied to each row. All from `mech_extremes`.

**I5 hard-stop**: `runner.rs:1102–1113` — if `audit_dead` is true after gate decisions are recorded, staged orders are aborted before venue submission. This is correctly positioned.

### Path 4 — execution → fill / cancel / ack

**WIRED. Fill to positions wired; fill to `fills` table NOT wired.**

`runner.rs:1442–1508` (`drain_fills`): polls `venue.fills_since(cursor)` per tick (up to 1000 pages). For `PaperLiveVenue`, `fills_since` delegates to `paper.fills_since` (the in-memory paper broker). Each fill is passed to `manager.ingest_fill` (`exec/manager.rs`), then applied to `positions.apply_fill` (in-memory position book), audited to `audit` table, and published to bus.

`fortuna_demo`: `audit` shows 2 fill events (market `KXBTC-26JUN1717-B66375`, No side, prices 91¢, qty 17+5, fee 3+1). The `fills` table has 0 rows — `FillsRepo::insert` is never called.

**Gap (P1)**: fills are durable only as JSONB in `audit`; the typed `fills` table is empty.

### Path 5 — accounting → settlement → realized & unrealized PnL

**PARTIALLY WIRED. In-memory settlement loop works; DB persistence is absent.**

`runner.rs:929`: `process_settlements()` is called every tick. It polls `venue.settlements_since(cursor)`, applies each notice via `apply_fresh_settlement` / `apply_void` / `apply_correction`. Settlement math is in `state/positions.rs:255–280`: `apply_settlement(winner, payout_cents)` adds `payout - basis` to `realized_pnl`, zeroes the lot quantities.

`runner.rs:2710–2777` (`digest_snapshot`): PnL per strategy is computed by summing `positions.realized_pnl` across markets attributed by `market_strategy`. This is the number the ROTA digest panel reads.

However: `SettlementsRepo::insert_entry` is never called in production. `fortuna_demo`: `settlement_entries = 0`, but `audit` has 42 `kind='settlement'` rows (all `"held": false, "owed": 0` — meaning no open positions at settlement time for those markets). If a position were held at settlement, the PnL would update in-memory, appear in `digest_snapshot`, but vanish on restart. PnL is not reconstructable from events alone without the `settlement_entries` chain.

**Gap (P0)**: settlement not persisted; realized PnL is ephemeral.

**Is PnL reconstructable from events?** Partially. The `audit` table contains fill JSONB (price, qty, fee, side, market) sufficient to reconstruct cost basis manually. Settlement audits show the winner side and owed amount. But this requires parsing untyped JSONB and is fragile — the `settlement_entries` and `fills` tables exist precisely to avoid this.

### Path 6 — replay (recorded events → deterministic decision reproduction)

**STRUCTURALLY PRESENT in DST; NOT WIRED for live runs.**

`bus.rs:270–340`: `replay_verify` takes a `Recording` and a set of handlers, re-injects external events, expects derived events to be regenerated deterministically. This is exercised by DST scenarios.

`runner.rs:1742`: `RunnerReport.recording_jsonl` captures the full bus recording via `bus.recording().to_jsonl()`. However, `RunnerReport` is returned by `runner.report()` which is only called in tests — never by `drive()` or `main.rs`.

`ShutdownReport` (returned by `drive()`) at `runner.rs:90–96` carries only order lifecycle counts, not `recording_jsonl`. `main.rs:979–980` reads only `shutdown.cancelled` and `shutdown.unacked`.

**Is every decision tied to a timestamped market snapshot?** The `manifest_hash` on proposals provides a content hash of the context items, but the price_snapshots table has 0 rows and `SnapshotsRepo::insert` is never called. The `manifest_hash` is an audit anchor but the input snapshots that generated it are not independently persisted.

---

## Self-adversarial pass

**Attack 1: Is the calibration persist claim (P0) truly missing, or did I miss a caller?**
The grep `grep -rn "CalibrationParamsRepo.*insert\|\.insert.*calibrat"` was run across all `*.rs` files. Production callers found: only `fortuna-ops/examples/rota_local.rs:342,357` (an example binary) and `fortuna-ops/tests/rota.rs:904` (tests). The `run_weekly_review` function at `daemon.rs:4184` calls `CalibrationParamsRepo::new(pool).latest()` to READ, not insert. The `ScopeCalibration.fitted` field is populated in memory but the insert call is genuinely absent. DB evidence: `calibration_params = 0`. **CONFIRMED P0.**

**Attack 2: Is settlement truly ephemeral, or is it persisted via intent_journal?**
`PgIntentJournal` records intent events (open/fill/cancel), not settlement chains. `intent_events = 82` in the demo. The settlement_entries table is separate and its `SettlementsRepo::insert_entry` is never called in production paths. DB confirms `settlement_entries = 0`. **CONFIRMED P0.**

**Attack 3: Is the fills table gap truly P1, or are fills reconstructable well enough from audit?**
The `audit` table JSONB rows for fills contain all the fields needed (market, side, price, qty, fee, at, fill_id, venue_order_id, client_order_id). An engineer could reconstruct fills. However, the `fills` table exists for this purpose, is listed in ROTA health, and the mismatch between its schema and its empty state is a maintenance liability. P1 is justified — not because reconstruction is impossible, but because the `fills` table's existence creates false confidence. However, I could see P2. **Kept P1.**

**Attack 4: Is the bus recording gap truly P1, or is the manifest_hash sufficient for spec 5.7?**
Spec 5.7 says decisions should be "replayable"; `manifest_hash` is a content hash of context items at decision time. The context items are reconstructable from signals/events in the DB but the exact serialized form that was hashed is not independently persisted. This is a grey area — the audit trail is rich enough for investigation but the byte-identical replay the bus mechanism enables is not exercised. Classified P1 (blocks MVP correctness claim), not P0, because the core trading behavior works without it. **Kept P1.**

**Attack 5: Did I correctly classify the dual-mode config as P2/PARK?**
Yes. `boot.rs:956–1000` has dedicated tests for each accepted/rejected combination. The two-section design is intentional and validated. A reader concern, not a real defect. **Kept P2/PARK.**

**What did I MISS?**
- I did not trace `PgIntentJournal.record_intent` → `intent_events` table to verify the full intent lifecycle is durable. Evidence (82 intent_events, 9 order audit rows, 2 fill audit rows) suggests it works but I did not verify schema.
- I did not verify the `audit` table's trigger (`audit_append_only BEFORE DELETE OR UPDATE`) fires correctly in Postgres — it's defined and visible in `\d audit` output, but I did not test a mutation attempt.
- I did not read the `market_back_discovery` path in detail (daemon.rs:1894–2113) to verify it persists events correctly — this is in Area 2/8 scope.

---

## Open questions for the Lead

1. **Settlement wiring priority**: The Phase-2 follow-on comment at `daemon.rs:1452` for the `settlement` panel (money/positions) is about the *live-venue account view* (async `Venue::account`). Is the `settlement_entries` table persistence (paper settlements from the in-memory paper broker) also intended as Phase-2, or was it an oversight? The code path exists (`SettlementsRepo`), it just isn't called.

2. **Calibration persist ownership**: Should the calibration insert happen inside `run_weekly_review` (alongside the read of prior versions) or in `drive()` after `run_weekly_review` returns? The latter is cleaner (keeps `run_weekly_review` read-only) but requires threading the pool deeper.

3. **Fills table intent**: Is `FillsRepo` intended as the canonical fills ledger (replacing the JSONB audit approach) or as a secondary mirror? If the former, P1 rating is correct. If the latter, the ROTA health panel that counts `fills` needs a note explaining why it shows 0.

4. **Recording persistence**: Is the plan to persist `recording_jsonl` to a file per run (for DST re-ingestion) or to a DB table? The current size of the bus recording after a soak run is unknown — could be large.

5. **Synthesis calibration bootstrap**: With 0 resolved beliefs (all 108 are `status='open'`), the weekly review will never produce a calibrated scope. Is there a plan to seed calibration_params from Aeolus-derived priors, or is the intent to let it accumulate from live resolution?
