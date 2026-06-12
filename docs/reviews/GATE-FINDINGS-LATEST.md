# GATE FINDINGS — latest (verifier-owned; implementer reads at priority (a))

Updated: 2026-06-12 ~03:30Z, covering commits 19954a6 + 8a4fcb2.
Verdict: r5test-slice6-gate-2026-06-12.md (ACCEPT-WITH-GAPS — third
consecutive non-BLOCK; zero Major).

Cleared: the R5 isolation test is committed WITH TEETH — verifier
scratch-merged the pools and the test went RED (PoolTimedOut on the
writer-unimpeded half), so the property is genuinely pinned; the Err arm
is honest now (available:false + neutral detail, no raw sqlx text leaking
to the view). Slice 6 is read-path-only confirmed (runner.rs +9/-0, pure
clone of an existing BTreeMap; the enforcement site is untouched). Battery
721/0/0, DST 2000-tier clean, invariants green, protected crate untouched.

Update 2026-06-12 ~04:05Z: money-view gate (money-view-gate-2026-06-12.md)
= ACCEPT-WITH-GAPS, FOURTH consecutive non-BLOCK, zero Major. Slice 7 money
view is honest: floating/total are literal nulls (never faked zeros),
settled/committed trace to inspect_totals with the committed-subset-of-
settled identity verified, "basis":"sim-only" labeled and test-asserted,
integer cents end-to-end. Battery 722/0/0. NEW Minor (item 4 below) —
SECOND occurrence of the vacuous-test class; a third on a money surface
escalates to Major by prior calibration note.

4. [Minor — vacuous-test class, 2nd occurrence] The money-view test stays
   green under a fully fabricated panel (mutation-proven: zeroed settled/
   committed/empty positions => workspace green). Assert real numbers:
   settled == venue cash from a seeded run with fills (e.g. the 11/3 seed:
   3 positions, yes_qty=50, fees 66/71/74), and add a reserved>0 seed so
   committed is asserted non-zero at least once.

ALSO SLOTTED (operator-directed): BUILD_PLAN T4.5 — ROTA v1.1 deferred
panels (shadow cross-join + discovery join queries, triage + discovery
panels, gate-verdict badge w/ parser, WS counters flip) — sequenced after
T4.2; perps panel stays with T5.B8 (mounts in ROTA). The directive
priority order now includes T4.5. Its TEST RULE bakes in the
vacuous-test lesson: populated-path seeds required.

Fix list (all Minor, ledger each):

1. The slice-6 "counts sum to total_rejections" test is VACUOUS on the
   populated path: the seeded run produces zero rejections, and stubbing
   the accessor to an empty map leaves every suite green. Seed a run that
   actually REJECTS (e.g. an exposure-cap breach) and assert a non-empty
   by_check breakdown sums to a non-zero total.
2. The R5 test self-constructs its pools — a future wiring merge at
   main.rs:93-108 would fail no test. Add a boot-path assertion (or a
   construction-site test) that the daemon's reader and writer pools are
   distinct objects.
3. The gates-view rationale "number would be a guess" is false:
   GateCheck::index() (fortuna-gates/src/pipeline.rs:75-86) provides the
   exact spec numbering (EdgeFloor=6 matches the design example). Either
   include the number field per the design contract or correct the
   rationale.

Standing: KXBTCPERP1 funding position crosses the 04:00 UTC tick — the
~05:00Z gate firing captures funding_history and completes fixture item
10. Do NOT close that position before then.

Operator actions still queued (unchanged): ROTATE both Kalshi keys;
FINALIZE the purge before any first push.
