# Review: f-batch-INDEPENDENT-gate — 2026-06-10
Base: 4305964  Head: 9b244ee  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (git diff --name-only 4305964...9b244ee | grep fortuna-invariants -> 0 files)

Independence: docs/reviews/f-batch-gate-2026-06-10.md and the rest of docs/reviews/
were NOT read. Range diff inspected with docs/reviews/ excluded (additive-only
confirmed via --stat and absence of deletions). All execution in a detached
worktree at 9b244ee (/tmp/fortuna-fgate); the flake adjudication used a second
worktree at 4305964.

## Criteria (fixed before reading the diff)

### Part A — F1-F3 close criteria (as ledgered in GAPS.md pre-fix)
- A1 F1 failure kind preserved, not `Err(_failure)`-discarded: PASS — diff to
  crates/fortuna-runner/src/synthesis.rs replaces `Err(_failure)` with a match
  preserving budget_exhausted (scope/spent_cents/cap_cents), provider,
  schema_invalid, refused, context; buffered as DegradeRecord.
- A2 F1 audit row per degraded cycle w/ kind+scope, spent/cap on BudgetExhausted:
  PASS — runner.rs drains DegradeRecords each tick into audit kind="cognition"
  rows + bus events; test
  crates/fortuna-runner/tests/synthesis_loop.rs::budget_breach_and_discarded_output_write_audit_rows
  asserts degrade=budget_exhausted, scope=per_cycle, spent_cents=50, cap_cents=50,
  strategy=synth_sim, and budget_breaches>=1. Green in the 636-test workspace run.
- A3 F1 ops alert rule referencing the signal (every-breach budget; threshold
  bursts): PASS — crates/fortuna-ops/src/alerts.rs::degrade_alerts alerts on
  every budget_breaches_delta>0 (one message per scrape with count) and on
  cognition_failures_delta >= threshold; tests every_budget_breach_alerts and
  failure_threshold_separates_blips_from_outages green. Spec line 238 quoted:
  "budget breach degrades to mechanical-only and alerts." Note Minor F-3 below:
  the rule has zero non-test callers.
- A4 F2 BUILD_PLAN T2.8 visible correction, history not erased: PASS — the
  E1-CORRECTION paragraph is inserted ABOVE the original "DONE 70c5df6:" note,
  which remains verbatim (git diff 4305964...9b244ee -- BUILD_PLAN.md).
- A5 F3 wholly-discarded model output writes an audit trace: PASS — synthesis.rs
  pushes degrade="model_proposals_discarded" with count; the same test asserts a
  cognition audit row with count==1 for an output whose only proposal is dropped
  and zero candidates derive.

### Part B — the three Minor-closure claims at 9b244ee
- B1 settlement_dst assert floor — ADJUDICATED AS FLAKE-FIX, NOT TEST-WEAKENING:
  PASS. Executed evidence:
  (a) base 4305964 (pre-floor asserts, file identical at 8a83c62),
      DST_MASTER_SEED=1781139292562, default 20 scenarios -> FAILED at
      settlement_dst.rs:533 "battery never halted (distribution bug)"; the arm
      draw was {SettleClean:1, SettleThenCorrect:4, Void:5, CanonicalDivergence:2,
      OrphanScan:4, WideBook:2, Overdue:2} — zero halting arms sampled. The old
      assert reds HEALTHY code on a legitimate 20-draw.
  (b) head 9b244ee, same seed, 20 scenarios -> ok (floor gates coverage asserts).
  (c) head, same seed, SETTLE_DST_SCENARIOS=100 -> asserts BIND and pass:
      16 discrepancies, 29 watchdog rows, 10 halts.
  (d) head, same seed, SETTLE_DST_SCENARIOS=200 -> per-arm block binds, all 10
      arms hit, 28 halts.
  (e) corpus N=10000 (run-dst.sh) -> aggregate AND per-arm asserts bound and
      passed; per-arm hits {SettleClean:995, SettleThenCorrect:1031, Void:987,
      Dispute:1028, VenueMismatch:980, CanonicalDivergence:987, OrphanScan:965,
      AuditDeath:997, WideBook:1004, Overdue:1026}; 1967 discrepancies, 3019
      watchdog rows, 1977 halts.
  No assertion deleted: the >=200 per-arm block is byte-identical to base
  (git show 4305964:...settlement_dst.rs lines 525-541 compared); the
  unconditional behavior assert on the failures list is untouched; only the
  three aggregate COVERAGE asserts gained the >=100 floor. run-dst.sh always
  passes SETTLE_DST_SCENARIOS=$N (default 2000), far above both floors.
