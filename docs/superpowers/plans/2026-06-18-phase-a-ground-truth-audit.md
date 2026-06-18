# Phase A — Ground-Truth Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run a read-only, evidence-based, ensemble deep audit of FORTUNA that produces the **atlas** (`AUDIT.md`), the **MVP closure plan** (which becomes Phase C), the **refactor/consolidation roadmap** (which becomes Phase B), a **branch ledger**, a **doc triage**, and a **Demo-Paper-Ready Readiness Scorecard**.

**Architecture:** The installed `deep-codebase-audit` skill is the brief. Nine independent audit areas each run a paired **Auditor + Adversary** subagent (read-only). A **Verifier** subagent re-checks every `file:line` citation. Two non-code legs (branch ledger, doc triage) run as their own tasks. The **Lead** (the executing session — not a subagent) synthesizes everything into the atlas + closure plan. Areas are independent and may be dispatched in parallel.

**Tech Stack:** Rust workspace (`crates/`), Postgres (`sqlx`), the `deep-codebase-audit` skill at `.claude/skills/deep-codebase-audit/`, subagents via the Agent tool.

## Global Constraints

Every area/leg subagent brief implicitly includes ALL of the following — copy into each dispatch:

- **READ-ONLY. No code edits, no migrations, no order placement, no mutating commands.** Output is findings files only.
- **Read the skill first:** `.claude/skills/deep-codebase-audit/SKILL.md` + `resources/fortuna_trading_system_profile.md` + `resources/audit_checklist.md`. Follow its protocol and severity scale.
- **Evidence rule:** every finding cites an exact `path:line` (or `MISSING: <thing> — expected at <where>`). No unsupported claims. **Verify against code, never trust README/docs/CLAUDE.md/prior claims.**
- **Severity:** P0 (money loss / unsafe live exec / security / data loss) · P1 (blocks MVP/demo correctness) · P2 (maintainability/test/delivery risk) · P3 (cleanup).
- **Readiness lens:** tag EVERY finding `BLOCKS` / `SERVES` / `BLOAT-cut` / `PARK` against the target state `fortuna start paper-demo` (live Kalshi data → closed paper loop → settled + scored, no order path, all strategies, one chain-view).
- **Ground-truth DB caveat:** the LIVE demo DB is `fortuna_demo` (last write most recent). The pointer file `data/runtime/current-demo-db-url` is STALE (points at an abandoned `…green_044732` frozen 12:50 UTC). Verify any DB claim against `fortuna_demo` via `psql -d fortuna_demo`; do not trust the pointer.
- **Audit the working tree** of branch `feature/paper-on-live-data` (the de-facto trunk). The tree has substantial UNCOMMITTED parallel-agent changes — audit what is on disk (that is the running reality) and explicitly flag any major uncommitted divergence.
- **Output structure** (each area writes `docs/audit/2026-06-18/area-N-<slug>.md`):
  ```
  # Area N — <name>
  ## Summary (3-5 sentences: is this area demo-ready? biggest risk?)
  ## Findings
  | Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
  ## Golden-path / subsystem trace (narrative with citations)
  ## Open questions for the Lead
  ```
- The **session evidence** listed under each task is a set of *claims to verify or refute* — strong leads from prior investigation, NOT ground truth. Confirm each with a fresh citation or mark it refuted.

---

## Task 0: Audit workspace + dispatch prep

**Files:**
- Create: `docs/audit/2026-06-18/` (output dir)
- Create: `docs/audit/2026-06-18/README.md` (index of area files + status)

- [ ] **Step 1:** Create the output dir and an index file listing the 9 areas + 2 legs + verifier + synthesis with a status column (`pending`/`done`).
- [ ] **Step 2:** Confirm the skill is present: `ls .claude/skills/deep-codebase-audit/` shows `SKILL.md`, `resources/`, `templates/`.
- [ ] **Step 3:** Confirm `psql -d fortuna_demo -c "select 1"` works (the live DB is reachable for area auditors that need DB facts).
- [ ] **Step 4: Commit** `chore(audit): scaffold Phase-A audit workspace`.

---

## Tasks 1–9: Area audits (each = Auditor + Adversary, read-only)

> Dispatch pattern for EACH area: (a) dispatch an **Auditor** (`general-purpose`) with the Global Constraints + the area's scope/questions/evidence; it writes `area-N-<slug>.md`. (b) dispatch an **Adversary** (`general-purpose`) given the Auditor's file with the brief *"challenge every finding: is the citation real? is the severity right? what did the auditor MISS? what's a false positive?"* — it appends an `## Adversary challenge` section. (c) Lead skims both; unresolved disputes go to the Verifier (Task 10). Areas 1–9 may be dispatched in parallel (independent, read-only).

