> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Create Multiple Orders

> Create up to 20 orders in a single request. If any entry fails request-shape validation, the whole batch is rejected by the gateway before reaching the exchange. Per-entry exchange outcomes (accept, fill, reject) are delivered on the order stream, not in this response. `createdOrderIds` are returned in request order.



## OpenAPI

````yaml /api-reference/oapi-schemas/orders-schema.json post /v1/orders/batched
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
  /v1/orders/batched:
    post:
      tags:
        - Orders
      summary: Create Multiple Orders
      description: >-
        Create up to 20 orders in a single request. If any entry fails
        request-shape validation, the whole batch is rejected by the gateway
        before reaching the exchange. Per-entry exchange outcomes (accept, fill,
        reject) are delivered on the order stream, not in this response.
        `createdOrderIds` are returned in request order.
      operationId: OrdersService_CreateOrderList
      requestBody:
        description: Request to create multiple orders in a single batched request
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateOrderListRequest'
      responses:
        '200':
          description: Orders created successfully
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/CreateOrderListResponse'
        '400':
          description: >-
            Bad request - invalid batched order request or exceeds max batch
            size
        '401':
          description: Unauthorized - invalid or missing authentication token
        '500':
          description: Internal server error
components:
  schemas:
    CreateOrderListRequest:
      type: object
      required:
        - orders
      properties:
        orders:
          type: array
          minItems: 1
          maxItems: 20
          items:
            $ref: '#/components/schemas/CreateOrderRequest'
          description: Orders to create (1 to 20).
      description: Batched create-order request.
    CreateOrderListResponse:
      type: object
      properties:
        createdOrderIds:
          type: array
          items:
            type: string
          description: Exchange-assigned order IDs, in request order.
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