# Review: t41-completion-gate — 2026-06-12

Base: 8467d0f  Head: 17245de (the T4.1 TICK)  Verdict: **BLOCK**
Protected crate touched: **no** (`git diff --stat 8467d0f..17245de -- crates/fortuna-invariants/` = empty)
Tier: FULL (money-path composition — the synthesis arm trades the real sizing path)
Worktree: /tmp/fortuna-gc @ 17245de detached; CARGO_TARGET_DIR=/tmp/fortuna-gate-target
DB for sqlx tests: postgres://xavierbriggs@localhost:5432/fortuna (CREATEDB-capable role;
the operator .env fortuna_app role cannot CREATE DATABASE — see Commands, note 1)

## Criteria (fixed before reading the diff: BUILD_PLAN T4.1 requirement list,
## docs/design/synthesis-edge-source-decision.md req 1-5, spec 5.8/5.9/5.14,
## fortuna-review checklist)

### A. Edge-source decision, requirement by requirement
- A-R1 EdgesRepo confirmed-tier load at composition: **PASS** — repos.rs
  `confirmed_edges()` (confirmed_by IS NOT NULL, non-superseded head, ORDER BY
  created_at, edge_id — deterministic); compose_runner -> compose::synthesis_edges.
  Test `confirmed_edges_returns_confirmed_current_heads_only` (fortuna-ledger
  tests/ledger.rs:548) ok, incl. the conservative superseded-by-UNCONFIRMED case.
- A-R2 Per-segment refresh; failure keeps LAST-KNOWN + counts/alerts; loop alive:
  **PASS (behavior, by gate mutation)** — daemon.rs drive() S4 block: Err arm does
  not refresh (last-known implicit), `edge_refresh_transition` latch (alert once
  per outage, count every failure), end-of-run `apply_external_alert` when
  edge_refresh_failures > 0. Gate mutation test
  `r2_refresh_failure_keeps_last_known_alerts_and_survives` (scratch, preserved at
  /tmp/t41-gate-scratch-mutations.rs.preserved): broken pool injected for refresh,
  DB seeded so a SUCCESSFUL refresh would read 0 — post-drive count stayed 1
  (last-known), audit row `kind='alert'` LIKE '%edge-refresh failure%' present,
  drive() returned Ok. PASSED. Committed-coverage gap => Finding m2.
- A-R3 Empty set = valid mechanical-only state: **PASS** —
  `empty_edge_set_fails_closed_but_a_present_edge_trades` (synthesis_loop.rs:267,
  non-vacuous by contrast) ok; `per_segment_refresh_picks_up_a_newly_confirmed_edge`
  boots at Some(0) and runs.
- A-R4 Config filters only, deterministic truncation: **PARTIAL** — venue filter +
  max_edges with sort-by-edge-id truncation implemented and tested
  (`synthesis_edges_loads_confirmed_mapped_and_filtered`, compose.rs:192, ok).
  The decision doc's "categories allowlist" filter DOES NOT EXIST;
  SynthesisSection.category was repurposed as the CALIBRATION scope selector.
  Deferral pointer stale (GAPS:231 "deferred to S3b" — S3b is closed). Config
  NARROWS-only and never DEFINES holds; absence of the allowlist is not fail-open.
  => Finding m1.
- A-R5 Named tests exist and run: **PASS-with-gap** — seeded-confirmed-edges-trade
  (`synthesis_arm_trades_with_ledger_calibration_and_an_injected_mind`, ok);
  unconfirmed/superseded excluded (ledger.rs:548, ok); empty-set boots clean +
  trades nothing (ok, twice); refresh-failure keeps last-known + alerts: ONLY the
  latch unit test is committed (daemon.rs
  `edge_refresh_transition_alerts_once_per_outage_and_counts_every_failure`);
  no committed integration test => Finding m2 (behavior proven by the gate's
  scratch reproduction).

