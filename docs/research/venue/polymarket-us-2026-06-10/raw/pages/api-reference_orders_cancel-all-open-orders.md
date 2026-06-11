> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Cancel All Open Orders

> Cancel all open orders, optionally filtered by market slugs



## OpenAPI

````yaml /api-reference/oapi-schemas/orders-schema.json post /v1/orders/open/cancel
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
  /v1/orders/open/cancel:
    post:
      tags:
        - Orders
      summary: Cancel All Open Orders
      description: Cancel all open orders, optionally filtered by market slugs
      operationId: OrdersService_CancelOpenOrders
      requestBody:
        description: Request to cancel all open orders
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CancelOpenOrdersRequest'
      responses:
        '200':
          description: Open orders canceled successfully
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/CancelOpenOrdersResponse'
        '400':
          description: Bad request - invalid cancel open orders request
        '401':
          description: Unauthorized - invalid or missing authentication token
        '500':
          description: Internal server error
components:
  schemas:
    CancelOpenOrdersRequest:
      type: object
      properties:
        slugs:
          type: array
          items:
            type: string
          description: >-
            List of market slugs to filter by. If empty, cancels all open
            orders.
      description: Request to cancel all open orders
    CancelOpenOrdersResponse:
      type: object
      properties:
        canceledOrderIds:
          type: array
          items:
            type: string
          description: List of canceled order IDs
      description: Response containing canceled order IDs
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