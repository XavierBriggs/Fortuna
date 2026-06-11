# Review: system-0-4-egate-INDEPENDENT — 2026-06-10
Base: 7bbc3ef  Head: 1e3e5e7 (current HEAD 4305964 verified docs/ledger-only over head)
Verdict: BLOCK (automatic: protected crate touched) — on engineering merits: ACCEPT-WITH-GAPS
Protected crate touched: YES — crates/fortuna-invariants/tests/i7_promotion_gates.rs (+1 fixture line)

Independence note: this verdict was produced without reading any file in docs/reviews/,
including the prior e-gate verdict for this same range. Rubric = the E1-E5 close
criteria ledgered in GAPS.md at 7bbc3ef + the fortuna-review mechanical checklist.

## Scope verification

- Range 7bbc3ef...1e3e5e7 = five commits: 1d1c033 (E1+E5a), 0a57686 (E2), 5954999 (E3),
  ca38028 (E4), 1e3e5e7 (E5). Matches the stated batch.
- 4305964 over 1e3e5e7 touches ONLY FINAL_REPORT.md, GAPS.md,
  docs/reviews/system-0-4-egate-2026-06-10.md (git diff --stat: 3 files, no code/tests).
  No finding.
- Battery executed at 1e3e5e7 in detached worktree /tmp/fortuna-egate-2 (removed after).

## Protected-crate patch (complete, quoted for operator adjudication — waive batch 4)

```diff
diff --git a/crates/fortuna-invariants/tests/i7_promotion_gates.rs b/crates/fortuna-invariants/tests/i7_promotion_gates.rs
index 9823120..1393094 100644
--- a/crates/fortuna-invariants/tests/i7_promotion_gates.rs
+++ b/crates/fortuna-invariants/tests/i7_promotion_gates.rs
@@ -141,6 +141,7 @@ fn runner_config() -> RunnerConfig {
             max_spread_cents: 20,
         },
         max_sets_per_proposal: 50,
+        kelly_fraction: 0.25,
         veto_mind: None,
         veto_strategies: Vec::new(),
     }
```

Assessment: a one-line compile fix in a test FIXTURE helper (RunnerConfig gained the
E1 `kelly_fraction` field); zero assertion logic touched; all i7 tests pass by name.
The automatic-BLOCK rule applies as written regardless of content. The touch is
ledgered (ASSUMPTIONS.md E1 entry; GAPS.md waive batch 4 — present at 4305964, see
Minor F4). Operator waive converts this BLOCK.

## Criteria (fixed before reading the diff — GAPS.md@7bbc3ef E1-E5 close criteria)

- C1 E1 haircut-Kelly + calibrated-p sizing (spec 5.8, 5.10, line 240): PASS / CLOSED —
  cycle.rs:351-368 calibrates each belief's raw p (fitted at resolved_n>=50, shrinkage
  toward Direct-edge market prior below, unwired scope = full shrink); runner.rs:529-559
  sizes calibrated legs as sets = affordable.min(kelly_contracts(...)) with fraction =
  kelly_fraction x quality; unwired quality => 0.0 => zero size (never headroom).
  Close-criterion tests executed green: kelly_haircut_binds_synthesis_sizing_below_affordability
  (asserts qty in (0,100) vs ~1000 affordable), full_quality_sizes_larger_but_never_above_affordability,
  missing_calibration_quality_fails_closed_to_zero_size,
  calibration_layer_adjusts_p_before_the_comparator,
  uncalibrated_scope_shrinks_to_market_and_cannot_trade. DST arm: synthesis_dst seeds
  calibration (unwired 1-in-5 / fitted) + seeded quality (diff @ synthesis_dst.rs:247-258).
- C2 E5a integer-money Kelly budget + visible doc corrections: PASS / CLOSED —
  sizing.rs:70-73 computes budget as i128(headroom) * floored-ppm / 1e6 (money never in
  f64; double-floor conservative; try_from fallback unreachable since headroom*ppm/1e6
  <= i64::MAX). Test kelly_budget_is_integer_exact_and_never_exceeds_headroom green
  (exact at $10B headroom where f64 loses ulps). Corrections visible: cycle.rs header
  rewritten true; ASSUMPTIONS.md carries explicit "(E1 correction... the claim was
  false)" entries; BUILD_PLAN T2.6 carries the E1 CORRECTION note. T2.8 note: see F2.
- C3 E2 AnthropicMind behind Mind (spec 5.9 "both behind Mind"): PASS / CLOSED —
  impl Mind for AnthropicMind<T> (mind.rs:573-606) with budget check before / spend
  after at the trait boundary (lock never held across the transport await); provenance
  stamped by the harness in call_priced; mind_from_env factory gates on
  ANTHROPIC_API_KEY (key absent => StubMind with empty script). Close-criterion tests
  executed green: anthropic_mind_decides_through_the_dyn_mind_trait (mock transport,
  dyn Mind, provenance + day-cap refusal), mind_from_env_gates_on_the_key, and composed
  anthropic_mind_trades_the_composed_loop_through_dyn_mind in synthesis_loop.rs.
