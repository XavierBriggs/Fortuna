# Area 2 — Duplication, Dead Code & Integration Debt

## Summary

The repository's ~127k src LOC (130k total, 66k in tests/examples) is broadly justified: most code is load-bearing, well-tested, and distinct. The biggest structural finding is a **verified dual execution-mode model** in `config/fortuna.toml` — `[daemon] data_source/execution` and `[runtime] execution_mode` coexist and their cross-validation logic adds ~150 lines of boot-guard complexity. This is a documented design choice (parallel-agent tracks), not accidental duplication. Two concrete gaps block demo-paper readiness: (1) `perp_event_basis` v1 (rung-0, 326 LOC) sits alongside v2 (1518 LOC) with no `[perp_event_basis]` section in the demo config — rung-0 is inert for the paper-on-live demo but retains its own tests and composition wiring; (2) five abandoned `fortuna_demo_paper_green_*` databases and one `fortuna_demo_paper_live` accumulate silently with no cleanup path. `AnthropicVetoMind` is openly acknowledged as missing in daemon.rs:49 (mech_extremes veto stays `StubVetoMind::allow_all`). The 3,300-line HTML mockup set has zero functional overlap with the real ROTA and is self-contained reference material.

---

## Findings

| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
|---|---|---|---|---|---|---|---|
| P2 | BLOAT-cut | **Dual execution-mode model: `[daemon] data_source/execution` + `[runtime] execution_mode/orders_enabled/production_unlock`** — two overlapping systems for expressing the same configuration intent (run paper vs live) cross-validated in `validate_bootable` | `config/fortuna.example.toml:105-115` ([runtime] block commented); `crates/fortuna-live/src/boot.rs:132-205` (DaemonSection + RuntimeSection + ExecutionMode); boot.rs:570-750 (validate_bootable cross-validation 180 LOC) | Dual model creates four possible combinations; ~150 lines of gate logic enforce legal pairings; any operator who edits one risks violating the other | Parallel agent tracks added `[runtime]` on top of `[daemon] data_source/execution` (the paper-on-live Phase 1 path) rather than unifying | Collapse to one canonical section in a follow-on milestone; until then the existing cross-validation is correct but must be kept in sync | `test: validate_bootable rejects every illegal (runtime, daemon) pairing in a truth-table test` |
| P2 | BLOAT-cut | **Stale DB proliferation: 4 `fortuna_demo_paper_green_*` snapshots + 1 `fortuna_demo_paper_live` accumulate with no DDL cleanup** | `psql -l` output: `fortuna_demo_paper_green_20260617042617` (36 beliefs), `…043157` (unknown), `…043555` (unknown), `…044732` (72 beliefs), `fortuna_demo_paper_live` (36 beliefs) vs live `fortuna_demo` (108 beliefs) | Operator confusion (the `close-the-loop-fixes.md:92` F11 finding confirms the stale `current-demo-db-url` pointer already caused a mis-analysis); disk bloat | No automated cleanup in `scripts/demo-launch.sh`; each aborted run leaves a snapshot | Add a `--drop-stale-snapshots` flag to `scripts/demo-launch.sh` or a `fortuna db prune-snapshots` CLI verb; update `data/runtime/current-demo-db-url` on each successful launch | CLI integration test verifying snapshot count bounded after N launches |
| P2 | PARK | **`perp_event_basis` rung-0 (326 LOC, `crates/fortuna-runner/src/perp_event_basis.rs`) coexists with v2 (1518 LOC)** — both composed via distinct `[perp_event_basis]` vs `[perp_event_basis_v2]` config sections | `crates/fortuna-runner/src/perp_event_basis.rs:1` (module doc: "rung-0"); `crates/fortuna-runner/src/perp_event_basis_v2.rs:1` (module doc: "v2 successor to rung-0"); `crates/fortuna-live/src/compose.rs:27-28` (both imported) | v1 is inert in the demo config (no `[perp_event_basis]` section active); config comment says v2 "COEXISTS with rung-0 [perp_event_basis] above (v2 activates only on coherent, fresh inputs; rung-0 is the fallback)"; so this is documented intentional layering (the "ladder" pattern) | Intentional multi-rung design; the comment in `fortuna.example.toml` explicitly says "rung-0 is the fallback" to v2 | Keep for now — the fallback semantics are valid; remove rung-0 only if v2 proves robust across a full soak cycle. PARK status | None needed while both rungs are intentional |
| P2 | BLOAT-cut | **`AnthropicVetoMind` missing — mech_extremes veto is `StubVetoMind::allow_all` (inert)** | `crates/fortuna-live/src/daemon.rs:49` ("AnthropicVetoMind does NOT exist … so the veto stays StubVetoMind::allow_all"); `daemon.rs:406,419,919,932` (veto_mind = Some(Arc::new(StubVetoMind::allow_all()))) | mech_extremes "reduce-only veto" is permanently bypassed — a named safety layer that exists in the trait/type system is inert at runtime | Phase 2 T2.5 landed the stub but not the real veto mind; tracked in GAPS | Build `AnthropicVetoMind` (the veto.rs `VetoMind` trait is ready at `crates/fortuna-cognition/src/veto.rs:136`); until then document the gap clearly (already done in daemon.rs comment) | DST scenario: `mech_extremes_veto_rejects_borderline_fade_when_real_mind_says_no` |
| P3 | BLOAT-cut | **3,304-line HTML mockup set in `docs/mockups/` duplicates no production ROTA code** — three standalone dashboards with hardcoded fake data | `docs/mockups/fortuna-operator-poc.html` (1399 LOC), `intelligence-workbench.html` (1074 LOC), `ramp-control-tower.html` (831 LOC) — no `fetch()` calls, no reference to `localhost:9187` or ROTA endpoints | Not a functional overlap with `crates/fortuna-ops/src/rota.rs` (the real 24-view ROTA); purely visual design reference — session evidence claim "duplicating real ROTA" is REFUTED | Created 2026-06-17 as design explorations | Delete or move to `docs/design-archive/` post-demo; they add no value once the real ROTA is functional and showing live data | N/A |
| P3 | BLOAT-cut | **World-forward watch events: 20 `watch:` events exist in `fortuna_demo` but 0 have beliefs attached** | `psql -d fortuna_demo -c "SELECT COUNT(*) FROM events WHERE event_id LIKE 'watch:%'"` → 20; `SELECT COUNT(*) FROM beliefs WHERE event_id LIKE 'watch:%'` → 0 | Session evidence claim "0 beliefs attached" CONFIRMED. The discovery path is wired (`[discovery]` block exists in `daemon.rs:1780-2930`) but the config section is commented out (fortuna.example.toml: "enabled = false => daemon is byte-identical") | `[discovery]` is opt-in, OFF by default; the 20 watch events were persisted when the loop was enabled during a soak, but no scoreable sources were registered (the unscoreable rule filtered all to beliefs=0) | Explicitly mark watch events with missing source_registry rows; the resolve loop has no way to clear unresolvable events from the events table | Test: `world_forward_refuses_beliefs_on_unscoreable` already exists at `crates/fortuna-cognition/tests/discovery.rs:444` |
| P3 | BLOAT-cut | **Untracked parallel-agent artifacts: 4 `AMENDMENT-*.md` files, `.agents/`, `.codex/` directories** | Root: `AMENDMENT-track-A-obs2.md` (2733 B), `AMENDMENT-track-C-funding-capture.md` (7174 B), `AMENDMENT-track-C-funding-poller.md` (3502 B), `AMENDMENT-track-C-slice-3b-v2.md` (6397 B); `.agents/skills/fortuna/`, `.codex/agents/` | These are operator-addressed inter-agent routing memos; most of their content (funding_poller, perp_event_basis_v2, OBS-2) appears to have already merged — content of build plans already in code | Multiple parallel agent tracks (A/B/C/D) from the prior sprint used AMENDMENT files as handoffs | Commit or delete once merged content is verified; the `.agents/` and `.codex/` directories are agent scaffolding, not code artifacts | N/A |
| P3 | PARK | **`basis` and `basis_v2` coexist in `fortuna-cognition`** — `basis.rs` (295 LOC, the rung-0 compute_basis kernel) and `basis_v2.rs` (826 LOC, the vol-model kernel) | `crates/fortuna-cognition/src/lib.rs:36-37`; `perp_event_basis.rs:56` uses `basis::compute_basis`; `perp_event_basis_v2.rs:224` uses `basis_v2::` | Both are actively consumed by their respective strategy rungs; the stratified design is documented and intentional | Ladder strategy pattern — rung-0 uses the median-gap kernel, v2 uses the vol-model kernel | Keep both; remove `basis` only when rung-0 is retired | N/A |
| P3 | SERVES | **`aeolus_reliability` and `aeolus_match` — test-only consumers in production code path** | `crates/fortuna-cognition/src/aeolus_reliability.rs:90` (`score_reliability` referenced only in a doc comment at `fortuna-sources/src/nws_climate.rs:20`); `aeolus_match.rs` consumed only in tests; `aeolus_dedup` consumed only in tests and ledger tests | These modules are load-bearing for test coverage of the aeolus pipeline but not directly called in `daemon.rs` production paths (the `aeolus_resolve.rs` calls `bracket_outcome` from `aeolus_reliability`, keeping it alive) | Feature-complete but not yet wired into the live resolve loop by daemon | No action needed — they are part of the weather resolve loop exercised in tests; daemon calls resolve indirectly | `aeolus_e2e.rs` at `crates/fortuna-ledger/tests/aeolus_e2e.rs` covers this |