### B. Synthesis trading path end-to-end in the booted daemon
- B1 context -> mind -> proposal -> calibrated-p -> haircut-Kelly -> gates -> sim
  venue -> belief drain -> persist -> calibration feedback: **PASS** —
  `synthesis_arm_trades_with_ledger_calibration_and_an_injected_mind` (daemon_smoke,
  REAL ledger params + 50 resolved beliefs -> quality -> sized+gated+submitted, ok);
  `drive_drains_and_persists_the_synthesis_arms_beliefs` (0 -> >=1 beliefs in PG, ok);
  `anthropic_mind_trades_the_composed_loop_through_dyn_mind` (synthesis_loop, ok);
  feedback closes via calibration_for_scope reading resolved_stats at composition
  (per contract: CalibrationParamsRepo.latest + resolved_stats -> CalibrationContext
  + set_calibration_quality — present in compose_runner, daemon.rs).
- B2 E1 fail-closed THROUGH the composition (no params => zero size): **PASS (gate
  mutation)** — `e1_no_calibration_params_sizes_zero_in_the_composed_daemon`
  (scratch): [synthesis] category set, believing mind, live book, example config
  WITH synthesis envelope+gate, NO params row -> orders_submitted == 0, no position.
  PASSED. Unit-layer pin `missing_calibration_quality_fails_closed_to_zero_size` ok.
- B3 Mind budget per-cycle + per-day binding in the composition; breach => degrade
  + audit + alert: **PASS (gate mutation)** — mind_from_env builds
  CostBudget::new(per_cycle_budget_cents, daily_budget_cents) from [cognition]
  (daemon.rs). `b3_zero_budget_breaches_degrade_audited_in_composition` (scratch):
  zero budget via mind_from_env + scripted transport that PANICS if called ->
  transport never called, orders 0, counters().budget_breaches >= 1, PG audit row
  kind='cognition' degrade='budget_exhausted'. PASSED. Runner-layer
  `budget_breach_and_discarded_output_write_audit_rows` ok (alert via degrade
  scrape consumer, alert_routing suite 6/0).

### C. Rearm adjudication (option a, restart-gated)
- C1 CLI rearm output documents restart-gating: **FAIL** — fortuna-cli main.rs:1085:
  `println!("re-armed {scope_raw} (operator: {operator})")` — no "pending daemon
  restart" notice; grep for rearm/restart text in fortuna-cli: none.
- C2 ROTA health surface documents it: **FAIL** — grep -rni "rearm" fortuna-ops/src:
  zero functional hits; no rearm-pending state in the health view.
- C3 ASSUMPTIONS documents it: **PASS** — ASSUMPTIONS.md:6-21: "The running daemon
  NEVER auto-clears a gate halt; a re-arm takes effect on the next daemon RESTART…
  Conservative reading of I2 ('no automatic resumption')… OPERATOR-FACING follow-on
  (track B…): the `fortuna` re-arm output + the ROTA health panel should surface
  're-arm pending daemon restart'… ledgered in GAPS". GAPS:148-163 carries the
  ledger entry.
- C4 No code path silently un-halts: **PASS** — only non-test `.rearm(` call site is
  the internal delegation pipeline.rs:180; run_loop Ok(None) arm resets the dedup
  latch ONLY (comment + code verified); `apply_external_halt` doc: "nothing here or
  anywhere in the daemon clears a halt"; i2 invariant tests 2/0; DB-level
  append-only trigger refused my scratch DELETE (I5 enforced at the store).
  The choice itself is I2-conservative-compliant. => Finding M3 (docs 1/3 complete).

