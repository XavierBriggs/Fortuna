# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-11 ~14:40Z, covering range 1b3fabb..817d2e7.
Verdict: t41-daemon-gate-2026-06-11.md (BLOCK).
Cleared this round: the audit-death Major (FIXED — probe-verified at 7/7
mid-Phase-A fail points: 0 submissions, halt set, reservations released);
daemon boots against real Postgres from the committed example config;
real-SIGTERM wiring works (verifier-probed); Kalshi refusal, GET-only,
fail-closed config all pinned; 691/0/0 tests; DST 10000x3 clean.

Fix list, priority order:

1. [MAJOR — PROCESS, SECOND CONSECUTIVE GATE] CLIPPY RED AT HEAD AGAIN:
   daemon.rs:191 drive() has 8/7 args (introduced 77588c5); commits
   77588c5 and 817d2e7 shipped without the DoD battery — the exact defect
   the previous gate's item 2 flagged. Fix the lint, then re-read the
   amended loop doc: THE BATTERY IS A COMMIT-GATE. A red battery means
   the commit does not happen this iteration.
2. [MAJOR — reproduced] Durable-halt re-audit flood: PgHaltPoller +
   run_loop.rs:106-110 re-apply and RE-AUDIT a standing halt on every
   poll (reproduced 20/20 polls; ~172,800 audit rows/day at the 500ms
   cadence for ONE persistent halt — audit-table bloat + replay noise).
   Fix: apply/audit once per halt IDENTITY (dedup on halt id/reason
   fingerprint; re-audit only on change), with a test asserting one
   standing halt over N polls yields exactly one application audit row.
3. [MAJOR — contract] The BINDING SHUTDOWN CONTRACT assertion is STILL
   unmet: the SignalKind::terminate handler exists (main.rs:96-110) and
   routes to shutdown(), but no COMMITTED test delivers an actual SIGTERM,
   and the smoke has no working orders at stop — so "SIGTERM => cancel
   working orders + final audit row" is not asserted. Land a thin-book
   smoke variant with working orders at stop + a signal-delivery test (or
   ledger a rationale for why signal delivery is untestable in-repo).
4. [Minor] The poll-failure alert (a T4.1 requirement) is unimplemented —
   Err arm is counted only and DegradeScrape never sees it; the comment at
   run_loop.rs:5 overstates. Implement or ledger; fix the comment.
5. [Minor] shutdown.rs comment now stale in the OPPOSITE direction (claims
   the handler "does not exist yet"; it does). Correct it.
6. [Minor] A1's committed regression test omits the reservation-release
   assertion (the probe verified it; the test should too); the DST-arm
   decision for audit-death-mid-staging is unledgered.
7. [Minor] Belief drain: non-ULID belief ids, duplicate detection by
   error-string sniffing, no audit row on drain, not yet called from main
   — tidy or ledger each.
8. [Note] Cognition compose bindings (degrade_alerts consumer +
   calibration_for_scope) have tested code but are NOT yet bound into the
   booted daemon — honestly ledgered; binding them is part of finishing
   T4.1 (box correctly unticked).

Operator actions still queued (unchanged): ROTATE both Kalshi keys;
FINALIZE the purge before any first push.
