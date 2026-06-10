# GAPS.md - honesty ledger (agent-maintained)

Open items the implementation defers, lacks, or needs from the operator. Acceptance
requires this file to contain ONLY operator-blocked items, each with exact unblock steps.

Status (T3.6, 2026-06-10): every remaining item is OPERATOR-BLOCKED with exact
unblock steps. Engineering items previously under "Open" were closed during the
build (divergence detector + orphan watchdog landed at T3.6; the I7 invariant
stubs at T3.3) or were decision records and moved to ASSUMPTIONS.md
(sub-cent exclusion, pair auto-netting, halt-poll documentation).

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
