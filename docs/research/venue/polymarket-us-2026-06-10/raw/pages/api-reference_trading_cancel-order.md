> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Cancel order

> Requests cancellation of a working order



## OpenAPI

````yaml /institutional/oapi-schemas/trading-schema.json post /v1/trading/orders/cancel
openapi: 3.0.1
info:
  title: Trading API
  version: v1.0.0
servers:
  - url: https://api.prod.polymarketexchange.com
security: []
tags:
  - name: OrderEntryAPI
paths:
  /v1/trading/orders/cancel:
    post:
      tags:
        - Trading
      summary: Cancel order
      description: Requests cancellation of a working order
      operationId: OrderEntryAPI_CancelOrder
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CancelOrderRequest'
        required: true
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/CancelOrderResponse'
components:
  schemas:
    CancelOrderRequest:
      type: object
      properties:
        orderId:
          type: string
        clordId:
          type: string
        symbol:
          type: string
    CancelOrderResponse:
      type: object

````