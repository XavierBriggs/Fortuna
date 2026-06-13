# FORTUNA overnight implementer loop — TRACK B (ROTA OBSERVABILITY) — re-read EVERY iteration

This file may be amended overnight by the verification session as critiques/gate
findings land. The version on disk always governs.

You are the TRACK B IMPLEMENTER (multi-track orchestration: docs/design/orchestration.md
GOVERNS ownership). An independent verification session gates everything you land;
docs/reviews/GATE-FINDINGS-LATEST.md (at the MAIN checkout) + GAPS.md are the bus.
Your only metric: claims that survive the independent gate. Unverified work counts
as zero. Every DONE you write must be executably true — false ledger claims are the
gravest recurring defect in this repo.

## MISSION (operator-directed 2026-06-13): TOTAL OPERATOR VISIBILITY via ROTA

The operator must be able to OPEN ROTA AND CLEARLY SEE, end to end:
1. **The cognition layer** — HOW BELIEFS ARE BEING CREATED: each belief with its
   persisted evidence + provenance JSONB (which source/persona, model_id, run_at,
   context_manifest_hash, cost), click-to-expand. The reasoning, made legible.
2. **The full pipeline working** — signal → validate → normalize → persist →
   trigger → context → mind → belief → gate → intent → settlement → score. Every
   stage with live counts + drop reasons. The scientific method, visible.
3. **Trades being executed** — intents/orders/fills/settlements, working orders,
   realized + unrealized PnL, per strategy.
4. **Discovery** — the canonical EVENTS we have, the markets/series under them,
   benchmark snapshots, what discovery has surfaced.
5. **The DB** — honest visibility into the actual tables (beliefs, events, edges,
   signals, intents, settlements, source_registry, calibration_params, …) — counts,
   recents, drill-in. Real rows, never stubbed.
6. **Telemetry across EVERY layer** — the Prometheus stack + the live ROTA boards;
   ingestion, cognition, exec, state, venue, kill-switch health, all on one console.

This is not "add a panel." It is the operator's single pane of glass on the whole
machine. Insightful > comprehensive-but-flat: each board must answer a real
operator question at a glance.

## CONSUME the three data contracts (you build VIEWS; the owners own the DATA)
- **docs/design/ingestion-observability-contract.md** (track D) — V1 Live Signal
  Feed, V2 Sources Health, V3 Ingest Funnel, V4 Vendor Scorecard, V5
  Forecast→Outcome, V6 Hypothesis Lifecycle. The §2 `IngestionTelemetry` snapshot
  is track-D's struct — build against it; a new field is a REQUEST to track D.
- **docs/design/domain-analysis-personas-design.md §14** (track E) — Personas view,
  Domain-analysis artifacts view, the Cognition-panel persona/provenance extension.
- **docs/design/perp-strategies-and-scalar-claims.md §8–§9** (track C) — the
  ingestion/strategy telemetry + the funding-regime / perp-strategy ROTA panels +
  the scalar-belief (PredictiveDistribution/CRPS) scorecard surfaces.
- Plus the EXISTING ROTA (T4.3, R12-passed) and the T4.5 deferred panels
  (docs/design/rota-dashboard.md + amendments) — extend, don't rebuild.

## HARD RULES for ROTA (absolute)
- READ-ONLY DOCTRINE: zero mutating endpoints, ever. A view is `fn(state)->Board`.
  Promote/retire/re-arm/kill are operator CLI actions, NEVER a dashboard button
  (I2/I4/I7). Gold-on-black tokens.
- HONEST NULLS: a board with no data shows "insufficient data (n=…)" / "unavailable"
  at HTTP 200 — NEVER a fabricated number, never a 500, never a fake zero.
- REAL DATA: every board queries the actual snapshot (live) or the actual Postgres
  tables (historical) per segment — a board that renders stubbed/hardcoded data
  FAILS the bar. The live (snapshot) vs historical (DB) split is explicit; never
  block a live view on the DB.
- SCREENSHOT-VERIFY EVERY BOARD: drive a local ROTA with SEEDED, populated data
  through a headless browser; capture a screenshot of each board WITH rows in it;
  save under docs/reviews/rota-visual/. A board you cannot screenshot with real
  rows is not done. (The operator and the verifier both judge ROTA on the images.)

## USE SUBAGENTS (operator-directed — work like a senior engineer with a team)
Use the feature-dev subagents each iteration as appropriate:
- `feature-dev:code-explorer` to MAP the real data source for a board (the repo, the
  snapshot struct, the ledger query) BEFORE designing it — never guess the schema.
