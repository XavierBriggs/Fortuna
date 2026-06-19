# WS1 — Live Spine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax. Each task's implementer investigates the real code against the task brief; this plan gives files, interfaces, the approach, and the **three required test layers** (unit/property · integration · live-smoke) per slice.
>
> **Revised 2026-06-19 after the plan-verify gate (6 Important findings folded in).**

**Goal:** Make the FORTUNA learning loop measurably close and score for *every* producer (Aeolus + meteorologist), compute CLV live, key scoring per-producer, and make `go_nogo` tell the calibration truth — the foundation the proof layer (WS2), backtest (WS3), and demo (WS4) build on.

**Architecture:** Weather-belief resolution today is Aeolus-only (`open_aeolus_weather_due` filters `provenance->>'model_id'='aeolus'`; the resolver strips an `aeolus:` event-id prefix; `parse_bracket_hint` is integer-only). WS1 makes resolution **producer-agnostic by DATA** (a belief is resolvable when it carries the weather grading keys `nws_station_id`/`variable`/`target_date`), handles BOTH event-id grammars and **fractional** bracket thresholds, wires the already-built `clv_bps` into the live resolver (with an idempotency migration for the capturer), adds per-producer scoring queries + deterministic best-producer selection, and extends `go_nogo` with the spec §11 Brier gate (+ a forward-only promotion filter).

**Tech Stack:** Rust 2021 workspace; Postgres via `sqlx` (compile-time-checked, OFFLINE mode); `tokio` at edges, single-threaded deterministic core; all time via injected `Clock`.

**Scope note (G2):** WS1 builds **persona** binary resolution + the unified dispatch. **Synthesis** binary resolution is DEFERRED (synthesis provenance carries no `producer`/grading keys — mind.rs:649; the demo head-to-head is Aeolus-vs-meteorologist) — recorded in `docs/superpowers/loop-close-gaps.md` + `loop-close-operator.md`.

## Global Constraints (verbatim from spec + constitution)

- Money is `Cents(i64)`; `Decimal` only at conversion boundaries; **probabilities/temperature comparisons are `f64` in cognition only**. **No `panic!`/`unwrap`/`expect`/`f64`-for-money in gates/exec/state/venues.** (Bracket-threshold comparison is a temperature path in cognition — `f64`/`Decimal` is allowed there.)
- **Decoupling (load-bearing):** producer/domain/venue are DATA. The shared resolver queue, the resolver spine, and best-producer selection carry **NO** producer/domain literal (no `if producer=="aeolus"`, no `model_id IN (...)`). Producer-instance files (`aeolus_beliefs.rs`) MAY carry their own literal. **NB:** the A7 guard (`i_decoupling_spine.rs`) scans only `fortuna-gates`/`-exec`/`-state` and EXCLUDES `fortuna-live`/`-ledger` — exactly where WS1's spine changes land. So WS1 decoupling is enforced by the **task-local no-literal assertions (Tasks 2, 3, 7)**, NOT by A7. Re-run A7 anyway (it must stay green), but do not claim it covers these changes.
- **Invariants:** I1 gate unchanged; I5 append-only (`resolve_and_score` set-once is app-layer `WHERE outcome IS NULL` + rows_affected guard — reuse, don't weaken; the `price_snapshots` append-only trigger refuses UPDATE/DELETE — `ON CONFLICT DO NOTHING` is INSERT-time, not a mutation, so it's allowed); I6 propose-only; I7 operator-only promotion; I4 killswitch dep-graph. `crates/fortuna-invariants/` is additions-only; protected baseline **`dadd28a`**.
- `cargo fmt --all --check` (use `if cargo fmt --all --check; then …`), `clippy --workspace --all-targets -D warnings`, full suite, `bash scripts/run-dst.sh` all green before a checkbox ticks. After any SQL change: drop+create `fortuna`, migrate, `SQLX_OFFLINE=true cargo sqlx prepare --workspace -- --all-targets`, commit `.sqlx/`.
- Ledger `#[sqlx::test]` via `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-ledger`.
- **Three test layers per slice** (where applicable): unit/property · integration (`#[sqlx::test]`) · live-smoke (`daemon_smoke.rs` / boot). A slice with only unit tests is NOT done.

