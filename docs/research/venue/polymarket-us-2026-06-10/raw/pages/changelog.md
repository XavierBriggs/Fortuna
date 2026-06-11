> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Changelog

> Updates and Announcements for all APIs, tagged and filterable

### Subscribe to all Changes:

* Add to any RSS reader using the URL: `https://docs.polymarket.us/changelog/rss.xml`
* Slack has a built-in reader: use `/feed subscribe https://docs.polymarket.us/changelog/rss.xml`

<Update label="June 9, 2026" description="v0.0.44" tags={["Maintenance", "Upcoming", "Institutional API", "Retail API"]} rss={{ title: "v0.0.44 - One-off maintenance window Thursday, June 11, 3am-5am EST", description: "FYI: a maintenance window is scheduled for Thursday, June 11 from 3:00am-5:00am EST. Please note the time change as this is different than our normal hours. This is a one-off time change." }}>
  - **Maintenance window — Thursday, June 11, 3:00am–5:00am EST.** Please note the time change, as this is different than our normal hours. This is a one-off time change.
</Update>

<Update label="June 9, 2026" description="v0.0.43" tags={["Upcoming", "Institutional API", "Retail API"]} rss={{ title: "v0.0.43 - Full partial-contract rollout moved to June 11", description: "Amendment to the previous partial-contract rollout notice: we heard feedback that some users needed more time to fully migrate, so full rollout has moved to Thursday, June 11, 2026 at 5:00 PM ET (21:00 UTC). Long-dated futures listed before then and all World Cup instruments are partial-contract markets." }}>
  We heard feedback from some of our users that they needed more time to fully migrate to partial contracts, so we pushed back full rollout. All newly listed instruments will become partial-contract markets on **Thursday, June 11, 2026 at 5:00 PM ET (21:00 UTC)**. Long-dated futures markets listed before then and all World Cup instruments are partial-contract markets, so all market makers and API users should support partial-contract instruments now.
</Update>

<Update label="June 6, 2026" description="v0.0.42" tags={["New Feature", "Institutional API", "Retail API"]} rss={{ title: "v0.0.42 - NHL hockey market types", description: "NHL hockey sports market types are live: game-to-overtime, game-to-double-overtime, and player goals, assists, and points. Read sportsMarketType from the Retail market object or market_sport_type from Institutional instrument reference data." }}>
  * **NHL hockey market types are live.** The following enums are added to `market_sport_type` (Retail `sportsMarketType`):
    * **Game:**
      * `hockey_game_overtime` (will the game go to overtime?)
      * `hockey_game_double_overtime` (will the game go to double overtime?)
    * **Player props:**
      * `hockey_player_goals`
      * `hockey_player_assists`
      * `hockey_player_points`
  * **Where to read it:** Retail — `sportsMarketType` from `GET /v1/market/slug/{slug}`; Institutional — `market_sport_type` from `SearchInstruments` / `GetInstrument`.
</Update>

<Update label="June 6, 2026" description="v0.0.41" tags={["New Feature", "Institutional API", "Retail API"]} rss={{ title: "v0.0.41 - UFC market types", description: "UFC sports market types are live: method of victory, go the distance, round of victory, round of finish, and method of finish. Read sportsMarketType from the Retail market object or market_sport_type from Institutional instrument reference data." }}>
  * **UFC market types are live.** The following enums are added to `market_sport_type` (Retail `sportsMarketType`):
    * `ufc_method_of_victory`
    * `ufc_go_the_distance`
    * `ufc_round_of_victory`
    * `ufc_round_of_finish`
    * `ufc_method_of_finish`
  * **Where to read it:** Retail — `sportsMarketType` from `GET /v1/market/slug/{slug}`; Institutional — `market_sport_type` from `SearchInstruments` / `GetInstrument`.
</Update>

<Update label="June 6, 2026" description="v0.0.40" tags={["New Feature", "Institutional API", "Retail API"]} rss={{ title: "v0.0.40 - Soccer market types", description: "Soccer sports market types are live: full-game and first-half spread/total, full-time and first-half winner, both teams to score, first team to score, exact score, total corners, plus player goals and assists. Read sportsMarketType from the Retail market object or market_sport_type from Institutional instrument reference data." }}>
  * **Soccer market types are live.** The following enums are added to `market_sport_type` (Retail `sportsMarketType`):
    * **Team — full game:**
      * `soccer_team_full_time_winner`
      * `soccer_team_full_game_spread`
      * `soccer_team_full_game_total`
    * **Team — first half:**
      * `soccer_team_first_half_winner`
      * `soccer_team_first_half_spread`
      * `soccer_team_first_half_total`
    * **Game props:**
      * `soccer_game_btts` (both teams to score)
      * `soccer_game_first_team_to_score`
      * `soccer_game_exact_score`
      * `soccer_game_total_corners`
    * **Player props:**
      * `soccer_player_goals`
      * `soccer_player_assists`
  * **Where to read it:** Retail — `sportsMarketType` from `GET /v1/market/slug/{slug}`; Institutional — `market_sport_type` from `SearchInstruments` / `GetInstrument`.
