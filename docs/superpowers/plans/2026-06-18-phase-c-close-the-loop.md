# Phase C — Close the Loop (Demo-Paper-Ready v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the loop end-to-end for every strategy and turn on `fortuna start paper-demo` so it accrues trustworthy validation data (realized PnL + per-producer scored beliefs), on the clean post-Phase-B baseline.

**Architecture:** Wire the four "computed-but-never-persisted" gaps (settlement, fills, recording, calibration) into the daemon as a **generic, idempotent, telemetered spine**; generalize discovery to a venue-neutral `MarketCatalog`; add the meteorologist persona as a second weather belief-producer scored head-to-head with Aeolus; ship a single fresh-start demo command + a ROTA chain-view. Spec: `docs/superpowers/specs/2026-06-18-phase-c-close-the-loop-design.md`.

**Tech Stack:** Rust 2021 workspace; `sqlx`/Postgres (offline mode, `SQLX_OFFLINE=true`); `tokio`; the injected `Clock`; the DST corpus; `fortuna-cognition` Mind/personas; `fortuna-ops` ROTA + `MetricsRegistry`.

## Global Constraints

Every task implicitly includes these (from the spec §2; a task violating one is wrong):

- **Decoupling:** no `"weather"`/`"kalshi"`/`"aeolus"` string literals or branches in spine paths (`fortuna-gates`/`-exec`/`-state`/`-ledger` money/scoring/calibration code). Domain/venue/producer are **data** (`venue_id`, `market_id`, `producer`, `category`, `rule`). Weather/Aeolus/Kalshi are plug-in instances only.
- **Idempotency:** every loop write is safe under retry/restart/replay — `ClientOrderId` orders, `ON CONFLICT DO NOTHING` fills (by `fill_id`), set-once settlement, window-deduped beliefs, DB-guarded event dedup, versioned calibration. Double-running any resolve/settle/score cycle yields identical ledger state.
- **Telemetry:** every stage emits metrics labelled by `producer`/`strategy`/`venue`/`category` (labels = data); degrades/staleness are visible; the ROTA chain-view shows the per-market chain + safety state.
- **Constitution (must stay green):** I1 sealed universal gate · I2 drawdown halt · I3 dual rate-limit · I4 standalone kill switch · I5 append-only audit · I6 propose-only/unsized · I7 no live capital. No real-order path constructible in paper. No `panic!/unwrap/expect`/`f64` in money paths. All time via `Clock`.
- **DoD per task:** TDD (failing test first, mutation-proof safety assertions); `cargo fmt --all --check`; `SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings`; touched-crate tests; `scripts/run-dst.sh` where the loop/Clock is touched; `scripts/check-protected-invariants.sh`; tick the box + one-line note.
- **Already closed in Phase B (do NOT redo):** F10 (gate-9 integrated), F11 (pointer fixed; proper daemon-write is task E2), F14 (stale DBs dropped), clippy `collapsible_if`.

---

## File Structure (what each workstream touches)

- **A — Spine:** `crates/fortuna-runner/src/runner.rs` (drain fills/settlement → repo writes), `crates/fortuna-live/src/daemon.rs` (settlement driver + recording persist + per-segment telemetry), `crates/fortuna-ledger/src/repos.rs` (idempotent insert guards), `crates/fortuna-ledger/migrations/` (fill_id unique, settlement set-once, recording table), `crates/fortuna-cognition/src/` (funding window-dedup scoring).
- **B — Model arm:** `crates/fortuna-live/src/daemon.rs` (persist weekly-review `fitted` calibration in `paper`), `crates/fortuna-runner/src/synthesis.rs` (gate the paid Mind call behind calibration-readiness), weather belief resolution path.
- **C — Discovery:** new `crates/fortuna-cognition/src/catalog.rs` (venue-neutral `MarketCatalog`/`MarketView`), `crates/fortuna-cognition/src/discovery.rs` (resolution_source match fix; controlled category), `crates/fortuna-live/src/daemon.rs` (market-back event-matching; ticker refresh), `crates/fortuna-gates/` (book-freshness check), move `WeatherMarketSource` off the `kalshi::` namespace.
- **D — Persona:** `crates/fortuna-live/src/main.rs` (per-persona `AnthropicMind` charter), `crates/fortuna-cognition/src/persona_runner.rs`, `config/personas/meteorologist/`, `config/fortuna.toml` (`[personas]`).
- **E — Demo surface:** `crates/fortuna-cli/src/main.rs` (`start paper-demo`, `doctor`), `crates/fortuna-live/src/daemon.rs` + `crates/fortuna-ops/src/rota.rs` (chain-view + safety pills), `scripts/`, `docs/runbooks/`.

---

## Workstream A — The generic spine (closes mechanical + perp arms; F1, F12, F2, F13, F4-recording)

