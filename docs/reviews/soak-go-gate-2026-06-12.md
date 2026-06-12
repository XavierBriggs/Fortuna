# Review: soak-go-gate — 2026-06-12 (Phase-4 EXIT soak GO/NO-GO)
Base: 93844eb  Head: 8ea8a4d  Verdict: ACCEPT (SOAK: GO)
Protected crate touched: NO (net diff vs crates/fortuna-invariants/ = 0 lines; the
perps-merge additions a586b4a were reverted in-range by 19b3888 — net cancel)

Reviewed independently in a detached worktree (/tmp/fortuna-gc @ 8ea8a4d,
CARGO_TARGET_DIR=/tmp/fortuna-gate-target). Rubric inputs: spec 5.4 + 5.8,
BUILD_PLAN T4.1/T0.6 contracts, GATE-FINDINGS-LATEST.md (client-id diagnosis),
operator-decisions-2026-06-12.md. No other docs/reviews file read (independence).

## Range contents (net)
12 files: the T4.1/M2 resolution batch (daily-reconciliation helper + drive()
wiring; [review] config + Weekly/MonthlyScheduler; weekly-review helper + wiring;
monthly-review helper + wiring), c25b368 (exec idempotency adjudication + pin),
two docs commits (revert diagnosis, operator decisions/leverage build item).
The perps merge + revert cancel to zero. BUILD_PLAN.md NOT touched (see Finding 1).

## Criteria (fixed before reading the diff)

### A — exec idempotency commit c25b368 (money path, maximum suspicion)
- A1 Adjudication sound vs spec 5.4 + T0.6: PASS — all three cited code facts
  verified by reading HEAD: (1) ClientOrderId::from_intent is PURE
  ("fortuna-{intent}", fortuna-core/src/market.rs:168); (2) new-intent IntentIds
  are IdGen-sequence draws (fortuna-runner/src/runner.rs:910
  `IntentId::new(self.ids.next(self.clock.now())?)`) — so NEW ids shift across
  builds, which is within-build-replay-only territory; (3) crash recovery never
  consults the IdGen: by_coid is rebuilt from the journal FOLD of persisted ids
  (manager.rs:208-218, fold insert :916) and boot_reconcile matches venue open
  orders/fills against it (:779, :562). Spec 5.4 "Client order ids are derived
  deterministically from intent ids, so resubmission after a crash is idempotent
  by construction" — holds across upgrades because the persisted id is READ, not
  recomputed. Option-1 rejection (context-free derivation) is sound: zero
  idempotency benefit given the persisted-id proof. The commit changes NO src
  (exec net diff: tests/manager.rs +55/-0 only).
- A2 Upgrade-safety pin mechanism + mutation check: PASS — the pin
  (crash_recovery_adopts_a_resting_order_via_its_persisted_client_order_id) is a
  derivation-PATH assertion: timeout-but-placed -> crash -> recover -> adopt via
  persisted id with NO re-gating (so the IdGen is provably not consulted),
  isolating what the older crash_resubmission test confounds (its candidate(seed)
  re-derivation is IdGen-pure). MUTATION-CHECKED in the worktree, both RED:
  M-A boot match forced to None (manager.rs:779) -> FAILED at tests/manager.rs:286;
  M-B fold key mangled ("-upgrade-shifted", manager.rs:916) -> FAILED at :278.
  Both reverted; worktree clean (git status porcelain: 0 lines).
  Calibration note: a mutation that purely RECOMPUTES from_intent(persisted
  IntentId) at recovery would not turn the pin red — but that mutation is itself
  upgrade-safe (pure fn of a persisted value), so the pin covers exactly the
  load-bearing failure class (IdGen consultation / persisted-pathway divergence).
- A3 T0.6 tests byte-untouched + green; DST crash arms unaffected: PASS —
  `git diff 93844eb..8ea8a4d -- crates/fortuna-exec/` = 55 insertions, 0 deletions;
  all 26 manager tests green (26/0/0, includes crash_resubmission_*, boot_*,
  recovery_rebuild_is_idempotent); DST stage 1 zero violations (below).
