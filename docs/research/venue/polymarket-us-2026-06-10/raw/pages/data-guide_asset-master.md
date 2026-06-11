> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Asset Master

> Hierarchical structure of instruments on the Polymarket Exchange

Every instrument on the Polymarket Exchange follows a hierarchical structure that organizes markets from broad categories down to specific tradable outcomes.

## Ontology Overview

The structure is: `Category → Subcategory → Series → Event → Product → Instrument(s)`

* **Category, Subcategory, Series** classify events (e.g., Sports → Soccer → MLS)
* **Events** represent real-world occurrences (games, elections, price movements)
* **Products** are templates that define different ways to trade those events
* **Instruments** are the specific tradable outcomes

A single event can have multiple products applied to it, creating different groups of instruments.

**Example:**

* Event: `mls-atl-clt-2026-03-22` (Atlanta United vs Charlotte FC)
* Products applied: ATC (3-way outcome), ASC (spreads), TSC (totals)
* Each product creates its own set of instruments for the same underlying event

## Complete Example

Here's a real CBB moneyline instrument showing the complete structure with all metadata:

```json theme={null}
{
  "id": "aec-cbb-char-hamp-2026-02-26",
  "product_id": "aec-cbb-char-hamp-2026-02-26",
  "description": "Who will win the college basketball event, scheduled for 2026-02-26, Charleston or Hampton?",
  "price_scale": 100,
  "attributes": {
    "tick_size": 0.01,
    "minimum_trade_qty": 1,
    "start_date": "2026-02-05",
    "expiration_date": "2026-03-12",
    "expiration_time": "12:00:00",
    "last_trade_date": "2026-03-12",
    "last_trade_time": "12:00:00",
    "price_limit": {
      "low": 1,
      "high": 99,
      "low_set": true,
      "high_set": true
    },
    "base_currency": "USD",
    "multiplier": 1,
    "clearing_house": "QCC",
    "cfi_code": "OMXXXX",
    "settlement_price_logic": "SETTLEMENT_PRICE_LOGIC_EVENT",
    "trade_day_roll_schedule": {
      "days_of_week": ["MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY", "SUNDAY"],
      "time_of_day": "17:00:00"
    }
  },
  "metadata": {
    "cftc_instrument_id": "aec-cbb-char-hamp-2026-02-26",
    "clearing_sym": "AEC-NCAA",
    "event_category": "SPR",
    "event_subcategory": "ncaa",
    "event_series": "cbb",
    "event_id": "cbb-char-hamp-2026-02-26",
    "event_start_time": "2026-02-26 16:00:00+00",
    "instrument_product": "aec",
    "instrument_product_series": "aec-cbb",
    "product_id": "aec-cbb-char-hamp-2026-02-26",
    "instrument_rules": "Who will win the upcoming college basketball event, scheduled for February 26 at 11:00AM ET, Charleston or Hampton? In the case of a tie or draw, the Exchange may, in its sole discretion, settle the instrument as it deems fair and appropriate (e.g., at last-traded prices, $0.50 per instrument, or other fair and equitable valuations). If the event is postponed, delayed, or rescheduled, the settlement date will be amended to the rescheduled date. If the rescheduled date is not within two weeks of the originally scheduled date, the Exchange may, in its sole discretion, settle the instrument as it deems fair and appropriate. Should the event be rescheduled to an earlier date than originally scheduled, the settlement date will be the rescheduled date. If the event is canceled, the Exchange may, in its sole discretion, settle the instrument as it deems fair and appropriate. If the event concludes early, or is shortened or truncated, the outcome shall be the declared official result. In the case of a withdrawal, walkover, forfeit, no-contest, or the removal of a participant in the event, the Exchange may, in its sole discretion, settle the instrument as it deems fair and appropriate.",
    "participant_type": "team",
    "long_participant_id": "cbb-char",
    "long_participant_name": "Charleston",
    "short_participant_id": "cbb-hamp",
    "short_participant_name": "Hampton",
    "outcome_type": "moneyline",
    "outcome_strike": "char",
    "event_external_id_sportsdataio": "60068117",
    "event_external_id_sportradar": "5c486835-c9a3-423a-b47a-c3fc48fda799"
  },
  "event_attributes": {
    "position_accountability_value": 50000,
    "payout_value": 100,
    "question": "CBB 2026 Moneyline Game - Charleston vs Hampton 2026-02-26",
    "event_display_name": "Charleston vs. Hampton 2026-02-26",
    "event_id": "aec-cbb-char-hamp-2026-02-26",
    "strike_value": "0.0",
    "evaluation_type": "==",
    "strike_unit": "decimal",
    "calculation_method": "CALCULATION_METHOD_VALUE",
    "time_specifier": "2026-02-26"
  }
}
```

