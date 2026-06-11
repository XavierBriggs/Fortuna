> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Market Settlement

> Retrieve the settlement price for a specific market by its slug



## OpenAPI

````yaml /api-reference/oapi-schemas/markets-schema.json get /v1/markets/{slug}/settlement
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
  /v1/markets/{slug}/settlement:
    get:
      tags:
        - Markets
      summary: Get Market Settlement
      description: Retrieve the settlement price for a specific market by its slug
      operationId: MarketService_GetMarketSettlement
      parameters:
        - name: slug
          description: Market slug
          in: path
          required: true
          schema:
            type: string
        - name: fromEp3
          description: Whether to get settlement from EP3
          in: query
          required: false
          schema:
            type: boolean
      responses:
        '200':
          description: Market settlement details
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/v1GetMarketSettlementResponse'
        '404':
          description: Market not found or not settled
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
    v1GetMarketSettlementResponse:
      type: object
      properties:
        slug:
          type: string
          description: Market slug
        settlement:
          type: number
          format: decimal
          description: Settlement price
      description: Response containing market settlement details

````