## File-structure map

| File | Responsibility | WS1 change |
|---|---|---|
| `crates/fortuna-cognition/src/aeolus_beliefs.rs:~97-108` | Aeolus binary weather-belief author (producer-instance) | + `producer:"aeolus"` (already has nws_station_id/variable/model_id/target_date) |
| `crates/fortuna-cognition/src/persona_beliefs.rs:~86,~95-116` | Persona belief author | + `producer:persona_id` + grading keys on the weather (`ge`) path |
| `crates/fortuna-cognition/src/aeolus_resolve.rs:72-105` | bracket parse + scorer | widen `parse_bracket_hint` to fractional; per-grammar hint |
| `crates/fortuna-ledger/src/repos.rs` | repos + queries | `open_weather_bracket_due`; `SnapshotsRepo::insert` ON CONFLICT; `resolved_stats_for_producer`; `scores_for_producer`; forward-only filter |
| `crates/fortuna-ledger/migrations/` | schema | + migration: `UNIQUE(market_id, at)` on `price_snapshots` |
| `crates/fortuna-live/src/daemon.rs` | resolvers + book poll + StrategyRecord build (`:5182`) | producer-agnostic resolution + dispatch; CLV capture + compute; best-producer selection; Brier wiring |
| `crates/fortuna-live/src/compose.rs:58` | calibration scope assembly | `calibration_for_scope(..., producer)` |
| `crates/fortuna-cognition/src/review.rs:160-301` | `StrategyRecord` + `go_nogo` | + `brier`/`market_baseline_brier` fields + Brier-beats-baseline gate |

---

## Task 1: Uniform `provenance.producer` + persona weather grading keys

**Files:** `aeolus_beliefs.rs:~97-108`, `persona_beliefs.rs:~86` (provenance) + `~95-116` (the `ge` threshold loop). Test: `crates/fortuna-cognition/tests/`.

**Interfaces — Produces:** every binary weather belief carries `provenance->>'producer'` (`"aeolus"` | persona_id) AND `nws_station_id`/`variable`/`target_date`.

**Verified de-risk:** the `beliefs` table has NO content_hash/method_hash column (repos.rs:1115-1132 persists only id/created_at/event_id/p/p_raw/horizon/evidence/provenance/supersedes). The hashes to protect are the persona **artifact** hash (`persona.rs:122`, hashes `persona_md`) and the **context-item** hash (`persona_beliefs.rs:261`, hashes the findings `body`) — neither takes provenance. So stamping provenance cannot change any hash or fixture.

- [ ] **Step 1 — failing unit tests:** (a) Aeolus belief provenance has `producer=="aeolus"`; (b) a meteorologist weather (`ge`) belief has `producer==<persona_id>` AND `nws_station_id`/`variable`/`target_date` matching its region_key (`weather:KNYC:tmax:2026-06-12` → `KNYC`/`tmax`/`2026-06-12`); (c) a meteorologist macro (`out:`/`macro:`) belief has `producer` but NO grading keys. FAIL.
- [ ] **Step 2 — implement:** add `"producer"` to both provenance builders. In the persona weather (`ge`) path, parse station/variable/date from `region_key` by **positional `:`-split** (`weather`/STATION/variable/DATE — this is NEW logic; `belief_horizon` at persona_beliefs.rs:158 only extracts the date, so reuse only its `is_iso_date` check, not its single-field return). Do NOT stamp grading keys on the macro path. Parse defensively (no `unwrap`; on a malformed key omit the key + debug-log, with a test for the malformed case).
- [ ] **Step 3 — regression (integration/fixtures):** the persona **replay-chain test + the 7 persona fixtures** pass; the persona-artifact hash (`persona.rs:122`) and context-item hash (`persona_beliefs.rs:261`) are byte-identical (provenance is not hashed). PASS.
- [ ] **Step 4:** `cargo test -p fortuna-cognition`; fmt/clippy. Commit `feat(beliefs): uniform provenance.producer + persona weather grading keys (WS1.1, D4 Part 0/1)`.

---

