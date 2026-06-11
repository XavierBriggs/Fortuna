> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Cancel Multiple Orders

> Cancel up to 20 orders in a single request. If any entry fails request-shape validation, the whole batch is rejected by the gateway before reaching the exchange. The exchange may silently ignore unknown `orderId`s. `canceledOrderIds` is an echo of the request, not a confirmation; observe actual outcomes on the order stream.



## OpenAPI

````yaml /api-reference/oapi-schemas/orders-schema.json post /v1/orders/batched/cancel
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
  /v1/orders/batched/cancel:
    post:
      tags:
        - Orders
      summary: Cancel Multiple Orders
      description: >-
        Cancel up to 20 orders in a single request. If any entry fails
        request-shape validation, the whole batch is rejected by the gateway
        before reaching the exchange. The exchange may silently ignore unknown
        `orderId`s. `canceledOrderIds` is an echo of the request, not a
        confirmation; observe actual outcomes on the order stream.
      operationId: OrdersService_CancelOrderList
      requestBody:
        description: Request to cancel multiple orders in a single batched request
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CancelOrderListRequest'
      responses:
        '200':
          description: All orders successfully submitted for cancellation
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/CancelOrderListResponse'
        '400':
          description: >-
            Bad request - invalid batched cancel request or exceeds max batch
            size
        '401':
          description: Unauthorized - invalid or missing authentication token
        '500':
          description: Internal server error
components:
  schemas:
    CancelOrderListRequest:
      type: object
      required:
        - orders
      properties:
        orders:
          type: array
          minItems: 1
          maxItems: 20
          items:
            $ref: '#/components/schemas/CancelOrderRequestEntry'
          description: Orders to cancel (1 to 20).
      description: Batched cancel-order request.
    CancelOrderListResponse:
      type: object
      properties:
        canceledOrderIds:
          type: array
          items:
            type: string
          description: >-
            Order IDs submitted for cancellation, in request order. Echo of the
            input, not a confirmation. Watch the order stream for actual
            outcomes.
    CancelOrderRequestEntry:
      type: object
      required:
        - orderId
        - marketSlug
      properties:
        orderId:
          type: string
          description: Exchange-assigned order ID to cancel.
        marketSlug:
          type: string
          description: Market slug the order belongs to.
      description: >-
        Same shape as the body of `POST /v1/order/{orderId}/cancel`, with
        `orderId` in the body instead of the URL path.
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