</Update>

<Update label="June 5, 2026" description="v0.0.39" tags={["Incentives"]} rss={{ title: "v0.0.38 - NBA Playoffs props expansion + moneyline Live reduction + World Cup start", description: "NBA Playoffs props rewards expanded to $20,000/game across Player, Game, and Other props, and moneyline Live reduced from $56,500 to $50,000 per game (effective 1pm ET, Friday June 5). World Cup liquidity rewards start time updated to 6pm ET, Thursday June 4." }}>
  * **NBA Playoffs props expansion (effective 1:00pm ET, Friday June 5):** Props pool increased from \$10,000 → \$20,000 per game. Categories reorganized:
    * **Player Props:** \$10,000/game (\$5,000 Day-of + \$5,000 Live).
    * **Game Props (new):** \$5,000/game (\$2,500 Day-of + \$2,500 Live).
    * **Other Props (new):** \$5,000/game (\$2,500 Day-of + \$2,500 Live).
    * **Team Props removed.**
  * **NBA Playoffs Moneyline reduction (Live):** \$56,500 → **\$50,000** per game.
  * **NBA Playoffs total liquidity per game:** \$100,000 → **\$103,500** (\$83,500 moneyline/spreads/totals + \$20,000 props).
  * **NBA Pool row updated:** Early \$4,000 / Day-of \$14,000 / Live \$85,500.
  * **Props discount factor and target size** are consistent across all categories: **0.35 / 2,500** for both Day-of and Live.
  * **World Cup liquidity rewards:** start time pushed to **6:00pm ET, Thursday June 4** (was June 3).
</Update>

<Update label="June 4, 2026" description="v0.0.38" tags={["Upcoming", "Institutional API", "Retail API"]} rss={{ title: "v0.0.38 - All new instruments are partial contracts starting June 8, 2026", description: "Starting Monday, June 8, 2026 at 12:00 PM EST, all newly listed instruments will be partial-contract markets. Do not assume whole-contract quantities. Derive the partial scale per instrument: Institutional reads instrument.fractionalQtyScale and instrument.minimumTradeQty; Retail reads minimumTradeQty from the market object and handles decimal quantity fields." }}>
  * **Starting Monday, June 8, 2026 at 12:00 PM EST, every newly listed instrument will be a partial-contract market.** Existing instruments are unchanged. Do not assume whole-contract quantities — derive the partial scale per instrument before submitting orders.
  * **Institutional API — derive the scale, then convert:**
    * `instrument.fractionalQtyScale` — divide raw integer quantities by this to get decimal contracts. For example, with `fractionalQtyScale == 100`, `quantity = 1` is `0.01` contracts and `quantity = 100` is `1` full contract.
    * `instrument.minimumTradeQty` — the smallest tradable integer quantity.
    * Initial partials use `fractionalQtyScale == 100` and `minimumTradeQty == 1`, so the minimum order is **1% of a contract**.
    * On the `Order` message, `fractional_quantity_scale` (field 49) carries the same scale for converting `order_qty`, `cum_qty`, and `leaves_qty`.
  * **Retail API — read the minimum, then handle decimals:**
    * `minimumTradeQty` on the market object (for example `GET /v1/market/slug/{slug}`) is expressed in contracts, so `0.01` means a **1%-of-a-contract** minimum.
    * Treat `quantity`, `cumQuantity`, and `leavesQuantity` as decimals, and use the decimal portfolio fields (`netPositionDecimal`, `qtyBoughtDecimal`, …).
</Update>

<Update label="June 2, 2026" description="v0.0.37" tags={["Upcoming", "Institutional API", "Retail API"]} rss={{ title: "v0.0.37 - NBA quarter markets and game-to-overtime", description: "Basketball per-quarter (1st-4th) spread and total markets plus a game-to-overtime market are live in preprod now and deploy to production by midnight EST on June 2, 2026. The 4th-quarter markets exclude overtime. Read sportsMarketType from the Retail market object or market_sport_type from Institutional instrument reference data." }}>
  * **NBA quarter spread + total markets and game-to-overtime (preprod now, production by midnight EST on June 2, 2026).** The following enums are added to `market_sport_type` (Retail `sportsMarketType`):
    * **Spread:** `basketball_team_first_quarter_spread`, `basketball_team_second_quarter_spread`, `basketball_team_third_quarter_spread`, `basketball_team_fourth_quarter_spread`
    * **Total:** `basketball_team_first_quarter_total`, `basketball_team_second_quarter_total`, `basketball_team_third_quarter_total`, `basketball_team_fourth_quarter_total`
    * **Game-to-overtime:** `basketball_game_overtime`
  * **Resolution:** each quarter market settles on points scored in that quarter only; 4th-quarter markets exclude overtime. The game-to-overtime market settles at the conclusion of the game.
  * **Example slugs (NBA Finals, New York vs. San Antonio, `nba-ny-sa-2026-06-03`):**
    * 1st quarter spread: `asc-nba-ny-sa-2026-06-03-q1-neg-2pt5`, total: `tsc-nba-ny-sa-2026-06-03-q1-56pt5`
    * 4th quarter spread (excl. OT): `asc-nba-ny-sa-2026-06-03-q4-neg-1pt5`, total: `tsc-nba-ny-sa-2026-06-03-q4-51pt5`
    * Game-to-overtime: `astatc-nba-ny-sa-2026-06-03-ot`
  * **Where to read it:** Retail — `sportsMarketType` from `GET /v1/market/slug/{slug}`; Institutional — `market_sport_type` from `SearchInstruments` / `GetInstrument`.
