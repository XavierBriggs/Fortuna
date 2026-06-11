# Review: T4.1 daemon increments + remediation — 2026-06-11
Base: 1b3fabb  Head: 817d2e7  Verdict: BLOCK
Protected crate touched: no (0-byte diff for crates/fortuna-invariants/ over the range)

Gated in a detached worktree (/tmp/fortuna-g4) at commit 817d2e7; the dirty live tree
was never used for evidence. DATABASE_URL=postgres://localhost/fortuna_dev.

## A. Remediation of GATE-FINDINGS-LATEST items 1-4

- A1 (Major, audit-death group fall-through): **FIXED** — evidence:
  - Fix at crates/fortuna-runner/src/runner.rs:877-888 (mid-Phase-A audit-dead check:
    release every staged reservation, return before the venue) + :934-941 (belt before
    Phase B). Reservation for the CURRENT leg is taken only in the Ok(gated) arm
    (:919), after the check — no leak. Phase A holds no journal writes (journaling is
    inside submit_group_concurrent, Phase B) — no orphan journal rows from the abort.
    Spec basis confirmed: docs/spec.md:367 "Audit write failure | Trading halts (no
    audit, no trading)".
  - Committed regression: crates/fortuna-runner/tests/audit_death_staging.rs — a
    boundary SWEEP (every fail point before first venue contact => 0 submissions +
    halt). Ran green: `test result: ok. 1 passed` (0.02s).
  - Independent verifier probe (scratch, deleted after run): 3-leg group, audit sink
    killed at rows {15,18,19,20,21,22,23} — `probe: 7/7 fail points landed
    mid-Phase-A`, all yielded orders_submitted=0, halted=true, AND
    fortuna_reserved_exposure_cents == 0 (reservations released — the committed test
    does not assert this; mine did, it holds).
  - Residue [Minor]: the finding asked for "a DST arm if expressible" — settlement_dst
    Arm::AuditDeath still asserts halt only (not zero-submissions-after-mid-staging
    death), no arm was added, and the absence is NOT ledgered in GAPS.md.
