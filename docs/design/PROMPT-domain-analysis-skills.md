# Session brief — own the FORTUNA domain-analysis "skills" / persona system

Hand this entire file to a fresh Claude Code session. It is the complete brief
for designing and building ONE feature end to end: a versioned, auditable
library of domain-expert reasoning personas ("skills") plus a persisted
"domain-analysis artifact" that those personas produce, which FORTUNA's
cognition layer uses when forming beliefs. You own it from brainstorm → spec →
implementation → gate.

---

## 0. First moves (do these before anything else)

1. You are in the FORTUNA repo. Read, in order: `CLAUDE.md`, `docs/spec.md`
   (the AUTHORITY; especially §3 invariants, §5.5 beliefs, §5.7 context
   assembler, §5.8 loops/triggers, §5.9 Mind, §5.10 calibration, §5.11 signal
   ingestion, §5.12 discovery), then `GAPS.md` and `ASSUMPTIONS.md`. Invoke the
   `fortuna` skill and the `superpowers:brainstorming` skill at the start.
2. This is a DESIGN-FIRST task. Do NOT write code until you have a written,
   committed design doc the operator has approved (brainstorming skill gates
   this). The spec wins on every conflict; where the spec is silent, record the
   gap in `GAPS.md` and choose the conservative option.
3. Use a subagent (the `Explore` agent) to map the existing cognition crate
   before designing — see §4. Do not inherit my description of the code; verify
   it.

## 1. What you are building (the idea, in one paragraph)

