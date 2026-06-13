# Multi-implementer orchestration (operator-directed 2026-06-12: "more fable implementers, code complete")

Three implementer tracks, one verifier. The quality bar is unchanged — every
track ships under the same loop rules (design-validate-before-build, battery
as commit-gate, DoD, boundaries, STOP conditions) and every batch passes the
independent gate. Parallelism comes from file-ownership partitioning, not
from relaxing anything.

## Tracks and ownership

TRACK A — the original session, MAIN checkout (/Users/xavierbriggs/fortuna):
  Queue: T4.1 tail (synthesis-in-main per docs/design/
  synthesis-edge-source-decision.md -> mech_extremes+veto -> mind binding ->
  rich digest) -> T4.2 post-fixture tranche -> T4.5.
  Owns: crates/fortuna-live, crates/fortuna-runner, crates/fortuna-venues/
  src/kalshi*, crates/fortuna-paper (T4.2 replay seam).

TRACK B — new session, worktree /Users/xavierbriggs/fortuna-wt-b (branch track-b):
  Queue: T4.4 CLI -> T4.3 cognition view + R7 ledger queries + the
  instrument presentation layer + logo asset.
  Owns: crates/fortuna-cli, crates/fortuna-ops (rota + shell), assets/rota/,
  the two R7 query additions in crates/fortuna-ledger/src/repos.rs.