## Task 2: Producer-agnostic resolver queue (`open_weather_bracket_due`)

**Files:** `repos.rs:~1342-1368` (replace `open_aeolus_weather_due`), `daemon.rs:~4643` (caller), `.sqlx/`. Test: `crates/fortuna-ledger/tests/` (`#[sqlx::test]`).

**Interfaces — Produces:** `BeliefsRepo::open_weather_bracket_due(&self, horizon_max: &str, limit: i64) -> Result<Vec<WeatherBracketDue>>`; `WeatherBracketDue { belief_id, event_id, p, nws_station_id, variable, target_date, horizon, producer }`.

- [ ] **Step 1 — failing integration test (`#[sqlx::test]`):** seed an Aeolus weather belief, a meteorologist weather belief, a persona macro belief, a funding scalar belief. `open_weather_bracket_due` returns the two weather beliefs (both producers), NOT the macro or funding belief. FAIL.
- [ ] **Step 2 — implement:** WHERE = `status='open' AND provenance->>'nws_station_id' IS NOT NULL AND provenance->>'variable' IS NOT NULL AND provenance->>'target_date' IS NOT NULL AND horizon<=$1`; also SELECT `provenance->>'producer' AS producer`. No producer literal. Update the daemon caller.
- [ ] **Step 3 — decoupling assertion (unit):** the query string contains no `'aeolus'`/`'meteorologist'` literal.
- [ ] **Step 4:** regen `.sqlx`; `cargo test -p fortuna-ledger`; full invariant suite incl. A7; fmt/clippy. Commit `feat(ledger): producer-agnostic weather-bracket resolver queue (decoupled by grading keys)`.

---

## Task 3: Producer-agnostic + fractional resolution (grammar-agnostic bracket hint)

> **Scope refined 2026-06-19 (captain, YAGNI):** the unified `PredictiveKind`→`BrierRule` **trait dispatch is DEFERRED to WS2/G5** (a no-op with only one rule; G5's RPS/Log give it purpose). Slice 3 = producer-agnostic + fractional resolution via a grammar-agnostic hint (removes the daemon.rs:4723 `aeolus:` literal); this fully delivers the head-to-head. See `loop-close-operator.md`.

**Files:** `daemon.rs` `resolve_and_score_weather_beliefs` (~4637-4741), `aeolus_resolve.rs:72-105` (`parse_bracket_hint`/`score_bracket`). Test: `crates/fortuna-live/tests/` + `daemon_smoke.rs` + `crates/fortuna-cognition/tests/`.

**Interfaces — Consumes:** `open_weather_bracket_due` (T2). **Produces:** every due weather belief (any producer, integer OR fractional bracket, either event-id grammar) gets `outcome` + `brier` via `resolve_and_score`, scored by the `ScoringRule` selected by `PredictiveKind::Binary → BrierRule`.

**Two event-id grammars** (the hint must handle both): Aeolus `aeolus:knyc-2026-06-13-tmax-ge87` (strip `aeolus:`); persona `weather:KNYC:tmax:2026-06-12#ge87` (take the token after `#`). **Fractional thresholds:** Kalshi brackets are fractional (`B87.5`, §7 worked example); persona thresholds come from `as_f64()` via `trim_num` so can be `ge87.5`. `parse_bracket_hint` currently does `i64::parse` → fractional silently becomes `None` → the belief is left UNSCORED.

- [ ] **Step 1 — failing tests:** (a) Aeolus + meteorologist beliefs for the SAME integer bracket + same `p` + a realized NWS CLI outcome → BOTH get the SAME `(outcome, brier)`; (b) a **FRACTIONAL** bracket (`ge87.5`) resolves correctly for BOTH producers; (c) both grammars (`aeolus:`-prefix and `#`-suffix) derive the same hint; (d) Aeolus integer golden → byte-identical `(outcome,brier)` vs pre-change. FAIL.
- [ ] **Step 2 — implement:** (i) widen `parse_bracket_hint` to return a **`Decimal`/`f64` threshold** and update `score_bracket`'s comparison (`realized_f` is already `f64`); behavior-preserving for integer brackets. (ii) Derive the `event_hint` per-grammar (strip `aeolus:` OR take after `#`); build the canonical comparison from the grading-key provenance (station/variable/date) + the suffix. (iii) Dispatch scoring by `PredictiveKind::Binary → BrierRule` (`BrierRule::score` gives identical `(p−o)²` to `score_bracket`'s `brier_score` for the same boolean). Realized lookup (`nws_cli_realized`) unchanged.
- [ ] **Step 3 — mutation-proof:** reverting the threshold to `i64::parse` leaves the fractional belief unscored → test (b) REDs.
- [ ] **Step 4 — live-smoke:** `daemon_smoke.rs` — a seeded meteorologist weather belief is resolved + scored over a daemon cycle.
- [ ] **Step 5:** DST; full invariant suite (i4 + A7); fmt/clippy. Commit `feat(live): producer-agnostic + fractional weather resolution + PredictiveKind dispatch (meteorologist scored vs Aeolus — the thesis)`.