- B2 fortuna_mind_spend_today_cents includes failed-call burn: PASS —
  Mind::spent_today_cents (default 0) + AnthropicMind impl reads the owned
  CostBudget; test crates/fortuna-cognition/tests/mind.rs::failed_calls_burn_into_spent_today
  drives a schema-invalid (prose) response, asserts MindError::SchemaInvalid and
  spent_today_cents()>0. Surface rides StrategyMetrics -> RunCounters ->
  metrics_export as fortuna_mind_spend_today_cents (name asserted in
  sim_loop section_8 test). Green.
- B3 false "cost rides in cognition audit rows" line corrected visibly: PASS —
  at 8a83c62 ASSUMPTIONS.md read "per-decision cost rides in the cognition
  audit rows"; at 9b244ee the line is corrected to belief PROVENANCE with the
  correction itself recorded ("the f-batch gate caught this entry claiming...").

### Part C — latency commits 44f4bb9 + 8a83c62 vs spec Section 8 (line 326
"order/fill latency" among required metrics; criteria fixed from spec text first)
- C1 order/fill latency measured and exported: PASS — LatencyStat
  (count/sum/max + 14 fixed buckets + overflow) for ack (submit->Acked, injected
  clock delta) and fill (fill.at - submit time by client order id, APPLIED fills
  only); Prometheus cumulative le-buckets with +Inf==count, p90/p95/p99 gauges.
  Test sim_loop.rs::fill_latency_is_measured_from_submit_to_execution forces
  ack_delay_pm=1000, asserts max_ms>=500 across the tick gap, exported max>=500,
  cumulative bucket monotonicity, +Inf==count.
- C2 time ONLY from injected Clock: PASS — submit stamps via
  self.clock.now().epoch_millis(); fill side uses venue-stamped fill.at.
  Tree-wide grep: zero SystemTime::now/Instant::now/Utc::now outside
  crates/fortuna-core/src/clock (and tests/doc comments).
- C3 deterministic percentile/export path: PASS — fixed LATENCY_BUCKETS_MS
  array scan (no unordered maps; submit_times and gate_rejections_by_check are
  BTreeMap); quantile is the conservative bucket UPPER edge (overflow -> observed
  max), never understating; same-seed determinism asserted (count/sum/max
  equality across two identically-seeded runs).
- C4 metrics do not touch money paths: PASS — observe()/export mutate counters
  only, no control-flow or money arithmetic change in submit/fill paths; the
  only f64 added is the quantile probability q (not money/prices).
- C5 no new panic/unwrap/expect in non-test code: PASS — per-file diff sweep
  over runner.rs, lib.rs, synthesis.rs, mind.rs, alerts.rs src additions: zero
  hits (all unwrap/expect additions are in tests/; mind.rs uses
  lock().unwrap_or_else(|e| e.into_inner())).

### Part D — battery and sweeps
- D1 cargo fmt --check: PASS (exit 0).
- D2 cargo clippy --workspace --all-targets -- -D warnings: PASS
  ("Finished `dev` profile", zero warnings, exit 0).
