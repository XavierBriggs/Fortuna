> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Modify Order

> Modify an existing order in the marketplace. Allows changing price, quantity, time in force, and other parameters.



## OpenAPI

````yaml /api-reference/oapi-schemas/orders-schema.json post /v1/order/{orderId}/modify
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
  /v1/order/{orderId}/modify:
    post:
      tags:
        - Orders
      summary: Modify Order
      description: >-
        Modify an existing order in the marketplace. Allows changing price,
        quantity, time in force, and other parameters.
      operationId: OrdersService_ModifyOrder
      parameters:
        - name: orderId
          in: path
          description: Exchange-assigned Order ID to modify
          required: true
          schema:
            type: string
      requestBody:
        description: Request to modify an existing order
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/ModifyOrderRequest'
      responses:
        '200':
          description: Order modified successfully
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ModifyOrderResponse'
        '400':
          description: Bad request - invalid order modification request
        '401':
          description: Unauthorized - invalid or missing authentication token
        '500':
          description: Internal server error
components:
  schemas:
    ModifyOrderRequest:
      type: object
      properties:
        marketSlug:
          type: string
          description: Unique market slug for the order being modified
        price:
          $ref: '#/components/schemas/Amount'
          description: New order price and currency
        quantity:
          type: number
          format: double
          description: >-
            Order quantity in contracts. Supports decimal quantities on markets
            whose minimumTradeQty is less than 1.
        tif:
          $ref: '#/components/schemas/TimeInForce'
          description: New order time in force
        participateDontInitiate:
          type: boolean
          description: Order must rest on the book prior to matching (maker only)
        goodTillTime:
          type: string
          description: Good till time for orders with TIME_IN_FORCE_GOOD_TILL_DATE
      description: Request to modify an existing order
    ModifyOrderResponse:
      type: object
      description: Response for order modification (empty on success)
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