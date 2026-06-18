> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Cancel Order

> Cancel a specific order by its exchange-assigned ID



## OpenAPI

````yaml /api-reference/oapi-schemas/orders-schema.json post /v1/order/{orderId}/cancel
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
  /v1/order/{orderId}/cancel:
    post:
      tags:
        - Orders
      summary: Cancel Order
      description: Cancel a specific order by its exchange-assigned ID
      operationId: OrdersService_CancelOrder
      parameters:
        - name: orderId
          in: path
          description: Exchange-assigned Order ID to cancel
          required: true
          schema:
            type: string
      requestBody:
        description: Request to cancel an existing order
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CancelOrderRequest'
      responses:
        '200':
          description: Order canceled successfully
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/CancelOrderResponse'
        '400':
          description: Bad request - invalid order modification request
        '401':
          description: Unauthorized - invalid or missing authentication token
        '500':
          description: Internal server error
components:
  schemas:
    CancelOrderRequest:
      type: object
      properties:
        marketSlug:
          type: string
          description: Unique market slug into which the order should be canceled
      description: Request to cancel an existing order
    CancelOrderResponse:
      type: object
      description: Response for order cancellation (empty on success)
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