TRACK C — new session, worktree /Users/xavierbriggs/fortuna-wt-c (branch track-c):
  Queue: T5.B2 perp core types -> T5.B3 gate extensions -> T5.B5 paper
  margin -> T5.B6 DST arms -> T5.B4 adapter (fixtures/kinetics-perps/ is
  recorded; kinetics/ dir is disjoint from track A's kalshi/).
  Owns: perp types module in fortuna-core, crates/fortuna-gates perp
  extensions, crates/fortuna-state margin pieces, crates/fortuna-venues/src/
  kinetics*, perp arms in the DST harnesses.
  PROTECTED-CRATE NOTE: T5.B3 adds I2-extension invariant tests — ADDITIONS
  ONLY; every touch still auto-flags for the operator waive queue (expected,
  batch them).

## Hard rules (all tracks)

- NEVER edit a file another track owns. If a task needs one, ledger it in
  your GAPS section and skip to your next queue item.
- Shared ledgers (BUILD_PLAN.md, GAPS.md, ASSUMPTIONS.md): append/edit ONLY
  within your own track's entries; tick ONLY your own boxes.
- The findings bus is read at the MAIN checkout absolute path:
  /Users/xavierbriggs/fortuna/docs/reviews/GATE-FINDINGS-LATEST.md
  (it is verifier-owned and lives on main; your worktree copy may be stale).
  A BLOCK naming your track preempts your queue.
- Tracks B/C: REBASE onto main at the start of every iteration
  (git fetch . main && git rebase main) — the verifier merges gated work
  into main between your iterations; rebase keeps you current and keeps
  merges trivial. Resolve only conflicts inside files you own; anything
  else, STOP and ledger.
- Worktrees have NO .env by design — tracks B/C need no venue credentials;
  the cargo [env] dev-DB default covers sqlx tests. Do not copy .env in.
- Each worktree builds in its own target dir automatically (cargo default
  per worktree). Batteries will contend for CPU across tracks — that is
  accepted; never skip the battery to dodge contention.

## Verifier protocol (the verification session executes this each firing)

1. Survey main + track-b + track-c heads since their last gated commits.
   (Trigger is EVENT-DRIVEN: a wake-on-commit monitor watches all three
   heads; the 2h cron is the fallback heartbeat only.)
2. Gate each track's new range (worktree-pinned, tier-appropriate battery).
3. Merge each ACCEPT/ACCEPT-WITH-GAPS track head into main; THEN run the
   POST-MERGE INTEGRATION CHECK on merged main — `cargo test --workspace`
   + clippy at minimum — before declaring main green: per-track gates test
   pre-merge heads, and only the merged combination tests the interaction.
   A red post-merge check reverts the merge and buses the conflict. (merge
   is promotion mechanics, not authorship; a BLOCK branch stays unmerged.)
4. One findings bus, per-track sections.
4b. DISK HYGIENE v2 (two disk-full incidents 2026-06-12; the second took
    the machine to literal zero bytes mid-gate): ALL gate builds share ONE
    target dir — every gate/battery command runs with
    CARGO_TARGET_DIR=/tmp/fortuna-gate-target — bounding total gate
    transients to a single warm cache regardless of worktree count (cargo's
    lock serializes concurrent gate builds; therefore GATES RUN ONE AT A
    TIME, queued, full-tier first). Gate worktrees stay persistent but
    carry NO per-worktree target. Verifier checks free space each firing:
    <10GB => bus alert + cargo clean the shared gate target (self-owned).
    The main checkout's 35GB target is cleaned only at operator-approved
    idle windows; the perishable dataset is NEVER touched.
5. MUTATION CHECKS are standard for any commit whose deliverable is a test:
   the gate stubs/mutates the tested surface and requires the test to go
   red (three vacuous tests were caught this way; green-only verification
   of tests is not verification). Mutations run in gate worktrees, never
   live trees.
6. Verification-infrastructure backlog (small, any idle capacity, track A
   domain): commit DST determinism-anchor seeds — the regression corpus
   has been empty since T0.4, so the corpus-replay arm is a no-op; even
   without red seeds, committing N high-activity seeds with their byte
   digests pins replay determinism across refactors.
7. SOAK WATCH (arms when T4.1 ticks): each firing during the Phase-4 EXIT
   soak additionally checks daemon uptime, halt count (must be zero
   unexplained), mind budget burn vs config, dead-man freshness, and
   belief-persistence growth — logged to docs/reviews/soak-log.md so the
   week of evidence EXISTS when the exit is graded, rather than being
   reconstructed.

## Session starts (operator pastes)

Track B session (cd /Users/xavierbriggs/fortuna-wt-b first):
  /ralph-loop Read docs/design/implementer-loop-track-b.md at the start of
  every iteration and follow it exactly.

Track C session (cd /Users/xavierbriggs/fortuna-wt-c first):
  /ralph-loop Read docs/design/implementer-loop-track-c.md at the start of
  every iteration and follow it exactly.

Track A continues unchanged (its loop file now carries the track-A
ownership note and the edge-source decision pointer).

## SHARED-TREE MERGE HAZARD (added 2026-06-13 after a live near-miss)

Track A's Ralph runs IN the main checkout (/Users/xavierbriggs/fortuna); the
verifier/orchestrator ALSO runs there. A `git merge` into main while track A
has uncommitted work is a hazard: it SUCCEEDED safely only because the merge
and track A's edit were on disjoint files (git refuses a merge that would
overwrite uncommitted tracked changes — so a collision fails loud, not
silent, but still interrupts the merge). RULES:
1. Before any orchestrator merge into main, `git status --short` — if a
   track-owned source file is modified (uncommitted), the merge MAY proceed
   ONLY if disjoint from the merge's file set; otherwise WAIT for the track's
   next commit (clean tree) or coordinate.
2. NEVER `git reset/checkout/stash` in the shared tree to clear a track's
   uncommitted work — that is the irreversible-destruction the classifier
   correctly blocks.
3. Post-merge integration checks run in a DETACHED worktree at the merge
   commit, never the shared tree — track A's own next commit-gate battery is
   the second integration layer.

## TRACK E — domain-analysis persona system (operator-directed 2026-06-13)
Worktree /Users/xavierbriggs/fortuna-wt-e (branch track-e). Loop:
docs/design/implementer-loop-track-e.md; brief: docs/design/track-e-persona-brief.md.
DESIGN-FIRST (operator-approval gate before any feature code). Owns: the
persona/domain-analysis LAYER in crates/fortuna-cognition (new modules) + new
ledger tables/repos. Consumes track D's signals; NEVER modifies fortuna-sources;
NEVER breaks the Mind/belief interface track A composes.
## TRACK C — RE-ARMED 2026-06-13 for T5.B7 (rung-0 perp strategies, fee-trap rule) + T5.B8 (perps ops); the perps PLANE is merged to main.
