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

### W2 — E3 endpoint `/api/rota/v1/chain` (+ the event-scoped ledger reads + validation reconcile)

> **V&V BLOCK folded:** the per-event, per-producer ledger reads the endpoint needs DO NOT EXIST today — `BeliefsRepo` readers are *category*-scoped and return only `ResolvedStat{p,outcome,brier,clv_bps}` (repos.rs:1484); `SignalsRepo`/proposal/gate have no event lookup; fills/settlements are *market*-keyed. So W2 splits into **W2.1 (the ledger reads, TDD in `fortuna-ledger`)** then **W2.2 (the endpoint)**. Producer attribution lives in `beliefs.provenance->>'producer'` (repos.rs:1226).

**Files:**
- Modify: `crates/fortuna-ledger/src/repos.rs` — the new event-scoped reads (W2.1).
- Create: `crates/fortuna-ledger/tests/chain_reads.rs` — one failing test per new read.
- Modify: `crates/fortuna-ops/src/rota.rs` (the `.route("/api/rota/v1/chain", get(view_chain))` line — insert before `.with_state`, ~rota.rs:94; the `view_chain` handler + a private `assemble_chain(pool, event_linkage)` helper).
- Modify: `crates/fortuna-ops/tests/rota.rs` (bump `PATHS` `[&str; 29]`→`[30]` + add `/api/rota/v1/chain`; `every_path_is_get_only_and_200` enforces it).
- Modify: `crates/fortuna-ops/tests/chain_view_contract.rs` (reconcile the `validation` golden to the REAL `ValidationRun` shape — step W2.2-6).
- Modify: `crates/fortuna-ops/Cargo.toml` (add `fortuna-backtest` as a **dev-dependency** ONLY — for the contract test; NOT a runtime dep; cycle-checked clean both directions).
- Create: `crates/fortuna-ops/tests/chain_endpoint.rs` (the seeded-event assembly test).

