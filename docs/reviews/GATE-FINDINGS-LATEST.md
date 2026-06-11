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

Fix list (all Minor):

1. [NEW, reproduced] scan_recorder fakes freshness on malformed
   generated_at: `parse_iso8601(...).unwrap_or(0)` then `.max(0)` clamps
   age to 0 => healthy:true on garbage input. Daemon-controlled input so
   Minor, but it violates degraded-never-faked: flip to unhealthy/absent
   on parse failure (one line) + test.
2. [Carried] /favicon.ico still 404s (your own GAPS entry agrees) — asset
   route or 204 stub; becomes an R12 pass/fail item at the T4.3 gate.
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