</Update>

<Update label="June 1, 2026" description="v0.0.36" tags={["Upcoming", "Institutional API", "Retail API"]} rss={{ title: "v0.0.36 - NBA second half team markets and new player props", description: "Basketball second-half spread, total, and moneyline team markets plus six new player props (rebounds, three-pointers made, steals, blocks, double-double, triple-double) are live in preprod now and deploy to production on June 2, 2026 at 6:00 PM EST. Read sportsMarketType from the Retail market object or market_sport_type from Institutional instrument reference data." }}>
  * **NBA second half + new player props (preprod now, production June 2, 2026 at 6:00 PM EST):** Basketball second-half team markets and six additional player props are live in preprod and will be deployed to production on **June 2, 2026 at 6:00 PM EST**. The following enums are added to the instrument `market_sport_type` field (Retail `sportsMarketType`):
    * **Team — second half:**
      * `basketball_team_second_half_winner`
      * `basketball_team_second_half_spread`
      * `basketball_team_second_half_total`
    * **Player props:**
      * `basketball_player_rebounds`
      * `basketball_player_threes`
      * `basketball_player_steals`
      * `basketball_player_blocks`
      * `basketball_player_double_double`
      * `basketball_player_triple_double`
  * **Example slugs (NBA Finals, New York vs. San Antonio, `nba-ny-sa-2026-06-03`):**
    * **Team — second half:**
      * 2H moneyline: `atc-nba-ny-sa-2026-06-03-sh-ny`, `atc-nba-ny-sa-2026-06-03-sh-sa`, `atc-nba-ny-sa-2026-06-03-sh-draw`
      * 2H spread: `asc-nba-ny-sa-2026-06-03-sh-neg-10pt5`, `asc-nba-ny-sa-2026-06-03-sh-pos-1pt5`
      * 2H total: `tsc-nba-ny-sa-2026-06-03-sh-105pt5`
    * **Player props** (each strike is its own market; player segment is first-3-of-first + first-3-of-last name):
      * Rebounds: `astatc-nba-ny-sa-2026-06-03-reb-vicwem-gte11`
      * Three-pointers made: `astatc-nba-ny-sa-2026-06-03-threes-jalbru-gte3`
      * Steals: `astatc-nba-ny-sa-2026-06-03-stl-jalbru-gte2`
      * Blocks: `astatc-nba-ny-sa-2026-06-03-blk-vicwem-gte2`
      * Double-double: `astatc-nba-ny-sa-2026-06-03-dd-vicwem-gte1`
      * Triple-double: `astatc-nba-ny-sa-2026-06-03-td-vicwem-gte1`
  * **Resolution — second half excludes overtime:** Second-half team markets (spread, total, moneyline) settle on points scored in the **third and fourth quarters only**; overtime is **not** included.
  * **Resolution — player props:** The new counting props (rebounds, three-pointers made, steals, blocks) settle on full-game box-score totals **including overtime**, consistent with the existing points and assists props. **Double-double** and **triple-double** resolve **Yes/No** — Yes when the player records 10 or more in at least two (double-double) or three (triple-double) of points, rebounds, assists, steals, or blocks.
  * **Where to read it:**
    * **Retail API** — read `sportsMarketType` from the market object (for example `GET /v1/market/slug/{slug}`).
    * **Institutional API** — read `market_sport_type` from instrument reference data (`SearchInstruments` / `GetInstrument`).
</Update>

<Update label="May 31, 2026" description="v0.0.35" tags={["Upcoming", "Retail API", "Institutional API"]} rss={{ title: "v0.0.35 - NBA Finals 0.5c tick sizes", description: "New York vs. San Antonio Game 2 of the NBA Finals, slug aec-nba-ny-sa-2026-06-05, will be listed on Monday, June 1, 2026 and will be the first market with a 0.5 cent tick size. The remainder of NBA Finals markets will also use 0.5 cent ticks. Clients should read tick size from the Retail market object or Institutional instrument reference data before submitting orders." }}>
  * **NBA Finals Game 2:** New York vs. San Antonio Game 2 of the NBA Finals (`aec-nba-ny-sa-2026-06-05`) will be listed on **Monday, June 1, 2026** and will be the first market with a **0.5 cent** tick size (`$0.005`). The remainder of NBA Finals markets will also use **0.5 cent** ticks.
  * **Retail API:** read `market.orderPriceMinTickSize` from the market response before submitting orders. For this market, use `GET /v1/market/slug/aec-nba-ny-sa-2026-06-05` and expect `orderPriceMinTickSize: 0.005`.
  * **Institutional API:** read `instrument.tickSize` from instrument reference data (`SearchInstruments` / `GetInstrument`). For this instrument, `instrument.tickSize = 0.005`.
  * **Institutional price scale:** prices submitted to the Institutional API are integer values. Read `instrument.priceScale` from the same instrument reference data response and divide submitted or returned integer prices by that value to get dollar prices. For example, if `instrument.priceScale == 1000`, `price = 5` means `$0.005`, `price = 500` means `$0.50`, and `price = 1000` means `$1.00`. With a 0.5 cent tick and `priceScale == 1000`, valid integer prices move in 5-unit increments.
  * **Action recommended:** do not assume 1 cent ticks. Read tick size and price scale per market or instrument before validating or submitting orders.
