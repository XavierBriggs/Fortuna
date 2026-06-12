# Review: track-b-cli-slice1 (T4.4 slice 1) — 2026-06-12
Base: 9466a08 (effective track-B base: merge-base 14288a0)  Head: 927a700
Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no

Range note: 9466a08..927a700 lists 3 commits, but 9942344 and 14288a0 are
already-gated MAIN commits (merge-base of main and track-b is 14288a0). The
track-B-authored work is exactly one commit, 927a700; all grading below is on
14288a0..927a700. Track-b's worktree has since advanced to bd4c5d7 — commits
after 927a700 are NOT covered by this gate.

Gate worktree: /tmp/fortuna-gb1 at 927a700, detached, removed after grading.
All mutations ran there, all restored (git status clean before removal).

## Criteria (fixed before reading the diff: docs/design/fortuna-cli.md incl.
## amendments A1-A10, BUILD_PLAN T4.4, docs/design/orchestration.md, skill checklist)

### A. Slice scope vs amended design
- A1 `config check`: PASS — calls fortuna_ops::FortunaConfig::load_file only,
  spawns nothing, exit 1 on bad/missing TOML, exit 0 + "config OK" on the
  example. Tests: config_check_rejects_bad_toml / _accepts_example /
  _missing_file_fails, all green; mutation (ignore validation result) turned
  2 of 3 RED.
- A2 `logs <daemon|recorder> [-f]`: PASS — log_path = $FORTUNA_RUNTIME_DIR/
  logs/<component>.log, default data/runtime/ (A5; data/ already gitignored,
  verified in .gitignore — no .gitignore edit needed); exec of
  `tail -n50 [-f]` per A4; unknown/missing component and missing file all
  fail informatively (4 tests green, incl. last-50-lines boundary L0011
  in / L0010 out).
- A3 `status` process health: PASS — pidfile `<pid>\n<name>` + ONE
  `ps -p <pid> -o comm=` call answering liveness and identity (A3
  amendment supersedes the body's bare kill -0); name mismatch / dead pid /
  malformed pidfile / pid<=0 ALL read stale-stopped (fail-closed: distrust
  never promotes). Section ordering process-health-before-DB is
  test-asserted (proc_at < db_at). Existing DB queries moved verbatim to
  status_db_section — extracted both versions and diffed: identical modulo
  move scaffolding, sole delta `&url`->`url` forced by the param type
  (checklist item 9 satisfied at query level). A9 behavior change (status
  without DATABASE_URL exits 0) IS in this slice and IS pinned:
  status_no_processes_no_db_exits_zero went RED under the reverted-behavior
  mutation. A7 stopping-marker display present and tested. A8 audit-age
  line correctly DEFERRED with a non-false-tell rationale (GAPS entry).
- A4 CUTS (A6): PASS — grep for migrate-status/allow-pending-migrations
  over crates/fortuna-cli: zero hits; no `mode` command arm (the one
  `"mode"` hit is the TOML key read inside the A6-prescribed
  "config on disk: venue/mode" status line, which is exactly what A6
  mandates INSTEAD of a mode command).
- A5 start/stop absent: PASS — grep '"start"|"stop"' in main.rs: zero hits.
  Build order matches the design §11 note (stop LAST, against T4.1's
  asserted SIGTERM contract).

### B. Tests-first / non-vacuous
- PASS (substance), sequence UNVERIFIABLE-by-construction: single commit
  carries tests+impl, so commit history cannot prove ordering. Per
  orchestration item 5 the standard is mutation: 3 mutation checks in the
  gate worktree all RED (name-validation disabled -> RED; A9 exit-0
  reverted -> RED; config validation ignored -> 2 tests RED). One vacuous
  surface found (finding 1). All design §9 + A9 test items applicable to
  this slice are present; the absent ones (stop_idempotent, recorder
  refusal, atomic claim, append redirect, SIGTERM gate) belong to the
  start/stop slices; mode/db-migrate-status tests are moot per A6.

### C. Ownership (orchestration)
- PASS — full file list of 14288a0..927a700: ASSUMPTIONS.md, BUILD_PLAN.md,
  Cargo.lock, GAPS.md, crates/fortuna-cli/{Cargo.toml,src/main.rs,
  tests/cli_integration.rs}. BUILD_PLAN edit is a PROGRESS block inside
  T4.4 only; GAPS/ASSUMPTIONS add new track-B sections only; Cargo.lock
  hunk touches only fortuna-cli's dep list (fortuna-ops per design §6 +
  toml, which was ALREADY a workspace dep — root Cargo.toml:29 — deviation
  ledgered in GAPS as required). No track-A/C file touched; no protected
  crate; no .env; no new binary; no clap; fortuna-killswitch absent from
  `cargo tree -p fortuna-cli`.

