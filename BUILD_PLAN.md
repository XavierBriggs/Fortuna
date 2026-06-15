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
      ✅ STATUS 2026-06-14 (verifier de-stale): core slices (i)–(v) DONE and the live
      e2e RUN this session; box stays open ONLY for (a) operator trade-through /
      multi-market FIXTURE recapture (busy-market recording) + (b) the small G2
      exchange-status DTO — every other slice is complete.
      Kalshi WS dial (signed handshake, keep-alive, redial w/ resubscribe-on-gap),
      venue-generic recorded-stream replay into PaperVenue under both mech
      strategies, kalshi adapter paper/live clearance vs fixtures, kill-switch
      KalshiVenue plug (FORTUNA_KILLSWITCH_* creds), Slack Socket Mode listener
      (app token; kill REQUESTS only, re-arms stay CLI-only).
      PROGRESS (box stays unticked — slices remain): (i) Kalshi WS dial slices
      1-2 + 4-5 + concrete transport CERTIFIED (t42-wsdial/redial/wsdial-transport
      gates 2026-06-13; live socket round-trip is operator-run first); (ii) book-
      driven recorded-stream replay into PaperVenue under both mech strategies DONE
      (e6dd7ec, recorded_replay.rs) — trade-through + multi-market-bracket replay
      stay fixture-blocked (ledgered GAPS, never fabricated); (iii) 27-item paper-
      clearance Cluster 1 DONE (f7206a4, kalshi_recorded.rs 18 tests + clearance
      record docs/design/track-a-kalshi-paper-clearance.md) — first recorded-fixture
      adapter tests; exposed 2 adapter gaps — G1 nested-error extraction RESOLVED
      (b2087fc), G2 exchange-status DTO pending; Cluster 2 CORE DONE (811e383,
      kalshi_recorded_roundtrip.rs: place/place-400/cancel-race/fills) + TAIL DONE
      (1e96d20: item 7 recorded 409→AlreadyExists; items 5/12 closed by cited
      coverage) — clearance now PASSes 5,7,12; Cluster 3 auth-401 routing DONE
      (fe86cb5), WS live handshake op-run. Remaining:
      (iv) kill-switch Kalshi freeze MACHINERY proven (4e3a484, mock; i4
      invariant green) + LIVE `freeze --venue kalshi` wiring DONE (7f69b81,
      fail-closed env creds + self-spun tokio runtime; i4 invariant still green;
      9 fail-closed tests) — operator-run live EXERCISE DONE 2026-06-14 (verifier ran
      the live e2e vs the Kalshi demo under operator override: signed WS handshake +
      fixture recording + `freeze --venue kalshi`); (v)
      Slack listener A1 decision logic DONE (ca5082d, socket.rs 14 tests; I2
      refusal airtight) + A2 ack-first envelope loop DONE (f52ee66,
      socket_loop.rs 12 tests; dedup/reconnect/cancel, mirrors the WS dial) —
      only B (daemon-wiring/WSS/token, operator-gated) pending.
- [ ] T4.3 ROTA — the operator dashboard (operator-directed 2026-06-11; design
      ✅ STATUS 2026-06-14 (verifier de-stale): TRACK work COMPLETE — all 9 slices
      (router/shell, views_from, mount, streams scan, audit pool, rejections,
      money SIM subset, cognition view + R7 queries, Phase-3 presentation + logo).
      Box stays open ONLY for the operator §5 full money model (re-scoped → T4.5)
      + the R12 manual browser pass. The dashboard renders live daemon data today.
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
      ✅ STATUS 2026-06-14 (verifier de-stale): the buildable surface is COMPLETE
      (recent-rejections + watchdog + verdict badge + discovery-edges joins, all
      verifier-gated). Box stays open ONLY for operator-BLOCKED items: (c) WS
      gap/resync counters (need the operator-run live dial wired into drive()) +
      (d) the full §5 money model (operator/design call).
      entry was lost in the perps merge-revert churn — completion-audit
      finding). Scope: the two ledger queries (shadow cross-join for triage
      recall/precision; Tradability/Edges discovery join) + their panels;
      the gate-verdict badge (header parser pinned against existing verdict
      files, parse failure renders "unknown"); WS gap/resync counters flip
      live (post-T4.2); PLUS re-scoped from T4.3 per the completion audit:
      the full s5 money model (mark-loop floating source) and audit-recents
      queries. TEST RULE: populated-path seeds; a test green under a
      stubbed-empty source does not count. Verifier-gated.
      PROGRESS (box stays unticked — slices remain): VALIDATED 2026-06-13
      (rota-dashboard.md §10 "T4.5 validation"; the R5 audit pool these were
      deferred behind is now live). Build order: (e) /gates.recent_rejections
      [gate_reject audit] -> (e) /settlement.recent_watchdog -> (a) discovery
      joins -> (b) verdict badge. SLICE 1 DONE (59fa594): /gates.recent_rejections
      (recent gate REJECTIONS from the audit gate_decision trail). SLICE 2 DONE
      (9558d56): /settlement.recent_watchdog_events (the audit watchdog rows —
      settlement_overdue/dispute_freeze/orphaned_position); both §5-shaped,
      newest-first, 3 populated-path tests each. SLICE 3 DONE (7ed3138): the
      gate-verdict badge /api/rota/v1/build (docs/reviews verdict parse, local
      console; RotaState.reviews_dir). Battery green (1391/0 + run-dst 200 0-viol).
      (a) the discovery joins are NOT track-A — design §4/§12 defer them + "discovery"
      observability is track-B. So the buildable-WITHOUT-OPERATOR T4.5 surface is
      COMPLETE; the rest is BLOCKED (operator/verifier, GAPS): the WS gap/resync
      counters (need the operator-run live dial wired into drive()); the full s5
      money model (need an operator/design call surfacing the mark-loop AccountView
      via a new SimRunner accessor).
      (a) DISCOVERY JOINS DONE (2026-06-14, track-B): the Discovery — Edges board
      (/api/rota/v1/discovery_edges) — the live (non-superseded) market↔event mappings
      ⋈ events (statement + mapping + confidence + confirmed/proposed status +
      proposer/confirmer), runtime sqlx, NO ledger change. Populated-path test (join
      carries statement + supersession EXCLUSION + honest-null confirmer + status split)
      + degraded-unavailable coverage + screenshot rota-discovery-edges-2026-06-14.png;
      code-reviewer-clean; full battery green. So the buildable T4.5 surface is now (e)+
      (e)+(b)+(a); only (c) WS counters + (d) full money model remain (operator-BLOCKED).
      (a) Tradability⋈Edges JOIN COMPLETE (2026-06-14, "complete all slices"): the
      Discovery — Edges board gains a Trad column (the market's latest tradability_scores
      score, correlated subquery; honest-null when unscored) — the literal "Tradability/
      Edges discovery join" §4 named. + two more ready ROTA follow-ons drained the same
      pass: Sources Health operational fields (fetch_errors/rearms/last_error) and the
      Analyses §20.2 expander (per-analysis findings + signal_manifest, untrusted →
      truncate_evidence + esc'd). Tests + curl-verified (browser screenshots deferred —
      chrome-devtools MCP disconnected); full workspace battery GREEN (167 suites 0-fail /
      DST exit-0). Remaining ROTA work is all data/operator-BLOCKED or marginal (GAPS
      "COMPLETE ALL SLICES"): T4.6 perp-CDF (track-C basis-v2), money mark-loop, WS
      counters, Vendor Scorecard, Forecast→Outcome, Hypothesis-Lifecycle full, persona
      verdict; sparklines/DB-drill-in/Events-drill-in are diminishing-returns.