### D. T4.1 completion accounting
- D1 Every requirement w/ executed evidence: **FAIL (two items absent)** —
  PRESENT+GREEN: config fail-closed (boot 12/0 incl. `each_missing_var_is_named_precisely`,
  `placeholder_values_refuse_loudly`, `halt_poll_over_500ms_refuses`); repos+AuditWriter
  no-audit-no-trading (`daemon_audit_rows_land_in_postgres_and_audit_death_halts` ok);
  RealClock-at-edges/SimClock-replay (RealCadence in main; smokes under SimClock);
  mind_from_env w/ CostBudget (unit test + B3 mutation); strategies from config
  (synthesis + mech_extremes opt-in smokes ok, veto-enrolled w/ StubVetoMind);
  Slack every-message-audited + degrade consumer (alert_routing 6/0); dead-man
  (deadman 2/0); metrics GET-only (`every_path_is_get_only_and_200`, POST 405);
  halt poll <=500ms + poll-failure alert (run_loop 6/0); graceful shutdown +
  SHUTDOWN CONTRACT (SignalKind::terminate at main.rs:150; shutdown 2/0;
  `signal_with_working_orders_cancels_them_and_audits` ok); Sim-only (E1 below);
  DST smoke (D2). ABSENT: the daily-reconciliation loop (00:00 UTC model journal
  re-run) and weekly/monthly reviews -> digest — only the DailyScheduler daily
  DIGEST exists. Disclosed in the tick ("HONESTLY DEFERRED post-tick") + ledgered
  (GAPS:452-453), but the box is TICKED with named contract items unbuilt.
  => Finding M2.
- D2 Daemon-composition DST smoke deterministic: **PASS** — daemon_smoke is
  run-dst.sh stage 5 (7/0 in the corpus run); clock-injected (StopAtCadence over
  SimClock), reran identically 3x this session.
- D3 Tick wording: **PASS w/ note** — body says "battery-gated (full fortuna-live +
  fortuna-runner suites + DST green)" and "The independent VERIFIER gates this
  batch AT the tick (per GATE-FINDINGS)" — it does NOT claim independent gating
  already happened; this gate is that gate. The per-crate battery scoping is what
  let M1 through (304f746 ran `-p fortuna-live` only — honest but insufficient).
- D4 Soak start state: **PASS** — no fortuna-live process, no pidfile (ps + glob
  empty); tick documents operator-started with exact prerequisites (secrets,
  ANTHROPIC_API_KEY, release build). Accurate.

### E. Sim-only + demo config
- E1 kalshi refuses without clearance: **PASS** — `venue_kalshi_refuses_until_fixture_clearance`
  re-run this session, ok (boot.rs VenueNotBootable arm).
- E2 Demo config no live enablement, no secrets: **PASS** — diff adds only
  [gates.per_strategy.synthesis] + synthesis_cents envelope; venue = "sim" retained;
  secrets sweep over the whole range diff: zero hits.

### F. Full battery
- fmt --check: **PASS** (exit 0).
- clippy --workspace --all-targets -D warnings: **PASS** (exit 0, Finished).
- cargo test --workspace: **FAIL — exit 101, exactly ONE red test** —
  fortuna-ops tests/config.rs:84 `example_config_file_parses_and_validates`:
  expected envelopes {mech_extremes, mech_structural} != actual {+ synthesis: 200000}.
  Broken by 304f746 (added synthesis_cents to the example; never updated the pin).
  Coverage of everything else: run A (through fortuna-live) 424/0 then halt at ops;
  run B (-p runner,ops,paper,recorder,state,venues --no-fail-fast) 384 passed /
  1 failed (the same single test). fortuna-invariants per-test: i1 2/0, i2 2/0,
  i3 1/0, i4 1/0, i5 1/0, i6 3/0, i7 3/0. => Finding M1, the BLOCK driver.
- scripts/run-dst.sh 10000: **PASS, exit 0, all stages** —
  stage 1/2: "[dst] regression corpus: 3 seed(s)" (the new determinism anchors
  replay) + "[dst] OK: 3 corpus + 10000 random seeds, zero invariant violations";
  stage 3: "[synthesis-dst] master seed 1781288801944 -> 10000 scenario(s)…
  132402 cognition failures… ok (91.65s)";
  stage 4: "[settlement-dst] master seed 1781288897327 -> 10000 scenario(s)" all
  11 arms exercised, ok; stage 5: daemon_smoke 7/0. NO perp stage exists in main's
  run-dst.sh (perps merge parked on operator signatures — correct).
