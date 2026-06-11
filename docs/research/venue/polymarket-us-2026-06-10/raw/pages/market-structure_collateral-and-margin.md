> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Collateral and Margin

> Learn how collateral and margin work for long and short positions

Polymarket Exchange operates with **fully-collateralized contracts**, meaning sufficient funds are locked to cover the maximum possible payout at the time the trade is executed. No additional funds are required afterward.

## How Collateral Works

When a trade executes at a given price:

**Buyers (Long Positions)**

* Pay the contract price
* No additional margin required
* Maximum loss: amount paid
* Maximum gain: \$1.00 – price paid

**Sellers (Short Positions)**

* Receive the contract price as proceeds
* Post \$1.00 margin per contract (full payout value)
* Fiat balance increases by sale proceeds
* Buying power decreases by (Payout Value – Sale Price)
* Maximum loss: \$1.00 – sale price
* Maximum gain: sale price

**Example: Trade at \$0.40**

| Participant | Cash Flow | Margin Required | Buying Power Change |
| ----------- | --------- | --------------- | ------------------- |
| Buyer       | –\$0.40   | \$0             | –\$0.40             |
| Seller      | +\$0.40   | \$1.00          | –\$0.60             |

At settlement, **Polymarket Clearing** holds the seller's \$1.00 margin to guarantee payout. The buyer's \$0.40 payment becomes the seller's proceeds.

## Maximum Gain and Loss

Once a trade is executed, maximum gain and loss are fixed and do not change regardless of subsequent price movements.

**For a contract trading at \$0.40:**

| Position     | Max Loss | Max Gain |
| ------------ | -------- | -------- |
| Buy (Long)   | \$0.40   | \$0.60   |
| Sell (Short) | \$0.60   | \$0.40   |

## Shorting Mechanics

Shorting lets you take the opposite side of a market by **selling a yes contract without owning it**. You receive the sale price immediately, and margin equal to the full payout value (\$1.00 per contract) is locked to cover your potential obligations at settlement.

Shorts are created by selling yes contracts and posting \$1.00 margin per contract. The collateral requirement is the full payout value (\$1.00), not max loss. There is no collateral release from offsets or favorable price moves.

### Trading Examples

**Buying yes at \$0.60 (Long Position)**

* **At trade:** You pay \$0.60 per contract. Fiat balance decreases by \$0.60, and buying power decreases by \$0.60 because cash has been converted into a position.
* **Position value:** \$0.60 (quantity × last price)
* **If yes wins:** You receive \$1.00 (P/L = +\$0.40)
* **If yes loses:** You receive \$0 (P/L = –\$0.60)

**Selling yes at \$0.60 (Short Position)**

* **At trade:** You receive \$0.60 in sale proceeds. Fiat balance increases by \$0.60. You must post margin equal to the payout value: \$1.00 per contract. The net effect on buying power: +\$0.60 (proceeds) – \$1.00 (margin) = **–\$0.40**.
* **Position value:** \$0.40 (quantity × \[\$1.00 – \$0.60])
* **If yes wins:** Loss is the full \$1.00 payout minus the \$0.60 proceeds (P/L = –\$0.40)
* **If yes loses:** You keep the \$0.60 proceeds and margin is released (P/L = +\$0.60)

### P/L Summary Table

| Action           | Outcome   | P/L     |
| ---------------- | --------- | ------- |
| Buy yes @\$0.60  | yes wins  | +\$0.40 |
| Buy yes @\$0.60  | yes loses | –\$0.60 |
| Sell yes @\$0.60 | yes wins  | –\$0.40 |
| Sell yes @\$0.60 | yes loses | +\$0.60 |

## Short Position Details

In a "Did X happen?" market, you can express a view by either buying yes (trading on the event happening) or selling yes (trading on the event not happening).

When you take a short position, you are selling yes without owning it; i.e., expressing the view that the event will not occur. All shorts must be fully collateralized through a margin requirement equal to the payout value (\$1.00 per contract), ensuring you can cover potential losses if the market moves against you.

For example, if yes is trading at \$0.60 and you short 100 contracts, you receive \$60 in proceeds from the buyer, but you must also post \$100 in margin. Your fiat balance increases by \$60, but your buying power decreases by \$40 (\$60 proceeds – \$100 margin locked).

**Cash flow by position type:**

* **Long yes:** Fiat balance decreases by the purchase cost, buying power decreases by the same amount (cash converted to position).
* **Short yes:** Fiat balance increases by sale proceeds, but buying power decreases by (Payout Value – Sale Price) due to the full \$1.00 margin locked per contract.

If you attempt a trade that would cause your buying power to fall below zero, the trade will fail automatically.

## Portfolio Value Calculation

Portfolio value accounts for both buying power and position values:

**For long positions:**

Portfolio Value = Buying Power + (Quantity × Last Price)

**For short positions:**

Portfolio Value = Buying Power + (Quantity × \[Payout Value – Last Price])

Example: You have \$5 starting balance and sell 1 yes contract @ \$0.70:

* Fiat Balance: \$5.70 (received \$0.70 proceeds)
* Margin Requirement: \$1.00 (locked)
* Buying Power: \$4.70 (\$5.70 – \$1.00)
* Position value: 1 × (\$1.00 – \$0.70) = \$0.30
* Portfolio Value: \$4.70 + \$0.30 = \$5.00 ✓

## Settlement and Payout

At settlement, **Polymarket Clearing** releases funds automatically:

* Winners receive **\$1.00 per contract**
* Losers receive **\$0**
* No margin calls, reconciliations, or additional obligations

**Settlement Example: Seller at \$0.40**

If the seller wins (event does not occur):

* Seller keeps: \$0.40 proceeds
* Margin released: \$1.00
* Total return: \$1.40
* Net profit: \$0.40

If the buyer wins (event occurs):

* Seller's \$1.00 margin → paid to buyer
* Seller keeps: \$0.40 proceeds
* Net loss: \$0.60

## Collateral Management

Collateral and margin are managed at the clearing level:

1. You submit a withdrawal (e.g., \$100) via Aeropay.
2. The DCO (Derivatives Clearing Organization) reviews it.
3. The DCM (Designated Contract Market) denies the request if it would leave insufficient buying power to meet margin obligations.

The same logic applies to open orders: unmatched longs or shorts cannot remain if they would breach margin limits.

## Portfolio Margin

Polymarket Exchange applies portfolio-level margining, meaning margin requirements consider your entire set of open positions rather than treating each market in isolation. You can maintain many open positions across different markets, as long as the aggregate exposure does not exceed your available buying power.

## Reducing or Closing a Short

You can reduce or close a short at any time by buying back the yes contracts you sold.

**Examples:**

1. **Short 1 yes, buy back at a lower price** - Your exposure is closed and margin is released.
2. **Short 10 yes, buy back 4** - Your remaining exposure decreases, and proportional margin is unlocked.

## Key Points

* Buyers post the purchase price; sellers post \$1.00 margin per contract.
* Maximum gain and loss are **fixed at the time of the trade**.
* Sellers receive sale proceeds but must post full payout value as margin.
* **Polymarket Clearing guarantees payout** from the seller's locked margin.
* There are no margin calls.
* Shorting lets you express a bearish view on the yes outcome and provides liquidity to the market.
