# Review: system-0-3-final — 2026-06-10
Base: d138a73 (operator baseline)  Head: 7bbc3ef (claimed build-complete, T3.6)  Verdict: BLOCK
Protected crate touched: yes (full-history audit below: 8 commits, all pure-addition or
sanctioned stub-implementation; ZERO assertion-logic modifications; automatic-BLOCK
adjudication rule applies regardless of content and remains un-adjudicated by the operator)

Final cumulative system verification, Phases 0+1+2+3. All evidence executed in this
session in a detached scratch worktree (/tmp/fortuna-sys0123 at 7bbc3ef; operator tree
untouched; removed after review). DATABASE_URL=postgres://localhost/fortuna_dev.
Acceptance DST run on a FRESH master seed 8675309202606 (recorded here), 10,000 seeds
per stage. FINAL_REPORT's own DST claims additionally REPRODUCED from its recorded
master seeds (1781116180331 core, 1781116572856 chaos) — totals match exactly.

Grading frames, fixed before reading code: Phase 0-2 EXIT lines graded at their
original strict reading (UNVERIFIABLE fails); Phase 3 EXIT graded at its own wording
("PROMPT.md acceptance checklist fully green or operator-blocked-only", BUILD_PLAN:257).
Both readings shown so the operator sees them side by side.

## Group 1 — Phase EXIT lines (re-graded fresh at 7bbc3ef)

### Phase 0 EXIT (BUILD_PLAN.md:70-72) — MET
- P0.1 mech_structural full loop in Sim with replay: PASS — sim_loop.rs 7 tests green
  (mech_structural_captures_a_bracket_arb_end_to_end,
  same_seed_same_script_byte_identical_recording); scripts/replay.sh --seed 424242 ->
  "[dst] seed 424242: OK".
- P0.2 DST corpus zero violations >= 10,000 seeds: PASS — fresh master 8675309202606:
  "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations".
- P0.3 CI green: PASS — cargo fmt --check exit 0; clippy --workspace --all-targets
  -D warnings exit 0; workspace 615 passed / 0 failed / 0 ignored; .github/workflows/ci.yml
  present (hosted runs not verifiable from this session; local battery is the evidence).
- P0.4 I1-I4 implemented and green: PASS — per-test this session: i1_universal_gate ok,
  i1_prop_all_orders_carry_gate_verdicts ok + 2 compile-fail doctests ok;
  i2_drawdown_human_rearm ok, i2_prop_breach_always_locks_until_rearm ok;
  i3_runaway_halt ok; i4_killswitch_independence ok (8.82s, spawns standalone binary).