- C4 E3 per-cycle budget binds (spec 5.9): PASS / CLOSED on the close criterion —
  CostBudget tracks spent_this_cycle_cents (reset via begin_cycle at cycle start,
  accumulated post-call incl. refusals/schema-rejects where usage returned); positive
  cap refuses the exceeding call. Close-criterion tests executed green:
  per_cycle_cap_rejects_the_call_after_the_cycle_spends_it (cap 100: 60+40 spends, third
  check refused with spent=100/cap=100), anthropic_mind_enforces_the_cycle_cap_at_the_trait_boundary
  (2nd same-cycle call refused; resets next cycle). Degrade: cycle error => zero
  proposals, loop continues (cognition_failure_degrades_to_zero_proposals_never_a_crash
  green). GAP: no audit row / no alert on the breach — see F1 (Major).
- C5 E4 settlement/watchdog DST with firing arms: PASS / CLOSED — settlement_dst.rs:
  10 seeded arms (SettleClean, SettleThenCorrect, Void, Dispute, VenueMismatch,
  CanonicalDivergence, OrphanScan, AuditDeath, WideBook, Overdue) with per-arm hit
  accounting, per-seed byte-identical replay, arm-specific invariants (mismatch =>
  discrepancy + GLOBAL halt + zero orders next tick; audit death => halt (I5);
  divergence/orphan/dispute/overdue => their audit rows), every-arm assertion at
  >=200 scenarios. Executed at 10,000: ALL 10 ARMS FIRED — hit counts
  {SettleClean:1010, SettleThenCorrect:983, Void:948, Dispute:1006, VenueMismatch:1041,
  CanonicalDivergence:1002, OrphanScan:1006, AuditDeath:991, WideBook:969, Overdue:1044};
  2043 discrepancies, 3056 watchdog rows, 2032 halts; 0 failed seeds. Red seeds to
  dst-corpus/: none produced (corpus remains 0 seeds; vacuity disclosed in ASSUMPTIONS).
- C6 E5 minor sweep: PASS / CLOSED (with F3 nuance) —
  (a) run-dst.sh fail-closed: the `else "passing vacuously" exit 0` branch removed;
  `cargo test --no-run` failure propagates under set -euo pipefail; settlement stage added.
  (b) run_watchdogs partitioned (runner.rs:1525+): venue-dependent checks (posted->
  confirmed, books-vs-venue) skip on outage, streaks neither advance nor clear;
  venue-independent (overdue, dispute-on-last-known-meta, orphan scan) run dark —
  venue_independent_watchdogs_run_through_a_venue_outage executed green.
  (c) discards counted (discarded_model_proposals -> StrategyMetrics -> RunCounters ->
  fortuna_model_proposals_discarded_total) and proposals audit manifest_hash
  (discarded_model_proposals_are_counted_and_proposals_audit_their_manifest green);
  SynthesisStrategy retains manifest_hash on every proposal. Audit nuance: F3.
  (d) cycle.rs f64 floor + review.rs f64 ratio ledgered visibly in ASSUMPTIONS.md E5.
  (e) .DS_Store: 4 tracked files deleted, .gitignore entry added, `git ls-files |
  grep -i ds_store` empty at HEAD.
