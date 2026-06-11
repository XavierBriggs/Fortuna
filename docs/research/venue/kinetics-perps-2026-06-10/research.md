# Kalshi Crypto Perpetual Futures ("Kinetics perps") — Venue Research (retrieved 2026-06-10/11)

Scope: Phase A ground-truth research for a possible FORTUNA venue module on Kalshi's
crypto perpetual futures product — traded on the KalshiEX DCM, cleared by Kalshi Klear
(DCO), carried for retail by Kinetic Markets LLC (FCM, branded "Kinetics"). READ-ONLY
research; no code. Every material claim cites an archived source in `SOURCES.md`
(IDs P*, H*, K*, R*, N*, L*, W*) with a confidence tag:

- **HIGH** — official spec/docs/registry/regulatory document, fetched and archived.
- **MEDIUM** — official marketing / help-center prose, fetched.
- **LOW** — third-party press, or inference from observed data.

Live API captures (L*) were taken unauthenticated from production and demo on
2026-06-11 ~05:50–06:05 UTC. Per repo rules nothing here is trusted for live use until
an operator-recorded fixture confirms it (Section 12).

Vocabulary note (HIGH, P5): "**'Perps', 'margin', and 'perpetual futures' all refer to
the same product.** The API surface uses *margin* throughout (endpoints under the
`/margin` namespace, margin-prefixed fields)".

---

## 1. Executive summary

Kalshi launched the first CFTC-regulated US perpetual futures on 2026-06-03 (launch
date per CNBC/company press, LOW; the CFTC approval order is 2026-05-29, HIGH; 14
crypto perps listed live as of retrieval, 11 active). Structure: KalshiEX LLC = DCM,
Kalshi Klear LLC = DCO, Kinetic Markets LLC ("Kinetics", NFA ID 0574784, wholly owned
by Kalshi Inc.) = FCM carrying retail margin accounts, segregated from the
event-contract account. Contracts are linear USD-margined micro-sized units (BTCPERP =
0.0001 BTC), tick $0.0001, 24/7 except Thursday 03:00–05:00 ET maintenance, funding
every 8h (00:00/08:00/16:00 ET) as a TWAP of 1-minute premiums, capped ±2%, vs CF
Benchmarks real-time indices. Max leverage is asset-specific (~5.9x BTC down to ~2x);
IM = 1.3 × MM; liquidation is run by the clearinghouse/FCM with a default-fund
waterfall. Fees are currently $0 (launch promo); the post-promo schedule is unpublished
— only a runtime API (`/margin/fee_tiers`, rates as decimal fraction of notional).
API: full REST (`/margin/*` on the existing trade-api hosts, same RSA-PSS auth), a
dedicated WS host, and FIX. **Demo is open to everyone today and serves the full perps
surface** — fixtures can be recorded without real margin. Key unknowns: position
limits, maintenance-margin formula, post-promo fees, margin-WS signing path.

## 2. Product + regulatory structure

**Entities (HIGH unless noted):**

| Entity | Role | Evidence |
|---|---|---|
| KalshiEX LLC | CFTC-designated contract market (DCM); lists the perp contracts | CFTC Order R1; PR R2; DCM Rulebook K4 |
| Kalshi Klear LLC | CFTC-registered derivatives clearing organization (DCO), registered Aug 2024 ("Order of Registration as a DCO"; 18th DCO) | PR R3; DCO Rulebook K5; help H4 ("Clearing: Kalshi Klear LLC (CFTC-regulated DCO)") |
| Kinetic Markets, LLC | FCM. NFA BASIC: "NFA Member Approved", "CFTC Registered: Futures Commission Merchant", NFA ID **0574784**, registered **03/24/2026** (pending since 11/21/2025), address 416 West 13th Street, New York, NY 10014, 0 regulatory actions | NFA BASIC R4 |
| Kalshi Inc. | Parent: "Kalshi Inc. is the parent entity of Kalshi Exchange, Kalshi Klear, and Kinetic Markets, LLC"; Kinetics "is a futures commission merchant ('FCM') that is **wholly owned by Kalshi Inc.**" | 155(k) K6; Affiliation disclosure K7 |

Kinetics principals per NFA BASIC (HIGH, R4): KALSHI INC (≥10% financial interest),
TAREK MANSOUR and LUANA LOPES LARA (indirect owners, ≥10%), LIOR SAMUEL HIRSCHFELD
(CEO), DAIHEE CHO (CFO), JAMES MICHAEL HILL (CCO). (Press named different CFO/CCO —
see Section 11.)

**Who does what (HIGH, K6 — 155(k) disclosure, as of June 3, 2026, verbatim):**
"Kinetic Markets will offer **only perpetual futures contracts** ... Kinetic Markets
will trade on KalshiEx, clear its trades through Kalshi Klear, and clear funds through
a fully-disclosed settlement process." "Kinetic Markets does not engage in proprietary
trading". "Kinetic Markets, LLC engages **Silicon Valley Bank, and Kalshi Klear, LLC**
for deposits of customers' funds." "Collateral and settlements are in **U.S. dollars
only**."

