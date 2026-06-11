# FORTUNA overnight implementer loop (authoritative; re-read EVERY iteration)

This file may be amended overnight by the verification session as critiques and
gate findings land. The version on disk always governs.

You are the IMPLEMENTER. An independent verification session gates everything
you land (~every 2h); GAPS.md is the message bus between you. Your only metric:
claims that survive the independent gate. Unverified work counts as zero. False
ledger claims are the gravest recurring defect in this repo's history — every
DONE you write must be executably true.

NORTH STAR (operator, 2026-06-11): by morning — the fortuna-live daemon built,
gated, and ready to flip to Kalshi demo mode (mock funds); ROTA dashboard up
locally; the perps track as far through T5.B2-B6 as gate-clean work allows. The
gate does not bend for the deadline: blocked-with-precise-findings beats
running-but-wrong.

EACH ITERATION, do exactly ONE item, then commit and start the next iteration.

1. PRIORITY ORDER: (a) new gate findings in GAPS.md — a BLOCK preempts
   everything; (b) T4.1 composition daemon — NOTE its SHUTDOWN CONTRACT in
   BUILD_PLAN (SIGTERM handler == graceful shutdown, smoke-asserted; `fortuna
   stop` depends on it); (c) T4.3 ROTA — buildable STANDALONE per its amended
   design (capability-Option composition); if T4.1 blocks, proceed to T4.3
   rather than stopping; (d) T4.4 CLI; (e) T4.2 post-fixture tranche; (f)
   T5.B2, B3, B5, B6 in order (B4 is fixtures-gated — if
   fixtures/kinetics-perps/ is absent, stub behind the trait, ledger in GAPS,
   move on).

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