- Sweeps (whole-range diff): test-weakening NONE (the only changed assertion is the
  favicon 204->200+content-type STRENGTHENING, track-B-gated); #[ignore]/proptest
  reductions NONE; SystemTime/Instant/Utc::now NONE; f64-money NONE (hits are
  test probabilities/brier); HashMap/HashSet in runner/live additions NONE
  (BTreeMap in digest_snapshot); unwrap/panic in money-path source NONE (one
  unwrap in a #[cfg(test)] block); place(/GatedOrder bypass NONE; secrets NONE.
- Mutations: 3/3 PASSED (A-R2, B2, B3 above; scratch preserved at
  /tmp/t41-gate-scratch-mutations.rs.preserved, removed from the worktree).

## Findings

- [Major] **M1 — workspace battery RED at head (BLOCK driver).** 304f746 added
  `synthesis_cents` + [gates.per_strategy.synthesis] to config/fortuna.example.toml
  but did not update the example-config pin; `cargo test --workspace` exits 101 at
  fortuna-ops tests/config.rs:84. Reproduction: `cargo test -p fortuna-ops --test
  config` -> `left: {"mech_extremes": 200000, "mech_structural": 300000,
  "synthesis": 200000} right: {"mech_extremes": 200000, "mech_structural": 300000}`.
  Root cause: per-crate batteries (-p fortuna-live) instead of the workspace
  battery (CLAUDE.md definition-of-done item 2). Fix: update the pinned BTreeMap to
  include ("synthesis", 200_000) — a pin TRACKING a deliberate config change, not a
  weakening — then run the FULL workspace battery.
- [Major] **M2 — ticked box with two named contract items unbuilt.** T4.1 requires
  "daily reconciliation 00:00 UTC; weekly/monthly reviews -> digest channel"; only
  the daily DIGEST scheduler exists. Disclosed in the tick + ledgered (GAPS:452-453)
  — honest, but completion accounting fails. Operator may waive by treating the
  disclosure as a contract amendment; recommend explicit BUILD_PLAN sub-checkboxes
  so the items cannot evaporate post-tick.
- [Major] **M3 — rearm adjudication documentation 1/3 complete.** The R12 finding's
  option (a) required documenting restart-gated rearm in CLI rearm output, ROTA
  health, AND ASSUMPTIONS. Only ASSUMPTIONS done; CLI+ROTA ledgered to track B
  (GAPS:148-163). Fail-safe direction (daemon stays halted), but the exact
  operator-confusion scenario R12 hit (rearm written, daemon still halted, no
  notice) remains live going INTO a soak full of drills. Behavior itself is
  I2-compliant (C4 PASS).
- [Minor] **m1 — R4 categories-allowlist filter absent, stale deferral.** GAPS:231
  says "deferred to S3b" but S3b closed without it; SynthesisSection.category now
  means calibration scope. Implement or re-ledger as a current open item with the
  events-category-join rationale.
- [Minor] **m2 — R5 refresh-failure integration test not committed.** Only the
  latch unit test exists; last-known retention + alert + loop-alive was proven by
  the GATE'S scratch test. Commit an equivalent (shape preserved at
  /tmp/t41-gate-scratch-mutations.rs.preserved; the superseding-insert
  disambiguation matters — DELETE is refused by the I5 trigger).
- [Minor] **m3 — stale operator guidance.** daemon.rs:42-44 and main.rs:76 still
  say synthesis cannot trade "until the operator config adds a synthesis_cents
  envelope + [gates.per_strategy.synthesis]" — false since 304f746 closed exactly
  that gap. A soak operator reading main.rs will mis-expect an inert arm.
- [Minor] **m4 — unannounced fixture re-record in 8b8b222.** The track-C re-gate
  commit bundles a 111-file kinetics-perps fixture re-recording (ws ticker 5000->245
  frames, new recorded_at) with zero mention in its message. Inert on main (sole
  consumer is the recorder example), provenance intact in .meta.json, but commit
  content must match commit message.
- [Info] **Session incident (verifier-owned):** this gate caused an ENOSPC by
  creating a second scratch CARGO_TARGET_DIR against the disk-hygiene v2
  single-target rule; freed (~30GB) mid-session. Machine has ~10GB free — the
  standing disk item in GATE-FINDINGS remains urgent.
