> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# List symbols

> Returns a list of symbols on the exchange



## OpenAPI

````yaml /institutional/oapi-schemas/refdata-schema.json post /v1/refdata/symbols
openapi: 3.0.1
info:
  title: Refdata API
  version: v1.0.0
servers:
  - url: https://api.prod.polymarketexchange.com
security: []
tags:
  - name: RefDataAPI
paths:
  /v1/refdata/symbols:
    post:
      tags:
        - ReferenceData
      summary: List symbols
      description: Returns a list of symbols on the exchange
      operationId: RefDataAPI_ListSymbols
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/ListSymbolsRequest'
        required: true
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ListSymbolsResponse'
components:
  schemas:
    ListSymbolsRequest:
      type: object
      description: Request for listing symbols.
    ListSymbolsResponse:
      type: object
      properties:
        symbols:
          type: array
          items:
            type: string
      description: Response with list of symbols.

````