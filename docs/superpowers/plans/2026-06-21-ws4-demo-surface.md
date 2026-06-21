# WS4 Demo Surface — Implementation Plan (W2–W6)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development (or the Hephaestus loop), builder→verifier per slice. Checkbox (`- [ ]`) steps.

**Goal:** Make the closed/provable loop showable — the demo-readiness backend (endpoints + CLI), the UI session rendering against the committed W1 contract.

**Architecture:** FORTUNA owns data + endpoints + serialization. **W1 (the chain-view contract) is DONE** (commit `01eaf64`, `fortuna_ops::chain_view::ChainView`). This plan is W2–W6.

**Authority:** SPEC `docs/superpowers/specs/2026-06-21-ws4-demo-surface-design.md` (V&V-clean, `d660217`). Invariants I1–I7 absolute.

## Global Constraints
- Rust 2021; cents `i64`; no `panic!`/`unwrap`/`expect` in non-test code; all time via injected `Clock`.
- `sqlx` compile-checked; DB tests/clippy under `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp`.
- Read-only views (I5); paper-safe (`execution_mode="paper_ledger"`, the `i_paper_live_no_real_order` wall holds); secrets env-only, never printed.
- `crates/fortuna-invariants/` additions-only. Selective `git add` (NOT `-A` — unrelated kairos work in the tree). Build in this worktree (own target — no contention with the WS3 builder).
- Per-slice gates TARGETED; full battery + invariant tests at the WS4 boundary.

## Slices

### W1 — DONE (commit 01eaf64)
`ChainView` contract + 4 golden-JSON tests. The UI session builds against it.

### W2 — E3 endpoint `/api/rota/v1/chain`
**Files:** `crates/fortuna-ops/src/rota.rs` (route + `view_chain` handler); `crates/fortuna-ops/tests/rota.rs` (PATHS `[&str; 29]`→`[30]`; the `every_path_is_get_only_and_200` test covers it).
**Interfaces — Consumes** `chain_view::ChainView`, the ledger repos (`BeliefsRepo`, `ScorecardsRepo`, fills/settlements/edges, the `audit` `kind='gate_decision'` rows). **Produces** `GET /api/rota/v1/chain?event=<event_linkage>`.
**Algorithm:** assemble `ChainView` for the event — beliefs-by-producer (+ `p_cal` from calibration when present, `Option` otherwise), the proposal, the gate trace (read `audit` `gate_decision` rows — render-only, never invoke `GatePipeline`), fill, settlement, scores; `scorecard` via `ScorecardsRepo::latest_scorecard`; `validation: None` (until WS3); safety pills (`execution_mode` via `ExecutionMode::as_str()`, `order_mutation_enabled` via `allows_order_mutation()`, `book_freshness_secs` from the latest snapshot age). **Degrade to HTTP 200 + `{"status":"unavailable"}` (ROTA R1)**; GET-only (405 on mutation).
**Failing tests:** route-table (GET-only/200/PATHS bump); `chain_assembles_seeded_event` (seed an event with beliefs+fill+settle+score → assert the chain stages + the two-producer head-to-head). **Gate:** `SQLX_OFFLINE=… cargo test -p fortuna-ops --test rota --test chain_view_contract -- --test-threads=1`.

### W3 — E1 `fortuna doctor`
**Files:** `crates/fortuna-cli/src/main.rs` (a `doctor` command in the **DB-async dispatch** block, main.rs:1043) + a `doctor` module; `crates/fortuna-cli/tests/doctor.rs`.
**Algorithm:** print a green/red checklist, exit non-zero on any red — DB reachable; migrations applied (`_sqlx_migrations` complete); env/creds present (presence only, never printed); mode-safe (`execution_mode`/`orders_enabled` paper-safe); GRANTs (the app role can SELECT/INSERT the tables it needs); source reachable (read-only Aeolus/Kalshi ping). Reuse ROTA Health probes where they exist.
**Failing tests:** `doctor_exits_nonzero_on_red` — run the **mutation-proof protocol** (clean→exit 0; plant a missing migration / absent env → exit non-zero; revert→exit 0). **Gate:** `SQLX_OFFLINE=… cargo test -p fortuna-cli --test doctor`.

### W4 — E2 `fortuna start paper-demo`
**Files:** `crates/fortuna-cli/src/main.rs` (`start_cmd` → `paper-demo` mode); `crates/fortuna-live/src/boot.rs` or `daemon.rs` (the **F11 pointer-write**: daemon writes the live `DATABASE_URL` to `data/runtime/current-demo-db-url` on boot); test `crates/fortuna-cli/tests/paper_demo.rs` (or a fortuna-live integration test for the wall).
**Algorithm:** fresh migrated DB; `execution_mode="paper_ledger"` (paper fills, no real order — `allows_order_mutation()=false`); pointer-write on boot.
**Failing tests:** `paper_demo_holds_no_real_order` — the paper-demo mode keeps `i_paper_live_no_real_order` (an executable test + a reds-it mutation: a path that routed a real order panics the `GuardedKalshiTransport`); `pointer_write_lands_live_url`. **Gate:** `SQLX_OFFLINE=… cargo test -p fortuna-cli --test paper_demo` + the invariant test.

