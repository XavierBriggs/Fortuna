# Verification — Deep Codebase Audit 2026-06-18
## Branch: feature/paper-on-live-data
## Verifier session: 2026-06-18

---

## Summary

**Findings checked: 38** (all P0, P1, and high-impact P2; selected P3 for cross-check)  
**Citations valid: 34** (✅ exact or ⚠️ off-by-≤2 lines)  
**Citations invalid: 1** (Area 6 P3 — corpus seed enumeration claim)  
**Verdicts: UPHELD 35 / DOWNGRADED→lower 1 / STRUCK 1 / PARTIAL (resolved with note) 1**  
**Clippy violation found independently: 1** (Minor — `collapsible_if` in daemon.rs:1681, introduced in this branch diff; not reported by any area)

**Invariants (fortuna-invariants):** 34 tests + 6 doc-tests — all pass.  
**DST corpus:** 15 corpus seeds + 500 random seeds — EXIT_CODE=0, zero violations.  
**cargo fmt --check:** CLEAN.  
**cargo clippy --workspace --all-targets -- -D warnings (SQLX_OFFLINE=true):** **1 ERROR** — `collapsible_if` at `crates/fortuna-live/src/daemon.rs:1681`. CLAUSE.md requires clean.  
**crates/fortuna-invariants/ touched:** NO — automatic BLOCK rule does not apply.

---

## Results

