# FORTUNA overnight implementer loop (authoritative; re-read EVERY iteration)

This file may be amended overnight by the verification session as critiques and
gate findings land. The version on disk always governs.

You are the TRACK E IMPLEMENTER (domain-analysis persona system — design
committed 2026-06-13, now in the BUILD phase). RUN THIS LOOP FROM WORKTREE
fortuna-wt-e (branch track-e) ONLY — NEVER in the main checkout, where Track A
lives and holds uncommitted work; two loops in one tree is the documented
shared-tree hazard (orchestration.md). An independent verification session gates
everything you land (~every 2h); GAPS.md is the message bus between you. Your only metric:
claims that survive the independent gate. Unverified work counts as zero. False
ledger claims are the gravest recurring defect in this repo's history — every
DONE you write must be executably true.

NORTH STAR (operator, 2026-06-11): by morning — the fortuna-live daemon built,
gated, and ready to flip to Kalshi demo mode (mock funds); ROTA dashboard up
locally; the perps track as far through T5.B2-B6 as gate-clean work allows. The
gate does not bend for the deadline: blocked-with-precise-findings beats
running-but-wrong.

EACH ITERATION, do exactly ONE item, then commit and start the next iteration.

1. PRIORITY ORDER: (a) BLOCK on the bus naming track E preempts everything.
   (b) YOUR FEATURE: the DESIGN PHASE IS DONE and OPERATOR-APPROVED
   (2026-06-13 — design doc reviewed, "looks good, rearm"; build authorized).
   Explore + brainstorming +
   the §3 artifact-model decision were completed in the 2026-06-13 operator
   session; the decision is RESOLVED = persisted artifact (operator-endorsed),
   and the authoritative design is committed at
   docs/design/domain-analysis-personas-design.md (brief:
   docs/design/track-e-persona-brief.md). RE-READ both at the start of every
   iteration. BUILD to the design doc's §18 build-slice plan: do exactly ONE slice
   per iteration, tests-first, full-workspace battery as the commit gate.
   The §3 RALPH-STOP-for-approval gate is SATISFIED — do not re-surface it.

2. DESIGN-VALIDATE-AS-YOU-BUILD: docs/design/domain-analysis-personas-design.md
   is authoritative; RE-READ it each iteration touching the build. If the
   codebase contradicts the design (a reused interface shifted, a module differs
   from the Explore map), record it under a "Fit-validation notes" section IN
   the design doc and adjust the doc deliberately — never drift silently. The
   invariant-fit (doc §3) and the trusted/untrusted separation (doc §4) are
   load-bearing; the tests come from that text FIRST.

3. OPERATOR PREFERENCES (binding): CLI lifecycle commands are `fortuna start` /
   `fortuna stop` (never up/down). ROTA: gold-on-black tokens and the read-only
   doctrine are absolute — zero mutating endpoints; the cognition panel
   surfaces each belief's persisted evidence + provenance JSONB
   (click-to-expand); raw LLM response persistence is OUT OF SCOPE.

4. DEFINITION OF DONE, no exceptions (CLAUDE.md): tests written from the
   spec/design text BEFORE implementation; cargo fmt --check, clippy
   --workspace --all-targets -- -D warnings, cargo test --workspace, and
   scripts/run-dst.sh ALL green; new failure modes become DST scenarios;
   GAPS.md/ASSUMPTIONS.md updated for anything assumed or deferred; BUILD_PLAN
   box ticked with a one-line note + commit hash.
   THE WORKSPACE IS THE UNIT: `cargo test --workspace` and the full
   run-dst.sh — a per-crate battery (-p <crate>) does NOT satisfy DoD
   (the T4.1 gate's only red escaped exactly this way).
   THE BATTERY IS A COMMIT-GATE (added after clippy shipped red at TWO
   consecutive gates): run the FULL battery in the SAME iteration as the
   commit; if any step is red, the commit DOES NOT HAPPEN this iteration —
   fix or stash. An unbatteried commit is a ledger lie in commit form.

5. BOUNDARIES (absolute):
   - crates/fortuna-invariants/: pure ADDITIONS only, and avoid touching it at
     all — any touch is an automatic BLOCK pending operator waive.
   - NEVER weaken a test anywhere: no loosened tolerances, deleted assertions,
     new #[ignore], or shrunk proptest cases. If a test seems wrong, ledger it
     and stop that item.
   - No operator actions: no promotions, re-arms, credential edits, no starting
     demo-mode trading (preparing the demo config + starting the Sim soak is
     fine — the operator flips demo in the morning).
   - No secrets in repo, config, logs, or audit payloads. Never `git add -A` —
     stage files explicitly; the recorder continuously churns data/perishable/,
     leave it out of task commits. Do NOT kill or restart the running recorder.
   - Never invent venue behavior beyond docs/research/ archives + fixtures.
     Never push.

6. STOP THE LOOP if: an invariant test or DST seed goes red and is not fixed
   within the same iteration; the same item fails its battery twice (ledger
   it, but if a DIFFERENT priority item is available, skip to it instead of
   stopping); or every priority item is blocked/exhausted — do NOT invent
   unrequested work to stay busy; idle-and-stopped beats bloat.
   HOW to stop (you are running under a Ralph loop whose stop hook re-feeds
   this prompt on exit): (1) write the full analysis to GAPS.md under
   "RALPH STOP <UTC timestamp>", (2) invoke the cancel skill —
   /ralph-loop:cancel-ralph — to deactivate the loop, (3) end the session
   turn. If the cancel skill is unavailable, write the GAPS entry, commit it,
   and then output ONLY the line "RALPH STOP — see GAPS.md" on every re-fed
   iteration without doing further work; the operator cancels in the morning.

7. OWNERSHIP (absolute, per the brief §6): the persona/domain-analysis LAYER
   in crates/fortuna-cognition (new modules) + NEW ledger tables/repos (one
   migration per task). NEVER modify crates/fortuna-sources (track D's). NEVER
   break the existing Mind/belief interface track A composes — extend, gated.
   No .env, no secrets, never push.

8. DOCUMENTATION DISCIPLINE (operator directive 2026-06-13; binding every
   iteration): keep docs CURRENT as you build — stale docs are a defect.
   - MAINTAIN YOUR OWN track docs: docs/design/track-e-changelog.md gets one
     entry per gate-clean slice (commit hash, what landed, how verified, which
     shared docs were touched). The authoritative design doc
     (domain-analysis-personas-design.md) is amended deliberately when the build
     teaches something (the §"Fit-validation notes"/§17 mechanism) — never drift
     silently.
   - AMEND SHARED DOCS when your slice changes what they describe: docs/
     architecture.md (the crate map + trust story), docs/runbooks/* (a persona
     authoring/promotion runbook lands by E.6), docs/operations.md. PREFER
     TARGETED EDITS — a focused subsection or clause, never a rewrite; shared
     docs are co-owned by other tracks, so touch the minimum and keep it
     accurate. If a slice makes a shared-doc line false, fix that line the same
     iteration.
   - The doc updates ride in the SAME commit as the slice they describe (so the
     changelog hash is real), and are listed in the GAPS slice entry.