- A4 Resolves the perps-merge failure mode: PASS WITH SCOPE STATED — the commit
  answers the load-bearing question (idempotency IS upgrade-stable) and pins it;
  it does NOT itself make a re-merge green. The unstable artifact was the
  kinetics_adapter.rs:168 TEST (pins a tree-state-dependent UUID; left f6384bf5
  vs right c445aeac), correctly disposed to track-C. RE-MERGE STILL NEEDS:
  (1) the kinetics test deriving its expectation through the same path (or an
  id-agnostic recorded-create assertion); (2) the operator's 2x leverage-cap
  build item riding the same track (operator-decisions item 4: [perp]
  max_leverage, min(config, venue curve), 2.01x-refused/1.99x-passes pin,
  ASSUMPTIONS note); (3) a full re-gate of the merged tree. Operator signatures
  (waive batch 5 + F1) remain valid per the bus.

### B — M2 completion (T4.1 tick must be simply true)
- B1 Daily reconciliation: PASS — rides drive()'s existing 00:00 UTC
  DailyScheduler boundary (daemon.rs, same due() as the digest); clock-injected
  (id_base from injected now.epoch_millis(), no wall read); journal-or-error:
  every non-write path audits (journal written / skipped: no journal / failure: e)
  and DB errors at the boundary route an Ops alert without crashing the loop;
  I6/spec-5.8 "No orders are placed from this loop" is STRUCTURAL
  (ReconciliationOutcome carries none; proposals counted-as-discarded) and
  test-asserted (orders_submitted unchanged). Restart-across-boundary: idempotency
  keys on POSTGRES (unique idx_journal_day, migration 20260609000001:265 +
  get_day pre-check), not process state — second same-day call asserted a no-op.
  Tests: daily_reconciliation_writes_a_journal_and_places_no_orders,
  daily_reconciliation_gracefully_skips_when_the_mind_writes_no_journal,
  drive_runs_daily_reconciliation_at_the_utc_day_boundary — all green; wiring
  test non-vacuous (sibling drives pass None and write nothing).
- B2 Weekly + monthly reviews REUSE T3.1 machinery: PASS —
  fortuna_cognition::review::{weekly_review, monthly_review} are the T3.1
  deliverable (d205787), reconciliation is T2.7 (38766b4); ZERO fortuna-cognition
  commits in range (`git log 93844eb..8ea8a4d -- crates/fortuna-cognition/` = 0).
  The daemon ASSEMBLES inputs (ScopeRecord from resolved_stats, prior versions
  from CalibrationParamsRepo.latest, StrategyRecord from digest_snapshot,
  AllocationInput from envelopes+digest+cognition counters) and routes outputs
  through route_alerts: weekly summary -> MessageKind::Digest, lesson candidates
  -> ::Review, monthly operator drills -> ::Ops, failures -> ::Ops — route_alerts
  audits EVERY message with or without a Slack router and audits send failures
  (daemon.rs:1234-1266). I7 honored: recommendations only, promotion/demotion/
  drills are operator items. Audit rows asserted in all four review tests.
- B3 Boundary determinism: PASS — WeeklyScheduler ((epoch_day+3).div_euclid(7),
  Monday-aligned; arithmetic re-verified by hand: 2026-06-11 = day 20615 -> week
  2945, Mon 2026-06-15 = 20619 -> 2946 exactly) and MonthlyScheduler ("YYYY-MM"
  key) are pure functions of injected now — no wall-time read; exact
  firing-sequence unit tests over fixed timestamps
  (weekly_scheduler_fires_once_per_monday_aligned_week,
  monthly_scheduler_fires_once_per_calendar_month) + exact-count boundary
  assertions under SimClock in the drive() e2e tests. Repo wall-clock sweep:
  SystemTime::now/Instant::now/Utc::now hits ONLY in fixture-recorder examples
  (operator tooling, pre-existing) — none in fortuna-live src or any new line.
- B4 Completion accounting: PASS with one Minor (Finding 1) — every T4.1 element
  now has executed evidence; GAPS records "===> M2 IS FULLY RESOLVED" (GAPS:319)
  with the honest monthly-won't-fire-in-a-week caveat. BUT two stale disclosure
  sites were never visibly corrected: GAPS.md:83 still lists "[Major M2] ...
  unbuilt ... queued" un-annotated, and BUILD_PLAN.md's T4.1 tick still says
  "HONESTLY DEFERRED post-tick ... daily reconciliation re-run + weekly/monthly
  cognition reviews" (BUILD_PLAN untouched in range).

