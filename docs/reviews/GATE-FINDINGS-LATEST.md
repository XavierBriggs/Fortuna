# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-11 ~12:10Z, covering range 16478bb..1b3fabb.
Verdict: t41-increments-gate-2026-06-11.md (BLOCK).
Prior round's 4 items: ALL FIXED and re-verified (the second-gate ledger
Major is closed with the corrected-visibly pattern — good).

Battery at 1b3fabb: 681/0/0 workspace, DST 10000x3 clean (all 11 arms),
invariants green, protected crate untouched, fmt clean — but CLIPPY IS RED
(see item 2). Fix list, priority order:

1. [MAJOR — money path, REPRODUCED] Audit-death fall-through in the
   concurrent group path: an audit append failure landing mid-Phase-A
   (after legs passed gates) still submits the WHOLE staged group in
   Phase B — probe reproduced `fail_at=21 orders_submitted=3 halted=true`:
   three orders placed at the venue AFTER the audit store died, their
   gate-decision + subsequent audit rows lost. Violates spec line 367
   ("no audit, no trading") / I5 replayability in that window. FIX: on
   audit failure during Phase A, abort the entire staged group and release
   reservations — symmetric with the existing `group_rejected` path in
   crates/fortuna-runner/src/runner.rs; then add the probe as a regression
   test (the gate's probe source was at /tmp/i5probe — reconstruct from
   the verdict file's description) and a DST arm if expressible.
2. [Major-as-DoD] CLIPPY RED AT HEAD: `cargo clippy --workspace
   --all-targets -- -D warnings` fails — fortuna-live test target
   "shutdown" has 2 compile errors under clippy. The last commit shipped
   without the full DoD battery. Fix the test target; re-run the full
   battery before the next tick.
3. [Minor] crates/fortuna-live/tests/shutdown.rs:6-7 carries a
   present-tense comment claiming a SIGTERM handler exists — it does not
   yet. Correct the comment (false present-tense doc claims are the
   repo's recurring defect — do not let them seed).
4. [Pending, honestly unticked] The BINDING T4.1 SHUTDOWN CONTRACT is
   still unmet: no SignalKind::terminate handler exists anywhere. The
   shutdown() core is test-asserted (cancel + exactly-one daemon_shutdown
   audit row, idempotent) — good — but the contract requires the SIGTERM
   handler wired to it plus the assertion. `fortuna stop` (T4.4) remains
   blocked on this.

Cleared this round: PgAuditSink bridge (fail-synchronous, no loss window
on the healthy path, halt-on-append-death test-proven, core stays
Pg-free); journal-generic runner (exec state machine byte-untouched,
byte-identical Pg crash-recovery fold); all four prior ledger items.

Operator actions still queued (unchanged): ROTATE both Kalshi keys (prod
key = kill-switch credential too) + new PEMs at .env paths; FINALIZE the
purge (refs/original drop + reflog expire + gc) BEFORE any first push.
