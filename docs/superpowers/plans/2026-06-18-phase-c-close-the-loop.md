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

### Task A1: Idempotent persistence guards (migration + repos)
**Files:** Modify `crates/fortuna-ledger/migrations/` (new migration: `fills.fill_id` UNIQUE; `settlement_entries` unique key on `(market_ticker, intent_id)` or `(market, funding_time)`; new `bus_recordings` append-only table). Modify `crates/fortuna-ledger/src/repos.rs` (`FillsRepo::insert` → `ON CONFLICT DO NOTHING`; `SettlementsRepo::insert_entry` set-once; new `RecordingsRepo`). Test: `crates/fortuna-ledger/tests/`.
**Interfaces — Produces:** `FillsRepo::insert(&Fill) -> Result<bool/*inserted*/>`; `SettlementsRepo::insert_entry(..) -> Result<bool>`; `RecordingsRepo::append(segment_jsonl) -> Result<()>`.
- [ ] **Step 1:** Failing test: inserting the same `fill_id` twice yields one row (second returns `false`/no-op); same for a settlement key.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Add the migration (unique constraints + recordings table + append-only trigger `fortuna_refuse_mutation`) + `ON CONFLICT` repo methods; `cargo sqlx prepare`.
- [ ] **Step 4:** Run → PASS. Mutation-proof: drop the unique constraint → the dup-insert test REDs.
- [ ] **Step 5:** Commit `feat(ledger): idempotent fills/settlement/recording persistence (F12/F1 prep)`.

### Task A2: Wire FillsRepo on fill-applied (F12)
**Files:** Modify `crates/fortuna-runner/src/runner.rs` (the `drain_fills`/`fill_applied` path ~`:1442`) to call `FillsRepo::insert` with a stable `fill_id`; thread the repo through the composition. Test: `crates/fortuna-runner/tests/` + a daemon_smoke assertion.
**Interfaces — Consumes:** A1 `FillsRepo::insert`.
- [ ] Step 1: Failing test — a paper fill produces exactly one `fills` row (and a replay of the same journal produces no second row). Step 2: FAIL. Step 3: wire the insert (idempotent). Step 4: PASS; mutation-proof (double-drain → still one row). Step 5: Commit `feat(runner): persist paper fills to the fills table (F12)`.

### Task A3: Settlement driver — resolve markets → realized PnL (F1)
**Files:** Modify `crates/fortuna-live/src/daemon.rs` (segment loop: drive `read_client.settlements_since`/`PaperVenue::settle_market` on resolved markets — the `:1452` "Phase-2 follow-on") → `SettlementsRepo::insert_entry` + realized PnL per intent; venue-agnostic (works for any `(venue,market)`). Test: `crates/fortuna-live/tests/` (sqlx) + DST.
**Interfaces — Consumes:** A1 `SettlementsRepo`; the venue `settlements_since`. **Produces:** realized PnL recorded per settled intent.
- [ ] Step 1: Failing test — a resolved market closes its open paper fills into `settlement_entries` with computed realized PnL; an unresolved market stays open; re-running the resolver is a no-op (idempotent). Step 2: FAIL. Step 3: implement the settlement driver (set-once, no `weather`/`kalshi` branch). Step 4: PASS. Step 5: DST seed (settlement replay deterministic) + Commit `feat(live): wire paper-live settlement → realized PnL (F1)`.

### Task A4: Trade scoring (per strategy/venue)
**Files:** Modify `crates/fortuna-cognition/src/scoring.rs` or a new `trade_score` path + `crates/fortuna-ledger` (`trade_scores` rows or reuse). Compute per-settled-intent: realized PnL after fees, fill realism (maker/through-not-touch), CLV. Test: cognition tests.
**Interfaces — Consumes:** A3 realized PnL + fills. **Produces:** `trade_score` per `(strategy, market)`.
- [ ] Steps: failing test (a settled fill yields a trade score with PnL-after-fees + CLV) → FAIL → impl (generic, keyed by strategy/venue) → PASS → Commit `feat: trade scoring from settled fills`.

### Task A5: Funding window-dedup scoring (F2, F13)
**Files:** Modify the funding scoring (`crates/fortuna-live/src/daemon.rs:resolve_and_score_funding_beliefs` + `crates/fortuna-cognition`) to score per **distinct `(producer, horizon)` window** (one unit/window, not per-tick); make the `[review].min_resolved_beliefs` gate count **distinct windows**. Verify `realized_value` matches `funding_rates_historical` at the exact `funding_time` (F13).
- [ ] Step 1: Failing test — N ticks targeting one funding window produce ONE scored unit; the promotion count is per-window. Step 2: FAIL. Step 3: dedup-by-window. Step 4: PASS + a test asserting `realized_value` == the settled rate (F13). Step 5: Commit `fix(scoring): funding scored per window, not per tick (F2/F13)`.

