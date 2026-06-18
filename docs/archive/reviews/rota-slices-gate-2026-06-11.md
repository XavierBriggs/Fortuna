# Review: rota-slices-gate (T4.1 increments + remediation2 fixes + T4.3 slices 1-3) — 2026-06-11

Base: dfb849f  Head: 75f4782  Verdict: BLOCK (narrow — one reproduced Major; all six remediation items cleared; battery fully green)
Protected crate touched: no (`git diff dfb849f..75f4782 -- crates/fortuna-invariants/` is empty)

Reviewed in a detached worktree at /tmp/fortuna-g6 (removed after review). Rubric fixed
before reading the diff: GATE-FINDINGS-LATEST items 1-6 (A), docs/design/rota-dashboard.md
incl. binding amendments R1-R12 (B), BUILD_PLAN T4.1/T4.3 + spec Section 8 (C), the
standard battery + fortuna-review mechanical sweeps (D). No other docs/reviews/ file read.

## A. Remediation of GATE-FINDINGS-LATEST 1-6

- A1 (Major, halt/poll-failure dedup hoist): **FIXED** — `last_halt`, `poll_failing`,
  `total_send_failures` now owned at `drive()` scope (daemon.rs:224-226), `last_halt`
  threaded `&mut` into `run_loop`. Committed test
  `a_standing_halt_audits_exactly_once_across_segment_boundaries` crosses THREE
  run_loop re-entries with caller-owned state (run_loop.rs tests) — ran green.
  Independent probe (scratch test, written/run/deleted, worktree verified clean after):
  drove `drive()` ITSELF across FOUR full segments (wakes_per_segment=5, stop at wake
  20) with (i) an always-halted poller -> exactly 1 `kind='halt'` audit row with
  `payload->>'source'='halt_poll'`; (ii) an always-failing poller -> exactly 1 audit
  row containing "halt-state poll FAILING". Both asserts passed
  ("test result: ok. 2 passed; 0 failed ... finished in 8.22s").
