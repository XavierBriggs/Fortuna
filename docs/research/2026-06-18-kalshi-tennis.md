# Kalshi tennis markets — coverage, microstructure, and what it means for DEUCE

**Date:** 2026-06-18 · Grounded in web research + Kalshi's public API (per the research-loop rule: Kalshi facts come from research, not memory). Tiers: T1 primary/official, T2 industry, T3 trade/news.

## What Kalshi actually lists (verified from the live API)

Queried `GET https://api.elections.kalshi.com/trade-api/v2/markets?series_ticker=KXATPMATCH` on 2026-06-18 (**T1, primary**). A live example contract:

- **Individual ATP match-winner markets exist** under series **`KXATPMATCH`** — e.g. "Zverev vs Collignon, **2026 ATP Halle Quarterfinal**". **Halle is an ATP 500, not a Slam — so coverage extends to tour events, not just Grand Slams.**
- **Binary, price = probability.** `last_price 0.85`, `no_bid 0.15 / no_ask 0.16` → **1¢ bid/ask spread** on a non-Slam QF. Settles $1 / $0.
- `open_interest` ~tens of thousands of contracts on this match; `liquidity_dollars` read 0.0000 at query time (top-of-book depth field — must be checked per match; the tight spread implies resting orders).
- **Resolution rule:** "...resolves to Yes **after a ball has been played**." → a mid-match **retirement resolves to the winner** (the live position isn't voided once a ball is struck); a walkover (no ball) presumably voids. This is the retirement/settlement handling the modeling memo flagged.
- Also live: `KXWTAGRANDSLAM` (WTA Slam futures), and a **CFTC self-cert (Jan 2026)** for **set-level** contracts "Will <player> win <set>?" (**T1**, CFTC portal).

## Microstructure vs sportsbooks

- **No devig needed** — Kalshi price *is* the probability. DEUCE's calibrated prob compares directly to the Kalshi mid. The entire sportsbook devig layer is irrelevant on Kalshi.
- **Fees: `7% × p × (1−p)` per contract (taker); maker ≈ 0 / rebate** (marketmath.io, deadspin, Kalshi fee schedule — **T2/T3**). At p=0.5 that's 1.75¢; at p=0.85 it's ~0.9¢. **Lower hurdle than Betfair commission (2–5%) or soft-book vig (4–10%)**, and limit orders cut it ~75%.
- **Venue character:** CFTC-regulated US exchange, **retail-heavy**, sports = **~89% of fee revenue**, $100B+ lifetime volume, $1B+ days (Legal Sports Report, casino.org, Substack — **T3**). But the volume is NFL/NBA/World-Cup-driven; **tennis is a minor sport on Kalshi** → plausibly **softer pricing** than the global sharp tennis books.

## Why this flips the edge thesis (the key insight)

The sportsbook conclusion was: *tennis closing lines (Pinnacle/Betfair) are razor-sharp; you can't beat them on liquid ATP.* Kalshi is the **opposite environment**: a US retail venue where tennis is a sideshow. The mispricing that doesn't exist on sharp books **may exist on Kalshi** — and crucially, it could be in **main-tour matches** (which Kalshi lists and which are liquid enough to trade), not just the unreachable Challenger tier.

**Reframed architecture:** the edge play is **Kalshi mid-price vs the sharp sportsbook line** (devigged Pinnacle/Betfair = the best available tennis probability), with **DEUCE as an independent third estimate / filter**. Trade when Kalshi diverges from the sharp consensus; size by edge minus Kalshi fees. Validation = **CLV vs the SHARP line** (not vs Kalshi's own possibly-soft close) and realized PnL after fees.

This also aligns the project with FORTUNA's actual venue — the Kalshi adapter already exists.

## Open questions (decision-blocking)

1. **Is Kalshi tennis actually inefficient vs the sharp line, and by how much?** Not yet measured. This is THE question — a few days of grass-season data (live right now) answers it.
2. **Real tradeable liquidity per match** beyond marquee events (depth field read 0; need order-book snapshots).
3. **Adverse selection on thin books** — do you only get filled when you're wrong / when Kalshi is stale-but-right?
4. **Coverage depth** — Slams + which tour tiers? (Halle ATP 500 confirmed; Challengers/ITF probably absent, but may not matter if main-tour is soft.)

## First live measurement (2026-06-19, grass season)

Ran the three-way capture (`deuce live-capture`) on 8 live ATP matches (Halle QFs +
Queen's/London). **All 8 matched to a sharp (Pinnacle/eu) line. Kalshi ≈ the sharp
line — max |Kalshi − sharp| was only ~1.3% (Zverev), most < 1%.** These are
*deep* books (Shelton–Fritz 176k OI, Medjedovic–Humbert 447k OI), and they track
the sharp consensus tightly. **The "Kalshi is soft" thesis does NOT hold on liquid
marquee matches** — efficiency rises with depth, as expected. If edge exists it
will be on thinner/lower-profile markets and/or in entry→close drift, which needs a
time series, not one snapshot. DEUCE diverged from the sharp line by 5–17% on
several matches — that is DEUCE *error* (it's ~0.04 logloss behind market), not
edge; trade Kalshi-vs-sharp, never DEUCE-vs-sharp.

**Benchmark corrected (same day):** the first pass used max-across-all-eu-books then
one devig — wrong (the "best price" is usually a soft-book outlier). Replaced with the
unweighted mean of **{Betfair-UK, Betfair-EU, Pinnacle, Matchbook}**, each devigged on
its OWN two prices (3–4 quoted per match), plus a cross-book **dispersion** column. The
finding held — **Kalshi ≈ sharp, max gap +0.015, most < 1%** — and the dispersion column
delivered the real insight: **`book_disp` (0.025–0.058) exceeds |Kalshi − sharp| in every
row.** The sharp books disagree with each other by more than Kalshi deviates from their
mean, so **Kalshi sits inside the sharp band — not an outlier, not tradeable** on these
liquid matches. The right question is "is Kalshi outside the noise floor," and here it is not.

Bug fixes: (b) particle/multi-word surnames ("de Minaur", "Davidovich Fokina") that
broke DEUCE name-matching → NaN are **FIXED** via a player alias-id map
(`identity.py` + `player_aliases.toml`, regenerable from Sackmann via `deuce
build-aliases`): both name formats now resolve to one canonical id, so the join is
exact, not a string collapse. (a) **Still open:** surface inferred from the Kalshi
string mislabels Queen's ("ATP London") as hard — derive surface from the matched
Odds API sport key (doesn't affect the Kalshi-vs-sharp signal).

## Sources (accessed 2026-06-18)
- Kalshi public API, `KXATPMATCH` series — https://api.elections.kalshi.com/trade-api/v2/markets (T1)
- Kalshi market pages: kalshi.com/markets/kxatpmatch, kalshi.com/markets/kxwtagrandslam (T1)
- CFTC self-cert filing, set-level tennis contract, 2026-01-16 (T1)
- Fees: marketmath.io/platforms/kalshi; kalshi.com/docs/kalshi-fee-schedule.pdf (T2/T1)
- Volume/venue: legalsportsreport.com (Kalshi $5B valuation, sports growth); casino.org / financemagnates.com ($100B lifetime); deadspin/thegameday/bettorsinsider reviews (T3)
