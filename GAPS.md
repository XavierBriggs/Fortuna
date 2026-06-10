# GAPS.md - honesty ledger (agent-maintained)

Open items the implementation defers, lacks, or needs from the operator. Acceptance
requires this file to contain ONLY operator-blocked items, each with exact unblock steps.

Status (post-verification, 2026-06-10): the T3.6 claim that "every remaining
item is OPERATOR-BLOCKED" was FALSIFIED by the full-build verification gate
(docs/reviews/system-0-3-final-2026-06-10.md, verdict BLOCK). Four Major
engineering items were open and unledgered at 7bbc3ef; they are now ledgered
below (section "Unresolved engineering items"). The build is production-ready
only after E1-E4 land, the full gate re-runs clean, and the operator actions
in "Path to production" complete. Engineering items genuinely closed during
Phase 3: divergence detector + orphan watchdog (T3.6), I7 invariant stubs
(T3.3); decision records moved to ASSUMPTIONS.md (sub-cent exclusion, pair
auto-netting, halt-poll documentation).

Verification record: five verdicts in docs/reviews/ (phase-1, phase-2,
system-0-1-2, phase-3, system-0-3-final, all 2026-06-10). The phase-3 gate
and the system gate disagreed on acceptance-item grading; the disagreement
was adjudicated with executed evidence (zero Kelly call sites, no
`impl Mind for AnthropicMind`, vacuous per-cycle budget, zero discrepancy
hits in seeded DST — all confirmed at 7bbc3ef). The system gate's stricter
reading governs.

## Unresolved ENGINEERING items (must close before any go-live; none are operator-blocked)

- **E1. Haircut-Kelly + calibrated-p sizing is unwired (Major; anti-conservative).**
  Spec 5.8 ("Calibration layer adjusts p") and spec line 240 (fractional
  Kelly, default 0.25, haircut by category calibration quality). Current
  state at 7bbc3ef: the ONLY sizing path for model-originated proposals is
  `affordable_sets(headroom, cost_per_set, max_sets_per_proposal)` at
  crates/fortuna-runner/src/runner.rs:478 — full envelope headroom up to
  caps. `kelly_binary` / `kelly_contracts` / `haircut_kelly_fraction`
  (sizing lib, T0.10/T2.6) and the T2.8 calibration layer have ZERO src
  call sites. Fix spec: in the synthesis sizing path, (1) fetch latest
  CalibrationParams for the (model, strategy, category) scope and apply to
  raw p; (2) haircut fraction = base 0.25 x calibration quality; (3)
  contracts = min(kelly_contracts(calibrated edge, envelope bankroll,
  haircut fraction), affordable_sets(...)). Fail CLOSED: missing or
  degenerate calibration params => quality floor => minimal/zero size,
  never full headroom. Prerequisite: fix E5a (fortuna-state sizing.rs:68
  f64 cents) FIRST — wiring E1 makes that dead code live. Also correct the false
  docs: cycle.rs:8-11 header, the ASSUMPTIONS.md sizing claim, BUILD_PLAN
  T2.6/T2.8 DONE notes. Close criterion: composed-loop test asserting
  sized contracts < affordable when Kelly binds + a DST arm; gate re-run.

- **E2. AnthropicMind is not behind the `Mind` trait (Major).** Spec 5.9
  ("both behind `Mind`"). Current: `impl Mind for StubMind` only
  (mind.rs:173); AnthropicMind has only an inherent impl and zero
  construction sites outside tests — unexercisable even with a key.
  Fix spec: `impl Mind for AnthropicMind<T>`; composition path that
  constructs it when ANTHROPIC_API_KEY is present (the env-key gate stays
  the feature flag) and StubMind otherwise; provenance stamping and budget
  checks at the trait boundary. Close criterion: composition test driving
  a mock-transport AnthropicMind through `dyn Mind` end to end.

- **E3. Per-cycle cost budget never binds (Major).** Spec 5.9 budget
  controls. Current: mind.rs:221-238 `check()` errors only when
  `per_cycle_cap_cents <= 0`; per-cycle spend is never tracked, so any
  positive cap is vacuous. Fix spec: track per-cycle spend (reset at cycle
  start, accumulate post-call cost), breach => degrade per spec (skip
  consult, mechanical path continues, audit row). Close criterion: test
  where a positive per-cycle cap rejects the call that would exceed it.

