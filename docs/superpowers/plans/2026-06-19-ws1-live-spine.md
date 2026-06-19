# WS1 — Live Spine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax. Each task's implementer investigates the real code against the task brief; this plan gives files, interfaces, the approach, and the **three required test layers** (unit/property · integration · live-smoke) per slice.

**Goal:** Make the FORTUNA learning loop measurably close and score for *every* producer (Aeolus + meteorologist), compute CLV live, key scoring per-producer, and make `go_nogo` tell the calibration truth — the foundation the proof layer (WS2), backtest (WS3), and demo (WS4) build on.

**Architecture:** Weather-belief resolution today is Aeolus-only (`open_aeolus_weather_due` filters `provenance->>'model_id'='aeolus'`; the resolver strips an `aeolus:` event-id prefix). WS1 makes resolution **producer-agnostic by DATA** (a belief is resolvable when it carries the weather grading keys `nws_station_id`/`variable`/`target_date`), unifies scoring on the `ScoringRule` trait dispatched by `PredictiveKind`, wires the already-built `clv_bps` into the live resolver, adds per-producer scoring queries + deterministic best-producer selection, and extends `go_nogo` with the spec §11 Brier gate.

**Tech Stack:** Rust 2021 workspace; Postgres via `sqlx` (compile-time-checked, OFFLINE mode); `tokio` at edges, single-threaded deterministic core; all time via injected `Clock`.

## Global Constraints (verbatim from spec + constitution)

