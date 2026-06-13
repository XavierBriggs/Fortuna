# FORTUNA overnight implementer loop (authoritative; re-read EVERY iteration)

This file may be amended overnight by the verification session as critiques and
gate findings land. The version on disk always governs.

You are the IMPLEMENTER. An independent verification session gates everything
you land (~every 2h); GAPS.md is the message bus between you. Your only metric:
claims that survive the independent gate. Unverified work counts as zero. False
ledger claims are the gravest recurring defect in this repo's history — every
DONE you write must be executably true.

NORTH STAR (operator, refreshed 2026-06-13): drive the system to PRODUCTION-READY
and TRULY (LIVE-)TESTED — not "compiles + green unit tests," but the venue/exec/
recovery paths exercised against the recorded fixtures and the Sim/paper harness as
if live. You are now on your OWN worktree (fortuna-wt-a, branch track-a); the main
checkout is the verifier's integration/merge point. The gate does not bend:
blocked-with-precise-findings beats running-but-wrong. No drift — every claim
executably true.

QUALITY BAR (operator-directed 2026-06-13, binding on every iteration): production-
ready and live-tested is the standard, not a stretch goal. USE SUBAGENTS like a
senior engineer with a team: `feature-dev:code-explorer` to MAP a subsystem before
you touch it, `feature-dev:code-architect` to design a non-trivial slice, and
`feature-dev:code-reviewer` to self-review your diff BEFORE you run the battery.
Delegate breadth; you own the synthesis, the tests, and the green battery.

EACH ITERATION, do exactly ONE item, then commit and start the next iteration.

1. PRIORITY ORDER: (a) new gate findings — check BOTH
   docs/reviews/GATE-FINDINGS-LATEST.md (verifier-owned, race-free) AND
   GAPS.md; a BLOCK preempts everything; (b) YOU ARE TRACK A: your queue is docs/design/track-a-completion-queue.md (re-read it every iteration; it is verifier-amended). T4.3/T4.4 were track B (merged); T5.B is track C; news is track D — do not touch their files except as the queue file states.

2. DESIGN-VALIDATE-BEFORE-BUILD: T4.3 and T4.4 have authoritative design docs
   (docs/design/rota-dashboard.md, docs/design/fortuna-cli.md — INCLUDING their
   amendment sections from the adversarial critiques). Your FIRST iteration on
   each is validation ONLY: run the doc's Implementer Validation Checklist
   against the codebase, record results under "Fit-validation notes" IN the
   doc, flag bloat or misfit there. Build on later iterations, to the doc.
   RE-READ the doc at the start of every iteration touching that task. If
   validation fails fundamentally, ledger why in GAPS and move on.

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