</Update>

<Update label="May 30, 2026" description="v0.0.34" tags={["New Feature", "Retail API"]} rss={{ title: "v0.0.34 - Retail API partial contracts and decimalized tick sizes", description: "Retail API schemas now document decimal order quantities, per-market minimumTradeQty, per-market orderPriceMinTickSize, decimal portfolio quantity fields, and legacy field deprecations for partial-contract markets." }}>
  * **Markets API:** market responses now document `minimumTradeQty` alongside `orderPriceMinTickSize`.
    * Applies to `GET /v1/markets`, `GET /v1/market/id/{id}`, `GET /v1/market/slug/{slug}`, and documented Retail API responses that embed the market object, including Events, Search, Sports, Sports Legacy, and Subjects.
    * `minimumTradeQty` is expressed in contracts. For example, `0.01` means the minimum order size is 1% of a contract.
    * `orderPriceMinTickSize` is expressed in dollars. For example, `0.005` means half-cent ticks.
  * **Market data:** order book and trade quantity fields can contain decimal contract quantities.
    * `GET /v1/markets/{slug}/book` and Markets WebSocket book levels return `qty` as a decimal string.
    * Markets WebSocket trade `quantity.value` is also a decimal string.
  * **Orders API:** order `quantity` fields support decimal contract quantities on partial-contract markets.
    * Applies to `POST /v1/orders`, `POST /v1/order/preview`, `POST /v1/order/{orderId}/modify`, `POST /v1/orders/batched`, and `POST /v1/orders/batched/modify`.
    * Order request and response `quantity`, `cumQuantity`, and `leavesQuantity` fields are JSON numbers and can contain decimals.
    * Private WebSocket order snapshots and updates use the same order quantity fields; execution `lastShares` is a decimal string.
    * Multi-leg execution `legPrices[].qty` is a decimal string.
    * Submit prices and quantities already aligned to the market's documented precision. Extra precision can be normalized in responses rather than rejected.
  * **Portfolio API:** use decimal quantity fields for positions and trades.
    * `GET /v1/portfolio/positions` returns `netPositionDecimal`, `qtyBoughtDecimal`, `qtySoldDecimal`, `bodPositionDecimal`, and `qtyAvailableDecimal`.
    * `GET /v1/portfolio/activities` trade payloads return `qtyDecimal`; the older trade `qty` field is rounded and deprecated.
    * Private WebSocket position messages can include `netPositionDecimal`, `qtyBoughtDecimal`, `qtySoldDecimal`, `bodPositionDecimal`, and `qtyAvailableDecimal`.
    * The older integer position fields `netPosition`, `qtyBought`, `qtySold`, `bodPosition`, and `qtyAvailable` remain for backward compatibility but are rounded and deprecated for partial-contract markets. `availablePositions` is also deprecated.
  * **Action recommended:** regenerate clients from the updated OpenAPI schemas and read quantity/tick constraints from each market before submitting orders. Do not assume whole-contract quantities or 1-cent price ticks, and do not rely on server-side rejection for extra decimal precision.
</Update>

<Update label="May 29, 2026" description="v0.0.34" tags={["New Feature", "Institutional API"]} rss={{ title: "v0.0.34 - Order fractional quantity scale and price-to-quantity-filled fields", description: "The Order message gains two additive fields: fractional_quantity_scale (field 49) for converting raw integer quantities to decimal, and price_to_quantity_filled (field 41), a map of price to quantity filled over the life of the order. They appear on every Institutional Trading and Report response that returns an Order or Execution, including the gRPC order stream." }}>
  * **Two additive fields on the `Order` message:**
    * `fractional_quantity_scale` (field 49, `int64`) — the fractional quantity scale copied from the instrument at order creation time. Divide raw integer quantities (`order_qty`, `cum_qty`, `leaves_qty`, etc.) by this value to get the properly scaled decimal quantity.
    * `price_to_quantity_filled` (field 41, `map<int64, int64>`) — quantity filled at each price point over the life of the order. The key is the price, the value is the quantity filled at that price.
  * **Where they appear:** every response that returns an `Order` or an `Execution` (which embeds `Order`), across the Institutional Trading and Report APIs and the gRPC order stream:
    * Trading API: `GET /v1/trading/orders/open` (`GetOpenOrders`) and the `CreateOrderSubscription` stream (snapshot orders and `update.executions[].order`).
    * Report API: `POST /v1/report/orders/search` (`SearchOrders`), `GET /v1/report/orders/{order_id}` (`GetOrder`), `POST /v1/report/executions/search` (`SearchExecutions`), and `GET /v1/report/executions/{exec_id}` (`GetExecution`).
  * **Backward compatible:** both fields are additive. Existing clients are unaffected; unset values decode as the proto defaults (`0` and an empty map).
  * **Action recommended:** rebuild your gRPC clients from the latest proto bundle to pick up the new fields.