- A2 (claim-vs-reality cluster): **FIXED** — (a) GAPS dead-man line now reads
  "independent task, WIRED" with an explicit stale-ledger correction note;
  (b) the dfb849f commit-message discrepancy is recorded in GAPS ("commit MESSAGE
  claimed a GAPS dead-man flip that the commit did not contain ... recorded for the
  trail"); (c) deadman_tick doc rewritten to match the eprintln reality (escalation
  policy is caller-supplied; external monitor's silence-page is escalation of record)
  — the fix-the-doc option the finding allowed; (d) run_loop.rs:9-11 comment updated
  ("drive routes an Ops alert on the failure transition — see daemon.rs").
  Residue (Minor, finding F4 below): the f78ba4e ASSUMPTIONS dead-man entry was left
  in place after 3a56cfc's clean RealClock fix, while GAPS says "No ASSUMPTIONS
  exception is needed"; the entry's first line ("reads SystemTime::now()") no longer
  matches the code.
- A3 (alert_routing audit assertions): **FIXED** — new SharedSink AuditSink;
  `alerts_route_to_slack_and_audit` asserts 2 audit rows + slack-ts marker;
  `no_router_still_audits_zero_posts` asserts 1 row; `slack_send_failure_is_counted_
  never_silent` asserts the row contains "SLACK SEND FAILED". Targeted run: 6 passed.
- A4 (raw SystemTime::now in main.rs): **FIXED** at 3a56cfc — both wall reads (start
  timestamp, dead-man tick) go through `RealClock.now()`. grep of
  fortuna-live/src + fortuna-ops/src for `SystemTime::now|Instant::now|Utc::now`:
  zero code hits (one comment mention). GAPS records the f78ba4e "5 minors" honesty
  correction (finding #4 was NOT in f78ba4e; self-caught, closed in 3a56cfc).
- A5 (understated open-set + _send_failures): **FIXED** — GAPS open-set now lists
  mech_extremes-WITH-VETO binding and mind_from_env/CostBudget binding explicitly;
  `_send_failures` is now `total_send_failures`, summed across segments and audited
  via `apply_external_alert` at shutdown when >0; GAPS claim updated to match.
- A6 (ROTA deadman-age seam): **RESOLVED BY DOC AMENDMENT** (the allowed path) —
  design §5 "DEAD-MAN AGE — CONTRACT RECONCILED": ROTA v1 reports
  `dead_man_last_ping_age_secs: null`; views_from emits null; committed views test
  asserts null ("note-6: dead-man age is null"). No atomic seam added; RotaState has
  no deadman field, consistent with the amended contract.

## B. T4.3 ROTA slices 1-3 vs the amended design (partial build, graded as such)

- B1 (R1 Option-capability): PASS — `RotaState { snapshot (mandatory), pool:
  Option<PgPool>, perishable_dir: Option<Arc<PathBuf>> }`;
  `degraded_surfaces_are_200_with_explicit_unavailable` asserts every view 200 +
  "unavailable" and audit `available:false` with empty views/no pool. Ran green.
- B2 (R2 data plane): PASS — handlers read `snapshot.views` + their own sqlx audit
  query only; `grep metrics_text crates/fortuna-ops/src/rota.rs` = 0 hits (no
  Prometheus-text parsing); `cargo tree -p fortuna-ops -e normal | grep -c
  fortuna-runner` = 0 (no runner edge; deps: core, gates, axum, tokio, sqlx, ...).
- B3 (R3 cursor-polled tail, no SSE): PARTIAL — endpoint exists
  (`?after=&limit=` clamped 1..500, ascending audit_id, next_after returned);
  grep `broadcast|SseHub|text/event-stream` in fortuna-ops/src = 0 hits.
  Cursor-pagination test honestly ledgered as remaining. **But the absent-cursor
  default is defective — finding F1 (Major), reproduced.**
- B4 (R4): PASS — fortuna-gates diff in range is EMPTY; `grep -rn rate_bucket`
  in fortuna-runner/src + fortuna-ops/src = 0 hits.
- B5 (R5 dedicated pool): HONESTLY PENDING — pool is None at every call site;
  max_connections(2)/timeouts not landed; ledgered in GAPS, BUILD_PLAN, and the
  design slice notes. Not graded against.
- B6 (R6 contracts): PASS for landed scope — views_from emits p90/p95/p99 only;
  committed test asserts the p50 key MUST NOT appear; quantiles null (not 0) when
  unobserved; money view absent (honest, needs boards "account"); generated_at is
  ISO8601-Z UTC; empty runner/views render zeros/"unavailable", never 500 (tests
  green; -1 limbo sentinel clamped to 0).
- B7 (R8 lock rule): PASS — read every handler: `read_view` clones
  (generated_at, view) inside a block and releases the read guard before any
  further await; `audit_tail` never touches the snapshot lock; main's
  between-segments closure builds views BEFORE `try_write`.
- B8 (route-table): PASS — `every_path_is_get_only_and_200` covers all 7 routes
  (/rota + 6 /api/rota/v1/*) x GET 200 and POST/PUT/DELETE/PATCH 405; the slice-3
  merge test re-asserts POST 405 through the real `serve_dashboard` tree.
- B9 (R11): PASS — `grid-template-columns:repeat(auto-fit,minmax(320px,1fr))`,
  exactly one line in ROTA_SHELL.
- B10 (shell): PASS — all seven Section 2 tokens present (#0A0A0B, #141416,
  #D4AF37, #FFB84D, #EDEDEA, #FF3B30, #30D158); halt red takeover (`#halt` fixed
  overlay, display:flex when health.halt_active); zero external/CDN fetches (grep
  "http" in rota.rs = only the axum::http import); JetBrains Mono/ui-monospace with
  tabular-nums. `shell_is_gold_on_black_html` pins tokens + "SYSTEM HALTED".
  Note: with the health view degraded, halt_active is undefined -> no takeover
  (acceptable pre-Phase-3 shell; the live daemon always populates health).
- B11 (slice-2 shaping side): PASS — `views_from` lives in fortuna-live (daemon
  side), pure/clock-free (generated_at caller-supplied); rota handlers are a
  literal `views.get(name).cloned()` passthrough; wired in main before try_write.

## C. T4.1 increments

- C1 (aec568b daily scheduler -> digest): PASS with minors — DailyScheduler is
  clock-injected (epoch-day = epoch_millis div_euclid 86_400_000), fires on UTC
  day change; unit test covers the 23:59:59 -> 00:00:00 boundary, same-day
  suppression, skipped days. Digest routes through `route_alerts`
  (MessageKind::Digest -> #fortuna-digest when Slack configured) and is ALWAYS an
  audit row via apply_external_alert — spec Section 8 ("#fortuna-digest: daily
  morning digest"; "every Slack message is also an audit row"). Minors: F3, F5.
- C2 (cf79577 design-blocked claim): VERIFIED REAL AND LEDGERED —
  `SynthesisConfig.edges: Vec<EdgeView>` (synthesis.rs:42) requires an edge source
  the daemon does not have; the open design question (EdgesRepo load vs config vs
  discovery loop) is recorded verbatim in GAPS and BUILD_PLAN; rich digest +
  weekly/monthly reviews correctly chained on it. The box stays unticked — honest.

## D. Battery at 75f4782 (all executed this session, worktree /tmp/fortuna-g6)

- `cargo fmt --check`: exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0
  ("Finished `dev` profile ... in 21.41s", zero warnings).
- `cargo test --workspace`: **suites=107 passed=711 failed=0**.
- fortuna-invariants per-test: 13/13 ok individually (i1 x2, i2 x2, i3, i4, i5,
  i6 x3, i7 x3) + 3 doctests (2 compile-fail) ok.
- `scripts/run-dst.sh 10000`: exit 0 —
  `[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations`;
  `[synthesis-dst] master seed 1781206197525 -> 10000 scenario(s)` ... ok (96.76s);
  `[settlement-dst] arms {SettleClean: 861, SettleThenCorrect: 901, Void: 909,
  Dispute: 1010, VenueMismatch: 891, CanonicalDivergence: 894, OrphanScan: 867,
  AuditDeath: 934, WideBook: 949, Overdue: 882, MultiLegGroup: 902}; 1787
  discrepancies, 2759 watchdog rows, 1826 halts` ... ok (16.10s); daemon smoke
  2 passed.
- Test-weakening sweep over the range: no `#[ignore]`, no proptest case
  reductions, no loosened tolerances; the single removed assertion
  (`stats.halt_polls == 20`) was superseded by the strictly stronger
  cross-segment restructure of the same test. All new unwrap/expect are in test
  files; no new ones in gates/exec/state/venues src.
- Secrets sweep: no token/key/secret literals added. `place(`/GatedOrder sweep:
  zero new call sites/constructors (one GAPS prose hit only).
- Box honesty: T4.1 and T4.3 both UNTICKED with accurate remaining-work lists that
  match the code (verified against GAPS + design slice notes).
  Remaining for T4.3: money/cognition views (R6 boards account field; R7 two
  ledger queries), gates.rejections_by_check accessor, audit recents + R5
  dedicated 2-conn pool, streams fs-scan + book_age_ms, cursor-pagination test,
  Phase-3 shell/assets (logo.svg per §9, per-panel rendering beyond raw JSON),
  R12 browser pass. Remaining for T4.1: synthesis-in-main (edge-source design
  call), mech_extremes-w/-veto binding, mind_from_env/CostBudget binding, rich
  digest + daily reconciliation re-run + weekly/monthly reviews, EXIT week.

## Findings

- [Major] F1: ROTA audit-tail absent-cursor default returns the OLDEST page, not
  the latest — rota.rs `let after = q.after.unwrap_or_default();` binds `""` into
  `WHERE audit_id > $1 ORDER BY audit_id ASC LIMIT $2`, while the field's own doc
  comment says "absent => the latest page", and ROTA_SHELL polls the endpoint
  cursor-less every 2s (never reads next_after) — so once the R5 pool lands, the
  live audit panel permanently displays the first 100 rows ever written: an I5
  audit *tail* that never shows new rows (design R3's purpose inverted).
  Reproduction (executed, scratch Postgres `fortuna_gate_scratch`, dropped after):
  the handler's exact SQL with after=''/limit 2 over rows
  01AAA…(old_row_1), 01BBB…(old_row_2), 01CCC…(newest_row) returned
  old_row_1, old_row_2 — newest_row unreachable. Latent at HEAD (every caller
  passes pool=None), in slice-1 code claimed complete in three ledgers — the
  recurring claim-vs-reality class. FIX (small): default the cursorless page to
  the tail (e.g. ORDER BY audit_id DESC LIMIT n, re-sort ASC, or seed `after`
  from MAX(audit_id)-window) OR make the shell cursor-track via next_after;
  align the comment; the already-owed cursor-pagination test MUST include the
  absent-cursor case.
- [Minor] F2: the rota audit query is runtime `sqlx::query_as` with a string —
  CLAUDE.md: "Postgres via sqlx (compile-time checked queries ...)". The
  cycle-avoidance rationale is in the design slice note, but the convention
  exception is not ledgered in GAPS/ASSUMPTIONS.
- [Minor] F3: DailyScheduler fires on FIRST call — every daemon restart emits an
  immediate "daily digest" mid-day (pinned deliberate by the unit test, but
  restart-flood-adjacent and unledgered as a choice).
- [Minor] F4: ledger contradiction — the f78ba4e ASSUMPTIONS dead-man entry
  ("reads SystemTime::now() at the IO edge") survived 3a56cfc's RealClock fix
  while GAPS states "No ASSUMPTIONS exception is needed"; the entry's opening
  claim no longer matches the code.
- [Minor] F5: terse_daily_digest labels run-cumulative counters as the day's
  ("the day's headline counters" in its doc; `ticks=... orders=...` are
  since-boot totals) — after a multi-day run each "daily digest" reports
  lifetime numbers; and no committed test asserts the digest is actually routed
  + audited through drive() (only the scheduler unit test exists).

## Commands run (verbatim verdict lines)

- cargo fmt --check -> FMT_EXIT=0
- cargo clippy --workspace --all-targets -- -D warnings -> exit 0, "Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.41s"
- cargo test --workspace -> suites=107 passed=711 failed=0
- cargo test -p fortuna-invariants -> 13 tests ok + 3 doctests ok (listed per-test above)
- scripts/run-dst.sh 10000 -> exit 0; "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations"; synthesis "ok. 1 passed ... 96.76s"; settlement "ok. 1 passed ... 16.10s" (11 arms quoted above); daemon smoke "ok. 2 passed"
- scratch drive() probe -> "ok. 2 passed; 0 failed ... 8.22s" (1 halt row / 1 FAILING alert across 4 segments)
- scratch SQL reproduction (F1) -> returned old_row_1, old_row_2 for the absent-cursor default
- cargo tree -p fortuna-ops -e normal | grep -c fortuna-runner -> 0
- git diff dfb849f..75f4782 -- crates/fortuna-gates/ crates/fortuna-invariants/ -> empty
