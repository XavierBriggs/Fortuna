> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Orders & Trading

> How trading works on Polymarket US — the YES/NO model, order types, and how positions work.

## The YES/NO model

Every market on Polymarket US has two sides: **YES** and **NO**. They always add up to \$1.00. If YES is priced at \$0.60, NO is priced at \$0.40.

You don't trade YES and NO as separate things. There's only one instrument per market — the YES side. To trade against an outcome, you **sell** YES (which is the same as buying NO).

| What you want to do                    | How you do it                      |
| -------------------------------------- | ---------------------------------- |
| Trade on the outcome **happening**     | Buy YES                            |
| Trade on the outcome **not happening** | Sell YES (equivalent to buying NO) |
| Close a winning YES position           | Sell YES                           |
| Close a losing NO position             | Buy YES back                       |

This matters because when you place an order, the price always refers to the YES side. If you want to buy NO at \$0.40, you're really selling YES at \$0.60 — the system handles this, but you need to understand it to set the right price.

## Order types

| Type             | How it works                                                                                   |
| ---------------- | ---------------------------------------------------------------------------------------------- |
| **Limit order**  | You set a price. The order sits on the book until someone trades against it, or you cancel it. |
| **Market order** | Fills immediately at the best available price. You get instant execution but pay the spread.   |

Most traders use limit orders. Market orders are useful when you need to get in or out quickly and don't mind paying a slightly worse price.

## Time in force

When you place a limit order, you choose how long it stays active:

| Option                        | What it means                                                |
| ----------------------------- | ------------------------------------------------------------ |
| **Good till cancel (GTC)**    | Stays open until it fills or you cancel it                   |
| **Good till date (GTD)**      | Stays open until a specific time, then cancels automatically |
| **Immediate or cancel (IOC)** | Fills whatever is available right now, cancels the rest      |
| **Fill or kill (FOK)**        | Must fill completely or not at all — no partial fills        |

## What happens after you place an order

Your order goes through a lifecycle:

1. **Pending** — the exchange has received your order
2. **Open** — it's resting on the book, waiting for a match
3. **Partially filled** — some of your order has matched, the rest is still open
4. **Filled** — your entire order has matched
5. **Canceled / Expired / Rejected** — the order didn't fill

## Positions

Once your order fills, you have a **position**. A position is simply the contracts you hold in a market.

* A **long position** means you own YES contracts — you profit if the outcome happens
* A **short position** means you've sold YES contracts — you profit if the outcome doesn't happen

Your **buying power** is the cash you have available to open new positions. When you buy contracts, your buying power decreases. When you sell or a market settles in your favor, it increases.

## Closing a position

You can close a position at any time by taking the opposite action:

* If you're long (bought YES), sell YES to close
* If you're short (sold YES), buy YES to close

You don't have to wait for the market to settle. If the price has moved in your favor, you can lock in a profit early.
