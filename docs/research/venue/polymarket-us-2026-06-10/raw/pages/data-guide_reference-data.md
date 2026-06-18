> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Reference Data

> Query instruments, symbols, and metadata from the Polymarket Exchange

Reference data provides information about available instruments and symbols on the Polymarket Exchange.

## Overview

The Reference Data API provides three endpoints:

* **List Instruments** - Returns complete instrument definitions including symbol, trading rules, state, dates, price limits, and market-specific metadata
* **List Symbols** - Returns just the symbol identifiers (trading symbols) without full instrument details
* **Get Metadata** - Returns server-level metadata about the exchange (not instrument-specific)

## Required Scope

| Scope              | Data Access                  |
| ------------------ | ---------------------------- |
| `read:instruments` | All reference data endpoints |

<Info>
  For details on the hierarchical structure of instruments (categories, series, events, products), see the [Asset Master](/data-guide/asset-master) guide.
</Info>

## REST API Endpoints

### List Instruments

Retrieve all available instruments:

```bash theme={null}
curl -X POST "https://api.preprod.polymarketexchange.com/v1/refdata/instruments" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{}'
```

**Response:**

```json theme={null}
{
  "instruments": [
    {
      "symbol": "aec-cbb-alcst-alast-2026-03-09",
      "tickSize": 0.01,
      "baseCurrency": "USD",
      "multiplier": 1,
      "minimumTradeQty": "1",
      "startDate": { "year": 2026, "month": 3, "day": 7 },
      "expirationDate": { "year": 2026, "month": 3, "day": 23 },
      "terminationDate": null,
      "tradingSchedule": [],
      "description": "Who will win the upcoming college basketball event...",
      "clearingHouse": "QCC",
      "minimumUnaffiliatedFirms": "0",
      "nonTradable": false,
      "jsonAttributes": "",
      "productId": "aec-cbb-alcst-alast-2026-03-09",
      "priceLimit": {
        "low": "1",
        "high": "99",
        "lowSet": true,
        "highSet": true,
        "relativeLow": 0,
        "relativeHigh": 0,
        "relativeLowSet": false,
        "relativeHighSet": false
      },
      "orderSizeLimit": null,
      "expirationTime": { "hours": 14, "minutes": 0, "seconds": 0 },
      "tradeSettlementPeriod": "0",
      "state": "INSTRUMENT_STATE_OPEN",
      "priceScale": "100",
      "fractionalQtyScale": "0",
      "settlementCurrency": "",
      "settlementPriceScale": "0",
      "metadata": {
        "cftc_instrument_id": "aec-cbb-alcst-alast-2026-03-09",
        "cftc_product_desc": "Athletic Event Contracts",
        "clearing_sym": "AEC-BASKETBALL",
        "event_category": "SPR",
        "event_external_id_sportradar": "4a55e75b-30b3-48c5-a90e-ecd3ae6f9de9",
        "event_external_id_sportsdataio": "60073199",
        "event_id": "cbb-alcst-alast-2026-03-09",
        "event_product_id": "aec-cbb-alcst-alast-2026-03-09",
        "event_series": "cbb",
        "event_start_time": "2026-03-09 18:00:00+00",
        "event_subcategory": "BASKETBALL",
        "instrument_product": "aec",
        "instrument_product_series": "aec-cbb",
        "instrument_rules": "Who will win the upcoming college basketball event...",
        "long_participant_id": "cbb-alcst",
        "long_participant_name": "Alcorn State",
        "outcome_strike": "0.0",
        "outcome_type": "moneyline",
        "participant_type": "team",
        "product_id": "aec-cbb-alcst-alast-2026-03-09",
        "short_participant_id": "cbb-alast",
        "short_participant_name": "Alabama State"
      },
      "eventAttributes": {
        "question": "Alcorn State vs. Alabama State",
        "payoutValue": "100",
        "evaluationType": ">",
        "eventId": "aec-cbb-alcst-alast-2026-03-09",
        "eventDisplayName": "Alcorn State vs. Alabama State 2026-03-09",
        "strikeValue": "0.0",
        "strikeUnit": "decimal",
        "calculationMethod": "CALCULATION_METHOD_VALUE",
        "positionAccountabilityValue": "50000",
        "timeSpecifier": { "year": 2026, "month": 3, "day": 9 }
      },
      "createTime": "2026-03-07T05:00:22.055331308Z",
      "updateTime": "2026-03-07T05:00:22.055331308Z"
    }
  ],
  "nextPageToken": "eyJvIjoyfQ==",
  "eof": false
}
```

