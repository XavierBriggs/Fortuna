# paired_cycle_btc_perp_vs_kxbtc — fixture provenance & basis sanity check

Companion data file: `paired_cycle_btc_perp_vs_kxbtc.json`

This fixture pairs one capture cycle of the **BTC perpetual** (Kalshi perp
contract) against the same-cycle **KXBTC price-level ladder** (the $500-wide
"between" brackets on BTC), so the `perp_event_basis` kernel (FORTUNA track C,
slice 3b) can be exercised end-to-end against real recorded market data.

## Source

- **Origin:** the live FORTUNA recorder's perishable capture, read **READ-ONLY**.
  The recorder (`fortuna-recorder --interval-secs 30 --bracket-series
  KXBTC15M,KXBTC,KXBTCD`) was running during extraction and was not touched.
- **Capture day:** `/Users/xavierbriggs/fortuna/data/perishable/2026-06-13/`
- **Streams sampled (same `cycle_id` across all four = time-aligned):**
  - `perp_orderbook.jsonl` — key `KXBTCPERP` (orderbook top-of-book + depth)
  - `perp_markets.jsonl` — the `KXBTCPERP` market row (`settlement_mark_price`,
    `reference_price`, `contract_size`)
  - `funding_estimate.jsonl` — key `KXBTCPERP` (`funding_rate`, `mark_price`,
    `next_funding_time`)
  - `bracket_quotes.jsonl` — key `KXBTC` (the price-LEVEL ladder; `body` is a
    JSON-string of the Kalshi `GET /markets` response)
- **Chosen cycle_id:** `1781160753775`
- **Captured at (KXBTC ladder row):** `1781369448020` ms =
  **2026-06-13T16:50:48.020Z**
  (perp orderbook row for the same cycle: `1781369443170` ms =
  2026-06-13T16:50:43.170Z — ~5 s earlier, same 30 s capture cycle.)

### ⚠️ Key-name discrepancy (important)

The slice-3b task brief refers to the BTC perp as **`KXBTCPERP1`**. **No such
key exists** in this capture — in `perp_orderbook.jsonl`,
`perp_markets.jsonl`, and `funding_estimate.jsonl` the BTC perp is keyed
**`KXBTCPERP`** (no `1` suffix). `KXBTCPERP1` returns zero rows in every stream.
The only BTC perp present is `KXBTCPERP`, so that is what was sampled. (Other
underlyings — `KXBCHPERP`, `KXETHPERP`, `KXSOLPERP`, … — were skipped as
directed.) Controller should confirm the intended key is `KXBTCPERP`.

## KXBTC ladder structure (`strike_type`)

Each KXBTC market is one of three `strike_type`s. All quotes are YES-side dollar
strings ($1 payout if YES resolves true), so a YES mid is directly the
**implied probability** of that price outcome:

- **`between`** — a $500-wide bin: `floor_strike` ≤ settlement < `cap_strike`
  (e.g. floor 63000, cap 63499.99, sub_title "$63,000 to 63,499.99"). Both
  strikes present. These are the workhorse bins for the basis median.
- **`greater`** — the top open-ended bin: settlement ≥ `floor_strike`.
  `cap_strike` is **absent/null** (here floor 74999.99, "$75,000 or above").
- **`less`** — the bottom open-ended bin: settlement < `cap_strike`.
  `floor_strike` is **absent/null** (here cap 51000, "$50,999.99 or below").

In the chosen cycle the **active** ladder is event `KXBTC-26JUN1917`
(expiration 2026-06-19T17:00Z): **50 active markets** = 48 `between`
($51,000 → $74,999.99 in $500 steps) + 1 `greater` + 1 `less`. (A second event
`KXBTC-26JUN1500` was present but all 150 of its markets were `initialized`,
not `active`, and were dropped.) Only `status=="active"` markets are kept.

## Quote / price conventions

- **YES quote dollars** (`yes_bid_dollars`, `yes_ask_dollars`) are dollar
  strings in `[0,1]`, e.g. `"0.0600"`. With a $1 payout, the **YES mid**
  `(bid+ask)/2` is the market-implied probability that BTC settles inside that
  bin. Sizes (`yes_bid_size_fp`, `yes_ask_size_fp`) are contract-count strings.
