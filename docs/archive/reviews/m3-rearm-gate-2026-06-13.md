# Review: M3 re-arm notices (track-A completion-queue item 1) — 2026-06-13

Base: 5b8b9ce  Head: 54ca2c5 (gated commit; main HEAD 8e8c348 is a later bus-doc commit, not gated)
Verdict: ACCEPT
Protected crate touched: no (fortuna-invariants untouched, confirmed)

This gate CERTIFIES a commit already on main as track A's own work. ACCEPT = certified.
A BLOCK would have required track A to fix-forward (not revert), per the gate charter.

## Rubric grades
- A. I2 CORRECTNESS .......... PASS
- B. TESTS WITH TEETH ........ PASS (both tests killed by mutation; both edges pinned)
- C. SCOPE / OWNERSHIP ....... PASS
- D. HONESTY BOUND .......... PASS
- E. BATTERY ................ PASS (fmt/clippy/workspace/invariants/DST-2000 all green; no test weakening)

## Criteria (fixed before reading the diff; from halt-and-rearm.md four-state table,
## CLAUDE.md I2, track-a-completion-queue.md item 1)

- A1 CLI rearm prints restart guidance (queue item 1 line 7-9): PASS
  evidence: crates/fortuna-cli/src/main.rs:1085 prints rearm_success_message(); the pure
  fn (lines 1097-1103) emits "halt cleared in the ledger; the RUNNING daemon resumes only
  on restart — run: fortuna stop && fortuna start". The rearm arm (lines 1074-1086) calls
  halts.record_rearm() + audit.append() ONLY — ledger-only, no daemon/poller contact.

