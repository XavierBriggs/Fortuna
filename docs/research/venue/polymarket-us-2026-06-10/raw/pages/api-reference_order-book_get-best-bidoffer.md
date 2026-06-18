> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get best bid/offer

> Returns the top of book (best bid and offer) for a symbol



## OpenAPI

````yaml /institutional/oapi-schemas/orderbook-schema.json get /v1/orderbook/{symbol}/bbo
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
  /v1/orderbook/{symbol}/bbo:
    get:
      tags:
        - Order Book
      summary: Get best bid/offer
      description: Returns the top of book (best bid and offer) for a symbol
      operationId: OrderBookAPI_GetBBO
      parameters:
        - name: symbol
          in: path
          required: true
          schema:
            type: string
          description: Instrument symbol (e.g., "BTC-USD")
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetBBOResponse'
components:
  schemas:
    GetBBOResponse:
      type: object
      properties:
        symbol:
          type: string
          title: Instrument symbol
        bestBid:
          $ref: '#/components/schemas/BookEntry'
          title: Best bid (highest buy price)
        bestOffer:
          $ref: '#/components/schemas/BookEntry'
          title: Best offer (lowest sell price)
        spread:
          type: string
          format: int64
          title: Spread in price units (best_offer.px - best_bid.px)
        midPrice:
          type: string
          format: int64
          title: Mid price ((best_bid.px + best_offer.px) / 2)
        state:
          $ref: '#/components/schemas/InstrumentState'
          title: Current trading state of the instrument (optional)
        transactTime:
          type: string
          format: date-time
          title: Server timestamp of the data
      description: Response containing the best bid and offer.
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

````