---

## Task 4: CLV-live — capture (`price_snapshots` writer + idempotency migration)

**Files:** `daemon.rs` (the existing book-poll segment), a new migration in `crates/fortuna-ledger/migrations/`, `SnapshotsRepo::insert` (repos.rs:791-823), `.sqlx/`. Test: `crates/fortuna-ledger/tests/` + `daemon_smoke.rs`.

**Schema reality:** `price_snapshots(snapshot_id PK, market_id, venue, event_id→events, kind CHECK IN ('t24h','t1h','t5m','on_trade','other'), best_bid_cents, best_ask_cents, bid_qty, ask_qty, liquidity_ok BOOL, at)`; PK is a ULID, NO unique on `(market_id, at)`, append-only trigger refuses UPDATE/DELETE. **Task 5's `latest_liquid_before(market_id, event_id, cutoff)` needs `event_id` — so captured snapshots MUST carry `event_id`.**

**Interfaces — Produces:** idempotent `price_snapshots` rows carrying `event_id`. Degraded/one-sided books are recorded with `liquidity_ok=false` + `kind='other'` (NO CHECK change). Horizon marks beyond the CHECK vocab (T-6h/T-1m) map to `'other'`.

- [ ] **Step 1 — failing integration test (`#[sqlx::test]`):** two `insert`s for the same `(market_id, at)` → ONE row (idempotent); a one-sided/empty book → a row with `liquidity_ok=false`, `kind='other'` (not a fake price); every captured row has a non-null `event_id`. FAIL.
- [ ] **Step 2 — migration + implement:** migration `CREATE UNIQUE INDEX idx_price_snapshots_market_at ON price_snapshots (market_id, at)`. `SnapshotsRepo::insert` → `INSERT ... ON CONFLICT (market_id, at) DO NOTHING` (INSERT-time, not a mutation → the append-only trigger is not fired). In the book-poll segment, sample each polled market's book → `insert` with `event_id`, liquidity gate (`liquidity_ok`), `kind` from the horizon mark (fallback `'other'`), `at` from the injected `Clock`. Cursor-driven (restart-safe).
- [ ] **Step 3 — live-smoke:** `daemon_smoke.rs` — after a poll cycle, `price_snapshots` is non-empty WITH `event_id` for a tracked market.
- [ ] **Step 4:** regen `.sqlx`; DST; fmt/clippy; full suite (the new UNIQUE index must not break existing append-only/i5 tests). Commit `feat(live): CLV capturer writes price_snapshots (idempotent via UNIQUE(market_id,at), liquidity-gated, carries event_id)`.

---

## Task 5: CLV-live — compute in the resolver