- A2 health view rearm_requires_restart TRUE exactly when running daemon halted, derived
  from running halt state (NOT a fabricated ledger-vs-running divergence the pure
  views_from cannot compute): PASS
  evidence: crates/fortuna-live/src/views.rs:65 halt_active = runner.active_halt().is_some()
  (active_halt reads self.gates.halts().global_halted(), runner.rs:537-539 — the RUNNING
  daemon's gate state); views.rs:127 "rearm_requires_restart": halt_active. views_from
  takes only &SimRunner + timestamp; it is pure (no DB pool), so the field is the honest
  always-true-when-halted I2 fact, not a divergence claim. Matches the adjudicated
  option (a) (GATE-FINDINGS R12 finding lines 185-191; ASSUMPTIONS T4.1).

- A3 ROTA/rota health panel renders it + halt overlay carries it: PASS
  evidence: crates/fortuna-ops/src/rota.rs:517 health() renders kv("re-arm","takes effect
  only on restart: fortuna stop && fortuna start") when j.rearm_requires_restart; the
  #halt overlay (rota.rs ROTA_SHELL) now carries the restart instruction inline. /rota
  200 serving guarded by fortuna-ops tests/dashboard.rs:135 (serve_dashboard_mounts_the_
  rota_console..., ran green). JS template content not unit-tested — disclosed, Minor.

- A4 (CRITICAL) NO code path clears a running halt on rearm; rearm stays ledger-only;
  running daemon resumes only on restart: PASS
  evidence: full-diff grep for clear/resume/unhalt/halt=false/apply_external — the only
  matches are in COMMENTS/STRINGS describing semantics; zero code path clears a running
  halt. run_loop.rs:126-140 (NOT touched by this diff): on Ok(None) the poller resets only
  the dedup latch (*last_halt = None), never resumes. SimRunner has apply_external_halt
  (SET only, runner.rs:523) and no inverse. Pinning test
  a_running_daemon_never_auto_clears_a_halt_on_rearm_only_a_restart_does ran GREEN.

- B1 rearm_message_tells_the_operator_to_restart has teeth: PASS
  mutation: strip restart guidance from rearm_success_message => RED, panicked at
  main.rs:1124 "must tell the operator a restart is required". Restored, re-verified.

- B2 a_halted_health_view_flags_that_rearm_requires_a_restart has teeth (BOTH edges): PASS
  mutation force-false (halt_active -> false): RED, views.rs:132 "a halted running daemon
  must flag that a re-arm takes effect only on restart". mutation force-true (always true):
  RED, "a clear daemon must not claim a restart is required". Field is pinned to halt
  state exactly, not a constant. Restored, worktree verified clean (git diff empty).

- C1 scope = fortuna-cli + fortuna-live + fortuna-ops (+ GAPS.md), all track-A-permitted: PASS
  evidence: git show --stat: only those three crates; GAPS.md the only non-crate file.
  Queue item 1 explicitly releases the fortuna-cli touch and the ROTA surface to track A.

- C2 no protected crate, no other crate, no test weakening: PASS
  evidence: fortuna-invariants absent from --stat. Test-weakening sweep diff-wide: zero
  removed lines in test files, zero #[ignore]/proptest-reduction/removed-assert/loosened-
  tolerance, zero added skip/ignore.

- D1 honesty bound: the views_from purity limitation is stated, not hidden; richer
  ledger-vs-running comparison ledgered: PASS
  evidence: GAPS.md DESIGN NOTE (added in this commit): "views_from is PURE (no DB), so it
  cannot compute the TRUE ledger-vs-running divergence (that needs the R5 read pool)... A
  richer ledger-vs-running comparison... is a possible later enhancement, ledgered here."

- E1 fmt: PASS — `cargo fmt --check` exit 0
- E2 clippy -D warnings: PASS — `cargo clippy --workspace --all-targets -- -D warnings` exit 0
- E3 workspace tests: PASS — `cargo test --workspace` exit 0; 110 "test result: ok" lines;
  0 failed, 0 ignored; both M3 tests ran ok.
- E4 invariants per-test (I2 emphasis): PASS — `cargo test -p fortuna-invariants` all green,
  i2_drawdown_human_rearm 2/2.
- E5 run-dst.sh default 2000 tier (non-DST surface; regression guard): PASS — exit 0,
  "[dst] OK: 3 corpus + 2000 random seeds, zero invariant violations"; synthesis_dst,
  settlement_dst, daemon_smoke all green.

## Findings
- [Minor] ROTA JS render path (rota.rs:517 health() + #halt overlay) is not content-tested;
  only the upstream data field (views.rs) and /rota 200 serving are. Disclosed in the
  commit body and GAPS. Presentation-layer gap, no money-path effect. Already ledgered;
  no action required for ACCEPT. reproduction: n/a (acknowledged limitation, not a defect).

No Critical, no Major.

## Commands run (verbatim results)
$ cargo fmt --check                                            -> FMT_EXIT=0
$ cargo clippy --workspace --all-targets -- -D warnings        -> CLIPPY_EXIT=0 (Finished)
$ cargo test --workspace                                       -> TEST_EXIT=0
    grep -c "test result: ok" -> 110 ; zero failed, zero ignored
    rearm_message_tells_the_operator_to_restart ... ok
    a_halted_health_view_flags_that_rearm_requires_a_restart ... ok
$ cargo test -p fortuna-invariants                             -> all ok (i2 2/2)
$ cargo test -p fortuna-live --test run_loop                   -> 9 passed; pinning test ok
$ bash scripts/run-dst.sh 2000                                 -> DST_EXIT=0
    [dst] OK: 3 corpus + 2000 random seeds, zero invariant violations
    synthesis_dst/settlement_dst/daemon_smoke: 15 passed
MUTATION (CLI, strip restart text)  -> RED at main.rs:1124 (restored, clean)
MUTATION (views, force false)       -> RED at views.rs:132 (restored, clean)
MUTATION (views, force true)        -> RED clear-branch "must not claim a restart" (restored, clean)
git show 54ca2c5 --stat: crates/fortuna-cli, crates/fortuna-live, crates/fortuna-ops, GAPS.md
fortuna-invariants: NOT touched

## Merge note
M3 is already on main as track A's own commit (54ca2c5). This gate CERTIFIES it: verdict
ACCEPT. The implementation lands the adjudicated option (a) — document restart-gated
rearm on the CLI + ROTA surfaces — and introduces NO auto-resume path. The prior open M3
finding (GATE-FINDINGS-LATEST.md item 3 / R12 finding) is RESOLVED on both missing
surfaces. The halt-and-rearm.md four-state table is now readable off the console.