---

## Trace / narrative

### LOC decomposition

Total workspace (excluding `target/`): **130,333 LOC across 304 `.rs` files**. The breakdown:

- Tests + examples: **66,724 LOC** (159 files) = 51% of total  
- Production src-only: **~63,609 LOC** (145 files) = 49%

Per-crate (all files):

| Crate | LOC |
|---|---|
| fortuna-cognition | 20,375 |
| fortuna-live | 19,848 |
| fortuna-runner | 19,135 |
| fortuna-venues | 18,880 |
| fortuna-ops | 10,904 |
| fortuna-sources | 6,909 |
| fortuna-ledger | 6,194 |
| fortuna-state | 5,179 |
| fortuna-core | 5,140 |
| fortuna-gates | 4,199 |
| fortuna-invariants | 3,433 |
| fortuna-exec | 3,320 |
| fortuna-paper | 2,124 |
| fortuna-killswitch | 2,121 |
| fortuna-cli | 2,083 |
| fortuna-recorder | 489 |

The largest files are `daemon.rs` (4,854 LOC — the entire composition + test harness in one file), `daemon_smoke.rs` (4,017 LOC — the integration test corpus), `runner.rs` (3,972 LOC), and `rota.rs` tests (3,080 LOC). These are large but not duplicative — each is a single-concern file.