**Files:** `daemon.rs` (both resolvers' persist path). Test: `crates/fortuna-live/tests/`.

**Interfaces — Consumes:** `events::clv_bps` (events.rs:314, `-> Option<i64>`); `SnapshotsRepo::latest_liquid_before(market_id, event_id, cutoff)` (repos.rs:830); the benchmark policy (config). **Produces:** a resolved TRADED belief carries `clv_bps` (persisted as `Option<f64>` into the `DOUBLE PRECISION` column via `resolve_and_score`).

- [ ] **Step 1 — failing integration test:** a traded belief with a price-snapshot series gets a non-null `clv_bps` at resolution = `devig(benchmark) − devig(entry)` (benchmark = `latest_liquid_before` the configured pre-horizon mark); a belief with no entry/snapshots gets `None`. FAIL.
- [ ] **Step 2 — implement:** select the benchmark via `latest_liquid_before` (market_id + event_id) per the config policy, call `clv_bps`, widen to `f64`, persist via `resolve_and_score` (replacing today's `None`). De-vig per the single-contract Kalshi convention. **Save the per-belief market-implied p (de-vigged benchmark) for Task 8b's baseline** (thread it out, or recompute there from the same snapshot).
- [ ] **Step 3 — live-smoke:** `daemon_smoke.rs` — a seeded traded belief with snapshots resolves with a populated `clv_bps`.
- [ ] **Step 4:** DST; fmt/clippy; full suite. Commit `feat(live): CLV computed live at resolution (the fast gate goes live; G1)`.

---

## Task 6: Per-producer scoring queries

**Files:** `repos.rs` (+2 methods), `.sqlx/`. Test: `crates/fortuna-ledger/tests/` (`#[sqlx::test]`).

**Interfaces — Produces:** `BeliefsRepo::resolved_stats_for_producer(producer, category) -> Vec<ResolvedStat>` (binary, adds `AND provenance->>'producer'=$1`); `BeliefScoresRepo::scores_for_producer(producer, rule_id, limit) -> Vec<BeliefScoreRow>` (scalar, joins on `scalar_beliefs.producer`).

- [ ] **Step 1 — failing integration test:** belief A (`producer=aeolus`) + B (`producer=meteorologist`) on the same event/category, both resolved+scored → `resolved_stats_for_producer("aeolus",cat)` returns ONLY A; `("meteorologist",cat)` ONLY B; merged `resolved_stats(cat)` returns BOTH. Mutation: dropping the producer filter REDs this. FAIL.
- [ ] **Step 2 — implement** both methods; regen `.sqlx`.
- [ ] **Step 3:** `cargo test -p fortuna-ledger`; full suite; fmt/clippy. Commit `feat(ledger): per-producer resolved-stats + scores queries (D4 measurement)`.

---

## Task 7: Synthesis prices the best-calibrated producer (deterministic, data-driven)

**Files:** `crates/fortuna-live/src/compose.rs:58` (`calibration_for_scope`), `daemon.rs` (the B3 synthesis-refresh block). Test: `crates/fortuna-cognition/tests/` + `crates/fortuna-live/tests/`.

**Reconcile:** `calibration_for_scope(params, beliefs, model_id, strategy, category, kind)` already takes `model_id: &str` (the producer/model scope key). Add per-producer selection: either pass the chosen producer as `model_id`, or add `producer: Option<&str>` if `model_id` is semantically distinct — the implementer reads the function first and states which. **Interfaces — Consumes:** `resolved_stats_for_producer` (T6), `calibration_quality` (calibration.rs:347). **Produces:** synthesis fed the best-calibrated producer's calibration.

- [ ] **Step 1 — failing test:** two producers, different `calibration_quality` (aeolus high, meteorologist low) → selection returns `aeolus` (quality DESC); flip → `meteorologist`; equal → producer-id ASC. Candidate set = producers WITH resolved beliefs in scope (DATA), not a hardcoded list. FAIL.
- [ ] **Step 2 — implement:** per-producer calibration via `resolved_stats_for_producer`; daemon computes each candidate's quality, picks max (tie → producer ASC), feeds the selected producer's calibration to synthesis. **No `aeolus`/`meteorologist` literal.**
- [ ] **Step 3 — decoupling + integration:** assert no producer literal in the selection path; synthesis warms with the selected producer's params; no producer warm → stays cold (B3 gate).
- [ ] **Step 4:** DST; full invariant suite incl. A7; fmt/clippy. Commit `feat(synthesis): price the best-calibrated producer (data-driven; the thesis payoff)`.

---

## Task 8a: `forward_only` promotion filter (exclude backtest rows from the GO count)

**Files:** the resolved-belief query feeding the go_nogo inputs — `crates/fortuna-live/src/compose.rs` / `repos.rs` resolved-stats/count queries (the implementer traces from `daemon.rs:5182` where `StrategyRecord` is built). `.sqlx/`. Test: `crates/fortuna-ledger/tests/` + `crates/fortuna-live/tests/`.

**Interfaces — Produces:** the resolved-belief counts/samples feeding `go_nogo` exclude `provenance->>'source' = 'historical-import'` (the §9.1 decision — a green GO never rests on backtested data).

- [ ] **Step 1 — failing integration test:** a scope with a mix of forward + `source='historical-import'` resolved beliefs → the go_nogo input count includes ONLY the forward ones. Mutation: removing the filter counts the imported rows → REDs. FAIL.
- [ ] **Step 2 — implement:** add `AND provenance->>'source' IS DISTINCT FROM 'historical-import'` to the resolved-belief input query (works before any import exists — defensive). Regen `.sqlx`.
- [ ] **Step 3:** `cargo test -p fortuna-ledger -p fortuna-live`; full suite; fmt/clippy. Commit `feat(review): forward-only promotion filter — backtest rows excluded from the GO count (§9.1)`.

---

## Task 8b: `go_nogo` Brier-beats-baseline gate (synthesis scopes)

**Files:** `review.rs:160-301` (`StrategyRecord` + `go_nogo`), `daemon.rs:~5182` (StrategyRecord build site). Test: `crates/fortuna-cognition/tests/review.rs` + `crates/fortuna-live/tests/`.

**Baseline source (decision, loop-close-operator.md):** the market-implied baseline Brier = per resolved belief, de-vig the **benchmark snapshot** (the SAME one Task 5 uses for CLV) → market-implied p → `BrierRule` vs the outcome → aggregate per scope. No separate baseline source exists; reuse the captured benchmark. **Interfaces — Produces:** `go_nogo` returns NO-GO when a synthesis scope's producer Brier does not beat its market-baseline Brier.

- [ ] **Step 1 — failing unit test:** add `brier: Option<f64>` + `market_baseline_brier: Option<f64>` to `StrategyRecord`; a synthesis scope with `brier ≥ market_baseline_brier` → NO-GO reason `brier_not_beating_baseline` (today it wrongly returns GO); a mechanical scope is unaffected (synthesis-only, spec §11). FAIL.
- [ ] **Step 2 — implement:** thread producer Brier + the de-vigged market-baseline Brier onto `StrategyRecord` at the daemon build site (daemon.rs:5182, using the per-scope resolved beliefs + their benchmark snapshots); add the synthesis-only Brier-beats-baseline branch to `go_nogo`. Thresholds use the SPEC values (paper ≥30 / ≥60 beliefs / fee <0.35; GAPS divergence noted).
- [ ] **Step 3 — integration:** `go_nogo` over real scored rows (a synthesis scope whose Brier beats / doesn't beat its baseline) → correct verdict + reasons.
- [ ] **Step 4:** full suite; fmt/clippy; DST. Commit `feat(review): go_nogo Brier-beats-baseline gate for synthesis (spec §11; close the silent omission)`.

---

## Self-review checklist (post-revision)
- **Spec coverage:** G1 (Tasks 4-5), G2 unified dispatch + **persona** resolution (Tasks 1-3; **synthesis binary resolution explicitly DEFERRED** — not silently dropped), G4 per-producer keying (Tasks 1,6,7), go_nogo §11 Brier gate (Task 8b) + §9.1 forward-only (Task 8a). All accounted; synthesis deferral recorded.
- **Type consistency:** `open_weather_bracket_due`/`WeatherBracketDue`, `resolved_stats_for_producer`, `scores_for_producer`, `calibration_for_scope` (fortuna-live), `StrategyRecord{brier, market_baseline_brier}` referenced consistently.
- **Decoupling:** Tasks 2,3,7 carry explicit no-producer-literal assertions (the real guard — A7 excludes live/ledger).
- **Citations corrected:** `calibration_for_scope` → `fortuna-live/src/compose.rs:58`; the content-hash de-risk targets the artifact/context-item hashes (the `beliefs` table has no hash column); Task 4 interface matches the real `insert` signature.
