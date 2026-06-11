# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-11 ~17:10Z, covering range 817d2e7..dfb849f.
Verdict: t41-remediation2-gate-2026-06-11.md (BLOCK — narrowing).
Cleared this round: CLIPPY GREEN at HEAD and at the first in-range commit
(the battery-commit-gate held — keep it); the BINDING SHUTDOWN CONTRACT is
MET (committed working-orders signal smoke: cancelled>=1 + journaled
cancels + exactly one daemon_shutdown row; real-OS delivery ledgered);
audit-death regression now asserts reservation release at every fail
point; degrade-alerts consumer wired into the booted daemon with env
fail-closed Slack config; dead-man wired with no real URL in tests.
Battery: 701/0/0, DST 10000x3 clean (all 11 arms), invariants green,
protected crate untouched.

Fix list, priority order:

1. [MAJOR — same defect, wrong scope, reproduced] Standing-halt dedup
   resets PER SEGMENT: `last_halt` is local to run_loop (run_loop.rs:94)
   and drive() re-enters run_loop every ~30s segment, so one standing halt
   re-applies + re-audits per segment (~2,880 rows/day; probe: 4
   applications over 4 segments for 1 halt). HOIST the dedup state to
   drive() scope (pass &mut Option<String> in, or a DriveState struct).
   The committed test MUST cross a segment boundary (the current one never
   does — that is exactly how the partial fix looked complete). Same
   hoisting for the poll-failure alert dedup (it floods per segment during
   a sustained store outage — same class).
2. [Minor cluster — claim-vs-reality, the recurring defect in new forms]
   (a) GAPS.md:107 still says dead-man "deliberately unwired" — stale at
   HEAD; (b) dfb849f's COMMIT MESSAGE claims a GAPS flip the commit does
   not contain — append a correcting note to GAPS (commit messages cannot
   be edited; the ledger can record the discrepancy); (c) deadman_tick doc
   claims "audits + Ops-alerts" but main's on_failure only eprintlns —
   implement a counter/alert or fix the doc; (d) run_loop.rs:9-11 comment
   still says "when the composition lands" — it landed.
3. [Minor] alert_routing tests named "..._and_audit" assert only mock
   posts — add the audit-row assertions their names claim.
4. [Minor] main.rs:131 raw SystemTime::now() outside Clock impls —
   CLAUDE.md calls this a defect even at the edge; route through RealClock
   or ledger the exception explicitly in ASSUMPTIONS.
5. [Minor] T4.1 open-set in GAPS is understated: add mech_extremes-with-
   veto strategy binding and mind_from_env/CostBudget binding to the open
   list (the box is correctly unticked — keep the list honest too).
   Also: `_send_failures` is discarded while GAPS claims "no remaining
   sliver" — count it or fix the claim.
6. [Note for T4.3] The ROTA deadman-age seam (`last_ping_at:
   Arc<AtomicI64>`) is absent and the closure-owned pinger conflicts with
   the ROTA design's expectation — reconcile when T4.3 wiring starts
   (either add the atomic seam or amend the ROTA doc's health contract).

Operator actions still queued (unchanged): ROTATE both Kalshi keys;
FINALIZE the purge before any first push.
