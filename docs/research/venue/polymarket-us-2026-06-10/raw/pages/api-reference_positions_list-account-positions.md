> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# List account positions

> Lists all positions within an account



## OpenAPI

````yaml /institutional/oapi-schemas/positions-schema.json get /v1/positions
openapi: 3.0.1
info:
  title: Positions API
  version: v1.0.0
servers:
  - url: https://api.prod.polymarketexchange.com
security: []
tags:
  - name: PositionAPI
paths:
  /v1/positions:
    get:
      tags:
        - Positions
      summary: List account positions
      description: Lists all positions within an account
      operationId: PositionAPI_ListAccountPositions
      parameters:
        - name: name
          in: query
          required: false
          schema:
            type: string
          description: Fully qualified resource name of the account.
        - name: symbol
          in: query
          required: false
          schema:
            type: string
          description: Symbol to filter (optional).
        - name: as_of_time
          in: query
          required: false
          schema:
            type: string
            format: date-time
          description: >-
            Query positions as of this timestamp (RFC3339 format, e.g.,
            2026-01-02T17:00:00Z). Returns historical positions instead of
            current. Mutually exclusive with as_of_date.
        - name: as_of_date.year
          in: query
          required: false
          schema:
            type: integer
          description: >-
            Year for end-of-trading-day position query (e.g., 2026). Use with
            as_of_date.month and as_of_date.day.
        - name: as_of_date.month
          in: query
          required: false
          schema:
            type: integer
            minimum: 1
            maximum: 12
          description: Month (1-12) for end-of-trading-day position query.
        - name: as_of_date.day
          in: query
          required: false
          schema:
            type: integer
            minimum: 1
            maximum: 31
          description: Day (1-31) for end-of-trading-day position query.
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ListAccountPositionsResponse'
components:
  schemas:
    ListAccountPositionsResponse:
      type: object
      properties:
        positions:
          type: array
          items:
            $ref: '#/components/schemas/Position'
            type: object
        availablePosition:
          type: array
          items:
            type: string
            format: int64
      description: Response with account positions.
    Position:
      type: object
      properties:
        account:
          type: string
        symbol:
          type: string
        netPosition:
          type: string
          format: int64
        qtyBought:
          type: string
          format: int64
        qtySold:
          type: string
          format: int64
        cost:
          type: string
          format: int64
        realized:
          type: string
          format: int64
        bodPosition:
          type: string
          format: int64
        expired:
          type: boolean
        updateTime:
          type: string
          format: date-time

````