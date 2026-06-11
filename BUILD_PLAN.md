# BUILD_PLAN.md — Phased Task List

Rules: tasks in order within a phase; a phase closes only when its exit criteria are
demonstrated (paste evidence in the completion note). Each task cites its spec section.
Tick boxes with a one-line completion note and the commit hash.

## Phase 0 — Verification spine and deterministic core (spec 5.1, Section 12)

- [x] T0.1 `fortuna-core`: `Clock` trait (real + sim), ULID ids, `Cents` newtype with
      checked arithmetic, error types. (5.1, conventions)
      DONE c1ad334: 56 tests (incl. 6 proptests) green; fmt/clippy clean; fixed-ms
      ISO8601 timestamps + SplitMix64 IdGen for byte-stable replay; 4 ASSUMPTIONS entries.
- [x] T0.2 `fortuna-core`: single-threaded deterministic event bus (`BusEvent`),
      replay recorder/player; same seed => byte-identical event stream test. (5.1)
      DONE a6cad5c: 23 bus tests (same-seed byte-identical stream; replay regen +
      divergence/tamper/truncation); replay-verify bin + replay.sh live; 78 tests total green.
- [x] T0.3 `fortuna-venues`: `Venue` trait, `FeeModel` (config-driven schedule
      interpreter: quadratic/flat/tiered + effective_date versioning), sim venue with
      seeded fault injection (delay/drop/dup/crash hooks, configurable book). (5.2)
      DONE a4c9071: 51 new tests green (19 fees incl. Polymarket-US rebate/banker's
      vectors, 26 sim venue incl. all fault arms + determinism, 6 vocab); fee facts
      grounded in docs/research/venue/ (Kalshi 0.07/0.0175 confirmed vs official PDF);
      spec 5.2 fee drift recorded in GAPS; sealed GatedOrder shell shipped (zero ctors).
- [x] T0.4 DST harness + `scripts/run-dst.sh` + CI wiring: randomized scenario runner,
      seed minimizer note, regression corpus directory. (5.1, PROMPT doctrine)
      DONE 3cdceaa: 2000 seeds/2.6s zero violations; 5 invariants incl. run-twice
      byte-determinism; teeth proven via planted fee bug (caught) + 2 real harness
      bugs found+fixed on first runs; replay.sh --seed works; corpus README + minimizer note.
- [x] T0.5 `fortuna-gates`: checks 1-10, TOML config, `GatedOrder` sealed constructor,
      audit record per verdict; property + boundary tests; first invariant tests
      implemented (I1, I3 subset). (5.3)
      DONE f463de2: 36 boundary tests (pass/reject/exact-cap per check) + 3 property
      suites + I1 (runtime + 2 compile-fail doctests) + I3 (rate halt survives time,
      re-arm only; dup coids exactly-once) green; fail-closed on unknown strategy/venue/
      reference; hot-reload preserves halts.
- [x] T0.6 `fortuna-exec`: intent journal state machine, deterministic client ids,
      IntentGroup completion policy, execution policy (TTL, re-quote,
      one-working-order rule), flatten planner; boot reconciliation; DST crash
      scenarios. (5.4)
      DONE ee96f14: 31 tests green (incl. crash-resubmit-via-AlreadyExists, late fills
      after cancel/boot-close, boot adopt/orphan/close paths); DST world now drives
      gates->manager->venue with crash+boot actions + I-journal invariants; 4000
      seeds x 5 masters zero violations.
- [x] T0.7 `fortuna-state`: positions, account views (settled/committed/floating/total),
      conservative marking, reservation ledger rebuilt-at-boot; drawdown halt flags
      (I2). (5.14, 5.13)
      DONE 11e8313: 64 state tests + I2 invariant (lifecycle + randomized equity-path
      property) green; hostile review caught + fixed the net-YES pair-value bug across
      venue/DST/state (per-side lots; pair settles at \$1); conservation proptested.
- [x] T0.8 `fortuna-ledger`: Postgres schema + migrations for ALL Section 7 tables;
      append-only audit writer (write failure => halt); sqlx setup. (5.5, 5.13, 7, I5)
      DONE a1e4449: 22 tables w/ DB-level append-only triggers + beliefs content guard
      (verified rejecting UPDATE/DELETE); Pg crash-recovery round trip proven; I5
      implemented (mutation refused, replay byte-identical, dead-store halt contract);
      halt_events make I2 survive restarts; .sqlx offline cache + CI postgres service.