</Update>

<Update label="May 28, 2026" description="v0.0.33" tags={["Upcoming", "Institutional API"]} rss={{ title: "v0.0.33 - Preprod partial contracts and decimalized tick sizes", description: "Dummy instruments are open in preprod for partial contract quantity handling plus 0.5c and 0.25c tick-size testing. Clients should read the instrument fields directly for quantity and price scaling." }}>
  * **Partial contracts in preprod:** `aec-mlb-az-mil-2026-06-15` is open in preprod as a dummy partial contract instrument.
    * Read `instrument.fractionalQtyScale` to determine how submitted integer order quantities are scaled. For example, if `instrument.fractionalQtyScale == 100`, submitting `quantity = 1` means 0.01 contracts, `quantity = 50` means 0.50 contracts, and `quantity = 100` means 1 full contract.
    * Read `instrument.minimumTradeQty` for the lowest scaled integer quantity that can be traded. For example, if `instrument.minimumTradeQty == 1` and `instrument.fractionalQtyScale == 100`, the minimum valid order quantity is `1`, which represents 0.01 contracts, or 1% of a contract.
    * Initial partial contract instruments will have `instrument.fractionalQtyScale == 100` and `instrument.minimumTradeQty == 1`, meaning the minimum order size is **1% of a contract**.
  * **Decimalization in preprod:** dummy instruments are open in preprod for smaller tick-size handling:
    * `aec-nba-mil-was-2026-06-15` has a **0.5c** tick size.
    * `aec-nhl-edm-ana-2026-06-15` has a **0.25c** tick size.
    * Read `instrument.priceScale` to determine how submitted integer order prices are scaled. For example, if `instrument.priceScale == 1000`, submitting `price = 5` means \$0.005, `price = 500` means \$0.50, and `price = 1000` means \$1.00.
    * Read `instrument.tickSize` for the tick size in dollars. For example, a 0.5c tick size is expressed as `instrument.tickSize = 0.005`, and a 0.25c tick size is expressed as `instrument.tickSize = 0.0025`.
  * **Action recommended:** read these values from the instrument before submitting orders. Do not infer quantity scale, price scale, or tick size from symbol, product category, or market type.
</Update>

