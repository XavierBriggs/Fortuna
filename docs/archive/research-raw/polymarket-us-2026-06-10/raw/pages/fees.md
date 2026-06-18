> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Fee Schedule

> Trading fee schedule, rebates, and examples

<Info>Effective exchange-wide from 3pm ET, Friday April 3, 2026.</Info>

## Trading Fees

Fees are computed using a symmetric formula that scales with price uncertainty:

```
Fee = Θ × C × p × (1 - p)
```

Where:

* **C** is the number of contracts
* **p** is the trade price (\$0.01 to \$0.99)
* **Θ** (theta) is the fee coefficient

|                  | Theta   | Max (p = \$0.50) |
| ---------------- | ------- | ---------------- |
| **Taker Fee**    | 0.05    | \$1.25           |
| **Maker Rebate** | -0.0125 | -\$0.31          |

* **Maker rebate** (25% of taker fees) is applied at the point of trade.
* **Taker rebate**: Participants who trade over \$250,000 in taker volume between May 15, 2026 and June 30, 2026 (inclusive) receive a taker rebate of 30% of their total taker fees for that period.

### Fee Schedule by Price

| Price  | Trade Value (100-lot) | Taker Pays (100-lot) | Maker Receives (100-lot) |
| ------ | --------------------- | -------------------- | ------------------------ |
| \$0.01 | \$1                   | \$0.05               | \$0.01                   |
| \$0.02 | \$2                   | \$0.10               | \$0.02                   |
| \$0.03 | \$3                   | \$0.15               | \$0.04                   |
| \$0.04 | \$4                   | \$0.19               | \$0.05                   |
| \$0.05 | \$5                   | \$0.24               | \$0.06                   |
| \$0.06 | \$6                   | \$0.28               | \$0.07                   |
| \$0.07 | \$7                   | \$0.33               | \$0.08                   |
| \$0.08 | \$8                   | \$0.37               | \$0.09                   |
| \$0.09 | \$9                   | \$0.41               | \$0.10                   |
| \$0.10 | \$10                  | \$0.45               | \$0.11                   |
| \$0.11 | \$11                  | \$0.49               | \$0.12                   |
| \$0.12 | \$12                  | \$0.53               | \$0.13                   |
| \$0.13 | \$13                  | \$0.57               | \$0.14                   |
| \$0.14 | \$14                  | \$0.60               | \$0.15                   |
| \$0.15 | \$15                  | \$0.64               | \$0.16                   |
| \$0.16 | \$16                  | \$0.67               | \$0.17                   |
| \$0.17 | \$17                  | \$0.71               | \$0.18                   |
| \$0.18 | \$18                  | \$0.74               | \$0.18                   |
| \$0.19 | \$19                  | \$0.77               | \$0.19                   |
| \$0.20 | \$20                  | \$0.80               | \$0.20                   |
| \$0.21 | \$21                  | \$0.83               | \$0.21                   |
| \$0.22 | \$22                  | \$0.86               | \$0.21                   |
| \$0.23 | \$23                  | \$0.89               | \$0.22                   |
| \$0.24 | \$24                  | \$0.91               | \$0.23                   |
| \$0.25 | \$25                  | \$0.94               | \$0.23                   |
| \$0.26 | \$26                  | \$0.96               | \$0.24                   |
| \$0.27 | \$27                  | \$0.99               | \$0.25                   |
| \$0.28 | \$28                  | \$1.01               | \$0.25                   |
| \$0.29 | \$29                  | \$1.03               | \$0.26                   |
| \$0.30 | \$30                  | \$1.05               | \$0.26                   |
| \$0.31 | \$31                  | \$1.07               | \$0.27                   |
| \$0.32 | \$32                  | \$1.09               | \$0.27                   |
| \$0.33 | \$33                  | \$1.11               | \$0.28                   |
| \$0.34 | \$34                  | \$1.12               | \$0.28                   |
| \$0.35 | \$35                  | \$1.14               | \$0.28                   |
| \$0.36 | \$36                  | \$1.15               | \$0.29                   |
| \$0.37 | \$37                  | \$1.17               | \$0.29                   |
| \$0.38 | \$38                  | \$1.18               | \$0.29                   |
| \$0.39 | \$39                  | \$1.19               | \$0.30                   |
| \$0.40 | \$40                  | \$1.20               | \$0.30                   |
| \$0.41 | \$41                  | \$1.21               | \$0.30                   |
| \$0.42 | \$42                  | \$1.22               | \$0.30                   |
| \$0.43 | \$43                  | \$1.23               | \$0.31                   |
| \$0.44 | \$44                  | \$1.23               | \$0.31                   |
| \$0.45 | \$45                  | \$1.24               | \$0.31                   |
| \$0.46 | \$46                  | \$1.24               | \$0.31                   |
| \$0.47 | \$47                  | \$1.25               | \$0.31                   |
| \$0.48 | \$48                  | \$1.25               | \$0.31                   |
| \$0.49 | \$49                  | \$1.25               | \$0.31                   |
| \$0.50 | \$50                  | \$1.25               | \$0.31                   |
| \$0.51 | \$51                  | \$1.25               | \$0.31                   |
| \$0.52 | \$52                  | \$1.25               | \$0.31                   |
| \$0.53 | \$53                  | \$1.25               | \$0.31                   |
| \$0.54 | \$54                  | \$1.24               | \$0.31                   |
| \$0.55 | \$55                  | \$1.24               | \$0.31                   |
| \$0.56 | \$56                  | \$1.23               | \$0.31                   |
| \$0.57 | \$57                  | \$1.23               | \$0.31                   |
| \$0.58 | \$58                  | \$1.22               | \$0.30                   |
| \$0.59 | \$59                  | \$1.21               | \$0.30                   |
| \$0.60 | \$60                  | \$1.20               | \$0.30                   |
| \$0.61 | \$61                  | \$1.19               | \$0.30                   |
| \$0.62 | \$62                  | \$1.18               | \$0.29                   |
| \$0.63 | \$63                  | \$1.17               | \$0.29                   |
| \$0.64 | \$64                  | \$1.15               | \$0.29                   |
| \$0.65 | \$65                  | \$1.14               | \$0.28                   |
| \$0.66 | \$66                  | \$1.12               | \$0.28                   |
| \$0.67 | \$67                  | \$1.11               | \$0.28                   |
| \$0.68 | \$68                  | \$1.09               | \$0.27                   |
| \$0.69 | \$69                  | \$1.07               | \$0.27                   |
| \$0.70 | \$70                  | \$1.05               | \$0.26                   |
| \$0.71 | \$71                  | \$1.03               | \$0.26                   |
| \$0.72 | \$72                  | \$1.01               | \$0.25                   |
| \$0.73 | \$73                  | \$0.99               | \$0.25                   |
| \$0.74 | \$74                  | \$0.96               | \$0.24                   |
| \$0.75 | \$75                  | \$0.94               | \$0.23                   |
| \$0.76 | \$76                  | \$0.91               | \$0.23                   |
| \$0.77 | \$77                  | \$0.89               | \$0.22                   |
| \$0.78 | \$78                  | \$0.86               | \$0.21                   |
| \$0.79 | \$79                  | \$0.83               | \$0.21                   |
| \$0.80 | \$80                  | \$0.80               | \$0.20                   |
| \$0.81 | \$81                  | \$0.77               | \$0.19                   |
| \$0.82 | \$82                  | \$0.74               | \$0.18                   |
| \$0.83 | \$83                  | \$0.71               | \$0.18                   |
| \$0.84 | \$84                  | \$0.67               | \$0.17                   |
| \$0.85 | \$85                  | \$0.64               | \$0.16                   |
| \$0.86 | \$86                  | \$0.60               | \$0.15                   |
| \$0.87 | \$87                  | \$0.57               | \$0.14                   |
| \$0.88 | \$88                  | \$0.53               | \$0.13                   |
| \$0.89 | \$89                  | \$0.49               | \$0.12                   |
| \$0.90 | \$90                  | \$0.45               | \$0.11                   |
| \$0.91 | \$91                  | \$0.41               | \$0.10                   |
| \$0.92 | \$92                  | \$0.37               | \$0.09                   |
| \$0.93 | \$93                  | \$0.33               | \$0.08                   |
| \$0.94 | \$94                  | \$0.28               | \$0.07                   |
| \$0.95 | \$95                  | \$0.24               | \$0.06                   |
| \$0.96 | \$96                  | \$0.19               | \$0.05                   |
| \$0.97 | \$97                  | \$0.15               | \$0.04                   |
| \$0.98 | \$98                  | \$0.10               | \$0.02                   |
| \$0.99 | \$99                  | \$0.05               | \$0.01                   |