### C — m1/m2 cleanups
- C1 Refresh-failure integration test has teeth: PASS — green
  (refresh_failure_keeps_last_known_edges_alerts_and_survives, 1/0/0 in 0.26s);
  non-vacuous by construction (booted edge superseded by an UNCONFIRMED successor,
  so a successful refresh reads zero — post-drive count 1 can only mean the
  failure arm retained last-known); MUTATION-CHECKED: fail-opening the failure
  arm (runner.refresh_synthesis_edges(&[]) on Err) -> FAILED at daemon_smoke.rs:596.
  Reverted; tree clean. (Note: test landed AT the base commit 93844eb, not inside
  the range — verified present and toothed at HEAD regardless.)
- C2 Categories-allowlist ledger consistency: PASS — decision doc
  (synthesis-edge-source-decision.md:34-41) and GAPS [Minor m1] carry the SAME
  disposition (deferred-by-choice, narrows-only, absence NOT fail-open,
  SynthesisSection.category is the calibration-scope selector not this filter)
  with mutual cross-references; the stale "(deferred to S3b)" pointer corrected.

### E — full battery (verbatim verdict lines)
- `cargo fmt --check` -> exit 0
- `cargo clippy --workspace --all-targets -- -D warnings` -> "Finished `dev`
  profile ... in 14.87s", exit 0
- `cargo test --workspace` -> TOTAL passed=803 failed=0 ignored=0, exit 0
- invariants per-test: i1 (2), i2 (2), i3, i4, i5, i6 (3), i7 (3) + 3 doctests
  (2 compile-fail) — ALL ok individually
- `scripts/run-dst.sh 10000` -> exit 0, all stages:
  "[dst] OK: 3 corpus + 10000 random seeds, zero invariant violations"
  "[synthesis-dst] master seed 1781307980119 -> 10000 scenario(s) ... ok (92.43s)"
  "[settlement-dst] master seed 1781308074918 -> 10000 scenario(s); arms
   {SettleClean:885 ... MultiLegGroup:928}; 1797 discrepancies, 2785 watchdog
   rows, 1799 halts ... ok (16.09s)"
  daemon_smoke 15 passed / 0 failed (36.98s)
