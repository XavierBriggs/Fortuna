# GATE FINDINGS — latest (verifier-owned)

## TRACK C — URGENT BRANCH REPAIR (priority (a); 2026-06-13)

Your rebase onto main SILENTLY DROPPED the entire perps tranche: the
content was merged (a586b4a) then REVERTED (19b3888) on main, so rebase
treated every patch as already-applied and finished at main's head with
zero commits. Nothing is lost — your true tip is in the reflog.

REPAIR (run in YOUR worktree, verify clean tree first):
  git status --short        # must show no tracked changes
  git reset --hard 3c8b126  # your true tip (incl. the red-seed BLOCK fix)
  KEEP the untracked corpus seed file and commit it with your next item.

RULE CHANGE (your loop file is amended): do NOT plain-rebase onto main
while the revert (19b3888) is in main's history — use
`git rebase --reapply-cherry-picks main` or skip rebasing entirely until
the re-merge. THE RE-MERGE MECHANICS (verifier-owned, when your package
is ready): main reverts the revert, THEN merges your tip, THEN the
post-merge battery incl. the kinetics client-id test.

YOUR PACKAGE (unchanged): 1) kinetics client-id test derives its
expectation through the derivation path (exec adjudication c25b368 on
main has the pinned mechanism — read it); 2) the 2x leverage cap
(operator-decisions-2026-06-12.md item 4: config + min(config, venue
curve) + boundary pin 2.01x-refused/1.99x-passes); 3) full re-gate at
10000 -> re-merge request.

# GATE FINDINGS — latest (verifier-owned)

## SOAK: GO (soak-go-gate-2026-06-12.md — ACCEPT, the first unconditional)

Daemon at 8ea8a4d is FIT TO START the 7-day Phase-4 EXIT soak. Battery
803/0/0, DST 10000x3 + anchors, mutations all red, exec idempotency
pinned (derivation-path assertion, proven both ways), reviews reuse T3.1
machinery verbatim, protected crate untouched. THE START IS THE
OPERATOR'S: build release, provision .env, run
./target/release/fortuna-live config/fortuna.toml (full contract in the
verdict). Soak-watch arms at my first post-start firing (ten metrics
enumerated in the verdict).

FOR TRACK A (holding — this re-points you, in order):
1. Commit the OPERATOR RUNBOOK (gate Minor 2: the start contract exists
   only by reconstruction — make it a doc; include the rearm-requires-
   restart fact).
2. M3 rearm notices (CLI output + ROTA surface) — BEFORE the operator's
   first soak halt drill.
3. Annotate the two stale M2-disclosure sites (GAPS.md:83, BUILD_PLAN
   T4.1 note) as RESOLVED-visibly.
4. Then T4.2 (your queue resumes).

FOR THE POOL / restarted TRACK C: the perps re-merge package — fix the
kinetics client-id test per the exec adjudication (derive the expectation
through the derivation path, not a pinned UUID), build the 2x leverage
cap (operator-confirmed; config + min(config, venue curve) + boundary
pin), then full re-gate => re-merge under the standing signatures.

# GATE FINDINGS — latest (verifier-owned)

NEW BUILD ITEM (operator-confirmed 2x leverage cap): see operator-decisions-2026-06-12.md item 4 — [perp] max_leverage config + gate enforcement min(config, venue curve) + boundary-pinned test. Rides with the perps re-merge fix (same files, same track).

## PERPS MERGE REVERTED (post-merge interaction failure, 2026-06-12 ~21:10Z)

The signed merge (a586b4a) failed the post-merge integration check:
kinetics_adapter.rs:168 place_maps_gated_order_to_the_recorded_create_
request — the DETERMINISTIC client_order_id derived in the merged tree
differs from the id the same test derived on track-c's tree (left
f6384bf5... vs right c445aeac...). Track-c gated green pre-merge; main
moved (T4.1 synthesis batch); the COMBINATION shifts the id derivation.

DIAGNOSIS NEEDED (pool; exec id-derivation is track-A domain, the test is
track-C domain): deterministic client ids must be STABLE under unrelated
code addition — find what the derivation salts on that changed in the
merged tree (enum ordering? seed-stream consumption? shared id-space
counter?). EITHER make the derivation context-free for a given (intent,
market, attempt) OR make the test derive its expectation through the same
path rather than pinning a tree-state-dependent UUID. LOAD-BEARING
QUESTION to adjudicate explicitly: if ids are not stable across builds,
crash-resubmission idempotency (AlreadyExists dedup) may be silently
broken across daemon restarts after upgrades.

Operator signatures (batch 5 + F1) REMAIN VALID; the merge re-runs after
the fix + re-gate. Main is GREEN again post-revert.

# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-12 ~10:30Z. Multi-track pipeline state.

## TRACK C — cumulative perps gate VERDICT (track-c-perp-gates-gate-2026-06-12.md)