### Task 1: Area 1 — Critical paths / the spine
**Files:** Create `docs/audit/2026-06-18/area-1-spine.md`
**Scope:** Trace the 6 golden paths end-to-end across crates, each as wired / open / duplicated:
market-data → snapshot → strategy input; decision → proposal; risk gate → seal; execution → fill/cancel/ack; accounting → settlement → realized/unrealized PnL; replay.
**Key questions:** Where does the loop break? Is PnL reconstructable from events? Is every decision tied to a timestamped snapshot?
**Session evidence to verify/refute:** F1 settlement unwired (`crates/fortuna-live/src/daemon.rs:1452` "Phase-2 follow-on"; `settlement_entries`=0 in `fortuna_demo`). F0 calibration fit-but-never-persisted (`daemon.rs:4184 run_weekly_review`; `review.rs:101 fit_platt`; NO production caller of `CalibrationParamsRepo::insert` repo-wide → `calibration_params`=0 → synthesis sizes zero). Dual mode model: `[runtime] execution_mode` (`boot.rs:166`) vs `[daemon] data_source/execution` both in `config/fortuna.toml`. Book path is REST-polled (`runner.rs:827`), not WS-streamed.
- [ ] **Step 1:** Dispatch the Auditor with Global Constraints + the above.
- [ ] **Step 2:** Dispatch the Adversary on the Auditor's output.
- [ ] **Step 3:** Lead reviews both; mark Task 1 `done` in the index; note disputes for the Verifier.

### Task 2: Area 2 — Duplication, dead code & integration debt
**Files:** Create `docs/audit/2026-06-18/area-2-duplication.md`
**Scope:** Overlapping/competing implementations, dead/unreachable code, parked threads, copy-paste across crates; the real "why 127k LOC" decomposition.
**Key questions:** What is load-bearing vs cruft? What can be safely deleted? Which abstractions are duplicated?
**Session evidence to verify/refute:** Two mode models (Area 1). Multiple live DBs (`fortuna`, `fortuna_demo`, several `fortuna_demo_paper_green_*`). Mockups (`docs/mockups/*.html`) vs the real ROTA. World-forward watch events are a dead-end (0 beliefs attached, `fortuna_demo`). Crate sizes: cognition ~20k, live ~19k, runner ~19k, venues ~18k LOC. Untracked `AMENDMENT-*.md`, `.agents/`, `.codex/`.
- [ ] **Step 1–3:** Auditor → Adversary → Lead review (as Task 1).

### Task 3: Area 3 — Safety & invariant integrity
**Files:** Create `docs/audit/2026-06-18/area-3-safety.md`
**Scope:** Verify I1–I7 + the skill's trading invariants are enforced by CODE/TESTS (not convention), with no bypass introduced by parallel work.
**Key questions:** Any path from a model/strategy to order mutation that skips the gate seal? Can a paper/live-data mode construct an execution path? Is the audit log truly append-only? Kill-switch independence?
**Session evidence to verify/refute:** Sealed `GatedOrder` (`fortuna-gates`), `i_paper_live_no_real_order.rs` invariant test (panics on real exec endpoint), kill-switch standalone no-Postgres (I4), append-only audit (931 rows, `fortuna_refuse_mutation` trigger), propose-only/unsized legs (I6), `execution_mode` enforcement (`daemon.rs:777-791`). Check `crates/fortuna-invariants/` is intact (protected; additions-only).
- [ ] **Step 1–3:** Auditor → Adversary → Lead review.

### Task 4: Area 4 — Module boundaries & legibility
**Files:** Create `docs/audit/2026-06-18/area-4-legibility.md`
**Scope:** Giant files, tangled responsibilities, unclear interfaces; what each crate/module owns; highest-value file-split/refactor targets.
**Key questions:** Which files are too large to hold in context / do too much? Where are responsibilities mixed? Minimal restructuring for legibility?
**Session evidence to verify/refute:** Large files: `crates/fortuna-live/src/daemon.rs` (>4500 lines), `crates/fortuna-runner/src/runner.rs`, `crates/fortuna-ledger/src/repos.rs`, `crates/fortuna-ops/src/rota.rs`. Use `find crates -name '*.rs' | xargs wc -l | sort -rn | head -20`.
- [ ] **Step 1–3:** Auditor → Adversary → Lead review.

### Task 5: Area 5 — Vendor/venue coupling
**Files:** Create `docs/audit/2026-06-18/area-5-vendor-coupling.md`
**Scope:** Does Kalshi/Kinetics shape leak into core / risk / PnL / strategies? Adapter boundary cleanliness.
**Key questions:** Do strategies/risk/PnL read raw vendor payloads or vendor-neutral domain types? Could a second venue be added without touching strategy/gate logic?
**Session evidence to verify/refute:** `Venue` trait (`crates/fortuna-venues/src/lib.rs:91`), `KalshiReadClient` (read-only), `PaperLiveVenue`, `Cents`/`GatedOrder` domain types at the boundary. Adapters: `kalshi/`, `kinetics`, `polymarket/`, `sim`.
- [ ] **Step 1–3:** Auditor → Adversary → Lead review.