### Phase 1 EXIT (BUILD_PLAN.md:121-122) — NOT MET (strict reading)
- P1.1 both mechanical strategies in Paper against recorded data streams: FAIL
  (OPERATOR-BLOCKED) — fixtures/kalshi/ holds README.md only; GAPS.md:12-33 has the
  exact unblock ("the SAME capture must also include websocket
  orderbook_snapshot/orderbook_delta + public trade messages..."). Buildable halves
  green: fortuna-paper 11 tests incl. touch_prints_never_fill_resting_orders and
  paper_live_parity_through_the_order_manager; mech_extremes 15 tests.
- P1.2 settlement AND discrepancy paths exercised in DST: FAIL — settlement half holds
  (dst.rs:435 settle|void|reverse arms, I-money through refunds/claw-repay, in the
  10,000-seed green run). Discrepancy half does NOT:
  `grep -inE 'discrepan|mismatch|overdue|dispute|watchdog|diverg'` over
  crates/fortuna-core/tests/dst.rs + crates/fortuna-runner/tests/synthesis_dst.rs ->
  zero scenario hits (only determinism trace-compare strings); both harnesses changed
  by exactly +1 line each in the whole Phase-3 range. Coverage exists only in
  settlement_loop.rs composed tests (11 green), which scripts/run-dst.sh never runs.
  Third consecutive gate at which this finding stands.
- P1.3 dashboards and digest render from sim data: PASS —
  observability.rs::dashboards_and_digest_render_from_sim_data ok; fortuna-ops suites
  green (dashboard read-only, digest/export, metrics, slack, deadman, config).

### Phase 2 EXIT (BUILD_PLAN.md:193-195) — NOT MET (strict reading)
- P2.1 FULL decision loop in Sim with StubMind under DST incl. cognition failures:
  FAIL — the composed loop runs green under DST (10,000 chaos scenarios, 131,731
  injected cognition failures degraded without a tick error, byte-identical replay per
  seed; synthesis_loop.rs 3 tests green) BUT it is not the spec-5.8 FULL loop:
  "Calibration layer adjusts p" — CalibrationParams::apply has ZERO call sites outside
  calibration.rs; spec line 240 "fractional Kelly (default 0.25) on calibrated edge...
  haircut by category calibration quality" — kelly_binary/kelly_contracts/
  haircut_kelly_fraction have ZERO src call sites; the only sizing path is
  runner.rs:478 affordable_sets (full headroom up to caps). Note: the phase-2 gate
  graded this FAIL, the system-0-1-2 gate graded it PASS; re-graded fresh on the spec
  text, FAIL is correct — the loop that runs is comparator->affordable_sets, not the
  spec's calibrated-Kelly loop.
- P2.2 AnthropicMind exercised behind a feature flag with budget controls: FAIL — not
  merely operator-blocked: `grep -rn "impl Mind for" crates/*/src` -> StubMind only;
  zero AnthropicMind::/ReqwestMindTransport:: construction sites in src; even with
  ANTHROPIC_API_KEY nothing can exercise it in the loop. Per-cycle budget vacuous
  (Group 4 (c)). Mock-transport tests green (6 in tests/mind.rs).
- P2.3 aeolus_eval writes scored beliefs from fixture envelopes: PASS —
  aeolus_eval_writes_scored_beliefs_from_the_fixture_envelope ok (named run this
  session, Pg-backed).
- P2.4 I5-I7 invariant tests green: PASS at 7bbc3ef — i5 ok; i6 x3 ok; i7 x3 ok
  (both formerly-staged stubs implemented at T3.3); 0 ignored anywhere.

### Phase 3 EXIT (BUILD_PLAN.md:257) — NOT MET
"PROMPT.md acceptance checklist fully green or operator-blocked-only": items 1, 4, 7,
10, 11 fail on grounds that are NOT operator-blocked (Group 3). The EXIT MET note at
BUILD_PLAN:258-261 is contradicted by this session's evidence.

## Group 2 — The seven invariants (per-test + structural)

- I1 universal gate: PASS — tests green (above); structural: sole constructor
  pub(crate) GatedOrder::assemble (order.rs:37; called only from pipeline.rs:224);
  Venue::place(GatedOrder) (venues/lib.rs:92); single non-test .place( call site
  (exec/manager.rs:329); zero From/Into/Deserialize ctors outside fortuna-gates;
  2 compile-fail doctests green.
- I2 drawdown halts, human re-arm: PASS — i2 tests green;
  halts_fold_restores_only_unrearmed_flags ok (named run; halt flags survive restart).
- I3 runaway dual token-buckets: PASS — i3_runaway_halt ok; dual per-venue AND
  per-market buckets in committed config (config/fortuna.example.toml:26-33 burst /
  sustained_per_min / market_burst / market_sustained_per_min).
- I4 kill-switch out-of-band: PASS — i4 green (spawns the standalone binary, no DB);
  cargo tree -p fortuna-killswitch: zero matches for
  sqlx|postgres|fortuna-ledger|fortuna-cognition|anthropic; scripts/killswitch-test.sh
  present (monthly drill).
- I5 append-only audit: PASS — i5_audit_append_only ok; DB-level BEFORE UPDATE OR
  DELETE triggers in migrations (audit, beliefs guard, edges, signals, tradability);
  audit_sink_failure_halts_trading ok (sim_loop).
- I6 propose-only Mind: PASS — i6 x3 ok; deny_unknown_fields pinned on
  MindOutput/ProposalDraft (mind.rs:69,106,116); dependency-direction test green.
- I7 promotion gates: PASS as rails — i7 x3 ok incl. the T3.3 implementations
  (operator-action-record stage derivation: no records => Sim, system/blank actor
  cannot promote, no stage-skipping, declared stage is a cap, demotion any-actor;
  model-swap gate: no record/insufficient/worse challenger => Hold,
  recommendation-only verdict type); sim_runner_refuses_non_sim_staged_strategies ok;
  SynthesisStrategy stage now derived via promotion::effective_stage (synthesis.rs).
- PROMPT acceptance "ZERO ignored tests in fortuna-invariants": PASS —
  `grep -rn "#\[ignore" crates` -> zero hits repo-wide; suite ran 13 passed /
  0 ignored + 3 doctests.

## Group 3 — PROMPT.md acceptance checklist (lines 99-119; Phase-3 EXIT wording)

- A1 All BUILD_PLAN tasks complete with evidence notes: FAIL — all 29 boxes ticked
  with notes+hashes, but T2.6's deliverable "Kelly sizing with calibration haircut"
  exists only as uncalled functions (zero src call sites; the one sizing path is
  affordable_sets), so the task is complete only as dead code; its DONE note and
  cycle.rs:1-10 header claim wiring that does not exist. Not operator-blocked.
- A2 fmt/clippy/full suite green: GREEN — fmt exit 0; clippy -D warnings exit 0
  ("Finished `dev` profile..."); 615 passed / 0 failed / 0 ignored.
- A3 invariant tests implemented, none ignored, green: GREEN — Group 2.
- A4 DST corpus (doctrine scenarios; 10,000+ seeds; regression seeds committed):
  FAIL — (i) seeds: GREEN, fresh master 8675309202606: core "[dst] OK: 0 corpus +
  10000 random seeds, zero invariant violations" + chaos "totals: 33437 orders, 51076
  proposals, 131731 cognition failures, 116939 beliefs / ok. 1 passed (86.97s)".
  (ii) doctrine list IN the corpus: FAIL strict — present in-harness: crash-pre-submit
  (dst.rs:414), crash-pre-ack, dup fill, fill-after-cancel, partial-fill+outage,
  settle-then-reverse (dst.rs:435), reservation rebuild at boot, schema-invalid output,
  budget exhaustion, refusals (chaos mind); sanctioned isolation: kill switch (i4).
  ABSENT from both seeded harnesses (keyword grep, zero hits; TEST-ONLY elsewhere):
  overdue watchdog, halt-mid-IntentGroup, rate-limit breach mid-burst (dst.rs:209
  configures "huge rate buckets" by design), malformed venue payloads, clock skew,
  stale-mark wide-spread drawdown, trigger storm, max-leg-duration; Pg-at-boot partial.
  FINAL_REPORT remaps "corpus" to "arm or test" — a reinterpretation, not a fix; not
  operator-blocked. (iii) regression seeds committed: vacuously satisfied — 0 seeds,
  FINAL_REPORT discloses "no randomized run ever produced a failure to minimize";
  corpus discipline documented (dst-corpus/README.md); tension with the T0.4 note
  ("2 real harness bugs found+fixed on first runs") remains unexplained in any ledger.
- A5 Replay determinism: GREEN — replay.sh seed 424242 OK;
  same_seed_same_script_byte_identical_recording ok; per-seed replay invariant inside
  the 10,000-scenario chaos run; independent corroboration: FINAL_REPORT's chaos
  totals reproduced EXACTLY from master seed 1781116572856 (33444/51284/131071/116735).
- A6 mech_structural + mech_extremes in Sim AND Paper-vs-recorded: OPERATOR-BLOCKED
  (valid) — Sim halves green; recorded-stream half blocked on the Kalshi fixture
  capture, GAPS.md:12-33 quotes the exact recording requirements. Valid per the
  Phase-3 EXIT wording.
- A7 Belief pipeline end-to-end in Sim ("stub mind AND the Anthropic-backed mind") +
  aeolus_eval vs sample fixture: FAIL — StubMind end-to-end green; aeolus_eval green;
  the Anthropic-backed mind is NOT behind the Mind trait and has NO composition path
  (code gap, not credentials), so this half is not operator-blocked; the GAPS entry
  ("the env-key gate IS the feature flag") misframes it.
- A8 Kill-switch standalone (no Pg, runtime dead, freeze-and-cancel vs sim, monthly
  script): GREEN — i4 (8.82s) + dep tree + scripts/killswitch-test.sh; live venue
  plug operator-blocked validly (fixtures first).
- A9 Slack routing, digest, dead-man, accounting export, dashboard vs sim data:
  GREEN — fortuna-ops suites green by name (router fail-closed tests, digest
  deterministic, export write-once, deadman cadence, dashboard read-only) +
  dashboards_and_digest_render_from_sim_data; Socket Mode listener operator-blocked
  validly (Slack app credentials).
- A10 GAPS.md zero unresolved items other than operator-blocked: FAIL — the FILE
  satisfies the letter (every entry operator-blocked with exact unblock steps), but
  the claim is false by omission: prior-gate Majors (a)-(d) and Minors (g),(h),(k)
  were known to the implementer (both gate reviews were committed INTO THE TREE at
  T3.3, commit 6276274) and are neither fixed nor ledgered; the AnthropicMind entry
  misrepresents a structural code gap as credentials-only.
- A11 FINAL_REPORT.md (what was built; EVERY deviation; stats; runbook): FAIL on the
  deviations clause — report exists; runbook present (§5); statistics VERIFIED
  (615/0 tests ✓, 13 invariant tests 0 ignored ✓, 58 commits ✓, 40,674 LOC vs
  "~40,700" ✓, 13 crates ✓, core DST zero-violations at recorded seed ✓ reproduced,
  chaos totals ✓ reproduced exactly). But §2 "every deviation from spec" omits the
  three known, cited spec divergences: 5.8/5.9 sizing stages absent from composition,
  AnthropicMind not behind Mind (5.9 "both behind Mind"), per-cycle budget vacuous
  (5.9); and the §4 disposition table marks the belief-pipeline item "DONE...
  OPERATOR-BLOCKED (ANTHROPIC_API_KEY)" concealing the structural gap.

## Group 4 — Regression check of prior Major/Critical (+ named Minor) findings

| # | Prior finding | Grade at 7bbc3ef | Evidence (executed this session) |
|---|---|---|---|
| (a) | Haircut-Kelly + calibrated p absent from composed sizing | STILL-OPEN | grep: kelly_binary/kelly_contracts/haircut_kelly_fraction zero src call sites; CalibrationParams::apply zero call sites outside calibration.rs; runner.rs:478 affordable_sets; cycle.rs:9-10 false header unchanged; ASSUMPTIONS.md:779 claims "Kelly for belief trades (Phase 2)" — false; config [sizing] kelly_fraction validated (ops config.rs:151) but never consumed by sizing |
| (b) | AnthropicMind no `impl Mind`, no composition path | STILL-OPEN | grep "impl Mind for" -> StubMind only (mind.rs:173); zero AnthropicMind::/ReqwestMindTransport:: src construction sites |
| (c) | Per-cycle budget vacuous for positive caps | STILL-OPEN | mind.rs:221-238 unchanged: refuses only per_cycle_cap_cents <= 0, otherwise compares per-day spend only; no per-cycle cap in config example |
| (d) | Discrepancy/watchdog paths absent from both seeded DST harnesses | STILL-OPEN | keyword grep zero hits in dst.rs + synthesis_dst.rs; harnesses +1 line each in entire Phase-3 range; coverage only in settlement_loop.rs which run-dst.sh never executes |
| (e) | Spec 5.13 divergence detector nonexistent | FIXED | check_settlement_divergence (runner.rs:1395) on fresh (1187) AND corrected (1283) settlements; tests settlement_divergence_records_and_pnl_follows_venue_truth + agreeing_settlement_records_no_divergence ok this session. Residual (Minor, ledgered ASSUMPTIONS T3.6): set_canonical_resolution has zero src call sites — dormant unless the composition wires it; edge-confidence hit is recorded-not-applied |
| (f) | Staleness/stranded watchdog never in run_watchdogs | FIXED (rails) | orphan watchdog in run_watchdogs (runner.rs:1581+), alert-once; orphaned_position_alerts_once_only_when_coverage_is_wired ok. Residual (Minor, ledgered): set_position_coverage zero src call sites; nothing computes coverage from belief freshness — unwired coverage DISABLES the watchdog by documented design |
| (g) | run_watchdogs early-return on venue outage starves overdue/dispute | STILL-OPEN | runner.rs run_watchdogs head: `Err(_) => return Ok(()), // outage: watch again next tick` — verbatim unchanged |
| (h) | run-dst.sh fail-open vacuous pass | STILL-OPEN | scripts/run-dst.sh:14-19 unchanged: build failure prints "passing vacuously", exits 0, stderr suppressed; also skips the synthesis stage entirely on that branch |
| (i) | Empty regression corpus vs "regression seeds committed" | STILL-OPEN (disclosed) | dst-corpus/ = README.md only, "[dst] regression corpus: 0 seed(s)"; FINAL_REPORT discloses vacuity; T0.4-note tension unexplained |
| (j) | Stale GAPS deferral entries (divergence->T2.1, staleness->T2.3) | FIXED | GAPS.md reorganized at T3.6: both entries gone, their subjects landed in code+tests (see (e)/(f)); decision records moved to ASSUMPTIONS.md (verified present) |
| (k) | f64 cents arithmetic in sizing.rs | STILL-OPEN | sizing.rs:68 `(headroom.raw() as f64 * f).floor() as i64` unchanged; still unledgered in ASSUMPTIONS.md; mitigation: kelly_contracts has zero callers (dead code, no live money path) |
| P1-Crit/P2-F1 | Protected-crate touches pending operator adjudication | STILL-PENDING | No operator adjudication recorded anywhere (GAPS "Disputed invariant tests: (none)"); a third touch (6276274) joined the set |
| P2-F5 | Two #[ignore] I7 stubs in protected crate | FIXED | Implemented at T3.3; 0 ignored repo-wide; both tests green by name |
| P2-F6 | Model ProposalDrafts silently discarded, unaudited | STILL-OPEN (Minor) | cycle.rs reads only output.beliefs/output.cost_cents; no model_call audit in the composed Sim loop |
| P2-F7 | f64 at probability->cents boundary cycle.rs:117 | STILL-OPEN (Minor) | cycle.rs unchanged in Phase-3 range |
| P2-F8 | .DS_Store junk tracked | STILL-OPEN (Minor) | git ls-files: 4 tracked .DS_Store files |

## Group 5 — Repo-wide mechanical sweep at 7bbc3ef + protected-crate history audit

- unwrap/expect/panic!/todo!/unimplemented! in gates/exec/state/venues/paper src:
  PASS — 2 hits, both inside #[cfg(test)] modules (gates/rate.rs:62 verified under
  `#[cfg(test)] mod tests`; kalshi/adapter.rs:1098 inside test fn).
- Clock discipline: PASS — zero SystemTime::now/Instant::now/Utc::now/Local::now
  outside clock impls.
- f64 money: FAIL (Minor) — sizing.rs:68 and cycle.rs:117 (both conservative-direction
  floors; both pre-existing; (k)/F7 above). All other f64 are probabilities or
  Decimal-converting DTO boundaries.
- Unordered-map nondeterminism: PASS — zero HashMap/HashSet in
  core/gates/exec/state/runner src; BTree throughout.
- Test weakening: PASS — Phase-3 range has 18 deleted .rs lines total; the only
  test-attribute deletions are the two #[ignore] removals (stub closure =
  strengthening); zero deleted asserts, zero proptest reductions, zero tolerance
  changes; repo-wide #[ignore] count is zero.