### Task A1: Schema + idempotency prerequisites (migration + repos)
> **Verified scope correction:** `fills.fill_id` is ALREADY `PRIMARY KEY` and `FillsRepo::insert` ALREADY has `ON CONFLICT (fill_id) DO NOTHING` (`repos.rs:58-79`); `SettlementsRepo::insert_entry` ALREADY exists (`repos.rs:280-343`) but is a **bare INSERT (no ON CONFLICT)**. Do NOT recreate the fill PK. The genuinely-new work below.
**Files:** New migration `crates/fortuna-ledger/migrations/20260618NNNNNN_phase_c_persistence.sql` (must sequence after `20260617000001`; reuse the existing `fortuna_refuse_mutation()` — do not redefine). Modify `crates/fortuna-ledger/src/repos.rs`. Test: `crates/fortuna-ledger/tests/` (run via `SQLX_OFFLINE=true` + `DATABASE_URL=postgres:///fortuna?host=/tmp`).
Migration adds:
- `fills.producer TEXT` + `fills.strategy TEXT` (nullable) — so a settled fill traces to its originating producer/strategy (prereq for A4/D4 scoring).
- `settlement_entries` business-key **UNIQUE** — separate per event type: binary-event `(market_ticker, intent_id)`; the funding plane is already handled by `scalar_beliefs` resolution, not `settlement_entries`.
- `scalar_beliefs.producer TEXT` (typed; today `producer` lives only in `provenance` JSON) + **UNIQUE (producer, event_key)** — replay-safe window dedup (prereq for A5).
- new `bus_recordings` append-only table (`recording_id, segment_seq, jsonl, created_at`; `fortuna_refuse_mutation` trigger).
**Interfaces — Produces:** `SettlementsRepo::insert_entry(..) -> Result<bool /*inserted*/>` (now `ON CONFLICT DO NOTHING`); `FillsRepo::insert(&Fill, producer, strategy)`; `RecordingsRepo::append(segment_seq, jsonl) -> Result<()>`; `ScalarBeliefsRepo` upsert `ON CONFLICT (producer, event_key)`.
- [ ] **Step 1:** Failing tests: a second `insert_entry` with the same `(market_ticker, intent_id)` is a no-op (returns `false`); a second scalar belief with the same `(producer, event_key)` is a no-op; `bus_recordings` rejects UPDATE/DELETE.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Write the migration + `ON CONFLICT` repo methods; `cargo sqlx prepare`.
- [ ] **Step 4:** Run → PASS. Mutation-proof: drop a UNIQUE → the dup test REDs.
- [ ] **Step 5:** Commit `feat(ledger): Phase-C persistence schema (settlement/belief idempotency keys, producer columns, recordings)`.

### Task A2: Wire FillsRepo on fill-applied (F12)
> Verified: `FillsRepo::insert` exists but has **zero production callers**; `drain_fills` is at `runner.rs:1442` (exact). `fill_id` already exists/stable. New work = actually call the repo, tagged with producer/strategy.
**Files:** Modify `crates/fortuna-runner/src/runner.rs` (`drain_fills` `:1442`) to call `FillsRepo::insert(fill, producer, strategy)` — `strategy` from the originating intent; `producer` from the belief/edge provenance behind the proposal (carry it on the intent so the fill can be attributed). Thread the repo through the composition. Test: `crates/fortuna-runner/tests/` + a daemon_smoke assertion.
**Interfaces — Consumes:** A1 `FillsRepo::insert(&Fill, producer, strategy)`.
- [ ] Step 1: Failing test — a paper fill produces exactly one `fills` row carrying its `strategy` (+`producer` when the proposal came from a belief); a journal replay produces no second row. Step 2: FAIL. Step 3: wire the insert (idempotent) + thread producer/strategy onto the intent. Step 4: PASS; mutation-proof (double-drain → one row). Step 5: Commit `feat(runner): persist paper fills (with producer/strategy) to the fills table (F12)`.