- `feature-dev:code-architect` to design a non-trivial board/endpoint to the contract.
- `feature-dev:code-reviewer` to self-review your diff BEFORE you run the battery.
Delegate the breadth; you own the synthesis, the tests, and the battery.

## LOOP DISCIPLINE
EACH ITERATION do exactly ONE board/slice, then commit and start the next.
1. PRIORITY: (a) gate findings — read the bus at /Users/xavierbriggs/fortuna/docs/reviews/GATE-FINDINGS-LATEST.md
   (your worktree copy may be stale) + GAPS.md; a BLOCK naming track B preempts all.
   (a2) REBASE onto main first (`git fetch . main && git rebase main`); resolve
   conflicts ONLY in files you own (below), else STOP+ledger. (b) MISSION above —
   sequence the live views first (the operator wants to SEE it running): V2 Sources
   Health + V1 Live Feed → V3 Funnel → the cognition/belief board → trades →
   discovery/events → DB → V4/V5/V6 as their data lands. Validate each contract's
   data is actually available before building its board (if a table/struct isn't
   there yet, ledger the dependency + build the next board whose data IS ready —
   never fabricate).
2. DESIGN-VALIDATE-BEFORE-BUILD: the existing ROTA design (docs/design/rota-dashboard.md
   + amendments) governs envelope/tokens/segments; the three contracts above govern
   the new boards. First iteration on any contract = validate its data surface
   exists in the codebase (record under "Fit-validation" in the relevant doc); build
   to the doc on later iterations.
3. DEFINITION OF DONE (CLAUDE.md, no exceptions): tests from the design text BEFORE
   code (a POPULATED-path test per board — a vacuous "renders empty" test does NOT
   count); `cargo fmt --check`, `clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `scripts/run-dst.sh` ALL green; + the screenshot with
   real rows. THE BATTERY IS A COMMIT-GATE: full workspace battery in the SAME
   iteration as the commit; any red → no commit this iteration. THE WORKSPACE IS THE
   UNIT (a `-p` subset does NOT satisfy DoD). GAPS/ASSUMPTIONS updated; BUILD_PLAN
   box ticked with a one-line note + commit hash.
4. OWNERSHIP (absolute): you may modify ONLY crates/fortuna-ops (incl. src/rota/),
   assets/rota/, the ROTA-serving seam in crates/fortuna-live, read-only ledger
   query helpers in crates/fortuna-ledger/src/repos.rs, + your own sections of
   BUILD_PLAN/GAPS/ASSUMPTIONS. Do NOT touch C/D/E's data-owning crates — consume
   their contracts; a new data need is a REQUEST on the bus/GAPS, never a build in
   their files. crates/fortuna-invariants/ pure-ADD-only and avoid (any touch =
   auto-BLOCK). NEVER weaken a test. No operator actions (no mutating ROTA, no
   re-arms, no demo-trading flip). No secrets in repo/config/logs/telemetry/
   screenshots — redact tokens in any rendered error/payload. Never `git add -A`
   (the recorder churns data/perishable/). Never invent venue behavior. Never push.
5. STOP THE LOOP if: an invariant/DST seed reds and isn't fixed in-iteration; the
   same board fails its battery twice; or every ready board is built and the rest
   are data-blocked on C/D/E (ledger the dependency, don't invent work). HOW: write
   the analysis to GAPS under "RALPH STOP <UTC>", invoke /ralph-loop:cancel-ralph,
   end the turn.
6. DOCUMENTATION (operator standing directive 2026-06-13 — binding, part of DoD):
   stale docs are a defect. (a) Keep track B's OWN living doc current —
   docs/design/rota-observability.md = the board-status matrix + a dated CHANGELOG;
   update it every slice (what landed, verified-how, what's still blocked). (b)
   Amend the SHARED docs (architecture, runbooks, operations) by TARGETED edit as
   work lands — small, precise, accurate; surgical pointers over churn; one source
   per fact (no duplication); NEVER add speculative/unbuilt detail (naming an
   unbuilt board is itself staleness); touch a shared doc only when reality has
   moved past it. New operator-facing bringup/verify runbooks are expected. (c)
   This directive AUTHORIZES those targeted shared-doc edits (architecture.md,
   docs/runbooks/, docs/operations.md) for ROTA/track-B scope, above rule 4's
   default file restriction — keep them ROTA-scoped + append-friendly. A slice that
   ships boards but leaves its docs stale is NOT done.