This example shows how all levels (Category → Subcategory → Series → Event → Product → Instrument → Participants → Outcome) come together in a single tradable instrument.

## Event

Events represent specific real-world occurrences (games, elections, price movements). Each event is classified by category, subcategory, and series, and can have multiple products applied to create different groups of instruments.

**Event ID Format:** `{series}-{descriptors}-{date/time}`

Where descriptors vary by event type:

* **Sports**: `mls-atl-clt-2026-03-22`
* **Politics**: `uspres-2028-11-05`
* **Crypto**: `btc-hit-2026-12-31`
* **Culture**: `oscars-2027-03-28`

**Metadata Fields:**

* `event_id` - Unique event identifier (e.g., `nfl-hou-mia-2025-12-16`)
* `event_start_time` - Event start timestamp (UTC)
* `event_category` - Category code (e.g., `SPR`, `POL`, `CRY`, `FIN`)
* `event_subcategory` - Subcategory code (e.g., `football`, `soccer`, `coin`, `uspres`)

**Example:** Event `mls-atl-clt-2026-03-22` with multiple products applied:

* ATC → 3 instruments (atl, draw, clt)
* ASC → multiple instruments (various spreads)
* TSC → multiple instruments (various totals)

### Classification Reference

**Categories:**

* **SPR** - Sports
* **POL** - Politics
* **CRY** - Crypto
* **CUL** - Culture
* **FIN** - Finance
* **MAC** - Macro
* **CLI** - Climate
* **GEO** - Geopolitics
* **TECH** - Technology
* **MEN** - Mentions

**Subcategories by Category:**

* **SPR**: soccer, football, basketball, baseball, combat, tennis, icehockey, ncaa, cricket, esports, golf, motorsport, olympics, rugby
* **POL**: uspres, ushse, usstate, ussen, intlpol, legislate, cabinet, fedagency, uscrt
* **CRY**: coin, cryptomkt, nft
* **CUL**: movies, music, tv, video, awd, people, entertain
* **FIN**: indices, forex, treasuries, bankruptcy, corpaction, earnings, ipo, commod
* **CLI**: weather, climate, geological, health, space
* **GEO**: namer, eur, apac, mena, latam, ssa, conflict, unitednat
* **MAC**: growth, inflation, monetary, employment, fiscal
* **TECH**: ai, autonom, mobile, platforms, social, compute, cybersec, industry, wear
* **MEN**: statement, pressconf, earncall

**Series by Subcategory:**

* **soccer**: epl, laliga, seriea, bund, ligue1, ucl, uel, facup, cara, wcup
* **football**: nfl
* **basketball**: nba
* **baseball**: mlb
* **ncaa**: cfb, cbb
* **coin**: btc, eth, sol, xrp, bnb, doge, usdc, usdt
* **uspres**: usp, prm, pres

## Product

Products are templates that define types of tradable outcomes. The same product can be applied to events across different series — products define *how* you trade an event, not *what* event you're trading.

**Metadata Fields:**

* `instrument_product` - Product code (e.g., `aec`, `asc`, `atc`, `tsc`)
* `instrument_product_series` - Combined product and series (e.g., `aec-nfl`, `atc-mls`). This is the canonical field for identifying the series of an instrument. Use this field for filtering by series rather than `event_series`

**Market Structures:**

* **Single (Binary)** - One instrument representing yes and no outcomes
* **Group (Exclusive)** - Multiple instruments, where only one outcome can be true
* **Group (Directional)** - Multiple instruments, where multiple can be true, and the outcomes sit in directional relation
* **Group (Independent)** - Multiple instruments, where multiple can be true, but the outcomes do not sit in directional relation

### Example Products