### Task A3: Settlement bridge — venue resolution → paper settle → realized PnL (F1)
> **Verified P0 corrections:** the paper engine does NOT self-settle — `PaperVenue::settle_market(market, winner: Side)` (`fortuna-paper/src/lib.rs:272`) needs the winning side supplied. The truth source is `KalshiReadClient::settlements_since(cursor)` → `GET /portfolio/settlements`, which parses `market_result` → `SettlementOutcome::Winner(Side)` (`kalshi/read_client.rs:63`, `kalshi/adapter.rs:919`). The bridge from one to the other **does not exist**. The `daemon.rs:1452` "Phase-2 follow-on" comment is in `paper_data_rota_views` (the ROTA builder), NOT the drive loop — the driver is new code in the segment loop.
**Files:** Modify `crates/fortuna-live/src/daemon.rs` (segment loop) to add the **settlement bridge**; thread the live `KalshiReadClient` handle (already built for reads) into it; persist a cursor in `exec_cursors`. Modify `crates/fortuna-runner`/`fortuna-state` so realized PnL is **reconstructable from `settlement_entries`**, not only the in-memory accumulator. Test: `crates/fortuna-live/tests/` (sqlx) + DST.
**The bridge (each segment, or on a cadence):** `read_client.settlements_since(cursor)` → for each settled market with an OPEN paper position: `paper_venue.settle_market(market, winner_side)` → `SettlementsRepo::insert_entry(..)` (idempotent via A1 key) → compute realized PnL per intent → advance + persist the cursor. Venue-agnostic: the `winner: Side` and `(venue, market)` are data; no `weather`/`kalshi` branch in the bridge logic (Kalshi is the read-client instance).
**PnL reconciliation (idempotency 1c):** designate **`settlement_entries` as the source of truth** for realized PnL. The in-memory `PositionBook.realized_pnl` (`fortuna-state/positions.rs:223`) is a live-session display; on restart it is **seeded from `settlement_entries`** (not re-accumulated from re-polled settlements). The bridge must not double-apply a settlement already in `settlement_entries` (the A1 UNIQUE + the cursor both guard this).
**Interfaces — Consumes:** A1 `SettlementsRepo::insert_entry`; `KalshiReadClient::settlements_since`. **Produces:** realized PnL per settled intent, reconstructable from the DB.
- [ ] Step 1: Failing tests — (a) a settled market (read-client returns `Winner(Yes)`) closes its open paper position into `settlement_entries` + computes realized PnL; (b) an unresolved market stays open; (c) **re-running the bridge is a no-op** (idempotent); (d) **restart → realized PnL reconstructed from `settlement_entries`, no double-count**. Step 2: FAIL. Step 3: implement the bridge + cursor + PnL-from-DB seeding. Step 4: PASS. Step 5: DST seed (settlement replay deterministic, byte-identical) + Commit `feat(live): settlement bridge (venue resolution → paper settle → realized PnL from DB) (F1)`.

### Task A4: Trade scoring (per strategy/venue)
**Files:** Modify `crates/fortuna-cognition/src/scoring.rs` or a new `trade_score` path + `crates/fortuna-ledger` (`trade_scores` rows or reuse). Compute per-settled-intent: realized PnL after fees, fill realism (maker/through-not-touch), CLV. Test: cognition tests.
**Interfaces — Consumes:** A3 realized PnL (from `settlement_entries`) + fills carrying `producer`/`strategy` (A1/A2). **Produces:** `trade_score` per `(strategy, market)` (and per `producer` where the fill is belief-originated — feeds D4).
- [ ] Steps: failing test (a settled fill yields a trade score with PnL-after-fees + CLV, attributed to its strategy/producer) → FAIL → impl (generic, keyed by strategy/venue/producer — no domain literals) → PASS → Commit `feat: trade scoring from settled fills (by strategy/producer)`.

### Task A5: Funding window-dedup scoring (F2, F13)
> **Verified key correction:** dedup must be on **`(producer, event_key)`** where `event_key = "{market}:{next_funding_time}"` (e.g. `KXBTC-…:2026-06-17T04:00:00Z`) — NOT bare `(producer, horizon)`, which would merge different markets (KXBTC, KXETH) sharing the same funding slot. Backed by A1's `UNIQUE (producer, event_key)` on `scalar_beliefs` (replay-safe, not app-layer-only).
**Files:** Modify the funding scoring (`crates/fortuna-live/src/daemon.rs:resolve_and_score_funding_beliefs` + `crates/fortuna-cognition`) to score per **distinct `(producer, event_key)`** (one scored unit per market-window, not per-tick); make the `[review].min_resolved_beliefs` gate count **distinct `event_key`s**. Verify `realized_value` matches `funding_rates_historical` at the exact `funding_time` (F13).
- [ ] Step 1: Failing test — N ticks for one `(market, funding_time)` produce ONE scored unit; two markets sharing a slot stay distinct; the promotion count is per-`event_key`. Step 2: FAIL. Step 3: dedup-by-event_key. Step 4: PASS + a test asserting `realized_value` == the `funding_rates_historical` rate at that `funding_time` (F13). Step 5: Commit `fix(scoring): funding scored per (producer, event_key) window, not per tick (F2/F13)`.

