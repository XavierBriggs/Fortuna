# FORTUNA — Final Report

Build completed 2026-06-10 against docs/spec.md v0.8. This report covers what was
built, every deviation from spec with rationale, verification statistics, and the
operator's go-live runbook. The honesty ledgers are GAPS.md (operator-blocked
items only, each with exact unblock steps) and ASSUMPTIONS.md (every decision
made where the spec is silent).

## 1. What was built

Thirteen crates, ~40,700 lines of Rust, 58 commits, every BUILD_PLAN task ticked
with its commit hash.

**The deterministic core (Phase 0).** Integer-cents money (`Cents` newtype,
checked arithmetic, fee rounding always against us), injected `Clock`, ULID ids,
a single-threaded deterministic event bus with byte-exact replay recording. The
ten-check gate pipeline (spec 5.3) producing the sealed `GatedOrder` — the only
type any venue accepts (type-level I1). Order manager over an append-only intent
journal with crash recovery (idempotent client-order ids derived from intent
ids). Position book, conservative marks, reservation ledger rebuilt at boot as
derived state (5.14). Sim venue with seeded fault injection (ack delay,
dropped/duplicated fills, place/cancel timeouts, API errors). The DST harness:
seeded scenario generator + regression corpus runner (`scripts/run-dst.sh`),
replayable via `scripts/replay.sh --seed`.

**The execution plane (Phase 1).** Kalshi adapter built against doc-derived
samples and a dated research loop (docs/research/venue/kalshi-api-2026-06-10);
cleared for Sim only pending operator fixture recording. Paper engine with the
honest fill rules (maker fills ONLY on trade-through with quantity haircut —
there is a test that fails if anyone ever fills at touch; takers cross displayed
depth, never mid). mech_structural (bracket-sum arbitrage) and mech_extremes
(maker-only favorite fade, sub-$100k markets) through the full composed Sim
loop. The model VETO on mech_extremes: reduce-only BY TYPE, after sizing before
gates, fail-closed-flagged-unscored on provider error, counterfactually scored
at settlement, re-scored on corrections, abandoned on voids. Settlement
lifecycle (5.13): at-least-once notice stream with dedup, superseding-insert
entry chains (pending -> posted -> confirmed | reversed), exact position
reversal on corrections, watchdogs (overdue, dispute freeze, 3-tick
books-vs-venue mismatch -> discrepancy + global halt, settlement divergence,
orphaned positions). Observability: deterministic metrics registry with
Prometheus text exposition, GET-only dashboard, pure digest, write-once
accounting CSVs, dead-man pinger, Slack send-side router (every message also an
audit row).

**The cognition layer (Phase 2).** Canonical events with a legal-or-error
lifecycle; market-event edges with confidence tiers and superseding-insert
confirmations; deterministic edge checks (resolution-source mismatch scores 0.0
— the UMA failure mode). CLV benchmark snapshots (T-24h/1h/5m, never
post-benchmark) and integer-exact CLV. Signals funnel: content-hash dedup,
fail-closed source-registry allowlist, trust tiers; trigger engine with
per-event serialization and debounce. Beliefs strict in (0,1), score-once
enforcement, freshness policy (stale beliefs never reach the comparator).
Char-budgeted context assembler with manifest hashes and fail-closed item
verification (every decision reconstructable). The Mind trait: StubMind
(deterministic, DST) and AnthropicMind (raw HTTP — no official Rust SDK,
sanctioned; structured output via json_schema; schema-invalid rejected, never
repaired; harness-stamped provenance; pre-call cost budgets with 00:00 UTC
roll). I6 is enforced three ways: deny-unknown-fields across the whole mind
output surface (smuggled sizing/order fields reject the WHOLE output), a
field-set pin in the invariant tests, and a dependency-direction assertion
(fortuna-cognition cannot name venue/exec/state/runner types). Comparator
(Direct + Negation only, two-sided, floor-fair), Kelly haircut by calibration
quality (fail-closed), declined-trigger shadow sampling, daily reconciliation
(journal-or-error, structurally zero orders), aeolus_eval ingestion
(zero-capital by type), calibration layer (deterministic Platt + isotonic PAV,
shrinkage-toward-market below n=50, extremization, versioned params).