ACCEPT-WITH-GAPS, zero Critical, MERGE CONDITIONAL ON TWO OPERATOR
DECISIONS (parked below). Cleared: B2 types integer end-to-end; B3 gate arm
fail-closed on every error arm, liquidation-loss worst case, pipeline
provably additive (checks 1-10 zero hunks); MarginSim byte-deterministic;
protected crate PURE ADDITIONS (628/0, full diff in the verdict); battery
852/0/0, DST 10000x4 clean.

TRACK C FIX LIST (Minor, ledger each):
1. [F2] At-boundary equality unpinned for liquidation floor + leverage cap
   (mutations M1/M3 survived — add exactly-at-boundary tests; the
   enforcement itself is mutation-proven one tick beyond).
2. [F3] The funding ±2% cap (research §4) is unenforced in MarginSim —
   enforce or ledger why simulation should not clamp.
3. [F5] perp.rs:729-730 (see verdict) minor cleanup.
4. [F1 follow-on, BINDING for B4]: build the leverage_estimates -> RiskCurve
   converter reading the RECORDED fixtures + a shape test against
   fixtures/kinetics-perps/markets__single.json; correct the T5.B5 tick
   wording (claims "recorded risk curves" — currently false; correct
   visibly, never erase).

## T4.1 COMPLETION GATE: CLEARED to ACCEPT-WITH-GAPS (M1 fixed, addendum in verdict; soak fit pending operator M2) — was: BLOCK (t41-completion-gate-2026-06-12.md) — one line from soak-ready

The daemon is mechanically SOUND (full synthesis chain proven in the booted
composition; E1 fail-closed THROUGH the composition; budgets bind w/
degrade audit rows; no silent unhalt; DST 10000 all stages clean; sweeps
clean; protected crate untouched). The BLOCK:

FOR TRACK A (priority (a), fix in ONE iteration):
1. [M1, the red] config/fortuna.example.toml gained synthesis entries
   (304f746) but the example-config pin (fortuna-ops/tests/config.rs:84)
   was never updated — ONE-LINE pin fix, then the FULL WORKSPACE battery +
   DST green. ROOT CAUSE: per-crate batteries (-p fortuna-live) were run
   instead of `cargo test --workspace`. THE COMMIT-GATE MEANS THE WORKSPACE
   BATTERY — a per-crate battery does not satisfy DoD, ever. (Loop rule 4
   now says so explicitly.)
2. [m1-m4 Minors] R4 categories-allowlist stale deferral; committed
   integration test for R2 refresh-failure (gate proved it by mutation —
   pin it); + two in the verdict file.

FOR THE POOL (track-B-owned files; track B stopped):
3. [M3] Rearm option-(a) notices landed 1 of 3 places — ASSUMPTIONS yes;
   the CLI rearm output ("re-armed {scope}" must say "pending restart")
   and a ROTA health-surface notice are MISSING. Land both before the
   first soak halt drill or the operator WILL be confused mid-drill.

OPERATOR (added to your queue):
4. [M2] The T4.1 tick stands with two disclosed-but-unbuilt items (daily
   reconciliation re-run + weekly/monthly reviews -> digest). Decide:
   waive with explicit BUILD_PLAN sub-checkboxes, or un-tick until built.
5. Runbook line: the fortuna_app Pg role cannot CREATE DATABASE, which
   fails every sqlx::test under the operator .env — document or grant.
6. DISK IS URGENT AGAIN (~10GB): machine-wide space + the 35GB cargo-clean
   window.

VERIFIER SELF-FINDING (owned): commit 8b8b222 bundled a 111-file kinetics
fixture re-record beyond its message (the funding-tick recorder refreshes
swept in via `git add fixtures/`). Inert content, dishonest commit shape —
the verifier's own claim-vs-reality slip, logged here for symmetry.

SOAK: NOT started (correctly — no process, operator-start documented).
Starts after M1 clears + your M2 decision.

## OPERATOR DECISIONS PARKED (morning queue)

1. PROTECTED-CRATE WAIVE batch 5: track-C invariant additions (perp I1
   seal, I2 extension, I3 cross-domain halt) — verified pure additions,
   628 insertions / 0 deletions, full diff quoted in the verdict file.
2. F1 DISPOSITION: verifier recommends waive + tick-wording correction +
   the B4 contract item above (the sim engine is sound; the tick
   overstated its data source).
3. LEVERAGE CAP NUMBER: the Phase-A note says ~5.9x -> 2x intent, but spec
   5.15 says "config-set at or below venue maxima" and NO 2x exists in any
   committed config. If 2x is the intended conservative posture, say so —
   it becomes a [perp] config entry + a pinned test. (Also: this gate's
   rubric wrongly asserted "spec says 2x" — verifier-session error,
   corrected here.)
4. Standing: keys rotation; purge finalization; machine disk (~16-33GB
   free, fluctuating); 35GB main-target cargo clean window.

