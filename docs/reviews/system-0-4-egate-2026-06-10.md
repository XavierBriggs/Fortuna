# Review: system-0-4-egate — 2026-06-10
Base: 7bbc3ef (prior gate head, verdict BLOCK)  Head: 1e3e5e7  Verdict: ACCEPT
Protected crate touched: yes — exactly one commit (1d1c033), net +1 line
(`kelly_fraction: 0.25,` added to the `runner_config()` test fixture in
i7_promotion_gates.rs); ZERO assertion changes (full patch quoted below).
Per gate instructions this joins the operator waive queue as batch 4 and does
not block alone; the waive itself remains an operator action.

Scope: five commits — 1d1c033 (E1+E5a), 0a57686 (E2), 5954999 (E3),
ca38028 (E4), 1e3e5e7 (E5 sweep). 39 files, +3142/-155. All evidence below
executed this session at HEAD 1e3e5e7, DATABASE_URL=postgres://localhost/fortuna_dev.

## Criteria (fixed from GAPS.md close criteria + gate instructions, before reading the diff)

### E1 — haircut-Kelly + calibrated-p sizing wired: CLOSED
- C-E1a real src call sites: PASS — runner.rs:14,30 imports; runner.rs:536
  `haircut_kelly_fraction`, :538 `kelly_contracts`; cycle.rs:23,366 `calibrate(`;
  cycle.rs:373 `shrink_toward_market` (unwired arm). grep transcript in session.
- C-E1b composed-loop test, Kelly binds below affordability: PASS —
  `kelly_haircut_binds_synthesis_sizing_below_affordability ... ok` (asserts
  position qty in (0,100) vs ~1000 affordable, through a full SimRunner tick);
  `full_quality_sizes_larger_but_never_above_affordability ... ok`;
  `calibration_layer_adjusts_p_before_the_comparator ... ok` (cycle suite).
- C-E1c fail-closed both layers: PASS —
  `uncalibrated_scope_shrinks_to_market_and_cannot_trade ... ok` (unwired
  context => belief.p = market prior => no candidates; shrink_toward_market
  w=0 at n=0 returns the prior exactly);
  `missing_calibration_quality_fails_closed_to_zero_size ... ok` (proposals>=1,
  orders_submitted==0, no position). Runner: unwired quality defaults 0.0 =>
  fraction 0 => kelly 0; `kelly_contracts(...).unwrap_or(0)` on error.
- C-E1d false docs corrected, not erased: PASS — cycle.rs:1-20 header rewritten
  (states unwired-shrinks-fully + downstream min(kelly,affordable));
  BUILD_PLAN.md:175 "E1 CORRECTION 2026-06-10 ... The verification gate caught
  it."; ASSUMPTIONS.md "E1/E5a" section + line ~837 preserves the false-claim
  history.
- C-E1e DST arm: PASS — synthesis_dst.rs seeds CalibrationContext (4/5
  near-identity Platt fitted on a FIXED sample vector, 1/5 unwired) + seeded
  quality (rng%101/100) + `runner.set_calibration_quality`; deterministic per
  seed; byte-identical replay enforced by the harness. 10,000-scenario run
  green (below).
- C-E5a integer Kelly budget: PASS — sizing.rs:58-77: budget =
  `i128::from(headroom.raw()) * f_ppm / 1_000_000`, f64 touches only the
  one-time fraction->ppm floor; double-floored (conservative); i64 saturation.
  `kelly_budget_is_integer_exact_and_never_exceeds_headroom ... ok` (exact at
  f=1.0 on ~$10B headroom; bound property; i64::MAX no-overflow),
  `kelly_contracts_floors_into_integer_money ... ok`.

### E2 — AnthropicMind behind `Mind`: CLOSED
- C-E2a trait impl: PASS — mind.rs:576 `impl<T: MindTransport> Mind for
  AnthropicMind<T>`.
- C-E2b env factory: PASS — `mind_from_env` (mind.rs:611): key present =>
  Claude-backed mind; absent => StubMind with EMPTY script (zero
  beliefs/proposals). `mind_from_env_gates_on_the_key ... ok` (both branches).
- C-E2c composed dyn-Mind test: PASS —
  `anthropic_mind_trades_the_composed_loop_through_dyn_mind ... ok`
  (ScriptedTransport -> AnthropicMind -> Arc<dyn Mind> -> SynthesisStrategy ->
  SimRunner -> fills + position). Also
  `anthropic_mind_decides_through_the_dyn_mind_trait ... ok`.
- C-E2d lock + budget discipline: PASS — `decide` (mind.rs:592-604): budget
  mutex acquired/released in a scoped block BEFORE `call_priced(...).await`,
  re-acquired after; check precedes the call; cost rounds UP (ceil_div).
  API key: manual Debug impl prints `<REDACTED>`; errors name only the env var.