### Execution-mode dual model (VERIFIED)

`config/fortuna.example.toml` contains two distinct execution-mode declaration patterns:

**Pattern A (daemon section, lines 151–157):** `[daemon] data_source = "kalshi_prod"` + `execution = "paper"` — the Phase 1 paper-on-live wiring.

**Pattern B (runtime section, lines 105–115):** `[runtime] execution_mode = "live_data_only"` + `orders_enabled = false` — the Phase 2 explicit mode.

Both are present as commented-out examples. The `validate_bootable()` at `boot.rs:570-750` enforces that they are mutually consistent when both appear. The `ExecutionMode` enum (boot.rs:166-193) has 5 variants: `LiveDataOnly`, `DryRun`, `PaperLedger`, `DemoOrders`, `ProductionOrders`. The cross-validation logic is ~150 LOC at boot.rs:570-750.

This is **VERIFIED** as an existing structural oddity (two patterns for the same intent). The session evidence claim is accurate. The risk is operator confusion: a future config edit that sets `[runtime] execution_mode = "production_orders"` while leaving `[daemon] execution = "paper"` creates a refusal at boot but only if the operator runs `config check` first.

### Multiple live DBs (VERIFIED)

`psql -l | grep fortuna` returns 9 databases:
- `fortuna` — the main production schema (owner `fortuna_app`)
- `fortuna_demo` — the active demo DB (108 beliefs, live data)
- `fortuna_demo_paper_green_20260617042617` — snapshot (36 beliefs)
- `fortuna_demo_paper_green_20260617043157` — snapshot
- `fortuna_demo_paper_green_20260617043555` — snapshot
- `fortuna_demo_paper_green_20260617044732` — snapshot (72 beliefs)
- `fortuna_demo_paper_live` — snapshot (36 beliefs)
- `fortuna_dev` — dev DB (no `beliefs` table; used for sqlx test routing per `crates/fortuna-live/tests/pg_journal.rs:6`)
- `alexandria_test` — unrelated (other project)

The `fortuna_demo_paper_green_*` databases are snapshots created by parallel agent runs during the 2026-06-17 demo soak. No cleanup script exists. `docs/close-the-loop-fixes.md:115` already flagged this as F11.

### HTML mockups vs real ROTA (REFUTED — no code duplication)

The three HTML files in `docs/mockups/` contain zero `fetch()` calls, no references to `localhost:9187`, and no ROTA endpoint calls. They are hardcoded visual mockups, not functional duplicates of `crates/fortuna-ops/src/rota.rs`. The real ROTA has 24 `view_*` functions (confirmed at rota.rs:114–1753) serving live DB-sourced JSON; the mockups serve static fake data. These are design reference artifacts, not code duplication. The session evidence claim "duplicating the real ROTA" is **REFUTED**.

### World-forward watch events (VERIFIED: 0 beliefs)