## R12 BROWSER PASS: PASSED (2026-06-12 ~10:25Z) — T4.3 final condition MET

Boot at merged main; ZERO console errors; all panels render the real
presentation layer (wheel logo, instrument panels, honest nulls for
floating/total, the amber "no cognition strategy composed yet" honest
state); halt drill -> full SYSTEM HALTED takeover verified; clean
SIGTERM shutdown (92 ticks, exactly-one shutdown row); screenshots in
docs/reviews/rota-visual/rota-r12-*. T4.3's track-B tick is now FULLY
accepted. Track-B merge complete; post-merge battery green INCLUDING the
first-ever corpus replay (3 anchor seeds).

## NEW FINDING from the R12 drill — for TRACK A (fortuna-live owner):

[Needs-adjudication] The halt poller APPLIES external halts (<=500ms) but
appears to NEVER CLEAR on rearm: a halt_events kind='rearm' row left the
daemon halted for minutes until a restart (whose boot fold DID read
set->rearm correctly — verified live). Either (a) deliberate-conservative
(rearm requires restart) — then DOCUMENT it in the CLI rearm output, ROTA
health panel, and ASSUMPTIONS, or (b) the poller should clear on rearm —
then implement + test (one standing rearm over N polls => unhalt exactly
once, audit row). Adjudicate against I2's wording (re-arm is the HUMAN
act; the poller reflecting it is not "automatic resumption").

## TRACK-B ORPHANED MINORS (track stopped; revert to pool — track A may take):
F-1: fortuna status missing the audit-age line (fortuna-cli.md:57, the
crash-tell) — implement or ledger. F-2: A2 spawn-cwd pinning (cwd-relative
out-dir edge). Plus: one manual §13 runbook execution before
managed-lifecycle adoption (operator).

## TRACK-C RE-GATE: ACCEPT-WITH-GAPS (track-c-regate-2026-06-12.md) — BLOCK CLEARED

Bidirectional proof: pre-fix harness reproduced the exact red seed at the
gate master (exit 101); post-fix green byte-identically; full battery
940/0/0 + run-dst.sh 10000 exit 0; protected crate untouched since the
parked waive. THE PERPS MERGE IS NOW READY AND WAITS ONLY ON THE TWO
OPERATOR SIGNATURES (waive batch 5 + F1 disposition).

TRACK-C LEDGER ITEMS (3 Minor, ride with B7): (1) document the corpus-
placement deviation (in-harness REGRESSION_SEEDS instead of dst-corpus/ —
functionally superior, unledgered); (2) tighten the designed-arm pin to
match the curve-exceeded reason string specifically; (3) the latent
same-class false-red at the invariant-3 instant-liquidation call site
(fail-noisy not fail-open; unhit in 20k scenarios) — fix or ledger.

## [historical] TRACK-C FINAL GATE: BLOCK — first true red DST seed (track-c-final-gate-2026-06-12.md)

Seed 11819682492387934495 (master 1781262032255, PERP_DST_SCENARIOS=10000):
wild-regime mark drift pushes notional past the harness's last risk-curve
tier ($100k); MarginSim FAIL-CLOSES (correct production behavior) but the
B6 harness's invariant-1 counts the designed refusal as failure => battery
exit 101, byte-identical on re-run. Everything else PASSED: all 4 prior
Minors fixed (surviving mutations now die), adapter fully fixture-traced
(8/8 behaviors), B4 tick honest, protected crate unchanged since the
parked waive. NEITHER parked operator decision changes.

FIX (track-c-owned files; track C is STOPPED — ownership released to the
pool, but this does NOT preempt track A's T4.1/soak critical path; next
available capacity or an operator-restarted track C takes it):
1. Harness-local: extend curve coverage past the last tier OR add a
   designed-refusal arm that asserts the spec-5.15 fail-closed behavior as
   SUCCESS (+ halt accounting if the spec demands one there).
2. Commit seed 11819682492387934495 to crates/fortuna-core/dst-corpus/
   with a comment naming the failure mode (the doctrine's first true red
   seed — never delete).
3. Re-run scripts/run-dst.sh at 10000 green end-to-end; then the track-c
   merge proceeds (after the operator waive batch 5 + F1 disposition).
4. [Minor, ride-along] annotate the stale "from recorded risk curves"
   wording in the GAPS B5 done-entry (verdict Finding 4).

## PIPELINE

Track B: final cumulative gate RUNNING (CLI slices 2+3 + ROTA cognition/
presentation/logo + both ticks); R12 browser pass follows its verdict,
run by the orchestrator. Track B session STOPPED itself clean (queue
exhausted) — protocol-conformant.
Track C increments queued: B6 DST arms + B4 slice 1 (gate after track-B).
Main: synthesis-in-main batch accumulating (S1, S2, S3a landed; gates at
the T4.1 tick or coherent slice); determinism anchors landed (corpus no
longer empty — closes the standing Info finding).
