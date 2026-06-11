> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Prices & Market Data

> How prices work and how the order book enables trading on Polymarket US.

## Prices are probabilities

Every contract on Polymarket US is priced between \$0 and \$1. The price represents the market's collective belief about how likely an outcome is.

| Price  | What it means                                  |
| ------ | ---------------------------------------------- |
| \$0.25 | The market thinks there's roughly a 25% chance |
| \$0.50 | Coin flip — the market is undecided            |
| \$0.75 | The market thinks it's likely (75% chance)     |

If you buy a YES contract at \$0.55 and the outcome happens, the contract settles at \$1.00 — you profit \$0.45 per contract. If it doesn't happen, it settles at \$0.00 and you lose your \$0.55.

## The order book

Polymarket US runs a **central limit order book**. Prices aren't set by Polymarket — they come from traders placing buy and sell orders against each other.

The order book has two sides:

| Side              | What it means                                          |
| ----------------- | ------------------------------------------------------ |
| **Bids**          | Buy orders — the prices traders are willing to pay     |
| **Asks (offers)** | Sell orders — the prices traders are willing to accept |

The **spread** is the gap between the best bid and the best ask. A tight spread means the market is liquid. A wide spread means fewer people are trading.

The **BBO (best bid and offer)** is the tightest price on each side — the highest bid and the lowest ask. This is the price you'd trade at if you placed a market order right now.

## How trades happen

When you place a **market order**, it fills immediately at the best available price on the other side of the book. You're a **taker** — you're taking liquidity.

When you place a **limit order** at a specific price and it doesn't fill immediately, it sits on the book waiting. You're a **maker** — you're providing liquidity for others to trade against.

If your limit order's price crosses the best available price on the other side, it fills immediately (like a market order). Otherwise, it rests on the book.

## Settlement

When a market resolves, every contract settles at either \$1.00 (YES won) or \$0.00 (NO won). The exchange handles this automatically — winning contracts are credited to your balance, losing contracts go to zero.

## Market states

A market goes through different states during its lifecycle:

| State         | What it means                                       |
| ------------- | --------------------------------------------------- |
| **Open**      | The market is accepting orders and actively trading |
| **Pre-open**  | The market exists but trading hasn't started yet    |
| **Suspended** | Trading is temporarily paused                       |
| **Halted**    | Trading has been stopped                            |
| **Expired**   | The market has ended and settled                    |
