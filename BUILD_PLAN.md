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
- [ ] T1.3 `mech_extremes` + model-veto scaffolding (veto reduce-only, counterfactual
      scoring records; stub mind acceptable this phase). (6)
- [ ] T1.4 Settlement lifecycle processors + watchdogs (overdue, dispute, divergence,
      stranded-state) + discrepancy records. (5.13)
- [ ] T1.5 Metrics (OpenTelemetry), minimal read-only dashboard, daily digest,
      accounting export. (8)

EXIT: both mechanical strategies in Paper against recorded data streams; settlement and
discrepancy paths exercised in DST; dashboards and digest render from sim data.

## Phase 2 — Belief pipeline (Section 12)

- [ ] T2.1 Events + edges + snapshot scheduler (benchmark_at; T-24h/1h/5m + on-trade);
      CLV scoring job with liquidity filter. (5.12, 5.5)
- [ ] T2.2 Signals: `Source` trait, normalizer/envelope/dedup, source registry with
      trust tiers, trigger engine (rules + debounce + per-event serialization). (5.11, 5.8)
- [ ] T2.3 Belief ledger ops + freshness policy + scoring (Brier, calibration curves
      per model/strategy/category). (5.5, 5.10)
- [ ] T2.4 Context assembler with manifests + replayability (snapshotted computed
      views); anonymization mode. (5.7)
- [ ] T2.5 `Mind` trait: StubMind (deterministic, for DST) + AnthropicMind (structured
      output via tool schema, cost tracking, budgets, schema-invalid handling). (5.9)
- [ ] T2.6 Decision cycle + comparator + Kelly sizing with calibration haircut;
      triage tier + triage scoring (declined-trigger shadow sampling). (5.8, 5.9, 5.14)
- [ ] T2.7 Daily reconciliation loop (journal, exits evaluation), aeolus_eval ingestion
      against the sample fixture. (5.8, 6)
- [ ] T2.8 Calibration layer (Platt/isotonic, shrinkage prior, versioned params). (5.10)

EXIT: full decision loop in Sim with StubMind under DST (including cognition failure
scenarios); AnthropicMind exercised behind a feature flag with budget controls;
aeolus_eval writes scored beliefs from fixture envelopes; I5-I7 invariant tests green.

## Phase 3 — Closing the loop (Section 12)

- [ ] T3.1 Weekly/monthly review jobs (calibration report, lesson promotion flow,
      envelope allocation report). (5.8)
- [ ] T3.2 Market-back discovery loop + edge confirmation cards; world-forward
      watchlist loop with cost cap + unscoreable rule. (5.12)
- [ ] T3.3 Shadow-mode model comparison harness (paired contexts, separate budget). (11)
- [ ] T3.4 Polymarket US adapter (fixtures-gated; stub + GAPS entry if fixtures absent).
- [ ] T3.5 `synth_events` strategy in Paper. (6)
- [ ] T3.6 FINAL_REPORT.md + operator go-live runbook + full acceptance checklist run.

EXIT: PROMPT.md acceptance checklist fully green or operator-blocked-only.