<Warning>
  **Integer Fields Encoded as Strings**

  Fields typed as `int64` (such as `minimumTradeQty`, `priceScale`, `fractionalQtyScale`, `priceLimit.low`, `priceLimit.high`) are serialized as **strings** in JSON responses per the proto3 JSON specification. Parse these values as numbers in your client code.
</Warning>

<Info>
  The `settlementCurrency` and `settlementPriceScale` fields are reserved for future use and currently return empty string and `"0"` respectively.
</Info>

### List Symbols

Retrieve all trading symbols:

```bash theme={null}
curl -X POST "https://api.preprod.polymarketexchange.com/v1/refdata/symbols" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{}'
```

**Response:**

```json theme={null}
{
  "symbols": [
    "aec-nfl-buf-nyj-2025-01-15",
    "aec-nba-bos-lal-2025-01-20",
    "aec-nhl-tor-mtl-2025-01-18"
  ]
}
```

### Get Exchange Metadata

Retrieve server-level metadata about the exchange:

```bash theme={null}
curl -X POST "https://api.preprod.polymarketexchange.com/v1/refdata/metadata" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{}'
```

**Response:**

```json theme={null}
{
  "metadata": {
    "exchange_name": "Polymarket Exchange",
    "server_version": "v1.0.0",
    "timezone": "America/New_York"
  }
}
```

Note: For instrument-specific metadata (participant names, event details, etc.), use the `/v1/refdata/instruments` endpoint which includes a `metadata` field in each instrument object.

## Instrument Lifecycle

Instruments follow the primary lifecycle: PENDING → OPEN → CLOSED → EXPIRED → TERMINATED. Instruments may also be SUSPENDED or HALTED during their lifecycle.

State values are prefixed with `INSTRUMENT_STATE_` in the API (e.g., `INSTRUMENT_STATE_OPEN`).

### Primary State Flow

| State                                               | Description                                                                                                                                                                                                      |
| --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PENDING`                                           | Initial state for a newly created instrument which has not yet begun trading.                                                                                                                                    |
| `OPEN`                                              | In this state, the instrument is open for continuous order entry and matching.                                                                                                                                   |
| `CLOSED`                                            | In this state, orders can not be entered, modified, or canceled, and no matching occurs. Any existing Day orders will be expired.                                                                                |
| `EXPIRED`                                           | An instrument moves to this state when its Expiration Date/Time is reached. In this state, any resting orders are expired and no new orders can be entered.                                                      |
| `TERMINATED`                                        | When an instrument's Termination Date is reached, the order book is removed from the matching engine, orders are canceled, and positions are closed. Historical data will still remain in Polymarket US ledgers. |

### Exception States

| State                                               | Description                                                                                   |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `SUSPENDED`                                         | Orders can be canceled but no matching occurs, and no order entry or modification is allowed. |
| `HALTED`                                            | This state is similar to SUSPENDED, with the exception that orders cannot be canceled.        |

### Other Possible States

| State                                               | Description                                                                                                                                                                                                                                                                                       |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PREOPEN`                                           | Orders can be entered and modified, but no matching occurs. When the instrument transitions to an OPEN state, the orders entered during PREOPEN will match at a single opening price that is automatically determined by an algorithm that is designed to maximize the volume traded at the open. |
| `MATCH_AND_CLOSE_AUCTION`                           | This state is similar to PREOPEN, with the exception that matching will occur upon the transition of this state to any other state. This state is useful if you want matching to occur at the end of the state, but you don't want the instrument to be open after.                               |

## Best Practices

### Caching

Reference data changes infrequently. Cache locally and refresh periodically:

```python theme={null}
import time

class ReferenceDataCache:
    def __init__(self, api_client, refresh_interval=300):
        self.api_client = api_client
        self.refresh_interval = refresh_interval
        self.instruments = {}
        self.last_refresh = 0

    def get_instruments(self):
        if time.time() - self.last_refresh > self.refresh_interval:
            self._refresh()
        return self.instruments

    def _refresh(self):
        response = self.api_client.list_instruments()
        self.instruments = {i['symbol']: i for i in response['instruments']}
        self.last_refresh = time.time()
```

### Filtering

Filter by instrument state:

```bash theme={null}
curl -X POST "https://api.preprod.polymarketexchange.com/v1/refdata/instruments" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "states": ["INSTRUMENT_STATE_OPEN"]
  }'
```

Filter by event series:

```bash theme={null}
curl -X POST "https://api.preprod.polymarketexchange.com/v1/refdata/instruments" \
  -H "Authorization: Bearer YOUR_ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "event_series": "nfl"
  }'
```

### Handling Updates

* Subscribe to instrument updates for real-time changes
* Check instrument status before placing orders
* Monitor for new instruments being listed
