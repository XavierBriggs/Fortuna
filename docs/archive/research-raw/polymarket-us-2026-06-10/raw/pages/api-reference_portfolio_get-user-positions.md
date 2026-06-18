> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get User Positions

> Get user's trading positions across all markets or filtered by specific market



## OpenAPI

````yaml /api-reference/oapi-schemas/portfolio-schema.json get /v1/portfolio/positions
openapi: 3.0.1
info:
  title: Portfolio API
  description: >-
    Portfolio and activity endpoints for tracking positions and trading history.
    All endpoints require Ed25519 signature authentication.
  version: v1.0.0
servers:
  - url: https://api.polymarket.us
security:
  - X-PM-Access-Key: []
    X-PM-Timestamp: []
    X-PM-Signature: []
tags:
  - name: Portfolio
  - name: Account
paths:
  /v1/portfolio/positions:
    get:
      tags:
        - Portfolio
      summary: Get User Positions
      description: >-
        Get user's trading positions across all markets or filtered by specific
        market
      operationId: PortfolioService_GetUserPositions
      parameters:
        - name: market
          in: query
          description: >-
            Filter positions by a specific market slug. When provided, returns
            only the position for that market. Example: `will-team-a-win`
          required: false
          schema:
            type: string
        - name: limit
          in: query
          description: >-
            Maximum number of positions to return per page. Use with `cursor`
            for pagination. Default: `100`. Example: `50`
          required: false
          schema:
            type: integer
            format: int32
        - name: cursor
          in: query
          description: >-
            Pagination cursor from a previous response's `nextCursor` field. Use
            to fetch the next page of results. Omit for the first request
          required: false
          schema:
            type: string
      responses:
        '200':
          description: List of user positions
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetUserPositionsResponse'
        '401':
          description: Unauthorized - invalid or missing API key
        '500':
          description: Internal server error
components:
  schemas:
    GetUserPositionsResponse:
      type: object
      properties:
        positions:
          type: object
          additionalProperties:
            $ref: '#/components/schemas/UserPosition'
          description: Map of market slug to position
        nextCursor:
          type: string
          description: Next cursor for pagination
        eof:
          type: boolean
          description: True if this is the last page
        availablePositions:
          type: array
          items:
            type: string
            format: int64
          description: Available position quantities factoring in open orders
          deprecated: true
    UserPosition:
      type: object
      properties:
        netPosition:
          type: string
          format: int64
          description: >-
            Net position quantity rounded to a whole number - deprecated, use
            netPositionDecimal
          deprecated: true
        qtyBought:
          type: string
          format: int64
          description: >-
            Total quantity bought rounded to a whole number - deprecated, use
            qtyBoughtDecimal
          deprecated: true
        qtySold:
          type: string
          format: int64
          description: >-
            Total quantity sold rounded to a whole number - deprecated, use
            qtySoldDecimal
          deprecated: true
        cost:
          $ref: '#/components/schemas/Amount'
          description: Total cost basis
        realized:
          $ref: '#/components/schemas/Amount'
          description: Realized profit/loss
        bodPosition:
          type: string
          format: int64
          description: >-
            Beginning of day position rounded to a whole number - deprecated,
            use bodPositionDecimal
          deprecated: true
        expired:
          type: boolean
          description: Whether the position has expired
        updateTime:
          type: string
          format: date-time
          description: Last update timestamp
        marketMetadata:
          $ref: '#/components/schemas/MarketMetadata'
        cashValue:
          $ref: '#/components/schemas/Amount'
          description: Unrealized PnL for the position
        qtyAvailable:
          type: string
          format: int64
          nullable: true
          description: >-
            Quantity available to trade rounded to a whole number - deprecated,
            use qtyAvailableDecimal
          deprecated: true
        netPositionDecimal:
          type: string
          format: decimal
          description: Net position quantity as a decimal string
        qtyBoughtDecimal:
          type: string
          format: decimal
          description: Total quantity bought as a decimal string
        qtySoldDecimal:
          type: string
          format: decimal
          description: Total quantity sold as a decimal string
        bodPositionDecimal:
          type: string
          format: decimal
          description: Beginning of day position as a decimal string
        qtyAvailableDecimal:
          type: string
          format: decimal
          nullable: true
          description: Quantity available to trade as a decimal string
      description: User's trading position information
    Amount:
      type: object
      required:
        - value
        - currency
      properties:
        value:
          type: string
          format: decimal
          example: '123.45'
          description: The amount as a decimal string
        currency:
          type: string
          description: The currency code
      description: Represents a monetary amount with its currency
    MarketMetadata:
      type: object
      properties:
        slug:
          type: string
          description: Market slug
        icon:
          type: string
          description: Market image URL
        title:
          type: string
          description: Market title
        outcome:
          type: string
          description: Market outcome
        eventSlug:
          type: string
          description: Event slug
  securitySchemes:
    X-PM-Access-Key:
      type: apiKey
      in: header
      name: X-PM-Access-Key
      description: >-
        Your API key ID (UUID). Generate at
        [polymarket.us/developer](https://polymarket.us/developer).
    X-PM-Timestamp:
      type: apiKey
      in: header
      name: X-PM-Timestamp
      description: >-
        Unix timestamp in milliseconds. Must be within 30 seconds of server
        time.
    X-PM-Signature:
      type: apiKey
      in: header
      name: X-PM-Signature
      description: >-
        Base64-encoded Ed25519 signature of `timestamp + method + path`. See
        [Authentication](/api/authentication) for details.

````