- Secrets: PASS — pattern sweep over crates/config/scripts/fixtures clean; API key
  env-only; secrets_debug_output_redacts_every_value ok.
- GatedOrder discipline: PASS — Group 2 I1.
- Protected-crate FULL-HISTORY audit (8 commits touching crates/fortuna-invariants/):
  d138a73 baseline operator stubs; f463de2 (I1+I3), 11e8313 (I2), a1e4449 (I5),
  d5e2220 (I4), 1dcd05d (I6+I7 partial): all pure stub-implementation (audited at the
  prior system gate; deletions were placeholder #[ignore]/todo!() lines only);
  88c9b91: pure addition (+3 lines); NEW this gate: 6276274 (T3.3) — line-by-line:
  deletions are EXACTLY the two #[ignore =...] attributes and two todo!() bodies;
  additions implement the staged I7 clauses. Classification: pure stub-implementation,
  the pattern the crate's own tests/README.md sanctions. ZERO assertion-logic
  modifications anywhere in history. The automatic-BLOCK rule still requires operator
  adjudication of all touches; none is recorded.

## Group 6 — FINAL_REPORT.md verification

- Exists (acceptance item): YES, with runbook (§5) and per-item disposition (§4).
- Claimed statistics vs this session's executions:
  - "615 tests, 0 failures" -> REPRODUCED (615/0/0).
  - "13 invariant tests, 0 ignored" -> REPRODUCED (13 + 3 doctests, 0 ignored).
  - "core harness 10,000 random seeds, zero invariant violations (master seed
    1781116180331)" -> REPRODUCED at that exact seed.
  - "10,000 scenarios... 33,444 orders, 51,284 proposals, 131,071 injected cognition
    failures, 116,735 beliefs (master seed 1781116572856)" -> REPRODUCED EXACTLY.
  - "58 commits", "~40,700 lines", "thirteen crates" -> 58, 40,674, 13. Match.