**Retail vs institutional access (MEDIUM, H12):** "Most retail users on Kalshi trade
perpetuals currently through Kinetics, a registered Futures Commission Merchant (FCM).
As an FCM, Kinetics handles your KYC, manages your margin account, monitors your risk
exposure, and holds your funds in a customer-segregated account on your behalf."
Institutions "may alternatively become Self-Clearing Members (SCMs) and clear their
trades directly with Kalshi Klear, bypassing the FCM layer" (contact
institutional@kalshi.com). The SCM ("Klear") API is a separate product —
`perps_scm_openapi.yaml` (HIGH, P3): bearer-token auth (`Authorization: Bearer
<admin_user_id>:<access_token>` from https://klearing.kalshi.com), host
`https://api.klear.kalshi.com/klear-api/v1` (demo `https://demo-api.kalshi.co/klear-api/v1`),
endpoints for margin reports, settlement obligations, settlement-buffer balance,
**guaranty fund balance**, and wire withdrawals; all monetary fields in **centicents**
(1 USD = 10,000 centicents).

**CFTC approval (HIGH, R1/R2):** Kalshi submitted BTCPERP 2026-05-28 under CEA
§5c(c)(4) / Reg 40.3 (voluntary approval, not just self-certification); the Commission
issued the Order of Approval 2026-05-29. The Order's operative custody language:
"**as futures contracts, customer positions in BTCPERP Contracts, and collateral
margining of such positions, shall be held in the futures account at both the futures
commission merchant and the derivatives clearing organization**." Scope is limited to
"the BTCPERP Contract and similarly structured perpetual contracts that reference the
spot price of bitcoin or other digital commodities that have deep, active, and
continuous spot market trading" — explicitly **not** other asset classes. Subsequent
non-BTC perps were self-certified under Reg 40.2(a) (LOW, press via search; the
certifications themselves were not located — Section 11).

**Account separation (MEDIUM, H6):** "Kalshi operates prediction markets (DCM) and
Kinetics facilitates perpetual futures trading (FCM). These are regulated differently,
and funds are held separately. Your predictions balance and your margin account do not
share funds and cannot affect each other. ... a liquidation event on a perps position
will never touch your predictions account balance." Margin "is held in a
customer-segregated account, separate from Kalshi's operating funds, as required by
CFTC regulations" (H6); 155(k) (HIGH, K6) cites CEA segregation / CFTC Rule 1.20 and a
Residual Interest Target Amount.

**Interest on margin (MEDIUM, H5/H6):** "Your margin earns interest while held at the
clearinghouse. The current rate is approximately **3.25% APY**, subject to market
conditions."

**Funding the margin account (MEDIUM, H13):** deposit via ACH/wire directly to the
margin account, or transfer from the predictions balance in the Portfolio tab;
"Margin currently allocated to open positions cannot be transferred until those
positions are closed"; credits/referrals are non-transferable between balances (H6).
The API transfer endpoint `POST /portfolio/intra_exchange_instance_transfer`
(event_contract ↔ margined) exists in the spec but "is currently not available"
(HIGH, P1/P5 — "enabled with the production rollout"). Its `amount` is **centicents**
(P1), while margin-subaccount transfers use `amount_cents` (P1) — unit mix, see §11.

**FCM financials (HIGH, K3 — kalshi.com/kinetics, retrieved 2026-06-11):** April 2026:
Net Capital $1,800,000; Net Capital Requirement $1,000,000; Excess $800,000.
Customer Segregated Funds (June 8, 2026): value held in segregation **$10,379,628**,
required $379,583, excess $10,000,045. 155(k) (K6): "The Firm generally maintains a
ratio of Adjusted Net Capital (ANC) to Regulatory Capital Required above 120%."

**Access gating (MEDIUM, H2/H12):** US-based KYC'd users; "Application required. You
must apply for margin access. Not all applicants are approved." Questionnaire +
"mandatory educational tutorial before placing your first trade." API-side: production
rollout is **gradual, member by member**; "call `GET /margin/enabled` to check whether
perps is enabled for your account in a given environment" (HIGH, P5).

## 3. Contract specifications

**Live production listing set (HIGH, L: `live_prod_margin_markets.json`, retrieved
2026-06-11 ~05:50 UTC; 14 markets):**

| Ticker | Title (= contract size) | contract_size | Status | leverage_estimate | Last price (per contract) |
|---|---|---|---|---|---|
| KXBTCPERP | 0.0001 BTC | 0.000100 | active | 5.89 | $6.2590 |
| KXETHPERP | 0.001 ETH | 0.001000 | active | 4.4984 | $1.6481 |
| KXLTCPERP | 0.1 LTC | 0.100000 | active | 3.6456 | $4.2642 |
| KXLINKPERP | 1 LINK | 1.000000 | active | 3.4037 | $7.7808 |
| KXDOTPERP | 10 DOT | 10.000000 | **inactive** | 3.3242 | — |
| KXBCHPERP | 0.01 BCH | 0.010000 | active | 3.0368 | $2.0051 |
| KXXRPPERP | 1 XRP | 1.000000 | active | 2.7942 | $1.1159 |
| KXDOGEPERP | 100 DOGE | 100.000000 | active | 2.7229 | $8.4909 |
| KXSOLPERP | 0.1 SOL | 0.100000 | active | 2.6924 | $6.5046 |
| KXHBARPERP | 100 HBAR | 100.000000 | **inactive** | 2.5065 | — |
| KXXLMPERP | 10 XLM | 10.000000 | **inactive** | 2.4935 | — |
| KXSUIPERP | 10 SUI | 10.000000 | active | 2.4862 | $7.5240 |
| KXHYPEPERP | 0.1 HYPE | 0.100000 | active | 2.1547 | $5.5345 |
| KXKSHIBPERP | 1K kSHIB (=1,000,000 SHIB) | 1000.000000 | active | 2.0063 | $4.7101 |

The kalshi.com learn page (MEDIUM, K1) shows the same per-asset max-leverage list and
says "Leverage values reflect current API data and may change without notice."
`leverage_estimate` is defined as "Leverage estimate (1 / margin_rate) evaluated at a
small retail-sized notional position. Actual leverage may be lower for larger positions
as the liquidation margin rate grows with size" (HIGH, P1). `leverage_estimates` keys
notionals "1000", "10000", "100000", "1000000" (P1) — leverage decays with size (demo
BTC example: 5.899 at $1k → 5.8143 at $1M; L: `live_demo_margin_markets.json`).

**BTC contract spec (MEDIUM, H4 — help-center table, verbatim values):**
Underlying: "Bitcoin (BTC) spot price in USD". Price index: "CF Benchmarks Bitcoin
Real-Time Index (BRTI)", "Index update frequency: 1 second". "Contract type: Linear
(USD-margined)". "Contract size: 0.0001 BTC per contract". "Min. position: 1 contract
(~$8 at current prices)". "Trading hours: 24/7 (with exception to maintenance)".
"Clearing: Kalshi Klear LLC (CFTC-regulated DCO)". "Settlement cycles: 12:00 PM ET and
4:00 PM ET daily". "Funding times: 12:00 AM, 8:00 AM, and 4:00 PM ET". "Funding rate
cap: +2% / –2% per 8-hour interval". "Margin type: Isolated (in Kalshi apps),
portoflio [sic] via API". The CFTC Order (HIGH, R1) confirms unit size: "The BTCPERP
Contract will trade in units of one ten-thousandth (1/10,000) of one bitcoin. It will
trade 24 hours per day, 7 days per week, throughout the lifetime of the contract,
subject to any trading halts imposed by the Exchange."

- **Quote convention**: prices are USD per contract as `FixedPointDollars` strings
  (HIGH, P1). E.g. BTCPERP at $6.2590/contract ⇔ BTC ≈ $62,590 (contract = 0.0001
  BTC). The market's `reference_price` is the "Underlying reference price, **scaled
  per contract**" (HIGH, P1).
- **Tick**: "For perpetual markets, prices move in `0.0001` dollar ticks" (HIGH, P6).
  Demo market objects report `tick_size: "0.0001"` (L); prod will report `tick_size`
  from the June 11, 2026 release (HIGH, P11 changelog). FIX: "Decimal dollars up to 4
  decimal places" (HIGH, P5).
- **Price banding (HIGH, P6, verbatim):** "Bids must be at least the lower of 80% of
  the best bid or 1,000 ticks below the best bid. Asks must be at most the higher of
  120% of the best ask or 1,000 ticks above the best ask." "Resting orders will not be
  canceled due to the price band movement." "If there are no resting orders on that
  side, there is no band limit for that side." "Order amends outside the price band
  are not allowed."
- **Minimum order / fractionality**: all 14 prod perps report
  `fractional_trading_enabled: false` (HIGH, L) → minimum order 1 contract, integer
  counts (LOW inference from the `FixedPointCount` schema: fractional 0.01 granularity
  applies only "on markets with fractional trading enabled", P1). Help confirms "Min.
  position: 1 contract" for BTC (MEDIUM, H4).
- **Max leverage / margin**: see §5. **Position limits: not published** — DCM Rulebook
  Rule 5.19 (HIGH, K4): "Kalshi may impose Position Limits on all Contracts, which
  will be specified in each contract's Terms and Conditions" — the perp contracts'
  Terms & Conditions documents were not publicly locatable (§11). The API exposes
  per-user **notional risk limits** instead: `GET /margin/notional_risk_limit` →
  `default_notional_value_risk_limit` (e.g. "5000.0000") plus per-market overrides
  (HIGH, P1).
- **Trading hours**: 24/7 except scheduled maintenance "on Thursdays from
  approximately 3:00 AM to 5:00 AM ET" — orders cannot be placed/modified, positions
  stay open, mark price does not change, funding scheduled during maintenance is
  processed when the exchange comes back online (MEDIUM, H6/H13). The site banner
  says maintenance 3–5 AM ET (K2/K3).
- **Market status enum**: `inactive | active | closed` (HIGH, P1
  `MarginMarketStatus`).
- **Demo listing set differs** (HIGH, L: `live_demo_margin_markets.json`): tickers
  carry a `1` suffix (KXBTCPERP1...), 17 markets including KXSHIBPERP1 (1M SHIB) and
  two **inactive test equity perps** (`TEST-EQUITY-AAPL-PERP`, `TEST-EQUITY-NVDA-PERP`)
  — note the CFTC order does **not** cover equity perpetuals (R1 footnote 25). Some
  demo contract sizes differ from prod: BCH 0.1 (vs 0.01), ETH 0.01 (vs 0.001), HYPE
  0.25 (vs 0.1), XLM 100 (vs 10), XRP 10 (vs 1).

## 4. Funding

- **Interval & times**: every 8 hours — "12:00 AM ET, 8:00 AM ET, 4:00 PM ET"
  (MEDIUM, H7; also "funding is charged every 8 hours" K1; "Funding rates are changed
  every eight hours" N1). **Empirically confirmed** (HIGH, L:
  `live_prod_funding_hist_btc.json` / `_all.json`): `funding_time` values are exactly
  04:00, 12:00, 20:00 UTC (= ET + 4h under EDT), 8h apart, across all markets.
- **Calculation (MEDIUM, H7, verbatim):** "Every 8 hours, Kalshi computes a
  **time-weighted average price (TWAP) of 1-minute candlestick premiums over the 480
  candles in that funding period**. This produces a single rate representing the
  average gap between the perp price and spot during that period." The API estimate
  endpoint description (HIGH, P1): "The value is a **time-weighted average of the
  premium index computed over `[last_funding_time, now)`**, so it continues to move as
  new data accumulates through the window and is only finalized at
  `next_funding_time`." The precise premium-index formula (which price vs which index)
  is not published — the market object distinguishes `settlement_mark_price` ("Mark
  price used for settlement **and funding**") from `liquidation_mark_price` and
  `reference_price` (HIGH, P1), implying premium = f(settlement mark, reference
  index); exact definition unverified (§11).
- **Cap (MEDIUM, H7/K1):** "The funding rate is clamped to **+2% / –2% per 8-hour
  interval**." VERIFIED as the claim made by two official surfaces; not yet observed
  at the cap in live data.
- **Zero threshold (MEDIUM, H7):** "If the absolute value of the rate is below
  **0.01%**, it is automatically set to zero and no payment is made." Consistent with
  live data: 64 of 100 recent funding records are exactly 0; nonzero values run
  ~±0.01–0.04% (HIGH, L: `live_prod_funding_hist_all.json`, e.g. KXBCHPERP
  2026-06-11T04:00Z `-0.0003971378687289`).
- **Index outage (MEDIUM, H7):** "If the CF Benchmarks price feed is unavailable,
  funding rates are paused until the feed is restored."
- **Sign conventions:** rate positive (perp above spot) → "long holders pay short
  holders"; negative → shorts pay longs; zero → no payment (MEDIUM, H7; HIGH, R1 for
  the same mechanics in the CFTC order). In the user-level API,
  `funding_amount` is the "Dollar amount of the funding payment (**positive =
  received, negative = paid**)" (HIGH, P1 `MarginFundingHistoryEntry`).
