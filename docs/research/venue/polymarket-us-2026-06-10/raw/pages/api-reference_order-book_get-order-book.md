> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get order book

> Returns the current aggregated order book for a symbol



## OpenAPI

````yaml /institutional/oapi-schemas/orderbook-schema.json get /v1/orderbook/{symbol}
openapi: 3.0.1
info:
  title: Order Book API
  version: v1.0.0
servers:
  - url: https://api.prod.polymarketexchange.com
security: []
tags:
  - name: OrderBookAPI
paths:
  /v1/orderbook/{symbol}:
    get:
      tags:
        - Order Book
      summary: Get order book
      description: Returns the current aggregated order book for a symbol
      operationId: OrderBookAPI_GetOrderBook
      parameters:
        - name: symbol
          in: path
          required: true
          schema:
            type: string
          description: Instrument symbol (e.g., "BTC-USD")
        - name: depth
          in: query
          required: false
          schema:
            type: integer
            format: int32
          description: Number of price levels to return (default 3, max 10)
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetOrderBookResponse'
components:
  schemas:
    GetOrderBookResponse:
      type: object
      properties:
        symbol:
          type: string
          title: Instrument symbol
        bids:
          type: array
          items:
            $ref: '#/components/schemas/BookEntry'
            type: object
          title: Bid side of order book (sorted by price descending)
        offers:
          type: array
          items:
            $ref: '#/components/schemas/BookEntry'
            type: object
          title: Offer/ask side of order book (sorted by price ascending)
        state:
          $ref: '#/components/schemas/InstrumentState'
          title: Current trading state of the instrument (optional)
        stats:
          $ref: '#/components/schemas/InstrumentStats'
          title: Market statistics (last trade, open/high/low/close, etc.)
        transactTime:
          type: string
          format: date-time
          title: Server timestamp of the data
      description: Response containing the order book snapshot.
    BookEntry:
      type: object
      properties:
        px:
          type: string
          format: int64
        qty:
          type: string
          format: int64
      x-deprecated-removed: symbolSubType field was intentionally removed - do not add it back
      description: BookEntry lists a price and volume for a market data record.
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
      description: InstrumentState represents the state of an instrument.
    InstrumentStats:
      type: object
      properties:
        openPx:
          type: string
          format: int64
          x-nullable: true
        closePx:
          type: string
          format: int64
          x-nullable: true
        lowPx:
          type: string
          format: int64
          x-nullable: true
        highPx:
          type: string
          format: int64
          x-nullable: true
        lastTradePx:
          type: string
          format: int64
          x-nullable: true
          description: Last trade price.
        lastTradeQty:
          type: string
          format: int64
          x-nullable: true
          description: >-
            Quantity of the most recent trade. Populated after any trade
            executes on the instrument.
        indicativeOpenPx:
          type: string
          format: int64
          x-nullable: true
        settlementPx:
          type: string
          format: int64
          x-nullable: true
        settlementSetTime:
          type: string
          format: date-time
          x-nullable: true
          description: >-
            Timestamp when the settlement price was set. Only populated when the
            instrument is in a settled state: CLOSED, TERMINATED, or EXPIRED.
        settlementPreliminary:
          type: boolean
          x-nullable: true
        settlementPriceCalculationMethod:
          type: string
          x-nullable: true
        settlementPriceCalculationText:
          type: string
          x-nullable: true
        sharesTraded:
          type: string
          format: int64
          x-nullable: true
        notionalTraded:
          type: string
          format: int64
          x-nullable: true
        openInterest:
          type: string
          format: int64
          x-nullable: true
      description: InstrumentStats contains statistics about an instrument.

````