- **E4. Discrepancy/watchdog paths + ~8 doctrine scenarios absent from the
  seeded DST corpus (Major).** Phase 1 EXIT ("discrepancy paths exercised
  in DST"); PROMPT.md:103-104 ("every scenario in the doctrine list
  present"). Current: zero discrepancy/watchdog hits in
  crates/fortuna-core/tests/dst.rs and crates/fortuna-runner/tests/
  synthesis_dst.rs; coverage lives only in settlement_loop.rs composed
  tests, which scripts/run-dst.sh never executes. Fix spec: add seeded
  scenario arms for position-mismatch (3-tick => discrepancy + GLOBAL
  halt), overdue settlement, dispute freeze; map the remaining doctrine
  scenarios into the generator; commit any red seed to dst-corpus/.
  Close criterion: 10,000-seed run clean WITH the new arms active.

- **E5. Minors (sweep before the gate re-run):**
  - (a) crates/fortuna-state/src/sizing.rs:68 computes the Kelly cents
    budget in f64 (`headroom.raw() as f64 * f`) — convert to checked
    integer/Decimal BEFORE E1 wires it live; (b) cycle.rs:117 f64 floor
    (conservative) and review.rs:278 f64 ratio (recommendation-only) —
    convert or record in ASSUMPTIONS.md.
  - scripts/run-dst.sh:13-19 fails OPEN ("passing vacuously", exit 0) if
    the dst target fails to BUILD; the harness exists now — a build
    failure must exit non-zero.
  - run_watchdogs early-returns on venue outage (runner.rs:1375-1378),
    starving venue-independent checks (overdue/dispute) — partition them.
  - DecisionCycle discards model ProposalDrafts uncounted and writes no
    model_call audit row in the Sim loop; SynthesisStrategy drops
    manifest_hash — make exclusions counted + audited.
  - Regression corpus is empty ("regression seeds committed" passes only
    vacuously) — commit the first red seed; until one exists the vacuity
    stays disclosed here and in FINAL_REPORT.md.
  - Remove 4 tracked .DS_Store files; add to .gitignore.

## Operator adjudication queue (operator actions; no code changes)

- **Protected-crate waives (3 batches).** crates/fortuna-invariants/ was
  touched in Phases 1-3; each touch triggered the automatic BLOCK rule.
  All three were audited line-by-line across the five reviews: every
  deletion in history was a baseline `#[ignore]`+`todo!()` placeholder
  replaced by implemented tests; ZERO existing assertions modified,
  deleted, or loosened. Batches: (1) Phase 1 — three one-line
  `volume_contracts: None` fixture compile-fixes (i1/i3/i4); (2) Phase 2 —
  I6/I7 stub closures + 2 new T3-staged stubs; (3) Phase 3 (6276274) —
  closure of those two I7 stubs. Unblock: operator reviews
  `git log -p -- crates/fortuna-invariants/` (or the diffs quoted in the
  verdict files) and records the waive decision in this file under
  "Disputed invariant tests" or in the ops log; that converts the
  rule-based BLOCKs.

## Path to production (ordered; after this list only operator actions remain)

1. ENGINEERING: land E1-E4 + E5 sweep (E5a strictly before E1). One task
   per session, verifier-gated per the house rules.
2. ENGINEERING: re-run the full-build gate (fortuna-review phase-gate mode
   at the new head); production work proceeds only on ACCEPT or
   ACCEPT-WITH-GAPS where every remaining gap is in this file's
   operator-blocked sections.
3. OPERATOR: adjudicate the protected-crate waives (queue above).
4. OPERATOR: provision credentials (.env per README) — ANTHROPIC_API_KEY
   first, then the one-haiku-smoke-call under a tight CostBudget; Slack
   app token + allow-listed user ids (Socket Mode listener exercise);
   Kalshi credentials last (they unlock nothing alone by design — I7).
5. OPERATOR: Kalshi demo-env fixture recording session (single session
   covers the 27-item checklist + websocket streams + voided market + fee
   fields — details in the Kalshi section below). Then ENGINEERING:
   venue-generic runner composition replaying recordings into PaperVenue
   (first post-fixture task), kill-switch KalshiVenue plug with its OWN
   FORTUNA_KILLSWITCH_* credentials.
6. OPERATOR: Aeolus recorded-export one-liner (section below); Polymarket
   research authorization if/when wanted (parallel, not on the critical
   path); spec v0.9 fee touch-up before any Polymarket go-live.
7. PROMOTION (I7, operator-only): mech strategies Sim -> Paper forward
   window -> operator promotion to live capital; model swaps only via the
   shadow comparison harness recommendation; drawdown-halt re-arms stay
   CLI-only out-of-band. Live trading remains OFF until every step above
   is recorded.

## Operator-blocked: Kalshi fixtures (one recording session unblocks all)

- **Kalshi fixture recording + adapter clearance (T1.1).** The adapter is
  BUILT and tested against doc-derived samples (124 venues tests), but it
  is cleared for Sim development ONLY. Paper/live clearance requires
  operator-recorded fixtures under fixtures/kalshi/ confirming the 27-item
  checklist in docs/research/venue/kalshi-api-2026-06-10/research.md
  (highest-stakes items: 409-duplicate body shape, error code catalog,
  cancel-reconcile race, fills cursor terminal semantics, timestamp skew
  tolerance, fee_multiplier maker scaling).
  Unblock: operator records demo-env fixtures per
  crates/fortuna-venues/tests/kalshi_doc_samples/README.md.
  The SAME capture must also include:
  - websocket `orderbook_snapshot`/`orderbook_delta` + public `trade`
    messages (paper-engine recorded-stream replay; first post-fixture task
    is the venue-generic runner composition replaying recordings into
    PaperVenue under both mechanical strategies),
  - a VOIDED market's settlement record (`market_result` documents only
    yes/no/scalar; the adapter hard-errors on anything else so a live void
    surfaces loudly instead of passing silently),
  - fee fields on fills (verifies the inferred maker-fee x multiplier
    scaling and the unused-in-the-wild `flat` fee_type mapping).