- **Accrual vs payment**: the estimate accrues continuously through the window and is
  "only finalized at `next_funding_time`" (HIGH, P1); payments apply at the funding
  times; funding due during maintenance "is processed when the exchange comes back
  online" (MEDIUM, H13). "Funding payments are transfers between traders, not fees
  charged by Kalshi" (MEDIUM, H7).
- **Funding rate units**: decimal fraction per 8h period (`funding_rate: number,
  double`, HIGH P1; observed e.g. `-0.000397` ≈ −0.0397%/8h).
- **API surface**: `GET /margin/funding_rates/estimate?ticker=` (public, returns
  `funding_rate`, `mark_price`, `computed_time`, required `next_funding_time`);
  `GET /margin/funding_rates/historical?ticker=&start_ts=&end_ts=` (public; ticker
  optional → all markets); `GET /margin/funding_history?start_date=&end_date=`
  (auth; the user's own funding payments joined with rates: `funding_rate`,
  `mark_price`, `funding_amount`, `quantity`, `subaccount_number`) (all HIGH, P1).
  WS `ticker` channel carries `funding_rate {rate, next_funding_time_ms, ts_ms}`
  (HIGH, P2).
- **Index per asset (MEDIUM, H3):** all Kalshi crypto perps "use CF Benchmarks
  indices as the reference price for funding and settlement". Per-asset (H3 table):
  BTC → "CF Benchmarks BRTI"; ETH → ETHUSD_RTI; SOL → SOLUSD_RTI; XRP → XRPUSD_RTI;
  DOGE → DOGEUSD_RTI; LINK → LINKUSD_RTI; DOT → DOTUSD_RTI; LTC → LTCUSD_RTI; BCH →
  BCHUSD_RTI; SUI → SUIUSD_RTI; XLM → XLMUSD_RTI; SHIB → SHIBUSD_RTI; HBAR →
  HBARUSD_RTI. (HYPE is live in prod but absent from the help table — gap, §11.)
  These are the **real-time** (RTI) indices, not the once-a-day reference rates: BRTI
  is "a once a second benchmark index price ... aggregates order data from
  Bitcoin-USD markets", "Registered Benchmark under UK BMR" (MEDIUM, W1). The CFTC
  Order (HIGH, R1): BRTI "provides a continuous measure of the U.S. dollar price of
  bitcoin, derived from observable transactions on major crypto asset trading
  platforms"; "CF Benchmarks is authorized by the UK Financial Conduct Authority under
  the EU/UK Benchmarks Regulation (FRN 847100), with compliance independently audited
  by KPMG under ISAE 3000 reasonable assurance."

## 5. Margin

- **Mode**: "**Isolated margin** is the only mode currently available in the Kalshi
  app. With isolated margin, the risk on each trade is ring-fenced to that position
  only." "**Portfolio margin** offers offsets across related perpetual crypto
  positions in a portfolio. ... allows for significant capital efficiencies in hedged
  portfolios." (MEDIUM, H5). The BTC spec table says "Margin type: Isolated (in Kalshi
  apps), portoflio [sic] via API" (MEDIUM, H4) — i.e. **API accounts appear to get
  portfolio margining**, consistent with the API's `GetMarginRiskResponse` exposing an
  "Estimated **portfolio-aware** liquidation price for this position within the
  subaccount" (HIGH, P1). Exact portfolio-margin methodology: unpublished (§11).