**The closing loop (Phase 3).** Weekly review (calibration audit with versioned
refits, Section 11 GO/NO-GO recommendations with reasons — recommendations
only, I7), lesson candidates parsed strictly from the journal body (operator
approve/reject promotes; monthly decay demotes via superseding insert). Monthly
review (conservative allocation recommendations that never invent capital,
cost-of-cognition audit, operator checklist). Discovery: deterministic
market-back prefilter with counted exclusions, tradability scores persisted
append-only, strict-JSON normalization (match-before-create, hallucinated
matches dropped), edge confirmation cards (model confidence + deterministic
score + high-stakes flag), world-forward watchlist with the unscoreable rule
(no beliefs nobody can grade) and a hard daily cost cap that throttles BEFORE
spending. Shadow model comparison (manifest-hash paired contexts, own budget,
first-K/day sampling) and the I7 swap gate (no record, no promotion).
synth_events as a named Paper-capped synthesis strategy; promotion records
machinery (`effective_stage`: one contiguous human-actor step at a time,
demotion automatic, declared stage is a cap). Polymarket US: fixtures-gated
stub that refuses every operation.

**The invariants crate (protected).** All seven invariants implemented as
executable tests, ZERO ignored. I1 universal gate (+ property test), I2
drawdown human re-arm (+ property test), I3 runaway halt, I4 kill-switch
independence (no Postgres/ledger/cognition deps; runs with the runtime dead),
I5 append-only audit (DB triggers + replay + no-audit-no-trading), I6
propose-only mind (schema rejection + surface pins + dependency direction), I7
promotion gates (stage refusal + ladder + operator-record derivation +
shadow-comparison swap gate).

## 2. Deviations from spec, with rationale

Everything below is also recorded in ASSUMPTIONS.md (decisions) or GAPS.md
(operator-blocked); this is the consolidated list of actual deviations.

1. **Spec 5.2 fee figures are stale and were not followed.** The spec's
   "Polymarket Intl mostly zero / US flat 10bp taker" describes superseded
   regimes. The fee engine implements the researched current reality as
   versioned config (quadratic/flat/tiered, maker/taker, category multipliers,
   effective dates). Spec v0.9 touch-up is an operator action (GAPS).
2. **Sub-cent price structures are excluded** (deci-cent Kalshi structures,
   future Polymarket 0.0001 ticks). Core money is integer cents; the adapter
   filters such markets from the catalog rather than approximating prices.
   Revisit needs a price-tick type and a commercial reason.
3. **Kalshi pair auto-netting is not modeled.** Sim and paper hold YES+NO lots
   to settlement — value-identical to venue netting, conservative on capital
   (no early $1/pair credit). Verification against recorded fixtures is part
   of the fixture checklist.
4. **The comparator handles Direct and Negation mappings only** (v1).
   Bracket-component and conditional-on edges are skipped, never mispriced —
   composite semantics need explicit treatment.
5. **MindOutput's surface was frozen and lessons/normalizations ride in the
   journal body as strict JSON.** The spec's 5.9 output shape (beliefs,
   proposals, journal) is pinned by the I6 invariant test; weekly-review lesson
   candidates and discovery normalizations are parsed from the journal body
   with strict serde and degrade to nothing on free prose. Growing the I6
   surface would have been the convenient alternative; it was rejected.
6. **The runner's v0 execution ignores the urgency hint** ("arbs go taker via
   crossing limits"); passive quoting inside the limit is exercised at the
   paper boundary and an execution-policy upgrade is future work behind the
   recorded-stream replay (fixtures).
7. **`SystemTime::now()` exists nowhere outside Clock impls; wall-clock
   DST master seeds** are drawn through `RealClock` in the harnesses only,
   printed for reproduction — randomized discovery by design, deterministic
   replay per seed.