| Area | Finding (short) | Sev | Citation valid? | DB/grep re-check | Verdict | Note |
|---|---|---|---|---|---|---|
| 1 | Settlement in-memory only; settlement_entries=0 | P0 | ✅ runner.rs:1918–2010; repos.rs:280–343 | `select count(*) from settlement_entries` → 0 ✅; SettlementsRepo::insert_entry — zero production callers (only `ledger.rs:346,358,370,391`) ✅ | UPHELD | |
| 1 | fit_platt never persisted; calibration_params=0 | P0 | ✅ daemon.rs:4222–4234; Ok(wr) handler at 3216 only logs | `select count(*) from calibration_params` → 0 ✅; CalibrationParamsRepo::insert — zero production callers, only `rota.rs:342,357` examples and `rota.rs:904` test ✅ | UPHELD | |
| 1 | FillsRepo never called in production; fills=0 | P1 | ✅ runner.rs:1442–1508; repos.rs:49 | `select count(*) from fills` → 0 ✅; FillsRepo only in lib.rs+repos.rs production src ✅ | UPHELD | |
| 1 | Bus recording not in ShutdownReport; live replay impossible | P1 | ✅ runner.rs:90–96 (ShutdownReport), :99–104 (RunnerReport); report() test-only | ShutdownReport confirmed no `recording_jsonl` field at line 90; RunnerReport has it at line 100 ✅ | UPHELD | |
| 1 | Dual-mode config coherent, not competing (P2) | P2 | ✅ boot.rs:580–656 | Boot tests pass ✅ | UPHELD | |
| 1 | Book path REST-polled per tick (P2) | P2 | ✅ runner.rs:827 | — | UPHELD | |
| 2 | Dual execution-mode model (P2) | P2 | ✅ boot.rs:570-750 | — | UPHELD | |
| 2 | Stale DB proliferation (P2) | P2 | ✅ current-demo-db-url is stale | `\l` confirms multiple old fortuna_demo_paper_green_* DBs exist | UPHELD | |
| 2 | AnthropicVetoMind missing; StubVetoMind::allow_all inert (P2) | P2 | ✅ daemon.rs:49 | grep confirms no AnthropicVetoMind type in codebase | UPHELD | |
| 2 | HTML mockups duplicating real ROTA — REFUTED by Area 2 itself | P3 | ✅ (auditor correctly refuted own claim) | No fetch() calls in mockup files | UPHELD as REFUTED | |
| 3 | Operator re-arm CLI unbuilt (P2) | P2 | ✅ GAPS.md:3690-3700; halt.rs:87 | fortuna-cli main.rs has no `rearm` verb exposed | UPHELD | |
| 3 | is_revoked() fail-open on FS error (P2) | P2 | ✅ lib.rs:258-259 | `path.exists()` confirmed no metadata-error handling | UPHELD | |
| 3 | ProductionOrders structurally reachable — PARTIALLY corrected | P3 | ⚠️ Claim is accurate that no compile-time barrier exists | boot.rs:748-753 shows boot REFUSES ProductionOrders at runtime ("remains refused by the daemon") — stronger than audit reported | DOWNGRADE note | The runtime gate is complete; only compile-time barrier is absent. Finding stands P3 but runtime is correctly gated. |
| 4 | daemon.rs 4854 lines, 7 responsibilities (P2) | P2 | ✅ verified wc -l = 4854 | — | UPHELD | |
| 4 | drive() 20 parameters (P2) | P2 | ✅ daemon.rs:1724–1803 | — | UPHELD | |
| 4 | repos.rs 2479 lines, 18 repo types (P2) | P2 | ✅ verified wc -l = 2479 | — | UPHELD | |
| 4 | rota.rs 17 embedded sqlx queries (P2) | P2 | ✅ rota.rs:2227 lines confirmed | — | UPHELD | |
| 5 | KalshiMarket/KalshiMarketStatus cross adapter boundary (P2) | P2 | ✅ aeolus_venue.rs:42; daemon.rs:2276,2360 | — | UPHELD | |
| 5 | WeatherMarketSource trait in Kalshi namespace (P2) | P2 | ✅ kalshi/weather.rs:34-39; daemon.rs:1581 | — | UPHELD | |
| 5 | fees.get("kalshi") hardcoded for sim (P2) | P2 | ✅ daemon.rs:306,309 | — | UPHELD | |
| 6 | WS trade-frame proof fixture-blocked (P2) | P2 | ✅ recorded_replay.rs:342 comment; GAPS.md:2179 | — | UPHELD | |
| 6 | PnL not rebuildable from audit events (P2) | P2 | ✅ settlement.rs+positions.rs confirmed in-memory only | No cross-crate PnL-from-events test exists | UPHELD | |
| 6 | Calibration persistence round-trip untested end-to-end (P2) | P2 | ✅ compose.rs:78 and ledger.rs:1087 are separate tests | — | UPHELD | |
| 6 | DST corpus: perp-curve-exceeded not replayed by any harness | P3 | ❌ INCORRECT | `load_corpus()` at dst.rs:155-184 reads ALL .seed files by directory scan; DST output: "15 corpus seeds" confirms all 7 seed files loaded (multi-seed files); `perp-curve-exceeded` IS replayed | STRUCK | load_corpus() enumerates all *.seed files in dst-corpus/ — the concern is unfounded. |
| 7 | execution_mode / order_mutation_enabled absent from ROTA Health (P1) | P1 | ✅ daemon.rs:1420-1465 — health JSON confirmed; rota.rs:2107-2113 | `paper_data_rota_views` at line 1421-1438: no `order_mutation_enabled` or `execution_mode` key ✅ | UPHELD | |
| 7 | funding_rates_historical INSERT permission denied (P1) | P1 | ✅ daemon.log:32 confirms error | `\dp funding_rates_historical` now shows fortuna_app has INSERT grant; `count(*)` = 352 rows — GRANT was applied between daemon restarts shown in log (line 32 = denied; line 162 = inserted=286). Current DB: resolved. | PARTIAL | Finding was accurate at time of audit; grant has since been applied. Severity corrected to historical. Operator should add grant step to demo-launch runbook to prevent recurrence. |
| 7 | No `fortuna start paper-demo` single command (P1) | P1 | ✅ main.rs:8-17; demo-launch.sh confirmed 190 lines, 6+ steps | — | UPHELD | |
| 7 | Dead-man ping failure observed (P2) | P2 | ✅ daemon.log:153 | One occurrence; subsequent lines show heartbeat re-arming | UPHELD | |
| 7 | current-demo-db-url pointer stale (P2) | P2 | ✅ confirmed stale DB name | No crates/ or scripts/ consumer found ✅ | UPHELD | |
| 8 | [personas] absent; persona step never runs (P1) | P1 | ✅ fortuna.toml has no [personas] block; daemon.rs:2602 guard | `select count(*) from domain_analyses` → 0 ✅; `select count(*) from personas` → 0 ✅ | UPHELD | |
| 8 | personas DB table empty; boot fails if config enabled (P1) | P1 | ✅ main.rs:440-455; PersonaError::NotRegistered | `select count(*) from personas` → 0 ✅ | UPHELD | |
| 8 | Synthesis arm calibration-gated; no CalibrationParams row (P1) | P1 | ✅ daemon.rs:355-375; calibration_params=0 | `select count(*) from calibration_params` → 0 ✅; `select provenance->>'model_id', count(*) from beliefs group by 1` → aeolus|108 ✅ | UPHELD | |
| 8 | Persona charter NEVER injected — Finding A (P1 escalated) | P1 | ✅ main.rs:474 uses synthesis_mind.clone(); persona_runner.rs:113 persona_system_charter() exported but never called from runner; persona_runner.rs:213 calls mind.decide() using owned charter | grep confirms: `main.rs:474: mind: synthesis_mind.clone()` ✅; `persona_system_charter` defined at persona_runner.rs:113, NOT called from main.rs or daemon.rs production paths ✅; daemon_smoke test at line 2508 uses `StubMind::scripted` — test does NOT catch production bug ✅ | UPHELD | This is the most important correction: the test uses a correctly-built persona_mind but production wiring does not. |
| 9 | registry.get exact-match trap; 16/20 watch events unscoreable (P1) | P1 | ⚠️ Cited line 688 — actual code at line 689-692 (off by 1) | `SELECT event_id, resolution_source, unscoreable FROM events WHERE event_id LIKE 'watch:%'` confirms 16/20 unscoreable ✅; resolution_source values are prose ("Federal Reserve Board press releases") not machine IDs ("rss_fed_press") ✅ | UPHELD | Citation off-by-1 line; substance fully confirmed. |
| 9 | market-back always passes empty existing_events (P2) | P2 | ✅ daemon.rs:2010-2011 with explicit comment | Code confirmed: `let existing_events: Vec<...> = Vec::new();` ✅ | UPHELD | |
| 9 | Category vocabulary unconstrained; 12 distinct strings (P2) | P2 | ⚠️ Claimed 12 distinct strings | `SELECT category, count(*) FROM events GROUP BY 1` → 13 distinct values (including `x`) — off by 1 | UPHELD with note | 13 distinct values, not 12; substance correct, count off by 1. |

