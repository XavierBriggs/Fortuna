> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Account Balances

> Get user's current account balances including buying power, asset values, and pending transactions



## OpenAPI

````yaml /api-reference/oapi-schemas/portfolio-schema.json get /v1/account/balances
openapi: 3.0.1
info:
  title: Portfolio API
  description: >-
    Portfolio and activity endpoints for tracking positions and trading history.
    All endpoints require Ed25519 signature authentication.
  version: v1.0.0
servers:
  - url: https://api.polymarket.us
security:
  - X-PM-Access-Key: []
    X-PM-Timestamp: []
    X-PM-Signature: []
tags:
  - name: Portfolio
  - name: Account
paths:
  /v1/account/balances:
    get:
      tags:
        - Account
      summary: Get Account Balances
      description: >-
        Get user's current account balances including buying power, asset
        values, and pending transactions
      operationId: AccountService_GetAccountBalances
      responses:
        '200':
          description: User's current account balances
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetAccountBalancesResponse'
        '401':
          description: Unauthorized - invalid or missing API key
        '500':
          description: Internal server error
components:
  schemas:
    GetAccountBalancesResponse:
      type: object
      properties:
        balances:
          type: array
          items:
            $ref: '#/components/schemas/UserBalance'
          description: List of account balances by currency
    UserBalance:
      type: object
      properties:
        currentBalance:
          type: number
          format: decimal
          description: Current fiat currency balance, not including security values
        currency:
          type: string
          description: Currency for this balance (e.g., USD)
        lastUpdated:
          type: string
          format: date-time
          description: Time of last balance change
        buyingPower:
          type: number
          format: decimal
          description: >-
            Unencumbered capital available for trading, factoring in all
            security valuations and open orders
        assetNotional:
          type: number
          format: decimal
          description: Aggregate notional value of all securities
        assetAvailable:
          type: number
          format: decimal
          description: Aggregate available collateral value of all securities
        pendingCredit:
          type: number
          format: decimal
          description: Aggregate value of all pending credits
        openOrders:
          type: number
          format: decimal
          description: Aggregate total notional value of open orders
        unsettledFunds:
          type: number
          format: decimal
          description: >-
            Aggregate total notional value of unsettled funds not yet available
            to trade
        pendingWithdrawals:
          type: array
          items:
            $ref: '#/components/schemas/PendingWithdrawal'
          description: List of active pending withdrawals
        marginRequirement:
          type: number
          format: decimal
          description: Aggregate margin requirement for event instrument positions
        balanceReservation:
          type: number
          format: decimal
          nullable: true
          description: Aggregate total notional value of balance reservations
      description: User's account balance information
    PendingWithdrawal:
      type: object
      properties:
        id:
          type: string
          description: Unique identifier for the withdrawal
        name:
          type: string
          description: Account name
        balance:
          type: number
          format: decimal
          description: Balance to withdraw
        description:
          type: string
          description: Description for the withdrawal request
        acknowledged:
          type: boolean
          description: Whether the pending withdrawal has been acknowledged
        bankId:
          type: string
          description: Bank ID of the recipient
        creationTime:
          type: string
          format: date-time
          description: Time the pending withdrawal was created
        destinationAccountName:
          type: string
          description: Destination account name for the deposit
      description: Pending withdrawal information
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