### Task A6: Bus recording persistence (F4-recording / replay)
> **Verified P0 correction:** do NOT add `recording_jsonl` to `ShutdownReport` (`runner.rs:90` — that's order-cancellation accounting). `recording_jsonl` ALREADY exists on `RunnerReport` (`runner.rs:100`), populated by `runner.report()` (`runner.rs:1742`); there's also `runner.recording()` (`runner.rs:761`) for per-segment snapshots. The daemon calls only `runner.shutdown()` (`daemon.rs:3321`) and drops the recording. Fix = call `report()`/`recording()` and persist.
**Files:** Modify `crates/fortuna-live/src/daemon.rs`/`main.rs` to call `runner.recording()` per segment (or `runner.report()` at shutdown) and persist via `RecordingsRepo::append` (A1). No change to `ShutdownReport`. Test: runner + a replay test.
- [ ] Steps: failing test (a recorded segment is persisted to `bus_recordings` + replays byte-identically) → FAIL → call `runner.recording()`/`report()` → `RecordingsRepo::append` (append-only, replay-safe) → PASS (replay deterministic) → Commit `feat: persist live bus recording for replay via RunnerReport (F4)`.

### Task A7: Spine telemetry + decoupling guard
> **Verified corrections:** (1) the Kalshi leak is in `fortuna-live` (`daemon.rs:2277,2361` + hardcoded `"weather"`/`"kalshi"`/`fees.get("kalshi")`) — a guard scoped to spine crates only would MISS it; so this task's `fortuna-live` assertions **depend on C1** (run A7 after C1). (2) Existing invariant fixtures contain `"weather"`/`"kalshi"` literals (`i7_promotion_gates.rs:90`, `i_paper_live_no_real_order.rs:17,18,126`) → the guard must exclude `**/tests/**` and config/adapter/producer-instance paths. (3) Name the metrics so they aren't dropped.
**Files:** Modify `crates/fortuna-ops`/`MetricsRegistry` + emit points across A2–A6. **Named counter families (all must exist):** `fills{venue,market}`, `settlements{venue}`, `realized_pnl{strategy}`, `trade_score{strategy,producer}`, `gate_rejections{check}` (the `gate_rejections_by_check` map at `runner.rs:195` — export it), `belief_scores{producer}`, `data_staleness{source}` (signal/book freshness), `mind_spend{role}` (synthesis/triage/recon — covers the B3 gap). Add a guard test in `crates/fortuna-invariants/tests/` (additions-only): grep `crates/fortuna-gates|-exec|-state|-ledger` **and** `crates/fortuna-live/src` (post-C1) for `"weather"|"kalshi"|"aeolus"` literals in money/gate/scoring/compose paths, excluding `**/tests/**`, `**/kalshi/**` (the adapter instance), and config.
- [ ] Steps: failing test (named series emitted after a settle cycle; guard greps the listed scope with exclusions) → FAIL → emit metrics + add guard → PASS → Commit `feat: spine telemetry (named families) + decoupling guard (incl. fortuna-live, post-C1)`.

---

## Workstream B — The model arm (F0, F3, F5)

### Task B1: Persist fitted calibration in `paper` — count-triggered (F0)
> **Verified P0 corrections:** (1) `run_weekly_review` (`daemon.rs:4185`) computes `wr.calibration[i].fitted` (when `n ≥ FULL_AUTONOMY_N=50`) but **never calls `CalibrationParamsRepo::insert`** — it just audits + returns. (2) It only fires on a **Monday-aligned 7-day boundary** (`daemon.rs:4560`), so a multi-day/mid-week demo **never warms the model arm** even with 50+ resolved beliefs. (3) `run_weekly_review` has **no `stage` parameter** — it must be plumbed from the caller's `ExecutionMode`/`stage`.
**Files:** Modify `crates/fortuna-live/src/daemon.rs`: (a) thread `stage` into `run_weekly_review`; when `stage=="paper"`, after `weekly_review()` returns, iterate `wr.calibration` and call `CalibrationParamsRepo::insert` for each `fitted` (versioned upsert). (b) Add a **count-triggered calibration persist on the daily-resolution boundary** (`daemon.rs:3127` weather / `:3161` funding region): when a scope crosses `FULL_AUTONOMY_N` resolved beliefs, fit + persist immediately (don't wait for Monday). `stage="live_*"` NEVER auto-persists (I7 — write the test as `ExecutionMode` membership, not string-equality). Test: `crates/fortuna-live/tests/` (sqlx).
**Interfaces — Consumes:** `CalibrationParamsRepo::insert`, `ScopeCalibration.fitted`, the daily-resolution counts.
- [ ] Step 1: Failing tests — (a) a scope reaching ≥50 resolved beliefs in `paper` writes a `calibration_params` row **on the daily boundary, no Monday required**; (b) re-run = versioned + idempotent; (c) `stage` ∈ live modes does NOT auto-persist. Step 2: FAIL. Step 3: plumb stage + count-trigger + persist. Step 4: PASS; mutation-proof (flip the stage gate; flip the count threshold). Step 5: Commit `feat(live): count-triggered calibration persist in paper → wakes the model arm (F0)`.

### Task B2: Verify weather belief resolution + signal-freshness (F3)
> **Verified reframe:** `resolve_and_score_weather_beliefs` is ALREADY implemented (`daemon.rs:3858`) and wired into the daily-boundary block (`daemon.rs:3127`), with a passing smoke (`daemon_smoke.rs:3901`). So this is **verification, not wiring** — the real risk is whether `nws.cli` signals are reliably ingested on the live cadence so the grader has data.
**Files:** Add a freshness/coverage check + telemetry (`data_staleness{source="nws_climate"}`); confirm the daily resolver consumes the latest CLI; confirm resolved counts feed B1's count-trigger. Test: cognition/live.
- [ ] Steps: failing test (a resolved weather belief gets `outcome`+`brier`+`clv`; the scope's resolved-count increments and is visible to B1; stale/absent CLI is surfaced as a metric, not silent) → run (may pass on existing code → then add the freshness assertion as the new coverage) → confirm → Commit `test(live): verify weather belief resolution + CLI signal-freshness (F3)`.

### Task B3: Gate the paid Mind call behind calibration-readiness (F5)
> **Verified safe (no deadlock):** weather beliefs that feed calibration come from the deterministic Aeolus daily-resolver path (`emit_aeolus_beliefs` → `daemon.rs:3127`), **independent of** synthesis's paid `decide()`. So skipping `decide()` while cold does NOT starve calibration. B3 depends on B1 to ever go warm — document that.
**Files:** Modify `crates/fortuna-runner/src/synthesis.rs` (`on_event`, the `cycle.run`/`decide` call ~`:176`): when the scope has no `calibration_params` row, skip the expensive Opus `decide()` (it would only size zero — `compose.rs:73-74`); still let the cheap deterministic belief substrate accrue. Telemetry: `mind_spend`/a "synthesis skipped: cold calibration" counter (no silent skip).
- [ ] Steps: failing test (cold scope → no paid `decide()`, skip metric emitted; warm scope after B1 → `decide()` proceeds) → FAIL → gate it → PASS → Commit `fix(synthesis): gate paid Mind call on calibration-readiness; depends on B1 (F5)`.

---

## Workstream C — Generic discovery / seeding (F4, F7, F9, stale-book gate; "easy weather discovery")

### Task C1: Venue-neutral `MarketCatalog`/`MarketView`
> **Verified P1 correction:** `market_to_bucket` (`aeolus_venue.rs:111`) reads `KalshiMarket`-specific bracket geometry — `strike_type` (`"between"/"greater"/"less"`), `floor_strike_int()`, `cap_strike_int()`. A `MarketView` lacking these would **silently break the weather path** (all buckets get `None` floor/cap → malformed/zero edges). `KalshiMarket` is leaked at `aeolus_venue.rs:42`, `daemon.rs:2277,2361` (the cache type + the `KalshiMarketStatus::Active` filter).
**Files:** Create `crates/fortuna-cognition/src/catalog.rs`: `MarketView { market_id, venue_id, title, category, status, strike_type: Option<String>, floor_strike: Option<i64>, cap_strike: Option<i64> }` (the geometry is **part of** the venue-neutral view — it's bracket semantics, not Kalshi-specific) + `trait MarketCatalog { async fn list(series_or_category) -> Vec<MarketView> }`. The Kalshi adapter populates `MarketView` (incl. geometry) from `KalshiMarket`; `market_to_bucket` consumes `MarketView`; move `WeatherMarketSource` off the `kalshi::` namespace; drop hardcoded `venue:"kalshi"` (`aeolus_venue.rs:165`) — use the `MarketView.venue_id`.
- [ ] Steps: failing test — (a) discovery consumes `MarketView`; no `KalshiMarket`/`KalshiMarketStatus` type appears in `fortuna-live` (grep guard); **(b) regression: the weather path produces the SAME bucket floor/cap/edges via `MarketView` as it did via `KalshiMarket`** (golden compare). → FAIL → introduce the abstraction + adapt → PASS (existing weather path green) → Commit `refactor: venue-neutral MarketCatalog w/ bracket geometry (decouple from Kalshi; Area 5)`.

### Task C2: World-forward resolution_source match fix (F4)
**Files:** Modify `crates/fortuna-cognition/src/discovery.rs:689` — the `registry.get(&resolution_source).unwrap_or(true)` exact-match against machine IDs while Opus emits prose. Normalize/fuzzy-map resolution_source → registry id so discovered events become **scoreable**.
- [ ] Steps: failing test (a prose `resolution_source` like "Federal Reserve Board press releases" maps to `rss_fed_press` → event scoreable, belief attaches) → FAIL → fix the match → PASS → Commit `fix(discovery): map prose resolution_source to registry ids; events become scoreable (F4)`.

### Task C3: Controlled category vocabulary (F9)
**Files:** Modify discovery output to validate/normalize `category` into a controlled enum/allowlist (reject/normalize free-form `macro`/`Macro/Fed`/`x`). Migration or validation at ingest.
- [ ] Steps: failing test (unknown/variant category normalized or rejected; no 13-spelling drift) → FAIL → controlled vocab → PASS → Commit `fix(discovery): controlled category vocabulary (F9)`.

### Task C4: Market-back event-matching + live-ticker discovery (F7)
**Files:** Modify `crates/fortuna-live/src/daemon.rs:2010` (the empty `existing_events` stub) so re-discovery matches existing events (idempotent, no duplicate events). Generalize ticker discovery so dated brackets are resolved live from the catalog (kills expired-JUN16; `mech_structural` gets a live ladder). (F7.)
- [ ] Steps: failing test (re-discovering the same market maps to the existing event, no dup; expired config tickers don't block discovery) → FAIL → wire event-matching + live ticker resolution → PASS → Commit `fix(discovery): market-back event-matching + live-ticker discovery (F7)`.

### Task C5: Book-freshness gate (stale-book P2)
**Files:** Modify `crates/fortuna-gates/src/pipeline.rs` (+ `crates/fortuna-venues/src/types.rs` book `observed_at`) — add a `BookAge` gate check rejecting orders on a stale book (config `max_book_age_ms`). Generic (any venue/market).
- [ ] Steps: failing test (stale book → proposal rejected with a clear reason; fresh → passes) → FAIL → add the gate check → PASS; mutation-proof + DST → Commit `feat(gates): book-freshness gate (reject stale-book orders)`.

---

## Workstream D — Meteorologist persona (the thesis; charter fix + parallel-scored)

### Task D1: Per-persona Mind charter (fixes the charter-injection bug)
**Files:** Modify `crates/fortuna-live/src/main.rs:474` (`synthesis_mind.clone()`) + `crates/fortuna-cognition/src/persona_runner.rs:213` — build a per-persona `AnthropicMind` whose `system_charter = def.method` (the persona's trusted method), not the synthesis charter. Test: cognition/live.
- [ ] Steps: failing test (a persona run sends the persona's charter, not the synthesis charter) → FAIL → build per-persona Mind → PASS; mutation-proof → Commit `fix(personas): inject the persona's own charter (audit Area 8)`.

### Task D2: Seed personas registry + enable config
**Files:** `config/personas/meteorologist/` (charter `persona.md` + `schema.json` — exist; verify), `config/fortuna.toml` (`[personas]` enable + budget + cadence), seed the `personas` table at boot (or migration). Test: boot test (personas enabled boots; registry non-empty).
- [ ] Steps: failing test (boot with `[personas]` enabled succeeds; `personas` rows exist) → FAIL → seed + enable → PASS → Commit `feat(personas): seed registry + enable meteorologist`.

### Task D3: Meteorologist authors weather beliefs + captures replayable reasoning (parallel to Aeolus)
**Files:** Wire the meteorologist persona to trigger on NWS AFD/alerts/CLI for existing weather events → `domain_analysis` + belief `producer="meteorologist"` (parallel to Aeolus; sees Aeolus + market in context per spec §5). Test: cognition.
**Reasoning capture (so events are replayable + explainable — "every claim has evidence"):**
- The meteorologist schema (`config/personas/meteorologist/schema.json`) carries a **free-text `rationale`** field (the verbatim "why"), in addition to the structured `regime`/`confidence`/`key_risk`/`outcomes[].p`.
- Persist `domain_analyses.signal_manifest` = the exact inputs (AFD/alerts/CLI/Aeolus/market) it reasoned over, plus `content_hash`/`manifest_hash`.
- Write `event_source_evidence` links (event → the `signal_id`s in context) so a replay can walk event → inputs → reasoning → belief without parsing free text.
- The belief's `provenance`/`evidence` references the `analysis_id`.
- [ ] Steps: failing test — a meteorologist run produces (a) a `domain_analysis` with `rationale` + `signal_manifest` + `event_source_evidence` rows for the inputs, and (b) a per-bracket belief `producer=meteorologist` referencing the analysis; the whole chain (inputs → rationale → belief) is reconstructable from the DB. → FAIL → wire trigger+context+persist+evidence-links → PASS → Commit `feat(personas): meteorologist authors weather beliefs + replayable rationale/evidence (the intelligence arm)`.

### Task D4: Per-producer scoring + synthesis prices best-calibrated
**Files:** Verify belief scoring keys per `producer` (aeolus vs meteorologist scored independently); modify `crates/fortuna-runner/src/synthesis.rs` to price the calibrated producer (or ensemble). Telemetry: per-producer Brier/CLV. Test: cognition.
- [ ] Steps: failing test (two producers on one event scored independently; synthesis prices the calibrated one) → FAIL → impl → PASS → Commit `feat: per-producer scoring; synthesis prices calibrated producer (the thesis)`.

---

## Workstream E — Demo surface (F5-CLI, F6, F8, F11-proper, ROTA chain-view)

### Task E1: `fortuna doctor` readiness check
**Files:** `crates/fortuna-cli/src/main.rs` (new `doctor` verb): checks DB reachable, migrations applied, required env/creds present, mode safe, `funding_rates_historical` GRANT present (F6-grant), Aeolus source reachable (F6). Test: cli.
- [ ] Steps: failing test (doctor flags a missing GRANT / unreachable DB) → FAIL → impl → PASS → Commit `feat(cli): fortuna doctor readiness check`.

### Task E2: `fortuna start paper-demo` (fresh DB, paper fills, no REAL order) + pointer-write (F11-proper)
> **Verified mode pin:** the demo must accrue paper fills → realized PnL, so it runs **`execution_mode = "paper_ledger"`** (routes `place()` to the LOCAL `PaperLiveVenue` — `OrderMutationPolicy::Enabled`, `daemon.rs:783-793`). `live_data_only` would block even paper fills (no loop closure). Safety is **no REAL-venue order path** — guaranteed structurally by the `KalshiReadClient` (no `place`/`cancel`) + the protected `i_paper_live_no_real_order` invariant, NOT by disabling mutation. Don't write "no order-mutation path constructible" (false for paper_ledger); write "paper fills only; no real-venue order".
**Files:** `crates/fortuna-cli/src/main.rs` (new `start paper-demo`: provision a fresh DB, apply migrations + the `funding_rates_historical` GRANT, set `stage="paper"`/`execution_mode="paper_ledger"`/`data_source="kalshi_prod"`/`execution="paper"`, run `doctor`, start the daemon). Daemon writes the **true** live `DATABASE_URL` to `current-demo-db-url` on boot (F11-proper). Test: cli/boot.
- [ ] Steps: failing test (paper-demo boots in `paper_ledger`; the `i_paper_live_no_real_order` wall holds — no real-venue order; pointer reflects the live DB) → FAIL → impl → PASS → Commit `feat(cli): fortuna start paper-demo (fresh DB, paper_ledger, no real order); daemon writes live db pointer (F11)`.

### Task E3: ROTA chain-view + safety pills + reasoning drill-in (audit #6)
**Files:** `crates/fortuna-live/src/daemon.rs:1421-1438` (emit `execution_mode`/`order_mutation_enabled`/book-freshness into the health view) + `crates/fortuna-ops/src/rota.rs:2107` (render safety pills + a per-event chain panel). Test: ops/rota.
The chain panel renders, per event/market: **signals (inputs) → belief(s) by producer with the model's `rationale` (the "why") → proposal → gate decision → fill → settlement → score** — an Event-Workbench-lite drill-in reading `event_source_evidence` (inputs), `domain_analyses.rationale` (reasoning), `beliefs`, `audit` (proposal thesis + gate), and `settlement_entries`/`trade_score`. This is the operator-facing "replay its reasoning for an event" surface.
- [ ] Steps: failing test (health view carries `order_mutation_enabled`; the chain panel for an event renders its inputs + the model's rationale + the full decision chain) → FAIL → emit + render → PASS → Commit `feat(rota): chain-view + safety pills + per-event reasoning drill-in (audit #6)`.

### Task E4: Dead-man ping + ops hardening (F8)
**Files:** Investigate/fix the `dead-man ping FAILED: transport failure` (`daemon.rs` dead-man + monitor endpoint); ensure the heartbeat reconnects + alerts on failure (telemetry). Test: live/ops.
- [ ] Steps: failing test (ping failure is retried + surfaced, not silent) → FAIL → fix → PASS → Commit `fix(ops): dead-man ping reconnect + alert (F8)`.

### Task E5: Demo runbook + Aeolus stable-source note (F6)
**Files:** `docs/runbooks/paper-demo.md` (the one runbook: `doctor` → `start paper-demo` → ROTA → daily/weekly cadence → kill switch), document the Aeolus stable-URL + rolling-date requirement (F6). Update CHANGELOG. (Docs task; no code.)
- [ ] Steps: write the runbook + CHANGELOG entry → verify links resolve → Commit `docs: paper-demo runbook + Aeolus stable-source note (F6)`.

### Task E6: Operator `rearm` CLI verb (MVP-CLOSURE §4; audit §10)
> **Verified gap:** MVP-CLOSURE-PLAN §4 lists the operator `rearm` verb as Phase C; AUDIT.md §10 flags it; it had no task. Without it, a halted daemon (drawdown/rate-limit/kill-switch) needs a full restart to re-arm (`rearm_requires_restart` in the ROTA health). `gates.rearm()` already exists (`fortuna-gates/src/halt.rs`); only the CLI surface is missing.
**Files:** `crates/fortuna-cli/src/main.rs` (new `rearm` verb → clears a human-cleared halt out-of-band per I2, with an audit row). Test: cli.
- [ ] Steps: failing test (`fortuna rearm` clears a halt + writes an audit row; refuses if the kill-switch sentinel is still present per I4) → FAIL → impl → PASS → Commit `feat(cli): operator rearm verb (audit §10 / MVP-CLOSURE §4)`.
> **Also (tracked, not a task):** the killswitch `is_revoked()` FS-permission fail-open (`lib.rs:258`, audit §9, P2, mitigated by `PgHaltPoller`) is recorded in `GAPS.md` for a hardening follow-on.

---

## Coverage matrix (every audit/MVP/F-item → task)

| Item | Source | Task |
|---|---|---|
| F1 settlement / scorecard#1 / MVP | wire SettlementsRepo | **A3** (+A1 idempotency) |
| F0 calibration persist / #2 / MVP | persist fitted Platt in paper | **B1** |
| F12 fills / #3 / MVP | wire FillsRepo | **A2** (+A1) |
| F4-recording / #4 / MVP | persist bus recording | **A6** |
| F5-CLI / #5 / MVP | `fortuna start paper-demo` | **E2** (+E1 doctor) |
| #6 / MVP | ROTA execution_mode/order-mutation + chain-view | **E3** |
| F-persona / #7 / MVP | charter + registry + enable | **D1, D2, D3** |
| F4 world-forward / #8 / MVP | resolution_source match (unblocks scoreability) | **C2** — *scope:* C2 makes world-forward events scoreable + belief-attachable; the full world-forward→market-mapping→trade chain stays watchlist-deferred per spec §6 (macro domain) |
| Operator rearm verb | MVP-CLOSURE §4 / audit §10 | **E6** |
| Restart PnL reconciliation | idempotency 1c | **A3** (settlement_entries = source of truth) |
| Producer columns (fills/scalar_beliefs) | per-producer scoring prereq | **A1** |
| F6-grant / #9 / MVP | funding GRANT | **E1** (doctor) + **E2** |
| F2 oversampling | window-dedup scoring + promotion count | **A5** |
| F3 calibration cold-start | weather resolution → calibration | **B2** |
| F5 synthesis budget waste | gate paid Mind on calibration | **B3** |
| F7 mech_structural expired brackets | live-ticker discovery | **C4** |
| F9 category vocab | controlled vocabulary | **C3** |
| F13 realized_value verify | assert vs settled rate | **A5** |
| stale-book (MVP P2) | book-freshness gate | **C5** |
| F6 Aeolus tunnel/dates | stable-source note + doctor reach-check | **E1, E5** |
| F8 dead-man ping | reconnect + alert | **E4** |
| Decoupling (Area 5) | venue-neutral catalog + guard | **C1, A7** |
| Trade scoring | per-strategy from settled fills | **A4** |
| Per-producer scoring (thesis) | aeolus vs meteorologist | **D4** |
| **Already done (Phase B):** F10 gate-9, F11 pointer (proper→E2), F14 stale DBs, clippy | — | n/a |

## Self-Review

- **Spec coverage:** spec §3 spine → A; §4 arms → A (mech/perp) + B (synthesis); §5 meteorologist → D; §6 discovery → C; §7 demo surface → E; §2 principles → Global Constraints + A1/A7 (idempotency/decoupling/telemetry guards). ✓
- **Finding coverage:** every F-item + scorecard row + MVP gap maps to a task in the matrix above; Phase-B-done items marked n/a. ✓
- **Placeholders:** each task names exact files, the test intent with concrete assertions, interfaces, and the commit. Per-task code bodies are expanded at execution (subagent-driven-development), following the repo's established plan style (`2026-06-16-paper-on-live-data.md`); load-bearing behavior (idempotency keys, persist calls, decoupling boundary, charter fix) is specified inline. ✓
- **Type consistency:** `FillsRepo::insert` + `SettlementsRepo::insert_entry` ALREADY exist (A1 *adds* the `ON CONFLICT`/business-key + the `producer`/`strategy` columns + `RecordingsRepo`); A2/A3/A6 consume those. `MarketView` w/ bracket geometry (C1) consumed by C2/C4 + `market_to_bucket`. `producer` column (A1) → fills (A2) → trade scoring (A4) → per-producer (D4). Recording uses `RunnerReport.recording_jsonl` (A6), not `ShutdownReport`. ✓
- **Build order + dependencies:** A (spine) → B (model arm) → **C1 before A7** (A7's `fortuna-live` decoupling guard depends on C1 removing the Kalshi leak) → C (rest) → D (persona) → E (demo surface). Each ships a testable deliverable; all together = Phase C DoD.

## Verification record (adversarial verify loop, 2026-06-18)

Three read-only verifiers (coverage+citations, technical soundness, principles+invariants) attacked this plan before any execution. Defects found and **fixed inline above** (high-confidence; several cross-corroborated by ≥2 verifiers):
- **P0** A3 settlement bridge underspecified (no venue-resolution→`settle_market(winner)` path; wrong `:1452` cite) → rewrote A3 with the explicit bridge + PnL-from-DB reconciliation.
- **P0** A6 targeted `ShutdownReport` for `recording_jsonl` which lives on `RunnerReport` → rewrote A6 to call `runner.report()`/`recording()`.
- **P0** B1 calibration never fires (Monday-only weekly review; no `stage` param) → rewrote B1 with a count-triggered daily persist + `stage` plumbing.
- **P1** C1 `MarketView` would drop bracket geometry (`strike_type`/floor/cap) → C1 now carries geometry + a weather-path regression test.
- **P1** A5 dedup key `(producer,horizon)` merges multi-market windows → fixed to `(producer,event_key)` + a DB UNIQUE (A1).
- **P1** no `producer` column on fills/scalar_beliefs (blocks per-producer scoring + DB dedup); settlement `insert_entry` had no `ON CONFLICT`; restart PnL double-count → all added to A1/A3.
- **P1** decoupling guard scoped to spine only would miss the `fortuna-live` Kalshi leak → A7 now includes `fortuna-live` (after C1) with test-fixture exclusions.
- **Scope trims:** `fills.fill_id` PK + `ON CONFLICT` already exist; `resolve_and_score_weather_beliefs` already wired → A1 re-scoped, B2 reframed to verification.
- **Dropped item:** operator `rearm` verb → added E6.
