# WS4 Demo Surface — Implementation Plan (W2–W7)

> **For agentic workers:** REQUIRED SUB-SKILL: the Hephaestus loop (hp-implementer → hp-verifier per slice; hp-guardian at phase/final) — or superpowers:subagent-driven-development. TDD. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make the closed/provable loop showable — the demo-readiness backend (endpoints + CLI + the serialized chain-view contract). The separate UI session renders against the committed W1 contract.

**Architecture:** FORTUNA owns data + endpoints + serialization. **WS3 is now MERGED into this branch** (`merge b12d498`) — so W2–W7 build against the REAL `ValidationRun` (`fortuna-backtest/src/sweep.rs:195`), `ValidationRunsRepo` (`fortuna-ledger/src/repos.rs:3103`), and the `validation_runs` table. **W1 (the chain-view contract) is DONE** (commit `01eaf64`, `fortuna_ops::chain_view::ChainView`). This plan is W2–W7.

**Authority:** SPEC `docs/superpowers/specs/2026-06-21-ws4-demo-surface-design.md` (V&V-clean). Invariants I1–I7 absolute; on spec/CLAUDE.md conflict, the spec/constitution wins.

## Global Constraints
- Rust 2021; money is integer `Cents` (`i64`); no `panic!`/`unwrap`/`expect` in non-test code (gates/exec/state/venues/money paths); `thiserror` per crate; `anyhow` only in binaries.
- All time via the injected `Clock` (`SystemTime::now()` outside `Clock` impls is a defect). `sqlx` compile-checked; DB tests + clippy under `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp`.
- Read-only views (I5 — no WS4 surface mutates a row); the model authors nothing new (I6); `crates/fortuna-invariants/` is **additions-only** (never weaken/delete/rename an existing invariant test's assertion).
- Paper-safe: the demo runs `execution_mode="paper_ledger"`; the `i_paper_live_no_real_order` wall holds (no real-venue order, ever).
- Secrets env-only, never printed (presence/length checks only), never in logs/audit payloads.
- Selective `git add` (NOT `git add -A` — unrelated kairos research + docs/reviews are in the tree). Build in the dedicated WS4 worktree (own `target/` — no build-lock contention).
- Per-slice gates are TARGETED (the touched crate's relevant tests + scoped clippy); the FULL battery + invariant tests + the live smoke run at the WS4 boundary.

## Slices

### W1 — DONE (commit 01eaf64)
`ChainView` contract (`fortuna-ops/src/chain_view.rs`) + 4 golden-JSON tests (`fortuna-ops/tests/chain_view_contract.rs`). The UI session builds against it. W2 reconciles its `validation` field to the now-merged WS3 type (below).

---

### W2 — E3 endpoint `/api/rota/v1/chain` (+ validation-field reconcile)

**Files:**
- Modify: `crates/fortuna-ops/src/rota.rs` (add the `.route("/api/rota/v1/chain", get(view_chain))` line in `rota_router`, ~rota.rs:92; add the `view_chain` handler + a private `assemble_chain(pool, event_linkage)` helper).
- Modify: `crates/fortuna-ops/tests/rota.rs` (bump the `PATHS` array count by 1 for `/api/rota/v1/chain`; the `every_path_is_get_only_and_200` test enforces it).
- Modify: `crates/fortuna-ops/tests/chain_view_contract.rs` (reconcile the `validation` golden to the REAL `ValidationRun` shape — see step 6).
- Modify: `crates/fortuna-ops/Cargo.toml` (add `fortuna-backtest` as a **dev-dependency** only — for the contract test to construct a real `ValidationRun`; NOT a runtime dep).
- Create: `crates/fortuna-ops/tests/chain_endpoint.rs` (the seeded-event assembly test).

**Interfaces — Consumes:**
- `fortuna_ops::chain_view::{ChainView, EventRef, SafetyPills, SignalRef, ProducerBelief, BeliefScore, ProposalRef, GateResult, GateCheck, FillRef, SettlementRef}` (W1).
- Ledger reads: `BeliefsRepo` (beliefs-by-producer for the event), `ScorecardsRepo::latest_scorecard(scope)` (the WS2 `Scorecard`), `ValidationRunsRepo::latest(scope, producer)` → `Option<serde_json::Value>` (the persisted `ValidationRun` JSONB payload, returned verbatim), the fills/settlements/edges repos, and the `audit` rows where `kind='gate_decision'`.
- `ExecutionMode::as_str()` + `allows_order_mutation()` for the safety pills (the enum is `Deserialize`-only — serialize via the helpers, never `serde::Serialize` on the enum).

**Produces:** `GET /api/rota/v1/chain?event=<event_linkage>` → `200 application/json` with a `ChainView` (or `{"status":"unavailable", ...}` on a degraded/absent pool).

**Algorithm (`assemble_chain`):**
1. Resolve the event row by `event_linkage` → `EventRef { event_linkage, category, scope, target_date, market_ticker }`. Absent → return the unavailable envelope.
2. `producers[]`: read every belief for the event grouped by producer → `ProducerBelief { producer_id, producer_type, mind_id?, mind_version?, p_raw, p_cal? (Some only when a calibration set exists for that producer/scope, else None), rationale? (append-only display text — NEVER executed), belief_at, score? }`. `score` is populated only post-resolution `{ status, outcome?, brier?, clv_bps? }`.
3. `signals[]`, `proposal?`, `gate?` (read the `audit` `gate_decision` row for the proposal — render the recorded checks; **never invoke `GatePipeline`** — I1), `fill?` (`orders == 0`, paper), `settlement?`.
4. `scorecard`: `ScorecardsRepo::latest_scorecard(event.scope)` → `Option<Scorecard>`.
5. `validation`: `ValidationRunsRepo::latest(event.scope, None)` → `Option<serde_json::Value>` assigned DIRECTLY to `chain.validation` (the JSONB payload IS the serialized `ValidationRun`; no deserialize, no `fortuna-backtest` runtime dep). `None` when no run exists for the scope (honest absence, not a fabricated verdict).
6. `safety`: `SafetyPills { execution_mode: mode.as_str().into(), order_mutation_enabled: mode.allows_order_mutation(), book_freshness_secs: <age of the latest snapshot for the market, else None> }`.
7. Degrade: any absent capability (no Pg pool, event not found) → HTTP **200** + `{"status":"unavailable","detail":"…"}` (ROTA R1; never a 5xx, never fabricated zeros). GET-only — a mutating method on the route → 405 (axum default for an unrouted method on a `get()`-only path).

- [ ] **Step 1 (failing test — route table):** extend `tests/rota.rs` `PATHS` with `/api/rota/v1/chain`; run `every_path_is_get_only_and_200` → FAILS (count mismatch / 404).
- [ ] **Step 2:** add the route + a stub `view_chain` returning the unavailable envelope → route-table test PASSES.
- [ ] **Step 3 (failing test — assembly):** `chain_endpoint.rs::chain_assembles_seeded_event` — seed (via `#[sqlx::test]`) an event with two producers' beliefs (aeolus + meteorologist), a proposal+gate_decision, a paper fill, a settlement, and a scorecard → assert the assembled `ChainView` carries every stage and BOTH producers in `producers[]` (the head-to-head). Run → FAILS.
- [ ] **Step 4:** implement `assemble_chain` per the algorithm → assembly test PASSES.
- [ ] **Step 5 (failing test — validation present):** `chain_endpoint.rs::chain_carries_validation_when_run_exists` — seed a `validation_runs` row for the event's scope (via `ValidationRunsRepo::insert` with a real `ValidationRun` payload) → assert `chain.validation` is `Some` and its `verdict` field round-trips. Run → FAILS, then PASSES after step 4 wires `ValidationRunsRepo::latest`.
- [ ] **Step 6 (validation-field reconcile — contract test):** in `chain_view_contract.rs`, replace the ad-hoc `validation` JSON (`{"pbo":…}`) with `serde_json::to_value(&<a real fortuna_backtest::sweep::ValidationRun>)` (dev-dep), and assert the `ChainView` round-trips it losslessly — so the golden contract test tracks the REAL wire shape the UI renders. Run the existing 4 contract tests → all PASS.
- [ ] **Step 7:** `cargo fmt`; targeted gate (below); commit.

**Gate (targeted):** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-ops --test rota --test chain_view_contract --test chain_endpoint -- --test-threads=1` + `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo clippy -p fortuna-ops --all-targets -- -D warnings`.

**Honesty:** `validation` is present ONLY when a real `validation_runs` row exists for the scope; until W7 wires the real edge provider, a real-data run is `Insufficient`-by-construction (W7 §). Do not synthesize a verdict in the endpoint.

---

### W3 — E1 `fortuna doctor`

**Files:**
- Modify: `crates/fortuna-cli/src/main.rs` (a `doctor` arm in the **DB-async dispatch** block — the block that resolves `DATABASE_URL` near main.rs:1071; add `"doctor"` to the usage string main.rs:160 and the dispatch match).
- Create: `crates/fortuna-cli/src/doctor.rs` (the checklist module) + register `mod doctor;`.
- Create: `crates/fortuna-cli/tests/doctor.rs`.

**Interfaces — Consumes:** the Pg pool; `_sqlx_migrations`; the env-var presence helpers; `ExecutionMode` / `orders_enabled` from config; reuse `fortuna_ops::deadman`/ROTA Health probes where they exist (source reachability). **Produces:** `fortuna doctor` → stdout green/red checklist; exit `0` all-green, non-zero on any red.

**Algorithm:** print a checklist, each line `[ok]`/`[FAIL]`, accumulate a fail flag, exit non-zero if set:
- DB reachable (a `SELECT 1`).
- Migrations applied (`_sqlx_migrations` has no dirty/missing rows vs the embedded set).
- Env/creds present (presence ONLY — `is_some()` + length; **never print the value**).
- Mode-safe (`execution_mode`/`orders_enabled` are paper-safe — `paper_ledger`, `allows_order_mutation()==false`).
- GRANTs (the app role can SELECT/INSERT the tables doctor names — a probe query, caught error → red).
- Source reachable (read-only Aeolus/Kalshi ping; a transport error → red, but a `--offline` flag skips the network checks for CI).

- [ ] **Step 1 (failing test):** `doctor.rs::doctor_exits_nonzero_on_red` via the **mutation-proof protocol**: a clean migrated DB → exit `0`; then plant a defect (drop a migration row / unset a required env) → exit non-zero; revert → exit `0`. Run → FAILS (no `doctor`).
- [ ] **Step 2:** implement the checklist → test PASSES.
- [ ] **Step 3:** `cargo fmt`; targeted gate; commit.

**Gate (targeted):** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-cli --test doctor` + scoped clippy.

---

### W4 — E2 `fortuna start paper-demo`

**Files:**
- Modify: `crates/fortuna-cli/src/main.rs` (`start` dispatch → accept a `paper-demo` mode).
- Modify: `crates/fortuna-live/src/boot.rs` (or `daemon.rs`) — the **F11 pointer-write**: on boot the daemon writes the live `DATABASE_URL` to `data/runtime/current-demo-db-url` (atomic write — temp + rename).
- Create: `crates/fortuna-cli/tests/paper_demo.rs` + a `fortuna-live` integration test for the wall.

**Interfaces — Consumes:** the boot path; `ExecutionMode::PaperLedger`; `GuardedKalshiTransport` (the wall). **Produces:** `fortuna start paper-demo` → a daemon booted with `execution_mode="paper_ledger"`, fresh-migrated DB, and the demo-db pointer written.

**Algorithm:** `paper-demo` → ensure a fresh migrated DB; set `execution_mode="paper_ledger"` (paper fills, `allows_order_mutation()==false`); on boot, atomic pointer-write of the live `DATABASE_URL` → `data/runtime/current-demo-db-url`.

- [ ] **Step 1 (failing test — the wall, mutation-proof):** `paper_demo_holds_no_real_order` — booting `paper-demo` keeps `i_paper_live_no_real_order`; the reds-it mutation: a path that routes a REAL order through `GuardedKalshiTransport` panics/errs (proving the wall is load-bearing, not vacuous). Run → FAILS.
- [ ] **Step 2 (failing test — pointer):** `pointer_write_lands_live_url` — after boot, `data/runtime/current-demo-db-url` contains the live URL. Run → FAILS.
- [ ] **Step 3:** implement the mode + the pointer-write → both PASS.
- [ ] **Step 4:** `cargo fmt`; targeted gate; commit.

**Gate (targeted):** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-cli --test paper_demo` + the relevant `fortuna-invariants` test (`i_paper_live_no_real_order`).

---

### W5 — G1 CLV-for-persona (the head-to-head completer)

**Files:**
- Modify: `crates/fortuna-live/src/daemon.rs` (the persona belief-formation path — insert the persona→market edge so `current_edges_for_event` resolves; the CLV resolver at daemon.rs:4928 is already producer-agnostic).
- Modify (if a lookup helper is needed): `crates/fortuna-ledger/src/repos.rs` (`insert_edge` at repos.rs:693, `current_edges_for_event` at repos.rs:728 already exist).
- Create: `crates/fortuna-live/tests/persona_clv.rs` (or extend `daemon_smoke.rs`).

**Interfaces — Consumes:** `insert_edge(event_id, market_id)` (repos.rs:693); `current_edges_for_event(event_id)` (repos.rs:728); the producer-agnostic CLV resolver (daemon.rs:4928, the WS1-slice-5 belief→event→edges→market→earliest-fill→snapshots→`clv_bps` path). **Produces:** a resolved meteorologist belief carries `clv_bps = Some(...)` (today always `None`).

**Algorithm (the genuine join sub-step — milestone open-Q#3):** at persona belief-formation, parse the persona's `…#ge<thr>` token (station/date/threshold) → look up the corresponding **Aeolus** event's existing `market_event_edge` for the SAME station/date/threshold → `market_id`; `insert_edge(persona_event_id, market_id)`. Then `current_edges_for_event(persona_event_id)` resolves → the existing resolver computes `clv_bps` for the meteorologist. **No `if producer=="aeolus"`** (A7 decoupling-clean).

**Honesty (carry into the demo + the contract):** CLV is computed from the EARLIEST fill on the shared edge-market → the persona's `clv_bps` will be **identical** to Aeolus's (market-level drift, NOT an independent per-producer confirmation). W5 makes it non-null; **Brier** is the per-producer differentiator. Do NOT claim "two independent CLV confirmations."

- [ ] **Step 1 (failing test):** `persona_clv.rs::meteorologist_belief_gets_nonnull_clv` — a resolved meteorologist belief on a bracket Aeolus also believes carries `clv_bps == Some(x)` equal to the Aeolus belief's on the same bracket. Run → FAILS (today `None`).
- [ ] **Step 2:** implement the threshold-match → `insert_edge` → test PASSES.
- [ ] **Step 3:** `cargo fmt`; targeted gate; commit.

**Gate (targeted):** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-live --test persona_clv -- --test-threads=1`.

---

### W7 — WS3 carry-over: real `validate` edge-provider + purged/embargoed CSCV plumbing (MONEY-MATH — gets the rigor)

> Built before W6 (W6 is docs/config/hardening; W7 is the honest-evidence payoff and shares the backtest test infra). hp-guardian audits this slice for leakage / backtest↔live parity / honest-GO.

**The problem (WS3 GAPS.md "G1"; hp-guardian + live-smoke Important):** `fortuna validate` is wired end-to-end (sweep → `validation_runs` → GO surface) but fed a **placeholder** `EdgeProvider` (`crates/fortuna-cli/src/backtest_cmd.rs:214` — "returns empty series") and `run_sweep` hard-codes **no purge windows** (`crates/fortuna-backtest/src/sweep.rs:329-336` — `no_windows: Vec<LabelWindow> = Vec::new(); embargo = Duration::zero()`). So on real replayed history `validate` can ONLY emit `GoDecision::Insufficient` (fail-safe — `decide` guards `effective_n<30 || n_logits==0 → Insufficient`; it can never emit a false GO), and the implemented + unit-proven purged+embargoed CSCV (`purged_cscv_bites_on_known_overlap`) has **no reachable production path**. The replay (`ReplayHarness::replay`, `harness.rs:123`) and the sweep are disconnected — no seam feeds the replayed scorecards back into the sweep matrix.

**Files:**
- Modify: `crates/fortuna-backtest/src/sweep.rs` — `run_sweep` (sweep.rs:256): thread real `LabelWindow`s + embargo into the `pbo` calls (sweep.rs:335-336) instead of `no_windows`/`Duration::zero()`. The windows come from the `EdgeProvider` (extend the trait with `fn windows(&self, scope) -> (Vec<LabelWindow>, Duration)` or return them alongside `ConfigEdges`).
- Modify: `crates/fortuna-cli/src/backtest_cmd.rs` (replace the placeholder provider with a real `LedgerEdgeProvider`) — or a new `crates/fortuna-backtest/src/edge_provider.rs` if it needs the harness/ledger types (keep `fortuna-backtest` core decoupling: no source-name literals; this provider reads generic scorecards, not Aeolus-specific rows).
- Modify (if needed): `crates/fortuna-backtest/src/harness.rs` / a ledger read — expose the replayed scorecards keyed by `(scope, config)` so the provider can assemble per-config OOS Brier-skill series + the per-label eval windows.
- Create: `crates/fortuna-backtest/tests/validate_real_edges.rs` (or extend `crates/fortuna-cli/tests/backtest_cli.rs`).

**Interfaces — Consumes:** `ReplayHarness::replay` output (the per-config scorecards, persisted `source='historical-import'`); `fortuna_scoring::{pbo, spa_c, ...}`; `LabelWindow`, `Duration`. **Produces:** a real `EdgeProvider` whose `edges(scope, config_index)` returns the per-config OOS **Brier-skill** edge series and whose windows feed `pbo`, so `fortuna validate` computes a REAL Brier-primary GO/NO-GO over the replayed track record.

**Algorithm:**
1. Replay the seeded history under EACH sweep config (cal-window/recal/scope/GO-threshold), scoring through the SAME `fortuna-scoring` path (G-PARITY), persisting scorecards keyed by config.
2. `LedgerEdgeProvider::edges(scope, config_index)`: read that config's scorecards → assemble the per-period OOS **Brier-skill** series (the gated headline) + the CLV series (corroborating). `windows(scope)`: the per-label eval windows (e.g. same-station-day weather brackets overlap → the purge must drop train labels whose eval window overlaps a test label; embargo buffers after each test fold).
3. `run_sweep` builds the `T × n_configs` Brier-skill matrix, runs **purged+embargoed** `pbo` (real windows now), `spa_c` on the Brier-loss differential, `effective_n`/`mintrl`, `dsr` over `family_n_trials` → the pure `decide` verdict (BLOCK-1 Brier-primary, BLOCK-2 family-N — already shipped + mutation-proven; W7 only connects real inputs).

- [ ] **Step 1 (failing test):** `validate_real_edges.rs` — seed a multi-config replay with KNOWN overlapping labels; assert `fortuna validate` yields a NON-`Insufficient` verdict (`Go`/`NoGo`) with `n_logits > 0` AND that purge actually bit (the PBO over the real overlapping windows differs from the no-window path — reuse the `purged_cscv_bites_on_known_overlap` style assertion). Run → FAILS (placeholder provider / no windows).
- [ ] **Step 2:** wire the real `LedgerEdgeProvider` + thread the windows into `run_sweep` → test PASSES.
- [ ] **Step 3 (leak guard):** assert no look-ahead — the provider only reads scorecards whose `available_at < decided_at` (strict, G-PIT discipline); a planted future-dated scorecard must NOT enter the series. Run → PASS.
- [ ] **Step 4:** `cargo fmt`; targeted gate; commit.

**Gate (targeted):** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-backtest -p fortuna-cli --test validate_real_edges --test backtest_cli -- --test-threads=1` + scoped clippy + the decoupling grep (no source-name literals leaked into `fortuna-backtest/src/` outside `src/sources/`).

**Honesty:** until W7 ships, the E5 runbook (W6) MUST present `validate` on real data as `Insufficient`-by-construction, NOT a tested-on-real-data verdict.

---

### W6 — E6 rearm-I4 + E4 dead-man + E5 docs + config-cleanup + WS3 decoupling regression test

**Files:**
- Modify: `crates/fortuna-cli/src/main.rs` (the CLI **ledger-rearm** arm — `"halt" | "rearm"` at main.rs:1127, calling `HaltsRepo::record_rearm` — NOT `HaltFlags::rearm`).
- Modify: `crates/fortuna-live/src/boot.rs` (read `[killswitch].revocation_file`, boot.rs:317) — the sentinel path for the I4 refusal.
- Modify: `crates/fortuna-ops/src/deadman.rs` (+ `fortuna-live/src/main.rs` pinger wiring) — fix the failing pinger (F8) + verify source-reconnect backoff.
- Modify: `config/fortuna.example.toml` (GO-gate values → spec §11; CLV constants → `[cognition]`).
- Modify: `crates/fortuna-live/src/daemon.rs` (CLV constants `CLV_MIN_TOUCH_QTY`/`CLV_MAX_SPREAD_CENTS`, daemon.rs:4835-4836 → read from `[cognition]` config).
- Create: a `#[test]` (in `crates/fortuna-backtest/tests/` or `fortuna-invariants` ADDITIONS-only) for the WS3 decoupling/purity carry-over.
- Docs: a demo runbook + `CHANGELOG.md`.

**Sub-steps (each its own failing-test → impl → commit):**
- [ ] **E6 rearm-I4:** before `record_rearm`, read `[killswitch].revocation_file` and **REFUSE the rearm if `fortuna_killswitch::is_revoked(path)`**. **FAIL CLOSED** — an unreadable/unverifiable sentinel dir REFUSES (note: `is_revoked` returns `false` on FS error — correct for the gate poller but the WRONG direction for a refusal; the rearm path must guard/invert it). *Tests:* `rearm_refuses_when_killswitch_sentinel_present` (mutation-proof) + `rearm_refuses_when_sentinel_unreadable` (fail-closed). I4 invariant test ADDITIONS-only.
- [ ] **E4 dead-man (RESCOPE — the external `DeadmanPinger` already exists):** fix the failing pinger (F8 "dead-man ping FAILED: transport failure") + verify Slack `SocketDial` / Kalshi `kalshi::dial` cap-exponential backoff is wired. State the precise delta over `deadman.rs`; do NOT add an internal self-checker (a step backward). *Test:* the pinger recovers after a transport failure (mock transport).
- [ ] **E5 docs:** demo runbook (`fortuna doctor` → WS3 `backtest`-seed → `fortuna validate` → `start paper-demo` → `/api/rota/v1/chain`) + Aeolus stable-source note + `CHANGELOG.md`. **Honesty:** present `validate` on real data per W7's status (Insufficient-by-construction until W7 ships, real verdict after).
- [ ] **Config-cleanup:** GO-gate example config → spec §11 (paper 30, fee 0.35, synth 60; `config/fortuna.example.toml`); CLV constants → `[cognition]` config.
- [ ] **WS3 decoupling regression `#[test]` (WS3 GAPS.md "G2", Minor):** an executable test that greps `crates/fortuna-backtest/src/` for source-name literals (`"aeolus"|"meteorologist"|"kalshi"|"historical-import"`, excluding `src/sources/`) AND asserts `fortuna-scoring`'s `Cargo.toml` has no `rand`/`getrandom`/`libm` and no `sqlx`/`tokio`/`PgPool` in `src/` — so the decoupling + scoring-purity invariants (today enforced ONLY by the WS3 boundary gate, not the test corpus) regress-detect permanently.

**Gate (targeted):** the per-component tests + `cargo clippy` (scoped) + `cargo fmt --check` for each touched crate.

---

## Boundary (after W2–W7)
Run from a clean state in the worktree:
1. Full battery for the touched crates: `fortuna-ops`, `fortuna-cli`, `fortuna-live`, `fortuna-ledger`, `fortuna-backtest`, `fortuna-scoring` (DB tests `--test-threads=1`).
2. Invariant tests: `fortuna-invariants` (esp. `i_paper_live_no_real_order`, the new rearm-I4 refusal) — additions-only.
3. `cargo clippy --workspace --all-targets -- -D warnings` (under `SQLX_OFFLINE`); `cargo fmt --check`.
4. The route-table test + the contract tests.
5. **Live smoke** (`.hephaestus/ws4.live.gates`, below) — like WS2/WS3.
6. hp-guardian final overview (north-star/drift/leak-gap + quant/leakage/parity on W7).
7. PAUSE for operator review (plan-gated; do NOT auto-merge).

## Live gate (`.hephaestus/ws4.live.gates`) — the demo e2e, run at the boundary
The demo's real behavior, against real data (read-only; paper-safe). Detailed at gate-setup; intent:
- `fortuna doctor` exits `0` against the real environment (real DB reachable, migrations applied, mode paper-safe, source pings ok).
- `fortuna backtest` over the real Aeolus archive (reuse the WS3 live-smoke script) → `fortuna validate` yields a REAL **non-`Insufficient`** verdict (W7's live payoff) with purge applied.
- The chain endpoint assembles a real seeded event with the two-producer head-to-head (Brier differentiates; CLV market-level).
- **Fail-closed honesty:** if live infra/creds are genuinely unreachable, report **not-yet-verified** — never fake green (Hephaestus §3).

## Sequencing
- **WS3 is merged** → W2–W7 are all unblocked; the chain renders the real backtested record.
- **W2 first** (the contract+endpoint the UI session consumes; reconciles `validation`).
- **W3, W4** (CLI/boot — `fortuna-cli` main.rs dispatch + `fortuna-live` boot): sequence to avoid main.rs churn.
- **W5** (daemon CLV) then **W7** (backtest money-math, independent crate) then **W6** (daemon CLV-constants config + docs/hardening) — W5 and W6 both touch the daemon; do W5's CLV edge first, then W6's CLV-constants-to-config, to avoid conflicting daemon edits.
- Build in the dedicated worktree (own `target/`).

## Self-review
- **Coverage:** W2↔E3-endpoint + validation-reconcile; W3↔E1-doctor; W4↔E2-paper-demo + the wall; W5↔G1-CLV-persona; W6↔E4-deadman + E5-docs + E6-rearm-I4 + config-cleanup + WS3-decoupling-test; W7↔real-validate-edge-provider + purge. W1 done. All spec slices (W1–W7) covered.
- **V&V folded (carried from the spec's V&V):** rearm CLI-path + config-sentinel + fail-closed (V I-2/G Adv-2); E4 rescope to fix-pinger (V I-3/G Adv-1); W5 threshold-match sub-step + CLV-market-level honesty (G Adv-3); doctor mutation-proof (V); the validation forward-decl reconciled to the REAL `ValidationRun` via a dev-dep contract test, NO runtime `fortuna-backtest` dep on the endpoint crate (V I-1, post-WS3-merge); PATHS bump (V M-4); W7 honest-GO + leak-guard (new, money-math).
- **Type consistency:** `ChainView`/`SafetyPills`/`ProducerBelief`/`BeliefScore` (W1) consumed by W2; `ValidationRunsRepo::latest`→`Option<serde_json::Value>` matches `ChainView.validation`'s type (no deserialize needed); `insert_edge`/`current_edges_for_event` (W5) match repos.rs; `EdgeProvider`/`run_sweep`/`LabelWindow` (W7) match sweep.rs.