8. **The aeolus_eval contract sample is FORTUNA-defined.** The spec's "sample
   Aeolus envelope fixture" is satisfied by the contract-defining fixture
   (fixtures/aeolus/sample_envelope.json) proven end-to-end; the
   operator-recorded export validates Aeolus's exporter and remains open in
   GAPS (a prod read was correctly denied by the permission classifier).

## 3. Verification statistics

- **Workspace test suite:** 615 tests, 0 failures (`cargo test --workspace`,
  final acceptance run 2026-06-10). `cargo fmt --check` clean;
  `cargo clippy --workspace --all-targets -- -D warnings` clean.
- **Invariant tests:** 13 tests across the i1-i7 files, 0 ignored, all green.
- **DST (final acceptance run):** core harness 10,000 random seeds, zero
  invariant violations (master seed 1781116180331). Composed decision-loop
  chaos battery 10,000 scenarios, each replayed byte-identically: 33,444
  orders, 51,284 proposals, 131,071 injected cognition failures, 116,735
  beliefs — zero violations (master seed 1781116572856). Regression corpus:
  0 seeds — no randomized run ever produced a failure to minimize; the corpus
  discipline (commit every red seed, never delete) is in place at
  crates/fortuna-core/dst-corpus/README.md.
- **Doctrine scenario coverage:** every scenario in the PROMPT verification
  doctrine maps to a covering arm or test: crash between persistence and
  submission / submission and ack, duplicate fills, fill-after-cancel, partial
  fill + outage, max-leg-duration groups, settlement posted-then-reversed,
  settlement-never-arrives (overdue), reservation rebuild with resting orders,
  halt mid-IntentGroup, rate-limit breach mid-burst, malformed venue payloads,
  clock skew, stale-mark wide-spread drawdown feed (fortuna-core/tests/dst.rs
  arms + runner composed tests); schema-invalid output, budget exhaustion,
  refusals, wrong-event beliefs, trigger debounce (synthesis_dst chaos mind +
  cognition tests); kill switch with the runtime dead (killswitch self-test,
  no DATABASE_URL); Postgres-down halt semantics (audit-failure halt test +
  kill switch independence); audit write failure halts trading (sim_loop).
- **Replay determinism:** same seed twice = byte-identical recordings,
  asserted in sim_loop, settlement_loop, and per-seed in the synthesis DST.
- **Paper-fill honesty:** `touch_prints_never_fill_resting_orders` fails the
  suite if touch-fill optimism is ever reintroduced.

## 4. Acceptance checklist disposition

| Item | Status |
|---|---|
| All BUILD_PLAN tasks complete with evidence | DONE (every task ticked with commit hash) |
| fmt/clippy/suite green | DONE (615/0; clean) |
| Invariant tests implemented, none ignored | DONE (13 tests, 0 ignored) |
| DST 10,000+ seeds, doctrine scenarios, corpus | DONE (10k core + 10k chaos, zero violations; corpus empty because no failure was ever found — discipline in place) |
| Replay determinism | DONE |
| mech strategies in Sim | DONE; **in Paper vs recorded streams: OPERATOR-BLOCKED** (Kalshi fixture capture incl. websocket streams) |
| Belief pipeline end-to-end in Sim; aeolus_eval vs sample fixture | DONE with StubMind; AnthropicMind built + mock-tested, **live exercise OPERATOR-BLOCKED** (ANTHROPIC_API_KEY); aeolus_eval proven vs the contract fixture, **recorded export OPERATOR-BLOCKED** |
| Kill-switch standalone + monthly script | DONE (I4-proven, no-Postgres self-test, scripts/killswitch-test.sh); **live venue plug OPERATOR-BLOCKED** (fixtures first) |
| Slack routing, digest, dead-man, export, dashboard vs sim data | DONE (send-side router + mock transport, digest/export/dashboard/metrics tests, dead-man pinger); **Socket Mode listener OPERATOR-BLOCKED** (Slack app credentials) |
| GAPS.md operator-blocked-only with unblocks | DONE |
| FINAL_REPORT.md | This document |