### Task 6: Area 6 — Test & replay posture
**Files:** Create `docs/audit/2026-06-18/area-6-tests-replay.md`
**Scope:** Classify existing tests (unit/integration/contract/e2e/replay/property/DST/soak); the test-gap report; can replay reproduce decisions deterministically?
**Key questions:** Which safety/critical-path behaviors are untested? Does replay rebuild decisions + PnL from recorded events?
**Session evidence to verify/refute:** DST corpus (`scripts/run-dst.sh`, `crates/fortuna-core/dst-corpus/`), invariant tests (`crates/fortuna-invariants/tests/`), contract/fixture tests (`crates/fortuna-venues/tests/`, `fixtures/kalshi/`), `scripts/replay.sh`. Mind tests 19/19 (`fortuna-cognition/tests/mind.rs`).
- [ ] **Step 1–3:** Auditor → Adversary → Lead review.

### Task 7: Area 7 — Operational readiness, demo CLI & observability
**Files:** Create `docs/audit/2026-06-18/area-7-ops-cli.md`
**Scope:** Kill switch, WS reconnect, rate limits, logs/metrics/alerts, runbooks; the gap to a clean `fortuna start paper-demo`; the single chain-view.
**Key questions:** Is there one clean demo entrypoint? Does ROTA show mode/order-mutation/freshness/the chain? WS reconnect + rate-limit + timeout behavior?
**Session evidence to verify/refute:** `scripts/demo-launch.sh` (current entrypoint), CLI verbs (`crates/fortuna-cli/src/main.rs`: status/halt/rearm/kill/config check/start/stop; NO `doctor`). ROTA ~26 sections (`crates/fortuna-ops/src/rota.rs`). `dead-man ping FAILED` in `data/runtime/logs/daemon.log`. WsDial backoff (`kalshi/dial.rs`). Rate limits (`[gates.rate]`). Stale `current-demo-db-url`.
- [ ] **Step 1–3:** Auditor → Adversary → Lead review.

### Task 8: Area 8 — Cognition: Mind, personas & belief authoring
**Files:** Create `docs/audit/2026-06-18/area-8-cognition.md`
**Scope:** Mind trait/tiers/budget/degrade; persona registry/config/runner/orchestrator; `domain_analyses`; how beliefs are authored (deterministic vs model).
**Key questions:** Is the Mind producing any durable artifact, or spending budget for nothing? Which personas are wired vs config-only? What activates a persona?
**Session evidence to verify/refute:** Mind trait works (`fortuna-cognition/tests/mind.rs` 19/19; `failed_calls_burn_into_spent_today`). `CostBudget` resets at UTC midnight + on restart (`mind.rs:265 roll`). In `fortuna_demo`: beliefs 100% `provenance.model_id=aeolus` (Mind authors 0), `personas`=0, `domain_analyses`=0. Persona configs exist (`config/personas/meteorologist`, `config/personas/macro-economist`) but `[personas]` opt-in/OFF (`boot.rs:319`). Budget exhausted then raised to $30/day (`config/fortuna.toml`).
- [ ] **Step 1–3:** Auditor → Adversary → Lead review.

### Task 9: Area 9 — Discovery, seeding & signal→event pipeline
**Files:** Create `docs/audit/2026-06-18/area-9-discovery-seeding.md`
**Scope:** World-forward + market-back discovery; signal ingestion/normalization; event seeding (operator + auto); event dedup; catalog→edge minting; the watch-event dead-end. Produce a clean target design for seeding + signal→event.
**Key questions:** Why does seeding/signals feel like a mess? Where do watch events die? Is dedup robust or brittle? Can an operator cleanly seed an event?
**Session evidence to verify/refute:** world-forward (`discovery.rs:632`, `[discovery].signal_kinds`), market-back (`discovery.rs:312`, `decide_structured`). Event dedup heuristic: `daemon.rs:1611 event_text_similarity` (Jaccard ≥0.55, or ≥0.35 same category+benchmark) + hardcoded `event_family_key:1626` (Miran/Warsh/FOMC/stress-test/wind only). Category vocab chaos in `fortuna_demo` events (`macro`/`Macro/Fed`/`monetary_policy`/`economy`/`x`). Signals ~27.5k; watch events have 0 beliefs attached.
- [ ] **Step 1–3:** Auditor → Adversary → Lead review.

---