- [x] T4.6 ROTA TOTAL OBSERVABILITY (mission 2; track B re-missioned 2026-06-13) — ✅ DONE: all 13 sub-boards built + screenshot-verified (the only open item in its range was this parent box). Ticked 2026-06-14 (verifier de-stale).
      §9.2 /perps DONE (2026-06-14): the perps DISPLAY half — realized funding + the §2.6
      A2d edge gate (forecast vs 4 baselines, beats_all) + perp basis-v2 (A10) regime +
      CDF divergence (the previously C-blocked perp-CDF, now shaped R2-clean from
      runner.metrics_export() via perps_basis_board→views["perps_basis"]). rota.rs +
      views_from seam; 4 tests; full battery GREEN. GAPS "§9.2 /perps board DONE".
      — the operator single pane of glass consuming the C/D/E observability
      contracts. Authoritative tracker (board matrix + changelog):
      docs/design/rota-observability.md; sequenced queue + cross-track data-seam
      requests: GAPS.md "TRACK B — RE-MISSIONED ... TOTAL ROTA OBSERVABILITY".
      Read-only + honest-nulls absolute; every board screenshot-verified with
      real rows before its sub-box ticks. Verifier-gated.
  - [x] item 0 — local bringup harness (crates/fortuna-ops/examples/rota_local.rs)
        + the 7 existing boards screenshot-verified with real rows off a seeded
        local DB (d8bae95). The reusable screenshot rig for later boards.
  - [x] V2 Sources Health (D ingestion contract) — generic {title,columns,rows,
        summary} boardTable renderer (reused by V1/V3-V6) + /api/rota/v1/ingest_sources
        handler + populated-path test; screenshot-verified with real rows (c521ae1).
        Prod data pending track-A OBS-2 publish (IngestionTelemetry on main).
  - [x] V1 Live Signal Feed (D ingestion contract, the marquee) — reuses boardTable
        via a data-driven `pill` column flag + /api/rota/v1/ingest_feed handler +
        populated-path test; screenshot-verified with real rows (cc463bb).
        Prod data pending track-A OBS-2 feed publish.
  - [x] V3 Ingest Funnel (D ingestion contract) — funnel-as-stage-table (reuses
        boardTable): fetched→validated→normalized→persisted with retention % +
        drop-offs + /api/rota/v1/ingest_funnel + populated-path test;
        screenshot-verified (d650710). Completes the live ingestion triad
        (V1/V2/V3, 10 boards). Prod data pending OBS-2 (loop-stages null-until-wired).
  - [x] Cognition belief LIFECYCLE (mission item 1 / D V6 partial) — the cognition
        board deepened with the belief status distribution + resolved calibration
        (Brier/CLV) via a real GROUP BY/AVG (runtime sqlx); DB-backed populated test
        (2b76e67). D V6 full belief→strategy→PnL is SCHEMA-BLOCKED (no
        belief→trade link; GAPS) — calibration edge proxy surfaced, never a fake PnL.
  - [x] OBS-2c — the live ingestion boards (V1/V2/V3) now render LIVE daemon data:
        merge_ingest_views (fortuna-live views.rs, the ROTA seam) shapes the
        published IngestionTelemetryHandle (track-D OBS-2b) into the board envelopes
        at the snapshot-composition site (this commit). Honest gate keeps the daemon
        byte-unchanged when ingestion is off (daemon_smoke 15/15); ROTA stays a pure
        snapshot reader (no fortuna-sources dep). The live ingestion triad is LIVE.
  - [x] OBS-3 RENDER — Sources Health surfaces domain_tags + trust_tier (2026-06-13;
        consumes track-D OBS-3, which populated SourceTelemetry.domain_tags AFTER OBS-2c
        shaped the board): sources_board (views.rs) now emits Domains (domain_tags joined;
        honest null when untagged) + Tier (trust_tier) — registry-admission config, NOT
        untrusted data; boardTable renders generically (no rota.rs/JS change). Existing
        shapes test asserts domains/tier + a new honest-null test (join + untagged→null);
        screenshot rota-sources-health-domains-2026-06-13.png; full battery GREEN (165
        suites 0-fail/DST exit-0). Closes the orchestrator "domain_tags" item; T4.5 panels
        = track-A/operator-blocked (not track-B).
  - [x] Recent Fills board (mission item 3, "trades being executed") — recent_fills
        runtime-sqlx query over the durable fills ledger + view_fills handler + a new
        data-driven `cents` column flag (price/fee as dollars); fortuna-ops only, the
        audit-tail pattern; DB-backed populated test; screenshot-verified (this commit,
        11 boards). Follow-ons (GAPS): per-strategy P&L + working orders (views_from
        from runner accessors), unrealized PnL gapped (no mark loop).
  - [x] Strategy P&L board (mission item 3, "realized PnL per strategy") — views_from
        (the ROTA seam) shapes runner.digest_snapshot().strategies into
        snapshot.views["strategies"] + view_strategies handler + the cents flag
        (realized/fees/open as dollars, negatives honest); fortuna-live + fortuna-ops;
        views.rs + handler tests + daemon_smoke 15/15; screenshot-verified (4c2fcd6,
        12 boards). Working orders + unrealized PnL remain (GAPS).
  - [x] Discovery — Events board (mission item 4, "canonical events + markets") —
        recent_discovery_events runtime-sqlx query (events LEFT JOIN
        market_event_edges, COUNT DISTINCT market_id supersession-safe) +
        view_discovery handler; fortuna-ops only; DB-backed populated test;
        screenshot-verified (this commit, 13 boards). Benchmark detail + per-event
        drill-in + sources inventory are follow-ons (GAPS).
  - [x] Database board (mission item 5, "honest visibility into the actual tables —
        counts") — db_table_counts runtime-sqlx (exact COUNT(*) over all 24 ledger
        tables incl. the scalar_beliefs/belief_scores plane, literal names/no
        injection, ORDER BY busiest-first) + view_db handler + {tables,total_rows}
        summary via boardTable; fortuna-ops only; DB-backed populated test (24-table
        inventory + real counts + ordering + honest 0 + scalar-plane sweep guard);
        reviewer-clean; FULL-WORKSPACE battery green (fmt+clippy+test 1263/0+run-dst
        exit 0); screenshot-verified (this commit, 14 boards). reltuples-at-scale +
        per-table drill-in are follow-ons (GAPS).
  - [x] Personas board (mission item 1, "how beliefs are formed — the roster of
        analysts"; track-E §20.1 registry half) — persona_registry runtime-sqlx over
        the personas table (every (persona_id, version) grouped, newest version first;
        status pill, 8-char method hash, reads_signal_kinds flattened) + view_personas
        handler + {personas,versions,active} summary via boardTable; valuePill extended
        for active; fortuna-ops only; DB-backed populated test (grouped ordering +
        active/retired status + joined reads + method prefix + summary); reviewer-clean;
        FULL-WORKSPACE battery green (fmt+clippy+test 1264/0+run-dst exit 0);
        screenshot-verified (this commit, 15 boards). §20.1 scorecard half
        (Brier/CLV/verdict) data-blocked on track-E persona scoring; §20.2 analyses
        browser + §20.3 cognition persona-provenance are the remaining E slices (GAPS).
  - [x] Domain Analyses board (mission item 1 / track-E §20.2, "the whole process —
        the analyses beliefs are built from") — recent_analyses runtime-sqlx over
        domain_analyses (artifact ledger newest-first: persona id@version, region_key,
        produced_at, cost as dollars, content_hash prefix, supersession status) +
        view_analyses handler + {analyses,open,cost_cents} summary via boardTable;
        UNTRUSTED findings/signal_manifest NOT exposed (metadata only — reviewer-
        confirmed); fortuna-ops only; DB-backed populated test (produced_at-DESC +
        persona render + cost + hash + open-vs-superseded supersession); reviewer-clean;
        FULL-WORKSPACE battery green (fmt+clippy+test 1265/0+run-dst exit 0);
        screenshot-verified (this commit, 16 boards). The per-artifact expander
        (findings/manifest/beliefs-fanout, esc'd) + §20.3 cognition persona-provenance
        + §20.1 scorecard are the remaining E slices (GAPS).
  - [x] Forecasts scorecard (track-C §9.1, "the outcomes of the whole process") —
        forecast_scorecard runtime-sqlx (scalar_beliefs ⋈ belief_scores, resolved
        only, GROUP BY producer,rule_id → mean CRPS lower=better + resolved_n + unit)
        + view_forecasts handler + {producers,rules,scored} summary via boardTable;
        untrusted quantiles/provenance NOT exposed (reviewer-confirmed); fortuna-ops
        only; DB-backed populated test (producer ordering + mean CRPS + resolved
        counts + unit, f64-tolerance); reviewer-clean; FULL-WORKSPACE battery green
        (fmt+clippy+test 1266/0+run-dst exit 0); screenshot-verified (this commit, 17
        boards). Degrades honest-unavailable until track-C daemon persist (slice 4)
        writes the tables. Recent-feed + coverage_bps + sparkline + §9.2 /perps are
        follow-ons (GAPS).
  - [x] Working Orders board (mission item 3, "trades being executed" — live side) —
        a views_from board (the ROTA seam, fortuna-live): views_from folds
        runner.manager().intents() filtered by IntentStatus::is_working() (submitted/
        acked/partially-filled) into snapshot.views["working_orders"] (market, side,
        action, limit as dollars, qty, filled, status pill, submitted-at) +
        view_working_orders read_view handler; pure panic-free read (daemon_smoke 15/15
        unchanged); fortuna-live populated-path test (ack_delay → 3 resting legs) +
        fortuna-ops handler test; reviewer-clean; screenshot-verified (this commit, 18
        boards). BATTERY: green for ALL track-B work + the whole workspace EXCEPT one
        PRE-EXISTING main red (kinetics_dto paired_cycle fixture unclassified, track-C
        @2c17295 — NOT track-B, fortuna-venues out of ownership; ledgered for verifier).
        Mission item 3 substantially complete (fills + working orders + strategy P&L);
        unrealized PnL remains the mark-loop gap.
  - [x] Persona Scorecard board (track-E §20.1 outcomes half — UNBLOCKED by the merged
        persona runtime) — persona_scorecard runtime-sqlx (AVG over resolved beliefs
        grouped by provenance->>'persona_id': n_resolved + mean Brier + mean CLV) +
        view_persona_scores handler + honest evaluating(n/60) verdict (PROMOTABLE/RETIRE
        + baselines + calibration_quality OMITTED — unpersisted/cognition logic, R2);
        fortuna-ops only; DB-backed populated test (per-persona MEAN brier/clv + verdict);
        reviewer-clean; battery green EXCEPT the same pre-existing main kinetics_dto red
        (track-C/A, not track-B); screenshot-verified (this commit, 19 boards). Completes
        the Personas board's two halves (registry + scorecard). The §20.1 baselines/
        verdict + §20.3 provenance-linking + §20.4 pipeline funnel remain (GAPS).
  - [x] Telemetry board (mission item 6, "the Prometheus stack on the console" — the
        LAST untouched pillar) — MetricsRegistry::telemetry_board (NEW fortuna-ops
        method) folds the registry's structured series into a board (one row per
        series: subsystem-from-name-prefix + metric + type + value, grouped by
        subsystem); daemon composition adds one additive views["telemetry"] key (the
        ROTA seam, daemon_smoke 15/15); view_telemetry is a read_view passthrough (R2 —
        no Prometheus-text parsing). metrics.rs unit test (the shaping) + fortuna-ops
        handler test + harness exercises the real telemetry_board; reviewer-clean;
        battery green EXCEPT the same pre-existing main kinetics_dto red (track-C/A);
        screenshot-verified (this commit, 20 boards). >>> THE SINGLE PANE OF GLASS NOW
        SPANS ALL 6 MISSION ITEMS (cognition, pipeline, trades, discovery, DB,
        telemetry). Live prod metrics populate when the daemon runs; help-text + metric
        search are later polish (GAPS).
  - [x] Forecast Feed board (track-C §9.1 recent half, "did the vendor call it?") —
        recent_forecasts runtime-sqlx over scalar_beliefs (the recent individual
        forecasts newest-first: producer, event, unit, the median extracted from the
        quantile fan, realized outcome / pending status); the companion to the
        /forecasts scorecard. Untrusted quantiles fan + provenance NOT exposed (only
        the median number — reviewer-confirmed); fortuna-ops only; DB-backed populated
        test (resolved + pending, median extraction + honest-null realized); reviewer-
        clean; battery green EXCEPT the same pre-existing main kinetics_dto red (a first
        full-workspace run also hit transient createdb contention from concurrent
        verifier/IDE Postgres load — proven transient by rota 34/34 in isolation + a
        clean re-run); screenshot-verified (this commit, 21 boards). §9.1's two halves
        (scorecard + feed) now both live; coverage_bps + sparkline + §9.2 /perps remain.
  - [x] Forecast Feed ENRICHED → rich scalar-belief board (operator "completely see the
        belief and everything", 2026-06-13; consumes track-C slice-4d/4e live persistence):
        switched to ScalarBeliefsRepo::recent (no ledger change); each belief a click-to-expand
        <details> (the /cognition precedent) surfacing the WHOLE quantile fan + the producer's
        EVIDENCE + provenance, SPLIT from the daemon's {"provenance":…,"evidence":…} wrapper
        (both-keys detection; non-wrapped shown whole, never partially nulled). Untrusted-data
        (5.11): clean_quantiles numbers-only + truncate_evidence + esc'd JSON. Test
        forecast_feed_surfaces_recent_scalar_beliefs_richly (full fan + split-out evidence +
        prov-survives-split + honest-null pending); code-reviewer-clean (||→&& both-keys fix +
        prov assertion applied); full workspace battery GREEN (fmt/clippy/test 159 suites
        0-fail/DST exit-0 incl. daemon_smoke 17); screenshot rota-forecast-feed-rich-2026-06-13.png.

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
- [x] T5.B7 ✅ DONE (rung-0 slices merged; design §5 EXIT; funding_carry data-only by design) — Strategies rung 0: perp_event_basis (Sim), funding_forecast
      (zero-capital scalar claims), funding_carry DATA-ONLY until >=60d regime
      evidence (amendment B). FEE-TRAP RULE (amendment C): edge floors at assumed
      post-promo fees (5-12 bps until fee_tiers is real); Sim gates re-run when
      fees activate; promo-$0 never justifies GO. I7 unchanged.
      SLICE 1 (track C, 2026-06-13, restarted/expanded scope): deterministic
      funding-forecast KERNEL — fortuna_core::perp FundingWindow (TWAP of
      1-minute premiums) + finalize_funding_rate (venue clamp +/-2% + 0.01%
      zero threshold), 13 spec-first tests. The in-ownership deterministic
      core funding_forecast wraps as its scalar claim and perp_event_basis
      uses for the perp point forecast. Fit-validation findings (the
      Strategy/Proposal/runner impedance, the missing perp execution path,
      the prob_claims/v1 scalar dependency, funding_carry data-only) +
      remaining slices ledgered in GAPS "T5.B7 fit-validation".
      SLICE 3+3b (track C): perp_event_basis basis KERNEL done — kernel 70f333a
      (merged 4db8764), refined 5fccd5f to the live 3-strike-type ladder
      (BracketStrike enum, open-tail None, zero money-touch) + real-data e2e on
      the committed paired-cycle fixture (median $63,961.53 / perp $63,906 /
      basis −$55.53). Composite fixture lives in fixtures/perp-basis/ (OUT of
      fixtures/kinetics-perps/ so the venue DTO-coverage tripwire is not tripped;
      operator-directed; see GAPS).
      SLICE 3b-STRATEGY (track C): perp_event_basis STRATEGY DONE — propose-only
      maker-only UNSIZED Cents bracket leg on the bin containing the perp forecast
      (fortuna-runner, additive; holds its own catalog, no venue-DTO change). A
      verification pass caught + fixed a bin_prob bug (one-sided bins were dropped
      to 0, breaking the validated basis); 14 tests + DST oracle. DEMO-ENV
      validated on a fresh live cycle (perp/ladder agree <0.1%, both basis signs).
      SLICE 4 (daemon composition) SCOPED: found that EventPayload::PerpTick has
      NO PRODUCER (only consumers) — registering the strategies alone leaves them
      inert; slice 4 must build the perp ingestion→PerpTick path. Decomposed 4a-4e
      (see GAPS). 4a DONE: KineticsPerpObservation::from_ws_ticker (fortuna-venues
      kinetics, bus-free, verbatim WS-ticker→perp-domain, 4 tests). 4b DONE:
      SimRunner::inject_perp_tick (the ingestion seam; tick() UNTOUCHED → DST
      corpus re-ran green; Sim-soak proves the real funding_forecast fires on an
      injected PerpTick). 4c DONE: registered both perp strategies into
      compose_runner via opt-in [funding_forecast]/[perp_event_basis] sections
      (config-supplied bracket ladder, strictly validated; no veto-enrollment;
      additive 489/0; 11 tests incl. a compose_runner boot test). 4d DONE: the
      scalar egress — drain_pending_scalar_beliefs wired into drive(), persisted
      to scalar_beliefs (gated on a composed scalar producer; synth-independent;
      own 01SCB id space; binary path + tick() byte-unchanged). 4e DONE: the
      Sim-soak PerpTickFeed (replays recorded kinetics ticker frames via
      [funding_forecast].ticker_feed_jsonl, one PerpTick/segment through the 4b
      seam) — funding_forecast now fires + PERSISTS in a soak. PROVEN end-to-end
      by a #[sqlx::test] (recorded PerpTick → scalar_beliefs row, MUTATION-PROVEN);
      5 new tests, all 8 drive() smokes at the 15-arg sig. Inert-producer finding
      CLOSED (for funding_forecast, Sim-soak file feed). Remaining for T5.B7: live
      wt-c daemon-run proof + perp_event_basis live-market catalog (folded into the
      Kalshi demo-flip).
      C-next-1a DONE (post-EXIT, the LIVE-path producer kernel): the basis-v2 arm
      fires only on PerpTick and nothing produced one from LIVE venue data, so it
      stayed inert on the live path. KineticsPerpObservation::from_rest (markets +
      funding-estimate REST → PerpTick, BRTI-reference fail-closed, obs_at ← anchor
      ts_ms) + fortuna-live::perp_tick_producer (host-pinned UNAUTH GET seam,
      poll_perp_ticks_once, fail-closed) mirror the funding poller; 11 tests,
      mutation-proven; full battery green. C-next-1b (the async loop + drive() drain
      + main spawn gated on [perp_event_basis_v2] + an e2e UNSIZED-proposal proof)
      makes the arm non-inert — NEXT slice; the arm stays inert until 1b. See GAPS.
- [x] T5.B8 Ops: kill-switch perps flatten (reduce_only IOC + cancel-all),
      margin/funding telemetry, funding-regime dashboard panel.
      ✅ DONE. The margin/funding telemetry + ROTA panel landed first (box ticked
      2026-06-14). The KILL-SWITCH PERP FLATTEN itself landed 2026-06-14 (track-A):
      `freeze_cancel_perp_and_flatten` (fortuna-killswitch) cancels all open perp
      orders then closes each non-flat position with a REDUCE-ONLY IOC that crosses
      the live book — every close a SEALED `GatedPerpOrder` via the real perp gate
      (`GatePipeline::evaluate_perp`; I1 STRENGTHENED — the switch is on the consumer
      side of the seal, no constructor/visibility/`place` change). Its OWN cred pair
      (`FORTUNA_KILLSWITCH_KINETICS_*`) + gate config (`..._GATE_CONFIG_PATH`,
      fail-closed); a one-shot current-thread runtime; no Postgres/cognition/event
      loop (I4 preserved — only fortuna-gates added, none of the forbidden set).
      New `flatten-perps` verb. Pinned by perp_i4_flatten_seal.rs (seal a/b/c/d +
      dep-graph) + fortuna-killswitch/tests/flatten.rs (long→SELL/short→BUY IOC,
      skip/error/cancel-sweep/gate-reject/fail-closed). Live perps TRADING is the
      separate I7-ladder/operator scope, not T5.B8.

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

## Track D — News-aggregation Phase A (operator-approved 2026-06-12)

Design authority: docs/superpowers/specs/2026-06-12-news-aggregation-design.md
(four-layer trust framework §4.4; Layers 0–2 Phase-A-binding per
docs/design/implementer-loop-track-d.md, which governs this track). Fixtures
first under fixtures/sources/; NO model in the ingestion path; ownership =
crates/fortuna-sources + fixtures/sources/ + one flagged drive() seam.

- [x] D1 Crate scaffold + per-source config types: SourcesConfig/SourceConfig/
      EventWindow/SourceKind parsed fail-closed from [sources.<id>] TOML
      (design §4.3); Phase-A refusals encoded in validation (enabled
      scrape/mcp/model-extraction rejected at parse, not by convention);
      13 unit tests written from the design text, incl. the §4.3 example
      verbatim. (DONE 2026-06-12, 17c95fa after rebase onto main e85f92c;
      full battery green.)
- [x] D2 FetchClient substrate: mockable FetchTransport trait + thin
      ReqwestFetchTransport (auto-redirect OFF so the client re-validates
      every hop); HostPin (https-only + exact-host, userinfo-smuggling
      refused); PoliteLimiter (GCRA, Clock-driven, integer-exact);
      conditional-GET (ETag/If-Modified-Since → 304 NotModified); size cap;
      RateLimited/Timeout/Outage + local BudgetExhausted classification.
      17 tests incl. 2 proptests (bucket never exceeds the refill bound;
      first attempt always granted). (DONE 2026-06-13, full battery green;
      hash recorded next iteration.)
- [x] D3 Layer 0+1 plumbing (re-decomposed — see note): Layer 1
      StructuralValidator (future-dated reject w/ clock-skew tolerance;
      stale-republication FLAG via bounded recent-hash buffer; per-tick
      volume envelope, §7 containment; future/dup never consume volume
      budget) + Layer 0 dossier TEMPLATE/rubric at docs/research/sources/
      TEMPLATE.md (six-dimension score, tier bands, consumption
      consequences). Deterministic + Clock-driven (DST-replayable). 8 tests.
      RE-DECOMPOSITION: the per-source dossiers move to D4–D7 — each adapter
      lands its source's dossier WITH its fixtures, facts grounded in
      research at record time (a source is vetted when it is built, not in
      the abstract). Phase A still ends with Layers 0–2 complete. Ledgered
      in ASSUMPTIONS/GAPS. (DONE 2026-06-13, full battery green; hash next
      iteration.)
      (Each of D4–D7 below ALSO lands its source's Layer-0 dossier from the
      TEMPLATE, grounded in research, with that source's fixtures.)
- [x] D4 NwsSource (impl cognition Source trait): fetches one configured NWS
      endpoint via FetchClient; parses the two REAL response shapes —
      `/alerts/active` FeatureCollection → `nws.alert` signals,
      `/products?type=AFD` `@graph` → `nws.afd` signals; conditional-GET
      validators carried across polls (304 → no signals); error envelopes &
      non-JSON → SignalError, never emitted as signal; `nws_claimed_time`
      extracts the Layer-1 future-check time (alert.sent / afd.issuanceTime).
      Dumb adapter (no dedup/trust/trigger — those are downstream). Fixtures
      under fixtures/sources/nws/ are REAL captures (2026-06-13, README has
      provenance + re-record cmds). Research-grounded dossier at
      docs/research/sources/nws/dossier.md (admitted tier 9). 9 tests.
      (DONE 2026-06-13, full battery green; hash next iteration.)
- [x] D5 RssSource (generic, impl cognition Source trait) via feed-rs:
      ONE adapter, N configured feeds; feed-rs unifies RSS 2.0 / Atom / RDF /
      JSON-Feed -> the same `rss.item` signal shape (format normalization, not
      content interpretation). Conditional-GET across polls; malformed body ->
      SignalError (never a panic). rss_claimed_time = published, fallback
      updated (Layer-1 future check). Dates rendered in the system's fixed-ms
      ISO8601. Fixtures REAL captures (2026-06-13): Fed press RSS 2.0 + SEC
      EDGAR Atom (proves both formats -> one shape) + a malformed doc; README
      has provenance. Research-grounded dossiers: rss_fed_press (tier 9) +
      rss_sec_edgar (tier 9). 8 tests (55 crate total). (DONE 2026-06-13,
      full battery green; hash next iteration.)
- [x] D6 CalendarSource (impl cognition Source trait): BLS macro release
      calendar, two feed modes. Schedule (bls.ics iCalendar) → release_scheduled
      signals {uid, release, scheduled_at (UTC), categories}; the iCalendar
      DTSTART;TZID=US-Eastern naive-local → UTC via chrono-tz America/New_York
      (deterministic, pinned tz db; VTIMEZONE confirms the mapping) — tested
      against a real EST (Jan→15:00Z) AND a real EDT (Jul→14:00Z) event.
      LatestReleases (bls_latest.rss) → release_printed (reuses the feed-rs
      parser via a shared kind-param helper). Fail-closed: unknown TZID /
      floating time / unparseable DTSTART refuse; incomplete VEVENT skipped.
      LOAD-BEARING NUANCE: calendar_claimed_time returns None for
      release_scheduled (its scheduled_at is intentionally FUTURE data, not an
      occurred-at claim — must not trip the Layer-1 future-dated reject);
      release_printed → publish time. Fixtures REAL (2026-06-13). Dossier
      calendar_bls (tier 9). FRED deferred (needs API key → GAPS). 10 tests
      (68 crate total). (DONE 2026-06-13, full battery green; hash next iter.)
- [~] D7 GdeltSource — DEFERRED (fixture-blocked, GAPS). The GDELT DOC API
      (api.gdeltproject.org/api/v2/doc/doc, mode=artlist&format=json) put this
      session's IP into a sustained 429 cooldown after a few probes (it allows
      1 req / 5s); no real fixture could be captured, and the loop rule is
      "missing fixture = stub + GAPS, never invent feed behavior." INTERIM
      COVERAGE: GDELT also serves format=rss, parseable by the existing D5
      RssSource with zero new code (config a GDELT feed). The dedicated
      GdeltSource (richer JSON: domain/sourcecountry/language for Layer-2
      corroboration) lands when a real artlist fixture can be captured (a
      later session / different network, or contact GDELT for a key). Not a
      blocker for Phase A. (Skipped 2026-06-13 per loop fixture rule.)
- [x] D8 Layer 2 corroboration (deterministic near-dup clustering): collapses
      SYNDICATION so it cannot launder a single-source claim into fake consensus
      ("ten outlets carrying one wire story are ONE origin"). corroborate() over
      a signal batch → token-set Jaccard similarity + union-find connected
      components → stable cluster ids + per-signal annotation distinguishing
      `single-source (tier t)` from `syndicated: same content from N sources
      [...] — treat as ONE origin`. Annotation computed deterministically, never
      self-reported by the model (spec 5.11). Same-source-repeat correctly NOT
      syndication; empty text never clusters on emptiness; output is replay-
      stable (test asserts identical output on re-run). BOUNDARY (honest): the
      deterministic half is text near-dup; "N independent origins corroborating
      EVENT X" (differently-worded items about one event) is semantic — the
      model / world-forward composes these clusters with its event grouping
      (noted in module). Algorithm = Jaccard now (simple, transparent, O(n²)
      bounded by the per-tick volume envelope); simhash/MinHash is the
      documented refinement for larger batches, interface stable across it. Pure
      logic, no network/fixtures. 7 tests (75 crate total). (DONE 2026-06-13,
      full battery green; hash next iter.)
- [x] D9 Ingestion scheduler (THE HARD GATE — re-gate required the validator
      live on the ingest path). IngestionScheduler::tick(now)->TickOutcome is
      the deterministic core (SimClock-driven, never sleeps; the async run-loop
      is the D10 seam). WIRES THE StructuralValidator on every fetched item:
      future-dated/republished/over-volume are REFUSED-and-recorded
      (DropReason), never passed downstream — HARD GATE SATISFIED, tested
      adversarially. Per-source cadence (daily time-windows boost the interval;
      day-set restriction is the Phase-B refinement) + health state machine
      Healthy→Degraded(n)→Quarantined (LOUD alert, deterministic exponential
      backoff, no auto-resume — operator rearm() only, I2 spirit) + per-source
      isolation (one fault never aborts the fleet) + trigger-floor TAG
      (wakes_decision_cycle = tier>=trigger_floor; below-floor still lands for
      slow discovery; resolution floor stays cognition-side). FIRST-CLASS
      per-source SourceMetrics telemetry (operator request): polls, empty_polls
      (304 proxy), fetch_errors, accepted, drops-by-reason, quarantines, rearms.
      content_hash via sha2 (bounded republication flag; ledger UNIQUE is
      authoritative). 10 unit tests + 5 ENUMERATED DST scenarios (timeout, 429
      storm, crash+rebuild, burst/volume-cap, quarantine+rearm) in
      tests/ingest_dst.rs, wired into scripts/run-dst.sh. Adds sha2 dep. 89
      crate tests + 5 ingest_dst. (DONE 2026-06-13, full battery green; hash
      next iter.)
- [x] D10 drive() seam: the scheduler wired LIVE into fortuna-live behind the
      [ingestion] flag (default OFF). Closes the hard gate — the Layer-1
      validator now runs on the LIVE ingest path in the daemon.
      Design choice: rather than mutate drive()'s huge shared signature (breaks
      every caller, high collision risk), ingestion runs as an INDEPENDENT loop
      spawned by main.rs alongside drive() — it is its own IO loop, not part of
      the deterministic trading cycle. Pieces:
      - fortuna-sources factory build_scheduler (D10 1/2, 30ae38f).
      - fortuna-live ingestion.rs: IngestionCore (scheduler tick -> validate ->
        normalize_and_dedup -> SignalEnvelopes; TESTABLE, no DB) + IngestionWiring
        (adds SignalsRepo persistence + Slack) + run_ingestion_loop (Clock-read,
        real-time sleep at the IO edge, stop-signal) + build_ingestion_wiring
        (loads tiers from source_registry, parses [sources], factory).
      - boot.rs [ingestion] section (enabled/tick_ms/trigger_floor/
        volume_envelope/user_agent), deny_unknown_fields, default off.
      - main.rs: spawn the loop when enabled; stop it with the daemon.
      HARD GATE PROVEN END-TO-END: ingestion test
      validator_is_live_a_future_item_never_becomes_an_envelope — a future-dated
      item is refused by the validator and NEVER reaches the signals store.
      REGRESSION: daemon_smoke 15/15 green (daemon byte-unchanged when off).
      OPERATOR PREREQ to enable (ledgered GAPS): seed source_registry rows
      (tiers, per the Layer-0 dossiers) + [sources.*] config + [ingestion]
      enabled = true. DEFERRED: ingestion-alert Slack routing (quarantines
      counted/logged now); wakes_decision_cycle tag not persisted (trigger-
      engine wiring is cognition-side). 3 ingestion tests (92 sources-crate +
      daemon_smoke 15). (DONE 2026-06-13, full battery green.)

### Track D — Aeolus integration (F-series; operator-approved 2026-06-13)

Authority: docs/design/aeolus-fortuna-source-contract.md (rev 3, reconciled with
the Aeolus producer handoff olympus/aeolus/docs/fortuna-integration-handoff.md).
The handoff's critical path: F2 (NWS observations grader) is the LONG POLE — it
blocks ALL Aeolus weather beliefs (§5.12 forbids unscoreable beliefs) and is
independently the grader for any weather belief; F1 (auth) blocks F3. PRIORITY
after the SSRF re-gate clears: F2 + F1 first (ahead of D6/D7), since they close
a full end-to-end loop and reuse the substrate already built.

OWNERSHIP: F1–F4 + F10(registry/dossier/fixture) are crates/fortuna-sources
(MINE). F5–F9 are cognition (NOT Track D — fortuna-cognition owner; F4's
scheduler is shared with D9). The skill/persona layer is a separate session
(docs/design/PROMPT-domain-analysis-skills.md).

- [x] F2 NWS observed-daily-extreme grader — NwsClimateSource (the long pole;
      the official resolution record). Ingests the NWS CLI (Climatological
      Report — Daily) products, which carry the OFFICIAL daily max/min — the
      same record the market (Kalshi) and Aeolus resolve against (a max-of-
      hourly-obs would be DERIVED and would NOT match). Chose CLI over
      /stations/{id}/observations for exactly that reason. TWO-HOP: the
      `/products?type=CLI` list (no text) -> per-product text fetch (with the
      report), bounded by a per-tick cap + a FIFO seen-set of product ids;
      conditional GET on the list (per-product texts immutable). Emits `nws.cli`
      signals carrying the RAW productText (authoritative) + a robustly-parsed
      report_date for indexing. DELIBERATELY DUMB about the temperatures: the
      CLI text is fragile (`MINIMUM 7676` = observed 76 + record 76 jammed), so
      the high-stakes max/min EXTRACTION is DEFERRED to the grader (cognition,
      at settlement) where ambiguity is flagged, not silently mis-graded.
      claimed_time = issuanceTime (past; report issues the morning after).
      Fixtures REAL (2026-06-13: cli_list + cli_product). Research-grounded
      dossier docs/research/sources/nws_climate/ (admitted tier 10 — the
      settlement record). 6 tests. (DONE 2026-06-13, battery green; hash next.)
- [x] F2-grader NWS-CLI productText → °F realized-extreme parser (the long-pole
      extraction F2 deferred; §3.2/§5.12 — F9's resolution input). New
      `nws_cli_realized(product_text, station) -> Option<RealizedExtreme>` in
      nws_climate.rs (source-side: deps run sources→cognition, so the grader can't
      sit in cognition; F9 takes a plain `realized_f: f64` and the composition
      layer bridges `high_f as f64` (TMAX) / `low_f as f64` (TMIN)). FAIL-LOUD —
      `None` on any ambiguity: a jammed column (`MINIMUM 7676`), a missing value
      (`MM`), an absent MAXIMUM/MINIMUM line, an inverted high<low, or an
      unparseable date — never a fabricated temperature. Robust to the real-data
      quirks captured: the daily line is the first `<keyword> <number>` (skips the
      monthly `MAXIMUM TEMPERATURE (F)` rows), record-tie flags (`91R`→91), and
      both date orders/abbreviations (`12 JUNE 2026` / `JUNE 13 2026` / `JUN 13`).
      RECORDED fixtures only: 2 NEW real captures (Troutdale KPQR 91/50, Pago Pago
      NSTU 82/75) for the success path + the existing PTKR (jammed min) for the
      hard-error path; mutation guard (drop the MAXIMUM line → None). 10 grader
      tests (141 sources lib total). Registry: the observed-daily-extreme feed is
      the resolution source (dossier updated; the source_registry seed is the
      operator prereq, ledgered in GAPS). (DONE 2026-06-14 a615cc3; battery green:
      fmt + clippy --workspace --all-targets -D warnings + check --workspace +
      cargo test --workspace 167 ok/0 failed + run-dst.sh 200.)
- [x] F1 Generic per-source auth header in FetchClient (subagent-built, I
      reviewed + verified). ReqwestFetchTransport.with_auth_header(name, secret):
      Aeolus = `x-api-key`, generic by name (Bearer drops in). The value is
      HeaderValue::set_sensitive(true) (the http crate prints "Sensitive", never
      logs it); manual Debug elides values as `<redacted>`; a malformed value's
      error reports only the (non-secret) name. SECRET resolved by the caller via
      a `secret_resolver: impl Fn(&str)->Option<String>` on build_scheduler — the
      LIB never reads env (the daemon does: `|n| std::env::var(n).ok()`).
      Fail-closed: a named auth_env that doesn't resolve is a hard error (no
      silent unauthenticated fetch); half-configured auth rejected. config gains
      auth_header + auth_env (env-var NAME, never the secret). SSRF host-pin code
      UNTOUCHED (pin_ tests 6/6 unchanged). Redaction tests
      (is_sensitive, Debug-redacts). (DONE 2026-06-13, battery green.)
- [x] F3 AeolusSource adapter (subagent-built, I reviewed + verified). Dumb
      wrapper over FetchClient (host-pin, conditional GET, politeness, F1 auth):
      splits `{"forecasts":[envelope]}` -> one RawSignal {kind aeolus.forecast,
      payload = raw envelope UNTOUCHED, received_at = clock.now()} per envelope;
      304 -> empty; non-JSON / missing forecasts[] -> SignalError (never panic,
      never silently emitted). NO strict-parse/validate/dedup (downstream, F6).
      aeolus_claimed_time = run_at (the forecast init_time, past). SourceKind
      ::Aeolus + factory arm. Fixtures REAL — captured from the LIVE Aeolus
      endpoint 2026-06-13 (fixtures/sources/aeolus/knyc_tmax+tmin.json; the
      response matched contract rev-3 EXACTLY incl. crpss_vs_raw=null,
      n_scored=30). Research-grounded dossier (tier 7 — sober: μ commodity,
      market edge unproven, Layer-3-earned; resolution-elig 2, NOT the grader).
      13 tests. (DONE 2026-06-13, battery green.)
- [x] F4 scheduler integration — wire the adapters into the factory so they are
      scheduler-validated + reachable. Added the (Nws, "climate") factory arm ->
      NwsClimateSource (F2) + nws_climate_claimed_time (the grader was built+
      exported but UNWIRED — the F2-gate residual). Aeolus's arm was already
      wired in F3. Every factory-built source goes through scheduler.register,
      which attaches the StructuralValidator (Layer-1) with NO bypass (D9 hard
      gate) — so "registered via the factory" == "Layer-1 validated on the ingest
      path". Test wires_the_climate_grader_and_aeolus asserts both register
      (mutation-proof: delete the climate arm -> feed="climate" reroutes to the
      (Nws,_) Err arm -> build_scheduler().unwrap() panics -> red). 113 sources
      tests (112 lib + 1) + 5 ingest_dst. (DONE 2026-06-13 6495058; SCOPED battery
      green — fmt --check + clippy -p fortuna-sources --all-targets -D warnings +
      test -p fortuna-sources + check -p fortuna-live consumer; full-workspace
      battery is the verifier's merge gate, see GAPS "TRACK D — F4".)
- [x] F4b ✅ DONE (track-E @0e20681; deferral lifted) — release-aware cadence: consume next_run_at + the GEFS release pattern
      to tighten the poll cadence around forecast issuance. DEFERRED to Phase B —
      a scheduling refinement, NOT the gate's reachability/validation ask (which
      F4 closes). Today the Aeolus source polls on its configured base_interval.
- [x] F10 registry row + Layer-0 dossier (docs/research/sources/aeolus/, stating
      MEASURED reality not an unproven edge, per contract §1/§5) + v1→v2 fixture
      migration (keep v1 behind "schema absent ⇒ v1"; aeolus_eval T2.7 stays
      green; do NOT weaken it).
      ✅ DONE (track-e-f10-e5): v1↔v2 schema dispatch `aeolus_forecast::parse_versioned`
      (absent⇒v1 reconciliation::AeolusEnvelope, kept green/unweakened; v2⇒strict F6 parse;
      else UnknownSchema) + 3 tests; dossier pre-existing/complete (tier-7 sober, measured
      reality); source_registry ROW = ledgered operator seed (values in GAPS).
- [x] ✅ DONE (track-E Aeolus F5–F9 merged @bdea003) — (cognition; OWNER = TRACK E as of 2026-06-14, reassigned from C — operator-
      directed; E owns the weather domain. New disjoint fortuna-cognition modules;
      reuse C's prob_claims/v1 + scalar_beliefs; do not touch C's perp/discovery files.)
      F5 identity-tuple dedup, F6 strict v2 parser + pinned-erf μ/σ→p, F7 world-forward
      match, F8 belief→calibration→gates→sizing, F9 Layer-3 empirical scoring.
- [x] ✅ DONE (track-E weather scoring bridge — CLOSE THE LOOP) — `resolve_and_score_weather_beliefs`
      (fortuna-live daemon.rs, standalone, mirrors the funding resolver; NOT yet wired into
      `drive()` — that one-line call is Track A's). Routes each DUE open Aeolus belief to its NWS
      CLI product by grading station (`aeolus_resolve::cli_serves_station`), grades via the Track-D
      `nws_cli_realized`, and resolves binary brackets (Brier of the PERSISTED p) + the scalar μ/σ
      belief (CRPS) vs the realized °F; None ⇒ OPEN, never fabricated. New: `aeolus_resolve.rs`
      (pure), F8 stamps `nws_station_id`, `BeliefsRepo::open_aeolus_weather_due`. Verified 8 unit +
      2 ledger + 4 live + upgraded e2e (real grader); full battery + DST green. Seams (NYC CLI
      fixture, multi-station, CLV, drive() wiring) ledgered in GAPS.
- [x] ✅ DONE (track-A) — the `drive()` daily-boundary WIRING that fires the above. On the UTC-day
      boundary (alongside the digest + reconciliation), `drive()` now calls
      `resolve_and_score_weather_beliefs` AND `resolve_and_score_funding_beliefs` via a new opt-in
      `resolution_pool: Option<PgPool>` (Some in main; None in smokes ⇒ byte-identical). Disjoint
      `01BSC…` score-id bases (distinct 2^56 high tags on the UTC-day epoch base). Alert-and-continue
      on failure (never crashes the boundary); idempotent (set-once resolve + score dedup). e2e
      `drive_resolves_due_weather_and_funding_beliefs_on_the_daily_boundary`: one tick resolves the
      recorded weather brackets+scalar AND a due funding belief (disjoint bases, no PK collision);
      second tick is a no-op. Full battery green (fmt + clippy + test --workspace + run-dst 200). The
      weather calibration loop is CLOSED end-to-end.

### Track D — ingestion observability (data surface; contract: docs/design/ingestion-observability-contract.md §2)

- [x] OBS-1 telemetry data surface (slice 1, fortuna-sources). The scheduler now
      projects a live `IngestionTelemetry` snapshot (§2): per-source
      `SourceTelemetry` (health, trust_tier, last_poll/last_success/next_due ISO
      timestamps, the D9 counters, redacted `last_error`, last-seen `kind`), a
      process-wide `FunnelCounts` (validate stages summed from per-source metrics;
      loop stages stay 0), and a bounded (256) newest-first `recent` feed of
      `SignalRecord`s — the operator's "signals coming in + their data" (V1). Each
      record carries a REDACTED `summary`: a 7-key allowlist projection truncated
      to 120 chars (untrusted payloads are never dumped; spec 5.11), with
      `redact_error` capping errors to 200. `telemetry(generated_at)` is a pure
      projection — Clock injected, no wall-time, no unwrap/panic. New pub types
      exported from lib.rs for track-B. 6 mutation-structured tests (truncation
      ==120 on a 5k input, ring bound ==256, last_error set→cleared, funnel
      1/2/3, accept+future-drop feed). Subagent-built, I reviewed + verified.
      (DONE 2026-06-13 07ae945; SCOPED battery green — fmt + clippy -p
      fortuna-sources --all-targets -D warnings + test -p fortuna-sources = 118
      lib + 5 ingest_dst; full-workspace battery is the verifier's merge gate,
      see GAPS "TRACK D — OBS-1".)
- [x] OBS-2 ✅ DONE (track-A) — funnel loop-stages + snapshot wiring (fortuna-live): the ingestion
      loop sets `normalized`/`deduped`/`persisted`/`persist_failures` on the funnel (OBS-2a) and
      `run_ingestion_loop` PUBLISHES the snapshot behind `Arc<RwLock<IngestionTelemetry>>` once per
      pass (OBS-2b @b5be944), read by the metrics renderer + ROTA (OBS-2c) (§2 "one writer, many
      readers"). CLOSED with the POPULATED-PATH test `ingestion_populated.rs` (`#[sqlx::test]`): the
      REAL `run_ingestion_loop` drives 2 scripted signals through normalize→PERSIST against a real
      signals store so `persisted`/`normalized` move off 0, a CONCURRENT reader sees the published
      snapshot live while the loop runs, and the signals really land in the append-only store. Full
      battery green; READ-ONLY observability (no money path).
- [x] OBS-3 ✅ DONE (track-B Sources Health @072f9a1) — domain_tags population: carry each source's domain (weather|macro|…)
      from the source_registry/config admission into `SourceTelemetry.domain_tags`
      (empty in slice 1). Needs a config/registry field — fold with F10.

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
- [x] E.3 ✅ DONE (E.3a–c + telemetry merged; the §15 PersonaOutcome pin landed as i6_persona_propose_only.rs, merged + passing) — Runner loop + triggers + budget + context + findings contract — scripted-StubMind
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
      green. E.3 COMPLETE: the §15 PersonaOutcome invariant pin landed as
      `i6_persona_propose_only.rs` (operator-approved waive, merged + passing in the invariants suite).]
- [x] E.4 Belief consumption — DomainAnalysis section + evidence/provenance citation; the
      μ/σ→p helper in code; `fortuna_persona_beliefs_total` metric.
      [E.4a (commit c1c1b55): fortuna_cognition::persona_beliefs — normal_cdf/prob_at_least (the
      μ/σ→p backbone, clamped) + map_persona_analysis (artifact findings → one binary BeliefDraft
      per threshold/outcome, evidence + provenance replay anchor, dedup'd event_ids).
      E.4b DONE this commit: SectionKind::DomainAnalysis (shared enum, additive, high priority) +
      domain_analysis_context_item (the artifact as a high-priority DATA context item for the
      synthesis Mind); shared-enum safety verified; 3 tests; full battery green. The
      fortuna_persona_beliefs_total metric folds into the §19 PersonaCounters at the live wiring.]
- [x] E.5 Scoring scope extension — ScopeKey + weekly-review promote/retire proposal (baseline +
      market comparison; recommendation-only); resolved_beliefs/clv_bp metrics.
      [E.5a DONE: fortuna_cognition::persona_scoring — PersonaScope + score_persona
      (Brier/quality/CLV via the existing calibration primitives) + propose_promotion (the §11
      beat-both-baselines gate: Evaluating/Promotable/RetireCandidate; recommendation-only, I7).
      E.5-REMAINDER DONE (track-e-f10-e5): the weekly-review FOLDING entry point
      `persona_scoring::weekly_persona_proposals(&[PersonaReviewInput], min_resolved) ->
      Vec<PersonaPromotionProposal>` — one call folds every registered (persona, version); +2 tests;
      handoff §8 updated to call it. Per Fit-validation §21 this is the ADDITIVE PARALLEL realization
      (scores by PersonaScope alongside the synthesis review::ScopeKey) — it does NOT edit the shared
      `ScopeKey` struct, whose literal is in Track-A's daemon composition (extending its fields would
      break daemon.rs:1024, which the loop forbids touching unilaterally). REMAINING = Track-A daemon
      coordination ONLY: call `weekly_persona_proposals` in drive()'s weekly review + route to
      #fortuna-review (the §8 handoff); + the resolved_beliefs/clv_bp metric labels fold into §19 at wiring.]
- [x] E.6 End-to-end meteorologist proof over Aeolus (+ NWS/fixture) + the macro mechanism test;
      the §11 evaluation gate wired; full battery green.
      (DONE this commit: crates/fortuna-ledger/tests/persona_e2e.rs — one #[sqlx::test] wires the
      WHOLE pipeline on the real DB [register→hash-bound load→run (scripted StubMind)→persist
      domain_analyses→fan-out 3 binary beliefs→persist→resolve→score_persona+propose_promotion],
      asserting belief-replays-to-artifact [analysis_id + content_hash anchor], the §11 zero-capital
      gate, and persist-path firewall. Boundary-clean [Track-E repos + cognition only, no daemon].
      Macro mechanism covered by E.4a's macro fan-out test; the live daemon wiring + the §11 gate
      WIRED INTO drive() is a Track-A coordination [GAPS]. The §12 spike de-risked the live-model
      shape. Full battery green; feature-dev review applied. CORE PIPELINE PROVEN END-TO-END.]

ROTA views (§14/§20) + persona telemetry (§19) are operator-requested detailed contracts
(2026-06-13) — Track B builds the four views; Track E provides the data across E.1–E.5.
- [x] OBS-2a funnel loop-stages (fortuna-live ingestion.rs). IngestionCore now
      accumulates `normalized` (items that became SignalEnvelopes) + `deduped`
      (authoritative-dedup drops); IngestionWiring accumulates `persisted` +
      `persist_failures`. Both expose `telemetry(now) -> IngestionTelemetry` (the
      core fills the normalize stages, the wiring overlays persistence) — so the
      funnel is complete end-to-end (it read 0 for these in OBS-1). Contained to
      ingestion.rs (no main.rs/boot.rs → zero track-A collision). 3 core tests
      (no DB): funnel projection, normalized accumulation across ticks, and that a
      cross-tick exact duplicate is caught by the validator's persistent
      recent-set (counts as `validated_dropped`, NOT `deduped` — `deduped` only
      fires for dups past the validator's window). (DONE 2026-06-13 <hash>; scoped
      battery green — fmt + clippy -p fortuna-live --all-targets -D warnings +
      test -p fortuna-live --lib ingestion = 6/6; the DB-backed fortuna-live
      suite is the verifier's merge gate.) [2ed28d3]
- [x] OBS-2b snapshot publish: the "one writer" side of §2. New
      `IngestionTelemetryHandle = Arc<RwLock<IngestionTelemetry>>` +
      `new_telemetry_handle()`; `run_ingestion_loop` publishes `wiring.telemetry(now)`
      into it each tick (the projection uses the same `now` as the tick).
      `IngestionTelemetry` derives `Default` (the empty pre-first-tick snapshot
      readers see). main.rs creates the handle (inert empty Arc when ingestion is
      OFF → daemon byte-unchanged), passes the writer to the loop, and logs the
      final funnel at shutdown. NO RotaState touch (track B owns the read endpoint
      = OBS-2c). 1 DB-free test (handle starts empty, then round-trips a published
      snapshot). (DONE 2026-06-13 <hash>; scoped battery green — fmt + clippy
      -p fortuna-sources -p fortuna-live --all-targets -D warnings + test
      -p fortuna-sources 119+5 + test -p fortuna-live --lib ingestion 7/7;
      daemon_smoke unaffected = verifier's merge gate.) [7a9e28d]
- [x] OBS-2c ROTA read wiring (track-B-coordinated) — ✅ DONE: the deferred read-wiring landed under T4.6 (the V1/V2/V3 ingestion boards render LIVE daemon data). Ticked 2026-06-14 (verifier de-stale). Original spec: track B adds an
      `ingestion: Option<IngestionTelemetryHandle>` reader to `RotaState`
      (fortuna-ops) + the V1/V2/V3 boards project it; main.rs passes
      `ingest_telemetry.clone()` into the dashboard state. Deferred to land WITH
      track B's ROTA harness (the bus queue item) so writer+reader stay coherent
      and we don't both edit RotaState.
- [x] OBS-3 domain_tags population: carry each source's domain (weather|macro|…)
      from the `source_registry` admission into `SourceTelemetry.domain_tags` (was
      hard-coded empty in OBS-1). Registry-sourced via a new `domain_of` resolver
      on `build_scheduler` (parallel to `tier_of`) → `SourceSchedule.domain_tags`
      → the telemetry projection; build_ingestion_wiring builds the domain map
      from the same `source_registry` rows it loads tiers from. No drift (the
      Layer-0 admission is the source of truth, not config). New test
      `domain_of_flows_through_to_telemetry`. SourceTelemetry now has no empty
      placeholder fields. (DONE 2026-06-13 <hash>; scoped battery green — fmt +
      clippy -p fortuna-sources -p fortuna-live --all-targets -D warnings +
      test -p fortuna-sources = 119 lib + 5 DST + test -p fortuna-live --lib
      ingestion 6/6. Subagent-built, main-loop reviewed + verified.) [06b247c]

## System-level invariant audit (operator-run 2026-06-14) — fixes

- [x] C2 / I4 "revoke order-placing capability" (spec.md:43): the killswitch's
      freeze + flatten verbs now WRITE a durable kill sentinel (`KILLSWITCH_REVOKED`,
      sibling of `--journal`; std::fs only — I4 independence intact) and fortuna-live's
      `RevocationHaltPoller` (over `PgHaltPoller`) reports its presence as a Global halt
      before any tick, so a kill revokes FUTURE placement and a daemon "boots revoked".
      Re-arm is CLI-only + restart-gated (`fortuna-killswitch clear-revocation`; spec
      Section 8 + I2). NEW invariant test `i4_killswitch_revocation.rs` (additions-only;
      `i4_killswitch_independence.rs` UNTOUCHED) + behavioral `revocation_poller.rs` +
      boot-parse tests for `[killswitch].revocation_file`. fortuna-gates byte-unchanged.
      GAPS item 1 → RESOLVED; ASSUMPTIONS updated (incl. operator path-consistency + the
      venue-API-key revocation future-hardening note). (DONE 2026-06-14 <hash>.)