- [x] T0.9 `fortuna-ops`: config loader, Slack client + channel routing, kill-switch
      STANDALONE binary (no Postgres, own credentials, freeze-and-cancel), CLI
      (halt/re-arm/kill/status), dead-man pinger. (8, I4)
      DONE d5e2220: killswitch in its OWN crate (I4 structural; dep-graph asserted by
      invariant + binary self-test with no DB + monthly drill script); ops 44 tests
      (config/secrets-redaction/slack-per-research/deadman); CLI halt/rearm/kill/status
      smoke-tested live (operator-attributed, durable halt_events); I4 implemented.
- [x] T0.10 `mech_structural` strategy plugin + Strategy trait + per-strategy
      comparator/sizing-lib split; runs in Sim. (6, 5.14)
      DONE 77c4c65: Strategy trait + I7 stage guard; sizing lib (Kelly boundary-tested);
      mech_structural bracket arb captured end-to-end NET OF FEES through settlement;
      7 composed-loop tests incl. byte-identical same-seed recordings, halt-mid-group,
      audit-failure-halts.

EXIT: mech_structural full loop in Sim with replay; DST corpus (all Phase-0-relevant
doctrine scenarios) zero violations across >= 10,000 seeds; CI green; I1-I4 invariant
tests implemented and green.
EXIT MET (2026-06-10, commit 77c4c65): full loop + replay = fortuna-runner tests
(arb end-to-end; same seed => byte-identical recordings). DST: "[dst] OK: 0 corpus +
10000 random seeds, zero invariant violations" (6m13s, gates->manager->venue world
with crash/boot actions). Local CI battery: cargo fmt --check clean, clippy
--workspace -D warnings clean, 341 workspace tests 0 failures (CI workflow incl.
postgres service committed). Invariants: I1, I2, I3, I4 implemented and green
(plus I5 ahead of schedule); I6/I7 staged for their owning Phase 2/3 tasks.

## Phase 1 — Mechanical paper path (Section 11, 12)

- [x] T1.1 Kalshi adapter against `fixtures/kalshi/` (auth/signing, markets, book,
      place/cancel, fills cursor, settlement notices); fee reconciliation per fill. (5.2)
      DONE 4310081 (buildable portion; delegated agent, independently verified): RSA-PSS
      signing + V2 DTOs + transport + adapter w/ reconcile-after-cancel + per-fill fee
      reconciliation; 76 new tests vs doc-derived samples; SIM-ONLY until operator
      fixtures confirm the 27-item checklist (GAPS operator-blocked entry).
- [x] T1.2 Paper engine: trade-through maker fills with haircut, taker crosses visible
      depth; paper/live parity at the Strategy interface. (11)
      DONE 0eb87f7: strict trade-through maker fills (touch NEVER fills — doctrine
      test), floor-rounded shared-FIFO haircut budget, taker crosses displayed depth
      only, sim/paper parity test at the gated-order boundary; 10 tests; recorded-
      stream runs blocked on fixture capture (GAPS).
- [x] T1.3 `mech_extremes` + model-veto scaffolding (veto reduce-only, counterfactual
      scoring records; stub mind acceptable this phase). (6)
      DONE 99752eb + 88c9b91: reduce-only veto BY TYPE (KeepBps 1..=9999, no grow
      variant; serde through checked ctor) + StubVetoMind (deterministic); veto sits
      after sizing/before gates, every consult audited, provider error = fail-closed
      flagged UNSCORED, counterfactuals scored exactly-once at settlement (maker-fee
      net, filled-at-limit recorded); mech_extremes maker-only fade w/ provable
      sub-$100k bound (contracts x $1), fail-closed catalog guards; 39 new tests;
      DST 2000 clean.