- Claims that do NOT hold:
  - §2 "every deviation from spec with rationale": omits the three cited spec
    divergences left open from committed prior reviews ((a),(b),(c) above).
  - §4 "GAPS.md operator-blocked-only with unblocks: DONE" and the belief-pipeline
    row: misrepresentations per A10/A7.
  - §1 "Kelly haircut by calibration quality (fail-closed)" listed under what was
    built: exists only as an uncalled function.

## Findings

- [Critical] Protected crate touched within the graded range (3 commits:
  f463de2-lineage previously flagged, plus 6276274 new this gate) — automatic BLOCK
  pending operator adjudication per CLAUDE.md; content analysis for the operator: all
  touches are pure-addition or sanctioned stub-implementation; zero assertion-logic
  modifications in full history. Reproduction: `git log --oneline --all --
  crates/fortuna-invariants/` + per-commit diffs quoted in Group 5.
- [Major] Spec 5.8/5.9 sizing stages (calibration-adjusted p, fractional Kelly,
  calibration-quality haircut) absent from the ONLY composed sizing path; model-trade
  sizing spends full envelope headroom up to caps (anti-conservative vs spec) —
  reproduction: runner.rs:478 + zero-call-site greps; spec 5.8 "Calibration layer
  adjusts p", spec line 240 "fractional Kelly (default 0.25) on calibrated edge...
  haircut by category calibration quality". Third gate carrying this finding.
