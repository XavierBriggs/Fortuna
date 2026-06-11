> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Market Book

> Retrieve current market data (order book, stats) for a specific market by its slug



## OpenAPI

````yaml /api-reference/oapi-schemas/markets-schema.json get /v1/markets/{slug}/book
openapi: 3.0.3
info:
  title: protos/gateway/market/v1/market.proto
  version: 1.0.0
servers:
  - url: https://gateway.polymarket.us
    description: Production server
security: []
tags:
  - name: MarketService
paths:
  /v1/markets/{slug}/book:
    get:
      tags:
        - Markets
      summary: Get Market Book
      description: >-
        Retrieve current market data (order book, stats) for a specific market
        by its slug
      operationId: MarketService_GetMarketBook
      parameters:
        - name: slug
          description: Market slug
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Market data including order book and stats
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/v1GetMarketBookResponse'
        '404':
          description: Market not found
          content:
            application/json:
              schema: {}
        '500':
          description: Internal server error
          content:
            application/json:
              schema: {}
components:
  schemas:
    v1GetMarketBookResponse:
      type: object
      properties:
        marketData:
          $ref: '#/components/schemas/v1MarketData'
      description: Response containing market book data including order book and stats
    v1MarketData:
      type: object
      properties:
        marketSlug:
          type: string
        bids:
          type: array
          items:
            $ref: '#/components/schemas/v1BookEntry'
        offers:
          type: array
          items:
            $ref: '#/components/schemas/v1BookEntry'
        state:
          $ref: '#/components/schemas/v1MarketState'
        stats:
          $ref: '#/components/schemas/v1MarketStats'
        transactTime:
          type: string
          format: date-time
    v1BookEntry:
      type: object
      properties:
        px:
          $ref: '#/components/schemas/v1Amount'
        qty:
          type: string
    v1MarketState:
      type: string
      enum:
        - MARKET_STATE_OPEN
        - MARKET_STATE_PREOPEN
        - MARKET_STATE_SUSPENDED
        - MARKET_STATE_EXPIRED
        - MARKET_STATE_TERMINATED
        - MARKET_STATE_HALTED
        - MARKET_STATE_MATCH_AND_CLOSE_AUCTION
    v1MarketStats:
      type: object
      properties:
        openPx:
          $ref: '#/components/schemas/v1Amount'
        closePx:
          $ref: '#/components/schemas/v1Amount'
        lowPx:
          $ref: '#/components/schemas/v1Amount'
        highPx:
          $ref: '#/components/schemas/v1Amount'
        lastTradePx:
          $ref: '#/components/schemas/v1Amount'
        indicativeOpenPx:
          $ref: '#/components/schemas/v1Amount'
        settlementPx:
          $ref: '#/components/schemas/v1Amount'
        sharesTraded:
          type: string
        notionalTraded:
          $ref: '#/components/schemas/v1Amount'
        lastTradeQty:
          type: string
        openInterest:
          type: string
        settlementPreliminaryFlag:
          type: boolean
        openSetTime:
          type: string
          format: date-time
        closeSetTime:
          type: string
          format: date-time
        highSetTime:
          type: string
          format: date-time
        lowSetTime:
          type: string
          format: date-time
        lastTradeSetTime:
          type: string
          format: date-time
        indicativeOpenSetTime:
          type: string
          format: date-time
        settlementSetTime:
          type: string
          format: date-time
        openInterestSetTime:
          type: string
          format: date-time
        notionalSetTime:
          type: string
          format: date-time
        settlementPriceCalculationMethod:
          $ref: '#/components/schemas/v1SettlementPriceCalculationMethod'
        settlementPriceCalculationText:
          type: string
          description: Free-form text describing the outcome that determined the settlement
        tradingReferencePx:
          $ref: '#/components/schemas/v1Amount'
        tradingReferenceSetTime:
          type: string
          format: date-time
          description: Time at which the trading reference price was set.
        currentPx:
          $ref: '#/components/schemas/v1Amount'
        lastPriceSample:
          $ref: '#/components/schemas/v1PriceSample'
      description: MarketStats are a collection of stats on an instrument.
    v1Amount:
      type: object
      properties:
        value:
          type: string
          format: decimal
          example: 123.45
          description: The amount as a decimal string.
        currency:
          type: string
          description: The currency code
      description: Represents a monetary amount with its currency.
      required:
        - value
        - currency
    v1SettlementPriceCalculationMethod:
      type: string
      enum:
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_1
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_1
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_2
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_3
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_4
        - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_1
        - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_2
        - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_3
        - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_4
        - SETTLEMENT_PRICE_CALCULATION_METHOD_OVERRIDE
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_5
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_6
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_7
        - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_8
      description: >-
        SettlementPriceCalculationMethod indicates the settlement price
        calculation method of the book.

         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_1: If using the VWAP Settlement Price Logic, indicates that there were enough trades to calculate the settlement price using Volume Weighted Average Price
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_1: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Best Bid / Best Ask / Last Trade existing.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_2: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Best Bid / Best Ask existing and Last Trade not existing.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_3: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Best Bid / Last Trade existing and Best Ask not existing.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_4: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Best Bid existing and Best Ask / Last Trade not existing.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_1: Indicates that the settlement price was set as a result of "PerformResolution".  In addition, settlement prices that have this calculation method are considered final and will no longer be updated by the Settlement Price Module.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_2: If using the EVENT Settlement Price Logic, indicates that the settlement price was set to the price of the trading day's latest trade for the Opposing Side = "As Defined" sub type
         - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_3: If using the EVENT Settlement Price Logic, indicates that at the market close no trades for the Opposing Side = "As Defined" sub type during the trade day, and therefore the settlement price was set to the current settlement price
         - SETTLEMENT_PRICE_CALCULATION_METHOD_EVENT_TIER_4: If using the EVENT Settlement Price Logic, indicates that there were no trades for the Opposing Side = "As Defined" sub type during the trade day, nor current settlement price, and therefore the settlement price was set to 0
         - SETTLEMENT_PRICE_CALCULATION_METHOD_OVERRIDE: Indicates the calculated settlement price was overridden by the exchange
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_5: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Best Ask / Last Trade existing and Best Bid not existing.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_6: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Best Ask existing and Best Bid / Last Trade not existing.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_7: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Last Trade existing and Best Bid / Best Ask not existing.
         - SETTLEMENT_PRICE_CALCULATION_METHOD_VWAP_TIER_2_RULE_8: If using the VWAP Settlement Price Logic, indicates that there weren't enough trades to calculate the settlement price using VWAP, and the Best Bid / Best Ask / Last Trade / Previous Settlement logic was used instead, with Best Bid / Best Ask / Last Trade not existing.
    v1PriceSample:
      type: object
      properties:
        longPx:
          $ref: '#/components/schemas/v1Amount'
        shortPx:
          $ref: '#/components/schemas/v1Amount'
        ts:
          type: string
          format: date-time

````