- **Kill-switch live venue plug (after fixture clearance).** The binary +
  freeze logic + self-test are complete and I4-proven against the sim
  venue; `freeze --venue kalshi` stays unwired until the adapter passes
  fixture confirmation — the kill switch must not take its first real
  cancel path through unverified venue code. Unblock: fixtures above, then
  wire KalshiVenue into the killswitch with its OWN credential set
  (FORTUNA_KILLSWITCH_* env).

## Operator-blocked: credentials

- **Venue + Anthropic + Slack credentials (env vars).** Unblock: operator
  provisions .env per README.
  - ANTHROPIC_API_KEY: AnthropicMind is BUILT and mock-tested; the env-key
    gate IS the feature flag. Recommended first exercise: one live smoke
    call against claude-haiku-4-5 under a tight CostBudget.
  - Slack app token(s): send-side routing (config-driven channel router,
    Block Kit approval builder) is built and tested with a mock transport;
    the Socket Mode interactivity listener (button presses, slash-command
    kill REQUESTS — never re-arms, which stay CLI-only) is built-to-contract
    work that needs a real app + allow-listed user ids to exercise.
    Research contract ready: docs/research/ops/ (apps.connections.open,
    envelope ack, user-id allow-listing).
  - Kalshi API credentials: live trading also requires promotions (I7) and
    fixtures above; credentials alone unlock nothing by design.

## Operator-blocked: Aeolus

- **Operator-recorded Aeolus envelope export (T2.7).** The ingestion
  contract is FORTUNA-defined (AeolusEnvelope, strict deny-unknown-fields)
  and the full fixture->drafts->persisted->scored path is proven against
  fixtures/aeolus/sample_envelope.json (the contract sample). The
  OPERATOR-RECORDED real export remains open — it validates Aeolus's
  exporter, not FORTUNA's parser. A read-only export from the Aeolus box
  was attempted and DENIED by the permission classifier (prod read without
  explicit approval — correct call). Unblock: operator runs ONE read-only
  command and commits the output as fixtures/aeolus/recorded_envelope.json:
  `ssh Aeolus 'sqlite3 -json /home/ec2-user/aeolus/artifacts/live/aeolus.db
  "SELECT ... one run ..."'` shaped to the contract (or adds an export
  endpoint to aeolus-runner). Any mismatch is a contract negotiation,
  never a silent adapt.

## Operator-blocked: Polymarket US

- **Polymarket US adapter is a fixtures-gated STUB (T3.4).**
  `fortuna_venues::polymarket::PolymarketUsStub` fills the trait slot;
  every operation (and its fee model) refuses with
  `VenueError::FixtureGated` — no market data, no orders, no invented
  fees. Building the real adapter requires, in order: (1) a venue research
  loop under docs/research/venue/polymarket-us-<date>/ (API auth model,
  CLOB endpoints, fee schedule incl. per-market runtime fee params
  (fd/feeSchedule fields — read at runtime, never hard-coded), sub-cent
  tick handling per the cents-only core policy, settlement/void
  representation, US-entity specifics), then (2) operator-recorded
  fixtures under fixtures/polymarket/ covering the same checklist shape as
  Kalshi's (catalog, books, order lifecycle incl. duplicate/timeout
  semantics, fills paging, settlements incl. voids). Unblock: operator
  authorizes the research loop + records demo fixtures; the adapter then
  builds against recordings only, like kalshi/.

## Operator-blocked: spec maintenance

- **Spec 5.2 fee claims are stale** (documented drift, not a code gap):
  "Polymarket Intl mostly zero" and "Polymarket US flat 10bp taker"
  describe superseded regimes. Current reality (researched 2026-06-09,
  docs/research/venue/): Intl per-category quadratic taker 0.03-0.07 +
  maker rebates; US quadratic taker 0.05 / maker -0.0125 with banker's
  rounding. The fee engine supports all of it via config. Unblock:
  operator issues a spec v0.9 touch-up (spec changes require a version
  bump, Section 3 preamble).

## Disputed invariant tests
(none)