| Product         | Code | Type                | Example                               | Description                            |
| --------------- | ---- | ------------------- | ------------------------------------- | -------------------------------------- |
| Athletic Event  | AEC  | Single (Binary)     | `aec-nfl-buf-nyj-2025-01-15`          | Moneyline: Will team A win?            |
| Athletic Tie    | ATC  | Group (Exclusive)   | `atc-mls-atl-clt-2026-03-22-draw`     | 3-way: Team A, Draw, or Team B wins    |
| Athletic Spread | ASC  | Group (Directional) | `asc-nfl-hou-mia-2025-12-16-pos-4pt5` | Will team A win by more than X points? |
| Total Score     | TSC  | Group (Directional) | `tsc-nfl-ne-den-2026-01-25-47-5`      | Will combined score be over X?         |
| Title Event     | TEC  | Group (Exclusive)   | `tec-ggb-bmpd-2026-01-11`             | Will participant win title?            |
| Title Award     | TAC  | Group (Exclusive)   | `tac-ggb-bmpd-2026-01-11-sinners`     | Which nominee will win award?          |
| Election Winner | EWC  | Group (Exclusive)   | `ewc-usp-pres-2028-11-07`             | Which candidate will win election?     |
| Crypto Price    | CPC  | Single (Binary)     | `cpc-btc-2026-12-31`                  | Price movement in period?              |

### Product Reusability

The same product can create instruments across different series:

**ATC (Athletic Tie Contract)** - 3-way match outcome:

* `atc-mls-atl-clt-2026-03-22-draw` (MLS)
* `atc-epl-liv-mci-2026-01-15-draw` (EPL)
* `atc-ucl-bar-psg-2026-04-20-draw` (UCL)

**ASC (Athletic Spread Contract)** - Point spreads:

* `asc-nfl-hou-mia-2025-12-16-pos-4pt5`
* `asc-nba-bos-lal-2026-01-20-pos-6pt5`
* `asc-cbb-duke-unc-2026-02-15-pos-3pt5`

## Instrument

Instruments are specific tradable outcomes created by applying a Product to an Event. Each instrument has a globally unique ID that combines the product code, event details, and specific outcome/strike.

**Instrument ID Format:** `{product_code}-{event_id}-{strike/outcome}`

**Metadata Fields:**

* `instrument_rules` - Resolution rules specific to this instrument

**Examples:**

* `aec-nfl-buf-nyj-2025-01-15` - Moneyline (no additional outcome specified)
* `atc-mls-atl-clt-2026-03-22-draw` - 3-way outcome: draw
* `asc-nfl-hou-mia-2025-12-16-pos-4pt5` - Spread: 4.5 points
* `tsc-nfl-ne-den-2026-01-25-47-5` - Total: 47.5 points

### Participants and Outcome

Each instrument has participants representing the possible outcomes. The long participant represents the "Yes" outcome that traders buy and sell. This is surfaced in instrument metadata:

* `participant_type` - Type of participant (team, player, nominee, candidate, etc.)
* `long_participant_id` - Globally unique ID for the long side (e.g., `cbb-akron`, `nfl-buf`)
* `long_participant_name` - Full display name for the long side (e.g., "Akron", "Buffalo Bills")
* `short_participant_id` - Globally unique ID for the opposing outcome (e.g., `cbb-murst`, `nfl-nyj`)
* `short_participant_name` - Full display name for the opposing outcome (e.g., "Murray State", "New York Jets")

Instruments are traded by buying and selling the long participant (the "Yes" outcome). For example, in an NFL moneyline contract like `nfl-hou-mia-2025-12-16`, traders buy and sell HOU. Buying HOU means taking a long position on Houston winning, while selling HOU creates a synthetic long position on Miami.

There is no direct way to trade the short participant - all positions on the opposing outcome are achieved synthetically by selling the long participant.

When you sell (short) an instrument, the cash flows differ from buying. Selling 10 contracts at \$0.60 means you receive \$6 from the buyer, but a margin requirement equal to the maximum payout (\$10 in this case) is imposed on your account. Therefore, you need \$4 in available funds to enter this short position (\$10 margin requirement minus \$6 received). This margin requirement ensures you can cover the full payout if the outcome occurs.
