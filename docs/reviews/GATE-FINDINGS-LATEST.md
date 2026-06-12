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

## TRACK-C FINAL GATE: BLOCK — first true red DST seed (track-c-final-gate-2026-06-12.md)

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