- C7 mechanical checklist: PASS — zero added unwrap/expect/panic/todo in any src file
  of the diff (per-file count all 0; mutex locks use unwrap_or_else(into_inner));
  no SystemTime/Instant/Utc::now added in code (hits only inside in-range markdown);
  new runner state uses BTreeMap/BTreeSet (deterministic); no secret literals (config
  example adds per_cycle_budget_cents only); no GatedOrder construction outside
  fortuna-gates; no new place( call sites.
- C8 test-weakening sweep (diff-wide): PASS — zero deleted asserts in crates/
  (grep over all '-' lines: empty); zero new #[ignore]; zero proptest case reductions;
  the only deleted test lines are 4 constructor-signature adaptations.
- C9 battery: PASS — fmt clean; clippy -D warnings clean; workspace tests all ok
  (0 failed, 0 ignored across every suite); fortuna-invariants per-test all ok
  (i1 x2, i2 x2, i3, i4, i5, i6 x3, i7 x3 + 3 doctests); full 3-stage
  scripts/run-dst.sh 10000 exit 0 (stage summaries below).
- C10 protected-crate rule: automatic BLOCK applied (patch quoted above).

## Findings

- [Major] F1: Per-cycle/day budget breach degrades silently — no audit row, no alert.
  Reproduction: synthesis.rs:168-173 maps every cycle error (incl.
  MindError::BudgetExhausted) to metrics.cognition_failures += 1 and Ok(vec![]);
  grep of all self.audit( kinds in runner.rs shows no cognition/budget/model_call kind;
  grep -rn cognition_failures crates/fortuna-ops/src/ is empty (no alert rule, no
  digest/dashboard-specific row — only the generic metric export
  fortuna_cognition_failures_total). Spec line 238: "budget breach degrades to
  mechanical-only and alerts"; spec line 340: "cost budget hit ... alert". The E3
  ledgered fix-spec explicitly required an "audit row". Binding + degrade + mechanical
  continuation ARE implemented and tested; the observability half is absent. Requires
  implementation or explicit operator-waived GAPS entry.
- [Minor] F2: BUILD_PLAN T2.8 DONE note ("quality = ... -> T2.6 haircut") was listed in
  the E1 ledgered fix-spec among false docs to correct visibly; only T2.6 received the
  correction note. The falsehood is recorded elsewhere (T2.6 note + ASSUMPTIONS), but
  the named T2.8 correction did not land. Ledger in GAPS.md or add the note.
- [Minor] F3: E5 "exclusions counted + audited": the discard EVENT reaches only the
  metrics plane (counter + export); audit coverage is indirect via the proposal rows'
  manifest_hash. A cycle whose model output is entirely discarded with no candidates
  leaves no audit trace of the discard. Same family as F1; fold into its resolution.
- [Minor] F4 (resolved at current HEAD; recorded for the rated commit): at 1e3e5e7,
  ASSUMPTIONS.md says the i7 touch was "added to the protected-crate waive queue in
  GAPS.md as batch 4", but GAPS.md at 1e3e5e7 lists only 3 batches; batch 4 appears in
  GAPS.md at 4305964. Transient intra-batch ledger lag; no action.
- [Note, not a finding — no reproduction reachable] E1 sizing keys Kelly off
  legs.first().calibrated_p with price = legs[0].limit_price but cost = whole-set cost;
  synthesis emits single-leg proposals only, so multi-leg calibrated sizing is
  currently unreachable. Worth a debt comment if multi-leg synthesis ever lands.
  Similarly, kelly_binary edge uses the raw limit price (fees enter via all-in cost
  divisor and the gates' net-edge floor), pre-existing T0.10 semantics.
- [Note] CalibrationParamsRepo.latest/resolved_stats have no non-test call site: the
  ledger fetch belongs to the live venue-generic composition, which does not exist yet
  by design (GAPS "Path to production" step 5, operator-blocked on Kalshi fixtures).
  Every layer accepts the wired CalibrationContext; tests compose it. Consistent with
  the disclosed build state; must land with the first post-fixture composition task.

## Commands run (verbatim verdict lines)

- git diff --stat 1e3e5e7..4305964 => "3 files changed" (FINAL_REPORT.md, GAPS.md,
  docs/reviews/system-0-4-egate-2026-06-10.md) — docs/ledger only.
- cargo fmt --check (worktree @1e3e5e7) => clean (exit 0).
- cargo clippy --workspace --all-targets -- -D warnings =>
  "Finished `dev` profile [unoptimized + debuginfo] target(s) in 19.91s" (zero warnings).
- DATABASE_URL=... cargo test --workspace => every suite "test result: ok"; 0 failed,
  0 ignored across all binaries/doctests.
- cargo test -p fortuna-invariants => 14 integration tests + 3 doctests, all ok, by name.
- scripts/run-dst.sh 10000 => exit 0. Stage summaries:
  - "[dst] regression corpus: 0 seed(s)"
  - "[dst] master seed 1781134933966 -> 10000 random scenario(s)"
  - "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations"
  - "[synthesis-dst] master seed 1781135317412 -> 10000 scenario(s)"
  - "[synthesis-dst] totals: 26921 orders, 41542 proposals, 131601 cognition failures,
    117733 beliefs" => "test result: ok. 1 passed ... finished in 86.91s"
  - "[settlement-dst] master seed 1781135407176 -> 10000 scenario(s)"
  - "[settlement-dst] arms {SettleClean: 1010, SettleThenCorrect: 983, Void: 948,
    Dispute: 1006, VenueMismatch: 1041, CanonicalDivergence: 1002, OrphanScan: 1006,
    AuditDeath: 991, WideBook: 969, Overdue: 1044}; 2043 discrepancies, 3056 watchdog
    rows, 2032 halts" => "test result: ok. 1 passed ... finished in 13.16s"
- Targeted suites (all ok by name): synthesis_loop (8), settlement_loop (12),
  observability (1), cognition mind (10), cognition cycle (10), state sizing (6),
  ops config (21).

## Verdict rationale

Every E-item's ledgered CLOSE CRITERION is met with executed evidence; the battery is
fully green at 10,000 seeds with all new arms demonstrably firing; no test weakening,
no money-path f64, no determinism or GatedOrder violations in the range. The verdict
is BLOCK solely under the CLAUDE.md automatic rule for the one-line protected-crate
fixture compile-fix (waive batch 4, queued for the operator). Upon that waive, the
batch stands at ACCEPT-WITH-GAPS: F1 (Major — budget-breach audit row + alert) must be
either implemented or explicitly operator-waived in GAPS.md; F2/F3 ledgered as Minors.
