# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-11 ~09:40Z, covering range b8fa0c8..16478bb.
Verdicts: overnight-code-gate-2026-06-11.md (ACCEPT-WITH-GAPS) and
overnight-incident-ledger-gate-2026-06-11.md (BLOCK).

Battery at 16478bb: 676/0/0 workspace, DST 10000x3 all stages clean (all 11
settlement arms hit), invariants green per-test, protected crate untouched,
spec v0.9 Section 3 SHA-verified byte-identical. Security-incident handling
graded CLEAN (purge effective on main; rotation + finalization correctly
operator-queued; fixtures secret-free; recorder redacts by construction).

The BLOCK is ledger accuracy ONLY. Fix list, priority order:

1. [MAJOR x2 — SECOND CONSECUTIVE GATE] GAPS.md "Minor engineering residue"
   CLOSED list still claims: "degrade_alerts/CalibrationParamsRepo
   not-yet-live overstatements corrected in ASSUMPTIONS; Polymarket source
   count corrected to 95 (erratum in the research doc)". BOTH remain FALSE
   at 16478bb: ASSUMPTIONS contains zero degrade_alerts mention; the
   Polymarket research doc contains no erratum and no "95". Fix by EITHER
   landing the two real corrections OR rewriting the sentence to what is
   true — corrected visibly, never erased. This claim already survived one
   dedicated remediation cycle.
2. [Minor] F5 of the B0/B1 self-gate remediation is unaccounted — record
   its disposition in GAPS.
3. [Minor] WS public `trade` frame was never observed in the 60-capture
   session (coverage: 18 covered / 6 partial / 3 missing — honestly
   ledgered). Add it explicitly to the fixture-session follow-up list; it
   gates the paper-engine trade-through replay.
4. [Minor, code gate] (a) recorder single-writer assumption undocumented
   (JSONL append corrupts under two writers; T4.4 start-refusal depends on
   it) — one ASSUMPTIONS entry; (b) BUILD_PLAN T5.B0 note says "restart cmd
   in data/recorder.log header" — the header carries parameters, not a
   command; (c) ROTA fit-validation V-5 note mislabels cognition as a
   fortuna-ledger dep (it is dev-only).

Cleared by the code gate: T5.B1 (spec v0.9) — the independent clearance is
overnight-code-gate-2026-06-11.md (the tick at 8544c3f cites an
implementer-side verdict; the independent one now also stands).

Operator actions queued (morning): ROTATE both Kalshi keys (demo + prod —
the prod key is also the kill-switch credential) and place new PEMs at the
.env paths; FINALIZE the purge (drop refs/original + reflog expire
--expire=now --all + gc --prune=now) BEFORE any first push — until then the
old key blobs remain recoverable from .git.