### Fee Rules

* Fees are symmetric around p = 0.50 and lowest near the extremes (0 and 1).
* All fees and rebates are rounded to the nearest \$0.01 using banker's rounding (round half to even).

## Examples

#### Example 1: Buy 1,000 contracts at \$0.10 — cheap contract

Buying a long shot. The fee scales with price uncertainty: p × (1 − p) = 0.10 × 0.90 = 0.09.

* **Buyer (taker):** 0.05 × 1,000 × 0.10 × 0.90 = **−\$4.50**
* **Seller (maker):** 0.0125 × 1,000 × 0.10 × 0.90 = **+\$1.13**

***

#### Example 2: Buy 1,000 contracts at \$0.65 — expensive contract

Buying a likely outcome. Higher price but lower p × (1 − p) than midpoint.

* **Buyer (taker):** 0.05 × 1,000 × 0.65 × 0.35 = **−\$11.38**
* **Seller (maker):** 0.0125 × 1,000 × 0.65 × 0.35 = **+\$2.84**

***

#### Example 3: Sell 1,000 contracts at \$0.30 — sell low probability

The seller is the aggressor. Both sides pay based on the same p × (1 − p) factor.

* **Seller (taker):** 0.05 × 1,000 × 0.30 × 0.70 = **−\$10.50**
* **Buyer (maker):** 0.0125 × 1,000 × 0.30 × 0.70 = **+\$2.63**