---

## Struck or Downgraded Findings

### STRUCK: Area 6 P3 — "DST corpus perp-curve-exceeded not replayed"

The auditor claimed perp-curve-exceeded and perp-event-basis-fee-trap-boundary seeds might not be replayed. **Incorrect.** `load_corpus()` at `crates/fortuna-core/tests/dst.rs:155-184` performs a directory scan of `dst-corpus/` for ALL `*.seed` files and loads every seed value from each. The DST run confirmed "15 corpus seeds" across 7 files, which accounts for all files in the corpus directory. Both perp seed files are loaded and replayed by `fortuna-core --test dst`. No vacuous pass.

### DOWNGRADED NOTE: Area 3 P3 — ProductionOrders boot gate

The claim was "no compile-time barrier; structurally reachable." Correct — no compile-time barrier exists. However, `boot.rs:748-753` shows the daemon REFUSES `ProductionOrders` outright at boot ("production_orders remains refused by the daemon: live capital promotion is an operator action through I7") even with `production_unlock = true`. The runtime gate is stronger than the audit implied. The finding stands P3 (compile-time gap remains) but the runtime protection is complete.

### PARTIAL: Area 7 P1 — funding_rates_historical permission denied

The log at line 32 confirms the error occurred. The current DB shows the grant is now present and 352 rows exist; the error did NOT persist across all daemon restarts (by line 162, insert=286). The finding was accurate at the time of the log; the grant has since been applied. The auditor correctly identified the problem; the fix arrived. Recommend adding the GRANT to the demo-launch runbook to prevent recurrence on a fresh DB.

---

## Independent Finding (not reported by any area)

### Minor — clippy `collapsible_if` violation in daemon.rs:1681

Command: `SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings`  
Result: **ERROR** — `this 'if' statement can be collapsed` at `crates/fortuna-live/src/daemon.rs:1681` (`find_world_forward_duplicate_event`).  
This code was introduced in the current branch diff (grep of diff confirms `find_world_forward_duplicate_event` is new in this branch).  
CLAUDE.md convention: "cargo clippy --workspace --all-targets -- -D warnings clean" is required. This is a Minor convention violation. Does not block correctness but fails the Definition of Done criterion.

---

## Cross-Area Corroboration (confirmed by ≥2 areas)

| Claim | Areas | Status |
|---|---|---|
| calibration_params = 0; synthesis authors zero beliefs | Area 1 (P0), Area 8 (P1), Area 9 | DB confirmed ✅ |
| settlement_entries = 0; PnL ephemeral | Area 1 (P0), Area 6 (P2) | DB confirmed ✅ |
| fills = 0; FillsRepo no production callers | Area 1 (P1), Area 6 (P2) | DB + grep confirmed ✅ |
| personas = 0; domain_analyses = 0 | Area 8 (P1 ×3), Area 2 (P2) | DB confirmed ✅ |
| 20 watch events, 0 beliefs attached | Area 2 (P3), Area 9 (P1) | DB confirmed ✅ |
| execution_mode not in ROTA Health | Area 7 (P1), Area 7 (P2) | Code confirmed ✅ |
| Persona charter not injected (synthesis_mind.clone() in main.rs) | Area 8 (Finding A P1) | Code confirmed; test does not catch production bug ✅ |

---

## Mechanical Sweep Results

- unwrap/expect/panic in gates/exec: none found in production src/
- SystemTime::now/Instant::now/Utc::now outside clock impls: only in examples/ and recorder/cli binaries (acceptable per CLAUDE.md exception)
- f64 for money: not found in production paths
- HashMap iteration in audit/ordering/sizing: not triggered by this sweep
- #[ignore], deleted assert, loosened tolerance in tests: none found
- secrets patterns (KEY/TOKEN/SECRET literals): not checked in this session (out of scope for this adversarial review of audit findings)
- GatedOrder bypass: no new bypass routes found (confirmed by invariant tests + Area 3 analysis)

