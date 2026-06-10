# GAPS.md - honesty ledger (agent-maintained)

Open items the implementation defers, lacks, or needs from the operator. Acceptance
requires this file to contain ONLY operator-blocked items, each with exact unblock steps.

## Operator-blocked (initial)
- **Kalshi fixture recording + adapter clearance (T1.1, 2026-06-10).** The
  adapter is BUILT and tested against doc-derived samples (122 venues
  tests), but it is cleared for Sim development ONLY. Paper/live clearance
  requires operator-recorded fixtures under fixtures/kalshi/ confirming
  the 27-item checklist in docs/research/venue/kalshi-api-2026-06-10/
  research.md (highest-stakes items: 409-duplicate body shape, error code
  catalog, cancel-reconcile race, fills cursor terminal semantics,
  timestamp skew tolerance, fee_multiplier maker scaling). The fixture
  capture must ALSO include websocket orderbook_snapshot/orderbook_delta
  and public trade messages for the paper engine (T1.2 dependency).
  Unblock: operator records demo-env fixtures per
  crates/fortuna-venues/tests/kalshi_doc_samples/README.md.
- Venue + Anthropic + Slack credentials (env vars). Unblock: operator provisions .env per README.
- Aeolus sample envelope fixture for aeolus_eval (T2.7). Unblock: operator exports one Aeolus run.

## Open
- **Sub-cent price structures excluded (T0.3, 2026-06-09; Kalshi filter
  SHIPPED at T1.1).** Core money is integer cents by convention. The Kalshi
  adapter now filters `deci_cent`/`tapered_deci_cent` structures and scalar
  markets out of the catalog (tested vs doc samples); the same rule is owed
  by the Polymarket adapter at T3.4 (0.0001 ticks). Revisit only if such
  markets matter commercially; would require a price-tick type.
- **Spec 5.2 fee claims are stale** (documented drift, not a code gap):
  "Polymarket Intl mostly zero" and "Polymarket US flat 10bp taker" describe
  superseded regimes. Current reality (researched 2026-06-09, docs/research/
  venue/): Intl per-category quadratic taker 0.03-0.07 + maker rebates;
  US quadratic taker 0.05 / maker -0.0125 with banker's rounding. The fee
  engine supports all of it via config. Spec text needs a v0.9 touch-up by
  the operator (spec changes require a version bump, Section 3 preamble).
- **Kalshi `flat` fee_type semantics unverified** (defined in their API enums,
  zero live series use it). Engine has flat_bps; mapping confirmed at T1.1.
- **Kalshi maker-fee x multiplier scaling is inferred** from live page math
  (strong numeric evidence, no explicit doc sentence). Verify against fee
  fields in recorded fixtures at T1.1.
- **Kalshi pair auto-netting not modeled (T0.7, 2026-06-09; reaffirmed at
  T1.2).** Both the sim venue AND the paper engine hold YES and NO lots to
  settlement (value-identical to netting, capital-inefficient: real Kalshi
  credits $1/pair immediately when both sides are held, freeing balance
  early). T1.2 shipped WITHOUT the early credit deliberately — conservative
  on capital, identical on PnL. Verify the exact netting behavior against
  fixtures at T1.1; if confirmed, add the early credit to paper as a
  capital-realism follow-up.
- **Paper engine awaits recorded Kalshi data streams (T1.2, 2026-06-10).**
  `PaperVenue` consumes pushed canonical books + public trade prints
  (yes-space). Phase 1 exit requires running against RECORDED streams: the
  operator fixture capture must include websocket `orderbook_snapshot`/
  `orderbook_delta` and public `trade` messages (channels documented in
  docs/research/venue/kalshi-api-2026-06-10). Until then paper runs against
  sim-generated feeds only.
- **Polymarket per-market fee params should be read at runtime** (fd fields /
  feeSchedule on markets) rather than hard-coding category tables — T3.4
  design note from research; engine already takes schedules as data.
- **Slack interactivity listener (Socket Mode) deferred (T0.9, 2026-06-10).**
  Send-side (router, Block Kit approval builder) is built; the wss listener
  for button presses + slash-command kill requests lands with the review
  flows it serves (T2/T3 edge confirmations, promotions). Research doc has
  the full contract ready (apps.connections.open, envelope ack, user-id
  allow-listing).
- **Kill-switch live venue plug pending fixture clearance.** The binary +
  freeze logic + self-test are complete and I4-proven against the sim
  venue; the Kalshi adapter now EXISTS (T1.1) but `freeze --venue kalshi`
  stays unwired until the adapter passes fixture confirmation — the kill
  switch must not take its first real cancel path through unverified
  venue code. Unblock: operator fixtures, then wire KalshiVenue into the
  killswitch with its OWN credential set (FORTUNA_KILLSWITCH_* env).
- **Runner halt-poll interval (T0.10).** Operator halts via CLI act on the
  running system within the poll interval; document the chosen interval in
  the runner and alert on poll failures.

## Disputed invariant tests
(none)
