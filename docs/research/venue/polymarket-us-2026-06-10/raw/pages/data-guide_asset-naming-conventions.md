> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Naming Conventions

> ID formats and naming patterns for instruments and events

IDs follow consistent patterns based on the hierarchy:

## Complete Hierarchy

```
Category → Subcategory → Series → Event + Product → Instrument
```

**Example:**

```
SPR → SOCCER → MLS → mls-atl-clt-2026-03-22 + ATC → atc-mls-atl-clt-2026-03-22-draw
```

## Event ID Format

Events are identified by series, descriptors, and date/time:

```
{series}-{descriptors}-{date/time}
```

**Sports**:

* `nfl-ne-den-2026-01-25`
* `mls-atl-clt-2026-03-22`

**Politics**:

* `uspres-2028-11-05`
* `ussen-2026-11-03`

**Crypto**:

* `btc-hit-2026-12-31`
* `eth-hit-2027-06-30`

**Culture**:

* `oscars-2027-03-28`
* `grammys-2027-02-14`

## Instrument ID Format

Instruments combine product, event, and outcome:

```
{product_code}-{event_id}-{strike/outcome}
```

**Examples:**

* `aec-nfl-ne-den-2026-01-25` - Moneyline (no additional outcome)
* `atc-mls-atl-clt-2026-03-22-draw` - 3-way outcome: draw
* `atc-mls-atl-clt-2026-03-22-atl` - 3-way outcome: Atlanta wins
* `asc-nfl-hou-mia-2025-12-16-pos-4pt5` - Spread: 4.5 points
* `tsc-nfl-ne-den-2026-01-25-47-5` - Total: 47.5 points

## Participant ID Format

```
{series}-{abbreviation}
```

**Examples:**

* `nfl-ne` (New England)
* `nfl-den` (Denver)
* `nba-bos` (Boston Celtics)
* `usp-harris` (Kamala Harris)

Participant abbreviations are standardized across all instruments in a series to enable consistent querying and position tracking.

### Participant Lookup API

Participant abbreviations and display names can be looked up via the public teams endpoint:

```bash theme={null}
GET https://gateway.polymarket.us/v1/sports/teams?limit=500&filters.league={series}
```

**Example:** Fetch all MLS teams:

```bash theme={null}
GET https://gateway.polymarket.us/v1/sports/teams?limit=500&filters.league=mls
```

**Response:**

```json theme={null}
{
  "teams": [
    {
      "id": "2263",
      "name": "Chicago Fire FC",
      "abbreviation": "chi",
      "league": "mls",
      "displayAbbreviation": "CHI",
      "alias": "The Fire",
      "logo": "https://polymarket-upload.s3.us-east-2.amazonaws.com/us-logos/MLS/Team%3DChicago+Fire+FC.png",
      "colorPrimary": "#5FC1EA",
      "record": "0-0-0",
      "providerIds": [
        {"provider": "PROVIDER_SPORTRADAR", "providerId": "sr:competitor:2505"},
        {"provider": "PROVIDER_SPORTSDATAIO", "providerId": "694"}
      ]
    }
  ]
}
```

**Mapping to instrument metadata:**

| Teams endpoint              | Instrument metadata                                                           | Example                                 |
| --------------------------- | ----------------------------------------------------------------------------- | --------------------------------------- |
| `abbreviation`              | `long_participant_id` / `short_participant_id` (as `{league}-{abbreviation}`) | `chi` → `mls-chi`                       |
| `name`                      | `long_participant_name` / `short_participant_name`                            | `Chicago Fire FC`                       |
| `league`                    | `instrument_product_series` (series component)                                | `mls` (from `aec-mls`, `atc-mls`, etc.) |
| `providerIds[SPORTSDATAIO]` | `event_external_id_sportsdataio`                                              | `694`                                   |
| `providerIds[SPORTRADAR]`   | `event_external_id_sportradar`                                                | `sr:competitor:2505`                    |

The endpoint also provides display data not present in instrument metadata: `logo`, `colorPrimary`, `alias`, `displayAbbreviation`, and `record`.

## Uniqueness Rules

Each level of the hierarchy has specific uniqueness scoping:

| Level           | Uniqueness Scope                                          | Example                            |
| --------------- | --------------------------------------------------------- | ---------------------------------- |
| **Category**    | Globally unique                                           | SPR, POL, CRY                      |
| **Subcategory** | Unique within Category                                    | SOCCER within SPR, COIN within CRY |
| **Series**      | Unique within Subcategory                                 | MLS within SOCCER, BTC within COIN |
| **Event**       | Unique within Series (globally unique with series prefix) | `mls-atl-clt-2026-03-22`           |
| **Product**     | Globally unique by Code                                   | ATC, AEC, ASC                      |
| **Instrument**  | Globally unique                                           | `atc-mls-atl-clt-2026-03-22-draw`  |

**Key Points:**

* Events belong to exactly one Series
* Products are independent and can be applied to events across any Series
* Instruments are globally unique combinations of Product + Event + Outcome

## Multi-Product Events

A single event can have multiple products applied to it, each creating its own set of instruments. This allows traders to speculate on the same event in different ways.

**Hierarchy:**

```
SPR → FOOTBALL → NFL → nfl-ne-den-2026-01-25
```

**Event:** `nfl-ne-den-2026-01-25` (New England vs Denver on Jan 25, 2026)

**Products Applied:**

1. **AEC (Athletic Event Contract)** - Single Binary
   * Instrument: `aec-nfl-ne-den-2026-01-25`
   * Question: "Will New England win?"
   * Outcome: moneyline, strike: 0.0

2. **ASC (Athletic Spread Contract)** - Group Directional
   * Instrument: `asc-nfl-ne-den-2026-01-25-pos-3pt5`
   * Question: "Will New England win by more than 3.5 points?"
   * Outcome: spread, strike: 3.5

3. **TSC (Total Score Contract)** - Group Directional
   * Instrument: `tsc-nfl-ne-den-2026-01-25-47-5`
   * Question: "Will total points be over 47.5?"
   * Outcome: total, strike: 47.5

All three instruments reference the same `event_id` but have different product codes, `outcome_type`, and `outcome_strike` values.

### Soccer Example with 3-Way Markets

**Hierarchy:**

```
SPR → SOCCER → MLS → mls-atl-clt-2026-03-22
```

**Event:** `mls-atl-clt-2026-03-22` (Atlanta United vs Charlotte FC)

**Products Applied:**

1. **ATC (Athletic Tie Contract)** - Group Exclusive (3-way)
   * `atc-mls-atl-clt-2026-03-22-atl` (Atlanta wins)
   * `atc-mls-atl-clt-2026-03-22-draw` (Draw)
   * `atc-mls-atl-clt-2026-03-22-clt` (Charlotte wins)

2. **AEC (Athletic Event Contract)** - Single Binary
   * `aec-mls-atl-clt-2026-03-22` (Will Atlanta win?)

3. **TSC (Total Score Contract)** - Group Directional
   * `tsc-mls-atl-clt-2026-03-22-2-5` (Over 2.5 goals)
   * `tsc-mls-atl-clt-2026-03-22-3-5` (Over 3.5 goals)

The same event supports both 3-way markets (ATC) and binary markets (AEC), plus totals (TSC).

## Complete Metadata Reference

All metadata fields available on instruments:

| Field                            | Level       | Required | Type      | Example                       | Description                                                                       |
| -------------------------------- | ----------- | -------- | --------- | ----------------------------- | --------------------------------------------------------------------------------- |
| `cftc_instrument_id`             | Instrument  | Yes      | String    | `"aec-nfl-ne-den-2026-01-25"` | CFTC registered instrument ID                                                     |
| `clearing_sym`                   | Instrument  | Yes      | String    | `"AEC-NFL"`                   | Clearing symbol prefix                                                            |
| `event_category`                 | Category    | Yes      | String    | `"SPR"`                       | Category code (SPR, POL, CRY, etc.)                                               |
| `event_series`                   | Series      | Yes      | String    | `"nfl"`                       | Series code within category                                                       |
| `instrument_product`             | Product     | Yes      | String    | `"aec"`                       | Product type code                                                                 |
| `instrument_product_series`      | Product     | Yes      | String    | `"aec-nfl"`                   | Combined product and series                                                       |
| `product_id`                     | Product     | Yes      | String    | `"aec-nfl-ne-den-2026-01-25"` | Product identifier                                                                |
| `event_id`                       | Event       | Yes      | String    | `"nfl-ne-den-2026-01-25"`     | Unique event identifier                                                           |
| `event_start_time`               | Event       | Yes      | Timestamp | `"2026-01-25 20:00:00+00"`    | Event start time (UTC)                                                            |
| `event_external_id_sportsdataio` | Event       | No       | String    | `"19449"`                     | SportsDataIO ID                                                                   |
| `event_external_id_sportradar`   | Event       | No       | String    | `"5848514c-..."`              | Sportradar ID                                                                     |
| `instrument_rules`               | Instrument  | Yes      | String    | `"Who will win..."`           | Instrument-specific rules                                                         |
| `participant_type`               | Participant | Yes      | String    | `"team"`                      | Type: team, player, nominee, etc.                                                 |
| `long_participant_id`            | Participant | Yes      | String    | `"nfl-ne"`                    | Long side participant ID                                                          |
| `long_participant_name`          | Participant | Yes      | String    | `"New England"`               | Long side display name                                                            |
| `short_participant_id`           | Participant | Yes      | String    | `"nfl-den"`                   | Short side participant ID                                                         |
| `short_participant_name`         | Participant | Yes      | String    | `"Denver"`                    | Short side display name                                                           |
| `outcome_type`                   | Outcome     | Yes      | String    | `"moneyline"`                 | Outcome type                                                                      |
| `outcome_strike`                 | Outcome     | Yes      | String    | `"ne"`                        | Strike value (participant abbreviation for moneyline, numeric for spreads/totals) |