***

#### Example 4: Sell 1,000 contracts at \$0.90 — sell high probability

When the price is close to \$1.00, p × (1 − p) is small and fees are minimal.

* **Seller (taker):** 0.05 × 1,000 × 0.90 × 0.10 = **−\$4.50**
* **Buyer (maker):** 0.0125 × 1,000 × 0.90 × 0.10 = **+\$1.13**

***

#### Example 5: Buy 1,000 contracts at \$0.50 — coin flip market

A 50/50 market. This is where the fee is highest per contract because p × (1 − p) = 0.25.

* **Buyer (taker):** 0.05 × 1,000 × 0.50 × 0.50 = **−\$12.50**
* **Seller (maker):** 0.0125 × 1,000 × 0.50 × 0.50 = **+\$3.13**

## FAQ

### Are fees deducted from my balance automatically?

Yes. Taker fees are deducted from your balance at the time of the trade. Maker rebates are credited to your balance at the time of the fill.

### Can fees ever be zero?

Yes. Fees are rounded to the nearest cent. On small trades (low quantity or prices near \$0.00 or \$1.00), the fee can round down to \$0.00.

### Do I pay fees when my order is canceled or expires?

No. Fees are only charged when a trade executes. If your order is canceled, expires, or is rejected, no fee is charged.

### What is banker's rounding?

Fees are rounded to the nearest cent using banker's rounding (round half to even). For example, \$0.025 rounds to \$0.02 (down to even), while \$0.035 rounds to \$0.04 (up to even).
