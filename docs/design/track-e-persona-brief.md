# Track E — Domain-analysis "skills" / persona system (operator-directed 2026-06-13)

You own ONE feature end to end: a versioned, auditable library of domain-expert
reasoning personas ("skills") + a persisted "domain-analysis artifact" those
personas produce, which FORTUNA's cognition layer uses when forming beliefs.
brainstorm → spec → implementation → gate. DESIGN-FIRST (see §0).

## 0. First moves (before anything else)

1. You are in the FORTUNA repo (worktree fortuna-wt-e, branch track-e). Read in
   order: CLAUDE.md, docs/spec.md (the AUTHORITY — esp. §3 invariants, §5.5
   beliefs, §5.7 context assembler, §5.8 loops/triggers, §5.9 Mind, §5.10
   calibration, §5.11 signal ingestion, §5.12 discovery), then GAPS.md,
   ASSUMPTIONS.md. Invoke the `fortuna` skill and `superpowers:brainstorming`
   at the start.
2. DESIGN-FIRST. Do NOT write code until a written, committed design doc is
   OPERATOR-APPROVED (brainstorming gates this). The spec wins on every
   conflict; where it is silent, record the gap in GAPS.md and choose the
   conservative option.
3. Use the Explore agent to MAP the existing cognition crate before designing
   (§4). Do not inherit any description of the code; verify it.

## 1. What you are building (one paragraph)

Today FORTUNA's Mind reasons over raw ingested signals to form beliefs. Insert
a layer of DOMAIN EXPERTISE between signals and beliefs: a library of reusable
"analyst personas" — a meteorologist for weather, a macro-economist for
CPI/NFP, a political analyst for elections, a box-office/awards analyst for
entertainment — each encoding HOW a professional in that field researches the
available data and reports findings. A persona runs (cheaply, on a schedule or
trigger), reads the relevant ingested signals, and emits a structured,
PERSISTED domain-analysis artifact ("NYC heat this week: ridge building, high
confidence, σ tightening, key risk = onshore flow Thu"). MANY downstream
beliefs/events reference that one analysis instead of each re-reasoning from
raw data. Personas are VERSIONED and SCOREABLE: every belief records which
persona+version informed it (provenance), so FORTUNA can measure whether
"meteorologist v3" yields better-calibrated beliefs than v2 and promote/retire
personas exactly like it promotes strategies.

## 2. Invariant fit (non-negotiable)

- The persona is part of the MODEL/cognition side — propose-only (I6). It
  produces an analysis ARTIFACT (data), never an order or a sizing. The harness
  still owns everything downstream. The persona has no tools that mutate
  external state.
- Persona prompts + the personas/artifacts they read are UNTRUSTED-data-shaped
  where they include ingested signals (§5.11): signal content reaches the model
  only inside delimited data blocks; a persona definition is trusted config
  (operator/versioned), the SIGNALS it reads are not.
- I5: persona runs, artifacts, and the persona+version provenance on every
  belief are append-only/auditable; a belief must replay to the exact persona
  version + artifact that informed it.
- Cost: persona runs consume the cognition cost budget (§5.9); a budget breach
  degrades (skip the persona run, beliefs fall back to raw-signal reasoning),
  never crashes. DST a runner under the budget.

## 3. The operator decision to surface FIRST (§7 — the design gate)

After exploring, BEFORE finalizing the design, bring back the ONE decision that
most shapes the feature: is the domain-analysis artifact a PERSISTED, reusable
record many beliefs reference (richer, cheaper at scale, auditable, closes the
scoring loop) — OR an ephemeral reasoning step inside one belief's synthesis
(simpler, less reuse)? RECOMMEND the persisted-artifact version (it is the
reason the feature is worth building and it closes the scoring loop), but the
OPERATOR decides before you commit the spec. This is your design-phase STOP
point: write the design doc, surface this question, RALPH STOP requesting
approval. Do not build until the operator approves.

## 4. Map before you design (Explore agent)

- The cognition crate: the Mind trait + AnthropicMind, the context assembler
  (§5.7 — budgeted packing, manifests), how a belief is currently formed
  (signal → trigger → context → mind → BeliefDraft), the evidence + provenance
  JSON on beliefs (you EXTEND these to cite persona + artifact).
- The ledger schema + migration pattern (you will likely add `personas` and/or
  `domain_analyses` tables — one migration per task, append-only INSERT-only
  repos, superseding rows for "updates").
- The lessons / lesson-promotion machinery in the weekly review — persona
  promotion REUSES it, never reinvents.

## 5. Definition of done

(a) An operator-approved design doc under docs/design/; then an implementation
shipping the persona definition + domain-analysis artifact + the runner +
belief-consumption + scoring/promotion; (b) proven END-TO-END on the
METEOROLOGIST persona over the existing weather signals (Aeolus + NWS); (c)
tests from spec first incl. the trusted-vs-untrusted separation + a DST
scenario for the runner under the cost budget; (d) full battery green (fmt,
clippy -D warnings, cargo test --workspace, scripts/run-dst.sh) with the
invariant crate UNTOUCHED. Update GAPS/ASSUMPTIONS for every spec-silence
resolved. EVERY claim survives the independent gate.

## 6. Coordination (ownership boundaries — absolute)

- Track D owns crates/fortuna-sources (the dumb adapters); it is actively
  building NWS/RSS/Calendar/GDELT + the scheduler. DO NOT modify that crate —
  you CONSUME the signals it produces. A new signal kind you need is a REQUEST
  to track D / a GAPS note, never something you build in the sources layer.
- The weather source contract is docs/design/aeolus-source-contract.md (read
  its trust-framework section + the boundary it hands you). The meteorologist
  persona is the natural first consumer of the Aeolus μ/σ forecast + the NWS
  observed-high grader.
- YOU OWN: the persona/domain-analysis LAYER inside crates/fortuna-cognition
  (new modules) + the new ledger tables/repos (one migration per task). You
  must NOT break the existing Mind/belief interface that track A composes in
  fortuna-live — extend, don't rewrite; if the interface must change, that is a
  design-doc item + a GAPS coordination note, gated.
- Branch track-e / worktree fortuna-wt-e; rebase on main each iteration
  (`git rebase --reapply-cherry-picks main` if a revert is in history); never
  weaken the invariant crate; never push.
- Read docs/reviews/GATE-FINDINGS-LATEST.md at priority (a) — a BLOCK naming
  track E preempts your queue.
