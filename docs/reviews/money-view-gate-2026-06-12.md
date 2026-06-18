# Review: T4.3-rota-slice7-money-view — 2026-06-12
Base: 4e8c48f  Head: a0ca008  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no

Scope: single commit a0ca008 ("T4.3 ROTA slice 7: money view — SIM-ONLY subset"),
gated in a detached worktree (/tmp/fortuna-g10) — the dirty working tree was not
consulted. Files: views.rs +41, tests/views.rs +55/-12, runner.rs +8/-0, plus
BUILD_PLAN/GAPS/design-doc notes. Rubric fixed before reading the diff: ROTA design
§5 money contract + AMENDMENTS R6/R8 at parent commit, BUILD_PLAN T4.3, fortuna-review
checklist, operator gate instructions A1-A5/B.

## Criteria (fixed before reading the diff)

- A1 Money identity honest, every number traced (R6): PASS — view emits
  {basis, settled_cents, committed_cents, floating_cents, total_cents, positions}.
  settled_cents <- boards["account"]["cash_cents"] <- SimVenue::inspect_totals().0
  = st.cash (sim.rs:289-292) via runner.rs:2409-2415; committed_cents <- .1 =
  st.reserved, which is a HOLD against cash (available = cash - reserved,
  sim.rs:663-666), so committed ⊆ settled exactly as R6 defines ("committed is
  informational (subset of settled)"). positions reshaped from boards positions
  (PositionBook, runner.rs:2391-2399) at views.rs:84-104 with §5 names
  yes_qty/no_qty. floating_cents and total_cents are literal Value::Null
  (views.rs:138-139) — NOT a fabricated 0; the mark loop (floating's only source,
  per the prior gate's finding that inspect_totals is (Cents,Cents,usize,usize)
  with no floating component) is not exposed. No fake-floating Major present.
  Instrumented execution (seed 11, 3 ticks): settled_cents=995639 (moved from the
  1,000,000 start by real fills+fees), 3 positions yes_qty=50, fees 66/71/74 —
  the served numbers are real venue state, verified by execution.
- A2 SIM-ONLY labeling + live-venue degrade (R6): PASS — "basis": "sim-only"
  (views.rs:135), asserted by the test; the shell renders the full view JSON
  verbatim (rota.rs ~341: JSON.stringify(j,null,2) into the Money panel), so the
  label reaches the operator. The live-venue path is unreachable today by
  construction: SimRunner.venue is concretely SimVenue (runner.rs:107); no live
  balance is claimed anywhere. Missing-account degrade: Value indexing yields
  null, never a fabricated number, never 500.
- A3 Integer cents end to end: PASS — cash.raw()/reserved.raw() are i64
  (money.rs:44); Contracts qty i64 (market.rs:121-126); realized_pnl/fees via
  .raw() i64 (runner.rs:2395-2396). Diff-wide grep: zero f32/f64 hits.
- A4 Read-path only; R8; GET-only routes: PASS — runner.rs change is +8/-0 inside
  boards_json (a &self read method) calling &self inspect_totals; no
  gates/exec/state enforcement line touched. read_view clones out of the snapshot
  RwLock in a scoped block and releases before responding (rota.rs:72-84, R8).
  Route table unchanged; every_path_is_get_only_and_200,
  dashboard_serves_metrics_boards_and_shell_read_only, and
  serve_dashboard_mounts_the_rota_console_alongside_the_instrument all ok in the
  workspace run.
- A5 Matches the endorsed unblock, no overreach: PASS — ships exactly the SIM-ONLY
  subset: real settled/committed, honest nulls for floating/total, strategies[]
  OMITTED (not faked; per-strategy attribution still open), GAPS.md re-ledgered
  with the remaining full-§5 items as an operator/design call, BUILD_PLAN box
  stays unticked. No live-venue money claimed.
- B1 fmt: PASS — `cargo fmt --check` exit 0.
- B2 clippy: PASS — `cargo clippy --workspace --all-targets -- -D warnings` exit 0.
- B3 workspace tests: PASS — `cargo test --workspace` exit 0; 107 suites, 722
  tests, 0 failed.
- B4 invariants per-test: PASS — I1 2/2, I2 2/2, I3 1/1, I4 1/1, I5 1/1, I6 3/3,
  I7 3/3 + 3 doctests, all ok, 0 failed.
- B5 DST (default 2000, read-path tier): PASS — exit 0. "[dst] OK: 0 corpus +
  2000 random seeds, zero invariant violations"; synthesis-dst 2000 scenarios ok;
  settlement-dst 2000 scenarios (11 arms, 368 discrepancies, 381 halts) ok;
  daemon_smoke 2 passed/0 failed. (Informational, pre-existing: dst-corpus/
  contains only README.md — zero committed regression seeds.)
- B6 mechanical + test-weakening sweep: PASS — diff-wide: no unwrap/expect/panic
  outside tests (one expect( in the new test, allowed), no SystemTime/Utc::now,
  no #[ignore], no proptest reductions, no secrets, no place( changes. One
  deleted assertion (`v.get("money").is_none(), "money is a later slice"`) is
  legitimately obsoleted — the money view now exists and a dedicated test
  replaced it. Not test weakening.
- B7 money test pins real numbers: FAIL — see Finding 1. The view is correct
  (verified by execution), but the suite cannot prove it.
- B8 protected crate: PASS — crates/fortuna-invariants/ untouched (diff stat).

## Findings

- [Minor] The slice-7 money test is VACUOUS on values — it cannot distinguish the
  real money panel from a fully fabricated one. Reproduction (mutation test in
  the disposable worktree, reverted after): hardcoding views.rs to
  `"settled_cents": 0, "committed_cents": 0, "positions": []` leaves the ENTIRE
  workspace green (`cargo test --workspace` exit 0, zero failing suites). The
  test asserts only is_number()/is_null()/shape; the positions for-loop runs zero
  assertions on an empty array; the seeded run (seed 11, 3 ticks) has
  committed=0 and realized_pnl=0, so even those values never see non-zero. No
  test anywhere asserts the account block's numbers (grep: cash_cents appears
  only in runner.rs + views.rs). This is the SECOND occurrence of the
  vacuous-populated-path class (slice-6 finding 1, GATE-FINDINGS-LATEST item 1 —
  published ~03:30Z, 7 minutes AFTER this commit, so not chargeable to it; the
  class still recurs). Severity calibrated Minor to match the prior gate's
  identical-class grading; on a money-bearing surface a third occurrence should
  escalate. Required fix (ledger in GAPS): assert settled_cents equals the
  venue's actual cash (inspect_totals/boards ground truth), assert positions
  non-empty with the known quantities (seed 11/3 yields 3 positions, yes_qty=50,
  fees 66/71/74, settled=995639), and add a seed with reserved > 0 (ack-delayed
  pending order) so committed_cents is pinned non-zero; a settled-position seed
  for non-zero realized_pnl_cents completes it.
- [Informational] dst-corpus/ holds zero regression seeds (README only) — the
  "replays every regression seed" arm of run-dst.sh is currently a no-op.
  Pre-existing, not introduced by this commit.
- [Informational] GATE-FINDINGS-LATEST fix-list items 1-3 (slice-6 vacuous test,
  R5 pool construction-site test, gates "number" field) are unaddressed here;
  the commit timestamp (2026-06-12 03:23Z) predates the findings update
  (~03:30Z), so they remain queued for the implementer, not violations of this
  commit.

## Commands run (verbatim results)

- `git worktree add --detach /tmp/fortuna-g10 a0ca008` — HEAD is now at a0ca008
- `cargo fmt --check` — exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` — exit 0, Finished dev profile
- `cargo test --workspace` — exit 0; 107 "test result:" lines, 722 passed, 0 lines with failures
- `cargo test -p fortuna-invariants` — i1..i7 all ok; 13 tests + 3 doctests, 0 failed
- `./scripts/run-dst.sh` — exit 0; "[dst] OK: 0 corpus + 2000 random seeds, zero invariant violations";
  "[synthesis-dst] master seed 1781236023792 -> 2000 scenario(s)";
  "[settlement-dst] master seed 1781236057196 -> 2000 scenario(s)"; daemon_smoke "2 passed; 0 failed"
- Instrumented test print (then reverted): SCRATCH-MONEY-VIEW settled_cents=995639,
  committed_cents=0, floating/total null, 3 positions yes_qty=50
- Mutation (then reverted): fabricated zeros + empty positions in views_from ->
  `cargo test --workspace` exit 0 (vacuousness proven)
- `git status --porcelain` in worktree after reverts — clean