#### W2.1 — event-scoped ledger reads (new; each red→green)
Add to `fortuna-ledger` (resolve `event_linkage`→event row first, then read by `event_id`; reuse market-keyed reads via event→market where they exist). Grep the exact `provenance`/column names before writing SQL.
- [ ] **`EventsRepo`/discovery `by_linkage(event_linkage) -> Option<EventRow{event_id,category,scope,target_date,market_id,market_ticker}>`** — add if absent (grep for an existing events/discovery read first). Test: seed an event → read it back.
- [ ] **`BeliefsRepo::beliefs_for_event(event_id) -> Vec<EventBelief>`** where `EventBelief{ producer_id (from `provenance->>'producer'`), producer_type, mind_id, mind_version, p_raw (`p`), p_cal (Option), rationale (Option), belief_at, status, outcome, brier, clv_bps }`. Test: seed two producers' beliefs (aeolus + meteorologist) on one event → assert both rows return with producer attribution + the scoring columns.
- [ ] **`SignalsRepo::signals_for_event(event_id) -> Vec<SignalRow>`** (or via the event→signal linkage; grep how signals link to events). Test: seed → read.
- [ ] **proposal + gate for event:** `ProposalsRepo::for_event(event_id) -> Option<ProposalRow>` + the recorded gate-decision (`audit` `kind='gate_decision'` keyed to the proposal/event — RENDER-ONLY). Test: seed a proposal + a gate_decision audit row → read both.
- [ ] **fill/settlement for event:** resolve `event_id`→`market_id`, then `FillsRepo::first_fill_for_market(market_id)` (repos.rs:103) + `SettlementsRepo::chain(market_id)` (repos.rs:370) — reuse existing market-keyed reads; add the event→market resolution only. Test: seed → read.
**Gate (W2.1):** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-ledger --test chain_reads -- --test-threads=1`.

#### W2.2 — the endpoint
**Interfaces — Consumes:** W2.1's reads; `fortuna_ops::chain_view::*` (W1); `ScorecardsRepo::latest_scorecard(scope, producer, window)` → `Option<Scorecard>` (**3 args** — repos.rs:3062; pass `producer=None` merged bucket + the `window` the WS2 scorecard job writes, e.g. `"30"` — confirm against the writer or the field is silently `None`); `ValidationRunsRepo::latest(scope, None)` → `Option<serde_json::Value>` (the verbatim JSONB payload = the serialized `ValidationRun`); `ExecutionMode::as_str()` + `allows_order_mutation()` (the enum is `Deserialize`-only — serialize via the helpers).
**Produces:** `GET /api/rota/v1/chain?event=<event_linkage>` → `200` `ChainView` (or `{"status":"unavailable",...}`).
**Algorithm (`assemble_chain`):** (1) `by_linkage` → `EventRef` (absent → unavailable envelope); (2) `beliefs_for_event` → `producers[]` (`p_cal` Some only when a calibration set exists for that producer/scope; `rationale` append-only display, NEVER executed; `score` only post-resolution); (3) `signals_for_event`, `for_event` proposal, the recorded `gate_decision` row (**never invoke `GatePipeline`** — I1), fill, settlement; (4) `latest_scorecard(scope, None, "30")`; (5) `validation = ValidationRunsRepo::latest(scope, None)` assigned DIRECTLY (no deserialize, no runtime `fortuna-backtest` dep); `None` = honest absence; (6) `safety` via the `ExecutionMode` helpers + the latest-snapshot age for `book_freshness_secs`; (7) any absent capability → HTTP **200** + `{"status":"unavailable","detail":"…"}` (ROTA R1; never 5xx, never fabricated zeros); GET-only (405 on a mutating method).

- [ ] **Step 1 (route table):** extend `PATHS` → `every_path_is_get_only_and_200` FAILS (count/404).
- [ ] **Step 2:** route + stub `view_chain` (unavailable envelope) → route-table PASSES.
- [ ] **Step 3 (assembly):** `chain_endpoint.rs::chain_assembles_seeded_event` — seed an event with two producers' beliefs + proposal+gate_decision + paper fill + settlement + scorecard → assert every stage + BOTH producers (head-to-head). FAILS.
- [ ] **Step 4:** implement `assemble_chain` → PASSES.
- [ ] **Step 5 (validation present):** `chain_carries_validation_when_run_exists` — `ValidationRunsRepo::insert` a real `ValidationRun` payload for the scope → assert `chain.validation` is `Some` and its `verdict` round-trips. FAILS → PASSES.
- [ ] **Step 6 (contract reconcile):** in `chain_view_contract.rs`, replace the ad-hoc `validation` JSON with `serde_json::to_value(&<real fortuna_backtest::sweep::ValidationRun>)` (dev-dep) and assert `ChainView` round-trips it → the 4 contract tests PASS, now pinning the REAL wire shape for the UI.
- [ ] **Step 7:** `cargo fmt`; targeted gate; commit.

**Gate (W2.2):** `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo test -p fortuna-ops --test rota --test chain_view_contract --test chain_endpoint -- --test-threads=1` + `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp cargo clippy -p fortuna-ops --all-targets -- -D warnings`.

**Honesty:** `validation` present ONLY when a real `validation_runs` row exists; until W7 wires the real edge provider, a real-data run is `Insufficient`-by-construction. The endpoint NEVER synthesizes a verdict.

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

**Interfaces — Consumes:** `insert_edge(edge_id, market_id, venue, event_id, mapping_type, confidence, proposed_by, confirmed_by, supersedes, created_at)` (repos.rs:693 — **10 args**; copy the live call pattern at daemon.rs:2477-2489: synthesize `edge_id` (ULID), `venue` (the Kalshi venue), `event_id`=persona event, `mapping_type` (the persona-edge kind), `confidence`, `proposed_by`/`confirmed_by` (the persona/system), `supersedes`=None, `created_at` via the injected `Clock`); `current_edges_for_event(event_id)` (repos.rs:728); the producer-agnostic CLV resolver (daemon.rs:4938, the WS1-slice-5 belief→event→edges→market→earliest-fill→snapshots→`clv_bps` path). **Produces:** a resolved meteorologist belief carries `clv_bps = Some(...)` (today always `None`).

**Algorithm (the genuine join sub-step — milestone open-Q#3):** at persona belief-formation, parse the persona's `…#ge<thr>` token (station/date/threshold) → look up the corresponding **Aeolus** event's existing `market_event_edge` for the SAME station/date/threshold → `market_id`; `insert_edge(..)` the persona_event_id→market_id row (full args above). Then `current_edges_for_event(persona_event_id)` resolves → the existing resolver computes `clv_bps` for the meteorologist. **No `if producer=="aeolus"`** (A7 decoupling-clean).

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
1. Replay the seeded history under EACH sweep config (cal-window/recal/scope/GO-threshold), scoring through the SAME `fortuna-scoring` path (G-PARITY — `harness.rs:121-123`), persisting scorecards keyed by `(scope, config)`.
2. `LedgerEdgeProvider::edges(scope, config_index)`: read that config's scorecards → assemble the per-period OOS **Brier-skill** series (gated headline) + CLV series (corroborating). **Length invariant (V&V):** every `(scope, config)` series MUST have the SAME length `t` (the per-scope matrix is `t × n_configs`; `run_sweep` takes the min length at sweep.rs:302-306 — ragged series silently shrink the matrix). `windows(scope) -> (Vec<LabelWindow> of length EXACTLY t, Duration embargo)`: the per-row label eval windows (same-station-day weather brackets overlap → purge drops train rows whose eval window overlaps a test row; embargo buffers after each test fold).
3. `run_sweep` builds the `t × n_configs` matrix and calls `pbo(&matrix, cscv_s, &windows, embargo)` (replacing the `no_windows`/`Duration::zero()` at sweep.rs:332-336). **`pbo` only purges when `windows.len() == t`** (cscv.rs:107) — so `run_sweep` MUST assert `windows.len() == matrix row count` (fail loudly), or purge silently no-ops. Then `spa_c`, `effective_n`/`mintrl`, `dsr` over `family_n_trials` → the pure `decide` verdict (BLOCK-1 Brier-primary, BLOCK-2 family-N — already shipped + mutation-proven; W7 only connects real inputs).