### Task A6: Bus recording persistence (F4-recording / replay)
**Files:** Modify `crates/fortuna-runner/src/runner.rs` (`ShutdownReport` ~`:90` to carry `recording_jsonl`) + `crates/fortuna-live/src/daemon.rs`/`main.rs` (persist via `RecordingsRepo` each segment, not dropped). Test: runner + a replay test.
- [ ] Steps: failing test (a recorded segment is persisted + replays byte-identically) → FAIL → thread recording to `RecordingsRepo` → PASS (replay deterministic) → Commit `feat: persist live bus recording for replay (F4)`.

### Task A7: Spine telemetry + decoupling guard
**Files:** Modify `crates/fortuna-ops/src/...` `MetricsRegistry` + emit points across A2–A6 (counters: fills/settlements/realized-PnL/trade-scores by `strategy`/`venue`). Add a guard test in `crates/fortuna-invariants/tests/` (additions-only): spine money/scoring paths contain no `"weather"`/`"kalshi"`/`"aeolus"` literals.
- [ ] Steps: failing test (key series emitted after a settle cycle; guard greps spine crates) → FAIL → emit metrics + add guard → PASS → Commit `feat: spine telemetry + decoupling guard test`.

---

## Workstream B — The model arm (F0, F3, F5)

### Task B1: Persist fitted calibration in `paper` (F0)
**Files:** Modify `crates/fortuna-live/src/daemon.rs` `run_weekly_review` (~`:4184`/`:4292`): when `stage == "paper"`, call `CalibrationParamsRepo::insert` with the review's `fitted` Platt (versioned upsert; the "not capital promotion in paper" boundary documented). Test: `crates/fortuna-live/tests/` (sqlx).
**Interfaces — Consumes:** existing `CalibrationParamsRepo::insert`, the weekly review `ScopeCalibration.fitted`.
- [ ] Step 1: Failing test — a scope with ≥`FULL_AUTONOMY_N` resolved beliefs + `stage="paper"` writes a `calibration_params` row; re-run = versioned, idempotent; `stage="live_*"` does NOT auto-persist (I7). Step 2: FAIL. Step 3: persist in paper. Step 4: PASS; mutation-proof (flip stage gate). Step 5: Commit `feat(live): persist fitted calibration in paper stage → wakes the model arm (F0)`.

