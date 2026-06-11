# Review: T4.1 remediation round 2 + degrade-alerts/dead-man increments — 2026-06-11
Base: 817d2e7  Head: dfb849f  Verdict: BLOCK
Protected crate touched: no (0 files under crates/fortuna-invariants/ in range)

Range: 871c339 (degrade alerts -> Slack + audit), 5a5ffe3 (gate remediation,
claims all 7 findings fixed/ledgered + env Slack router), dfb849f (dead-man
heartbeat wired). Gated at COMMIT dfb849f in a detached worktree
(/tmp/fortuna-g5); the dirty working tree was not reviewed.

## Criteria (fixed before reading the diff)

### A — remediation of GATE-FINDINGS-LATEST (1b3fabb..817d2e7) items 1-8

- A1 clippy green at HEAD + battery discipline (item 1, Major/process): **FIXED** —
  CLIPPY_EXIT=0 at dfb849f; spot-check worktree at 871c339 (first in-range commit)
  also CLIPPY_871_EXIT=0; all three commit messages carry battery results with real
  exit codes; docs/design/implementer-loop.md amended: "THE BATTERY IS A COMMIT-GATE".
- A2 halt re-audit flood (item 2, Major): **STILL-OPEN — reproduced.** The committed
  test (run_loop.rs::a_standing_halt_audits_exactly_once_over_many_polls, 20 polls =>
  exactly 1 audit row) passes, BUT the dedup state `last_halt` is local to
  `run_loop` (run_loop.rs:92) and `drive` (daemon.rs:214-223) re-enters `run_loop`
  once per segment, resetting it. Scratch probe (verifier, worktree-only,
  tests/scratch_probe.rs): ONE standing halt across 4 segments (20 polls, segment=5)
  => `halts_applied: 4`, FAILED `assertion left == right, left: 4, right: 1`.
  At main.rs:180 (60-wake segments ~30s) one standing halt re-applies + re-audits
  ~2,880x/day. The finding demanded "apply/audit once per halt IDENTITY"; delivered:
  once per identity PER SEGMENT. 60x amplitude reduction, semantics not met; the
  committed test never crosses a segment boundary so it cannot see this.
- A3 SIGTERM contract assertion (item 3, Major/contract): **FIXED + LEDGERED** —
  daemon_smoke.rs::signal_with_working_orders_cancels_them_and_audits committed and
  green: depth-1 asks leave resting working orders; stop fires the same channel
  main's SIGTERM handler fires (main.rs:94-110); asserts cancelled>=1, journaled
  cancel events >=1, exactly one `daemon_shutdown` audit row. Real OS-signal
  delivery ledgered in GAPS (F3, "the untestable seam") — within the finding's
  explicit allowance. (Rationale is contestable — raise(SIGTERM) in-process is
  feasible — but the prior gate verifier-probed real delivery.)
- A4 poll-failure alert + comment (item 4, Minor): **FIXED with residue** — drive
  pushes an Ops alert when a segment had poll failures (daemon.rs:238-247), routed +
  audited via route_alerts. RESIDUE: run_loop.rs:9-11 still says the Slack alert
  rides the degrade wiring "when the composition lands" — it landed in this range;
  comment stale in the opposite direction (again). And the alert has no
  cross-segment dedup (see finding 4).
- A5 shutdown.rs stale comment (item 5, Minor): **FIXED** — now states main's
  SIGTERM/SIGINT handler routes to shutdown() via the stop channel.
- A6 reservation-release assertion + DST-arm ledger (item 6, Minor): **FIXED** —
  audit_death_staging.rs asserts `reserved_total("mech_structural").raw() == 0` on
  every abort fail-point (new read-only SimRunner::reserved_total accessor); DST-arm
  decision ledgered in GAPS (deterministic sweep pins the staging boundary better
  than seeded sampling — reasoned, accepted).
- A7 belief-drain tidy (item 7, Minor): **FIXED/LEDGERED** — error-string sniffing
  replaced by a checked EXISTS query (daemon.rs:336-359); non-ULID ids ledgered
  (GAPS F7) against the CLAUDE.md IDs-are-ULIDs convention; no-audit-row choice
  ledgered (append-only beliefs table is the substrate); not-called-from-main
  ledgered (edge-source design-blocked).
- A8 compose bindings (item 8, Note): **PARTIALLY CLEARED, honestly tracked** —
  degrade_alerts consumer is now bound into the booted daemon (B below);
  calibration_for_scope/synthesis remains unbound and ledgered; T4.1 box correctly
  unticked. Accounting gap: see finding 7.

### B — 871c339 degrade-alerts consumer (T4.1 req 3 + house Slack rule)