- [Info] Soak start correctly NOT performed; state documented accurately (D4).

## Commands run (verbatim verdict lines)

1. DB note: `cargo test --workspace` with the operator .env DATABASE_URL
   (fortuna_app) fails environmentally: "permission denied to create database"
   (sqlx::test). All results below use postgres://xavierbriggs@localhost:5432/fortuna.
   i5_audit_append_only with that role: `test result: ok. 1 passed; 0 failed`.
2. `cargo fmt --check` -> exit 0.
3. `cargo clippy --workspace --all-targets -- -D warnings` -> "Finished `dev`
   profile … in 20.93s", exit 0.
4. `cargo test --workspace` -> exit 101; PASSED=424 before halt;
   `---- example_config_file_parses_and_validates stdout ---- … left:
   {"mech_extremes": 200000, "mech_structural": 300000, "synthesis": 200000}
   right: {"mech_extremes": 200000, "mech_structural": 300000}`.
5. `cargo test -p fortuna-runner -p fortuna-ops -p fortuna-paper -p fortuna-recorder
   -p fortuna-state -p fortuna-venues --no-fail-fast` -> exit 101; PASSED=384
   FAILED=1; "error: 1 target failed: `-p fortuna-ops --test config`" (the ONLY red).
6. `cargo test -p fortuna-invariants` -> i1 2/0, i2 2/0, i3 1/0, i4 1/0, i5 1/0,
   i6 3/0, i7 3/0 (all "test result: ok").
7. `bash scripts/run-dst.sh 10000` -> exit 0;
   "[dst] OK: 3 corpus + 10000 random seeds, zero invariant violations";
   "[synthesis-dst] master seed 1781288801944 -> 10000 scenario(s)" + "totals:
   26804 orders, 41257 proposals, 132402 cognition failures, 118026 beliefs" ok;
   "[settlement-dst] master seed 1781288897327 -> 10000 scenario(s)" ok;
   daemon_smoke "test result: ok. 7 passed".
8. `cargo test -p fortuna-live` (incl. gate scratch) -> alert_routing 6/0,
   belief_persist 1/0, boot 12/0, compose 5/0, daemon_smoke 7/0, deadman 2/0,
   pg_audit 1/0, pg_journal 2/0, run_loop 6/0, shutdown 2/0, views 7/0,
   zz_gate_mutation_scratch 3/0.
9. Soak check: `ps aux | grep fortuna-live` -> none; no pidfiles.

## Disposition

BLOCK. Re-gate path (cheap): fix M1 (one-line pin update in
crates/fortuna-ops/tests/config.rs) -> run `cargo fmt --check`, clippy, FULL
`cargo test --workspace`, `scripts/run-dst.sh 10000` -> all green -> the BLOCK
clears bidirectionally. M2 needs the operator's waive-or-subtask decision; M3
(CLI+ROTA rearm notices) should land before the soak's first halt drill.
The daemon composition itself is mechanically sound: every targeted suite green,
DST 10000 all stages green, and all three gate mutations (refresh-failure
last-known, no-params zero-size, zero-budget degrade) hold in the composed daemon.

## ADDENDUM — M1 remediation re-gate (verifier session, 2026-06-12 ~17:30Z)

Fix de0426c verified bidirectionally: the pin was red at 17245de
(byte-stable across 3 runs, prior gate) and is green at de0426c, with the
pin TRACKING the deliberate config addition (comment states lockstep-not-
loosened intent). Full battery at de0426c: workspace 791/0, clippy -D
warnings clean, run-dst.sh 10000 ALL stages green (3 corpus anchors +
10000 core; synthesis 10000 w/ 132,786 injected failures; settlement
10000 all 11 arms; smoke 7/7). M1 CLEARED. The T4.1 verdict converts to
ACCEPT-WITH-GAPS conditional on: [M2] the operator's tick decision
(waive-with-subcheckboxes or untick for the two disclosed-unbuilt review
loops) and [M3] the rearm notices landing before the first soak halt
drill. The daemon is mechanically FIT TO START THE SOAK.