## Task 10: Verifier — citation & dispute pass
**Files:** Create `docs/audit/2026-06-18/verification.md`
**Depends on:** Tasks 1–9.
**Brief (subagent_type `verifier`, read-only):** "For each finding across `area-1..9-*.md`, open the cited `path:line` and confirm it supports the claim. Re-run any cited DB query against `fortuna_demo`. Resolve every Auditor↔Adversary dispute with a verdict + fresh citation. Output a table: `Finding | Citation valid? | Verdict | Note`. Flag any P0/P1 whose evidence does NOT hold — those get downgraded or struck."
- [ ] **Step 1:** Dispatch the Verifier.
- [ ] **Step 2:** Lead applies the verifier's verdicts to the area files (strike/downgrade unsupported findings). Mark Task 10 `done`.

## Task 11: Branch ledger (no work lost)
**Files:** Create `docs/audit/2026-06-18/branch-ledger.md`
**Brief (subagent_type `general-purpose`, read-only EXCEPT git tag):**
- [ ] **Step 1:** Tag every branch immutably first: for each `b` in `git branch --format='%(refname:short)'`, run `git tag archive/$b $b` (recoverable forever; no deletion in Phase A).
- [ ] **Step 2:** For each branch, content-diff vs trunk `feature/paper-on-live-data`: `git diff --stat feature/paper-on-live-data...$b` and `git log --oneline feature/paper-on-live-data..$b`. Classify `absorbed` (no unique content) / `stranded` (real unmerged work — list files) / `redundant`.
- [ ] **Step 3:** Write the ledger table: `Branch | classification | unique files | evidence | recommended action`. **Recommend deletions but DO NOT delete** (Phase B, after review). Judge by content, not ancestry.
- [ ] **Step 4:** Mark Task 11 `done`.

## Task 12: Doc triage
**Files:** Create `docs/audit/2026-06-18/doc-triage.md`
**Brief (subagent_type `general-purpose`, read-only):** Inventory `*.md` at repo root + `docs/**`. Classify each `authoritative` / `stale` / `archive`, with a one-line reason and (for stale) the contradicting code citation. Identify the intended single source of truth per topic (spec, runbook, changelog).
- [ ] **Step 1:** Dispatch. Cover at least: `docs/spec.md`, `CLAUDE.md`, `AGENTS.md`, `README.md`, `GAPS.md`, `ASSUMPTIONS.md`, `CHANGELOG.md`, `docs/design/*`, `docs/reviews/*`, `docs/mockups/*`, `AMENDMENT-*.md`, the vision docs.
- [ ] **Step 2:** Mark Task 12 `done`.

## Task 13: Lead synthesis — the atlas + MVP closure plan
**Files:**
- Create: `docs/audit/2026-06-18/AUDIT.md` (use `.claude/skills/deep-codebase-audit/templates/audit_report.md`)
- Create: `docs/audit/2026-06-18/MVP-CLOSURE-PLAN.md` (use `templates/mvp_closure_plan.md`)
**Depends on:** Tasks 1–12. **Done by the Lead (this session), not a subagent.**
- [ ] **Step 1:** Fill `AUDIT.md` from the verified area files: exec summary, what-it-is, north star, architecture map, critical-path table, risk register (P0–P3, deduped + grouped by root cause), vendor-coupling report, test-gap report, security review, ops readiness, refactor roadmap (**= Phase B**), next-3-moves.
- [ ] **Step 2:** Fill `MVP-CLOSURE-PLAN.md` (**= Phase C**): north star, definition of done (`fortuna start paper-demo`), required loop, current gaps table (severity-ranked from the audit), the 5 phased sections, non-negotiable safety checks.
- [ ] **Step 3:** Add the **Demo-Paper-Ready Readiness Scorecard** to `AUDIT.md`: roll up every `BLOCKS` finding = the exact distance to the demo, in severity order.
- [ ] **Step 4: Commit** `docs(audit): Phase-A atlas, MVP closure plan, branch ledger + doc triage`.
- [ ] **Step 5:** Present to the operator: the bottom-line readiness verdict, the P0/P1 blockers, and the recommended next move (which becomes the Phase B and Phase C plans).

---

## Self-Review (Lead, before Task 13 Step 5)

- **Spec coverage:** every spec §4.1 area (1–9) has a task; both legs (§4.2) have tasks; the readiness lens (§4.3) is in every area's output + the scorecard. ✓
- **Driven-by-audit:** B and C are authored in Task 13 from verified findings — not pre-written. ✓
- **No placeholders:** each area task carries real scope + key questions + citable session evidence to verify. ✓
- **Consistency:** output paths (`docs/audit/2026-06-18/area-N-*.md`), the `fortuna_demo` DB caveat, and the read-only constraint are uniform across all tasks. ✓
