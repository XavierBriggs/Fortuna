# Review: track-b FINAL cumulative gate (T4.4 slices 2+3, T4.3 slices 8+9, RALPH STOP) — 2026-06-12
Base: 9f042c3 (main; merge-base 2d5c31c)  Head: 399d3fa  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: NO (git diff main...track-b -- crates/fortuna-invariants/ = 0 lines)

Range gated: main..track-b = 5 commits (02b5079 start, 05bbc17 stop+TICK,
16be4ed cognition, a4fac10 presentation+logo, 399d3fa RALPH STOP docs-only).
Worktree /tmp/fortuna-gb (detached @ 399d3fa), CARGO_TARGET_DIR=/tmp/fortuna-gate-target.
Disk at start: 16Gi free (above the 10Gi floor). Live recorder pid 79813 verified
intact BEFORE and AFTER the entire battery and all mutations — never signaled.

## Criteria (fixed before reading the diff: CLI amendments A1-A10, ROTA R1-R12,
## BUILD_PLAN T4.3/T4.4 text, T4.1 SHUTDOWN CONTRACT, orchestration ownership)

### A. fortuna start (slice 2)
- A-1 A2 unmanaged-recorder refusal: PASS — pgrep -f fail-closed (pgrep failing to
  run = refusal, main.rs:420-432); refusal is deterministic via a PLANTED DECOY whose
  script path contains "fortuna-recorder" (cli_integration.rs:377-402) — the test
  spawns/kills only its OWN children (ChildGuard); pid 79813 never touched (verified
  alive post-battery). Migration instruction text asserted. Mutation M2 (check
  neutralized) => start_refuses_on_unmanaged_recorder FAILED. Killed.
- A-2 A3 atomic claim: PASS — OpenOptions create_new (O_EXCL) claim BEFORE spawn
  (main.rs:293-298, 320-353); EEXIST => classify_existing Running/Stale/MidClaim
  (validate-then-decide, MidClaim never stolen). Race test run:
  claim_pidfile_race_has_exactly_one_winner (8 threads) ok. Mutation M1
  (create_new=>create) => race test FAILED (winners=8). Killed.
- A-3 A4 detach: PASS — process_group(0) via std CommandExt (NO nix dep: grep
  crates/fortuna-cli/Cargo.toml = 0 hits), stdin(Stdio::null()), APPEND-mode logs
  (log_redirection_appends_and_never_truncates ok); spawn-failure releases claim
  (spawn_component_releases_claim_on_spawn_failure ok).
- A-4 A1 wiring (config-check gate): PASS — start_fails_config_check_first ok;
  refused start claims no pidfiles (asserted).
- A-5 A8 active-halts print: PARTIAL — implemented (start_db_section, main.rs:568-584,
  5s-bounded, best-effort) but no automated test (DB-backed path); covered only by
  the §13 manual runbook step 3. See F-3.
- A-6 A2 cwd pin: DIVERGENCE — see F-2 (Minor).

### B. fortuna stop (slice 3)
- B-1 A1 log-confirmed shutdown: PASS — marker "fortuna-live: clean shutdown" is
  REAL (fortuna-live/src/main.rs:224 prints it on the graceful path); stop captures
  the append-log byte offset BEFORE SIGTERM and accepts the marker only at/after it
  (main.rs:689-733); exit-without-marker = warning + exit 1; pre-seeded stale marker
  rejected. Tests: stop_graceful_daemon_confirms_log_line_and_cleans_up,
  stop_exit_without_shutdown_line_is_not_success,
  stop_ignores_pre_existing_marker_lines_in_the_append_log — all ok. Mutation M3
  (log confirmation skipped) => BOTH A1 tests FAILED. Killed. Ship-gate dependency:
  T4.1 SIGTERM contract test-asserted and RUN green this session
  (daemon_smoke 2 ok, shutdown 2 ok, incl. signal_with_working_orders_cancels_them_and_audits).
- B-2 A7: PASS — default --timeout-secs 60 (main.rs:645-646: None => 60); NEVER
  SIGKILL: grep SIGKILL/kill -9/.kill( across cli+ops src = only doc comments and one
  #[cfg(test)] cleanup kill of a test-spawned `sleep` (main.rs:1157, commented as
  such); timeout => verbatim guidance "daemon is cancelling working orders — do NOT
  kill -9; watch `fortuna logs daemon`; if the venue is unreachable use `fortuna
  kill`" (main.rs:710-714, text-asserted), .stopping marker + pidfile left, STILL
  proceeds to recorder, exit 1 (stop_timeout_warns_proceeds_and_leaves_state ok;
  TERM-ignoring stub asserted alive after stop). Name-mismatched pid NEVER signaled
  (stop_never_signals_a_name_mismatched_pid ok). Zombie reads as exited
  (zombie_child_reads_as_not_running ok).