The session evidence claim is confirmed: `fortuna_demo` has 20 watch events in the `events` table and 0 beliefs with `event_id LIKE 'watch:%'` in the `beliefs` table. The discovery loop is opt-in (disabled by default). The 20 events were created during earlier runs. The `[discovery]` section in the config is commented out. This is expected behavior — the feature is implemented but opt-in.

### Parallel-agent artifacts (VERIFIED)

The four `AMENDMENT-*.md` files are inter-agent routing memos for tracks A and C (perp_event_basis_v2, funding rates capture/poller, ingestion OBS-2). Cross-checking against the codebase: `funding_poller.rs` exists and is wired in `main.rs:703-790`; `perp_event_basis_v2.rs` is 1518 LOC and wired in `compose.rs:28`; `funding_rates_historical` table exists with 352 rows in `fortuna_demo`. These tracks appear to have merged. The AMENDMENT files are **navigation artifacts** from the sprint, not live design documents.

`.agents/skills/` contains `fortuna/SKILL.md` and `fortuna-review/SKILL.md` — installed skill definitions for the Claude Code harness, not code artifacts.

### AnthropicVetoMind (VERIFIED: inert stub)

`daemon.rs:49` explicitly documents: "the mech_extremes VETO mind — AnthropicVetoMind does NOT exist (fortuna-cognition, which Track A consumes-not-edits; veto.rs promised it for Phase 2 T2.5 but it never landed), so the veto stays StubVetoMind::allow_all." This is a known gap, acknowledged in the code. The VetoMind trait exists at `crates/fortuna-cognition/src/veto.rs:136`; only the StubVetoMind implementation exists (veto.rs:149-196).

---

## Self-adversarial pass

**1. Dual execution-mode severity might be P3, not P2.** The cross-validation in `validate_bootable` works correctly — it refuses illegal combinations. No demo-breaking path exists. Counter-argument: this is complexity that increases the cognitive load on operators and the probability of a future misconfiguration. P2 stands because the "config operator confusion" risk is real in a production trading system.

**2. "Stale DB proliferation" might be cosmetic.** Postgres databases are cheap. Counter-argument: the `current-demo-db-url` pointer mis-analysis in `close-the-loop-fixes.md:92` shows this already caused a real audit error in the prior session. P2 stands.

**3. Did I miss a real code duplication?** I checked: `basis` vs `basis_v2` (different kernels for different strategy rungs — intentional); `perp_event_basis` vs `perp_event_basis_v2` (ladder rungs — intentional); `mind_from_env` vs `triage_from_env` (distinct tier builders — intentional). No copy-paste duplication found in src files.

**4. The `aeolus_reliability`/`aeolus_match` "test-only" finding might be wrong.** `aeolus_resolve.rs:35` calls `aeolus_reliability::bracket_outcome` — this is in src/, so `aeolus_reliability` IS used in production. I weakened this finding to P3/SERVES to reflect the partial usage.

**5. Missing cargo dead-code warnings.** `cargo build` produced no dead-code warnings (confirmed: command returned empty output). This means the compiler's dead-code pass found nothing to flag — a strong signal that the LOC is actively reachable. This increases my confidence that the LOC count is mostly justified.

**6. Possible false negative: record_kinetics_fixtures.rs (1806 LOC) and record_kalshi_fixtures.rs (1209 LOC) are examples.** These are fixture recorders — large but expected. They count in the test/example LOC bucket, not production src.

---

## Open questions for the Lead

1. **Dual mode unification timeline**: Is there a planned milestone to collapse `[daemon] data_source/execution` and `[runtime] execution_mode` into one canonical section? The cross-validation is correct but fragile — a new operator reading the config sees two separate override patterns.

2. **Stale DB cleanup**: Should `scripts/demo-launch.sh` actively drop `fortuna_demo_paper_green_*` and `fortuna_demo_paper_live` before each run? Or is keeping them for post-hoc analysis intentional?

3. **perp_event_basis rung-0 retirement gate**: What is the condition under which rung-0 (`perp_event_basis`) gets removed? v2 needs a soak benchmark showing it strictly dominates rung-0 before retirement. Is that gate defined?

4. **AnthropicVetoMind priority**: The veto path is currently `StubVetoMind::allow_all`. Is mech_extremes trading with this gap acceptable for the demo, or does this need to be surfaced in the demo launch checklist?

5. **World-forward 20 orphan events**: The 20 `watch:*` events in `fortuna_demo.events` with no beliefs will persist across demo runs (FK on `events`; no TTL). Is there a maintenance plan to expire or archive unresolvable watch events?