- [x] T1.4 Settlement lifecycle processors + watchdogs (overdue, dispute, divergence,
      stranded-state) + discrepancy records. (5.13)
      DONE a0aa419: Venue::settlements_since notice stream (sim/paper/kalshi);
      SettlementLedger superseding-insert chains + exact position reversal w/ veto
      re-scoring; runner processor (dedup/correction/void) + watchdogs (overdue,
      dispute freeze w/ new Disputed status, 3-tick position-mismatch -> discrepancy +
      GLOBAL halt); Settlements/Discrepancies Pg repos; DST void+reversal arms; 25 new
      tests, DST 3000 clean. Divergence detector deferred to T2.1 (needs events/edges,
      GAPS).
- [x] T1.5 Metrics (OpenTelemetry), minimal read-only dashboard, daily digest,
      accounting export. (8)
      DONE bee37a6: deterministic MetricsRegistry -> Prometheus text exposition 0.0.4
      (research: OTel Rust exporters Beta/RC, OTLP = documented upgrade path); runner
      MetricSample export w/ strategy attribution; read-only axum dashboard (GET-only
      by construction); pure digest composer; write-once accounting CSVs; full
      sim->metrics->dashboard->digest chain test; 11 new tests.

EXIT: both mechanical strategies in Paper against recorded data streams; settlement and
discrepancy paths exercised in DST; dashboards and digest render from sim data.
EXIT MET TO THE BUILDABLE EXTENT (2026-06-10, commit bee37a6):
- Settlement and discrepancy paths in DST: settle/void/REVERSAL action arms drive the
  sim venue with I-money extended through refunds and claw/repay; 3000-seed run clean
  at T1.4 (a0aa419); composed-loop tests cover processor dedup/correction/void,
  overdue/dispute/mismatch watchdogs, discrepancy + global halt.
- Dashboards and digest from sim data: tests/observability.rs runs the mech_structural
  arb end-to-end and renders the SAME run through registry -> /metrics scrape ->
  /api/boards -> digest text ("dashboards_and_digest_render_from_sim_data").
- Both mechanical strategies: mech_structural + mech_extremes run the full composed
  Sim loop (mech_extremes WITH its veto); paper engine proven at the gated-order
  parity boundary. "Against recorded data streams" is OPERATOR-BLOCKED on the Kalshi
  fixture capture (websocket book/trade streams) — GAPS has the exact unblock steps;
  first post-fixture task is the venue-generic runner composition replaying recordings
  into PaperVenue.
- Battery at exit: 504 workspace tests 0 failures; fmt + clippy -D warnings clean.

## Phase 2 — Belief pipeline (Section 12)

- [x] T2.1 Events + edges + snapshot scheduler (benchmark_at; T-24h/1h/5m + on-trade);
      CLV scoring job with liquidity filter. (5.12, 5.5)
      DONE 1a4f635: lifecycle legal-or-error (5.13 model); edge tiers gate structurally
      (confirmed-only for multi-leg); deterministic source/horizon checks (mismatch=0,
      UMA-mode); once-per-window scheduler excluding post-benchmark; integer-exact CLV
      with None-over-fake-CLV liquidity filter; Events/Edges/Snapshots Pg repos.
      Belief-row job wiring -> T2.3; on-trade hook -> T2.6.
- [x] T2.2 Signals: `Source` trait, normalizer/envelope/dedup, source registry with
      trust tiers, trigger engine (rules + debounce + per-event serialization). (5.11, 5.8)
      DONE 674f7f0: drain-on-poll Source trait; canonical-JSON SHA-256 dedup per
      (source, hash) w/ boot rebuild; fail-closed registry allowlist (tier 0..=10);
      trigger engine w/ one-cycle-per-event serialization + debounce + REPORTED
      coalesced counts; Signals/SourceRegistry Pg repos; 11 new tests.
- [x] T2.3 Belief ledger ops + freshness policy + scoring (Brier, calibration curves
      per model/strategy/category). (5.5, 5.10)
      DONE 8921071: strict (0,1) probability validation (certainty = schema-invalid);
      transactional supersession; score-once repo enforcement (abandoned never scores,
      superseded does); abandonment excluded from calibration; freshness w/ tightened
      pre-benchmark window + signal-postdates rule; resolved-only calibration buckets.