- B-3 Idempotent: PASS — stop_is_idempotent_when_nothing_runs ok (exit 0, both
  components "already stopped").

### C. T4.4 completion accounting (box TICKED at 05bbc17)
- A1 PASS (B-1). A2 PASS w/ F-2 cwd note + pinned invocation (30s,
  KXBTC15M,KXBTC,KXBTCD, out-dir absolutized; [recorder] override read — example-file
  gap ledgered in GAPS). A3 PASS. A4 PASS. A5 PASS (data/runtime; .gitignore:8
  `data/`). A6 PASS (mode + db migrate-status absent from usage/arms; config-on-disk
  one-liner asserted; metrics-poll Section 3 deferral DECLARED in the tick — but see
  F-4). A7 PASS. A8 PARTIAL — halts print untested (F-3) and the "age of the most
  recent audit row" line is NOT IMPLEMENTED and NOT ledgered (F-1). A9 PASS — every
  named test exists and ran green: live pidfile (status_shows_live_pidfile_as_running),
  written-then-killed (status_dead_pid_is_stale_not_running), name-mismatch
  (status_name_mismatch_is_stale_not_running), recorder refusal
  (start_refuses_on_unmanaged_recorder), claim race
  (claim_pidfile_race_has_exactly_one_winner), append redirect
  (log_redirection_appends_and_never_truncates), status-no-DATABASE_URL-exit-0
  (status_no_processes_no_db_exits_zero), A1 SIGTERM ship gate (fortuna-live
  daemon_smoke + shutdown, run green). A10 PASS — best-effort lifecycle rows, 5s
  bound, dead DB never blocks (code-verified both paths).
- Five-command scope: PASS — start/stop/status/logs/config check all present, 38
  crate tests (11 unit + 27 integration) all green this session.
- Tick honesty: the tick text claims nothing false; the A8 age-line omission is the
  one unledgered element => the tick stands WITH F-1 required in GAPS.

### D. ROTA slice 8 (cognition / R7)
- D-1 BeliefsRepo::recent: PASS — ORDER BY belief_id DESC, limit clamp [1,500],
  evidence+provenance JSONB (repos.rs:1022-1053); populated-path test with REAL
  values (p=0.70, evidence-2, cost_cents 14, newest-first, limit=2). Mutation M4a
  (DESC=>ASC) => beliefs_recent_lists_newest_first... FAILED. Killed.
- D-2 CalibrationParamsRepo::scopes: PASS — DISTINCT ON (model,strategy,category,kind)
  ... ORDER BY version DESC (repos.rs:1338-1362); test pins max-version-wins (v2) and
  two distinct scopes; empty-DB => empty vec test.
- D-3 sqlx prepare: PASS — exactly 2 new .sqlx JSONs committed; verified each contains
  the corresponding new SQL (grep hash files). The 39 stale entries claim: 0 untracked
  .sqlx files at this head; owners' refresh remains a main-side chore.
- D-4 Evidence display + 4KB truncation: PASS — truncate_evidence (rota.rs:107-121,
  char-boundary-safe, truncated:true + bytes_total + preview);
  cognition_truncates_evidence_over_4kb ok. Mutation M4b (truncation disabled) =>
  test FAILED. Killed.
- D-5 Populated-path rule: PASS — seeded sqlx::test with non-zero real values in BOTH
  the ledger tests and the ops handler test (cognition_serves_seeded_beliefs_and_scopes).
  Not vacuous; two mutations killed across the query/view pair.
- D-6 R8 lock rule: PASS — read_view and view_cognition clone out of the RwLock in a
  block and release BEFORE any await/query (rota.rs:75-88, 140-146 incl. the comment);
  view_streams likewise.
- D-7 Degraded paths: PASS — cognition_degrades_without_pool_but_stays_200 ok;
  counters absent => explicit counters_status:"unavailable" (never zeros);
  counters merge test ok; each ledger array degrades independently with neutral
  detail (raw sqlx text never leaked to the client).
- D-8 RotaState §3 budget fields deliberately omitted: declared in GAPS + design doc
  (track A's struct-literal construction site). Accepted as ownership-correct.

### E. ROTA slice 9 (presentation + §9 logo)
- E-1 Section 2 tokens: PASS — verified in ROTA_SHELL css: #0A0A0B bg, #141416 card,
  #D4AF37 gold, #FFB84D amber, #EDEDEA text, #FF3B30 halt (used only for halt/breach
  pills + takeover), #30D158 ok; JetBrains Mono/ui-monospace + tabular-nums lining-nums;
  no gradients (test-asserted on the mark).
- E-2 R11: PASS — grep: exactly 1 `auto-fit` (grid-template-columns:repeat(auto-fit,
  minmax(320px,1fr))), 0 `@media`.