### Attributes vs Metadata vs Event Attributes

**Attributes** contain trading and settlement parameters:

* `tick_size` - Minimum price increment
* `minimum_trade_qty` - Minimum order size
* `price_limit` - Price bounds (low, high, low\_set, high\_set)
* `expiration_date` - Contract expiration
* `expiration_time` - Settlement time
* `last_trade_date` - Final trading day
* `last_trade_time` - Final trading time
* `base_currency` - Settlement currency
* `clearing_house` - Clearing organization
* `cfi_code` - Classification of Financial Instruments code
* `settlement_price_logic` - Settlement logic type
* `trade_day_roll_schedule` - Trading day rollover schedule

**Metadata** contains descriptive and reference information:

* `cftc_instrument_id` - CFTC registered instrument identifier
* `clearing_sym` - Clearing symbol prefix
* Event details (category, series, participants)
* Resolution rules
* External data provider IDs
* Display names and formatting

**Event Attributes** contain event-specific trading parameters:

* `position_accountability_value` - Position limit threshold
* `payout_value` - Contract payout amount
* `question` - Human-readable question
* `event_display_name` - Formatted display name for the event
* `event_id` - Event identifier
* `strike_value` - Strike value for the outcome
* `evaluation_type` - Comparison operator for resolution
* `strike_unit` - Unit type for strike (string, decimal)
* `calculation_method` - Settlement calculation method
* `time_specifier` - Date for event occurrence

### External Data Provider IDs

External IDs enable integration with third-party data sources:

**`event_external_id_sportsdataio`**

* SportsDataIO event identifier
* Used for real-time scores and statistics
* Example: `"19449"`

**`event_external_id_sportradar`**

* Sportradar event identifier (UUID format)
* Used for official data feeds and settlement
* Example: `"5848514c-3977-4aa3-9db0-94ed5d0ebb34"`

These IDs allow automated resolution based on official data provider results.

## Common Query Patterns

Use metadata fields to filter and find specific instruments:

### Filter by Series

Use `instrument_product_series` for reliable series filtering. This field combines the product code and series (e.g., `aec-nfl`, `atc-mls`) and is consistently populated across all instrument types.

Find all NFL moneyline instruments:

```
metadata.instrument_product_series = "aec-nfl"
```

Find all NBA moneyline instruments:

```
metadata.instrument_product_series = "aec-nba"
```

Find all NFL instruments (any product):

```
metadata.instrument_product_series LIKE "%-nfl"
```

### Filter by Product Type

Find all moneyline contracts:

```
metadata.outcome_type = "moneyline"
```

Find all spread contracts:

```
metadata.outcome_type = "spread"
```

### Filter by Participant

Find all instruments involving New England:

```
metadata.long_participant_id = "nfl-ne" OR metadata.short_participant_id = "nfl-ne"
```

### Filter by Event

Find all instruments for a specific game:

```
metadata.event_id = "nfl-ne-den-2026-01-25"
```

### Combined Filters

Find all NFL moneyline contracts:

```
metadata.instrument_product_series = "aec-nfl"
```

Find all spread contracts with New England:

```
metadata.outcome_type = "spread" AND
(metadata.long_participant_id = "nfl-ne" OR metadata.short_participant_id = "nfl-ne")
```
