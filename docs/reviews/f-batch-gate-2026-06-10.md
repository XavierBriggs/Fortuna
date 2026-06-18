# Review: f-batch-gate (F1/F2/F3 targeted re-gate + telemetry) — 2026-06-10
Base: 1e3e5e7 (batch incl. ledger-sync 4305964; code commits b4c839f, 44f4bb9, 8a83c62)
Head: 8a83c62  Verdict: ACCEPT-WITH-GAPS
Protected crate touched: no (git log 1e3e5e7..HEAD -- crates/fortuna-invariants/ is empty)

This is the targeted re-gate named by the GAPS.md F1 close criterion ("test asserting
a BudgetExhausted cycle writes the audit row + an ops-layer rule referencing the
signal; then a targeted re-gate of this item alone"), extended at operator request to
the latency/Section-8 telemetry additions in the same batch.

## Item verdicts

- **F1 (Major, budget/cognition degrade silent): CLOSED** — degrade kind preserved with
  scope/spent/cap, drained to kind="cognition" audit rows + bus events every tick,
  counted once, exported, alert rule tested. Evidence under C1a-C1e.
- **F2 (Minor, T2.8 missing visible correction): CLOSED** — BUILD_PLAN.md:192-196 now
  carries the correction note mirroring T2.6's.
- **F3 (Minor, wholly-discarded output traceless): CLOSED** — discard writes a
  kind="cognition" row, degrade=model_proposals_discarded with count; test-asserted.

## Criteria (fixed before reading the diff)

- C1a degrade arm preserves failure kind (spec line 238; GAPS F1 fix-spec): PASS —
  synthesis.rs:170-210: BudgetExhausted carries {scope, spent_cents, cap_cents};
  provider/schema_invalid/refused/context each typed with detail; buffered as
  DegradeRecords, never swallowed. Strategy increments only cognition_failures.
- C1b runner drains records into audit + bus, breaches counted ONCE: PASS —
  runner.rs:523-556: drain merges detail into payload {strategy, event_id, degrade,
  ...detail}, appends kind="cognition" via self.audit (I5 path), publishes
  cognition_degrade bus event. budget_breaches incremented at exactly one site
  (runner.rs:536, drain only); counters() (runner.rs:2210-2221) rebuilds from a copy
  each call — no accumulation drift, no double count between strategy metrics and
  drain. Drain iterates Vecs — deterministic order.
- C1c budget_breach test asserts the row fields: PASS — `cargo test -p fortuna-runner
  --test synthesis_loop budget_breach` => ok (1 passed). Test
  budget_breach_and_discarded_output_write_audit_rows asserts kind=="cognition",
  degrade=="budget_exhausted", scope=="per_cycle", spent_cents==50, cap_cents==50,
  strategy=="synth_sim", and counters().budget_breaches >= 1.
- C1d ops alert rule: PASS — fortuna-ops/src/alerts.rs degrade_alerts: every breach
  alerts (one MessageKind::Alert per scrape carrying the delta count), cognition
  failure bursts alert at/above threshold, quiet scrape silent. `cargo test -p
  fortuna-ops --test alerts` => ok (2 passed: every_budget_breach_alerts,
  failure_threshold_separates_blips_from_outages).
- C1e reachability + determinism (adversarial): PASS — fortuna_budget_breaches_total
  exported (runner.rs:2034-2037, help text "each one alerts");
  fortuna_cognition_failures_total exported (runner.rs:2014). Scrape-diff composition
  is the ledgered post-fixture task (GAPS "Path to production" step 5) — disclosed,
  not silent. `SETTLE_DST_SCENARIOS=300` => all 10 arms fired incl. AuditDeath x28,
  0 failed seeds (no conflict between degrade rows and the AuditDeath arm);
  `SYNTH_DST_SCENARIOS=300` => 0 failures with 4,251 cognition failures exercised and
  per-seed byte-identical replay (degrade audit rows + bus events ARE deterministic).
  Synthesis on_event matches only EventPayload::BookSnapshot (synthesis.rs:127) — the
  cognition_degrade Raw event cannot re-trigger a cycle; no degrade feedback loop.
- C2 (F2) BUILD_PLAN T2.8 visible correction: PASS — BUILD_PLAN.md:192-196
  "(E1 CORRECTION 2026-06-10, completing the F2 gate finding: like T2.6 ...)".
- C3 (F3) discarded-output audit trace: PASS — synthesis.rs:219-226 pushes
  model_proposals_discarded with count; test second half asserts the row
  (degrade=="model_proposals_discarded", count==1) on a wholly-discarded MindOutput.
- C4 LatencyStat correctness: PASS — runner.rs:168-223: fixed const bounds
  (14: 1ms..60s) + overflow, derive Copy. quantile_ms: count==0 / non-finite /
  q outside [0,1] => None; target=ceil(q*count).max(1); walk returns UPPER edge of the
  first bucket reaching target — the nearest-rank order statistic is <= that edge, so
  the estimate NEVER understates; overflow bucket reports observed max_ms (exact upper
  bound). q=0.0 => upper edge of the min's bucket; q=1.0 => target==count. No
  off-by-one found in the cumulative walk (trailing fallback is unreachable-but-safe).
  observe() clamps negative skew (ms.max(0)), saturating sum.
- C5 latency/section_8 tests + export completeness: PASS — `cargo test -p
  fortuna-runner --test sim_loop` => ok (9 passed).
  fill_latency_is_measured_from_submit_to_execution asserts: delayed fills measure
  >=500ms, conservative estimates (est >= 500 for q in {.5,.9,.95,.99}), empty =>
  None, +Inf bucket == count, buckets cumulative monotone, p90/95/99 gauges exported,
  same-seed determinism. section_8_metric_surface_is_present asserts all 12 required
  names incl. fortuna_budget_breaches_total, by-check labels on a provoked rejection,
  strategy label on envelope bps. Code-verified: buckets cumulative with le labels and
  +Inf == count (overflow only in +Inf — correct Prometheus convention);
  gate_rejections_by_check (runner.rs:740-744 -> 2140-2148); venue_api_errors at 3
  outage sites (1031, 1341, 1696); settlement voids/reversals; envelope utilization
  bps guarded envelope>0 (runner.rs:2125-2137); cognition cost total (1879).
  Fill latency on APPLIED fills only, fill.at minus submit time, submit_times
  (BTreeMap) pruned at terminal states; ack latency only on Acked; Unknown
  submissions store submit time without faking an ack.
- C6 determinism / renderer / secrets: PASS — metrics_export is pull-only with zero
  call sites in the tick path; the Recording serializes bus events only
  (runner.rs:1245) so the BTreeMap-keyed exports cannot enter replay comparison;
  same_seed_same_script_byte_identical_recording ok. All new keyed state is BTreeMap
  (submit_times:146, gate_rejections_by_check:148) — no HashMap iteration feeding
  bus/audit. Ops renderer (fortuna-ops/src/metrics.rs:29-44) sorts labels and escapes
  backslash/quote/newline; le values are numeric strings/"+Inf", check values are
  Debug enum names — no clash. Degrade rows carry no secrets: from_env errors name the
  env VAR only; reqwest error strings never include headers (api key rides in a
  header); breach detail is integers + static scope.
- C7 regression: PASS — `cargo fmt --all -- --check` => clean; `cargo clippy
  --workspace --all-targets -- -D warnings` => exit 0; `cargo test --workspace` =>
  635 passed / 0 failed (on re-run; first run hit the pre-existing settlement_dst
  coverage flake — finding 1); scripts/run-dst.sh full battery => all stages ok
  (settlement stage at 2000 scenarios, all 10 arms fired); invariants crate untouched.

## Findings

- [Minor, PRE-EXISTING at base, newly discovered, unledgered] settlement_dst aggregate
  coverage assertions flake at the default 20 scenarios (~7%: 4/60 hammer runs plus
  the first workspace run this session). Reproduction (deterministic):
  `DST_MASTER_SEED=1781139292562 cargo test -p fortuna-runner --test settlement_dst`
  => panic settlement_dst.rs:533 "battery never halted (distribution bug)"; also
  1781139293628, 1781139298297 (line 533) and 1781139297249 (line 525, "never
  produced a discrepancy"). Verified PRE-EXISTING: identical failure with the same
  seed at base 1e3e5e7 (scratch worktree) — NOT caused by this batch, and the F-batch
  provably did not perturb the seed's behavior (identical panic site). Fail direction
  is conservative (false RED, never false green), but it makes the mandatory
  regression command intermittently red on healthy code. Fix shape: gate the
  aggregate nonzero asserts on scenario count the way the per-arm asserts already are
  (settlement_dst.rs:534 `if scenarios >= 200`), or raise the default. MUST be
  ledgered in GAPS.md; red seeds above belong in the corpus discussion.
- [Minor, NEW in this batch] False doc claim "per-decision cost rides in the cognition
  audit rows" (commit 8a83c62 message, ASSUMPTIONS.md telemetry section, RunCounters
  doc runner.rs:255-257). kind="cognition" rows carry spend only on budget-breach
  degrades (scope cumulative, not per-decision). The actual per-decision carrier is
  belief provenance (mind.rs:565-569, spec line 181) on the live-mind path. Same
  falsehood class the T2.6/T2.8 corrections exist for — correct the three doc sites
  or move the cost into a real per-decision audit row. Ledger in GAPS.md.
- [Minor, NEW in this batch] fortuna_cognition_cost_cents_total undercounts true
  spend: failed model calls record spend against the budget (mind.rs:597-601,
  record_spend on success AND failure) but StrategyMetrics.cognition_cost_cents
  accrues only on Ok (synthesis.rs:218). A schema-failing or refused mind consumes
  budget invisibly to the metric until the breach row reveals true spent_cents.
  Conservative direction preserved (the budget binds on true spend; every failure now
  audits via F1), but the ops cost board will read low. Ledger + either accumulate
  failed-call cost into the metric or document the semantics where the metric is
  defined.

Observations (no action required for this gate): percentile gauges export 0 when
empty (unwrap_or(0)) — indistinguishable from a true 0ms; skipping the sample would
be cleaner. DegradeThresholds.failure_alert_threshold == 0 silently DISABLES burst
alerts (alerts.rs:46) rather than meaning "always" — document at the composition.
Bucket series are exported as counter-typed *_bucket rather than a true Prometheus
histogram TYPE; histogram_quantile still works on the le labels. Fill latency for
submits predating a restart is unmeasured (in-memory submit_times) — inherent to the
design, disclosed here.

## Commands run (verbatim verdict lines)

- `cargo fmt --all -- --check` => clean (FMT_OK)
- `cargo clippy --workspace --all-targets -- -D warnings` => Finished, exit 0
- `cargo test -p fortuna-runner --test synthesis_loop budget_breach` =>
  "test result: ok. 1 passed; 0 failed" (8 filtered)
- `cargo test -p fortuna-ops --test alerts` => "test result: ok. 2 passed; 0 failed"
- `cargo test -p fortuna-runner --test sim_loop` => "test result: ok. 9 passed; 0 failed"
- `SETTLE_DST_SCENARIOS=300 cargo test -p fortuna-runner --test settlement_dst --
  --nocapture` => "ok"; arms {SettleClean:38, SettleThenCorrect:26, Void:25,
  Dispute:20, VenueMismatch:32, CanonicalDivergence:34, OrphanScan:34, AuditDeath:28,
  WideBook:31, Overdue:32}; 66 discrepancies, 86 watchdog rows, 60 halts
- `SYNTH_DST_SCENARIOS=300 cargo test -p fortuna-runner --test synthesis_dst --
  --nocapture` => "ok"; totals: 966 orders, 1451 proposals, 4251 cognition failures,
  3855 beliefs
- `cargo test --workspace` => first run: settlement_dst FAILED (default-20 coverage
  flake, finding 1); re-run: 635 passed / 0 failed across 64 test binaries
- 60x hammer of settlement_dst at default 20 => 4 failures, seeds 1781139292562,
  1781139293628, 1781139297249, 1781139298297; pinned-seed re-run reproduces;
  same seed at base 1e3e5e7 (scratch worktree, removed after) reproduces identically
- `scripts/run-dst.sh` => all stages ok; settlement stage "master seed 1781139852641
  -> 2000 scenario(s)", all 10 arms fired, 396 discrepancies, 594 watchdog rows,
  390 halts, "1 passed; 0 failed"
- `git log 1e3e5e7..HEAD -- crates/fortuna-invariants/` => empty
- Diff sweeps: no removed asserts/#[ignore]/tolerance changes (the only test diff
  outside new tests ADDS an assertion, settlement_loop.rs:369-370); no
  unwrap/expect/panic/SystemTime::now/Instant::now/Utc::now added in src; no
  KEY/TOKEN/SECRET literals; no new GatedOrder construction or place( call sites.

## Verdict

F1 CLOSED, F2 CLOSED, F3 CLOSED on their ledgered close criteria with executed
evidence. Telemetry additions are correct where it counts (conservative quantiles,
deterministic replay untouched, no money-path contact). ACCEPT-WITH-GAPS: the three
Minors above must be ledgered in GAPS.md by the implementer (the flake fix and the
two doc/metric accuracy items); none blocks F1 closure. GAPS.md may now flip F1 from
OPEN to CLOSED citing this file.