- [x] T2.4 Context assembler with manifests + replayability (snapshotted computed
      views); anonymization mode. (5.7)
      DONE 69a37a2: priority-order char-budgeted packing (greedy, whole items only);
      manifest hash for provenance (byte-identical on same inputs); strict
      point-in-time + counted exclusions; fail-closed item hash verification;
      anonymization w/ stable pseudonyms (manifest keeps real ids); <context-item>
      data delimiting.
- [x] T2.5 `Mind` trait: StubMind (deterministic, for DST) + AnthropicMind (structured
      output via tool schema, cost tracking, budgets, schema-invalid handling). (5.9)
      DONE 684964f: propose-only MindOutput (I6); raw-HTTP transport trait (no Rust
      SDK; documented wire shape, adaptive thinking, json_schema structured output);
      schema-invalid rejected never repaired; harness-stamped provenance; pre-call
      budget checks w/ 00:00 UTC day roll; cost from usage x configured prices; env-key
      gate = the live feature flag (operator-blocked). 6 new tests.
- [x] T2.6 Decision cycle + comparator + Kelly sizing with calibration haircut;
      (E1 CORRECTION 2026-06-10: the original DONE note implied the haircut-Kelly
      sizing was composed into the runner — it was a LIBRARY ONLY with zero call
      sites until the post-verification E1 fix wired calibrate->comparator->
      min(kelly, affordable) end to end. The verification gate caught it.)
      triage tier + triage scoring (declined-trigger shadow sampling). (5.8, 5.9, 5.14)
      DONE ea65ff2: two-sided comparator (Direct+Negation only; floor fair
      values; stale/tier/floor exclusions); fail-closed haircut; first-K-per-day
      deterministic shadow sampling (scored, never traded); DecisionCycle w/ manifest
      provenance + cost. Sim/DST composition -> Phase 2 exit.
- [x] T2.7 Daily reconciliation loop (journal, exits evaluation), aeolus_eval ingestion
      against the sample fixture. (5.8, 6)
      DONE 38766b4: journal-or-error reconciliation w/ structurally-zero orders
      (mind proposals counted + discarded); strict AeolusEnvelope contract -> zero-
      capital BeliefDrafts (aeolus: namespace, p_raw preserved); JournalRepo
      one-per-day. Real-fixture validation operator-blocked (prod read denied by
      classifier; exact unblock in GAPS).
- [x] T2.8 Calibration layer (Platt/isotonic, shrinkage prior, versioned params). (5.10)
      (E1 CORRECTION 2026-06-10, completing the F2 gate finding: like T2.6, this
      DONE note described a LIBRARY — calibrate()/quality had zero call sites in
      the composed loop until the post-verification E1 fix wired the layer into
      the decision cycle and the sizing haircut. The independent e-gate caught
      that this note never received the visible correction T2.6 got.)
      DONE 70c5df6: deterministic Platt (Newton, refuses degenerate records) +
      isotonic PAV (step apply); shrinkage w=min(n/50,1) ignores fit+extremization
      below threshold; quality = n-ramp x reliability-gap accuracy -> T2.6 haircut;
      versioned CalibrationParamsRepo (INSERT-only, latest per scope).

EXIT: full decision loop in Sim with StubMind under DST (including cognition failure
scenarios); AnthropicMind exercised behind a feature flag with budget controls;
aeolus_eval writes scored beliefs from fixture envelopes; I5-I7 invariant tests green.
EXIT MET TO THE BUILDABLE EXTENT (2026-06-10):
- Full decision loop in Sim under DST: SynthesisStrategy adapts DecisionCycle to the
  Strategy trait (same sizing/gates/audit path, I1); tests/synthesis_loop.rs proves
  belief -> comparator -> proposal -> fill end to end, cognition-failure degrade
  (4 error kinds, zero proposals, loop continues), and shadow-run no-trade;
  tests/synthesis_dst.rs is stage 2 of scripts/run-dst.sh (chaos mind: 813 cognition
  failures across 60 seeds, zero violations, byte-identical replay per seed).
- aeolus_eval scored beliefs: fixtures/aeolus/sample_envelope.json (FORTUNA-defined
  contract sample) -> strict parse -> zero-capital drafts -> BeliefsRepo -> scored ->
  calibration record (ledger test). Operator-recorded real export still open in GAPS.