Today FORTUNA's Mind reasons over raw ingested signals (weather forecasts,
news, macro releases) to form beliefs. We want to insert a layer of
DOMAIN EXPERTISE between the signals and the beliefs: a library of reusable
"analyst personas" — a meteorologist for weather, a macro-economist for
CPI/NFP, a political analyst for elections, a box-office/awards analyst for
entertainment — each encoding HOW a professional in that field researches the
available data and reports findings. A persona runs (cheaply, on a schedule or
trigger), reads the relevant ingested signals, and emits a structured,
persisted **domain-analysis artifact** ("NYC heat this week: ridge building,
high confidence, σ tightening, key risk = onshore flow Thu"). MANY downstream
beliefs/events then reference that one analysis instead of each re-reasoning
from raw data. The personas are versioned and SCOREABLE: because every belief
records which persona+version informed it (provenance), FORTUNA can measure
whether "meteorologist v3" produces better-calibrated beliefs than v2 and
promote/retire personas the same way it promotes strategies. This closes the
scientific-method loop on the REASONING, not just the strategies.

## 2. Hard boundaries (non-negotiable — get these wrong and the feature is wrong)

- **This is a COGNITION / Mind feature. It is NOT a source adapter.** The
  `crates/fortuna-sources` layer (a separate track, "Track D") is DUMB
  deterministic acquisition — fetch, emit, never reason (spec 5.11). You do not
  touch it and you do not put reasoning in it. Your personas consume signals
  that are ALREADY in the append-only `signals` table; they never fetch.
- **Invariant I6 (propose-only): the model has zero tools that mutate external
  state.** A persona reasons over already-ingested data and emits an analysis
  artifact + (eventually) belief drafts. It NEVER fetches, sizes, places orders,
  or mutates anything outside the cognition store. "Research the weather" means
  "reason over the ingested weather signals," never "go get more data."
- **Trusted scaffolding vs. untrusted data (spec 5.11).** The persona METHOD
  (the prompt/procedure: what a meteorologist looks at, what questions they ask)
  is operator-authored TRUSTED material, like the system charter — it may shape
  the model's reasoning. The SIGNALS it reasons over remain UNTRUSTED data,
  passed only inside the context assembler's delimited data blocks. Keep these
  two streams structurally separate so a poisoned news payload can never rewrite
  a persona's method. This separation is the heart of the design — make it
  explicit and testable.
- **I1 (universal gate) / I5 (append-only audit) still apply.** Any belief that
  results still passes the same deterministic gate pipeline. The analysis
  artifact and every persona invocation are append-only audited; a belief's
  provenance records the persona id + version + the analysis-artifact hash so a
  decision replays byte-identically (spec 5.7 manifest discipline).
- **Determinism / Clock.** All time via the injected `Clock`; no wall-time. LLM
  calls go behind the existing `Mind` trait (or its tiering); cheap-tier for
  routine analysis, synthesis-tier only where it earns it. Per-cycle/per-day
  cost budgets apply (spec 5.9) — a persona library must not blow the budget;
  the once-per-region-per-day artifact pattern is the cost lever (one analysis,
  many beliefs).
- **House style (CLAUDE.md):** Rust 2021, no `panic!`/`unwrap`/`expect` in
  money/cognition paths, `thiserror` per crate, ULIDs, UTC ISO8601, sqlx
  compile-checked queries + one migration per schema-touching task, `cargo fmt`
  + `clippy -D warnings` + full tests + `scripts/run-dst.sh` all green, tests
  written from the spec BEFORE implementation. Never weaken a test in
  `crates/fortuna-invariants/`.

## 3. The shape to design (a starting point, not a mandate — improve it)

Decompose and brainstorm, but these are the pieces you will likely need:

- **Persona definition.** A versioned, declarative description of a domain
  analyst: id, domain tags, the trusted method/prompt, which signal kinds it
  reads, which model tier, its output schema. Stored where? (config file +
  registry table? a `personas` table?) — decide and justify. It must be
  versioned and immutable-by-supersession (append-only discipline).
- **Domain-analysis artifact.** The persisted structured output of one persona
  run: id (ULID), persona id+version, domain, the signal manifest it consumed
  (ids + hashes, point-in-time like 5.7), `produced_at`, a structured findings
  payload, and a content hash. Append-only. This is the reusable thing many
  beliefs reference.
- **The runner / trigger integration.** When does a persona run? On the
  schedule/triggers that already exist (spec 5.8) — a new signal of its domain
  arrives, a daily cadence, a release window. It must serialize per-event like
  the decision cycle and respect the cost budget. Likely a new loop or an
  extension of the discovery loops (5.12).
- **How beliefs consume the artifact.** Synthesis (the existing belief-forming
  path) reads the relevant analysis artifact as a high-value context item and
  cites it in evidence/provenance. Define the reference so a belief's provenance
  names (persona id, version, artifact hash).
- **Scoring & promotion (the loop — design this carefully).** Persona-attributed
  beliefs are scored (Brier/CLV) at settlement; the weekly/monthly review (5.8)
  aggregates per-(persona, version, domain) calibration and proposes
  promote/retire — HUMAN-gated, exactly like strategy promotion (I7 analog). A
  persona that doesn't beat the no-persona baseline gets retired on the record.
- **Bootstrapping personas.** Start with ONE (the meteorologist, since the
  weather domain + Aeolus contract are the most developed — see
  `docs/design/aeolus-fortuna-source-contract.md`). Prove the whole loop on one
  domain before generalizing to economist/political/entertainment.

## 4. Explore before you design (use the Explore subagent)

Map the real cognition crate — do not trust this brief's description:
- `crates/fortuna-cognition/src/`: the `Mind` trait + tiering (`mind.rs`), the
  context assembler (`context.rs`), the decision cycle (`cycle.rs`), beliefs
  (`beliefs.rs`), discovery loops (`discovery.rs`), calibration
  (`calibration.rs`), the weekly/monthly review (`review.rs`),
  reconciliation/Aeolus parsing (`reconciliation.rs`), the signals subsystem
  (`signals.rs`).
- `crates/fortuna-ledger/`: the append-only tables + migration pattern (you will
  likely add a `personas` and/or `domain_analyses` table — one migration per
  task, append-only INSERT-only repos, superseding rows for "updates").
- How beliefs currently carry `evidence` and `provenance` JSON (you extend
  these to cite a persona + artifact).
- The existing `lessons` / lesson-promotion machinery in the weekly review —
  your persona promotion should reuse, not reinvent, it.

## 5. Definition of done

A committed, operator-approved design doc under `docs/superpowers/specs/` or
`docs/design/`; then an implementation that (a) ships the persona definition +
domain-analysis artifact + the runner + belief-consumption + scoring/promotion,
(b) is proven end-to-end on the meteorologist persona over the existing weather
signals (Aeolus + NWS), (c) has tests written from the spec first incl. the
trusted-vs-untrusted separation and a DST scenario for the runner under the cost
budget, and (d) passes the full battery (fmt, clippy -D warnings, workspace
tests, run-dst) with the invariant crate untouched. Update GAPS.md /
ASSUMPTIONS.md for every spec-silence you resolve.

## 6. Coordination

- A parallel track ("Track D") owns `crates/fortuna-sources` (the dumb source
  adapters) and is actively building NWS/RSS/Calendar/GDELT + the ingestion
  scheduler. DO NOT modify that crate; you consume the `signals` it produces.
  If you need a new signal kind ingested, that is a request to Track D / a GAPS
  note, not something you build in the sources layer.
- The weather domain's source side is specified in
  `docs/design/aeolus-fortuna-source-contract.md` — read §5 (trust framework)
  and §11 (it explicitly hands this persona/analysis feature to you and marks
  the boundary). The meteorologist persona is the natural first consumer of the
  Aeolus μ/σ forecast + the NWS observed-high grader.
- Work on your own branch/worktree; rebase on main; never weaken the invariant
  crate; never push without being asked.

## 7. The opening question to bring back to the operator

After exploring, before finalizing the design, surface the ONE decision that
most shapes the feature: **is the domain-analysis artifact a persisted,
reusable record that many beliefs reference (richer, cheaper at scale, more
auditable), or just an ephemeral reasoning step inside one belief's synthesis
(simpler, less reuse)?** Recommend the persisted-artifact version (it is the
reason the feature is worth building and it closes the scoring loop), but let
the operator decide before you commit the spec.
