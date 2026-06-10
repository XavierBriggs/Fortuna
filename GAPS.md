# GAPS.md - honesty ledger (agent-maintained)

Open items the implementation defers, lacks, or needs from the operator. Acceptance
requires this file to contain ONLY operator-blocked items, each with exact unblock steps.

## Operator-blocked (initial)
- Kalshi API fixtures not yet captured (see fixtures/kalshi/README.md). Unblock: operator records fixtures.
- Venue + Anthropic + Slack credentials (env vars). Unblock: operator provisions .env per README.
- Aeolus sample envelope fixture for aeolus_eval (T2.7). Unblock: operator exports one Aeolus run.

## Open
- **Sub-cent price structures excluded (T0.3, 2026-06-09).** Kalshi has live
  `deci_cent`/`tapered_deci_cent` markets (2 as of 2026-06-09) and Polymarket
  ticks go to 0.0001: core money is integer cents by convention, so adapters
  MUST filter these market structures out (T1.1/T3.4 filter + test). Revisit
  only if such markets matter commercially; would require a price-tick type.
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
- **Kalshi pair auto-netting not modeled (T0.7, 2026-06-09).** The sim venue
  holds YES and NO lots to settlement (value-identical to netting, capital-
  inefficient: real Kalshi credits $1/pair immediately when both sides are
  held, freeing balance early). Verify the exact netting behavior against
  fixtures at T1.1 and model the early credit in the paper engine (T1.2) if
  confirmed — it affects capital-efficiency realism, never PnL totals.
- **Polymarket per-market fee params should be read at runtime** (fd fields /
  feeSchedule on markets) rather than hard-coding category tables — T3.4
  design note from research; engine already takes schedules as data.
- **Slack interactivity listener (Socket Mode) deferred (T0.9, 2026-06-10).**
  Send-side (router, Block Kit approval builder) is built; the wss listener
  for button presses + slash-command kill requests lands with the review
  flows it serves (T2/T3 edge confirmations, promotions). Research doc has
  the full contract ready (apps.connections.open, envelope ack, user-id
  allow-listing).
- **Kill-switch live venue plug pending T1.1.** The binary + freeze logic +
  self-test are complete and I4-proven against the sim venue; `freeze
  --venue kalshi` exits loudly until the fixture-built Kalshi adapter
  exists. Unblock: T1.1 (operator fixtures) then wire the adapter with its
  OWN credential set (FORTUNA_KILLSWITCH_* env).
- **Runner halt-poll interval (T0.10).** Operator halts via CLI act on the
  running system within the poll interval; document the chosen interval in
  the runner and alert on poll failures.

## Disputed invariant tests
(none)