- [ ] **Step 1 (failing test — honest verdict, NOT a forced GO):** `validate_real_edges.rs` — seed a multi-config replay over a **pre-registered fixture** (document in the test which config is genuinely skilled vs noise, so the verdict is a DERIVATION, not a knob). Assert `fortuna validate` reports **whatever verdict the real Brier math yields** (`Go` OR `NoGo` — the test MUST NOT target a specific verdict) with `n_logits > 0` AND `effective_n >= 30` AND purge applied. The test fails today (placeholder empty series → `Insufficient`). Run → FAILS.
- [ ] **Step 2:** wire the real `LedgerEdgeProvider` + thread windows into `run_sweep` + the `windows.len()==t` assertion → Step-1 test PASSES.
- [ ] **Step 3 (purge bites — DIRECTIONAL):** on a deliberately-leaky fixture (overlapping labels), assert `purged.pbo >= nopurge.pbo` AND strictly `purged.pbo > nopurge.pbo + ε` (purge NEVER understates overfitting — mirror `purged_cscv_bites_on_known_overlap`, deflation.rs:78-97). A two-sided "differs" is NOT acceptable. Run → PASS.
- [ ] **Step 4 (leak guard — at the BELIEF/harness layer, where G-PIT lives):** plant a future-dated **belief** (`available_at >= decided_at`) in the seed; assert `ReplayHarness::replay`'s strict-PIT rejection counter increments and the leaked belief NEVER reaches the scored series (harness.rs:8-9,88 owns the guard — NOT a scorecard-level check). Run → PASS.
- [ ] **Step 5:** `cargo fmt`; targeted gate; commit.

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
- [ ] **E6 rearm-I4 (THREE-WAY FS check — V&V: NOT `!is_revoked`):** before `record_rearm`, read `[killswitch].revocation_file` (boot.rs:324) and apply a three-way guard, because `fortuna_killswitch::is_revoked` is `path.exists()` (killswitch lib.rs:258) which collapses *absent* and *unreadable* both to `false` — so negating it is lossy and would refuse on the normal (absent) happy path. The guard:
  - **present** (`is_revoked(path)==true`) → **REFUSE** (the I4 refusal);
  - **absent AND readable** (parent dir stat OK, file not present) → **ALLOW** the rearm;
  - **unreadable/unverifiable** (parent-dir/`path.try_exists()` → `Err`) → **REFUSE** (FAIL CLOSED).
  *Tests:* `rearm_refuses_when_killswitch_sentinel_present` (mutation-proof — plant the sentinel file → refuse; drop the guard → the test reds); `rearm_refuses_when_sentinel_unreadable` (plant an actually-unreadable sentinel dir, e.g. perms `0o000`, → refuse — this test exercises the `try_exists`/stat probe, NOT `is_revoked`). I4 invariant test ADDITIONS-only.