- E-3 §9 logo: PASS — assets/rota/logo.svg: viewBox 0 0 48 48, wheel circle r=20
  @(24,24) stroke #D4AF37 sw=2, hub r=4 filled, EIGHT spokes sw=1 at 45° steps,
  cornucopia tip (14,14) curving to lower-right, mouth ellipse rx=5 ry=3
  rotate(45 34 34), all gold, no gradients. include_str! bake-in (missing asset =
  build error). Test logo_asset_serves_the_section9_geometry pins viewBox, 8 <line>,
  ellipse, gold, no-gradient. Favicon: 204 stub REPLACED with 200 + image/svg+xml +
  wheel-markup asserts — strictly STRONGER, evolution declared in GAPS and anticipated
  by the original test's own comment. NOT a test weakening.
- E-4 Routes: PASS — PATHS extended to 9 (incl. /assets/rota/logo.svg,
  /api/rota/v1/cognition); every_path_is_get_only_and_200 asserts GET 200 +
  POST/PUT/DELETE/PATCH 405 on all 9; favicon POST 405 asserted separately. ZERO
  mutating routes added.
- E-5 No CDN: PASS — grep http/cdn in rota.rs: only the SVG xmlns. Icon + logo local.
- E-6 Render-from-view-JSON: PASS — all panel renderers read fetched JSON fields,
  nulls => em-dash, per-panel raw-JSON expander preserves the payload; halt takeover
  driven by health.halt_active. fmtCents is display-only division of integer cents.

### F. T4.3 completion accounting (track-B-owned claim set)
- PASS — claim set (cognition view, R7 queries, presentation layer, logo) all
  verified above. Box correctly UNTICKED with remaining surface enumerated (full §5
  money model = operator call; audit-recents queries; R12). Verdict badge/triage/
  discovery (T4.5) and perps panel (T5.B8) absence is CORRECT per the design's
  DEFERRED table and orchestration ownership.

### G. RALPH STOP commit (399d3fa)
- PASS — docs-only (GAPS.md +39, zero code delta; git log --stat verified); queue
  accounting in the entry checks out against orchestration.md (both queue items done,
  no bus findings name track B, remaining surface owned elsewhere); cross-track
  notes declared. Protocol-conformant.

### H. Ownership + battery (shared warm target)
- Ownership: PASS — touched files ⊆ {fortuna-cli, fortuna-ops, assets/rota/,
  ledger repos.rs R7 additions} plus three declared boundary items: lib.rs ONE
  additive pub-use line (queries unreachable without it — GAPS-flagged, accepted),
  NEW additive test file crates/fortuna-ledger/tests/rota_queries.rs (R7 mandates
  tests), 2 new .sqlx JSONs, Cargo.lock (+1 from the ops dep). fortuna-ops gained
  fortuna-ledger (cycle-checked V-5, comment in Cargo.toml). BUILD_PLAN/GAPS/
  ASSUMPTIONS edits stay inside track-B entries; the only BUILD_PLAN deletion is the
  T4.4 checkbox flip. No protected-crate lines.
- fmt: PASS (exit 0). clippy --workspace --all-targets -D warnings: PASS (exit 0).
- cargo test --workspace: PASS — 775 passed / 0 failed across 109 suites (exit 0).
- Invariants per-test: PASS — I1(2) I2(2) I3(1) I4(1) I5(1) I6(3) I7(3) + 3 doctests,
  all ok.
- DST default tier (2000): PASS — core dst "OK: 0 corpus + 2000 random seeds, zero
  invariant violations"; synthesis_dst 2000 ok; settlement_dst 2000 ok; daemon_smoke
  ok; exit 0. READ-PATH RANGE: this diff touches no gates/exec/state/runner code, so
  DST exercises unchanged machinery — run as battery regression, not as new-coverage
  evidence. NOTE (Info): corpus = 0 at this head because main's 3 determinism anchors
  (anchor-777/8675309/31337) postdate the merge-base 2d5c31c; the post-merge battery
  on main picks them up automatically.