- B1 wired into the booted daemon: PASS — drive scrapes per segment
  (daemon.rs:229-248) and routes via route_alerts; main builds the router from
  validated env and passes it (main.rs:163-192). Not compose-layer-only anymore.
- B2 every Slack message also writes an audit row: PASS in code (route_alerts
  audits on None-router, on send success [with slack ts], and on send FAILURE —
  daemon.rs:393-421); **the committed tests do not assert the audit row** despite
  names claiming it (finding 3).
- B3 mock transport only in tests: PASS — alert_routing.rs MockTransport;
  ReqwestTransport/ReqwestPing appear only in main.rs (grep quoted below); no
  network in any test.
- B4 once-per-breach: PASS for degrade alerts (DegradeScrape::scrape is
  saturating-delta — only NEW breaches alert); the new poll-failure alert repeats
  per segment during a sustained outage (finding 4).
- B5 env router, fail-closed: PASS — validate_env (boot.rs:107-112) REQUIRES
  FORTUNA_SLACK_BOT_TOKEN, all five FORTUNA_SLACK_CHANNEL_* ids, and
  FORTUNA_DEADMAN_URL; absence => boot refuses (fail-closed, pinned by boot tests
  and the prior gate). build_slack_router: token-present + missing channel id is a
  LOUD error, never silent None (test pinned:
  build_router_missing_channel_id_is_loud_not_silent_none). The None=Slack-disabled
  path is unreachable in main (env requires the token) — composition-API-only.

### C — dfb849f dead-man heartbeat

- C1 wired per T4.1 (FORTUNA_DEADMAN_URL, every minute): PASS — env-validated URL,
  ping_interval_secs=60 in the committed example config, independent tokio task in
  main (main.rs:118-146), due()-gated deadman_tick.
- C2 no real URL pinged in tests: PASS — deadman.rs uses MockPing +
  "https://hc.example/never-hit"; grep over test code finds only example.com /
  hc.example placeholders and local-bound observability addresses; ReqwestPing is
  referenced solely from main.rs. The operator's real monitor is not armed by any
  test.
- C3 failure never crashes the loop: PASS for the never-crashes half (committed
  test ping_failure_escalates_and_does_not_record: error handed to the closure,
  no record, next tick retries). PARTIAL for counted/alerted: main's on_failure is
  eprintln-only — no audit row, no Ops alert, no counter — while the deadman_tick
  doc claims "the daemon audits + Ops-alerts it" (finding 5). The external monitor
  paging on silence is the designed escalation of record, so the behavior is
  defensible; the doc overclaim is not.
- C4 ROTA deadman_ping_age seam: ABSENT — docs/design/rota-dashboard.md:233-234
  specifies `last_ping_at: Arc<AtomicI64>` on DeadmanPinger; HEAD has a private
  `Option<UtcTimestamp>` and main moves the pinger into the spawned closure (no
  shared handle). T4.3-owned per the design, but this wiring will need rework and
  that is not ledgered (finding 8).

### D — battery at dfb849f (all executed in the detached worktree)

- D1 cargo fmt --check: PASS — FMT_EXIT=0.
- D2 cargo clippy --workspace --all-targets -- -D warnings: PASS — CLIPPY_EXIT=0.
- D3 cargo test --workspace: PASS — 701 passed / 0 failed across 105 suites,
  TEST_EXIT=0 (matches the dfb849f commit claim "TESTS 701-0").
- D4 invariants per-test: PASS — i1 (2), i2 (2), i3 (1), i4 (1), i5 (1), i6 (3),
  i7 (3) + 3 doc/compile-fail tests, all ok individually.
- D5 scripts/run-dst.sh 10000: PASS — DST_EXIT=0; stage lines quoted below; all 11
  settlement arms hit (incl. AuditDeath: 910).
