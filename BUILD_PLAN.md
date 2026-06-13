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

- [x] T4.1 The composition daemon (`fortuna-live` binary in fortuna-runner or its own
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
      SHUTDOWN CONTRACT (CLI critique 2026-06-11, BINDING): install a SIGTERM
      handler (tokio signal SignalKind::terminate, NOT ctrl_c-only) treated
      IDENTICALLY to graceful shutdown; the smoke/DST must assert SIGTERM ->
      cancel working orders + final audit row. `fortuna stop` (T4.4) depends
      on this contract existing and being test-asserted.
      Verifier-gated like every batch.
      PROGRESS 2026-06-11 (box stays UNTICKED — one design-blocked item
      remains): fortuna-live BOOTS AND RUNS, gate-clean, 702 tests / DST
      all stages green, real-exit-code battery. WIRED + tested: fail-
      closed boot (config+env), PgIntentJournal + PgAuditSink (I5 fail-
      synchronous, audit-death-mid-staging aborts the group), journal-
      generic SimRunner, clock-injected run loop (halt poll <=500ms with
      identity-dedup + poll-failure alert), SIGTERM/SIGINT graceful
      shutdown (cancels working orders + final audit row, smoke-asserted
      via the stop channel incl. a working-orders-at-stop variant),
      GET-only metrics endpoint, degrade-alert scrape consumer routed to
      Slack (env-built router) + always audited, dead-man heartbeat
      (independent task, mock-tested seam), daily-boundary scheduler ->
      #fortuna-digest, belief-drain + persist path, kalshi-refuses-until-
      fixtures, [sim] world from config. Survived the daemon gate's BLOCK
      (all 7 findings fixed/ledgered). REMAINING (the one blocker to the
      tick): the SYNTHESIS strategy composed into the daemon main — feeds
      calibration_for_scope + persist_beliefs + the rich digest + weekly/
      monthly reviews. BLOCKED on a deliberate design call (where the
      daemon's synthesis EDGES come from: EdgesRepo load vs config vs the
      discovery loop) — recorded in GAPS, not guessed. EXIT (continuous
      Sim week with the real mind) also awaits that wiring.
      PROGRESS 2026-06-12 (box STILL UNTICKED — tick awaits S5 mind + S6 +
      soak start): the synthesis edge-source design call is RESOLVED
      (docs/design/synthesis-edge-source-decision.md: EdgesRepo confirmed-tier,
      req 1-5). SYNTHESIS-IN-MAIN is now COMPLETE — S3b composes the opt-in arm;
      S4 (req 2) re-loads the confirmed set per drive() segment (SimRunner::
      refresh_synthesis_edges; fail-closed: empty=valid, reload-failure keeps
      last-known + alerts once, never crashes) with 3 non-vacuous tests (runner
      swap; integration count 0->1 on a mid-run confirmation; the dedup latch).
      The arm is still INERT (StubMind, calibration None). mech_extremes+veto
      DONE — compose_runner composes the opt-in [mech_extremes] favorite-longshot
      fade (spec Section 6) enrolled in the reduce-only model veto with a
      StubVetoMind::allow_all placeholder (real veto mind = S5); composition
      wiring only (strategy + veto machinery already exist + tested), inert in
      pure-sim (no volume/close metadata) until real markets (T4.2).
      PROGRESS 2026-06-12 (box STILL UNTICKED): S5a mind binding DONE —
      compose_runner takes a synthesis mind PARAM (main passes an inert
      StubMind; tests inject scripted minds) and binds the "synth_events"-scoped
      ledger calibration; a daemon_smoke live test proves the arm prices a
      seeded confirmed edge + a believing mind -> proposes + sizes + submits an
      order. Surfaced two config gaps (BINDING for the live arm): the synth arm
      needs a `synthesis_cents` envelope + `[gates.per_strategy.synthesis]` (the
      example has neither). S5b mind_from_env DONE (synthesis side): daemon::
      mind_from_env builds the Claude AnthropicMind from env when keyed (model
      from [cognition].model, transport injected — scripted in tests, never a
      real key) else StubMind; main wires it. The VETO side stays blocked
      (AnthropicVetoMind unbuilt, fortuna-cognition). S6a belief drain+persist
      WIRED into drive() DONE — per segment the synth arm's drafts are drained +
      persisted (calibration substrate; persist failure alerts, never crashes);
      mutation-proven non-vacuous daemon_smoke test. S6b RICH digest DONE —
      a fortuna-runner digest_snapshot() composes per-strategy PnL/fees/fills/
      exposure + the honesty numbers + veto; fortuna-live rich_daily_digest
      renders it via fortuna_ops::compose_daily_digest, replacing terse in drive's
      daily block. DEMO-CONFIG PREP 2026-06-12: closed the S5a config gaps in
      config/fortuna.example.toml (synthesis_cents envelope + [gates.per_strategy.
      synthesis]) AND fixed an S5b bug — CognitionSection's field was `model` but
      the example uses `synthesis_model`, silently dropping the operator's choice;
      renamed to `synthesis_model`. The demo config now supports the synthesis
      arm. Loop-doc tail next: TICK T4.1 (tick the box; the Sim soak is operator-
      started — it needs operator secrets [Slack/deadman, outward-facing], the
      ANTHROPIC_API_KEY for live synthesis, + a release build). Daily
      reconciliation + weekly/monthly reviews are post-tick; the veto's
      AnthropicVetoMind (fortuna-cognition) is the remaining live-synthesis prereq.
      TICKED 2026-06-12 (tail commits 64d45db..304f746): the composition daemon
      is BUILT + battery-gated (full fortuna-live + fortuna-runner suites + DST
      green) — boot-validated config + PgIntentJournal/PgAuditSink, journal-
      generic SimRunner, clock-injected run loop (halt poll <=500ms + poll-failure
      alert), SIGTERM/SIGINT graceful shutdown, GET-only metrics, dead-man
      heartbeat, mech_structural + the opt-in [synthesis] arm (mind_from_env, the
      "synth_events" ledger calibration, per-segment confirmed-edge refresh,
      belief drain+persist) + the opt-in [mech_extremes] arm (reduce-only veto,
      stub) + the RICH daily digest, kalshi-refuses-until-fixtures. The independent
      VERIFIER gates this batch AT the tick (per GATE-FINDINGS). The Sim SOAK is
      OPERATOR-STARTED — running the daemon continuously needs operator-provisioned
      secrets (Slack bot token + deadman URL, both OUTWARD-FACING on the operator's
      infra), the ANTHROPIC_API_KEY for the live synthesis mind (absent => the
      inert StubMind), + a `cargo build --release -p fortuna-live`; I do NOT
      autonomously start an outward-facing continuous daemon on un-gated code.
      HONESTLY DEFERRED post-tick (ledgered, NOT part of the soak's mechanical
      run): daily reconciliation re-run + weekly/monthly cognition reviews; live
      synthesis also needs AnthropicVetoMind (fortuna-cognition, out of bounds).
      [RESOLVED 2026-06-12 — the reviews are no longer deferred: daily
      reconciliation + weekly + monthly reviews are BUILT, wired into drive(),
      tested + mutation-proven, and gate-graded on executed evidence (M2 slices;
      docs/reviews/soak-go-gate-2026-06-12.md B1-B4; GAPS "M2 IS FULLY
      RESOLVED"). The AnthropicVetoMind stays a LIVE-only future item — never a
      Sim-soak blocker. Annotated per the SOAK: GO re-pointed queue item 3.]
      COMPLETION GATE 2026-06-12 (docs/reviews/t41-completion-gate-2026-06-12.md):
      BLOCK with ONE mechanical driver M1 (example-config pin out of sync after
      304f746 added synthesis_cents). FIXED + FULL re-gate battery green
      (fmt/clippy --workspace --all-targets/cargo test --workspace/run-dst.sh
      10000 all exit 0); m3 stale operator-guidance comments also corrected. The
      gate's M2 (daily reconciliation + reviews unbuilt) awaits the operator's
      waive-or-subtask call; M3 (rearm CLI/ROTA notices) is track-B. Details in
      the GAPS "T4.1 completion gate" entry.
- [ ] T4.2 POST-FIXTURE tranche (blocked on the operator recording session):
      Kalshi WS dial (signed handshake, keep-alive, redial w/ resubscribe-on-gap),
      venue-generic recorded-stream replay into PaperVenue under both mech
      strategies, kalshi adapter paper/live clearance vs fixtures, kill-switch
      KalshiVenue plug (FORTUNA_KILLSWITCH_* creds), Slack Socket Mode listener
      (app token; kill REQUESTS only, re-arms stay CLI-only).
- [ ] T4.3 ROTA — the operator dashboard (operator-directed 2026-06-11; design
      AUTHORITATIVE at docs/design/rota-dashboard.md INCLUDING its amendments
      section): read-only gold/black operator console — server-rendered Rust
      shell (NO SPA, no Node toolchain) + versioned /api/rota/v1 JSON views +
      cursor-polled lossless audit tail (SSE cut per critique); capability-
      Option composition: BUILDABLE STANDALONE off SimRunner snapshots — does
      NOT wait for T4.1, which wires it in afterward; dedicated 2-connection
      Pg pool (never the daemon's); structured snapshot views (no Prometheus-
      text parsing); five surfaces first then cognition (two new ledger
      queries owned here); cornucopia/wheel logo under assets/rota/. ZERO
      mutating endpoints (route-table test); seeded-run chain, empty-DB,
      Pg-down, cursor-pagination tests per the doc. Verifier-gated.
      PROGRESS (box stays unticked — slices remain): slice 1 (c3550f9) =
      read-only router + Option-capability state + gold/black shell;
      slice 2 (f8502c9) = daemon-side `views_from` populating the
      health + settlement views and gates/streams scalars from the runner's
      counters()/boards_json()/new active_halt(), wired into main's
      between-segments closure (4 view tests; daemon_smoke green);
      slice 3 (75f4782) = serve_dashboard MOUNTS rota_router (§6) so the
      running daemon actually serves /rota + /api/rota/v1/* (was wired into
      nothing); red-first merge test proves the populated view is served and
      read-only survives;
      slice 4 (ee7ab9d) = streams recorder filesystem-scan (scan_recorder,
      metadata-only — never a content read, dodging the 1.3GB line-count DoS;
      §5 rows_today/key_count deferred), merged into /streams when
      perishable_dir present; daemon wires perishable_dir="data/perishable";
      slice 5 (468c8c1) = R5 dedicated audit pool (connect_readonly_pool,
      isolated 2-conn, never the writer's) wired into the daemon's RotaState so
      the audit TAIL is LIVE; available:true path HTTP-tested. Also: rota-slices
      + audit-tail-fix gate findings ALL remediated (F1-F6 + #1-#4);
      slice 6 (19954a6) = gates.rejections_by_check (new SimRunner::
      rejections_by_check() read accessor -> views_from; {check,count} summing
      to total_rejections, the consistency invariant tested);
      slice 7 (this commit) = money view SIM-ONLY subset (R6; verifier-endorsed
      unblock) — boards "account" block from inspect_totals; settled=cash,
      committed=reserved, positions; floating/total NULL (mark loop not exposed).
      The FIVE primary surfaces now carry real data.
      REMAINING: cognition view (R7, pool-unblocked); the FULL §5 money model
      (mark-loop floating + per-strategy attribution — operator/design call);
      Phase-3 shell/assets; R12 browser pass.
      slice 8 (track B, this commit) = COGNITION panel (R7): the two
      T4.3-owned ledger queries (BeliefsRepo::recent w/ evidence+provenance,
      CalibrationParamsRepo::scopes DISTINCT-ON max version) + sqlx prepare
      (2 new cache JSONs committed), /api/rota/v1/cognition merging the
      daemon counters view (explicit unavailable until synthesis-in-main —
      never fabricated zeros) with ROTA's own ledger arrays over the R5
      pool, 4KB evidence truncation (explicit truncated:true), shell panel
      + poll. 4 ledger + 5 ops tests, populated-path seeds throughout.
      RotaState budget fields deliberately omitted (track A's struct-
      literal construction site — GAPS note). REMAINING: full §5 money
      model (operator call), recent_rejections/recent_watchdog audit
      queries, Phase-3 logo/shell assets, R12 browser pass.
      slice 9 (track B, this commit) = Phase-3 presentation + §9 logo:
      assets/rota/logo.svg (8-spoke wheel + cornucopia, all gold, asserted
      geometry) served at /assets/rota/logo.svg + as the SVG favicon
      (interim 204 stub replaced w/ STRONGER test asserts per the stub
      test's own anticipation note); shell upgraded to per-panel renderers
      — cents→dollars, UTC labels (R6), halt pill, cognition click-to-
      expand evidence rows, per-panel raw expander, §0.4 poll cadences.
      Track B's T4.3 queue items are now COMPLETE (cognition view + R7
      queries + presentation layer + logo). Box stays unticked: full §5
      money model (operator call), audit-recents queries, and the R12
      browser pass (verifier) remain.
- [x] T4.4 Operator CLI lifecycle (operator-directed 2026-06-11; design
      AUTHORITATIVE at docs/design/fortuna-cli.md INCLUDING its amendments
      section): `fortuna start/stop/status/logs/config-check` extending
      crates/fortuna-cli (start/stop naming binding; v1 cut to FOUR new
      commands per critique — mode and db migrate-status dropped); pidfile +
      SIGTERM with name-validated PIDs + atomic create_new claim; start
      REFUSES on an unmanaged running fortuna-recorder; detach via
      process_group(0) + append-mode logs; runtime dir data/runtime/
      (gitignored), NOT /tmp; depends on the T4.1 SHUTDOWN CONTRACT above.
      Verifier-gated.
      PROGRESS (track B; box stays unticked — start/stop remain): fit-
      validation recorded in the design doc §11 (BUILDABLE AS AMENDED);
      slice 1 (this commit) = read-only surfaces, tests-first (15 in new
      tests/cli_integration.rs): `config check` (FortunaConfig whole-shape
      validation), `logs daemon|recorder [-f]` (exec tail -n50 of the
      redirected logs), `status` process-health section from name-validated
      pidfiles (A3 semantics: dead pid / name mismatch / unparseable all
      read stale-stopped) + A7 stopping-marker display + A6 config-on-disk
      venue line + degradable db section (A9 pinned: no DATABASE_URL ->
      exit 0; unreachable Pg -> warn + exit 0, 5s bound). db queries moved
      verbatim to status_db_section. NEXT: `start` (A2 recorder refusal,
      A3 atomic claim, A4 detach), then `stop` LAST against T4.1's asserted
      SIGTERM contract (A1).
      slice 2 (this commit) = `fortuna start [--foreground]`: config-check
      gate -> per-component already-running scan (validate-then-decide on
      existing pidfiles: running/stale/mid-claim) -> A2 unmanaged-recorder
      refusal via pgrep -f (whole-start refusal, decoy-tested
      deterministically) -> A3 O_EXCL claim-then-spawn (8-thread race
      unit-tested, claim released on spawn failure) -> A4 detach
      (process_group(0), stdin null, append-mode logs unit-tested never to
      truncate) -> A8 active-halts print + A10 best-effort lifecycle audit
      row (5s bound); recorder invocation pinned to the A2 live defaults w/
      optional [recorder] config override; --foreground execs fortuna-live
      (exit code propagation tested). Success spawn path = manual runbook
      per §9 (+ this box intentionally hosts the operator's unmanaged
      recorder). 9 unit + 6 new integration tests. NEXT: `stop` (A1/A7).
      slice 3 (this commit) = `fortuna stop [--timeout-secs]` — COMPLETES
      the v1 command set; BOX TICKED. SIGTERM (shell-out kill -15) daemon
      then recorder, never SIGKILL; A1: success requires the daemon's
      "fortuna-live: clean shutdown" line in the managed log AT/AFTER the
      pre-signal byte offset (stale markers from earlier runs rejected,
      test-pinned incl. a pre-seeded-marker case); A7: timeout => verbatim
      do-NOT-kill-9 warning, pidfile + stopping marker left, STILL
      proceeds to the recorder, exit 1; stale pidfiles never signaled
      (A3); zombies read as not-running (production-correct comm_of stat
      check, found red-first); best-effort lifecycle stop row (A10, dead
      DB never blocks). 7 new integration + 2 new unit tests (38 total in
      the crate); §13 manual smoke runbook recorded in the design doc.
      A6 deferral stands: status Section 3 (metrics poll) awaits ROTA.
      Verifier gate pending as with every batch.

- [ ] T4.5 ROTA v1.1 — deferred panels (RESTORED 2026-06-13; the original
      entry was lost in the perps merge-revert churn — completion-audit
      finding). Scope: the two ledger queries (shadow cross-join for triage
      recall/precision; Tradability/Edges discovery join) + their panels;
      the gate-verdict badge (header parser pinned against existing verdict
      files, parse failure renders "unknown"); WS gap/resync counters flip
      live (post-T4.2); PLUS re-scoped from T4.3 per the completion audit:
      the full s5 money model (mark-loop floating source) and audit-recents
      queries. TEST RULE: populated-path seeds; a test green under a
      stubbed-empty source does not count. Verifier-gated.

OPERATOR DIRECTIVE (2026-06-11 night, recorded by the verification session):
morning target = the daemon running in DEMO mode (Kalshi demo env, mock funds)
with the Anthropic mind active under budgets, ROTA up locally, and the perps
track as far through T5.B2-B6 as gate-clean work allows. Priority order for the
implementer loop: gate findings in GAPS -> T4.1 -> T4.3 (ROTA) -> T4.4 (CLI)
-> T4.2 -> T5.B tranche in order. The north star governs: work that has not survived an
independent gate counts as zero; demo-mode startup is itself gated on T4.1+T4.2
clearance, and NOTHING here changes I1-I7, fixture-gating, or operator-only
actions (promotions, re-arms, live capital stay with the operator).

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
  Phase B CONFIRMED by the operator 2026-06-11 (B1-B8 + amendments A/B/C +
  original-list merge; docs/design/kinetics-perps-module-plan.md is authoritative):

- [x] T5.B0 Perishable-data recorder (amendment A; FIRST and standalone): public
      perps books + spreads, KXBTC15M bracket quotes paired per capture cycle,
      intraday funding estimates/marks; JSONL persistence; runs continuously.
      (DONE 2026-06-11: crates/fortuna-recorder, 8 unit tests, RUNNING since
      06:52 UTC — series KXBTC15M,KXBTC,KXBTCD; the log header records the
      RUNNING PARAMETERS (not a command — ledger-gate fix 4b); restart from
      repo root: `./target/release/fortuna-recorder --interval-secs 30
      --bracket-series KXBTC15M,KXBTC,KXBTCD >> data/recorder.log 2>&1 &`.
      ONE WRITER ONLY (see ASSUMPTIONS single-writer entry); survives session
      end, NOT reboot — launchd plist on request.)
- [x] T5.B1 Spec v0.9 amendment: 5.15 perps domain (InstrumentKind, PerpPrice,
      portfolio-margin exposure stance + dedicated margin envelope, liquidation-
      distance floor + leverage caps, funding cash-flow entries, liquidation-fill
      lifecycle, fee-trap rule, funding_carry data-only) + stale 5.2 fee touch-up.
      (DONE: spec commit 213e41f [post-rewrite hash]; graded faithful on all
      C3 criteria by the batch gate; tick released by the remediation ACCEPT
      addendum, docs/reviews/perps-b0-b1-remediation-regate-2026-06-11.md.)
- [x] T5.B2 Core perps types: PerpPrice, signed PerpPosition, FundingAccrual,
      MarginAccountView w/ conservative marking; property tests on conversions.
      (DONE track C 2026-06-12, commit 56d07db: fortuna_core::perp — 38 tests
      incl. 7 property suites written from spec 5.15 before implementation;
      battery green except ONE pre-existing fmt diff on main outside track-C
      ownership, ledgered in GAPS.)
- [x] T5.B3 Gate extensions: margin headroom, liquidation-distance floor,
      per-asset leverage caps, funding-drag-in-edge, notional caps; worst case =
      liquidation loss. Invariant-crate ADDITIONS only (I2 extension tests).
      (DONE track C 2026-06-12, commits 7782f5c + b4561ca: perp gate arm on
      the shared GatePipeline + equity_with_margin I2 composition + perp
      I1/I2/I3 invariant ADDITIONS [pure, 0 deletions — operator waive
      queued in GAPS]; daemon wiring of the composed equity lands with the
      B4/B5 runtime integration, ledgered.)
- [x] T5.B4 Kinetics adapter (own credentials; dedicated 5.14 envelope):
      REST+WS from the archived specs, doc-derived samples, FIXTURES-GATED vs
      fixtures/kinetics-perps/ (18-item list, research section 12).
      (DONE track C 2026-06-12, commits dd82ca1 + c4f6248 + e3d0dde +
      bbadfc0 + converter in 3b21b7e: DTOs [all 76 bodies, full-coverage
      classification], client [every request meta-equality-gated],
      adapter [GatedPerpOrder-only place, reduce-only/IOC pre-wire rule,
      AlreadyExists via list scan, Liquidation fill class, fee
      reconciliation surfacing the promo-\$0 discrepancy], WS session
      [recorded commands, seq/torn discipline, both streams replay
      gapless]. OPEN venue-state items stay in GAPS: funding_history
      entry shape, notional-limit values, duplicate-scan pagination,
      live-dial composition.)
- [x] T5.B5 Paper engine margin semantics: funding accrual on SimClock,
      liquidation sim from recorded risk curves, mark-based PnL.
      (DONE track C 2026-06-12, commit e8fe069: fortuna-state MarginSim —
      VWAP-against-us position math, 04/12/20-UTC funding schedule +
      append-only accrual log, whole-account liquidation at worse-mark +
      penalty with unbounded-curve/missing-mark = error; 20 spec-first
      tests; fortuna-paper wiring ledgered for that crate's owner.)
      [CORRECTED 2026-06-12 per gate fix F1, never erased: the original
      note's "liquidation sim from recorded risk curves" OVERSTATED the
      data source — at e8fe069 the sim consumed operator-CONFIG curves
      and no recorded-curve path existed. The converter landed with the
      gate-fix batch: RiskCurve::from_leverage_estimates builds the
      curve from the RECORDED venue leverage_estimates, shape-tested
      against fixtures/kinetics-perps/markets__single.json.]
- [x] T5.B6 DST arms: funding-tick chaos, liquidation under ack-delay/api-error,
      system-fill ingestion, margin-call sequences, demo-divergence confusion.
      (DONE track C 2026-06-12, commit 335e5e6: perp_dst battery stage —
      6 accounted arms incl. wild-regime margin-call sequences reaching
      real liquidations [786 @ 2000 scenarios]; 7 per-seed invariants
      incl. gate-pass=>no-instant-liquidation, never-silent liquidation,
      exact conservation, same-seed determinism; coverage floor at 100+.)
- [ ] T5.B7 Strategies rung 0: perp_event_basis (Sim), funding_forecast
      (zero-capital scalar claims), funding_carry DATA-ONLY until >=60d regime
      evidence (amendment B). FEE-TRAP RULE (amendment C): edge floors at assumed
      post-promo fees (5-12 bps until fee_tiers is real); Sim gates re-run when
      fees activate; promo-$0 never justifies GO. I7 unchanged.
- [ ] T5.B8 Ops: kill-switch perps flatten (reduce_only IOC + cancel-all),
      margin/funding telemetry, funding-regime dashboard panel.

PHASE 5 EXIT (written 2026-06-13, completion-audit finding — no EXIT existed):
the perps plane merged to main gate-clean (re-merge package: client-id test
fix + the operator-confirmed 2x leverage cap + full 10k re-gate + post-merge
integration incl. the kinetics adapter test); T5.B7 rung-0 strategies run in
Sim under the fee-trap rule with promo-$0 never justifying GO; T5.B8 ops
landed (kill-switch perps flatten, margin/funding telemetry, ROTA panel);
perp DST arms in every battery; zero changes to the invariant middle
(additions under signed waives only). Live perps trading remains behind the
I7 ladder and the operator's Kinetics PROD parity sweep — out of scope for
the EXIT.


## Track E — Domain-analysis personas (operator-directed 2026-06-13; design committed + APPROVED)

Authoritative design: docs/design/domain-analysis-personas-design.md (§18 six-slice plan).
A versioned, auditable library of analyst personas + a persisted, append-only domain-analysis
artifact the cognition layer consumes when forming beliefs; proven end-to-end on the
meteorologist. Ownership (absolute): the persona LAYER in fortuna-cognition + new ledger
tables/repos; never touches fortuna-sources (Track D); extends, never breaks, the Mind/belief
interface Track A composes. ROTA views (§14/§20) are Track B's to build (Track E provides
data); persona telemetry (§19) folds into slices E.3–E.5. fortuna-invariants is touched only
at E.3 (the PersonaOutcome no-order/size field-surface pin) under operator waive.

- [x] E.1 Ledger — `personas` + `domain_analyses` tables + migration + append-only repos
      (content-immutable guard on domain_analyses freezing all 12 content columns, only
      `status` flips; supersedes-chained `personas` with UNIQUE(persona_id,version)).
      (DONE this commit: migration 20260613000001_personas.sql; PersonasRepo +
      DomainAnalysesRepo in fortuna-ledger; 6 #[sqlx::test] guard/round-trip/supersession
      tests, mutation-proven; per-crate .sqlx regenerated; full workspace battery green
      [fmt/clippy --workspace --all-targets/cargo test --workspace 123 ok-suites/run-dst 2000];
      adversarial spec+code review clean [no Critical/Major]; fortuna-invariants UNTOUCHED.)
- [x] E.2 Persona definition + registry — skill-file loader (config/personas/<id>/), method_hash
      validation against the registry head; refuse a config/registry hash mismatch.
      (DONE this commit: fortuna_cognition::persona — PersonaDef::parse [TOML `+++` frontmatter +
      trusted method body; method_hash = SHA-256 of the whole persona.md] + validate_against
      [fail-closed; refuses NotRegistered/Inactive/VersionMismatch/HashMismatch — §4(d)/§6];
      pure core, no fs IO; RegistryHead a pure cognition input. Shipped config/personas/
      meteorologist/{persona.md v1, schema.json}. 14 tests; full workspace battery green;
      feature-dev review applied [status fail-closed + split_frontmatter .get() hardening].)
- [ ] E.3 Runner loop + triggers + budget + context + findings contract — scripted-StubMind
      determinism, the trusted/untrusted separation tests (§4 a–d), DST runner-under-budget arm;
      persona telemetry counters (§19); the PersonaOutcome no-order/size invariant pin (§15).
      [E.3a DONE (commit 4e8b9e4): fortuna_cognition::persona_runner — run_persona_analysis
      (budget-first, untrusted-signals-only context, Mind.decide, strict findings validation,
      content_hash anchor); PersonaOutcome order-free + Serialize; the FIREWALL headline tests
      (method never in context; planted injection renders as data) + determinism + degrade arms.
      E.3b DONE (commit 96cdb79): fortuna_cognition::persona_trigger — Cadence (fire-once-per-period,
      generalizing DailyScheduler) + validate(); PersonaTriggerSpec::fires_on_signal; and the
      PersonaTriggerGate reusing signals::TriggerEngine for per-(persona,region) coalescing.
      E.3c DONE (commit 510ee8e): the seeded persona-runner DST arm (tests/persona_dst.rs).
      E.3 telemetry DONE this commit: fortuna_cognition::persona_metrics — PersonaCounters folds
      PersonaOutcomes → the §19 funnel counters + cost counter + spend_today gauge; samples()
      shape-compatible with MetricSample; accounting identity test-pinned; 10 tests; full battery
      green. REMAINING E.3: ONLY the §15 PersonaOutcome invariant pin (operator-waive, see GAPS).]
- [ ] E.4 Belief consumption — DomainAnalysis section + evidence/provenance citation; the
      μ/σ→p helper in code; `fortuna_persona_beliefs_total` metric.
      [E.4a DONE this commit: fortuna_cognition::persona_beliefs — normal_cdf/prob_at_least (the
      μ/σ→p backbone, clamped) + map_persona_analysis (artifact findings → one binary BeliefDraft
      per threshold/outcome, evidence + provenance replay anchor, dedup'd event_ids); 12 tests;
      full battery green. REMAINING E.4: E.4b SectionKind::DomainAnalysis context item +
      fortuna_persona_beliefs_total (the beliefs metric folds into the §19 counters at wiring).]
- [ ] E.5 Scoring scope extension — ScopeKey + weekly-review promote/retire proposal (baseline +
      market comparison; recommendation-only); resolved_beliefs/clv_bp metrics.
- [ ] E.6 End-to-end meteorologist proof over Aeolus (+ NWS/fixture) + the macro mechanism test;
      the §11 evaluation gate wired; full battery green.

ROTA views (§14/§20) + persona telemetry (§19) are operator-requested detailed contracts
(2026-06-13) — Track B builds the four views; Track E provides the data across E.1–E.5.