- I5 (T0.8) green; I6 implemented (schema-rejects smuggled sizing, field-set pins,
  dependency-direction); I7 implemented for existing rails (stage-violation refusal,
  ordered ladder) with T3.1/T3.3 clauses staged as owned stubs (GAPS).
- AnthropicMind live exercise OPERATOR-BLOCKED on ANTHROPIC_API_KEY (the env-key gate
  IS the feature flag; budget controls built + mock-tested at T2.5). GAPS has the
  recommended first exercise (one haiku smoke call under a tight CostBudget).

## Phase 3 — Closing the loop (Section 12)

- [x] T3.1 Weekly/monthly review jobs (calibration report, lesson promotion flow,
      envelope allocation report). (5.8)
      DONE d205787: deterministic-core weekly review (versioned refits at n>=50,
      Section 11 GO/NO-GO recommendations w/ reasons, I7-clean) + strict-JSON lesson
      candidates from the journal body; monthly review (conservative allocation
      recommendations, cost audit, lesson decay, operator checklist); LessonsRepo
      superseding-insert chains + journal range + resolved_stats queries.
- [x] T3.2 Market-back discovery loop + edge confirmation cards; world-forward
      watchlist loop with cost cap + unscoreable rule. (5.12)
      DONE 753a288: deterministic prefilter w/ counted exclusions; tradability
      persisted append-only; strict-JSON normalization (match-before-create,
      hallucinations dropped); cards carry model + deterministic scores w/
      high-stakes flag; wake_events trigger; world-forward unscoreable rule
      (registry-checked, beliefs refused) + DiscoveryBudget throttle-before-spend;
      calibration queries exclude unscoreable.
- [x] T3.3 Shadow-mode model comparison harness (paired contexts, separate budget). (11)
      DONE 6276274: manifest-hash pairing (provable identical contexts); structurally
      zero-order ShadowRun; own budget checked before sampling; first-K/day sampling;
      evaluate_model_swap I7 gate (>=30 paired per active category, Brier+CLV no
      worse, dedup on pairing key, recommendation-only). Closed BOTH staged I7
      invariant stubs (+ runner::promotion::effective_stage) — zero ignored tests
      in fortuna-invariants.
- [x] T3.4 Polymarket US adapter (fixtures-gated; stub + GAPS entry if fixtures absent).
      DONE 31926b9: fixtures absent -> PolymarketUsStub behind the Venue trait;
      every operation + fee model refuses (VenueError::FixtureGated); GAPS entry
      sequences research loop -> operator fixtures -> recordings-only adapter.
- [x] T3.5 `synth_events` strategy in Paper. (6)
      DONE 0ab04d9: named SynthesisStrategy config (confirmed edges only, Paper
      stage cap; effective stage via I7 promotion records, starts Sim); paper
      parity boundary proven strategy-in-front (maker quote within limit,
      touch-no-fill, trade-through fill w/ haircut, settlement). Recorded-stream
      replay operator-blocked (GAPS).
- [x] T3.6 FINAL_REPORT.md + operator go-live runbook + full acceptance checklist run.
      DONE (this commit): closed the deferred 5.13 divergence detector (venue-vs-
      canonical -> settlement_divergence discrepancy w/ edge hit, on fresh AND
      corrected settlements; PnL stays venue-truth) and the orphan watchdog
      (coverage-fed, alert-once); GAPS reorganized to operator-blocked-only;
      acceptance battery: fmt --check + clippy -D warnings clean, 615 tests 0
      failures, 10,000 core DST seeds + 10,000 chaos scenarios (131,071 injected
      cognition failures) zero violations; FINAL_REPORT.md w/ deviations, stats,
      checklist disposition, and the operator go-live runbook.

EXIT: PROMPT.md acceptance checklist fully green or operator-blocked-only.
EXIT MET (2026-06-10): every checklist item DONE or OPERATOR-BLOCKED with exact
unblock steps — see FINAL_REPORT.md Section 4 for the per-item disposition and
GAPS.md for the blocked set (Kalshi fixtures, credentials, Aeolus export,
Polymarket research+fixtures, spec v0.9 touch-up).

## Phase 4 — Live composition (post-acceptance; operator-directed)