### E3 — per-cycle budget binds: CLOSED
- C-E3a positive cap binds: PASS — mind.rs `check()`: refuses when
  `per_cycle_cap_cents <= 0 || spent_this_cycle_cents >= per_cycle_cap_cents`.
  `per_cycle_cap_rejects_the_call_after_the_cycle_spends_it ... ok` (cap 100:
  60+40 spent => refused with scope "per_cycle"; new cycle resets; day cap
  still binds across cycles).
- C-E3b trait-boundary + cycle reset: PASS —
  `anthropic_mind_enforces_the_cycle_cap_at_the_trait_boundary ... ok` (cap 2c:
  call 1 ok, call 2 same cycle refused, next cycle ok); DecisionCycle::run
  calls `mind.begin_cycle()` (cycle.rs:354) before `decide`.
- C-E3c config surface: PASS — config/fortuna.example.toml:80
  `per_cycle_budget_cents = 50`; ops CognitionConfig field (config.rs:97) with
  non-negative validation (config.rs:185); ops config test asserts the value.

### E4 — discrepancy/watchdog seeded DST: CLOSED
- C-E4a arms complete: PASS — settlement_dst.rs ARMS = {SettleClean,
  SettleThenCorrect (reversal), Void, Dispute, VenueMismatch,
  CanonicalDivergence, OrphanScan, AuditDeath, WideBook, Overdue}. Mismatch arm
  asserts discrepancy != 0 AND global halt AND `post.orders_submitted == 0`
  (zero post-halt orders); divergence/orphan/dispute/overdue assert their
  audit rows; counter==audited-rows cross-check; every seed run TWICE with
  byte-identical recording required.
- C-E4b script stage + fail-closed: PASS — run-dst.sh stages 3+4 run
  synthesis_dst and settlement_dst; the old "passing vacuously; exit 0" branch
  is DELETED (diff quoted in session); executed probe:
  `CARGO_TARGET_DIR=/dev/null/notadir bash scripts/run-dst.sh 1` => exit 101.
- C-E4c targeted run, every arm fires: PASS — SETTLE_DST_SCENARIOS=400, master
  seed 1781133196437: arms {SettleClean:35, SettleThenCorrect:50, Void:31,
  Dispute:32, VenueMismatch:41, CanonicalDivergence:48, OrphanScan:37,
  AuditDeath:33, WideBook:44, Overdue:49}; 89 discrepancies (= 41+48 EXACTLY),
  118 watchdog rows, 74 halts (= 41+33 EXACTLY); zero failing seeds; replays
  byte-identical. Exit 0.
- C-E4d 10,000-seed run clean with arms active: PASS — `scripts/run-dst.sh
  10000` exit 0:
  - `[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations`
    (master 1781133339173)
  - `[synthesis-dst] totals: 27310 orders, 41831 proposals, 133489 cognition
    failures, 118696 beliefs` (master 1781133706113)
  - `[settlement-dst] arms {SettleClean:1021, SettleThenCorrect:973, Void:1019,
    Dispute:1035, VenueMismatch:994, CanonicalDivergence:964, OrphanScan:961,
    AuditDeath:1015, WideBook:1027, Overdue:991}; 1958 discrepancies (=994+964
    EXACT), 2987 watchdog rows, 2009 halts (=994+1015 EXACT)` (master
    1781133793536)

### E5 — minor sweep: CLOSED
- C-E5b watchdog partition: PASS — run_watchdogs (runner.rs:1524) fetches
  positions once; venue-DEPENDENT (confirm step, mismatch streak) gated on the
  poll; overdue/dispute/orphan run on clock + last-known meta regardless.
  `venue_independent_watchdogs_run_through_a_venue_outage ... ok` (venue dark
  24h, overdue + orphan still fire).
- C-E5c discards visible + provenance: PASS — StrategyMetrics/RunCounters
  `model_proposals_discarded` -> prometheus
  `fortuna_model_proposals_discarded_total`; every proposal audits a
  "proposal" row with manifest_hash (runner.rs handle_proposal; synthesis
  passes Some(hash), mechanical None).
  `discarded_model_proposals_are_counted_and_proposals_audit_their_manifest ... ok`.
- C-E5d hygiene: PASS — `git ls-files | grep -i ds_store` empty; .gitignore:6
  `.DS_Store`; 4 tracked .DS_Store files deleted in the batch.
