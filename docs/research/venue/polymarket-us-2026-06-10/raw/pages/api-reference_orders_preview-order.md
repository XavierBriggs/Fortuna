> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Preview Order

> Preview an order before submission to validate parameters and see expected fills



## OpenAPI

````yaml /api-reference/oapi-schemas/orders-schema.json post /v1/order/preview
openapi: 3.0.1
info:
  title: Orders API
  description: >-
    Order management endpoints for trading. All endpoints require Ed25519
    signature authentication.
  version: v1.0.0
servers:
  - url: https://api.polymarket.us
security:
  - X-PM-Access-Key: []
    X-PM-Timestamp: []
    X-PM-Signature: []
tags:
  - name: Orders
paths:
  /v1/order/preview:
    post:
      tags:
        - Orders
      summary: Preview Order
      description: >-
        Preview an order before submission to validate parameters and see
        expected fills
      operationId: OrdersService_PreviewOrder
      requestBody:
        description: Request to preview an order
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/PreviewOrderRequest'
      responses:
        '200':
          description: Order previewed successfully
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/PreviewOrderResponse'
        '400':
          description: Bad request - invalid order preview request
        '401':
          description: Unauthorized - invalid or missing authentication token
        '500':
          description: Internal server error
components:
  schemas:
    PreviewOrderRequest:
      type: object
      properties:
        request:
          $ref: '#/components/schemas/CreateOrderRequest'
          description: The order to preview
      description: Request to preview an order
    PreviewOrderResponse:
      type: object
      properties:
        order:
          $ref: '#/components/schemas/Order'
          description: Previewed order with calculated values
      description: Response containing previewed order
    CreateOrderRequest:
      type: object
      required:
        - marketSlug
      properties:
        marketSlug:
          type: string
          description: Unique market slug into which the order should be entered
        type:
          $ref: '#/components/schemas/OrderType'
          description: Type of order (limit or market)
        price:
          $ref: '#/components/schemas/Amount'
          description: Order price and currency. Required for limit orders.
        quantity:
          type: number
          format: double
          description: >-
            Order quantity in contracts. Supports decimal quantities on markets
            whose minimumTradeQty is less than 1.
        tif:
          $ref: '#/components/schemas/TimeInForce'
          description: Order time in force policy
        participateDontInitiate:
          type: boolean
          description: >-
            If true, order must rest on the book prior to matching (maker only).
            Order will be rejected if it would immediately match.
        goodTillTime:
          type: string
          description: Expiration time for orders with TIME_IN_FORCE_GOOD_TILL_DATE
        intent:
          $ref: '#/components/schemas/OrderIntent'
          description: >-
            Order intent. Either `intent` or (`outcomeSide` + `action`) must be
            set.
        outcomeSide:
          $ref: '#/components/schemas/OutcomeSide'
          description: >-
            Outcome side. Set with `action` as an alternative to `intent`. Takes
            priority if both are set.
        action:
          $ref: '#/components/schemas/OrderAction'
          description: Order action. Set with `outcomeSide` as an alternative to `intent`.
        cashOrderQty:
          $ref: '#/components/schemas/Amount'
          description: >-
            Order quantity in cash and currency. Used for market orders instead
            of share quantity.
        manualOrderIndicator:
          $ref: '#/components/schemas/ManualOrderIndicator'
          description: Indicates whether the order was placed manually or automatically
        synchronousExecution:
          type: boolean
          description: >-
            If true, will block until the order is filled, rejected, canceled,
            or expired, up to maxBlockTime seconds
        maxBlockTime:
          type: string
          format: int64
          description: Maximum block time in seconds if synchronous execution is requested
        slippageTolerance:
          $ref: '#/components/schemas/SlippageTolerance'
          description: Slippage tolerance configuration for the order
      description: Request to create a new order.
    Order:
      type: object
      properties:
        id:
          type: string
          description: Exchange-assigned order ID
        marketSlug:
          type: string
          description: Market slug for this order
        side:
          $ref: '#/components/schemas/OrderSide'
          description: Side of the order (buy or sell)
        type:
          $ref: '#/components/schemas/OrderType'
          description: Type of order (limit or market)
        price:
          $ref: '#/components/schemas/Amount'
          description: Order price and currency
        quantity:
          type: number
          format: double
          description: Original order quantity in contracts
        cumQuantity:
          type: number
          format: double
          description: Cumulative filled quantity in contracts
        leavesQuantity:
          type: number
          format: double
          description: Remaining unfilled quantity in contracts
        tif:
          $ref: '#/components/schemas/TimeInForce'
          description: Order time in force policy
        goodTillTime:
          type: string
          format: date-time
          description: Expiration time for GTD orders
        intent:
          $ref: '#/components/schemas/OrderIntent'
          description: Order intent (buy/sell YES or NO)
        marketMetadata:
          $ref: '#/components/schemas/MarketMetadata'
          description: Market metadata
        state:
          $ref: '#/components/schemas/OrderState'
          description: Current order state
        commissionNotionalTotalCollected:
          $ref: '#/components/schemas/Amount'
          description: Total notional value of all commissions collected on this order
        commissionsBasisPoints:
          type: string
          description: Commission rate in basis points
        makerCommissionsBasisPoints:
          type: string
          description: Maker commission rate in basis points
        avgPx:
          $ref: '#/components/schemas/Amount'
          description: Average fill price
        cashOrderQty:
          $ref: '#/components/schemas/Amount'
          description: Total cash order quantity for market orders
        insertTime:
          type: string
          format: date-time
          description: Time order was inserted into the book or replaced
        createTime:
          type: string
          format: date-time
          description: Time order was created
        outcomeSide:
          $ref: '#/components/schemas/OutcomeSide'
          description: Outcome side.
        action:
          $ref: '#/components/schemas/OrderAction'
          description: Order action.
      description: Order information
    OrderType:
      type: string
      enum:
        - ORDER_TYPE_LIMIT
        - ORDER_TYPE_MARKET
      description: >-
        Type of order. `ORDER_TYPE_LIMIT` requires a price, `ORDER_TYPE_MARKET`
        executes at best available price.
    Amount:
      type: object
      required:
        - value
        - currency
      properties:
        value:
          type: string
          format: decimal
          example: '0.55'
          description: The amount as a decimal string
        currency:
          type: string
          example: USD
          description: The currency code (e.g., 'USD')
      description: Represents a monetary amount with its currency
    TimeInForce:
      type: string
      enum:
        - TIME_IN_FORCE_DAY
        - TIME_IN_FORCE_GOOD_TILL_CANCEL
        - TIME_IN_FORCE_GOOD_TILL_DATE
        - TIME_IN_FORCE_IMMEDIATE_OR_CANCEL
        - TIME_IN_FORCE_FILL_OR_KILL
      description: >-
        Time in force policy. `TIME_IN_FORCE_DAY` expires at end of trading day.
        `TIME_IN_FORCE_GOOD_TILL_CANCEL` (GTC) remains active until filled or
        canceled. `TIME_IN_FORCE_GOOD_TILL_DATE` (GTD) expires at specified
        time. `TIME_IN_FORCE_IMMEDIATE_OR_CANCEL` (IOC) fills immediately or
        cancels remaining. `TIME_IN_FORCE_FILL_OR_KILL` (FOK) must fill entirely
        or cancel.
    OrderIntent:
      type: string
      enum:
        - ORDER_INTENT_BUY_LONG
        - ORDER_INTENT_SELL_LONG
        - ORDER_INTENT_BUY_SHORT
        - ORDER_INTENT_SELL_SHORT
      description: >-
        Intent of the order. `ORDER_INTENT_BUY_LONG` = Buy YES contracts,
        `ORDER_INTENT_SELL_LONG` = Sell YES contracts, `ORDER_INTENT_BUY_SHORT`
        = Buy NO contracts, `ORDER_INTENT_SELL_SHORT` = Sell NO contracts.
    OutcomeSide:
      type: string
      enum:
        - OUTCOME_SIDE_YES
        - OUTCOME_SIDE_NO
      description: Outcome side of the market (YES = long, NO = short).
    OrderAction:
      type: string
      enum:
        - ORDER_ACTION_BUY
        - ORDER_ACTION_SELL
      description: Order action (BUY or SELL).
    ManualOrderIndicator:
      type: string
      enum:
        - MANUAL_ORDER_INDICATOR_MANUAL
        - MANUAL_ORDER_INDICATOR_AUTOMATIC
      description: >-
        Indicates whether the order was placed manually by a user or
        automatically by a trading system
    SlippageTolerance:
      type: object
      properties:
        currentPrice:
          $ref: '#/components/schemas/Amount'
          description: >-
            Current price of the market used as reference for slippage
            calculation
        bips:
          type: integer
          format: int32
          nullable: true
          description: Slippage tolerance in basis points (1 bip = 0.01%)
        ticks:
          type: integer
          format: int32
          nullable: true
          description: >-
            Slippage tolerance in price ticks. Takes priority over bips if both
            are set.
      description: Slippage tolerance configuration for market orders
    OrderSide:
      type: string
      enum:
        - ORDER_SIDE_BUY
        - ORDER_SIDE_SELL
      description: Side of the order (buy or sell)
    MarketMetadata:
      type: object
      properties:
        slug:
          type: string
          description: Market slug identifier
        icon:
          type: string
          description: Market image URL
        title:
          type: string
          description: Market title/name
        outcome:
          type: string
          description: Market outcome description
        eventSlug:
          type: string
          description: Parent event slug
        teamId:
          type: integer
          format: int32
          nullable: true
          description: Team ID (deprecated, use team object instead)
          deprecated: true
        team:
          $ref: '#/components/schemas/Team'
          nullable: true
          description: Team information with full details including ordering
      description: Metadata about the market associated with an order
    OrderState:
      type: string
      enum:
        - ORDER_STATE_NEW
        - ORDER_STATE_PARTIALLY_FILLED
        - ORDER_STATE_FILLED
        - ORDER_STATE_CANCELED
        - ORDER_STATE_REPLACED
        - ORDER_STATE_REJECTED
        - ORDER_STATE_EXPIRED
        - ORDER_STATE_PENDING_NEW
        - ORDER_STATE_PENDING_REPLACE
        - ORDER_STATE_PENDING_CANCEL
        - ORDER_STATE_PENDING_RISK
      description: >-
        Current state of the order. `ORDER_STATE_NEW` = Order accepted by the
        matching engine and resting on the book. `ORDER_STATE_PENDING_NEW` =
        Order received but not yet processed by matching engine.
        `ORDER_STATE_PENDING_REPLACE` = Modify request received but not yet
        processed. `ORDER_STATE_PENDING_CANCEL` = Cancel request received but
        not yet processed. `ORDER_STATE_PENDING_RISK` = Order is pending risk
        approval. `ORDER_STATE_REPLACED` = Order was modified via
        cancel-replace.
    Team:
      type: object
      properties:
        id:
          type: integer
          format: int32
          description: Team ID
        name:
          type: string
          description: Team name
        abbreviation:
          type: string
          description: Team abbreviation
        league:
          type: string
          description: League the team belongs to
        record:
          type: string
          description: Team record (wins-losses)
        logo:
          type: string
          description: Team logo URL
        alias:
          type: string
          description: Team alias
        safeName:
          type: string
          description: URL-safe team name
        homeIcon:
          type: string
          nullable: true
          description: Home game icon URL
        awayIcon:
          type: string
          nullable: true
          description: Away game icon URL
        colorPrimary:
          type: string
          nullable: true
          description: Primary team color (hex)
        ordering:
          type: string
          nullable: true
          description: Display ordering
        ranking:
          type: string
          format: int64
          nullable: true
          description: Team ranking
        conference:
          type: string
          nullable: true
          description: Conference the team belongs to
      description: Team information for sports markets
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