# Kinetics perps module — Phase B plan (CONFIRMED 2026-06-11)

Status: CONFIRMED by the operator 2026-06-11 — "your B1–B8 order supersedes
the truncated directive" — with three amendments, all incorporated below:

- **(A) New B0 before B1:** standalone perishable-data recorder on public
  endpoints — perps orderbook snapshots + spreads, KXBTC15M bracket quotes
  paired-timestamped with perp books, intraday funding estimates. Ships
  independently; nothing else blocks on it.
- **(B) funding_carry DEMOTED to data-collection-only.** No Sim status until
  >= 60 days of funding history demonstrates regime persistence.
  perp_event_basis and funding_forecast proceed as proposed.
- **(C) Fee-trap rule (B7):** all edge floors evaluated at assumed
  post-promo fees (use the 5–12 bps schema examples until /margin/fee_tiers
  returns real rates); Sim gates re-run when fees activate. Promo-$0
  economics never justify a GO.

The operator's recovered original list is folded in where it adds design
content (recorded verbatim in §6 for the trail): InstrumentKind threading
(-> B1/B2), liquidation-distance floor + per-asset leverage caps +
liquidation-loss-as-worst-case (-> B3, already aligned), adapter with its
OWN credentials + segregated margin account as a DEDICATED capital envelope
per spec 5.14 (-> B4), recorder-first-and-standalone (-> B0 = amendment A).

Grounding: docs/research/venue/kinetics-perps-2026-06-10/research.md.
Constraint quoted from the directive and binding throughout: "new
capability, zero changes to the invariant middle."

## 1. What the research says we are building against

- Linear USD-margined micro perps (BTCPERP = 0.0001 BTC ≈ $6.26/contract),
  14 listed / 11 active, 24/7 minus a Thursday maintenance window.
- Tick is $0.0001 with prices as fixed-point dollar STRINGS; counts are
  fixed-point strings, fractional currently disabled (min 1 contract).
- `client_order_id` is REQUIRED on create → our crash-resubmission idempotency
  model transfers intact (the thing Polymarket retail lacks). Limit orders
  only; `reduce_only` requires IOC/FOK (flatten planner must comply).
