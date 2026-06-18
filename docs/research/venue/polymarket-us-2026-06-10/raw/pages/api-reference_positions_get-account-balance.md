> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get account balance

> Gets the balance for a currency in an account



## OpenAPI

````yaml /institutional/oapi-schemas/positions-schema.json post /v1/positions/balance
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
  /v1/positions/balance:
    post:
      tags:
        - Positions
      summary: Get account balance
      description: Gets the balance for a currency in an account
      operationId: PositionAPI_GetAccountBalance
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/GetAccountBalanceRequest'
        required: true
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetAccountBalanceResponse'
components:
  schemas:
    GetAccountBalanceRequest:
      type: object
      properties:
        name:
          type: string
          description: Fully qualified resource name of the account.
        currency:
          type: string
          description: ISO currency code (e.g., "USD").
      description: Request to get the balance of an account in a currency.
    GetAccountBalanceResponse:
      type: object
      properties:
        balance:
          type: string
        capitalRequirement:
          type: string
        excessCapital:
          type: string
        buyingPower:
          type: string
        securities:
          type: object
          additionalProperties:
            $ref: '#/components/schemas/SecurityEntry'
        totalSecurityNotionalValue:
          type: string
        totalSecurityAvailableValue:
          type: string
        openOrders:
          type: string
        alternateCapitalRequirementCurrency:
          type: string
        alternateCapitalRequirement:
          type: string
        unsettledFunds:
          type: string
        marginRequirement:
          type: string
        updateTime:
          type: string
          format: date-time
      description: Response with account balance details.
    SecurityEntry:
      type: object
      properties:
        balance:
          type: string
        marketValue:
          type: string
        haircut:
          type: string
          format: int64
        notionalValue:
          type: string
        availableValue:
          type: string
      description: SecurityEntry describes the valuation of a security.

````