<Update label="May 26, 2026" description="v0.0.32" tags={["New Feature", "Institutional API"]} rss={{ title: "v0.0.32 - Ledger endpoints, InstrumentStats fields, KeepAliveCommand", description: "New position and balance ledger endpoints (REST + gRPC stream), new InstrumentStats fields (last_trade_qty, settlement_set_time), and a new KeepAliveCommand on BiDirectionalStreamMarketData." }}>
  * **New ledger endpoints** for reconciliation, point-in-time replay, and end-of-day reporting:
    * **Position ledger (REST):** `GET /v1/positions/ledger`, `GET /v1/positions/ledger/download` — paginated query + streamed CSV of position changes (with both deltas and post-change cumulative state). See [Position Ledger](/institutional/positions/overview#position-ledger).
    * **Balance ledger (REST):** `GET /v1/funding/balance-ledger`, `GET /v1/funding/balance-ledger/download` — paginated query + streamed CSV of cash balance changes (deposits, withdrawals, fills, fees, corrections). See [Balance Ledger](/institutional/funding/overview).
    * **Balance ledger (gRPC):** `CreateBalanceLedgerSubscription` for real-time push of balance ledger entries. See [Balance Ledger Stream](/streaming-endpoints/balance-ledger-stream).
    * All three are scoped under `read:positions`. Both ledgers enforce a hard historical floor of **`2026-05-01T00:00:00Z`**; pre-floor entries are not retrievable.
  * **`InstrumentStats` additions** on the market data stream and `GetOrderBook` / `GetBBO` responses:
    * `last_trade_qty` (field 14, `optional int64`) — quantity of the most recent trade. Populated after any trade executes on the instrument.
    * `settlement_set_time` (field 15, `optional google.protobuf.Timestamp`) — timestamp when the settlement price was set. Populated only when the instrument is in a settled state.
  * **New `KeepAliveCommand`** on `BiDirectionalStreamMarketDataRequest` (field `keepalive = 7`). Sending one puts a client-to-server frame on the wire without modifying subscription state; the server returns no response. Solves the AWS Application Load Balancer 1-hour idle timeout (`RST_STREAM`) for long-lived bidirectional subscriptions with no client-to-server traffic. Recommended cadence: every **30–60 minutes** (well below the 3600s ALB timeout). Only applies to `BiDirectionalStreamMarketData`; server-streaming `CreateMarketDataSubscription` is not affected.
  * **Stream limits relaxed:** the per-firm cap is now **20 concurrent streams** with no per-stream-type restrictions. Previously, some stream types had individual caps; now the 20-stream budget is pooled across all gRPC subscriptions.
  * **Action recommended:** rebuild your gRPC clients from the latest proto bundle to pick up the new endpoints and additive fields above.
</Update>

<Update label="May 26, 2026" description="v0.0.32" tags={["New Feature", "Retail API"]} rss={{ title: "v0.0.32 - Portfolio activity types for taker fee rebates and liquidity program", description: "GET /v1/portfolio/activities now returns ACTIVITY_TYPE_TAKER_FEE_REBATE and ACTIVITY_TYPE_LIQUIDITY_PROGRAM for taker rebate credits and liquidity program payouts respectively. Both previously surfaced under ACTIVITY_TYPE_REFERRAL_BONUS / ACTIVITY_TYPE_TRANSFER." }}>
  * **Portfolio Activities API:** added two activity types now returned by `GET /v1/portfolio/activities`:
    * `ACTIVITY_TYPE_TAKER_FEE_REBATE` — taker fee rebate credit. Previously surfaced under `ACTIVITY_TYPE_REFERRAL_BONUS`.
    * `ACTIVITY_TYPE_LIQUIDITY_PROGRAM` — liquidity program payout. Previously surfaced under `ACTIVITY_TYPE_TRANSFER`.
  * Both carry an `accountBalanceChange` payload identical in shape to other balance-change activities.
  * Clients that have not regenerated against the updated OpenAPI schema will decode the new values as unknown enum members. Regenerate to surface the proper label.
</Update>

<Update label="May 21, 2026" description="v0.0.31" tags={["Incentives"]} rss={{ title: "v0.0.31 - Incentive reward updates", description: "Volume incentives are now live with NBA Playoffs Moneyline rewards, while select liquidity rewards were reduced for MLB Futures, IPL Games, and Politics events." }}>
  * **Volume Incentive Program is now live:** Program status moved from coming soon to open, with rewards based on share of eligible **taker-side notional** volume.
  * **Increase / new reward launch:** Added **NBA Playoffs Moneyline Volume Rewards** with a **\$100,000 in-game reward pool per market** (live May 21, 2026).
  * **Volume eligibility details:** only trades executed between **\$0.03 and \$0.97** count; minimum **\$500 notional** required to qualify for payout.
  * **Reduction — MLB Futures:** reduced from **\$5,000/day** (pooled across instruments) to **\$1,000/day**.
  * **Reduction — IPL Games:** reduced from **\$40,000/game** to **\$20,000/game**; moneyline split updated to **\$500 / \$1,500 / \$18,000** (Early / Day-of / Live).
  * **Reduction — Politics events:** reduced from **\$5,000/day** to **\$1,000/day**.
</Update>

<Update label="May 21, 2026" description="v0.0.30" tags={["Upcoming", "Institutional API", "Retail API"]} rss={{ title: "v0.0.30 - NBA Props live in production; derive tick size per instrument", description: "Basketball player props and first half markets go live in production the morning of May 22, 2026. Reminder: always derive tick size from the instrument — do not assume all instruments under a single contract type share the same tick. Upcoming World Cup futures are TEC contracts but will not be decimalized." }}>
  * **NBA Props (production):** Basketball player props and first half markets are going live in production the morning of **May 22, 2026**. The `market_sport_type` enums previously released to preprod will be active in production:
    * `basketball_player_points`
    * `basketball_player_assists`
    * `basketball_team_first_half_winner`
    * `basketball_team_first_half_spread`
    * `basketball_team_first_half_total`
  * **Tick size — always read from the instrument, not the contract type:** Do not assume that every instrument under a given contract type shares the same minimum price increment. Notably, **upcoming World Cup futures are Title Event Contracts (TEC) but will not be decimalized**, so they will not share a tick size with existing TEC futures. Pull the tick from the instrument before submitting any order.
    * **Retail API** — read `market.orderPriceMinTickSize` from `GET /v1/market/slug/{slug}`.
    * **Institutional API** — read `instrument.tickSize` from the instrument reference data response (`SearchInstruments` / `GetInstrument`).
</Update>

<Update
  label="May 21, 2026"
  description="v0.0.28"
  tags={["Upcoming","Retail API"]}
  rss={{
title: "v0.0.28 - Removing usernames from trade tape",
description: "Usernames are being removed from the Retail API trade tape responses for privacy.",
}}
>
  * **Retail API:** Removing usernames from trade tape responses
</Update>

<Update label="May 20, 2026" description="v0.0.29" tags={["Upcoming", "Institutional API"]} rss={{ title: "v0.0.29 - NBA Props enums in preprod", description: "Added basketball player props and first half market enums to the instrument market_sport_type field. Available in preprod." }}>
  * **NBA Props (preprod):** Added the following enums to the instrument `market_sport_type` field:
    * `basketball_player_points`
    * `basketball_player_assists`
    * `basketball_team_first_half_winner`
    * `basketball_team_first_half_spread`
    * `basketball_team_first_half_total`
</Update>

<Update label="May 4, 2026" description="v0.0.27" tags={["Bug Fix", "Institutional API"]} rss={{ title: "v0.0.27 - Execution response fields", description: "Commission and trade date fields now exposed on all execution-level responses including SearchExecutions, DownloadExecutions, and CreateOrderSubscription." }}>
  * **Execution responses:** Now exposing commission and trade date fields on all execution-level responses:
    * `commissionNotionalCollected` - Commission amount collected
    * `commissionSpreadPx` - Commission spread price
    * `transactTradeDate` - Trade transaction date
  * Applies to: SearchExecutions, DownloadExecutions, and CreateOrderSubscription execution updates
</Update>

<Update label="April 21, 2026" description="v0.0.26" tags={["New Feature", "Retail API"]} rss={{ title: "v0.0.26 - Retail batch endpoints and schema updates", description: "Documented batched order endpoints and added outcomeSide/action as alternative to intent on CreateOrderRequest. New enum members added to schema." }}>
  * **Retail Orders API:** documented three batched endpoints: `POST /v1/orders/batched`, `/v1/orders/batched/cancel`, `/v1/orders/batched/modify`. The first two were already shipped; the third is new.
  * **Retail Orders API:** documented `outcomeSide` + `action` as an alternative to `intent` on `CreateOrderRequest`, and added both fields to the `Order` response. `intent` is no longer marked `required` on `CreateOrderRequest`. Existing requests that send `intent` keep working; regenerated clients will see it flip from required to optional.
  * **Retail Orders API:** added enum members that were already on the wire but missing from the schema: `TIME_IN_FORCE_DAY`, `ORDER_STATE_NEW`, `EXECUTION_TYPE_NEW`, `ORD_REJECT_REASON_EXCHANGE_OPTION`.
</Update>

<Update label="April 17, 2026" description="v0.0.25" tags={["Documentation", "Institutional API"]} rss={{ title: "v0.0.25 - gRPC endpoint correction", description: "Corrected production gRPC endpoint from grpc-api.polymarketexchange.com to grpc-api.prod.polymarketexchange.com." }}>
  * Corrected production gRPC endpoint from `grpc-api.polymarketexchange.com` to `grpc-api.prod.polymarketexchange.com`
</Update>

<Update label="April 13, 2026" description="v0.0.24" tags={["Maintenance"]} rss={{ title: "v0.0.24 - Maintenance window change", description: "Weekly maintenance window moved from Tuesday 4am-6am ET to Thursday 6am-8am ET, effective April 16, 2026." }}>
  * Weekly maintenance window moved from **Tuesday 4am–6am ET** to **Thursday 6am–8am ET**, effective April 16, 2026
</Update>

<Update label="April 10, 2026" description="v0.0.23" tags={["Breaking Change", "Institutional API", "Retail API"]} rss={{ title: "v0.0.23 - Rate limit updates", description: "Rate limits reduced across all APIs: Institutional Gateway to 100 msg/s, FIX to 150 msg/s, Retail API to 20 req/s." }}>
  * Updated rate limits across all APIs:
    * Institutional Gateway (REST/gRPC): reduced to **100 messages per second** per firm
    * FIX Protocol: reduced to **150 messages per second** per session (all participants)
    * Retail API: reduced to **20 requests per second** per API key
</Update>

<Update label="March 30, 2026" description="v0.0.22" tags={["Breaking Change", "Bug Fix", "Documentation", "Institutional API"]} rss={{ title: "v0.0.22 - FIX field and route corrections", description: "FIX Product field now optional. Corrected REST API routes and production base URLs across documentation." }}>
  * FIX API: `Product` field (tag 460) changed from required to optional on New Order Single. All current products on Polymarket are `Product=12` (OTHER).
  * Corrected REST API routes: `/v1/accounts/whoami` → `/v1/whoami`, `/v1/accounts/users` → `/v1/users`, `/v1/accounts/accounts` → `/v1/accounts`
  * Fixed price scale examples across documentation to reflect correct values
  * Corrected production API base URLs to `api.prod.polymarketexchange.com` across all documentation
</Update>

<Update label="March 3, 2026" description="v0.0.21" tags={["Improvement", "Institutional API"]} rss={{ title: "v0.0.21 - Proto state field now optional", description: "State field changed from required to optional in MarketDataUpdate, GetOrderBookResponse, and GetBBOResponse. Use instrument state subscription for state changes." }}>
  * Edited proto files to improve the gRPC streaming experience
  * `state` field changed from required to optional in three messages:
    * `MarketDataUpdate.state` (field 4) in `marketdatasubscription.proto`
    * `GetOrderBookResponse.state` (field 4) in `orderbook.proto`
    * `GetBBOResponse.state` (field 6) in `orderbook.proto`
  * Participants should utilize the instrument state change subscription for state changes
</Update>

<Update label="February 26, 2026" description="v0.0.20" tags={["Improvement", "Institutional API"]} rss={{ title: "v0.0.20 - Settlement and price_scale fields", description: "Added settlement_price_calculation_text to settlement responses and price_scale to order messages." }}>
  * Updated settlement responses in `marketdatasubscription`, adding `settlement_price_calculation_text`
  * Added `price_scale` to order message
</Update>

<Update label="January 10, 2026" description="v0.0.19" tags={["New Feature", "Institutional API"]} rss={{ title: "v0.0.19 - Bidirectional market data streaming", description: "New BiDirectionalStreamMarketData RPC allows dynamic symbol subscription without reconnecting. Includes new Go and Python examples." }}>
  * Added Bidirectional Market Data Streaming API: `BiDirectionalStreamMarketData` RPC
  * Dynamically add and remove symbols during subscription lifetime without reconnecting
  * New response types: `SubscriptionAck` and `SubscriptionError` for subscription management
  * Updated client sample code with new Go and Python examples (Example 20)
  * Updated proto packages with bidirectional streaming support
</Update>

<Update label="January 4, 2026" description="v0.0.18" tags={["New Feature", "Institutional API"]} rss={{ title: "v0.0.18 - Account Valuation APIs", description: "New valuation endpoints for book-close accounting with historical queries via as_of_time or as_of_date. Includes CSV download for multi-account summaries." }}>
  * Added Account Valuation APIs for book-close accounting use cases
  * `POST /v1/valuations/accounts/statement/download`: Multi-account summaries as CSV
  * All new endpoints support historical queries via `as_of_time` or `as_of_date`
  * Cross-ISV protection enforced on all valuation endpoints
</Update>

<Update label="January 3, 2026" description="v0.0.17" tags={["New Feature", "Institutional API"]} rss={{ title: "v0.0.17 - Instrument query filters", description: "Added configurable instrument queries with pagination, state filtering, and sports league metadata filters." }}>
  * Documented configurable instrument queries: pagination, state filtering, and metadata filters
  * Added sports league filtering via `metadata.sports_game_league` (nfl, nba, mlb, nhl, cbb, cfb)
  * Added instrument metadata field documentation with sports-specific attributes
</Update>

<Update label="January 3, 2026" description="v0.0.16" tags={["New Feature", "Institutional API"]} rss={{ title: "v0.0.16 - Historical Positions API", description: "Query positions at any point in time using as_of_time or as_of_date for end-of-day reporting and regulatory snapshots." }}>
  * Added Historical Positions API: query positions at any point in time using `as_of_time` (RFC3339 timestamp) or `as_of_date` (trade date)
  * Use cases: end-of-day reporting, regulatory snapshots, position reconciliation
</Update>

<Update label="January 3, 2026" description="v0.0.15" tags={["Documentation"]} rss={{ title: "v0.0.15 - Documentation refresh", description: "Documentation deployment refresh with no API changes." }}>
  * Documentation deployment refresh
</Update>

<Update label="December 31, 2025" description="v0.0.14" tags={["Improvement", "Institutional API"]} rss={{ title: "v0.0.14 - Slow consumer handling and proto downloads", description: "Added skip-to-head behavior for slow consumers on streaming endpoints. Proto files now available for direct download." }}>
  * Added slow consumer handling option for streaming endpoints with skip-to-head behavior
  * Proto files now available for direct download (polymarket-protos.zip)
  * Added FAQ clarifying ISV-Participant relationship and participant\_id usage
</Update>

<Update label="December 8, 2025" description="v0.0.13" tags={["New Feature", "Institutional API"]} rss={{ title: "v0.0.13 - 25 REST API endpoints", description: "Added 25 REST API endpoints with full OpenAPI documentation covering Authentication, Accounts, Orders, Positions, Market Data, and Drop Copy." }}>
  * Added 25 REST API endpoints with full OpenAPI documentation
  * New sections: Authentication, Accounts, Orders, Positions, Market Data, Drop Copy
  * Organized API documentation by functional category
</Update>

<Update label="November 25, 2025" description="v0.0.12" tags={["Documentation", "Institutional API"]} rss={{ title: "v0.0.12 - gRPC and proto documentation", description: "Complete gRPC streaming documentation with Python examples, Protocol Buffer reference, VPC setup guide, and troubleshooting guide." }}>
  * Added complete gRPC streaming API documentation with Python code examples for market data and order execution streams
  * Introduced Protocol Buffer reference documentation with detailed message structures and field definitions
  * Added VPC connection setup guide with AWS PrivateLink configuration instructions
  * Created common pitfalls troubleshooting guide for integration issues
</Update>

<Update label="August 18, 2025" description="v0.0.11" tags={["Documentation", "Institutional API"]} rss={{ title: "v0.0.11 - Aesthetic updates", description: "Documentation aesthetic improvements including new figures and cleaner FIX example formatting." }}>
  * Aesthetic changes including new figures and cleaner formatting of FIX examples.
</Update>

<Update label="August 14, 2025" description="v0.0.10" tags={["Documentation"]} rss={{ title: "v0.0.10 - Initial documentation", description: "First draft of Polymarket Exchange Documentation." }}>
  * First DRAFT of Polymarket Exchange Documentation
</Update>
