> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# List instruments

> Returns a list of instruments matching the request



## OpenAPI

````yaml /institutional/oapi-schemas/refdata-schema.json post /v1/refdata/instruments
openapi: 3.0.1
info:
  title: Refdata API
  version: v1.0.0
servers:
  - url: https://api.prod.polymarketexchange.com
security: []
tags:
  - name: RefDataAPI
paths:
  /v1/refdata/instruments:
    post:
      tags:
        - ReferenceData
      summary: List instruments
      description: Returns a list of instruments matching the request
      operationId: RefDataAPI_ListInstruments
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/ListInstrumentsRequest'
        required: true
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ListInstrumentsResponse'
components:
  schemas:
    ListInstrumentsRequest:
      type: object
      properties:
        pageSize:
          type: integer
          format: int32
          description: 'Results per page (default: 50, max: 1000)'
        pageToken:
          type: string
          description: Pagination cursor from previous response
        symbols:
          type: array
          items:
            type: string
          description: Filter by specific instrument symbols
        productId:
          type: string
          description: Filter by product ID
        tradableFilter:
          $ref: '#/components/schemas/TradableFilter'
          description: Filter tradable vs non-tradable instruments
        states:
          type: array
          items:
            $ref: '#/components/schemas/InstrumentState'
          description: Filter by instrument states (e.g., INSTRUMENT_STATE_OPEN)
        eventSeries:
          type: string
          description: Filter by event series (e.g., 'nfl', 'nba', 'cbb')
        eventCategory:
          type: string
          description: Filter by event category (e.g., 'SPR', 'POL', 'CRY')
        clearingSym:
          type: string
          description: Filter by clearing symbol prefix (e.g., 'AEC-NFL')
        startTimeGte:
          type: string
          description: >-
            Filter instruments starting on or after this date (format:
            'YYYY-MM-DD')
        startTimeLte:
          type: string
          description: Filter instruments starting on or before this date
        endTimeGte:
          type: string
          description: Filter instruments expiring on or after this date
        endTimeLte:
          type: string
          description: Filter instruments expiring on or before this date
        filter:
          $ref: '#/components/schemas/InstrumentFilter'
          description: >-
            Advanced filter with whereClause and/or fieldFilters (ANDed together
            if both provided)
      description: Request for listing instruments with optional filters and pagination.
    ListInstrumentsResponse:
      type: object
      properties:
        instruments:
          type: array
          items:
            $ref: '#/components/schemas/Instrument'
            type: object
          description: List of matching instruments
        nextPageToken:
          type: string
          description: Token for fetching next page (empty if no more results)
        eof:
          type: boolean
          description: True when no more results available
      description: Response with list of instruments and pagination info.
    TradableFilter:
      type: string
      enum:
        - TRADABLE_FILTER_TRADABLE
        - TRADABLE_FILTER_NON_TRADABLE
        - TRADABLE_FILTER_ALL
      description: >-
        TradableFilter for filtering instruments by tradability. TRADABLE
        returns instruments where nonTradable=false. NON_TRADABLE returns
        instruments where nonTradable=true. ALL returns all instruments
        regardless of tradability.
    InstrumentState:
      type: string
      enum:
        - INSTRUMENT_STATE_CLOSED
        - INSTRUMENT_STATE_OPEN
        - INSTRUMENT_STATE_PREOPEN
        - INSTRUMENT_STATE_SUSPENDED
        - INSTRUMENT_STATE_EXPIRED
        - INSTRUMENT_STATE_TERMINATED
        - INSTRUMENT_STATE_HALTED
        - INSTRUMENT_STATE_MATCH_AND_CLOSE_AUCTION
        - INSTRUMENT_STATE_PENDING
      description: >-
        **PENDING**: Initial state for a newly created instrument which has not
        yet begun trading.


        **CLOSED**: No order entry, modification, cancellation, or matching. Day
        orders expired.


        **OPEN**: Continuous order entry and matching.


        **PREOPEN**: Orders accepted, no matching. Dutch Auction on transition
        to OPEN.


        **MATCH_AND_CLOSE_AUCTION**: Like PREOPEN, matches on transition out.


        **SUSPENDED**: Cancel only. No entry, modification, or matching.


        **HALTED**: Like SUSPENDED, no cancels allowed.


        **EXPIRED**: All resting orders expired, no new orders.


        **TERMINATED**: Order book removed, all orders and positions closed.
    InstrumentFilter:
      type: object
      properties:
        whereClause:
          type: string
          description: >-
            SQL-like string expression. Supported operators: =, LIKE (with %
            wildcard), AND. Supported columns: state, symbol, event_series,
            event_category, clearing_sym, clearing_house, product_id. Do NOT use
            instrument_product, outcome_type, or event_subcategory (causes HTTP
            500).
        fieldFilters:
          type: array
          items:
            $ref: '#/components/schemas/FieldFilter'
          description: >-
            Structured type-safe filters. ANDed with whereClause if both
            provided.
      description: >-
        InstrumentFilter for advanced instrument queries. Both whereClause and
        fieldFilters can be used independently or combined (ANDed together).
    Instrument:
      type: object
      properties:
        symbol:
          type: string
        tickSize:
          type: number
          format: double
        baseCurrency:
          type: string
        multiplier:
          type: number
          format: double
        minimumTradeQty:
          type: string
          format: int64
        startDate:
          $ref: '#/components/schemas/Date'
        expirationDate:
          $ref: '#/components/schemas/Date'
        terminationDate:
          $ref: '#/components/schemas/Date'
        tradingSchedule:
          type: array
          items:
            $ref: '#/components/schemas/TradingHours'
            type: object
        description:
          type: string
        clearingHouse:
          type: string
        minimumUnaffiliatedFirms:
          type: string
          format: int64
        nonTradable:
          type: boolean
          title: type_attributes omitted - complex oneof with many swap types
        jsonAttributes:
          type: string
        productId:
          type: string
        priceLimit:
          $ref: '#/components/schemas/PriceLimit'
        orderSizeLimit:
          $ref: '#/components/schemas/OrderSizeLimit'
        expirationTime:
          $ref: '#/components/schemas/TimeOfDay'
        tradeSettlementPeriod:
          type: string
          format: int64
        state:
          $ref: '#/components/schemas/InstrumentState'
        priceScale:
          type: string
          format: int64
        fractionalQtyScale:
          type: string
          format: int64
        settlementCurrency:
          type: string
        settlementPriceScale:
          type: string
          format: int64
        metadata:
          type: object
          additionalProperties:
            type: string
          description: >-
            Additional instrument metadata including sports league, market
            category, and game identifiers
        eventAttributes:
          $ref: '#/components/schemas/EventAttributes'
        createTime:
          type: string
          format: date-time
          description: Instrument creation timestamp (RFC 3339)
        updateTime:
          type: string
          format: date-time
          description: Last update timestamp (RFC 3339)
      description: |-
        Instrument represents a tradable instrument.
        Wire-compatible with Connamara EP3 Instrument (core fields).
    FieldFilter:
      type: object
      properties:
        field:
          type: string
          description: >-
            Column name to filter on. Supported: state, symbol, event_series,
            event_category, clearing_sym, clearing_house, product_id.
        operator:
          $ref: '#/components/schemas/FilterOperator'
          description: Comparison operator
        stringValue:
          type: string
          description: String value to compare against (for EQ, LIKE)
        stringList:
          $ref: '#/components/schemas/StringList'
          description: List of string values (for IN operator)
      description: >-
        FieldFilter for structured filtering. Provide exactly one of stringValue
        or stringList depending on the operator.
    Date:
      type: object
      properties:
        year:
          type: integer
          format: int32
        month:
          type: integer
          format: int32
        day:
          type: integer
          format: int32
      description: Date represents a calendar date.
    TradingHours:
      type: object
      properties:
        daysOfWeek:
          type: array
          items:
            type: integer
            format: int32
        timeOfDay:
          $ref: '#/components/schemas/TimeOfDay'
        duration:
          type: string
        state:
          $ref: '#/components/schemas/InstrumentState'
        hideMarketData:
          type: boolean
        expireAllOrders:
          type: boolean
      description: TradingHours describes an instrument trading schedule segment.
    PriceLimit:
      type: object
      properties:
        low:
          type: string
          format: int64
        high:
          type: string
          format: int64
        lowSet:
          type: boolean
        highSet:
          type: boolean
        relativeLow:
          type: number
          format: double
        relativeHigh:
          type: number
          format: double
        relativeLowSet:
          type: boolean
        relativeHighSet:
          type: boolean
      description: PriceLimit describes optional price limits on an instrument.
    OrderSizeLimit:
      type: object
      properties:
        low:
          type: string
          format: int64
        high:
          type: string
          format: int64
        lowSet:
          type: boolean
        highSet:
          type: boolean
        totalNotionalLow:
          type: string
          format: int64
        totalNotionalHigh:
          type: string
          format: int64
        totalNotionalLowSet:
          type: boolean
        totalNotionalHighSet:
          type: boolean
      description: OrderSizeLimit describes optional order size limits on an instrument.
    TimeOfDay:
      type: object
      properties:
        hours:
          type: integer
          format: int32
        minutes:
          type: integer
          format: int32
        seconds:
          type: integer
          format: int32
      description: TimeOfDay represents a time of day.
    EventAttributes:
      type: object
      properties:
        question:
          type: string
          description: Event resolution question
        payoutValue:
          type: string
          format: int64
          description: Payout value on resolution
        eventDisplayName:
          type: string
          description: Display name for the event
        eventId:
          type: string
          description: Event identifier
        strikeValue:
          type: string
          description: Strike value
        evaluationType:
          type: string
          description: Evaluation comparison operator
        strikeUnit:
          type: string
          description: Unit of the strike value
        calculationMethod:
          type: string
          description: Calculation method for resolution
        timeSpecifier:
          type: string
          description: Time specifier for the event
        positionAccountabilityValue:
          type: string
          format: int64
          description: Position accountability limit value
      description: >-
        EventAttributes contains event-specific resolution and settlement
        details.
    FilterOperator:
      type: string
      enum:
        - FILTER_OPERATOR_UNSPECIFIED
        - FILTER_OPERATOR_EQ
        - FILTER_OPERATOR_LIKE
        - FILTER_OPERATOR_IN
      description: |-
        FilterOperator defines comparison operators for FieldFilter.

        **EQ**: Exact match (=)
        **LIKE**: SQL LIKE pattern (% wildcard)
        **IN**: Match any value in list
    StringList:
      type: object
      properties:
        values:
          type: array
          items:
            type: string
          description: List of string values
      description: StringList for IN operator.

````