- A2 (clippy red at head): **STILL-OPEN — REGRESSED AGAIN.** `cargo clippy --workspace
  --all-targets -- -D warnings` at 817d2e7: **exit 101** — `error: this function has
  too many arguments (8/7)` at crates/fortuna-live/src/daemon.rs:191 (`drive`). The
  8th argument landed at 77588c5: the final TWO commits of this range shipped
  clippy-red. This is the exact defect class item 2 flagged ("the last commit shipped
  without the full DoD battery"), repeated in the very range that was meant to fix it.
- A3 (false present-tense SIGTERM comment): **FIXED as written, re-stale in reverse**
  — the diff is comment-only (no assertions touched), but at HEAD
  crates/fortuna-live/tests/shutdown.rs:6-8 now claims the composition main "does not
  exist yet" while 77588c5 landed both main and the handler. [Minor, doc-drift class]
- A4 (BINDING SHUTDOWN CONTRACT): **STILL-OPEN (handler done, assertion not).**
  - Handler EXISTS: main.rs:96-110 installs SignalKind::terminate (+ ctrl_c), both
    feed ONE oneshot stop channel; run_loop exits on it (run_loop.rs:98-102); drive()
    then runs SimRunner::shutdown (daemon.rs:236) — same path, signal or not.
  - The contract's ASSERTION does not exist: no committed test delivers an actual
    SIGTERM, and daemon_smoke's books (qty 80) fully fill — no working orders exist at
    stop, so "SIGTERM => cancel working orders" is never asserted through the signal
    path (cancels are asserted only via direct shutdown() calls in shutdown.rs).
  - Verifier live probe (out-of-repo evidence the WIRING works): booted the real
    binary against a scratch Pg DB from the committed example config, sent OS SIGTERM:
    "fortuna-live: SIGTERM" -> "clean shutdown — ticks=5 polls=11 ... cancelled=0
    unacked=0", exit clean, exactly 1 `daemon_shutdown` audit row. The test obligation
    remains; per the contract text this requirement is STILL unmet.

## B. T4.1 increments vs the BUILD_PLAN T4.1 entry

- B1 Run loop (10430c0): **PASS** — wall time enters only at main.rs:57 (binary edge)
  and RealCadence's tokio sleep (run_loop.rs:54-61), which then advances the injected
  SimClock; workspace grep shows no other SystemTime/Instant outside clock impls,
  tests, the recorder binary, and the fixtures example (preexisting). halt_poll_ms
  <=500 enforced fail-closed at boot (boot.rs:229-236) and exercised in tests
  (run_loop.rs: 10 wakes -> 10 polls/5 ticks; polled halt applies to gates AND audits
  with source=halt_poll; poll failure counted, non-fatal, loop keeps polling). Smoke
  deterministic under SimClock, committed into scripts/run-dst.sh stage 5, green
  twice this session. Two findings below (halt re-audit flood; poll-failure alert).
- B2 Cognition compose (25bfb1b): **PARTIAL** — (i) DegradeScrape IS the
  degrade_alerts consumer (compose.rs:109 calls fortuna_ops::alerts::degrade_alerts)
  and drive() calls it once per segment, routing alerts via apply_external_alert into
  audit rows (daemon.rs:217-223); scrape semantics pinned 4 ways (compose.rs test:
  alert-once, zero-delta silent, threshold burst, restart-reset saturation). No test
  drives a real alert through drive() to an 'alert' audit row; Slack routing
  outstanding (honestly ledgered). (ii) calibration_for_scope fetches latest +
  resolved_stats -> CalibrationContext + quality; fail-closed is TEST-PINNED: no
  params row => ctx None + quality 0.0 ("sizes zero" — the E1 behavior holds at this
  seam); corrupt params row errors loudly (test green). NOT yet fed into a booted
  SynthesisStrategy in main (mech_structural-only composition; honestly ledgered).
- B3 Composition main (77588c5): **PASS with ledgered gaps** — config fail-closed
  (typed BootError; missing sections refuse; placeholder env refusal; stub mind is
  opt-in, main.rs:48-54); venue from config and kalshi REFUSES (boot.rs:214-221;
  test boot.rs:114 venue_kalshi_refuses_until_fixture_clearance); Sim-only default
  (example venue="sim"); metrics GET-only (fortuna-ops dashboard test asserts POST
  =>405, ran green; live probe GET=200/POST=405); AuditWriter mandatory
  (compose_runner unconditionally builds PgAuditSink; Pg connect failure aborts
  boot); shutdown wired through drive(). Binary boot VERIFIED LIVE against a scratch
  Postgres (created/dropped this session) using the committed example config: 5
  ticks, SIGTERM, clean exit, exactly 1 daemon_shutdown row. Dead-man pinger NOT
  wired — deliberate (first ping arms the monitor), ledgered in GAPS + main.rs note;
  requirement outstanding, and consequently no test pings any URL.
- B4 Belief drain (817d2e7): **PASS at path level** — Strategy::drain_beliefs
  (default empty), SynthesisStrategy buffers outcome.beliefs including shadow cycles,
  runner drains per tick, persist_beliefs upserts events FK-first then inserts
  beliefs WITH provenance; drain-once + event-upsert idempotency test-pinned
  (belief_persist.rs green). Zero-capital preserved (producer emits no proposals;
  persistence touches no money path). Gaps [Minor]: no audit row accompanies belief
  persistence; belief_id is a hand-rolled "01BLF{:021}" string, not a ULID (CLAUDE.md
  "IDs are ULIDs"); event-duplicate detection sniffs error strings
  ("duplicate"/"unique") instead of ON CONFLICT; daemon main does not yet drain or
  call persist_beliefs (no belief-producing strategy composed — honestly ledgered).
- B5 Completion accounting: **HONEST** — the T4.1 box is UNTICKED (BUILD_PLAN:274).
  DONE-with-evidence: config fail-closed; Pg repos + mandatory audit; clock-edged run
  loop + <=500ms halt poll + audited external halts; metrics GET-only; kalshi
  refusal; daemon-smoke DST; graceful-shutdown core; belief persistence PATH;
  degrade-scrape consumer + calibration fetch (compose layer). OUTSTANDING:
  mind_from_env/AnthropicMind composition; synth_events + mech_extremes in main;
  scheduled daily/weekly/monthly loops; Slack routing (every message an audit row);
  dead-man pinger; poll-failure ALERT; the SIGTERM contract assertion; belief drain
  in main. GAPS.md matches this set except the poll-failure alert and the DST-arm
  absence, which are not ledgered.

## Findings

- [Major] CLIPPY RED AT HEAD (remediation item 2 repeated): clippy exit 101,
  daemon.rs:191 too_many_arguments (8/7), introduced 77588c5 — two commits shipped
  without the DoD battery in the range that was supposed to close this finding.
- [Major] Durable-halt re-audit flood, REPRODUCED: PgHaltPoller returns the active
  halt on EVERY poll and run_loop re-applies it (run_loop.rs:106-110 ->
  apply_external_halt audits each time). Probe: 20 wakes under one persistent halt ->
  halts_applied=20, 20 'halt' audit rows — ~2 rows/sec (~172,800/day) into the
  append-only audit table for the life of any halt, plus halts_applied metric
  inflation. Needs dedup (apply-once-per-halt-identity) before any continuous run.
- [Major / pending contract] BINDING T4.1 shutdown contract still unmet: handler
  exists and routes correctly, but no committed test asserts SIGTERM -> cancel
  working orders + final audit row (see A4). `fortuna stop` (T4.4) stays blocked.
- [Minor] DST arm for audit-death-mid-staging neither added nor its absence ledgered
  (GATE item 1 tail; settlement Arm::AuditDeath asserts halt only).
- [Minor] Poll-failure ALERT (T4.1 requirement text "halt-state poll <= 500ms w/
  poll-failure alert") unimplemented: failures counted in LoopStats, printed only at
  exit; DegradeScrape carries no poll-failure signal; run_loop.rs:5 comment implies
  the alert "rides the req-3 degrade wiring" — it cannot. Not ledgered.
- [Minor] shutdown.rs:6-8 comment stale in reverse at HEAD ("does not exist yet").
- [Minor] persist_beliefs: non-ULID belief ids; error-string duplicate sniffing; no
  audit row for belief persistence.

## Commands run (verbatim results)

- `cargo fmt --check` -> FMT_EXIT=0 (0 lines of output)
- `cargo clippy --workspace --all-targets -- -D warnings` -> CLIPPY_EXIT=101;
  "error: this function has too many arguments (8/7) --> crates/fortuna-live/src/daemon.rs:191"
- `cargo test --workspace` -> exit 0; 103 result lines; total passed: 691, failed: 0
- `cargo test -p fortuna-invariants` -> 13 tests + 3 doctests, all ok (per-test listed
  in session; i1 x2, i2 x2, i3, i4, i5, i6 x3, i7 x3)
- `scripts/run-dst.sh 10000` -> DST_EXIT=0
  - "[dst] OK: 0 corpus + 10000 random seeds, zero invariant violations"
  - "[synthesis-dst] master seed 1781185110801 -> 10000 scenario(s)" / "totals: 27277
    orders, 41933 proposals, 132594 cognition failures, 118688 beliefs" -> ok (93.82s)
  - "[settlement-dst] arms {SettleClean: 889, SettleThenCorrect: 927, Void: 865,
    Dispute: 920, VenueMismatch: 928, CanonicalDivergence: 946, OrphanScan: 924,
    AuditDeath: 908, WideBook: 894, Overdue: 927, MultiLegGroup: 872}; 1874
    discrepancies, 2771 watchdog rows, 1836 halts" -> ok (16.13s)
  - daemon_smoke_boot_ticks_signal_shutdown -> ok (0.87s)
- `cargo test -p fortuna-runner --test audit_death_staging` -> ok, 1 passed
- Verifier probes (scratch, deleted): i5 staging probe -> 7/7 mid-Phase-A fail points
  clean (0 submitted, halted, 0 reserved); halt-spam probe -> 20/20 re-audited
  (the Major above); live binary boot + OS SIGTERM -> clean, 1 daemon_shutdown row,
  metrics GET 200 / POST 405
- Mechanical sweeps over 1b3fabb...817d2e7: no unwrap/expect/panic added in src; no
  test weakening (shutdown.rs change is comment-only); no secrets; no f64 on money;
  no GatedOrder construction outside fortuna-gates; no HashMap iteration in core
  paths; `git diff -- crates/fortuna-invariants/` -> 0 bytes