- [ ] **E4 dead-man (BISECT-FIRST — V&V: premise unconfirmed):** the external `DeadmanPinger` (deadman.rs:66) is correct-by-design — `ping()` surfaces typed errors with NO internal retries (deadman.rs:122-126), the loop is the runner's job (module doc 6-11). So a "transport failure" log is the EXPECTED behavior when `FORTUNA_DEADMAN_URL` is unset/unreachable, NOT a pinger defect — and a "pinger recovers after transport failure" test has no recovery path to exercise (it would be vacuous). **First bisect the real F8 symptom:** grep `crates/fortuna-live/src/main.rs` (the pinger construction + loop wiring) and the `FORTUNA_DEADMAN_URL` resolution + the Slack `SocketDial` / Kalshi `kalshi::dial` backoff sites. THEN either (a) name the concrete defect (e.g. the runner loop drops the pinger on first error, or backoff isn't wired) + a test that reds on it, or (b) **DROP E4 as already-satisfied / config-only** and record that in GAPS.md. Do NOT ship a recovery test against a no-recovery-by-design component.
- [ ] **E5 docs:** demo runbook (`fortuna doctor` → WS3 `backtest`-seed → `fortuna validate` → `start paper-demo` → `/api/rota/v1/chain`) + Aeolus stable-source note + `CHANGELOG.md`. **Honesty:** present `validate` on real data per W7's status (Insufficient-by-construction until W7 ships, real verdict after).
- [ ] **Config-cleanup:** GO-gate example config → spec §11 (`config/fortuna.example.toml:92-94`): `min_paper_days_mechanical` 14→**30**, `max_fee_pnl_ratio` 0.5→**0.35**. For `min_resolved_beliefs_synthesis`: the WS4 spec §11 says **60** but the milestone D-F notes the shipped **100** is "stricter, fine" — **adopt 60** (the WS4 spec is WS4's authority) and record the tolerance in GAPS.md. CLV constants (`CLV_MIN_TOUCH_QTY`/`CLV_MAX_SPREAD_CENTS`, daemon.rs:4835-4836) → `[cognition]` config.
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
- **V&V folded (spec V&V):** rearm CLI-path + config-sentinel + fail-closed; E4 rescope; W5 threshold-match + CLV-market-level honesty; doctor mutation-proof; validation reconciled via dev-dep contract test (no runtime backtest dep); PATHS bump.
- **Plan-V&V folded (2026-06-22, hp-verifier BLOCK+4 Important / hp-guardian PASS+6 Adv):**
  - **[BLOCK] W2 data-access layer** — the event-scoped reads don't exist → W2 split into **W2.1** (new `fortuna-ledger` reads: `by_linkage`, `beliefs_for_event` w/ `provenance->>'producer'`, `signals_for_event`, proposal+gate, event→market fill/settlement) each TDD, then **W2.2** the endpoint.
  - **[Important] W2** `latest_scorecard(scope, producer, window)` is **3-arg** (was mis-cited 1-arg); specify `(None,"30")`.
  - **[Important] W6 rearm-I4** — `is_revoked`=`path.exists()` collapses absent+unreadable → a **three-way FS check**, NOT `!is_revoked`.
  - **[Important] W6 E4** — pinger correct-by-design → **bisect F8 first** (or drop); no vacuous recovery test.
  - **[Important]+[Adv] W7** — `pbo` silently no-purges unless `windows.len()==t` → length invariant + a loud assertion; leak-guard at the **belief/harness** layer (not scorecards); honest-GO test reports **whatever verdict the math yields** over a pre-registered fixture; purge assertion is **directional** (`purged.pbo > nopurge.pbo`).
  - **[Minor]** W5 `insert_edge` 10-arg; config synth 60 (milestone tolerates 100).
- **Type consistency:** `ChainView`/`SafetyPills`/`ProducerBelief`/`BeliefScore` (W1) consumed by W2; `ValidationRunsRepo::latest`→`Option<serde_json::Value>` matches `ChainView.validation`'s type (no deserialize needed); `insert_edge`/`current_edges_for_event` (W5) match repos.rs; `EdgeProvider`/`run_sweep`/`LabelWindow` (W7) match sweep.rs.