- Money is `Cents(i64)`; `rust_decimal::Decimal` only at conversion boundaries; probabilities are `f64` in cognition only. **No `panic!`/`unwrap`/`expect`/`f64`-for-money in gates/exec/state/venues.**
- **Decoupling (load-bearing):** producer/domain/venue/strategy/mind are DATA. The shared resolver queue, the resolver spine, and best-producer selection carry **NO** producer/domain literal (no `if producer=="aeolus"`, no `model_id IN (...)`). Producer-instance files (`aeolus_beliefs.rs`, `aeolus_venue.rs`) MAY carry their own instance literal. The A7 guard (`crates/fortuna-invariants/tests/i_decoupling_spine.rs`) is re-run on any spine touch.
- **Invariants:** I1 gate unchanged; I5 append-only (`resolve_and_score` set-once is app-layer WHERE `outcome IS NULL` + rows_affected guard — reuse, don't weaken); I6 propose-only; I7 operator-only promotion; I4 killswitch dep-graph. `crates/fortuna-invariants/` is additions-only; protected baseline **`dadd28a`** (`scripts/check-protected-invariants.sh dadd28a`).
- `cargo fmt --all --check` (use `if cargo fmt --all --check; then …` — piping masks the exit code), `clippy --workspace --all-targets -D warnings`, full suite, and `bash scripts/run-dst.sh` all green before a checkbox ticks. After any SQL change: drop+create `fortuna`, migrate, `SQLX_OFFLINE=true cargo sqlx prepare --workspace -- --all-targets`, commit `.sqlx/`.
- Ledger `#[sqlx::test]` runs via `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-ledger` (the `fortuna_app` role lacks CREATEDB; superuser socket).
- **Three test layers per slice** (where applicable): unit/property · integration (`#[sqlx::test]`, real Postgres) · live-smoke (`daemon_smoke.rs` / boot / a real-data exercise). A slice with only unit tests is NOT done.

## File-structure map

| File | Responsibility | WS1 change |
|---|---|---|
| `crates/fortuna-cognition/src/aeolus_beliefs.rs` | Aeolus binary weather-belief author (producer-instance) | + `producer:"aeolus"` in provenance (already has nws_station_id/variable/model_id/target_date, :103-104) |
| `crates/fortuna-cognition/src/persona_beliefs.rs` | Persona (meteorologist) belief author | + `producer:persona_id` + grading keys on the weather (`ge`) path |
| `crates/fortuna-ledger/src/repos.rs` | Ledger repos + queries | `open_weather_bracket_due` (generalize); `resolved_stats_for_producer`; `scores_for_producer` |
| `crates/fortuna-live/src/daemon.rs` | Daemon: resolvers + book poll | producer-agnostic resolution + `PredictiveKind` dispatch; CLV capture + compute; best-producer selection |
| `crates/fortuna-cognition/src/scoring.rs` | `ScoringRule` trait + rules | (read-only this WS — dispatch consumes `BrierRule`) |
| `crates/fortuna-cognition/src/compose.rs` | calibration scope assembly | `calibration_for_scope(..., producer: Option<&str>)` |
| `crates/fortuna-cognition/src/review.rs` | `go_nogo` verdict | + Brier-beats-baseline gate (synthesis) + `forward_only` filter |

---

## Task 1: Uniform `provenance.producer` + persona weather grading keys

**Files:**
- Modify: `crates/fortuna-cognition/src/aeolus_beliefs.rs:~100-105` (provenance json)
- Modify: `crates/fortuna-cognition/src/persona_beliefs.rs:~86` (provenance json) + the weather `ge` path (~108)
- Test: `crates/fortuna-cognition/tests/` (persona_beliefs / aeolus_beliefs unit tests)

**Interfaces:**
- Produces: every binary weather belief carries `provenance->>'producer'` (`"aeolus"` | persona_id) AND `nws_station_id`/`variable`/`target_date`. Later tasks select + key on these.

**Investigation note:** the belief CONTENT hash is `content_hash_of(&body)` (analysis body) and `method_hash` is `content_hash_of(persona_md)` — neither includes `provenance` (verified persona_beliefs.rs:261, persona.rs:122). Stamping provenance is therefore fixture-safe; confirm by running the persona fixtures + replay-chain test after.

- [ ] **Step 1 — failing unit test:** an Aeolus belief's provenance has `producer="aeolus"`; a meteorologist weather belief's provenance has `producer=<persona_id>` AND `nws_station_id`/`variable`/`target_date` (extracted from its `region_key` `weather:STATION:variable:date`); a persona MACRO (`out:`) belief has `producer` but NO grading keys. Run → FAIL.
- [ ] **Step 2 — implement:** add `"producer"` to both provenance builders; in the persona weather (`ge`) path, parse station/variable/date from `region_key` (mirror the `belief_horizon` parse at persona_beliefs.rs:158) and stamp `nws_station_id`/`variable`/`target_date`. Do NOT stamp grading keys on the macro path.
- [ ] **Step 3 — content-hash regression (integration):** persist a persona belief; assert `content_hash`/`method_hash` are byte-identical to a golden (provenance is metadata); run the persona replay-chain test + the 7 persona fixtures → PASS.
- [ ] **Step 4:** fmt/clippy; `cargo test -p fortuna-cognition`; commit `feat(beliefs): uniform provenance.producer + persona weather grading keys (D4 Part 0/1)`.

---

## Task 2: Producer-agnostic resolver queue (`open_weather_bracket_due`)

**Files:**
- Modify: `crates/fortuna-ledger/src/repos.rs:~1342-1368` (rename/replace `open_aeolus_weather_due`)
- Modify: `crates/fortuna-live/src/daemon.rs:~4643` (caller)
- Regenerate: `.sqlx/`
- Test: `crates/fortuna-ledger/tests/` (`#[sqlx::test]`)

**Interfaces:**
- Produces: `BeliefsRepo::open_weather_bracket_due(&self, horizon_max: &str, limit: i64) -> Result<Vec<WeatherBracketDue>>` selecting ANY open belief carrying the grading keys. `WeatherBracketDue` carries `belief_id, event_id, p, nws_station_id, variable, target_date, horizon, producer`.

- [ ] **Step 1 — failing integration test (`#[sqlx::test]`):** seed an Aeolus weather belief, a meteorologist weather belief, a persona macro belief, and a funding scalar belief. `open_weather_bracket_due(...)` returns the two weather beliefs (both producers), and NOT the macro or funding belief. Run → FAIL.
- [ ] **Step 2 — implement:** replace the WHERE clause `provenance->>'model_id'='aeolus'` with `provenance->>'nws_station_id' IS NOT NULL AND provenance->>'variable' IS NOT NULL AND provenance->>'target_date' IS NOT NULL`; also SELECT `provenance->>'producer' AS producer`. No producer literal. Update the daemon caller name.
- [ ] **Step 3 — decoupling assertion (unit):** a test asserts the query string contains no `'aeolus'`/`'meteorologist'` literal (guards regression).
- [ ] **Step 4 — regen + verify:** drop/create/migrate, `cargo sqlx prepare`; `SQLX_OFFLINE=true cargo test -p fortuna-ledger`; full invariant suite incl. A7 guard; fmt/clippy. Commit `feat(ledger): producer-agnostic weather-bracket resolver queue (decoupled by grading keys)`.

---

## Task 3: Producer-agnostic resolution + unified `PredictiveKind` dispatch (the head-to-head core)

**Files:**
- Modify: `crates/fortuna-live/src/daemon.rs` `resolve_and_score_weather_beliefs` (~4637-4741)
- Test: `crates/fortuna-live/tests/` + `daemon_smoke.rs`

**Interfaces:**
- Consumes: `open_weather_bracket_due` (Task 2); `score_bracket` (aeolus_resolve.rs:101) OR `BrierRule` (scoring.rs:248) via `PredictiveKind::Binary`.
- Produces: every due weather belief (any producer) gets `outcome` + `brier` via `resolve_and_score`, scored by the `ScoringRule` selected by `PredictiveKind`.

- [ ] **Step 1 — failing integration test:** seed an Aeolus belief and a meteorologist belief for the SAME (station,date,variable,bracket) with the same `p`; provide a realized NWS CLI outcome; run the resolver. BOTH get the SAME `(outcome, brier)`. Separately: an Aeolus-only golden asserts byte-identical `(outcome,brier)` vs pre-change (regression). Run → FAIL.
- [ ] **Step 2 — implement:** derive the `event_hint` from the grading keys + bracket suffix (drop the `aeolus:`-prefix requirement — build `{station_lower}-{date}-{variable}-{suffix}`, valid for both event-id shapes); dispatch scoring by `PredictiveKind::Binary → BrierRule` (replacing the bare `score_bracket` call where it sits behind the trait — keep `score_bracket`'s threshold/direction parse if `BrierRule` needs the realized boolean). The realized-temperature lookup (`nws_cli_realized`) is unchanged (already producer-agnostic).
- [ ] **Step 3 — unit:** dispatch selects `BrierRule` for a `Binary` belief; funding path still uses `CrpsPinballRule` (unchanged).
- [ ] **Step 4 — live-smoke:** extend `daemon_smoke.rs` — a seeded meteorologist weather belief is resolved + scored over a daemon cycle (was silently skipped before).
- [ ] **Step 5:** DST corpus; full invariant suite (esp. i4 + A7); fmt/clippy. Commit `feat(live): producer-agnostic weather resolution + PredictiveKind dispatch (meteorologist now scored vs Aeolus — the thesis)`.

---

## Task 4: CLV-live — capture (`price_snapshots` writer)

**Files:**
- Modify: `crates/fortuna-live/src/daemon.rs` (the existing book-poll segment — sample it; do NOT add a second poller)
- Test: `crates/fortuna-ledger/tests/` (`#[sqlx::test]`) + `daemon_smoke.rs`

**Interfaces:**
- Consumes: `SnapshotsRepo::insert` (repos.rs:791); the daemon's existing book quotes.
- Produces: `price_snapshots` rows `(market_id, snapshot_at, yes_bid, yes_ask, mid, …, source, trigger)`, append-only, idempotent on `(market_id, snapshot_at)`.

- [ ] **Step 1 — failing integration test:** inserting two snapshots for the same `(market_id, snapshot_at)` is idempotent (one row); a liquid book persists, an empty/one-sided book records a `degraded` marker (not a fake price). Run → FAIL.
- [ ] **Step 2 — implement:** in the book-poll segment, sample each polled market's book → `SnapshotsRepo::insert` with the liquidity gate; cursor-driven so restart is safe. All timestamps from the injected `Clock`.
- [ ] **Step 3 — live-smoke:** `daemon_smoke.rs` — after a poll cycle, `price_snapshots` is non-empty for a tracked market.
- [ ] **Step 4:** regen `.sqlx` if needed; DST; fmt/clippy; full suite. Commit `feat(live): CLV capturer writes price_snapshots (liquidity-gated, idempotent, restart-safe)`.

---

## Task 5: CLV-live — compute in the resolver

**Files:**
- Modify: `crates/fortuna-live/src/daemon.rs` (both resolvers' persist path)
- Test: `crates/fortuna-live/tests/` + `crates/fortuna-cognition/tests/` (clv_bps already unit-tested)

**Interfaces:**
- Consumes: `events::clv_bps` (events.rs:314, `-> Option<i64>`); `SnapshotsRepo::latest_liquid_before` (repos.rs:830); the benchmark policy (config).
- Produces: a resolved TRADED belief carries `clv_bps` (persisted as `Option<f64>` into the `DOUBLE PRECISION` column via `resolve_and_score`).

- [ ] **Step 1 — failing integration test:** a traded belief with a price-snapshot series gets a non-null `clv_bps` at resolution = `devig(benchmark) − devig(entry)` (benchmark = last liquid snapshot before the configured pre-horizon mark); a belief with no entry/snapshots gets `None`. Run → FAIL.
- [ ] **Step 2 — implement:** in the resolve path, select the benchmark via `latest_liquid_before` per the config policy, call `clv_bps`, widen to `f64`, persist via `resolve_and_score` (replacing the `None` passed today). De-vig per the single-contract Kalshi convention.
- [ ] **Step 3 — live-smoke:** `daemon_smoke.rs` — a seeded traded belief with snapshots resolves with a populated `clv_bps`.
- [ ] **Step 4:** DST; fmt/clippy; full suite. Commit `feat(live): CLV computed live at resolution (the fast gate goes live; G1)`.

---

## Task 6: Per-producer scoring queries

**Files:**
- Modify: `crates/fortuna-ledger/src/repos.rs` (+2 methods), `.sqlx/`
- Test: `crates/fortuna-ledger/tests/` (`#[sqlx::test]`)

**Interfaces:**
- Produces: `BeliefsRepo::resolved_stats_for_producer(producer: &str, category: &str) -> Result<Vec<ResolvedStat>>` (binary, adds `AND provenance->>'producer'=$1`); `BeliefScoresRepo::scores_for_producer(producer: &str, rule_id: &str, limit: i64) -> Result<Vec<BeliefScoreRow>>` (scalar, joins on `scalar_beliefs.producer`).

- [ ] **Step 1 — failing integration test:** belief A (`producer=aeolus`) + belief B (`producer=meteorologist`) on the same event/category, both resolved+scored → `resolved_stats_for_producer("aeolus",cat)` returns ONLY A; `("meteorologist",cat)` ONLY B; the merged `resolved_stats(cat)` returns BOTH. Mutation-proof: dropping the producer filter REDs this. Run → FAIL.
- [ ] **Step 2 — implement** both methods; regen `.sqlx`.
- [ ] **Step 3:** `SQLX_OFFLINE=true cargo test -p fortuna-ledger`; full suite; fmt/clippy. Commit `feat(ledger): per-producer resolved-stats + scores queries (D4 measurement)`.

---

## Task 7: Synthesis prices the best-calibrated producer (deterministic, data-driven)

**Files:**
- Modify: `crates/fortuna-cognition/src/compose.rs` (`calibration_for_scope` gains `producer: Option<&str>`)
- Modify: `crates/fortuna-live/src/daemon.rs` (B3 synthesis-refresh block: best-producer selection)
- Test: `crates/fortuna-cognition/tests/` + `crates/fortuna-live/tests/`

**Interfaces:**
- Consumes: `resolved_stats_for_producer` (Task 6); `calibration_quality` (calibration.rs:347).
- Produces: synthesis is fed the calibration of the best-calibrated producer in scope.

- [ ] **Step 1 — failing test:** two producers, different `calibration_quality` (aeolus high, meteorologist low) → selection returns `aeolus` (quality DESC); flip the qualities → returns `meteorologist`; equal quality → producer-id ASC. Candidate set comes from the producers that HAVE resolved beliefs in scope (DATA), not a hardcoded list. Run → FAIL.
- [ ] **Step 2 — implement:** `calibration_for_scope(..., Some(p))` uses `resolved_stats_for_producer`; `None` = legacy merged (back-compat). Daemon: compute each candidate producer's quality, pick max (tie → producer ASC), feed `calibration_for_scope(Some(best))` to synthesis. **No `aeolus`/`meteorologist` literal** in the selection.
- [ ] **Step 3 — decoupling + integration:** assert no producer literal in the selection path; integration — synthesis warms with the selected producer's params; if no producer warm, stays cold (B3 gate).
- [ ] **Step 4:** DST; full invariant suite incl. A7; fmt/clippy. Commit `feat(synthesis): price the best-calibrated producer (data-driven; the thesis payoff)`.

---

## Task 8: `go_nogo` Brier-beats-baseline gate + `forward_only` filter

**Files:**
- Modify: `crates/fortuna-cognition/src/review.rs` (`go_nogo` ~189-308; `GoNoGoThresholds`)
- Test: `crates/fortuna-cognition/tests/review.rs`

**Interfaces:**
- Consumes: per-producer/per-scope Brier + the market-implied baseline; `provenance->>'source'`.
- Produces: `go_nogo` returns NO-GO when a synthesis scope's Brier does not beat the market baseline; backtest rows (`source="historical-import"`) are excluded from the promotion count.

- [ ] **Step 1 — failing unit test:** a synthesis scope with Brier ≥ baseline → NO-GO with reason `brier_not_beating_baseline` (today it wrongly returns GO); a mechanical scope is unaffected (Brier gate is synthesis-only, spec §11). Separately: resolved beliefs stamped `source="historical-import"` are EXCLUDED from the volume/metric count (`forward_only`), so a GO never rests on backtested data. Run → FAIL.
- [ ] **Step 2 — implement:** add the Brier-beats-baseline branch (synthesis scopes) per spec.md:384; add the `forward_only` filter (`source IS DISTINCT FROM 'historical-import'`) to the go_nogo input query. Thresholds use the SPEC values (paper 30d / 60-beliefs / fee 0.35) per the demo config (GAPS divergence noted).
- [ ] **Step 3 — integration:** `go_nogo` over real scored rows (mix of forward + imported) counts only forward; verdict + reasons correct.
- [ ] **Step 4:** full suite; fmt/clippy; DST. Commit `feat(review): go_nogo Brier-beats-baseline gate + forward-only promotion filter (spec §11; close the silent omission)`.

---

## Self-review checklist (run before execution)
- **Spec coverage:** WS1 items G1 (Tasks 4-5), G2 unified dispatch + persona resolution (Tasks 2-3), G4 per-producer keying (Tasks 1,6,7), go_nogo extension + §9.1 forward_only (Task 8) — all covered.
- **Type consistency:** `open_weather_bracket_due`/`WeatherBracketDue`, `resolved_stats_for_producer`, `scores_for_producer`, `calibration_for_scope(producer)` referenced consistently across tasks.
- **Decoupling:** Tasks 2,3,7 each carry an explicit no-producer-literal assertion.
