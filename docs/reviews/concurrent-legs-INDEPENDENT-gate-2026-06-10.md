# Review: concurrent-legs-INDEPENDENT-gate — 2026-06-10
Base: 9b244ee  Head: 10443c5  Scope: 5a1581a (concurrent leg submission), cc3bde0 (tick bench), full regression battery at head
Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (`git diff 9b244ee...10443c5 -- crates/fortuna-invariants/` is empty)

Independence note: docs/reviews/ was not read. cc3bde0 adds
docs/reviews/f-batch-INDEPENDENT-gate-2026-06-10.md; its presence is recorded,
its content was not opened.

## Criteria (fixed from spec 5.1/5.2/5.3/5.4 + CLAUDE.md before reading the diff)

- C1 Phase-split spec compliance (spec 5.1, 5.4): PASS — Spec 5.1: "Single-threaded
  deterministic message bus for all trading-relevant events ... with tokio runtimes for
  network IO feeding into it. Deterministic event ordering is what makes replay and
  audit meaningful." Concurrency is confined to venue network IO (Phase 1 join_all);
  every journal write and outcome-processing step is sequential in leg order (manager.rs
  phases 0/2, runner.rs phases A/C). This is the edge-IO concession the spec itself
  makes, not a divergence; the doctrine interpretation is visibly ledgered in
  ASSUMPTIONS.md lines 6-16 ("Concurrent leg submission preserves the determinism
  doctrine"). Replay evidence: sim_loop::same_seed_same_script_byte_identical_recording
  ok over the 3-leg concurrent path; 10,000-seed synthesis DST (replay criterion
  "recording differs on replay" never fired) clean.
- C2 Sealed GatedOrder, no bypass (spec 5.2 "place accepts only GatedOrder"): PASS —
  place( sweep over the 5a1581a diff: only `venue.place(order.clone())` where order is
  `fortuna_gates::GatedOrder` (staged: Vec<(usize, fortuna_gates::GatedOrder)>), plus
  the test mock's `place(&self, order: fortuna_gates::GatedOrder)`. No new constructor,
  no From/Into. i1_universal_gate + i1_prop_all_orders_carry_gate_verdicts ok.
- C3 Journal-before-network (spec 5.4 "persisted ... BEFORE any network call"): PASS —
  Phase 0 journals Created + SubmitAttempted for EVERY leg before the first
  venue.place future is created (manager.rs submit_group_concurrent, Phase 0 loop ends
  before the join_all expression). Crash between persist and submit is a standing DST
  arm: dst.rs:414-416 `created_only_crash` (2% arm, "spec 5.4: crash between intent
  persistence and submission. Boot closes it later") plus final crash_and_recover
  resolving every Submitted-unknown intent (dst.rs:524-525); 10,000 seeds, zero
  invariant violations. Client order ids derive from intent ids (idempotent resubmit).
- C4 Deterministic outcome processing (spec 5.1 deterministic ordering): PASS —
  join_all output preserves input order; Phase 2 iterates `staged` (leg order) zipped
  with placements, so completion order cannot reach the journal. Proof test asserts
  outcomes align to leg order and each intent journals to Acked. Byte-identity:
  sim_loop::same_seed_same_script_byte_identical_recording ok (3-leg group through the
  concurrent path); full DST 10,000 x3 stages clean. Caveat -> Finding 2 (no
  reversed-completion-order mock; the guarantee is structural).
- C5 All-or-nothing abort + in-flight coverage (spec 5.4 IntentGroup): PASS — Runner
  Phase A: any gate rejection on a grouped proposal sets group_rejected, releases every
  staged reservation, returns with nothing submitted. Test
  a_mid_group_gate_rejection_submits_nothing_and_releases_reservations: ok (asserts
  orders_submitted == 0 AND fortuna_reserved_exposure_cents == 0). This UPGRADES the
  old path, which only aborted when the FIRST leg rejected. Post-network venue
  rejection of one leg: rejected leg journals Rejected, reservation released, group
  opens with acked legs only and the pre-existing unwind machinery
  (decide_complete_or_unwind; exec tests groups_flatten.rs:
  unwind_when_completion_edge_is_gone, unwind_when_depth_is_insufficient_or_book_missing,
  taker_complete_when_edge_still_clears_floor) is untouched by the diff and green.
  Note (info, not a finding): a HARD journal failure mid-group propagates out of
  handle_proposal without releasing staged in-memory reservations — identical shape to
  the old path's `Err(e) => return Err(e.into())`; bounded by reservations rebuilt at
  boot as derived state (spec 5.14).
- C6 Rate limits I3 per leg (spec 5.3 gate 7): PASS — Phase A runs evaluate_gates per
  leg inside the loop before anything is staged; buckets are stateful BTreeMaps owned
  by the pipeline (pipeline.rs:154-155, venue_buckets + market_buckets); the diff does
  not touch fortuna-gates. Tokens are consumed at gate time, before concurrency exists.
  i3_runaway_halt ok.
- C7 Proof test (3 legs in flight, single-threaded executor): PASS —
  concurrent::group_legs_place_concurrently_and_outcomes_keep_leg_order uses
  futures::executor::block_on and a YieldOnce x2 mock venue; asserts
  venue.max_seen() == 3 ("legs must overlap at the venue"). Ran: ok (3/3 in the
  concurrent module, incl. rejected-leg-keeps-slot and collision-never-touches-venue).
- C8 Mechanical sweep over 5a1581a: PASS — added src lines contain zero
  unwrap/expect/panic!/todo!/unimplemented!/SystemTime/Instant::now/Utc::now/f32/f64
  (grep exit 1 = no hits); the only `.iter()` additions are over ordered Vec `staged`;
  no HashMap/HashSet iteration feeds journaling; secrets sweep clean (only
  sort_by_key false positives).
- C9 cc3bde0 bench discipline: PASS — Instant::now confined to
  crates/fortuna-runner/tests/sim_loop.rs (test code, outside src); the 50ms bound is
  an ADDED assert (sanity only); GAPS.md gains 5 ledgered minors, deletes nothing.
  Ran: prints [tick-bench] lines, ok. (Debug-build numbers 46us/461us vs the commit's
  release-build 9us/62us claims — expected, not a finding; the bound passes.)

## Findings

- [Minor] No seeded DST arm drives multi-leg (>1) groups through
  submit_group_concurrent under randomized venue faults. The seeded corpora propose
  single legs only (settlement_dst.rs:185-193 `group_policy: None`; synthesis is
  single-leg per the GAPS "Kelly sizing keys off legs[0]" note), so chaos coverage of
  the distinctive multi-leg overlap is deterministic-test-only (exec concurrent suite,
  sim_loop arb tests). Ledger in GAPS.md; add a multi-leg arm to the DST scenario set
  (CLAUDE.md definition-of-done item 3 spirit).
- [Minor] No test exercises legs COMPLETING in non-input order: OverlapVenue's
  symmetric YieldOnce x2 means completion order == poll order, and SimVenue::place has
  no internal await points. Leg-order journaling under out-of-order completion rests on
  join_all's output-order guarantee — structurally sound, but a staggered-yield mock
  (leg 3 completes first) would pin it against future refactors. Ledger in GAPS.md.

## Commands run (verbatim verdict lines; worktree /tmp/fortuna-cleg at 10443c5, removed after)

- git diff 9b244ee...10443c5 -- crates/fortuna-invariants/  -> empty (protected crate untouched)
- cargo fmt --check  -> exit 0
- DATABASE_URL=postgres://localhost/fortuna_dev cargo clippy --workspace --all-targets -- -D warnings
  -> "Finished `dev` profile [unoptimized + debuginfo] target(s) in 46.25s", exit 0
- cargo test --workspace -> 88 result lines, "654 passed total", "0 failed total" (matches the 654/0 claim)
- cargo test -p fortuna-invariants -> 13 integration tests ok (i1 x2, i2 x2, i3, i4, i5, i6 x3, i7 x3) + 3 doctests ok, 0 failed
- cargo test -p fortuna-exec --test manager concurrent -- --nocapture
  -> "test result: ok. 3 passed; 0 failed" (proof test included)
- cargo test -p fortuna-runner --test sim_loop a_mid_group_gate_rejection_submits_nothing_and_releases_reservations -- --exact -> ok
- cargo test -p fortuna-runner --test sim_loop same_seed_same_script_byte_identical_recording -- --exact -> ok
- cargo test -p fortuna-runner --test sim_loop tick_wall_time -- --nocapture
  -> "[tick-bench] steady-state: 46us/tick over 2000 ticks"
     "[tick-bench] full trade tick (scan+gates+3 submits+fills): 461us avg over 200 runs" ... ok
- scripts/run-dst.sh 10000 -> exit 0:
     "[dst] regression corpus: 0 seed(s)"
     "[dst] master seed 1781151301505 -> 10000 random scenario(s)"
     "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations"
     "[synthesis-dst] master seed 1781151695947 -> 10000 scenario(s)"
     "[synthesis-dst] totals: 27351 orders, 42064 proposals, 133259 cognition failures, 118838 beliefs"
     "test synthesis_loop_survives_cognition_chaos_on_every_seed ... ok" (98.71s)
     "[settlement-dst] master seed 1781151798155 -> 10000 scenario(s)"
     "[settlement-dst] arms {SettleClean: 1025, SettleThenCorrect: 966, Void: 1014, Dispute: 1029, VenueMismatch: 1032, CanonicalDivergence: 955, OrphanScan: 1075, AuditDeath: 967, WideBook: 961, Overdue: 976}; 1987 discrepancies, 3080 watchdog rows, 1999 halts"
     "test settlement_and_watchdog_paths_survive_seeded_chaos ... ok" (13.81s)
- Test-weakening sweep (whole range, crates/): 0 deleted assert lines, no #[ignore],
  no ProptestConfig/case reductions; 55 deleted lines total (runner rewiring).
