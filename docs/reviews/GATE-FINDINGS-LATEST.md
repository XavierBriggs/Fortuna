# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-11 ~21:40Z, covering range 75f4782..2e54c18.
Verdict: audit-tail-fix-gate-2026-06-11.md (ACCEPT-WITH-GAPS — first
non-BLOCK after four consecutive BLOCKs; zero Major).

Cleared: the audit-tail cursorless Major (verified by HTTP reproduction —
newest page on empty cursor, lossless 251/251 forward walk, live pickup of
post-poll inserts); slice 4 recorder scan clean on all five criteria
(metadata-only, bounded flat scan 10k files in 80ms, no client-controlled
paths, degraded-not-500); sqlx compile-time exception properly ledgered
with live test coverage. Battery green (716/0/0; DST at task-review
default 2000 — read-path-only range; the 10000 bar stays for
money-path/phase gates).

Update 2026-06-11 ~23:20Z (cheap-tier gate, range 2e54c18..7e35f51,
verified directly by the verification session — diffs read, targeted
tests run): items 1 and 2 below are CLEARED. (1) scan_recorder now maps
parse failure to unhealthy + null age (`.unwrap_or(0)` -> `.ok()`, None
flows through; comment cites the finding; new test green — 11/11 rota).
(2) /favicon.ico serves 204 through the live merged router, test-pinned
(dashboard.rs:160). clippy -p fortuna-ops clean. Remaining items renumber
below.

Fix list (all Minor):

3. [Carried] DailyScheduler restart-fire + cumulative-vs-daily labeling +
   missing drive()-level digest assertion — fix or ledger.
4. [Carried] GAPS.md:142 "No ASSUMPTIONS exception is needed" contradicts
   ASSUMPTIONS.md:1220 (the SystemTime exception entry exists) — reconcile
   the two lines.

Remaining for T4.3 (per the honest open list): money/cognition views, the
R5 dedicated pool, the instrument presentation layer (panels still raw
JSON), favicon/logo assets. Remaining for T4.1: the synthesis-in-main
edge-sourcing design question (genuinely blocked, ledgered).

Operator actions still queued (unchanged): ROTATE both Kalshi keys;
FINALIZE the purge before any first push.