- Same RSA-PSS signing + hosts as the event API, paths under `/margin/*`;
  separate perps rate-limit buckets; dedicated margin-WS host (signing path
  string undocumented — fixture item #2).
- Funding every 8h (00/08/16 ET), TWAP of 480 one-minute premiums, capped
  ±2%, zeroed below 0.01%, accrues on `settlement_mark_price`, positive =
  longs pay. Full history is PUBLIC (no auth) — strategy research can start
  now.
- Margin: IM = 1.3 × MM; MM FORMULA UNPUBLISHED; isolated in-app but
  PORTFOLIO margin via API; liquidation run by Klear via system market
  orders (`order_source=system`); liquidation ratio 1.0, queue entry 0.91
  (prod; demo differs).
- Fees: $0 launch promo; real per-market maker/taker rates (decimal fraction
  of notional) become visible via `GET /margin/fee_tiers` from the June 11
  release; post-promo numbers UNPUBLISHED.
- Demo: OPEN TO EVERYONE, mock funds, full perps surface — but demo ≠ prod
  (tickers suffixed `1`, newer API build, different risk params): record on
  demo, re-verify shapes on prod read-only before live.

## 2. The four design problems (each needs spec text before code)

1. **Price domain.** Tick $0.0001 breaks `Cents(i64)` as the price carrier.
   Proposal: a perps-confined `PerpPrice` integer newtype in ten-thousandths
   of a dollar (i64; checked ops; Decimal only at the venue payload
   boundary), conversions to Cents ONLY at notional/PnL level with rounding
   always against us. The core event-contract path keeps Cents untouched —
   that is the "zero changes to the invariant middle" reading. (This is also
   the spec-level price-tick decision Polymarket was shelved on; solving it
   inside a venue-scoped type first, without touching the shared core, keeps
   the blast radius bounded.)
2. **Loss model.** Event contracts: max loss = premium, known at gate time.
   Perps: API accounts are PORTFOLIO-margined, so one position's excursion
   can consume the whole margin account. Conservative gate stance until
   fixtures prove otherwise: the margin ACCOUNT (not the position) is the
   exposure unit; the envelope is the deposited margin balance; per-order
   worst case = margin consumed at the liquidation point + funding drag, and
   the unpublished MM formula is approximated from recorded
   `leverage_estimates` curves with a safety multiplier, refusing any order
   the approximation cannot bound.
3. **Funding cash flows.** Positions held across funding timestamps generate
   venue-initiated cash movements that are neither fills nor settlements.
   New append-only accrual records reconciled against the venue's funding
   endpoints; drawdown (I2) must count funding + unrealized mark-to-mark, on
   `settlement_mark_price` per the conservative-marking rule.
4. **Venue-originated fills.** Klear liquidations arrive as fills we never
   placed (`order_source=system`). Today an unexplained fill is an orphan
   alarm; perps adds a LEGITIMATE class. Design: liquidation fills get a
   dedicated lifecycle state + mandatory alert + halt-evaluation (a
   liquidation means our margin model was wrong) — never silently absorbed.

## 3. Invariant mapping (unchanged middle, new checks at the edges)

- I1: perps orders are the same sealed `GatedOrder` through the same
  pipeline; the new margin/liquidation/funding checks ADD gates, never fork
  the path. I3 token buckets are already per-venue; perps buckets are config.
- I2: drawdown definition extended (spec text) to include funding accruals
  and margin unrealized PnL; breach semantics identical.
- I4: kill switch gains perps coverage — flatten via `reduce_only` IOC
  orders + cancel-all, still Postgres-free, still its own credential pair.
- I6: the model never sees a margin mutation tool; perps proposals are
  propose-only like everything else.
- I7: any perps strategy walks Sim → Paper → forward gate → operator
  promotion, no shortcuts for "it's mechanical".

## 4. Strategy plan — how perps adds to the $50k/month ambition

Ordered by edge-credibility, all data-backed from public endpoints today:

1. **perp_event_basis (mech, flagship).** Kalshi lists BOTH crypto event
   brackets and the perp on the same underlying with the same CF Benchmarks
   reference. A bracket ladder implies a distribution over the fixing; the
   perp + funding curve implies a point forecast. Systematic inconsistencies
   between the two surfaces on ONE venue (no cross-venue settlement risk, no
   wire latency between legs) is exactly the Atlas-lineage structural scan
   generalized — and FORTUNA already owns the bracket machinery. Fee-free
   while the promo lasts.
2. **funding_carry — DATA-COLLECTION-ONLY (operator amendment B).** Captured
   window shows all nonzero funding NEGATIVE (perp below index → shorts pay
   longs) at 0.01–0.04% per 8h, but one week of history proves nothing. No
   Sim status until >= 60 days of funding history demonstrates regime
   persistence; until then the B0 recorder + public funding history ARE the
   strategy work. (Unhedged leg = directional risk; CME micro futures as a
   hedge leg is size-mismatched — 0.1 BTC vs 0.0001 BTC — until notional
   grows.)
3. **funding_forecast (synth, zero-capital first).** Funding = deterministic
   TWAP of observable premiums → forecastable from 1-minute candles (public).
   Scalar claims in the prob_claims/v1 contract (docs/design/
   signal-contract.md) scored before any capital — the aeolus_eval pattern
   applied to a second domain.
4. **Portfolio effects.** 24/7 venue (event markets sleep), capacity that
   event contracts lack (BTCPERP did >$1B notional in week one vs 40 months
   for event contracts), and a second regulated venue family for I3-diverse
   exposure.

## 5. Confirmed Phase B order

- B0 Perishable-data recorder (amendment A; ships first and standalone):
  public-endpoint capture loop — perps orderbook snapshots + derived
  top-of-book spreads for active perps, KXBTC15M bracket quotes with
  timestamps paired to the same capture cycle as the perp books, intraday
  funding estimates + mark prices. JSONL on disk for later relative-value
  analysis (perp_event_basis groundwork, funding_carry's 60-day evidence
  clock). Runs continuously even if everything else slips.
- B1 Spec v0.9 amendment: perps domain — InstrumentKind { BinaryEvent,
  Perp } threaded through Market/positions/gates (perp positions never
  resolve: no settlement lifecycle; margin/maintenance state, mark-price
  feed, funding accruals as a new periodic cash-flow entry type),
  PerpPrice type, portfolio-margin
  loss model, funding cash-flow accounting, liquidation-fill lifecycle, I2
  drawdown extension, demo/prod divergence discipline. (Bundles the stale
  5.2 fee-claim touch-up already queued in GAPS.)
- B2 fortuna-core perps types: PerpPrice, signed PerpPosition, FundingAccrual
  (append-only), MarginAccountView with conservative marking. Property tests
  on price/notional conversions (rounding always against us).
- B3 Gate extensions: margin-headroom gate, liquidation-distance FLOOR
  (configurable min distance to liquidation at conservative mark) +
  per-asset leverage caps (operator original item 2), funding-drag-in-edge,
  per-venue notional caps. Worst-case exposure for perps uses LIQUIDATION
  loss, never premium. Invariant-crate ADDITIONS (new tests only) for the
  I2 extension.
- B4 Venue adapter (fortuna-venues/src/kinetics/) with its OWN credential
  pair; the segregated margin account maps to a DEDICATED capital envelope
  (spec 5.14) — perps capital is allocated explicitly, never bled from the
  event-contract envelope (operator original item 3). REST client + DTOs
  from perps_openapi.yaml doc-derived samples (kalshi pattern), WS message
  layer from perps_asyncapi.yaml, FIXTURES-GATED clearance vs
  fixtures/kinetics-perps/ (18-item request list in research §12; the
  fixture session is credential-unblocked as of 2026-06-11).
- B5 Paper engine margin semantics: funding accrual on SimClock timestamps,
  liquidation simulation from the recorded risk-param curves, mark-based
  PnL. A liquidation under-modeled = test failure, not surprise.
- B6 DST arms: funding-tick chaos, liquidation under ack-delay/api-error,
  system-fill ingestion, margin-call sequences, demo-divergence (suffixed
  tickers) confusion.
- B7 Strategies rung 0: perp_event_basis in Sim; funding_forecast as
  zero-capital scalar claims; funding_carry data-collection-only
  (amendment B). FEE-TRAP RULE (amendment C): every edge floor evaluates
  at assumed post-promo fees — the 5–12 bps schema examples until
  /margin/fee_tiers returns real rates — and Sim gates re-run when fees
  activate. Promo-$0 economics never justify a GO. I7 path unchanged.
- B8 Ops: kill-switch perps flatten, margin/funding telemetry (existing
  LatencyStat/percentile machinery), funding-regime dashboard panel.

Sequencing vs the rest of the program: T4.1 (daemon) is unaffected and stays
first in engineering order; B4's fixture session shares the operator's demo
key fix with the T4.2 Kalshi session — one credential unblock, two recording
sessions, ideally run back-to-back. Fee re-check (post-June-11 fee_tiers)
folds into the fixture session.

## 6. Operator's original Phase B list (recorded verbatim, 2026-06-11)

> 1. InstrumentKind { BinaryEvent, Perp } threaded through Market, positions, and
>    gates. Perp positions never resolve: no settlement lifecycle; instead
>    margin/maintenance state, mark-price feed, and funding accruals as a new
>    periodic cash-flow entry type.
> 2. New gate: liquidation-distance floor (configurable min distance to liquidation
>    at conservative mark) plus per-asset leverage caps. Worst-case exposure for
>    perps uses liquidation loss, not premium.
> 3. Kinetics Venue adapter (fixtures-gated like Kalshi) with its own credentials.
>    Its segregated margin account maps to a dedicated capital envelope (spec 5.14).
> 4. Funding/spread recorder FIRST and standalone — it ships and runs even if the
>    rest slips: funding rates, perp spreads, mark-vs-trade, and KXBTC15M binary
>    quotes, persisted for later relative-value analysis.

Disposition: 1 -> B1+B2; 2 -> B3; 3 -> B4; 4 -> B0 (= amendment A). The
confirmed B0–B8 sequence carries every element.