- Sweeps over the net range diff: ZERO hits — no deleted test lines anywhere
  (`git diff ... -- "*/tests/*" | grep "^-"` empty), no #[ignore], no proptest
  reductions, no new unwrap()/expect(/panic!/todo! in src, no wall-clock, no
  secrets literals, no new place( call sites, no GatedOrder construction outside
  fortuna-gates. Protected crate: `git diff 93844eb..8ea8a4d --
  crates/fortuna-invariants/ | wc -l` = 0.
- New-code determinism: BTreeMap throughout; one ORDER-BY-less SQL (lessons,
  monthly review) feeds COUNTS only (Finding 4, Info).

## Findings
- [Minor] Stale M2 disclosure sites un-annotated: GAPS.md:83 "[Major M2] ...
  unbuilt ... queued" and BUILD_PLAN.md T4.1 "HONESTLY DEFERRED post-tick"
  contradict GAPS:319 "M2 IS FULLY RESOLVED". House practice is correct-visibly;
  annotate both sites. — reproduction: grep -n "Major M2" GAPS.md; grep -n
  "HONESTLY DEFERRED" BUILD_PLAN.md at 8ea8a4d.
- [Minor] No committed OPERATOR RUNBOOK (start/stop/observe) — the T4.1 kickoff
  (docs/kickoff/T4.1-kickoff.md:126) names it; grep for a start command across
  committed .md finds none. The start contract below is reconstructed from
  main.rs + validate_env + the example config; commit it as a runbook before or
  with the soak start. — reproduction: grep -rn "target/release/fortuna-live"
  --include="*.md" . -> 0 hits.
- [Info] In-memory schedulers re-fire weekly/monthly reviews on daemon restart
  (fire-on-boot pattern, ledgered-as-intended for the daily digest; daily
  reconciliation is Postgres-idempotent, the reviews are not). Soak-watch must
  treat boot re-fires as EXPECTED, not anomalies.
- [Info] lessons query lacks ORDER BY (counts-only consumption today — no
  determinism leak; latent if a future consumer iterates rows). Journal row id
  "01JRN{:021}" is deterministic-ULID-shaped, not a real ULID (replay-friendly;
  note in ASSUMPTIONS eventually).
- [Info] StrategyRecord approximations (invariant_violations=0, paper_days=
  daemon uptime, synth-attributed cognition cost) — disclosed in GAPS; GO/NO-GO
  is advisory (I7), acceptable.
- [Prior-gate item, still open, NOT this range] M3 rearm notices (CLI "pending
  restart" + ROTA health surface) — land before the soak's FIRST HALT DRILL
  (pool-owned per GATE-FINDINGS).

## D — SOAK GO/NO-GO

GO. The daemon at 8ea8a4d is fit to start the 7-day Phase-4 EXIT soak NOW.
Every gate that was holding it (t41 M1 example-config pin, operator M2 decision,
the perps-revert idempotency question) is closed with executed evidence; main is
green across the full battery; the money-path commit changes zero src lines and
its safety claim is mutation-pinned. The two Minors are documentation items —
ledger them; the runbook should land immediately. The START is the operator's
(outward-facing secrets + release build), per BUILD_PLAN T4.1.

SOAK-WATCH METRICS (orchestration item 7 + this batch; log each verifier firing
to docs/reviews/soak-log.md):
1. Daemon uptime (restarts noted — each restart re-fires reviews, expected).
2. Halt count — zero UNEXPLAINED; any halt_events row gets a cause.
3. Mind budget burn vs [cognition] daily/per-cycle budgets (degrade alerts).
4. Dead-man freshness (FORTUNA_DEADMAN_URL pings every minute).
5. Belief-persistence growth (beliefs table monotone under the synthesis arm).
6. NEW: exactly one journal row per UTC day (journal table, unique day index);
   reconciliation skip/failure audit rows are the honest-degrade signal when the
   mind is unkeyed or Postgres blips.
7. NEW: weekly_review audit row + #digest summary once per Monday-aligned week
   (boot fire on day one, then 2026-06-15, -22, -29 for a soak started now).
8. NEW: monthly_review audit row only if the soak crosses 2026-07-01 (plus the
   boot fire).
9. Slack send-failure markers ("[SLACK SEND FAILED:" audit rows) + the shutdown
   summary row ("N Slack alert send(s) failed over this run").
10. Metrics endpoint reachable (GET-only) + halt-poll-failure alerts absent.

OPERATOR START COMMAND (from committed artifacts: main.rs contract, validate_env,
config/fortuna.example.toml):
    cd /Users/xavierbriggs/fortuna
    cargo build --release -p fortuna-live
    # .env (or systemd EnvironmentFile) MUST provide: DATABASE_URL,
    # FORTUNA_SLACK_BOT_TOKEN, FORTUNA_SLACK_CHANNEL_ALERTS, _DIGEST, _OPS,
    # _REVIEW, _TRADING, FORTUNA_DEADMAN_URL, and ANTHROPIC_API_KEY
    # (or set [cognition] allow_stub_mind = true to opt into the stub).
    set -a && source .env && set +a
    ./target/release/fortuna-live config/fortuna.toml
    # config/fortuna.toml copied from config/fortuna.example.toml (now ships
    # [review] + the synthesis envelope); venue stays "sim" (kalshi refuses
    # without fixture clearance). Stop: SIGTERM (graceful-shutdown contract).

## Commands run (verbatim, this session, worktree /tmp/fortuna-gc @ 8ea8a4d)
    cargo fmt --check                                              # exit 0
    cargo clippy --workspace --all-targets -- -D warnings          # exit 0
    cargo test --workspace                                         # 803/0/0
    cargo test -p fortuna-invariants                               # all green
    cargo test -p fortuna-exec --test manager                      # 26/0/0
    scripts/run-dst.sh 10000                                       # exit 0
    # mutations (applied -> test -> reverted; final git status clean):
    #   manager.rs:779 match->None        => pin FAILED (manager.rs:286)  RED
    #   manager.rs:916 fold key mangled   => pin FAILED (manager.rs:278)  RED
    #   daemon.rs Err-arm refresh(&[])    => refresh test FAILED (:596)   RED