- **Initial vs maintenance**: "Initial margin is the amount you post to open a
  position. It is determined by the leverage you select and the notional size of your
  position. Maintenance margin is the minimum balance required to keep a position
  open." (MEDIUM, H5). System-wide parameters (HIGH, L:
  `live_prod_risk_parameters.json`): `initial_margin_multiplier` = **1.3 for all 14
  markets** ("The initial margin requirement is the maintenance margin multiplied by
  this value", HIGH P1), `liquidation_margin_ratio_threshold` = **1**,
  `queue_entry_margin_ratio_threshold` = **0.91**. The **maintenance-margin formula
  itself is not published**; it is size-dependent (the "liquidation margin rate grows
  with size", P1) and observable per-position via `GET /margin/risk`
  (`maintenance_margin_required`, `position_leverage`,
  `estimated_liquidation_price`) and `GET /margin/balance` (`maintenance_margin`,
  `initial_margin`) (HIGH, P1).
- **Margin-ratio direction** (LOW, inference): with thresholds 0.91 (queue entry) <
  1.0 (liquidation), the ratio is most plausibly maintenance-margin ÷ equity, rising
  as equity falls (queue first at 0.91, liquidate at 1.0). Direction is not defined in
  the docs — fixture-confirm (§12).
- **Margin currency**: USD only. "Collateral and settlements are in U.S. dollars
  only." (HIGH, K6). No stablecoin margin documented anywhere.
- **Unrealized PnL / available margin**: `MarginPosition.unrealized_pnl`
  ("Mark-to-market unrealized PnL"), `margin_used` ("Maintenance-margin-based capital
  usage"), `roe` (= unrealized_pnl / margin_used × 100) (HIGH, P1).
  `GET /margin/balance` returns per-subaccount `position_value`, `account_equity`,
  `maintenance_margin`, `initial_margin`, `resting_orders_margin`,
  `available_balance` plus top-level `settled_funds`; `available_balance` and
  `resting_orders_margin` are only computed when `compute_available_balance=true`
  (rate-limit cost **50 tokens** vs 5 normally — "the available-balance computation
  scans all resting orders") (HIGH, P1).
- **Variation margin / settlement cycles (MEDIUM, H5):** "Variation margin is the
  profit and loss credited or debited to your account multiple times a day. ...
  Kalshi Klear runs two daily settlement cycles at approximately **12:00 PM ET and
  4:00 PM ET**. During settlement, all positions are marked to a fresh **Volume
  Weighted Average Price** and any variation margin owed is settled." (The SCM API
  exposes the clearing-member view: `variation_margin_centicents`,
  `maintenance_margin_delta_centicents`, settlement obligations per cycle — HIGH, P3.)
- **Auto-deleveraging**: no ADL mechanism is documented anywhere in the fetched
  sources. Loss absorption beyond margin goes to the clearinghouse waterfall (§6).
  (Absence of evidence — flag, §11.)
- **Leverage selection (MEDIUM, H8):** the app lets you "select your leverage"
  (table shows 1x–6x examples); the API has **no leverage field** — you control
  leverage implicitly via position size against margin (HIGH, P1: CreateMarginOrder
  has no leverage/margin parameter). App marketing slider shows up to 10x (K1
  illustration) but actual per-asset caps are the API `leverage_estimate` values
  (~5.9x BTC max; K1: "each with its own maximum leverage").

## 6. Liquidation

- **Trigger (MEDIUM, H11):** "If the market moves against your position and your
  account equity drops below the maintenance margin, your position is at risk of
  liquidation. **Kalshi Klear monitors all positions continuously, 24/7.**" Two-step
  queue per risk parameters: queue entry at margin ratio 0.91, liquidation at 1.0
  (HIGH, L/P1; direction inference per §5).
- **Mechanics (MEDIUM, H11, verbatim):** "Kalshi Klear places opposing market orders
  to close the defaulting position as quickly as possible. Any remaining margin after
  the close is returned to your account. If the position closes with a deficit, the
  clearinghouse's risk waterfall absorbs it — your other margin funds are unaffected."
  The marketing page frames it as broker-run: "Liquidation is managed by your broker"
  (MEDIUM, K1); the 155(k) gives the FCM's right: "If a customer fails to meet margin
  requirements during market moves, **Kinetic Markets may liquidate positions without
  prior notice and at adverse prices**" (HIGH, K6). In the API, liquidation orders
  surface as `order_source: "system"` — "'system' indicates a system-generated
  liquidation order" (HIGH, P1 `OrderSource`) — and order updates can carry
  `last_update_reason: "MarginCancel"` (HIGH, P1/P2 enum).
- **Mark price**: "Liquidation is calculated against the mark price — an aggregated
  reference drawn from multiple spot markets — which reduces the risk of liquidation
  from brief price spikes on a single exchange" (MEDIUM, K1). The market object
  carries a dedicated `liquidation_mark_price` distinct from `settlement_mark_price`
  (HIGH, P1) — methodology for each is unpublished (§11).
- **Partial vs full**: not documented. Help describes closing "the defaulting
  position"; no partial-liquidation tiering is mentioned (§11).
- **Liquidation fee/penalty**: none documented (§11).
- **Loss waterfall / insurance**: "Kalshi therefore maintains a **waterfall based
  financial safeguard, including a default fund and its own contributions**, designed
  to help absorb shortfalls that may arise during default management" (MEDIUM, H5).
  SCM members post **guaranty fund** contributions (`GET /margin/guaranty_fund_balance`,
  HIGH, P3). No customer loss-mutualization/ADL is documented.
- **Negative balances (MEDIUM, K1 footer, verbatim):** "Rapid or extreme market
  movements ... may result in execution at prices significantly worse than the
  liquidation trigger, potentially producing a negative account balance. If your
  account balance becomes negative, **you are liable for the outstanding amount** and
  must deposit additional funds". Also "losses in black swan scenarios can exceed
  posted margin" (MEDIUM, H5; HIGH, K6 similar).
- **During maintenance (MEDIUM, H11):** "No. The mark price does not change during
  scheduled maintenance windows ... Liquidation monitoring resumes immediately when
  the exchange comes back online."
- **TP/SL**: app-only protective orders; "A stop loss ... should trigger before your
  liquidation price" (MEDIUM, H9). **The public perps REST/WS/FIX API has no TP/SL or
  trigger-order endpoints** (HIGH, P1/P2 — absent from specs). Stop prices "are not
  guaranteed execution prices" (K1 footer).
- **Resting-order constraint (MEDIUM, H8/H10):** "If you have an open resting limit
  order on a position, you cannot partially or fully close that position until the
  resting order is cancelled." Adapter-relevant ordering constraint.

## 7. Fees

- **Current state: zero.** "**Zero trading fees for a limited time on Kalshi
  Perpetuals.**" (HIGH-as-statement/MEDIUM-as-policy, K2 — kalshi.com/fee-schedule,
  retrieved 2026-06-11). Kinetics' Fee Disclosure Statement (HIGH, K8, as of June 3,
  2026): exchange fee per executed contract "**$0.00 (none)**"; "The Firm charges no
  commission, platform fees, account maintenance fees, inactivity feeds [sic], or
  market interest charges"; "The Firm passes through the Kalshi Exchange fee without
  markup." Press corroborates a launch fee holiday (LOW, search snippets).
- **Schedule shape (HIGH, P1/P11):** per-market maker/taker rates as **decimal
  fractions of notional** via authenticated `GET /margin/fee_tiers` →
  `maker_fee_rates` / `taker_fee_rates` maps ("e.g. 0.0005 = 0.05% = 5 bps. Multiply
  notional by this value to compute the fee" — maker example; "e.g. 0.0012 = 0.12% =
  12 bps" — taker example; **these are schema examples, not a published schedule**).
  Changelog May 11, 2026 (HIGH, P11): response changed from tier-name maps to rate
  maps ("e.g. `0.0008` = 0.08% = 8 bps. Compute the expected fee directly as
  `notional * rate`"). Changelog **June 11, 2026** (HIGH, P11): "`GET
  /trade-api/v2/margin/fee_tiers` now returns **active maker and taker fee rates** for
  each eligible margin market **instead of zeroing the response**" — i.e. real rates
  become visible the day after retrieval; **the actual post-promo numbers are not
  published anywhere fetched** (§11/§12).
- **Units**: bps of notional (decimal fraction × notional dollars), NOT per-contract
  cents and NOT the event-contract quadratic formula (HIGH, P1). Volume tiers: the
  fee-tier concept exists per user×market ("Returns a map of margin market tickers to
  their fee tier strings" — endpoint summary, P1), but tier thresholds are
  unpublished.
- **Fees in fills**: `MarginFill.fees` = "Fees paid on filled contracts, in dollars";
  order-create/amend responses include `average_fee_paid` "per contract ... Only
  present when fill_count > 0"; WS `fill.fee_cost` (HIGH, P1/P2).
- **Funding is not a fee** and passes between traders (MEDIUM, H7). **Maker rebates:
  none documented.** Withdrawal/transfer fees between event and margin accounts: none
  documented (transfers "processed promptly", H13; the unavailable API transfer
  endpoint carries no fee field, P1).
- App order-type framing: "Market orders pay the taker fee", resting limit orders
  "pay the maker fee, which is lower than the taker fee" (MEDIUM, H8).

## 8. API surface

### 8a. REST

- **Hosts (HIGH, P1 `servers` + P5):** production
  `https://external-api.kalshi.com/trade-api/v2` ; demo
  `https://external-api.demo.kalshi.co/trade-api/v2`. All perps paths live under the
  **`/margin/` namespace on the same trade-api host** as the event API. Production
  access is "rolling out member by member" (P5).
- **Auth (HIGH, P1 securitySchemes — identical to event API):** headers
  `KALSHI-ACCESS-KEY` ("Your API key ID"), `KALSHI-ACCESS-SIGNATURE` ("RSA-PSS
  signature of the request"), `KALSHI-ACCESS-TIMESTAMP` ("Request timestamp in
  milliseconds"). P5: "It mirrors the existing event contract API ... **same
  patterns, authentication, and conventions**, just under `/margin`." Same keys work
  for both event and perps REST (no separate perps key documented; FIX is the
  exception — separate keys, §8c). Signing payload presumed
  `timestamp + METHOD + path-without-query` per the event-API docs — re-verified for
  event API 2026-06-10; not independently restated in the perps docs → fixture (§12).
- **Endpoint catalog (HIGH, P1 — all paths in `perps_openapi.yaml`):**

  | Path | Method(s) | Auth | Notes |
  |---|---|---|---|
  | `/account/limits/perps` | GET | yes | perps rate-limit tier + read/write buckets |
  | `/margin/exchange/status` | GET | no | `{exchange_active, trading_active}` (live: both true — L) |
  | `/margin/enabled` | GET | yes | `{enabled}` — is perps on for this account |
  | `/margin/risk_parameters` | GET | no | liquidation/queue thresholds + IM multiplier map |
  | `/margin/markets` | GET | no | list; `status` filter (`inactive,active,closed`) |
  | `/margin/markets/{ticker}` | GET | no | single market |
  | `/margin/markets/{ticker}/orderbook` | GET | no | `depth` (0=all), `aggregation_tick_size` (dollars string) |
  | `/margin/markets/{ticker}/candlesticks` | GET | no | `start_ts`,`end_ts`,`period_interval` ∈ {1,60,1440} min |
  | `/margin/trades` | GET | no | public prints; `ticker` required; cursor pagination |
  | `/margin/orders` | GET, POST | yes | list (limit ≤ **10000**) / create |
  | `/margin/orders/{order_id}` | GET, DELETE | yes | get / cancel (`reduced_by` response) |
  | `/margin/orders/{order_id}/decrease` | POST | yes | `reduce_by` XOR `reduce_to` |
  | `/margin/orders/{order_id}/amend` | POST | yes | price and/or count; queue-position note below |
  | `/margin/fcm/orders` | GET | yes (FCM) | FCM members filter by `subtrader_id` |
  | `/margin/fills` | GET | yes | user fills; cursor |
  | `/margin/positions` | GET | yes | per-market signed positions |
  | `/margin/balance` | GET | yes | cost 5 tokens (50 w/ `compute_available_balance`) |
  | `/margin/risk` | GET | yes | account+position leverage, liquidation prices |
  | `/margin/notional_risk_limit` | GET | yes | per-user notional caps |
  | `/margin/fee_tiers` | GET | yes | maker/taker rate maps |
  | `/margin/funding_history` | GET | yes | own funding payments |
  | `/margin/funding_rates/historical` | GET | no | all-market or per-ticker history |
  | `/margin/funding_rates/estimate` | GET | no | in-progress period estimate |
  | `/margin/order_groups` (+ `/create`, `/{id}`, `/{id}/reset`, `/{id}/trigger`, `/{id}/limit`) | GET/POST/DELETE/PUT | yes | rolling-window contract limiter (kill-switch primitive) |
  | `/portfolio/margin/subaccounts` | POST | yes | create margin subaccount (1–63) |
  | `/portfolio/margin/subaccounts/transfer` | POST | yes | `client_transfer_id` (uuid, idempotency), `amount_cents` |
  | `/portfolio/intra_exchange_instance_transfer` | POST | yes | event↔margin; **"currently not available"**; `amount` in **centicents** |

  **Not available on margin** (HIGH, P5): batch order ops, queue positions,
  events/series/milestones/multivariate/structured targets, RFQs/quotes, historical
  data endpoints, exchange announcements/schedule.
- **Order create (HIGH, P1 `CreateMarginOrderRequest`):** required `ticker`,
  **`client_order_id` (required — stricter than event API)**, `side` (`bid`|`ask` —
  real two-sided book, no yes/no legs), `count` (FixedPointCount string), `price`
  (FixedPointDollars string), `time_in_force` (`fill_or_kill` | `good_till_canceled` |
  `immediate_or_cancel`), `self_trade_prevention_type` (`taker_at_cross` | `maker`).
  Optional: `expiration_time` (int64), `post_only`, `cancel_order_on_pause`,
  `reduce_only` ("Orders with reduce_only set to true **will be rejected unless
  time_in_force is immediate_or_cancel or fill_or_kill**"), `subaccount` (0–63),
  `order_group_id`. **No order `type` field — limit orders only**; market orders
  exist only in the app (MEDIUM, H8) — emulate with aggressive IOC. **No TP/SL.**
  Response 201 (HIGH, P1): `{order_id, client_order_id?, fill_count, remaining_count,
  average_fill_price?, average_fee_paid?}` (IOC: remaining_count "reflects the final
  state after unfilled contracts are canceled"). Errors declared: 400, 401, 409
  (Conflict — "resource already exists", i.e. duplicate client_order_id), 429, 500;
  `ErrorResponse {code, message, details, service}` (HIGH, P1).
- **Amend (HIGH, P1, verbatim note):** "Amending a resting order preserves queue
  position **only when the amendment decreases size**. All other amendments — like
  increasing size or changing price forfeit queue position". Request requires
  `ticker, side, price, count` (count = total/max fillable); supports
  `updated_client_order_id`.
- **Order object (HIGH, P1 `MarginOrder`):** `order_id, user_id, client_order_id,
  ticker, side, price, fill_count, remaining_count, last_update_reason` (enum: `""`,
  `Decrease`, `Amend`, `MarginCancel`, `SelfTradeCancel`, `ExpiryCancel`, `Trade`,
  `PostOnlyCrossCancel`), `expiration_time/created_time/last_update_time` (RFC3339,
  nullable), `self_trade_prevention_type`, `cancel_order_on_pause`, `order_group_id`,
  `order_source` (`user`|`system`). **No `status` field** — order state must be
  derived from counts/last_update_reason (difference from event API; fixture §12).
- **Orderbook (HIGH, P1):** real `bids` and `asks` arrays of
  `[price_dollars, count_fp]`; spec says "Bid price levels, ordered from best bid
  downward ... ask ... from best ask upward", but the **live response was ordered
  worst→best on both sides** (L: `live_prod_orderbook_btc.json` — bids ascending
  6.2569→6.2577, asks descending 6.2587→6.2578, best at array END) — conflict §11.
  Optional `aggregation_tick_size` ("e.g., 0.10 for 10 cent buckets").
- **Fills (HIGH, P1 `MarginFill`):** `fill_id, order_id, is_taker, side, count,
  created_time, ticker, price, entry_price` ("Position entry price used to compute
  incremental realized PnL"), `fees` (dollars), `realized_pnl` (per-fill incremental),
  `order_source`. Richer than the event-API fill (PnL attribution built in).
- **Positions (HIGH, P1 `MarginPosition`):** `market_ticker, position` ("positive =
  long, negative = short"), `entry_price` (weighted avg), `unrealized_pnl`,
  `margin_used`, `fees` (lifetime of current open position, "resets when position is
  fully closed"), `roe`.
- **Rate limits (HIGH, P12):** "perps traffic is metered in its own Read and Write
  buckets ... **up to four independent buckets**: event-contract Read, event-contract
  Write, perps Read, and perps Write." "Perps Write budgets match the [event] table at
  every tier. **Perps Read budgets are higher at the lower tiers: 400 at Basic and 600
  at Advanced**, versus 200 and 300 for event contracts. From Premier up they are
  identical." Check with `GET /account/limits/perps`. Default request cost 10 tokens
  (P1 RateLimitError description); balance = 5/50 (P1). Grants land on lanes
  `event_contract` | `margined` (HIGH, P1 `ExchangeInstance`, P12).
- **Pagination**: cursor-based, `limit` default 100 (max 1000; margin orders max
  10000) (HIGH, P1).
- **Idempotency**: `client_order_id` required on create; 409 Conflict declared.
  Subaccount transfers: `client_transfer_id` uuid "for idempotency" (HIGH, P1).

### 8b. WebSocket

- **Hosts (HIGH, P2 servers + P5):** production
  `wss://external-api-margin-ws.kalshi.com/trade-api/ws/v2/margin`; demo
  `wss://external-api-margin-ws.demo.kalshi.co/trade-api/ws/v2/margin` — **dedicated
  margin WS hosts**, distinct from event WS.
- **Auth (HIGH, P2/P8):** "Authentication is required during the WebSocket handshake"
  (apiKey scheme). Same three headers presumed; the exact string to sign for the
  margin WS path is not documented (event API signs
  `timestamp + "GET" + "/trade-api/ws/v2"`) → fixture (§12).
- **Channels (HIGH, P2):** `orderbook_delta`, `ticker`, `trade`, `fill`,
  `user_orders`, `order_group_updates`. Not available vs event WS (HIGH, P5):
  `market_positions`, `market_lifecycle_v2`, `multivariate*`, `communications`.
- **Commands (HIGH, P2):** `subscribe` / `unsubscribe` / `update_subscription`
  (`add_markets`/`delete_markets`, `sid` or single-element `sids`) /
  `list_subscriptions`; params `market_ticker`/`market_tickers`,
  `send_initial_snapshot` (ticker), `skip_ticker_ack`. Responses `subscribed`,
  `unsubscribed`, `ok`, `error {code: 1–18, msg, market_ticker?}` (P2 bounds the code
  range to 18; per-code meanings not enumerated in the perps spec).
- **Timestamps (HIGH, P5):** "all timestamp fields in margin WebSocket payloads use
  Unix epoch milliseconds and an `_ms` suffix" (no RFC3339 anywhere).
- **orderbook_delta (HIGH, P2):** snapshot `{market_ticker, bid: [[price,count_fp]],
  ask: [...]}` then deltas `{market_ticker, price, delta, side: bid|ask,
  last_update_reason?, client_order_id?, subaccount?, ts_ms}`; `sid`+`seq` for gap
  detection ("Sequence number used for snapshot/delta consistency"). Native bid/ask —
  no yes/no leg transform needed (unlike event API).
- **ticker (HIGH, P2):** "coalesced to at most one per market per second (latest
  value wins)". Required: `market_ticker, price, bid, ask, bid_size_fp, ask_size_fp,
  last_trade_size_fp, volume, volume_notional_value_dollars, volume_24h,
  volume_24h_notional_value_dollars, open_interest,
  open_interest_notional_value_dollars, ts_ms`; optional `reference_price`,
  `settlement_mark_price`, `liquidation_mark_price` (each `{price, ts_ms}`), and
  `funding_rate {rate, next_funding_time_ms, ts_ms}` — **mark prices + funding stream
  on the ticker channel**.
- **trade (HIGH, P2):** `{trade_id, market_ticker, price, count, taker_side, ts_ms}`.
- **fill (private; HIGH, P2):** `{trade_id, order_id, client_order_id?,
  market_ticker, is_taker, side, ts_ms, price, count, fee_cost, post_position,
  subaccount?}` — fee included on the WS fill (unlike event WS).
- **user_orders (private; HIGH, P2):** `{order_id, user_id, client_order_id, ticker,
  side, price, fill_count, remaining_count, created_ts_ms, last_updated_ts_ms?,
  expiration_ts_ms?, self_trade_prevention_type?, order_group_id?,
  subaccount_number?}`.
- **Keep-alive (HIGH, P2):** "Kalshi sends Ping frames every 10 seconds with body
  `heartbeat`. Clients should respond with Pong frames."

### 8c. FIX (existence + pointers)

(HIGH, P5/P9.) Separate gateway from event FIX. Prod order-entry/drop-copy host
`margin-fix-api.fix.elections.kalshi.com`; prod market data "Coming soon". Demo:
`margin-fix.demo.kalshi.co` (OE/DC) and `margin-marketdata.fix.demo.kalshi.co` (MD).
Ports/TargetCompIDs: 8228 KalshiNR, 8229 KalshiDC, 8230 KalshiRT, 8233 KalshiMD.
FIXT.1.1 / FIX50SP2, TLS ≥1.2, RSA-PSS-signed logon (`RawData` over
`SendingTime|MsgType|MsgSeqNum|SenderCompID|TargetCompID`), **SendingTime must be
within 30 seconds of server time**. "API keys **should not be shared** between the
event contract and margin FIX gateways" (P5). `UseDollars(21005)` always enabled —
"Decimal dollars up to 4 decimal places" vs integer cents on event FIX. FIX messages
draw from the same margin Read/Write token buckets; Mass Cancel limited to 1/s; no
RFQ/quotes or settlement reports on margin FIX. Docs: docs.kalshi.com/fix-margin/*
(archived P9).

## 9. Demo / sandbox — load-bearing for build sequence

- **Demo perps is open now**: "The demo environment is **open to everyone today**;
  production access is enabled in phases" (HIGH, P5). Confirmed empirically: demo
  REST serves the full `/margin/*` surface unauthenticated for public endpoints
  (HIGH, L: demo markets/risk/funding captures).
- Demo REST root `https://external-api.demo.kalshi.co/trade-api/v2/margin/`; demo WS
  `wss://external-api-margin-ws.demo.kalshi.co/trade-api/ws/v2/margin`; demo FIX hosts
  above (HIGH, P5). Demo accounts/keys are created at the demo web app with **mock
  funds**, same key-creation flow as prod (HIGH — verified for the event API research
  2026-06-10 from docs S3/S5/S6; the perps docs add no separate demo-signup step).
- **Fixtures can be recorded without real margin** — mock-fund demo accounts +
  the open demo rollout means order-lifecycle, balance, risk, funding-history and WS
  fixtures are all recordable today, subject to demo/prod divergences below.
- **Demo ≠ prod in important ways (HIGH, L):** tickers suffixed `1`
  (`KXBTCPERP1`); some contract sizes differ (§3); demo carries extra markets (SHIB,
  test equity perps); demo runs a **newer API build** — demo market objects already
  include `tick_size`, `leverage_estimates`, `settlement_mark_price`,
  `liquidation_mark_price`, `reference_price`, notional fields that prod will only
  gain in the **June 11, 2026** release (HIGH, P11 changelog: "Perps margin market
  responses now include mark prices", "Tick size added", notional fields); demo risk
  parameters differ (queue_entry 0.8 vs prod 0.91; HYPE/SHIB IM multiplier 1.1 vs
  1.3) (L: `live_demo_risk_parameters.json`). Record fixtures on demo, then re-verify
  shapes against prod read-only endpoints before live use.
- Production gating: even with the endpoints live, "your account may not have
  production perps access yet"; check `GET /margin/enabled` (HIGH, P5). Retail
  product access requires application + education (§2).

## 10. Data for strategy

- **Funding-rate history**: public, no auth, full history per market or all markets
  (`/margin/funding_rates/historical`, optional `start_ts`/`end_ts`; "If omitted,
  defaults to the earliest available data") (HIGH, P1). Product launched 2026-06-03,
  so history depth is ~1 week at retrieval. Live sample: 100 records / 5 days across
  14 markets; 36 nonzero; all nonzero rates negative (perp below spot — shorts paying
  longs) in the captured window, magnitudes 0.01–0.04% per 8h (HIGH, L).
- **Candlesticks**: public, 1m/1h/1d, with bid/ask OHLC, trade OHLC+mean(VWAP)+
  previous, volume, volume-notional, OI, OI-notional per period (HIGH, P1).
  1-minute candles matter because funding = TWAP of 1-minute premiums (§4).
- **Trades**: public, paginated, `taker_side` included (HIGH, P1).
- **Open interest / volume**: on every market object and WS ticker; dollar-notional
  companions added June 11 release (HIGH, P1/P11/L). Live snapshot 2026-06-11:
  BTCPERP OI 341,585 contracts (≈$2.1M notional at $6.259), lifetime volume 328.1M
  contracts; demo shows `open_interest_notional_value_dollars` directly (L).
  Press (LOW, N2 — CNBC, company-shared): ">$1 billion in [notional] trading volume
  within a week", ">$100 million in volume" first 24h, ~1M-person waitlist, vs 40
  months for event contracts to reach $1B.
- **Index data**: CF Benchmarks real-time indices (§4). Kalshi exposes a **CF
  Benchmarks REST passthrough**: `GET /trade-api/v2/cfbenchmarks/values?id=BRTI`
  forwarded to `https://www.cfbenchmarks.com/api/v1/`, authenticated with normal
  Kalshi signing, **cost 50 tokens/request**, "available only to accounts with the
  appropriate entitlement" (HIGH, P10). Index methodology docs at
  docs.cfbenchmarks.com; BRTI is order-book based, 1/second (MEDIUM, W1).
- **Basis-relevant venues for a US-regulated trader**:
  - CME (DCM): dated BTC/ETH futures — the standard regulated dated-futures leg vs
    Kalshi's perp leg (LOW — general market knowledge; CME's use of CF Benchmarks
    BRR/BRTI corroborated by W1: BRTI "serves as the price input for settlement of
    event contracts and prediction markets offered by Kalshi, CME Group and
    ForecastEX").
  - Coinbase Derivatives: "nano BTC and ETH perpetual contracts" (perpetual-**style**,
    5-year expirations, funding mechanism, up to 10x leverage), live since 2025-07-21
    via self-certification; on 2026-05-29 (same day as Kalshi's order) Coinbase
    "received approval from regulators to offer its U.S. traders access to global
    perp contracts through an affiliate" (LOW — press/search snippets + CNBC N2; the
    Coinbase blog itself returned 403 and was not archived).
  - Offshore perps (Binance etc.) are not accessible to US-regulated traders; the
    CFTC order itself notes the product was "entirely closed off to American
    institutions until now" framing (N1, MEDIUM).
- **Basis math caution**: Kalshi perp prices are per-contract (×contract_size to get
  asset price); funding accrues on `settlement_mark_price` (§4); CME settles to CF
  Benchmarks BRR (daily) vs Kalshi's real-time RTI family — different fixings (LOW,
  inference; verify before building a basis strategy).

## 11. Conflicts + unknowns

**Conflicts (source vs source, or source vs observation):**

1. **Orderbook ordering**: spec text says best-first ("ordered from best bid
   downward"); live prod returned both sides ordered worst→best (best at array end)
   (P1 vs L). Must fixture and code defensively (sort, don't assume).
2. **Help "Available assets" table column** (H3): lists "Contract Size 1 BTC / Min.
   Order Size 0.0001 BTC". The API (L), the BTC spec article (H4), and the CFTC order
   (R1) all say the **contract size is 0.0001 BTC** (= H3's "Min. Order Size"
   column). H3's "Contract Size" column (1 BTC, 10 ETH, 1,000 SOL...) matches no
   other source — treat as a mislabeled column (LOW).
3. **NFA ID**: NFA BASIC shows 0574784 (R4); Kinetics' own 155(k) text block prints
   "(NFA ID: 0577164)" (K6). Registry wins; the 155(k) figure is unexplained
   (possibly a different affiliate's ID or a typo).
4. **Kinetics Fee Disclosure wording** (K8) describes fees "in connection with
   **event contract trading** carried by the Firm" while the 155(k) (K6) says
   Kinetics offers "**only perpetual futures contracts**" — template error in the fee
   disclosure; the $0 figures still stand.
5. **Press vs registry on officers**: bitcoin.com-derived press named Sam Rosner
   (CFO) / Joshua Beardsley (CCO); NFA BASIC lists Daihee Cho (CFO) / James Michael
   Hill (CCO) (R4 wins).
6. **Prod vs demo market-object shape**: prod list/single responses currently omit
   `tick_size`, mark prices, `leverage_estimates`, notional fields that demo returns;
   changelog dates these prod additions to the June 11, 2026 release (P11). Until
   then prod parsing must treat them as optional.
7. **Demo BTC `tick_size` = ""** (empty string) while other demo markets show
   "0.0001" (L) — parser must tolerate empty/missing tick_size.
8. **App leverage slider shows up to 10×** in marketing copy (K1 illustrative; help
   table shows 6× rows) while actual caps are per-asset ~2–5.9× (API). Treat API
   `leverage_estimate(s)` as the truth.
9. **Funding "every 8 hours" vs "three times per day"** (H5 says "Funding rate
   adjustments occur three times per day") — consistent (3×8h=24h), but note the
   settlement cycles (2×/day, 12pm/4pm ET) are a separate clearing process that
   shares the 4 PM ET timestamp with one funding time — don't conflate.

**Unknowns / spec gaps (no fetched source answers these):**

- **Position limits** for perp contracts (Rulebook Rule 5.19 defers to per-contract
  Terms & Conditions; the perp T&C documents / 40.3 submission appendices were not
  publicly locatable; the product-certifications page lists no PERP cert PDFs in its
  anchor list). Per-user `notional_risk_limit` exists but its default value and
  adjustment process are undocumented.
- **Maintenance-margin formula** (size-dependent margin rate curve) — only observable
  via `leverage_estimates` and `GET /margin/risk`.
- **Margin-ratio direction** for the 0.91/1.0 thresholds (inference only, §5).
- **Post-promo fee schedule numbers** (active rates appear via API June 11, 2026;
  the schema's 5/12/8 bps figures are examples only).
- **Premium-index definition** for funding (which mark vs which index, candle source).
- **Mark-price methodologies** (`settlement_mark_price` vs `liquidation_mark_price`
  construction; K1 says liquidation mark is "an aggregated reference drawn from
  multiple spot markets").
- **Partial liquidation, liquidation fees/penalties, ADL** — not documented at all.
- **REST timestamp skew tolerance** for perps (FIX documents 30s; REST silent — same
  gap as the event API).
- **Margin WS signing path string** (`/trade-api/ws/v2/margin` presumed, not stated).
- **Margin WS error-code meanings** (P2 bounds codes 1–18 but doesn't enumerate; the
  event-API table may not transfer).
- **`GET /margin/orders` `status` filter vocabulary** (generic "Possible values
  depend on the endpoint"; MarginOrder has no status field).
- **HYPE's reference index** (absent from H3's table; CFB ticker for Hyperliquid
  unknown). Also whether each non-BTC perp was 40.2-self-certified vs 40.3-approved.
- **WS connection/session limits for the margin WS host** (event WS default 200
  connections; nothing stated for margin).
- **Whether unauthenticated REST access to public margin endpoints is rate-limited**
  (we observed it working; budget unknown).
- **Interest mechanics** (3.25% APY: accrual basis, payment cadence, which balance).

## 12. Operator fixture requests

Demo account (mock funds, open access) is sufficient for all of these except where
marked PROD. Archive raw request/response pairs under `fixtures/kinetics-perps/`.

1. **Auth round-trip on perps REST**: signed `GET /margin/enabled` and
   `GET /margin/balance` on demo — confirm the event-API signing recipe
   (`ts + METHOD + /trade-api/v2/margin/...`, no query) works unchanged; capture 401
   bodies for bad signature / stale timestamp; probe skew at ±5s/±30s/±5min.
2. **Margin WS handshake**: connect to
   `wss://external-api-margin-ws.demo.kalshi.co/trade-api/ws/v2/margin` — confirm the
   signed path string (try `/trade-api/ws/v2/margin`, fall back to
   `/trade-api/ws/v2`); capture `subscribed`, orderbook snapshot+delta with `seq`,
   ticker with `funding_rate` + mark prices, heartbeat ping/pong.
3. **Order lifecycle**: create (GTC limit, post_only on/off), amend (both
   queue-keeping decrease and queue-losing price change), decrease, cancel; capture
   201/200 bodies, `average_fill_price`/`average_fee_paid` presence rules, and the
   user_orders/fill WS events incl. `last_update_reason` transitions.
4. **client_order_id duplicate** → exact 409 `ErrorResponse.code` string; whether a
   canceled order's client_order_id frees up.
5. **reduce_only with GTC** → confirm rejection + exact error body (spec says only
   IOC/FOK allowed).
6. **Insufficient margin order** → exact 400 code/message.
7. **Price-band violation** (bid < band) and off-tick price (e.g. "6.25905") → exact
   error bodies.
8. **Orderbook ordering**: capture `GET /margin/markets/{t}/orderbook` at depth 0 and
   5 and document actual sort order (conflict §11.1); test `aggregation_tick_size`.
9. **Position + risk surfaces with an open position**: `GET /margin/positions`,
   `/margin/balance?compute_available_balance=true`, `/margin/risk` — verify
   margin_used vs risk_parameters math (IM = 1.3×MM), liquidation-price plausibility,
   and the margin-ratio direction inference (§5).
10. **Funding payment**: hold a small demo position across a funding time (04/12/20
    UTC); capture `GET /margin/funding_history` entry sign vs position direction.
11. **Fee fields once fees go live** (post June 11): `GET /margin/fee_tiers` actual
    maker/taker rates per market (PROD read-only too); fill `fees` vs rate×notional
    reconciliation incl. rounding.
12. **Order groups as runaway rail**: create group with `contracts_limit`, attach
    orders, trigger, reset; capture `order_group_updates` WS events — candidate
    venue-side I3 backstop.
13. **Subaccount transfer idempotency**: same `client_transfer_id` twice → behavior.
14. **`GET /margin/orders` status filter**: probe accepted values; how
    canceled/executed orders are represented without a status field.
15. **Exchange status during the Thursday 03:00–05:00 ET window** (PROD, read-only):
    `/margin/exchange/status` flips; behavior of resting orders +
    `cancel_order_on_pause`.
16. **Intra-exchange transfer** (`/portfolio/intra_exchange_instance_transfer`):
    confirm still 4xx "not available" and capture the body (it's the future
    event↔margin rail; PROD gating may differ).
17. **PROD parity sweep** (read-only, after June 11 release): re-capture
    `/margin/markets`, `/margin/risk_parameters`, fee tiers, and one orderbook; diff
    against demo fixtures.
18. **Operator-only questions for Kalshi support/institutional**: position limits per
    perp contract (T&C documents), post-promo fee schedule, premium-index formula,
    mark-price methodologies, ADL existence, margin WS connection limits.

## 13. Checklist for the adapter build (Phase B, after fixtures)

1. [ ] Config: venue `kinetics-perps`; hosts = existing trade-api hosts (REST) +
   dedicated margin WS host; demo/prod selection mirrors the event adapter; reuse
   RSA-PSS signer (same headers); **separate FIX keys if FIX ever built** (not Phase
   B).
2. [ ] Market catalog: poll `GET /margin/markets` (public); parse
   `contract_size` (6dp string), `tick_size` (tolerate ""/absent pre-June-11 prod),
   `status` (`inactive|active|closed`), `leverage_estimate(s)`, mark prices
   (optional), notional fields (optional). Asset price = contract price ÷
   contract_size only for display; all money in Cents/Decimal at boundaries per house
   rules — note prices have 4dp ($0.0001 tick): the `Cents` newtype is insufficient;
   plan a `Microdollars`/`Decimal` price type decision in the design doc.
3. [ ] Order entry: `POST /margin/orders` with required
   `client_order_id` (ULID→string), `side` bid/ask, `count`/`price` as strings,
   explicit `time_in_force` + `self_trade_prevention_type`; enforce in the gate
   pipeline: price within band (§3), on-tick, count ≥ 1 integer (fractional disabled),
   reduce_only⇒IOC/FOK rule; map 409 to idempotent-replay semantics only after
   fixture 4 confirms code strings.
4. [ ] Cancel/amend: `DELETE /margin/orders/{id}` → `reduced_by`; amend forfeits
   queue position except pure decreases — prefer decrease over amend for size-downs;
   reconcile every mutation via `GET /margin/orders/{id}` (no status field — derive
   state from `remaining_count`/`fill_count`/`last_update_reason`).
5. [ ] Positions/risk loop: poll `/margin/positions` + `/margin/risk` +
   `/margin/balance` (budget the 5-vs-50-token cost; only pass
   `compute_available_balance=true` at low cadence); gate new entries on
   maintenance-margin headroom with our own conservative margin model until the venue
   MM formula is reverse-engineered from fixtures.
6. [ ] Funding: ingest `/margin/funding_rates/historical` (all markets, public) into
   the ledger on schedule; join `/margin/funding_history` (auth) for realized
   transfers; treat funding as a cashflow event type distinct from fills; alarm if
   |estimate| approaches the ±2% cap or if `next_funding_time` drifts from the
   04/12/20 UTC grid (DST: re-derive from API, never hardcode UTC hours).
7. [ ] Fees: read `/margin/fee_tiers` at startup and on a timer; compute expected fee
   = notional × rate, reconcile against fill `fees` per fill; alert on schedule
   change (it changes "without notice" per K8).
8. [ ] Market data: Phase-B REST polling of orderbook (sort levels ourselves —
   §11.1) + trades; Phase-C WS (`orderbook_delta` with seq-gap resubscribe, `ticker`
   for marks/funding, `fill`/`user_orders` private). All margin WS timestamps are
   `_ms` integers.
9. [ ] Safety rails mapping: I3 runaway → venue order groups (15s rolling contract
   limit + trigger) as a backstop behind our own token buckets; I2/halt →
   `/margin/exchange/status` + `cancel_order_on_pause=true` on all resting orders;
   kill switch stays fully out-of-band per I4 (venue order-group trigger is an extra,
   not the primary).
10. [ ] Liquidation awareness: subscribe `user_orders`/`fill` and flag
    `order_source=system` / `last_update_reason=MarginCancel` as
    liquidation events → immediate halt of strategy + operator alert (a liquidation
    means our margin model failed).
11. [ ] Maintenance window: scheduler must treat Thu 03:00–05:00 ET as no-trade,
    expect WS disconnects, not panic on stale marks (mark prices freeze), and expect
    funding catch-up after reopen.
12. [ ] Account separation: perps margin balance is a separate ledger account from
    the event-contract balance; transfers are operator actions only (and the API rail
    is disabled anyway).
13. [ ] DST scenarios to add: duplicate-create retry after timeout (409 path),
    cancel-then-fill race (cancel returns but fill WS arrives), seq gap in
    orderbook_delta, funding event during open position, liquidation order observed,
    fee-rate flip mid-session, maintenance-window order rejection, demo→prod ticker
    remap (`KXBTCPERP1` vs `KXBTCPERP`).
14. [ ] Promotion gate (I7): paper/shadow on demo first; prod read-only parity sweep
    (fixture 17) must pass before any prod write path is enabled; prod
    `GET /margin/enabled` must be true and operator-confirmed.

---

*Research complete 2026-06-11 ~06:10 UTC. 40 docs pages + 3 API specs + 5 regulatory
PDFs + 2 rulebooks + 13 help articles + 12 live API captures archived under `raw/`.
See `SOURCES.md` for the full manifest.*
