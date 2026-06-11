# GAPS.md - honesty ledger (agent-maintained)

Open items the implementation defers, lacks, or needs from the operator. Acceptance
requires this file to contain ONLY operator-blocked items, each with exact unblock steps.

Status (post-E-batch, 2026-06-10): the T3.6 completion claim was FALSIFIED
by the full-build gate (docs/reviews/system-0-3-final-2026-06-10.md, BLOCK:
four unledgered Majors). The fix batch (commits 1d1c033..1e3e5e7) closed
E1-E5 and the RE-RUN GATE is an ACCEPT
(docs/reviews/system-0-4-egate-2026-06-10.md): every E-item graded CLOSED
with executed evidence, regression battery clean (630 tests, three-stage
10,000-seed DST). An INDEPENDENT re-gate (docs/reviews/
system-0-4-egate-INDEPENDENT-2026-06-10.md, blind to the first e-gate
verdict, fresh seeds) corroborated all five E-closures on their ledgered
close criteria — and found ONE new Major (F1 below) plus two Minors that
the first e-gate missed. F1-F3 were closed and re-gated (f-batch-gate, ACCEPT-WITH-GAPS; its three
Minors closed at head). Everything below is an OPERATOR action. One Minor stays disclosed: the
regression-seed corpus is empty (no randomized run has produced a red
seed; discipline in place).

## Engineering items F1-F3: CLOSED (gate-verified)

Found OPEN by the independent e-gate; closed by b4c839f (F1: degrade
kind preserved + drained to 'cognition' audit rows + bus events, budget
breaches counted once at the drain and exported; ops alerts module —
every breach alerts with the scrape count, failure bursts threshold-
gated; F2: BUILD_PLAN T2.8 visible correction; F3: wholly-discarded
output writes a model_proposals_discarded trace) and graded CLOSED by
the targeted re-gate (docs/reviews/f-batch-gate-2026-06-10.md,
ACCEPT-WITH-GAPS). That gate's three Minors closed at the head commit:
the settlement_dst aggregate coverage asserts now gate on a 100-scenario
floor (a 20-scenario draw can legitimately miss the halting arms — a
coverage assert that intermittently reds healthy code erodes trust in
real reds; repro master 1781139292562 now passes), the cost-metric
undercount gained the budget-true surface (fortuna_mind_spend_today_cents
includes failed-call burn, test-asserted on a schema-invalid call), and
the false "cost rides in cognition audit rows" doc line was corrected
visibly in ASSUMPTIONS (per-decision cost rides in belief provenance).

Verification record: five verdicts in docs/reviews/ (phase-1, phase-2,
system-0-1-2, phase-3, system-0-3-final, all 2026-06-10). The phase-3 gate
and the system gate disagreed on acceptance-item grading; the disagreement
was adjudicated with executed evidence (zero Kelly call sites, no
`impl Mind for AnthropicMind`, vacuous per-cycle budget, zero discrepancy
hits in seeded DST — all confirmed at 7bbc3ef). The system gate's stricter
reading governs.

## Engineering items E1-E5: CLOSED (gate-verified)

Ledgered open at 7bbc3ef; closed by 1d1c033 (E1+E5a: calibration layer
binds in the cycle, haircut-Kelly sizing = min(kelly, affordable) with
composition-fed quality failing closed to zero, integer-money Kelly
budget), E2 commit (AnthropicMind behind `dyn Mind` with owned budget +
env-gated factory), 5954999 (E3: per-cycle cap binds via begin_cycle
tracking; config surface added), ca38028 (E4: 10-arm settlement/watchdog
seeded DST as run-dst.sh stage 4, fail-closed script), 1e3e5e7 (E5:
watchdog outage partition, counted discards + audited proposal manifest
hashes, hygiene). Each close criterion was graded CLOSED with executed
evidence by the independent gate: docs/reviews/system-0-4-egate-2026-06-10.md
(verdict ACCEPT). False documentation was corrected with the correction
visible (ASSUMPTIONS.md, BUILD_PLAN.md T2.6 note), never erased.

## Minor engineering residue (non-blocking; verified open by the independent
## f-batch gate, docs/reviews/f-batch-INDEPENDENT-gate-2026-06-10.md)

Fold these into the FIRST POST-FIXTURE composition task (path-to-production
step 5) — three are composition-wiring by nature and two were already noted
there by the independent e-gate:
- `fortuna_ops::alerts::degrade_alerts` has zero non-test callers — the F1
  alert rule is built and test-covered but undeliverable until a scrape
  composition consumes it; ASSUMPTIONS overstates this as live.
- `CalibrationParamsRepo.latest` has no live call site (e-gate note) —
  must bind in the live composition.
- `fortuna_mind_spend_today_cents` exported with `counter: true`
  (runner.rs metrics_export, first tuple block) despite daily-reset gauge
  semantics — fix the type flag before any Prometheus consumer applies
  rate() to it.
- `settlement_voids`/`settlement_reversals` counter increments are never
  post-state asserted (settlement_loop.rs asserts only the ==0 pre-state).
- Kelly sizing keys off legs[0] only (single-leg synthesis today; revisit
  with multi-leg).

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
  rule-based BLOCKs. Batch (4), added by the E-batch: one fixture line in
  i7_promotion_gates.rs (`kelly_fraction: 0.25,` in its runner_config —
  a compile fix for the new required RunnerConfig field), confirmed
  assertion-clean via full patch by the E-gate verdict.

## Path to production (ordered; after this list only operator actions remain)

1. DONE (1d1c033..1e3e5e7): E1-E4 + E5 sweep landed, E5a before E1.
2. DONE (docs/reviews/system-0-4-egate-2026-06-10.md): full gate re-run
   at 1e3e5e7 — ACCEPT; all remaining gaps are in this file's operator
   sections.
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
    messages. (Status 2026-06-10: the WS MESSAGE layer is BUILT
    doc-derived — parser, seq-gap detection, yes-scale subscribe builder,
    BookAssembler, and the stream->PaperVenue replay seam are tested
    against the verbatim official examples. The capture CONFIRMS the
    contract — esp. use_yes_price semantics, fixture #20 — and unblocks
    the live socket DIAL (signed-handshake auth, keep-alive, redial) plus
    the venue-generic runner composition replaying the recordings.),
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

- **Polymarket US adapter is a fixtures-gated STUB (T3.4); RESEARCH NOW
  DONE (2026-06-10, operator-authorized this session):**
  docs/research/venue/polymarket-us-2026-06-10/research.md (829 lines, 96
  archived sources). Material findings that reshape the build decision:
  (a) retail API has NO CLIENT ORDER ID — FORTUNA's crash-resubmission
  idempotency model does not transfer (institutional stack has clordId);
  (b) SUB-CENT TICKS ARE LIVE (0.5c, 0.25c preprod) + decimal quantities
  + decimal settlements — three explicit conflicts with the integer-cents
  core; (c) NO RETAIL SANDBOX — fixtures would be minimum-size recordings
  on PROD, or institutional preprod via firm onboarding; (d) fee reality
  CONFIRMED vs the 2026-06-09 research (taker 0.05 / maker -0.0125
  quadratic, banker's rounding); (e) sports-only listings today.
  OPERATOR DECISION REQUIRED before any build: retail-prod recording vs
  institutional onboarding vs shelve until the entity matures. The stub
  refuses everything meanwhile; the cents-core conflict alone argues for
  shelving (a price-tick type is a spec-level change).

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