### D. Battery at 927a700 (gate worktree, shell DATABASE_URL unset ->
### cargo [env] dev default postgres://localhost/fortuna_dev)
- fmt: RED workspace-wide, exit 1 — ONE offender,
  crates/fortuna-venues/examples/record_kinetics_fixtures.rs:801, a
  TRACK-A-OWNED file byte-identical across this range (empty diff stat for
  fortuna-venues). Pre-existing at the merge-base; track-B's own files are
  fmt-clean (the fmt diff lists no other file). Correctly ledgered by the
  implementer in GAPS as a cross-track bus item. Not a track-B defect.
- clippy --workspace --all-targets -D warnings: PASS (exit 0).
- cargo test --workspace: PASS — exit 0, total passed=738 failed=0.
- fortuna-cli targeted: 15/15 green (cli_integration.rs).
- invariants per-test: PASS — i1 x2, i2 x2, i3, i4, i5, i6 x3, i7 x3 + 3
  doc/compile-fail tests, all ok.
- DST: scripts/run-dst.sh DEFAULT TIER (2000 seeds — CLI is a non-DST
  surface; default tier is the appropriate battery): exit 0; "[dst] OK: 0
  corpus + 2000 random seeds, zero invariant violations"; synthesis_dst,
  settlement_dst, daemon_smoke green.
- Mechanical sweeps over the diff: no unwrap/expect/panic/todo in
  src/main.rs (none at all, stricter than the binaries-may-anyhow rule);
  no SystemTime::now/Instant::now/Utc::now in new code (stopping_since
  reads file MTIME, not a clock); no #[ignore]/proptest reductions/deleted
  asserts; no secrets patterns; no place(/GatedOrder surface.

## Findings
- [Minor] A6 config-on-disk line is vacuously covered: mutating
  config_on_disk to unconditionally return "venue=FABRICATED" leaves all
  15 tests green (tests only assert the "config on disk:" prefix because
  they run where the default config path never exists). The implementation
  itself is correct — manual probe against config/fortuna.example.toml
  prints "config on disk: venue=sim (daemon may differ until restart)".
  Third overall occurrence of the vacuous-coverage class, but NOT a money
  surface, so it stays Minor per the standing calibration note. Fix: one
  status test writing a minimal TOML with [daemon] venue="sim" and passing
  --config-path, asserting "venue=sim" verbatim. Ledger in GAPS (track B).
- [Minor, cross-track — for the bus, owner TRACK A] workspace
  `cargo fmt --check` is red at HEAD on
  crates/fortuna-venues/examples/record_kinetics_fixtures.rs:801
  (pre-existing, untouched by this range; already self-reported in
  track-B's GAPS entry). One `cargo fmt` in fortuna-venues clears it; it
  will fail every track's battery until then.
- [Note, no action] status_db_section wraps fortuna_ledger::connect (which
  auto-migrates, per A6's own observation) in a 5s tokio timeout; a
  timeout drops a mid-flight migration future, which is safe (sqlx applies
  migrations transactionally) and the connect-in-status behavior is
  pre-existing, unchanged by this diff. The 5s bound + degradable posture
  is ledgered in GAPS and pinned by status_db_unreachable_still_exits_zero.

## Commands run (verbatim verdict lines)
- cargo fmt --check -> exit 1; offenders: "Diff in /private/tmp/fortuna-gb1/
  crates/fortuna-venues/examples/record_kinetics_fixtures.rs:801:" (sole hit)
- cargo clippy --workspace --all-targets -- -D warnings -> exit 0,
  "Finished `dev` profile [unoptimized + debuginfo] target(s) in 35.34s"
- cargo test --workspace -> "cargo test exit=0", "total passed=738 failed=0"
- cargo test -p fortuna-cli -> "test result: ok. 15 passed; 0 failed"
- cargo test -p fortuna-invariants -> all suites "test result: ok"
  (i4 32.63s, i5 10.21s included)
- ./scripts/run-dst.sh -> "dst exit=0", "[dst] OK: 0 corpus + 2000 random
  seeds, zero invariant violations"
- Mutation 1 (name validation disabled) ->
  "test status_name_mismatch_is_stale_not_running ... FAILED" (restored)
- Mutation 2 (A9 reverted) ->
  "test status_no_processes_no_db_exits_zero ... FAILED" (restored)
- Mutation 3 (config validation ignored) -> "config_check_missing_file_fails
  ... FAILED", "config_check_rejects_bad_toml ... FAILED" (restored)
- Mutation 4 (config_on_disk fabricated) -> "test result: ok. 15 passed"
  (finding 1; restored)
- Query-move check -> "QUERIES IDENTICAL (modulo move scaffolding)"
- Live probes: status (no DB) exit 0 / "venue=sim"; status (dev DB) exit 0,
  "halts: none"

## Merge recommendation
MERGE 927a700 into main (ACCEPT-WITH-GAPS; finding 1 to track-B's GAPS
ledger, finding 2 to the bus for track A). Run the post-merge integration
check per orchestration item 3 after merging.