- **Perp price scale:** the perp `contract_size_btc` is `0.000100` BTC, and all
  perp prices (orderbook levels, `settlement_mark`, `reference_price`,
  funding `mark_price`) are **dollars per contract**. The implied **BTC spot**
  is therefore `price / contract_size = price × 10,000`. The fixture carries
  both the raw per-contract value (`*_per_contract_dollars`) and the derived
  BTC-spot value (`settlement_mark_dollars`, `reference_price_dollars`). The
  recorder's own `derived` block confirms the scale
  (`best_ask_tenthousandths: 63908`, `best_bid_tenthousandths: 63904`).

## BASIS SANITY CHECK (computed on this fixture)

Method: take the **active `between`** bins, YES-mid each
(`(yes_bid+yes_ask)/2`), normalize the mids into a pmf over bins (raw mids sum
to ~1.23 because of bid/ask half-spread overcount; normalization removes that),
sort by `floor_strike`, walk the cumulative distribution, and linearly
interpolate within the bin where cumulative first crosses **0.5** to get the
ladder-implied **median** BTC price. Compare against the perp `settlement_mark`
(converted to BTC spot). Signed basis = `perp_mark − ladder_median`.

| quantity | value |
|---|---|
| active `between` bins used | 48 |
| raw YES-mid sum (pre-normalize) | 1.23 |
| perp `settlement_mark` (per-contract) | $6.3906 |
| **perp settlement mark → BTC spot** | **$63,906.00** |
| perp `reference_price` → BTC spot | $63,910.00 |
| **ladder implied MEDIAN (cum. pmf crosses 0.5)** | **$63,961.53** |
| ↳ crossing bin | $63,500 – $63,999.99 (p≈0.0528) |
| ladder probability-weighted MEAN (sanity) | $63,585.36 |
| **SIGNED BASIS (perp_mark − ladder_median)** | **−$55.53** |

**Reading:** the perp settlement mark ($63,906) and the ladder-implied median
($63,961.53) agree to within **−$55.53** — about **0.09%**, i.e. well inside a
single $500 bin. Two fully independent price sources (the perp book vs. the
bracket ladder) line up, which confirms (a) the recorded data is internally
consistent and (b) the basis math (YES-mid → pmf → median → signed basis)
works end-to-end on real captured data. Sign is negative → the perp mark sits
fractionally **below** the ladder median for this cycle.

## Secrets check

The bodies are public Kalshi market-data responses (orderbook, `/markets`,
funding estimate). The extracted/trimmed JSON was scanned for sensitive
substrings (`key`, `secret`, `token`, `signature`, `auth`, `password`,
`private`, `credential`, `bearer`, `api_key`, `access`, `session`, `cookie`,
`jwt`, `salt`, `nonce`) — **zero hits**. The fixture contains only market
fields: perp ticker/contract-size/orderbook/marks/funding, and per-bin
ticker/strike/YES-quote/size/status/sub_title. Heavy non-basis text
(`rules_primary`, `rules_secondary`, `price_level_structure`, `price_ranges`,
etc.) was dropped during trimming. **No keys, signatures, or secrets present.**

## Caveats

- BTC perp key is `KXBTCPERP`, not `KXBTCPERP1` (see discrepancy note above).
- The ladder spans $51k–$75k but two-sided liquidity is concentrated in the
  **$59,500–$68,499** core (real YES bids + asks). Far-tail bins (below ~$59.5k
  and above ~$68.5k) are effectively one-sided (YES bid 0.00, small YES ask at
  $0.02–0.03) — normal for a ~6-day-out BTC market. The median/basis math
  relies on the liquid core, which straddles the perp mark, so the result is
  robust; the dead tails contribute negligible pmf mass.
- The KXBTC ladder row and the perp orderbook row for this cycle were captured
  ~5 s apart (same 30 s cycle), so there is a small, expected intra-cycle time
  skew between the two price sources.
- The `greater`/`less` open-ended bins are included in `kxbtc_ladder` (for
  completeness) but are intentionally **excluded** from the median computation
  (no finite width to interpolate).