- C-E5e f64 ledger: PASS — ASSUMPTIONS.md "E5" section records cycle.rs
  fair-value floor (conservative direction) + review.rs ratio
  (recommendation-only); new probability-space f64s (calibrated_p,
  kelly_fraction, quality) are non-money; ops config.rs documents the
  exemption; kelly_fraction validated (0,1] in ops and clamped/zeroed
  (non-finite => 0) in the runner.

### Regression + hygiene
- C-R1 fmt: PASS — `cargo fmt --all -- --check` exit 0.
- C-R2 clippy: PASS — `cargo clippy --workspace --all-targets -- -D warnings`
  exit 0.
- C-R3 workspace: PASS — `cargo test --workspace`: 630 passed / 0 failed /
  0 ignored (exit 0).
- C-R4 invariants: PASS — fortuna-invariants exit 0, all by name: i1 x2 +
  2 compile-fail doctests, i2 x2, i3, i4 (26.74s, real killswitch spawn), i5,
  i6 x3, i7 x3; zero ignored.
- C-P protected crate: CONFIRMED ASSERTION-CLEAN — `git log/diff
  7bbc3ef..HEAD -- crates/fortuna-invariants/` = one commit, +1 fixture line:
  ```
  +        kelly_fraction: 0.25,
  ```
  in fn runner_config() (test fixture builder). No assertion, tolerance,
  attribute, or deletion anywhere. Queued: operator waive batch 4.

## New-gap hunt (adversarial sweeps, all executed)
- Gate bypass: NONE — sized candidates flow through evaluate_gates; the single
  `place(` site (exec/manager.rs:329) takes `fortuna_gates::GatedOrder`; no
  GatedOrder construction outside fortuna-gates; Kelly only ever REDUCES sets
  (`sets = sets.min(kelly)`); veto remains reduce-only, pre-gate.
- Secrets: NONE — proposal/sizing audit rows carry thesis/manifest/p/quality
  only; transport Debug redacts api_key; diff-wide secrets grep clean.
- Determinism: clean — calibration_quality and market_meta are BTreeMaps; DST
  calibration uses a fixed Platt sample vector; replay-equality enforced at
  400/2000/10000.
- Panic/clock sweeps on added src lines: zero unwrap/expect/panic (only total
  `unwrap_or`/`unwrap_or_else(into_inner)`); zero SystemTime/Instant/Utc::now.
- Test weakening, diff-wide: zero deleted asserts, zero new #[ignore], zero
  proptest reductions (all grep hits are prose in review/ledger docs).

## Findings
- [Note — operator queue] Protected-crate one-line fixture compile fix
  (1d1c033) triggers the automatic-BLOCK rule; assertion-clean confirmed;
  joins waive queue batch 4 per gate instructions. Not blocking alone.
- [Minor — pre-existing, ledgered] Regression seed corpus still empty
  (`[dst] regression corpus: 0 seed(s)`); vacuity disclosed in ASSUMPTIONS.md
  and GAPS.md (no red seed has ever been produced to commit). Carried, not new.
- [Housekeeping] GAPS.md still lists E1-E5 under "Unresolved ENGINEERING
  items" — correct conservative posture pending this gate; implementer should
  now annotate them CLOSED (citing this verdict) without deleting the history.
- No new Critical or Major findings. ACCEPT is for the E-batch only: live
  trading remains gated behind the operator queue (waive batches 1-4,
  credentials, Kalshi fixtures, promotions per I7) exactly as ledgered.

## Commands run (verbatim verdict lines)
- cargo fmt --all -- --check                              -> exit 0
- cargo clippy --workspace --all-targets -- -D warnings   -> exit 0
- cargo test --workspace                                  -> 630 passed; 0 failed; 0 ignored (exit 0)
- cargo test -p fortuna-invariants                        -> exit 0 (suites 2/2/1/1/1/3/3 + 2 doctests; 0 ignored)
- SETTLE_DST_SCENARIOS=400 cargo test -p fortuna-runner --test settlement_dst -- --nocapture
    -> ok; master 1781133196437; all 10 arms; 89 discrepancies/118 watchdog/74 halts
- bash scripts/run-dst.sh        (N=2000, battery)        -> exit 0; "[dst] OK: 0 corpus + 2000 random seeds, zero invariant violations"
- bash scripts/run-dst.sh 10000                           -> exit 0; "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations";
    synthesis 10000 scenarios (133489 cognition failures degraded); settlement 10000 scenarios, every arm 961-1035 hits, exact counter arithmetic
- CARGO_TARGET_DIR=/dev/null/notadir bash scripts/run-dst.sh 1 -> exit 101 (fail-closed probe)
- Logs: /tmp/fortuna-egate/{battery,clippy,workspace-tests,invariants,settlement-dst,dst-full,dst-10k,failclosed}.log