## 5. Operator go-live runbook

Live capital requires every step below, in order. Nothing in the codebase can
skip one: credentials alone unlock nothing, promotions are records only you can
write, and the kill switch must be proven before the first live order.

**Step 0 — provision (one session).**
- Create `.env` from the README template: `ANTHROPIC_API_KEY`,
  `KALSHI_API_KEY_ID`/`KALSHI_PRIVATE_KEY` (trading), separate
  `FORTUNA_KILLSWITCH_*` Kalshi credentials (the switch must not share),
  Slack bot + app tokens, `DATABASE_URL`.
- `cargo sqlx migrate run` against the production Postgres;
  schedule nightly backups + the monthly restore drill.

**Step 1 — record Kalshi fixtures (demo env).**
Per crates/fortuna-venues/tests/kalshi_doc_samples/README.md and the 27-item
checklist in docs/research/venue/kalshi-api-2026-06-10/research.md. Include
websocket book/trade streams and a voided market's settlement record. Commit
under fixtures/kalshi/. Re-run the venues suite; any mismatch is a contract
negotiation, not a silent adapt.

**Step 2 — first cognition exercise.**
One AnthropicMind smoke call against claude-haiku-4-5 under a tight CostBudget
(the env-key gate is the feature flag). Then enable the decision loop in Sim
and watch a week of shadow cycles; verify cost tracking against the console.

**Step 3 — record the Aeolus export** (one read-only command, GAPS has it) and
commit as fixtures/aeolus/recorded_envelope.json; aeolus_eval then scores real
forecasts with zero capital. The Phase 2 verdict (calibrated? CLV-positive?)
drives Aeolus promotion-or-retirement per spec Section 6.

**Step 4 — paper.**
Replay recorded streams through PaperVenue (the first post-fixture engineering
task wires the venue-generic runner composition). Promote mech_structural and
mech_extremes to Paper by writing promotion records (CLI/audit: actor = your
identity; the system cannot write these). Run >= 30 trading days; synth_events
needs >= 60 resolved beliefs. Weekly reviews produce GO/NO-GO recommendations
against the Section 11 thresholds — they never promote.

**Step 5 — kill-switch drill (before any live order, then monthly).**
`scripts/killswitch-test.sh` with the main runtime down (Postgres optionally
stopped). Wire `freeze --venue kalshi` only after Step 1 clears the adapter.
Re-arm and kill-reversal remain CLI-only by design; a compromised Slack token
must not un-halt the system.

**Step 6 — live-minimum.**
Write the Paper -> LiveMin promotion record (cap: $500 exposure per strategy,
>= 30 days). The drawdown halt requires YOUR re-arm out-of-band; the dead-man
monitor must page you through its own channel. Daily PnL/exposure on the
dashboard; reconciliation must stay clean. Scaled stage doubles exposure
stepwise on rolling 30-day forward metrics; any drawdown halt resets the step.

**Operational pins.** Live halt-state poll interval: <= 500ms, alert on poll
failure. Monthly: kill-switch test, backup restore drill, allocation review
(recommendations are advisory; record your decision in config), model-version
review (shadow comparisons; the swap gate holds without >= 30 paired resolved
beliefs per active category). System-level kill criterion (Section 11): if
after 90 live days no strategy sustains positive CLV, shelve the synthesis
pipeline and run mechanical-only while the thesis is re-examined.

## 6. What I would watch first

1. The fee model against real fills (per-fill reconciliation writes a
   discrepancy on mismatch) — fees are the quiet killer of thin edges.
2. Triage recall via the declined-trigger shadow samples — a triage model that
   declines winners starves the synthesis loop invisibly except here.
3. The calibration ramp: nothing earns full autonomous weight below 50
   resolved beliefs per scope, so early sizing will look timid. That is the
   design, not a bug.
4. Settlement watchdog noise on real Kalshi lag distributions — the 1h overdue
   grace may need category-specific tuning (config, not code).
