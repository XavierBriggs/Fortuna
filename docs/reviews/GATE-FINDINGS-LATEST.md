# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-12 ~00:50Z, covering range 7e35f51..468c8c1.
Verdict: r5-pool-gate-2026-06-11.md (ACCEPT-WITH-GAPS — second consecutive
non-BLOCK; zero Major).

Cleared: both carried Minors (scheduler/digest honesty with committed
drive()-level assertion; dead-man ledger contradiction reconciled — zero
raw SystemTime in fortuna-live src). Slice 5 R5 pool VERIFIED on the
property that matters: distinct reader/writer pools (construction sites
main.rs:64 vs :93), reader capped at max_connections(2)/acquire 3s/
statement_timeout 3000 — and the isolation proven empirically: a SATURATED
reader pool degrades to HTTP 200 in ~3s while a concurrent audit INSERT on
the writer pool completes in <900ms. Audit tail end-to-end through the
real pool: cursorless-latest, pagination, past-end, empty-DB, pool-absent
all green. Money design-block verified REAL and exact (inspect_totals
returns (Cents, Cents, usize, usize) — no floating component; the mark
loop is the missing source, as the design critique predicted). Battery:
720/0/0, DST 2000-tier (read-path range), invariants green, protected
crate untouched.

Fix list:

1. [Minor — only item] The R5 saturation/isolation property has NO
   committed test: the verifier proved it by scratch reproduction, but
   nothing in the repo pins reader-pool-saturation => degraded-200 while
   writer-pool-append proceeds unimpeded. Commit the handler-level
   version of that test (the verdict file describes the shape) and note
   it in GAPS. Until pinned, a future refactor can silently merge the
   pools back.

Remaining for T4.3: money view (design-blocked, ledgered — resolve the
floating/mark-loop sourcing question or ship the view with the SIM-ONLY
subset per R6), cognition view (R7's two ledger queries), instrument
presentation layer (panels still raw JSON), logo asset (favicon 204 stub
satisfies the console-error criterion; the logo itself is Section 2).
Remaining for T4.1: synthesis-in-main edge sourcing (design-blocked,
ledgered).

Operator actions still queued (unchanged): ROTATE both Kalshi keys;
FINALIZE the purge before any first push.