- [ ] T4.1 The composition daemon (`fortuna-live` binary in fortuna-runner or its own
      crate): config load (fortuna.toml + env secrets, fail-closed on missing),
      Postgres-backed repos + AuditWriter (no audit, no trading), SimRunner-based
      tick loop on a real-time cadence (RealClock at the edges, SimClock semantics
      preserved for replay), mind_from_env (AnthropicMind w/ CostBudget from
      [cognition] config), strategies from config (mech_structural, mech_extremes
      w/ veto, synth_events w/ CalibrationParamsRepo.latest + resolved_stats ->
      CalibrationContext + set_calibration_quality — closes the residue binding),
      scheduled loops (decision cadence; daily reconciliation 00:00 UTC; weekly/
      monthly reviews -> digest channel), Slack routing (every message also an
      audit row) + degrade_alerts scrape consumer (closes the other residue
      binding), dead-man pinger (FORTUNA_DEADMAN_URL, every minute), metrics HTTP
      endpoint (GET-only, Tailscale-bound per spec), halt-state poll <= 500ms w/
      poll-failure alert, graceful shutdown (cancel working orders, final audit
      row). Sim venue ONLY until fixtures clear (venue selection is config;
      kalshi refuses without fixture clearance). DST: a daemon-composition
      smoke (boot -> N ticks -> clean shutdown, deterministic under SimClock).
      Verifier-gated like every batch.
- [ ] T4.2 POST-FIXTURE tranche (blocked on the operator recording session):
      Kalshi WS dial (signed handshake, keep-alive, redial w/ resubscribe-on-gap),
      venue-generic recorded-stream replay into PaperVenue under both mech
      strategies, kalshi adapter paper/live clearance vs fixtures, kill-switch
      KalshiVenue plug (FORTUNA_KILLSWITCH_* creds), Slack Socket Mode listener
      (app token; kill REQUESTS only, re-arms stay CLI-only).

EXIT: the daemon runs for a continuous week in Sim with the real mind — beliefs
calibrated/persisted/scored in Postgres, cost tracked against config budgets,
digests and alerts landing in Slack, dead-man green — with zero unexplained
halts and a clean operator-runnable shutdown/restart (boot reconciliation).

## Phase 5 — Kinetics perps module (operator-directed 2026-06-10; spec-governed extension)

Constraint quoted from the directive: "new capability, zero changes to the invariant
middle." Every perps order passes the same I1 gate pipeline; the model stays
propose-only (I6); promotion gates (I7) apply to any perps strategy unchanged. Margin
changes the loss model (no longer premium-bounded) — gate design must precede adapter
code.

- [x] T5.0 Phase A research (read-only, no code): ground Kinetics perps in fetched
      truth — fee schedule, contract specs (sizes/tick/leverage per asset), funding
      (interval/cap/index), margin + maintenance formulas, liquidation mechanics,
      API auth + rate limits, WS channels, FIX existence, DEMO availability —
      docs/research/venue/kinetics-perps-2026-06-10/research.md with sources,
      retrieval dates, confidence per claim; live-account-only items to GAPS.md.
      (DONE 2026-06-10/11: 844-line research.md, ~50 sources, 110 raw archives
      incl. all three official API specs verbatim + live prod/demo captures;
      structure + headline claims verified against the manifest. Load-bearing:
      DEMO CARRIES PERPS with mock funds — the perps fixture session rides the
      same demo-key unblock as the event-API session. Phase B proposal:
      docs/design/kinetics-perps-module-plan.md, operator confirmation pending.)
- [ ] T5.B Phase B — design then implement. NOT ENUMERATED: the operator's directive
      was truncated mid-list ("Phase B — Design then implement, in order:"). A
      proposed order goes to the operator with the research summary; Phase B does
      not build until the order is confirmed. Expected shape (PROPOSAL ONLY):
      spec amendment (margin-aware loss bounds, funding cash flows, price-tick
      domain) -> core types (signed positions, funding accrual, margin account
      view) -> gate extensions (maintenance-margin headroom gate, liquidation-
      distance gate, funding-cost-in-edge) -> kinetics venue adapter behind the
      Venue trait (fixtures-gated like kalshi) -> paper engine margin semantics ->
      strategy (funding/basis) behind I7 promotion -> ops (kill-switch coverage,
      margin telemetry).