### Task B2: Weather belief resolution → calibration substrate (F3)
**Files:** Verify/repair the path: NWS CLI grader resolves open weather beliefs → `outcome/brier/clv` → feeds B1. Modify the grader cadence/wiring (`crates/fortuna-cli` grader + `crates/fortuna-live` resolution call) so weather beliefs actually resolve on the live daemon. Test: cognition/live.
- [ ] Steps: failing test (a resolved weather belief gets `outcome`+`brier`; its scope's resolved-count increments) → FAIL → wire the resolution → PASS → Commit `fix: weather belief resolution feeds calibration (F3)`.

### Task B3: Gate the paid Mind call behind calibration-readiness (F5)
**Files:** Modify `crates/fortuna-runner/src/synthesis.rs` (`on_event` ~`:174`): skip/triage-only the expensive `decide()` when no `calibration_params` row exists for the scope (don't pay Opus to size zero); still accrue the cheap belief substrate. Telemetry: a "synthesis skipped: cold calibration" metric (no silent skip).
- [ ] Steps: failing test (cold-calibration scope → no paid synthesis call, metric emitted; warm scope → call proceeds) → FAIL → gate it → PASS → Commit `fix(synthesis): gate paid Mind call on calibration-readiness (F5)`.

---

## Workstream C — Generic discovery / seeding (F4, F7, F9, stale-book gate; "easy weather discovery")

### Task C1: Venue-neutral `MarketCatalog`/`MarketView`
**Files:** Create `crates/fortuna-cognition/src/catalog.rs` (a venue-neutral `MarketView { market_id, venue_id, title, category, status, ... }` + `trait MarketCatalog { async fn list(series/category) }`). Move `WeatherMarketSource` out of `kalshi::` to consume the catalog; stop `KalshiMarket` DTOs leaking into `fortuna-live` (`aeolus_venue.rs:42`, `daemon.rs:2276/2360`); drop hardcoded `venue:"kalshi"` (`aeolus_venue.rs:165`). (Spec §2.1; audit Area 5.)
- [ ] Steps: failing test (discovery consumes `MarketView`, no `KalshiMarket` type crosses into `fortuna-live`; a guard grep) → FAIL → introduce the abstraction + adapt the Kalshi adapter to emit `MarketView` → PASS (existing weather path still green — regression) → Commit `refactor: venue-neutral MarketCatalog (decouple discovery from Kalshi; Area 5)`.

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

### Task D3: Meteorologist authors weather beliefs (parallel to Aeolus)
**Files:** Wire the meteorologist persona to trigger on NWS AFD/alerts/CLI for existing weather events → `domain_analysis` + belief `producer="meteorologist"` (parallel to Aeolus; sees Aeolus + market in context per spec §5). Test: cognition (a scripted-Mind persona run yields a belief + analysis).
- [ ] Steps: failing test (meteorologist trigger → domain_analysis + per-bracket belief, `producer=meteorologist`) → FAIL → wire trigger+context+persist → PASS → Commit `feat(personas): meteorologist authors weather beliefs (the intelligence arm)`.

### Task D4: Per-producer scoring + synthesis prices best-calibrated
**Files:** Verify belief scoring keys per `producer` (aeolus vs meteorologist scored independently); modify `crates/fortuna-runner/src/synthesis.rs` to price the calibrated producer (or ensemble). Telemetry: per-producer Brier/CLV. Test: cognition.
- [ ] Steps: failing test (two producers on one event scored independently; synthesis prices the calibrated one) → FAIL → impl → PASS → Commit `feat: per-producer scoring; synthesis prices calibrated producer (the thesis)`.

---

## Workstream E — Demo surface (F5-CLI, F6, F8, F11-proper, ROTA chain-view)

### Task E1: `fortuna doctor` readiness check
**Files:** `crates/fortuna-cli/src/main.rs` (new `doctor` verb): checks DB reachable, migrations applied, required env/creds present, mode safe, `funding_rates_historical` GRANT present (F6-grant), Aeolus source reachable (F6). Test: cli.
- [ ] Steps: failing test (doctor flags a missing GRANT / unreachable DB) → FAIL → impl → PASS → Commit `feat(cli): fortuna doctor readiness check`.

### Task E2: `fortuna start paper-demo` (fresh DB, no order path) + pointer-write (F11-proper)
**Files:** `crates/fortuna-cli/src/main.rs` (new `start paper-demo`: provision a fresh DB, apply migrations + GRANTs, set `execution_mode=live_data_only`/`paper_ledger`, run `doctor`, start daemon; daemon writes the **true** live `DATABASE_URL` to `current-demo-db-url` on boot — F11 proper). Test: cli/boot.
- [ ] Steps: failing test (paper-demo boots with no constructible order path; pointer reflects the live DB) → FAIL → impl → PASS → Commit `feat(cli): fortuna start paper-demo (fresh DB, no order path); daemon writes live db pointer (F11)`.

### Task E3: ROTA chain-view + safety pills (audit #6)
**Files:** `crates/fortuna-live/src/daemon.rs:1421-1438` (emit `execution_mode`/`order_mutation_enabled`/book-freshness into the health view) + `crates/fortuna-ops/src/rota.rs:2107` (render safety pills + a per-market chain panel: signal→belief(by producer)→proposal→gate→fill→settle→score). Test: ops/rota.
- [ ] Steps: failing test (health view carries `order_mutation_enabled`; chain panel renders a market's chain) → FAIL → emit + render → PASS → Commit `feat(rota): chain-view + execution-mode/order-mutation safety pills (audit #6)`.

### Task E4: Dead-man ping + ops hardening (F8)
**Files:** Investigate/fix the `dead-man ping FAILED: transport failure` (`daemon.rs` dead-man + monitor endpoint); ensure the heartbeat reconnects + alerts on failure (telemetry). Test: live/ops.
- [ ] Steps: failing test (ping failure is retried + surfaced, not silent) → FAIL → fix → PASS → Commit `fix(ops): dead-man ping reconnect + alert (F8)`.

### Task E5: Demo runbook + Aeolus stable-source note (F6)
**Files:** `docs/runbooks/paper-demo.md` (the one runbook: `doctor` → `start paper-demo` → ROTA → daily/weekly cadence → kill switch), document the Aeolus stable-URL + rolling-date requirement (F6). Update CHANGELOG. (Docs task; no code.)
- [ ] Steps: write the runbook + CHANGELOG entry → verify links resolve → Commit `docs: paper-demo runbook + Aeolus stable-source note (F6)`.

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
| F4 world-forward / #8 / MVP | resolution_source match | **C2** |
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
- **Type consistency:** A1 produces `FillsRepo::insert`/`SettlementsRepo::insert_entry`/`RecordingsRepo` consumed by A2/A3/A6; `MarketView` (C1) consumed by C2/C4; `producer` keying (A4) consumed by D4. ✓
- **Build order:** A (spine) → B (model arm) → C (discovery) → D (persona) → E (demo surface); each ships a testable deliverable; all together = Phase C DoD.
