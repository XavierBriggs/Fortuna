# Review: r5test-slice6 (19954a6 + 8a4fcb2) — 2026-06-12
Base: 468c8c1 (scope: commits 19954a6 and 8a4fcb2 ONLY; b18e93a and 5f4a017 out of
scope per operator)  Head: 8a4fcb2  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (file list of both commits: BUILD_PLAN.md, GAPS.md,
docs/design/rota-dashboard.md, fortuna-live/{src/views.rs,tests/views.rs},
fortuna-ops/{src/rota.rs,tests/rota.rs}, fortuna-runner/src/runner.rs)

Gated at commit 8a4fcb2 in detached worktree /tmp/fortuna-g9 (clean, exact HEAD).
Only prior review artifact read: docs/reviews/GATE-FINDINGS-LATEST.md (per operator).

## Criteria (fixed before reading the diff)

### A1 — R5 saturation/isolation test + honest Err arm (8a4fcb2; gate finding #1)
- C1 Saturated 2-conn reader pool => GET /audit degrades to HTTP 200/available:false,
  bounded, never hung/500: PASS — test exhausted_rota_pool_degrades_to_200_while_the_
  writer_is_unimpeded (crates/fortuna-ops/tests/rota.rs:450-533) holds both reader
  conns, asserts available==false, rows empty, read_dur < 3s (acquire_timeout 1s).
  Executed: `cargo test -p fortuna-ops --test rota exhausted_rota_pool` =>
  "1 passed; 0 failed ... finished in 1.27s"; the server-side degraded log
  ("rota: audit-tail read degraded: pool timed out while waiting for an open
  connection") printed during the run — the Err arm fired via real pool starvation.
- C2 Concurrent writer-pool INSERT unimpeded WHILE reader saturated: PASS — the read
  and the INSERT run concurrently via tokio::join!; write_dur asserted < 1s AND the
  row asserted committed (SELECT COUNT(*) FROM audit == 2). Both halves present;
  not PARTIAL.
- C3 Test goes red if the pools are merged: PASS at the pool-topology level —
  scratch-patched the test so writer_pool = rota_pool.clone() (merged world):
  "panicked at crates/fortuna-ops/tests/rota.rs:350:6 ... PoolTimedOut" =>
  "test result: FAILED. 0 passed; 1 failed" — red exactly in the writer-unimpeded
  half. Patch reverted (git status clean). RESIDUAL: see Finding 3 — the test
  self-constructs its pools; the daemon wiring seam is not pinned.
- C4 Honest Err arm: PASS — before (diff context): Err arm returned
  `"available": true, "error": e.to_string()` — a FAILED read labeled available,
  leaking raw sqlx/Pg error text to the client. After: available:false + neutral
  detail ("audit read unavailable (dashboard pool degraded)"), cause logged
  server-side via eprintln (consistent with existing main.rs:97 convention).
  Exercised live in C1's run.
- C5 No test weakening; placed in fortuna-ops tests, not the protected crate: PASS.

### A2 — gates.rejections_by_check (19954a6)
- C6 runner.rs change is READ-PATH ONLY: PASS — the runner.rs diff is +9/-0: a doc
  comment plus `pub fn rejections_by_check(&self) -> BTreeMap<String,u64> {
  self.gate_rejections_by_check.clone() }` (&self, pure clone). The enforcement/
  counting site (runner.rs:912-931: `match outcome.gated { Err(rejection) => ...
  *self.gate_rejections_by_check.entry(format!("{:?}", rejection.check)) ... }`)
  is pre-existing and untouched (zero deletions in the file). No gate-decision or
  money-path semantics changed. BTreeMap => deterministic ordering, no
  unordered-iteration leak.
- C7 Per-check JSON matches the design gates contract: PASS WITH DEVIATION — sorted
  `{check, count}` entries; check names are GateCheck Debug strings (Halts, Capital,
  PositionCaps, PriceSanity, SizeSanity, EdgeFloor, RateLimits, Idempotency,
  EventExposure, InternalNetting) matching the design example "EdgeFloor"
  (docs/design/rota-dashboard.md:261). The body contract's `"number"` field is
  OMITTED; deviation is documented (design-doc slice-6 note + BUILD_PLAN), but the
  recorded rationale is false — see Finding 2. Spec grounding of the surface itself
  verified: docs/spec.md:348 (Section 8) "gate rejection counts by reason", verbatim.
- C8 Empty/no-rejections renders cleanly: PASS — scratch probe of the committed test
  run printed `total_rejections=0 by_check=Array []` — field present as an empty
  array (not absent/null) at zero; sum invariant 0==0 holds. BUT the committed test
  never exercises a nonzero count — see Finding 1.
- C9 R8 lock rule on touched handlers: PASS — audit_tail (the only handler touched,
  8a4fcb2) never takes the snapshot RwLock at all (pool-only). read_view and
  view_streams clone inside a block scope and release before any further await.
  19954a6 touched no handler (views.rs is daemon-side shaping per R2).

### B — battery at 8a4fcb2
- C10 fmt: PASS — `cargo fmt --check` => FMT_EXIT=0.
- C11 clippy: PASS — `cargo clippy --workspace --all-targets -- -D warnings` exit 0;
  cache-staleness ruled out by touching the three changed files and re-linting
  fortuna-ops/fortuna-live/fortuna-runner with -D warnings => exit 0.
- C12 workspace tests: PASS — `cargo test --workspace` => WORKSPACE_EXIT=0,
  107 suites "test result: ok", passed=721 failed=0 (prior gate's 720 + the new
  rota test, matching the commit message's claim).
- C13 invariants per-test: PASS — fortuna-invariants: i1_universal_gate,
  i1_prop_all_orders_carry_gate_verdicts, i2_drawdown_human_rearm,
  i2_prop_breach_always_locks_until_rearm, i3_runaway_halt,
  i4_killswitch_independence, i5_audit_append_only, i6 x3, i7 x3 — all ok; plus
  3 compile-fail doctests ok. 0 failed.
- C14 DST: PASS — `scripts/run-dst.sh` (default N=2000) => DST_EXIT=0;
  "[dst] OK: 0 corpus + 2000 random seeds, zero invariant violations";
  synthesis_dst 1 passed, settlement_dst 1 passed, daemon_smoke 2 passed.
  TIER: 2000 = the default read-path tier — appropriate: both commits are
  read-path/display-only. ("0 corpus" verified legitimate: dst-corpus/ has only
  README.md since T0.4; no corpus seed was ever committed or deleted.)
- C15 mechanical sweep (both diffs, added lines): PASS — every unwrap/expect/
  Instant::now hit is inside test files (integration tests against real Pg;
  wall-time bounds are the property under test). No f64-money, no HashMap/HashSet
  in shaped paths (BTreeMap), no secrets patterns, no place( sites, no GatedOrder
  construction outside fortuna-gates.
- C16 test-weakening sweep: PASS — the only deleted assertions
  (views.rs: asserted-absent rejections_by_check + is_number total) were replaced
  by strictly stronger ones (present-array, per-entry shape, sum==total as u64).
  No #[ignore], no proptest case reduction, no loosened tolerance anywhere in the
  two diffs.
- C17 protected crate: PASS — crates/fortuna-invariants/ absent from both commits'
  file lists; all invariant tests re-run green (C13).

## Findings

- [Minor] Slice-6's advertised consistency test is VACUOUS on the populated path.
  The seeded run (ticked_runner(10,2)) produces total_rejections=0, by_check=[]
  (probe output), so "counts SUM to total" passes as 0==0. Mutation reproduction:
  stubbing the new accessor to `BTreeMap::new()` (drop ALL counts) leaves
  fortuna-live views tests (4 passed) AND fortuna-runner sim_loop (11 passed)
  green — no test in the workspace catches a broken accessor->view flow. (sim_loop's
  nonzero by-check coverage reads the field via metrics_export(), not the accessor.)
  Fix is cheap: drive the views test with a config that forces >=1 rejection (the
  sim_loop edge-floor recipe) so sum==total is exercised nonzero. Ledger in GAPS.
- [Minor] The recorded rationale for omitting §5's per-check "number" is factually
  wrong. Commit message / design note / views.rs comment all claim "the runner keys
  by check NAME only, so a gate number would be a guess" — but GateCheck::index()
  (crates/fortuna-gates/src/pipeline.rs:75-86, "1-based pipeline position (spec
  numbering)") provides the exact number, in the crate the runner already depends
  on; the design example's EdgeFloor number 6 == GateCheck::EdgeFloor.index(). The
  omission itself is harmless display-conservatism and is documented; the
  justification should be corrected (or the field added exactly) so the ledger
  stays honest. Ledger in GAPS.
- [Minor] The R5 test pins the handler + pool-topology property, NOT the daemon
  wiring seam. It constructs its own reader/writer pools; a future refactor of
  crates/fortuna-live/src/main.rs:93-108 that wires the daemon's WRITER pool into
  RotaState.pool (the exact "silently merge the pools back" feared by the prior
  gate finding) fails no committed test. The prior finding asked for the
  handler-level test and got exactly that — this residual is recorded so the seam
  is ledgered, not forgotten. Ledger in GAPS.
- [Note, no severity] Err-arm logging uses eprintln rather than a structured logger —
  consistent with the crate's existing convention (main.rs:97); no action.

## Commands run (verbatim verdict lines; worktree /tmp/fortuna-g9 at 8a4fcb2)

- cargo fmt --check => FMT_EXIT=0
- cargo clippy --workspace --all-targets -- -D warnings => exit 0
  (forced re-lint of fortuna-ops/fortuna-live/fortuna-runner after touch => exit 0)
- cargo test --workspace => WORKSPACE_EXIT=0; 107x "test result: ok";
  passed=721 failed=0
- cargo test -p fortuna-invariants => all suites ok (13 tests + 3 doctests, 0 failed)
- scripts/run-dst.sh (N=2000) => DST_EXIT=0;
  "[dst] OK: 0 corpus + 2000 random seeds, zero invariant violations"
- cargo test -p fortuna-ops --test rota exhausted_rota_pool =>
  "test result: ok. 1 passed; 0 failed ... finished in 1.27s"
- merged-pool scratch variant (writer_pool = rota_pool.clone()) =>
  "panicked at crates/fortuna-ops/tests/rota.rs:350:6 ... PoolTimedOut" =>
  "test result: FAILED. 0 passed; 1 failed" [reverted]
- zero-case probe println => "SCRATCH_PROBE total_rejections=0 by_check=Array []"
  [reverted]
- accessor mutation (return BTreeMap::new()) => views "4 passed; 0 failed",
  sim_loop "11 passed; 0 failed" [reverted; git status --porcelain => 0 lines]
