# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-11 ~20:10Z, covering range dfb849f..75f4782 + the FIRST LIVE
BROWSER PASS (daemon booted from the committed example config, scratch Pg).
Verdict: rota-slices-gate-2026-06-11.md (BLOCK, narrow). Screenshots:
docs/reviews/rota-visual/.

Cleared: ALL six prior items (dedup hoisted with cross-segment test —
live-confirmed: halts_applied=1 across 385 polls in the browser-pass run);
clippy/battery green (711/0/0, DST 10000x3, 11/11 arms); ROTA slices 1-3
conform to amendments R1/R2/R4/R6/R7/R8/R9/R10/R11 (Option-capability,
no runner dep, no SSE, no gates surface, lock rule, route-table, auto-fit
grid, tokens + working halt takeover, zero CDN). Live browser pass: boot,
all served panels render, degraded states correct, halt takeover verified
visually end-to-end (DB insert -> red SYSTEM HALTED within one segment).

Fix list, priority order:

1. [MAJOR — reproduced, latent] Audit-tail cursorless default returns the
   OLDEST page: `after.unwrap_or_default()` => `audit_id > '' ASC`, while
   the handler comment claims "absent => latest page" and ROTA_SHELL polls
   cursor-less at 2s — when the R5 pool lands, the live audit panel will
   permanently show the first 100 rows ever written. Fix: cursorless =>
   LATEST page (or shell tracks next_after); align the comment; the owed
   cursor-pagination test MUST include the absent-cursor case.
2. [Minor] /favicon.ico 404s — the only browser console error in the live
   pass (R12 pass/fail criterion at T4.3 completion). Land assets/rota/
   favicon + logo route, or stub a 204 until the asset slice.
3. [Minor] rota audit query is runtime sqlx::query_as, not compile-time
   checked — convert or ledger the exception.
4. [Minor] DailyScheduler fires immediately on every restart (mid-day
   "daily digest" per boot) — deliberate per test but unledgered; also the
   digest reports cumulative-since-boot counters labeled as the day's, and
   drive()-level digest routing/audit has no committed assertion.
5. [Minor] ASSUMPTIONS dead-man entry contradicts GAPS ("no exception
   needed") and still says SystemTime::now post-RealClock-fix — reconcile.
6. [BROWSER-PASS NOTE, informational until T4.3 gate] Panels currently
   render RAW JSON — the instrument presentation layer (formatted numbers,
   tabular figures, per-panel layouts per the design Section 2/5) is the
   remaining UI work; the full-page halt overlay leaves below-fold panels
   visible in fullPage capture (fine in viewport — confirm fixed-position
   intent); LIVE OBSERVATION: the recorder risk_parameters stream showed
   unhealthy/411s-stale on boot — investigate the capture loop (the
   dashboard did its job; do not let this signal rot).

Operator actions still queued (unchanged): ROTATE both Kalshi keys;
FINALIZE the purge before any first push.