- Sweeps: PASS — no unwrap/expect/panic in changed non-test src; no SystemTime/
  Instant/Utc::now in changed src (stop's deadline uses RealClock at the binary edge,
  the sanctioned source; stopping_since reads file MTIME = recorded state); no f64
  money (p/p_raw/brier/clv are cognition probability fields — sanctioned); no secrets;
  no clap; diff-wide test-weakening sweep = only the two declared favicon 204->200
  STRONGER evolutions; no place( / GatedOrder surface in range.

## Findings
- [Minor] F-1: A8's second element — "`status` prints the age of the most recent
  audit row (a stale age + live pidfile = crash tell)" (fortuna-cli.md:56-57) — is
  NOT implemented (status_db_section prints recent rows' timestamps, computes no age)
  and NOT ledgered, while the T4.4 box is ticked with "amendments binding".
  Reproduction: grep -n "age" crates/fortuna-cli/src/main.rs => no status-age
  computation; GAPS contains no entry. Fix: implement the one-line age print or
  ledger the omission in GAPS under T4.4.
- [Minor] F-2: A2's "spawn cwd specified as repo root" is not implemented — spawned
  children inherit the CLI's cwd; the recorder out-dir is absolutized against THAT
  cwd (main.rs:400-407) and data/runtime is cwd-relative. Edge: `fortuna start
  --config-path /abs/cfg` from a non-repo cwd with a relative [recorder].out_dir
  silently forks the B0 dataset path; status/stop from the wrong cwd report
  "stopped" while the daemon runs. Mitigation already present: the DEFAULT config
  path is cwd-relative, so a wrong-cwd start fails the config check first. Fix:
  ledger + either pin the spawn cwd or refuse a relative out_dir/runtime dir when
  cwd is not the repo root.
- [Info] F-3: A8 active-halts print and the full success spawn path are verified
  only by the §13 manual runbook (sanctioned by design §9 — real forking excluded
  from CI; this box hosts the live recorder). The runbook has NOT been executed
  this gate; it remains an operator/orchestrator step before managed-lifecycle
  adoption.
- [Info] F-4: A6 deferred status Section 3 (metrics-endpoint poll) "until ROTA
  lands" — ROTA's serving layer landed in this same range, so the trigger has
  fired. Ledger the follow-up so the deferral does not silently become permanent.
- [Info] F-5: DST corpus empty at track-b head (anchors postdate the rebase point).
  Re-run the corpus on main post-merge (the standing merge battery covers this).

## Mutations (all killed; worktree restored to HEAD after each — git status clean)
- M1 claim_pidfile create_new=>create  => claim_pidfile_race_has_exactly_one_winner FAILED ("O_EXCL must admit exactly one claimant", winners=8)
- M2 unmanaged_recorder_pids => Ok(vec![]) => start_refuses_on_unmanaged_recorder FAILED
- M3 stop skips log_contains_after      => stop_exit_without_shutdown_line_is_not_success + stop_ignores_pre_existing_marker_lines_in_the_append_log BOTH FAILED
- M4a BeliefsRepo::recent DESC=>ASC     => beliefs_recent_lists_newest_first_with_evidence_and_provenance FAILED
- M4b truncate_evidence disabled        => cognition_truncates_evidence_over_4kb FAILED

## Commands run (verbatim verdict lines)
- CARGO_TARGET_DIR=/tmp/fortuna-gate-target cargo fmt --check  => exit 0
- CARGO_TARGET_DIR=/tmp/fortuna-gate-target cargo clippy --workspace --all-targets -- -D warnings  => "Finished `dev` profile ... in 52.89s", exit 0
- CARGO_TARGET_DIR=/tmp/fortuna-gate-target cargo test --workspace  => 109x "test result: ok", total 775 passed, 0 failed, exit 0
- cargo test -p fortuna-invariants  => 13 invariant tests + 3 doctests, all ok
- cargo test -p fortuna-cli  => 11 unit + 27 integration, all ok (full names in session log)
- cargo test -p fortuna-live --test daemon_smoke --test shutdown  => 4 ok (SIGTERM contract)
- CARGO_TARGET_DIR=/tmp/fortuna-gate-target ./scripts/run-dst.sh  =>
  "[dst] OK: 0 corpus + 2000 random seeds, zero invariant violations";
  synthesis-dst 2000 ok; settlement-dst 2000 ok; daemon smoke ok; DST_EXIT=0
- pgrep -fl fortuna-recorder (before + after battery + after mutations) =>
  "79813 ./target/release/fortuna-recorder --interval-secs 30 --bracket-series KXBTC15M,KXBTC,KXBTCD" — intact throughout

## R12 condition (binding on the T4.3 tick's FINAL acceptance)
The R12 live browser pass (seeded Sim serving ROTA; 1440px screenshots per panel;
simulated halt => red takeover renders; ZERO console errors; 390x844 informational
shot; archive under docs/reviews/rota-visual/) is run by the ORCHESTRATOR session
AFTER this verdict, per R12 ("verification-layer concern"). This gate verifies the
server side only (favicon 200/svg, asset routes, shell markers). The track-B
"items complete" claim is accepted CONDITIONAL on that pass; a failed R12 reopens
slice 9, not this verdict's other findings.

## Merge recommendation
MERGE track-b into main. Zero Critical, zero Major. Ledger F-1 and F-2 in GAPS at
merge time (one paragraph each); F-3 runbook execution and the R12 browser pass are
the two post-merge operator/orchestrator conditions. Post-merge battery on main
re-runs the DST corpus with the 3 anchors (F-5).
