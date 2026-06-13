# FORTUNA overnight implementer loop (authoritative; re-read EVERY iteration)

This file may be amended overnight by the verification session as critiques and
gate findings land. The version on disk always governs.

You are the TRACK C IMPLEMENTER (multi-track orchestration: docs/design/orchestration.md GOVERNS ownership; read it once, re-read on conflicts). An independent verification session gates everything
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

1. PRIORITY ORDER: (a) new gate findings — read the bus at the MAIN
   checkout: /Users/xavierbriggs/fortuna/docs/reviews/GATE-FINDINGS-LATEST.md
   (your worktree copy may be stale); a BLOCK naming track C preempts
   everything; (a2) REBASE onto main before starting (git fetch . main && git rebase --reapply-cherry-picks main — NEVER a plain rebase while main carries the perps revert 19b3888: plain rebase drops your already-merged-then-reverted commits as duplicates); resolve conflicts only in files you own, else STOP+ledger; (b) YOUR QUEUE (re-armed 2026-06-13 — perps PLANE is MERGED to main; these
   are the two UNBUILT finish items that close the Phase-5 EXIT): T5.B7
   strategies rung 0 — perp_event_basis (Sim), funding_forecast (zero-capital
   scalar claims), funding_carry DATA-ONLY until >=60d regime evidence
   (amendment B); FEE-TRAP RULE (amendment C): edge floors at assumed
   post-promo fees 5-12bps, promo-$0 NEVER justifies GO; I7 unchanged. THEN
   T5.B8 ops — kill-switch perps FLATTEN (reduce_only IOC + cancel-all; the
   kill switch is a SEPARATE binary, I4 — extend IT, never couple your crates
   to it), margin/funding telemetry, the funding-regime ROTA panel (mounts in
   ROTA). Rebase: the perps plane is on main now, so a PLAIN `git rebase main`
   is safe again (the revert is behind the re-merge).

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

7. OWNERSHIP (absolute): you may modify ONLY: perp modules in fortuna-core, fortuna-gates perp extensions, fortuna-state margin pieces, crates/fortuna-venues/src/kinetics*, perp DST arms. Plus your own
   sections of BUILD_PLAN/GAPS/ASSUMPTIONS and your own boxes. Any other
   file => ledger + skip. No .env, no venue credentials, ever.

8. DOCUMENTATION (operator directive, 2026-06-13): keep the docs current as you
   build — stale docs are a defect.
   - MAINTAIN track C's OWN docs: docs/design/perp-strategies-and-scalar-claims.md
     is the authoritative design artifact (amend it as the design evolves). Per
     the doc-hygiene directive (bus 2026-06-13: NO per-track changelog FILES),
     log changes in the ROOT CHANGELOG.md — APPEND to track C's own
     `### Cognition belief-pipeline & perps (Track C)` subsection under
     `## [Unreleased]` (one concise bullet per landed slice; the verifier
     reconciles subsections on merge).
   - AMEND the SHARED docs when a slice changes their truth: docs/architecture.md
     (new crates/tables/seams/telemetry/ROTA contracts), docs/operations.md +
     docs/runbooks/ (new operational steps, e.g. the sqlx migrate/prepare flow,
     the new DB tables, perps telemetry/kill-switch). TARGETED edits ONLY —
     carefully thought out, never bloat, never duplicate the design doc into them
     (LINK to it instead), never let them drift.
   - Doc updates ride the SAME commit as the slice they describe (or a dedicated
     docs commit in the same iteration); the battery still gates the code.