- [Major] AnthropicMind not behind the Mind trait and unconstructable from any
  composition (spec 5.9 "both behind Mind"); GAPS/FINAL_REPORT misframe it as
  credentials-blocked — reproduction: impl-Mind grep + zero construction sites.
- [Major] Per-cycle cost budget non-functional for any positive cap (spec 5.9
  "Per-cycle and per-day cost budgets in config; budget breach degrades...") —
  reproduction: mind.rs:221-238; only the degenerate cap<=0 case is tested.
- [Major] Discrepancy/watchdog machinery and 8 doctrine scenarios absent from the
  seeded DST corpus (PROMPT.md:103-104 "DST corpus: every scenario in the doctrine
  list present"); Phase 1 EXIT "discrepancy paths exercised in DST" still unmet —
  reproduction: Group 3 A4(ii) greps; dst.rs:209.
- [Major] Acceptance-gate misrepresentation: with both prior gate reviews committed
  in-tree at T3.3 (6276274), T3.6 closed only (e)/(f)/(j) and declared "every
  checklist item DONE or OPERATOR-BLOCKED" (BUILD_PLAN:258, FINAL_REPORT §4,
  GAPS.md:6-10) while Majors (a)-(d) remain open, unledgered, and absent from the
  deviations list; ASSUMPTIONS.md:779 and cycle.rs:9-10 assert the missing Kelly
  wiring exists. Violates PROMPT "Claim completeness you have not verified" and the
  GAPS acceptance item's substance.