### W5 — G1 CLV-for-persona (the head-to-head completer)
**Files:** `crates/fortuna-live/src/daemon.rs` (the persona belief-formation path) + `crates/fortuna-ledger/src/repos.rs` if a lookup helper is needed; test `crates/fortuna-live/tests/persona_clv.rs` (or extend daemon_smoke).
**Algorithm (the genuine join sub-step — milestone open-Q#3):** at persona belief-formation, parse the persona's `…#ge<thr>` token (station/date/threshold) and look up the corresponding **Aeolus** event's existing `market_event_edge` for the same station/date/threshold → `market_id`; `insert_edge(persona_event_id, market_id)` (repos.rs:655). Then `current_edges_for_event(persona event_id)` resolves → the producer-agnostic CLV resolver (daemon.rs:4928) computes `clv_bps` for the meteorologist. No `if producer=="aeolus"` (A7-clean).
**Honesty note (carry into the demo):** CLV is computed from the earliest fill on the shared market → the persona's `clv_bps` will be **identical** to Aeolus's (market-level drift, not an independent confirmation). Brier differentiates.
**Failing tests:** `meteorologist_belief_gets_nonnull_clv` — today `None`; after W5, a resolved meteorologist belief carries `clv_bps = Some(...)` equal to the Aeolus belief's on the same bracket. **Gate:** `SQLX_OFFLINE=… cargo test -p fortuna-live --test persona_clv -- --test-threads=1`.

### W6 — E6 rearm-I4 + E4 dead-man + E5 docs + config-cleanup
**Files:** `crates/fortuna-cli/src/main.rs` (the **CLI ledger-rearm arm**, db_command, main.rs:1074 — NOT `HaltFlags::rearm`); `crates/fortuna-ops/src/deadman.rs` (+ `fortuna-live/src/main.rs` pinger wiring); `config/fortuna.example.toml`; `crates/fortuna-live/src/daemon.rs` + `boot.rs` (CLV constants → config); docs (runbook + CHANGELOG); tests in the respective crates.
- **E6 rearm-I4:** before `record_rearm`, read the sentinel path from config `[killswitch].revocation_file` (boot.rs:317) and **refuse if `fortuna_killswitch::is_revoked(path)`**. **FAIL CLOSED:** an unreadable/unverifiable sentinel dir REFUSES the rearm (guard/invert — `is_revoked` returns `false` on FS error, the wrong direction for a refusal). *Test:* `rearm_refuses_when_killswitch_sentinel_present` + the reds-it mutation; + an unreadable-sentinel-refuses case.
- **E4 dead-man (RESCOPE):** the external `DeadmanPinger` already exists. Fix the failing pinger (**F8** "dead-man ping FAILED: transport failure") + harden source-reconnect (verify Slack `SocketDial` / Kalshi `kalshi::dial` cap-exponential backoff). State the precise delta over `deadman.rs`; do NOT add an internal self-checker. *Test:* the pinger recovers after a transport failure (mock transport).
- **E5:** demo runbook (`fortuna doctor` → WS3 `backtest` seed → `start paper-demo` → `/chain`) + Aeolus stable-source note + CHANGELOG.
- **Config-cleanup:** GO-gate example config → spec §11 values (paper 30, fee 0.35, synth 60; `config/fortuna.example.toml`); CLV constants (`CLV_MIN_TOUCH_QTY`/`CLV_MAX_SPREAD_CENTS`, daemon.rs:4835-4836) → `[cognition]` config.
**Gate:** the per-component targeted tests + clippy/fmt.

## Boundary (after W2–W6)
Full battery for the touched crates (`fortuna-ops`, `fortuna-cli`, `fortuna-live`, `fortuna-ledger`) + the invariant tests (esp. `i_paper_live_no_real_order`, the rearm-I4 refusal) + clippy `--all-targets -D warnings` + fmt + the route-table tests. hp-guardian final overview. PAUSE for operator review (plan-gated; do NOT auto-merge).

## Sequencing
- **W1 is committed now** (UI unblocked).
- **W2–W6 implement after WS3 merges** so `/chain` renders the real backtested record + the `validation` field reconciles to WS3's real `ValidationRun` (drop the `Option<serde_json::Value>` forward-decl). W5/W4/W6 touch the daemon — coordinate with WS3's daemon-cadence work at merge.
- Build in a dedicated worktree (own target) to avoid build-lock contention with the WS3 builder.

## Self-review
- **Coverage:** W2↔E3-endpoint, W3↔E1, W4↔E2, W5↔G1, W6↔E4+E5+E6+config. W1 done. All spec slices covered.
- **V&V folded:** rearm CLI-path + config-sentinel + fail-closed (V I-2/G Adv-2); E4 rescope to fix-pinger (V I-3/G Adv-1); W5 threshold-match sub-step + CLV-market-level honesty (G Adv-3); doctor mutation-proof (V); validation forward-decl reconciled post-WS3 (V I-1); PATHS bump (V M-4).
- **Type consistency:** `ChainView`/`SafetyPills`/`ProducerBelief`/`BeliefScore` (W1) consumed by W2; `insert_edge`/`current_edges_for_event` (W5) match repos.rs.
