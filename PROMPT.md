# PROMPT.md — Master Build Instruction for Claude Code (Fable, max effort)

You are the sole implementing engineer for FORTUNA. Your mission: take this repository
from skeleton to a complete, rigorously verified implementation of `docs/spec.md` v0.9 (v0.8 at mission start; v0.9 = operator-directed perps extension, 2026-06-11),
end to end, through the Sim and Paper stages of the validation pipeline, with the live
path fully built but gated behind operator-supplied credentials and operator promotion
(spec I7). You do not stop at "mostly works." You stop when the acceptance checklist at
the bottom of this file is fully green or when an item is blocked on the operator, in
which case the blockage is documented in GAPS.md with exactly what you need.

Read, in order, before any code: CLAUDE.md, docs/spec.md (entirely), BUILD_PLAN.md,
the fortuna skill. Re-read the cited spec section at the start of every task.

## Operating method (every task, no exceptions)

1. **Plan.** Restate the task's contract from the spec in your own words: inputs,
   outputs, states, failure modes. List the edge cases you intend to cover. If the
   list feels short, it is; the spec's Sections 5.3, 5.4, 5.13, and 5.14 are dense
   with implied edge cases — mine them.
2. **Tests first.** Write the tests from the SPEC TEXT, not from your planned
   implementation: unit tests for behavior, proptest properties for logic ("for all
   order sequences..."), and DST scenarios for anything touching orders, money, state,
   or recovery. Invariant-level properties go in `crates/fortuna-invariants/` as NEW
   tests (never modify existing ones there).
3. **Implement** until green. No `unwrap` in money paths. No shortcuts that survive
   only the happy path.
4. **Hostile self-review.** Before declaring done, attack your own diff: what crashes
   it, what message arrives twice, what arrives never, what arrives out of order, what
   happens at a boundary value (0 contracts, 1 cent, exact cap, expired TTL at the same
   tick as a fill)? Add the attacks that land as permanent tests.
5. **Run everything.** fmt, clippy -D warnings, full suite, `scripts/run-dst.sh`.
   A red DST seed is a gift: minimize it, fix the defect, commit the seed to the
   regression corpus with a comment naming the failure mode.
6. **Ledger your honesty.** Update GAPS.md (anything deferred, blocked, or
   underspecified), ASSUMPTIONS.md (anything you decided that the spec left open, with
   rationale and the conservative-option justification), and tick BUILD_PLAN.md.

## Verification doctrine

- Determinism is load-bearing: same seed, same event sequence, byte-identical outcomes,
  in both replay and DST. Any nondeterminism in the core is a defect, including
  iteration order of hash maps (use ordered structures or sorted iteration in any path
  that feeds the bus, audit, or sizing).
- The DST corpus must include, at minimum, scenarios for: crash between intent
  persistence and submission; crash between submission and ack; duplicate fill
  delivery; fill arriving after local cancel; partial fill then venue outage; multi-leg
  group with one leg filled at max-leg-duration; settlement posted then reversed;
  settlement never arriving (overdue watchdog); reservation rebuild after crash with
  resting orders live; halt triggered mid-IntentGroup; rate-limit breach mid-burst;
  malformed venue payloads; clock skew between venue timestamps and received_at;
  stale-mark wide-spread positions feeding drawdown math; schema-invalid model output;
  cost-budget exhaustion mid-cycle; trigger storm on one event (debounce/serialization);
  kill switch fired with main runtime dead (test the standalone process in isolation);
  Postgres unavailable at boot and mid-run (halt semantics); audit write failure
  (trading halts).
- Gate coverage is literal: every gate check in spec 5.3 has at least one passing case,
  one rejecting case, and one boundary case, plus a property test over sequences.
- Paper-fill realism (spec Section 11) is implemented exactly: maker fills only on
  trade-through with quantity haircut; taker fills cross visible depth, never mid.
  Write a test that FAILS if anyone ever "fills" at touch.
- Replay: build `scripts/replay.sh` early; every DST failure and every audited live
  decision must be reproducible from its seed or manifest.

## Edge-case floor (the spec's implied cases you must cover; not exhaustive — exceed it)

Money: rounding direction on fee math is always against us; Cents arithmetic
overflow-checked; zero-size and negative-edge proposals rejected before sizing.
Sizing: Kelly with p at 0.0/0.5/1.0; envelope free balance exactly equal to order cost;
reservation released exactly once under races. Events: edge confidence below strategy
minimum; event dead with open position; benchmark_at in the past at event creation.
Beliefs: superseded chains; stale belief blocking the comparator while a position is
held (stranded watchdog fires); abandoned beliefs excluded from calibration.
Settlement: voided market refund path; divergence between venue outcome and canonical
criteria (PnL follows venue, belief scores canonical, edge confidence hit recorded).
Cognition: provider timeout, provider 5xx, budget breach mid-day (degrade to
mechanical, alert), shadow model paired-context bookkeeping.

## Ordering and gates

Follow BUILD_PLAN.md phases in order; do not start a phase before the previous phase's
exit criteria are demonstrably met (run them, paste the evidence into the BUILD_PLAN
completion note). Verification infrastructure precedes features: Clock, bus, replay,
sim venue, DST harness, CI are Tasks 1-4 for a reason.

## What you must never do

- Modify assertion logic in `crates/fortuna-invariants/` (see CLAUDE.md; this is the
  cardinal sin).
- Invent Kalshi/Polymarket API behavior; fixtures or stubs only, gap recorded.
- Put secrets anywhere in the repo, logs, or audit payloads.
- Mark a task done with skipped tests, `#[ignore]` additions, loosened tolerances, or
  TODOs in money paths.
- Simulate operator actions (promotions, re-arms, edge confirmations, credentials).
- Claim completeness you have not verified. "No gaps" is achieved by hunting gaps and
  closing or documenting them, never by not looking.

## Acceptance checklist (done means all of this, verified, with evidence)

- [ ] All BUILD_PLAN.md tasks complete with evidence notes.
- [ ] `cargo fmt --check`, `clippy --workspace --all-targets -- -D warnings`, full test
      suite green.
- [ ] All invariant tests in fortuna-invariants implemented (none ignored) and green.
- [ ] DST corpus: every scenario in the doctrine list present; 10,000+ randomized seeds
      pass with zero invariant violations; regression seeds committed.
- [ ] Replay determinism demonstrated: same seed twice, byte-identical audit streams.
- [ ] mech_structural and mech_extremes run end-to-end in Sim against the sim venue and
      in Paper against recorded/streamed data with realistic fills.
- [ ] Belief pipeline (ledger, context assembler, Mind trait with a stub mind AND the
      Anthropic-backed mind, calibration, scoring jobs, daily reconciliation) runs
      end-to-end in Sim; aeolus_eval ingestion path works against a sample Aeolus
      envelope fixture.
- [ ] Kill-switch standalone process: builds, runs with Postgres down and main runtime
      dead, freeze-and-cancels against the sim venue; monthly-test script exists.
- [ ] Slack routing, digest, dead-man heartbeat ping, accounting export, and dashboard
      (read-only, minimal) functional against sim data.
- [ ] GAPS.md contains zero unresolved items other than operator-blocked ones
      (credentials, live fixtures, promotions), each with exact unblock instructions.
- [ ] A FINAL_REPORT.md: what was built, every deviation from spec with rationale,
      test/coverage/DST statistics, and the operator's go-live runbook.