- D6 mechanical + test-weakening sweep over 817d2e7..dfb849f: no new unwrap/expect/
  panic in money paths (added unwraps are test-code or the binary's infallible
  epoch-0 fallback); no #[ignore], no proptest reductions, no deleted asserts
  (shutdown.rs change is comment+import only; set_arb_books moved behind
  #[allow(dead_code)] in common); no f64 money; BTreeMap everywhere on routed
  paths; no GatedOrder bypass (zero new place( sites); fake tokens only
  (xoxb-test/xoxb-x) — no secrets. ONE hit: main.rs:131 raw SystemTime::now() in
  the deadman task (finding 6).
- D7 protected crate: NOT touched.
- D8 T4.1 accounting: box correctly UNTICKED at HEAD (BUILD_PLAN.md:274). Open
  before tick: synthesis-in-main + mind_from_env/CostBudget binding + mech_extremes
  w/ veto (the latter two missing from GAPS' open list — finding 7), scheduled
  daily/weekly loops, belief-persist-from-main, the A2 reopen above, and the stale
  GAPS dead-man line (finding 2).

## Findings

- [Major] Standing-halt audit dedup resets every segment: `drive` re-enters
  `run_loop` per segment and `last_halt` re-initializes, so ONE standing halt
  re-applies+re-audits each ~30s segment (~2,880 audit rows/day in main) — the
  prior gate's "once per halt IDENTITY" is not met and the committed test never
  crosses a segment boundary. Reproduction: scratch probe via drive (4 segments,
  20 polls, 1 standing halt) => `halts_applied: 4`, audit-row assert FAILED
  (left: 4, right: 1). Fix: hoist the identity fingerprint to drive/daemon scope
  (or into the poller) and extend the committed test through drive across >=2
  segments.
- [Minor] GAPS.md:107 still ledgers the dead-man pinger as "deliberately unwired"
  at HEAD although dfb849f wired it; dfb849f's commit message claims "GAPS/doc:
  dead-man flipped ... to WIRED" but the commit touches no GAPS.md — the ledger
  misstates the open set and the commit message misstates the commit.
- [Minor] alert_routing tests assert Slack posts and failure counts but never the
  audit rows their names claim (alerts_route_to_slack_and_audit,
  no_router_still_audits_zero_posts; MemoryAuditSink never inspected) — the house
  every-Slack-message-audits rule is implemented but not test-asserted.
- [Minor] Poll-failure Ops alert has no cross-segment dedup: a sustained halt-store
  outage emits one Slack message + audit row per ~30s segment (~2,880/day) — the
  same flood class as the halt finding, lower amplitude.
- [Minor] deadman_tick doc overclaims ("the daemon audits + Ops-alerts it") vs
  main's eprintln-only on_failure; ping failures produce no audit row, alert, or
  counter. Third consecutive gate with a comment-vs-code overclaim in this crate.
- [Minor] main.rs:131 adds a raw SystemTime::now() in the deadman task — CLAUDE.md:
  "SystemTime::now() anywhere outside the Clock impls is a defect." Precedent
  (main.rs:57, binary edge) was previously accepted; route through the runner's
  RealClock like the cadence does, or ledger the binary-edge exemption explicitly.
- [Minor] T4.1 open-set accounting: mech_extremes-with-veto and the
  mind_from_env/AnthropicMind+CostBudget daemon binding are T4.1 contract items
  absent from both the composition (compose_runner: mech_structural only,
  veto_mind: None) and GAPS' "HONESTLY STILL OPEN" list.
- [Note] ROTA's deadman age seam (last_ping_at: Arc<AtomicI64>, design doc 233-234)
  is absent and the pinger-moved-into-closure wiring conflicts with it; unledgered
  rework ahead for T4.3.
- [Note] `_send_failures` from route_alerts is discarded in drive; the dead-man
  escalation of Slack send failures lives only in a code comment while GAPS claims
  "No remaining sliver on this residue line".

## Commands run (verbatim results, detached worktree /tmp/fortuna-g5 at dfb849f)

```
$ cargo fmt --check                                  -> FMT_EXIT=0
$ cargo clippy --workspace --all-targets -- -D warnings -> CLIPPY_EXIT=0
$ cargo test --workspace                             -> 701 passed / 0 failed (105 suites), TEST_EXIT=0
$ cargo test -p fortuna-invariants                   -> i1..i7 all ok per-test (14 + 3 doc tests)
$ scripts/run-dst.sh 10000                           -> DST_EXIT=0
  [dst] OK: 0 corpus + 10000 random seeds, zero invariant violations
  [synthesis-dst] master seed 1781191946446 -> 10000 scenario(s)
  [synthesis-dst] totals: 27567 orders, 42328 proposals, 132702 cognition failures, 117784 beliefs
  [settlement-dst] arms {SettleClean: 891, SettleThenCorrect: 937, Void: 923, Dispute: 980,
    VenueMismatch: 918, CanonicalDivergence: 868, OrphanScan: 842, AuditDeath: 910,
    WideBook: 912, Overdue: 902, MultiLegGroup: 917}; 1787 discrepancies, 2724 watchdog rows, 1829 halts
$ cargo test -p fortuna-live --test run_loop ...     -> a_standing_halt_audits_exactly_once_over_many_polls ok (+13 targeted, all ok)
$ scratch probe (worktree-only, through drive, 4 segments x 5 wakes, standing halt):
  assertion `left == right` failed: ONE standing halt across segments applies once
  (got LoopStats { ticks: 8, halt_polls: 20, poll_failures: 0, halts_applied: 4 })
$ clippy at 871c339 (second worktree)                -> CLIPPY_871_EXIT=0
$ git diff --name-only 817d2e7..dfb849f | grep -c fortuna-invariants -> 0
```