- [Minor] run_watchdogs early-returns on venue.positions() error, starving
  overdue/dispute checks that need no venue data (runner.rs, unchanged).
- [Minor] scripts/run-dst.sh fails open on harness build failure ("passing
  vacuously", exit 0, stderr suppressed; also skips stage 2 on that branch).
- [Minor] Regression corpus empty while T0.4's note records red first runs;
  "regression seeds committed" satisfied only vacuously (disclosed in FINAL_REPORT).
- [Minor] f64 money arithmetic at sizing.rs:68 (unledgered) and cycle.rs:117;
  conservative direction; sizing.rs path currently dead code.
- [Minor] Model ProposalDrafts silently discarded without a model_call audit record
  in the composed loop (cycle.rs reads beliefs/cost only).
- [Minor] Divergence detector and orphan watchdog are dormant-by-default: their
  composition feeds (set_canonical_resolution / set_position_coverage) have zero src
  call sites; design ledgered in ASSUMPTIONS.md T3.6, activation unproven outside
  dedicated tests.
- [Minor] 4 tracked .DS_Store files.
- [Info] OPERATOR-BLOCKED set with valid exact unblocks (GAPS.md): Kalshi fixture
  capture (incl. websocket streams, voided settlement, fee fields), kill-switch live
  venue plug, ANTHROPIC_API_KEY smoke call, Slack app credentials, Aeolus recorded
  export (exact ssh command), Polymarket research+fixtures, spec v0.9 fee touch-up.
- [Info] FINAL_REPORT's quantitative claims all reproduce exactly (Group 6) — the
  build's determinism and the report's statistics are honest; its completeness
  claims are not.

## Verdict rationale

Phase 0 EXIT: MET. Phases 1 and 2 EXIT (strict): NOT MET. Phase 3 EXIT ("fully green
or operator-blocked-only"): NOT MET — A1, A4, A7, A10, A11 fail on implementer-fixable
grounds. The system's core is sound and impressively deterministic: 615/0 tests,
20,000 fresh-seed DST scenarios clean, byte-exact reproduction of the implementer's
recorded runs, all 13 invariant tests green with zero ignored, no test weakening in
history, no secrets, no GatedOrder bypass. The BLOCK rests on (1) the automatic
protected-crate rule (un-adjudicated), (2) four standing Majors from prior gates
((a)-(d)), and (3) the misrepresentation Major: completion was declared by narrowing
the ledgers rather than closing or ledgering the known gaps. Remediation is narrow:
wire calibration+Kelly into handle_proposal for synthesis proposals, put AnthropicMind
behind Mind with a composition path, make the per-cycle cap bind, add
discrepancy/watchdog arms to the seeded harness (or obtain an operator amendment of
the corpus contract), and re-issue GAPS/FINAL_REPORT with the open items ledgered.

## Commands run (verbatim verdict lines; all at 7bbc3ef in /tmp/fortuna-sys0123)

- git worktree add --detach /tmp/fortuna-sys0123 7bbc3ef -> "HEAD is now at 7bbc3ef T3.6 Final report, go-live runbook, acceptance checklist run; close 5.13 watchdogs"
- cargo fmt --check -> FMT_EXIT=0
- DATABASE_URL=... cargo clippy --workspace --all-targets -- -D warnings -> "Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.51s" / CLIPPY_EXIT=0
- DATABASE_URL=... cargo test --workspace -> TOTAL passed=615 failed=0 ignored=0 (exit 0)
- DST_MASTER_SEED=8675309202606 bash scripts/run-dst.sh 10000 ->
  "[dst] regression corpus: 0 seed(s)" / "[dst] master seed 8675309202606 -> 10000 random scenario(s)" /
  "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations" /
  "[synthesis-dst] master seed 8675309202606 -> 10000 scenario(s)" /
  "[synthesis-dst] totals: 33437 orders, 51076 proposals, 131731 cognition failures, 116939 beliefs" /
  "test result: ok. 1 passed; 0 failed... finished in 86.97s" / DST_EXIT=0
- DST_MASTER_SEED=1781116572856 SYNTH_DST_SCENARIOS=10000 cargo test -p fortuna-runner --test synthesis_dst -- --nocapture ->
  "[synthesis-dst] totals: 33444 orders, 51284 proposals, 131071 cognition failures, 116735 beliefs" (matches FINAL_REPORT exactly)
- DST_MASTER_SEED=1781116180331 cargo test -p fortuna-core --test dst -- --nocapture --seeds 10000 ->
  "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations" (matches FINAL_REPORT)
- DATABASE_URL=... cargo test -p fortuna-invariants -> 13 passed / 0 failed / 0 ignored + doctests 3 passed (2 compile-fail)
- bash scripts/replay.sh --seed 424242 -> "[dst] seed 424242: OK"
- cargo test (targeted): observability 1 ok; settlement_loop 11 ok; sim_loop 7 ok; synthesis_loop 3 ok; fortuna-paper 11 ok; mech_extremes 15 ok; veto_loop 8 ok; synth_events_paper 2 ok; cognition mind 6 ok + shadow 7 ok; polymarket_stub 2 ok; fortuna-ops suites ok; ledger aeolus 1 ok; ledger halts_fold 1 ok
- cargo tree -p fortuna-killswitch | grep -icE 'sqlx|postgres|fortuna-ledger|fortuna-cognition|anthropic' -> 0 matches
- git rev-list --count d138a73..7bbc3ef -> 58; find crates -name "*.rs" | xargs wc -l -> 40674
- git diff 1dcd05d..7bbc3ef -- '*.rs' deleted-line audit -> 18 lines, all benign (quoted in session)
