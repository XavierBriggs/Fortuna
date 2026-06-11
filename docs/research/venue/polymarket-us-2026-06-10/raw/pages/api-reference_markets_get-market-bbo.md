> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Market BBO

> Retrieve current market data (best bid/offer, stats) for a specific market by its slug in a lightweight format



## OpenAPI

````yaml /api-reference/oapi-schemas/markets-schema.json get /v1/markets/{slug}/bbo
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
  /v1/markets/{slug}/bbo:
    get:
      tags:
        - Markets
      summary: Get Market BBO
      description: >-
        Retrieve current market data (best bid/offer, stats) for a specific
        market by its slug in a lightweight format
      operationId: MarketService_GetMarketBBO
      parameters:
        - name: slug
          description: Market slug
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Market data lite including best bid/offer and stats
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/v1GetMarketBBOResponse'
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
    v1GetMarketBBOResponse:
      type: object
      properties:
        marketData:
          $ref: '#/components/schemas/v1MarketDataLite'
      description: Response containing market BBO data in a lightweight format
    v1MarketDataLite:
      type: object
      properties:
        marketSlug:
          type: string
        currentPx:
          $ref: '#/components/schemas/v1Amount'
        lastTradePx:
          $ref: '#/components/schemas/v1Amount'
        settlementPx:
          $ref: '#/components/schemas/v1Amount'
        sharesTraded:
          type: string
        openInterest:
          type: string
        bestAsk:
          $ref: '#/components/schemas/v1Amount'
        bestBid:
          $ref: '#/components/schemas/v1Amount'
        askDepth:
          type: integer
          format: int32
        bidDepth:
          type: integer
          format: int32
        lastPriceSample:
          $ref: '#/components/schemas/v1PriceSample'
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