- D3 cargo test --workspace (DATABASE_URL=postgres://localhost/fortuna_dev):
  PASS — TOTAL passed=636 failed=0.
- D4 scripts/run-dst.sh 10000: PASS —
  stage 1/2: "[dst] OK: 0 corpus + 10000 random seeds, zero invariant
  violations" (master 1781141529660);
  stage 3: "[synthesis-dst] master seed 1781141905136 -> 10000 scenario(s)"
  ... ok (26696 orders, 40953 proposals, 132348 cognition failures, 116593
  beliefs), 90.02s;
  stage 4: "[settlement-dst] master seed 1781141995609 -> 10000 scenario(s)"
  ... ok, per-arm hits as quoted under B1(e).
- D5 fortuna-invariants per-test at head: PASS — i1_universal_gate,
  i1_prop_all_orders_carry_gate_verdicts, i2_drawdown_human_rearm,
  i2_prop_breach_always_locks_until_rearm, i3_runaway_halt,
  i4_killswitch_independence, i5_audit_append_only, i6 x3, i7 x3 all ok,
  plus 3 doc/compile-fail tests ok.
- D6 mechanical sweep over the range diff: PASS — no ambient time added; no
  HashMap/HashSet added; no #[ignore] or proptest case reductions; no secrets
  (only benign input_tokens/output_tokens hits); the single `.place(` call site
  (crates/fortuna-exec/src/manager.rs:329) untouched; no GatedOrder
  constructors outside fortuna-gates introduced; the ONLY deleted asserts in
  the entire range are the three settlement_dst aggregate asserts re-added
  under the >=100 floor (adjudicated in B1).
- D7 protected crate: NOT TOUCHED.

## Findings
- [Minor] F-1: settlement_voids / settlement_reversals counter increments are
  never behavior-asserted. The only added assertion
  (crates/fortuna-runner/tests/settlement_loop.rs:369-370) checks the PRE-state
  (==0 before the tick) and nothing checks the post-tick increment; the
  section-8 surface test asserts only metric-name presence. A regression that
  stops incrementing either counter passes the suite. Reproduction: grep -rn
  "settlement_voids|settlement_reversals" crates/fortuna-runner/tests/ -> one
  hit, pre-state only.
- [Minor] F-2: fortuna_mind_spend_today_cents is exported with counter: true
  (runner.rs metrics_export, first sample block) while documented and behaving
  as a GAUGE ("resets at the mind's 00:00 UTC roll", lib.rs field doc). Latent
  today (MetricSample has no non-test consumer) but will emit "# TYPE ...
  counter" for a day-resetting value once composed into the ops registry
  (fortuna-ops/src/metrics.rs render_prometheus), corrupting rate() queries.
- [Minor] F-3: fortuna_ops::alerts::degrade_alerts has ZERO non-test callers
  (grep -rn degrade_alerts crates/ excluding tests -> definition only), yet the
  alerts.rs module doc and ASSUMPTIONS.md state in present tense "The live
  composition diffs counters per scrape and routes through SlackRouter" — no
  such composition exists in the repo. The ledgered F1 close criterion ("an
  ops-layer rule existing and referencing the signal") is met as written, so
  this does not reopen F1; but the doc overstates, and the alert is not
  deliverable until a scrape composition exists. Should be ledgered as the
  wiring gap it is.

## Commands run (verbatim verdict lines)
- cargo fmt --check -> exit 0
- cargo clippy --workspace --all-targets -- -D warnings -> "Finished `dev`
  profile [unoptimized + debuginfo] target(s) in 19.84s" (exit 0)
- cargo test --workspace -> TOTAL passed=636 failed=0
- base@4305964 DST_MASTER_SEED=1781139292562 settlement_dst -> "panicked at
  crates/fortuna-runner/tests/settlement_dst.rs:533: battery never halted
  (distribution bug) ... test result: FAILED. 0 passed; 1 failed"
- head@9b244ee same seed, 20 scen -> "test result: ok. 1 passed"
- head SETTLE_DST_SCENARIOS=100 same seed -> "...16 discrepancies, 29 watchdog
  rows, 10 halts ... ok"
- head SETTLE_DST_SCENARIOS=200 same seed -> all 10 arms hit, "...28 halts ... ok"
- scripts/run-dst.sh 10000 -> "[dst] OK: 0 corpus + 10000 random seeds, zero
  invariant violations"; synthesis-dst ok (90.02s); settlement-dst ok
  (per-arm 965..1031, 1977 halts)
- cargo test -p fortuna-invariants -> 13 tests + 3 doc-tests, all ok
