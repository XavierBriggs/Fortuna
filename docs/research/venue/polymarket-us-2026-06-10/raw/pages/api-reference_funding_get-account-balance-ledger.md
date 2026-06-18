> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get account balance ledger

> Returns historical balance changes for an account.



## OpenAPI

````yaml /institutional/oapi-schemas/funding-schema.json get /v1/funding/balance-ledger
openapi: 3.0.1
info:
  title: Funding API
  description: >-
    Account balance ledger endpoints. Tracks every change to an account's cash
    balance with both a delta (`before_balance` -> `after_balance`) and a typed
    `entry_type`.
  version: v1.0.0
servers:
  - url: https://api.prod.polymarketexchange.com
security: []
tags:
  - name: Funding
paths:
  /v1/funding/balance-ledger:
    get:
      tags:
        - Funding
      summary: Get account balance ledger
      description: Returns historical balance changes for an account.
      operationId: FundingAPI_GetAccountBalanceLedger
      parameters:
        - name: account
          in: query
          required: true
          schema:
            type: string
          description: >-
            Required. Fully qualified resource name of the account. Example:
            `firms/ISV-Alice/accounts/alice-trading`.
        - name: currency
          in: query
          required: false
          schema:
            type: string
          description: >-
            Optional. ISO currency code (e.g., `USD`). Omit to return entries in
            all currencies for the account.
        - name: start_time
          in: query
          required: false
          schema:
            type: string
            format: date-time
          description: >-
            Optional. Inclusive lower bound on `update_time` (RFC3339). Clamped
            upstream to `2026-05-01T00:00:00Z`.
        - name: end_time
          in: query
          required: false
          schema:
            type: string
            format: date-time
          description: Optional. Inclusive upper bound on `update_time` (RFC3339).
        - name: entry_types
          in: query
          required: false
          schema:
            type: array
            items:
              $ref: '#/components/schemas/LedgerEntryType'
          description: >-
            Optional. Filter by one or more entry types from the allowlist.
            Requesting a suppressed type returns `Aborted` (HTTP 409). Multiple
            values can be provided. Example:
            `entry_types=DEPOSIT&entry_types=ORDER_EXECUTION`.
        - name: symbol
          in: query
          required: false
          schema:
            type: string
          description: >-
            Optional. Filter by instrument symbol (e.g., for `ORDER_EXECUTION`
            entries).
        - name: description
          in: query
          required: false
          schema:
            type: string
            maxLength: 200
          description: >-
            Optional. Substring filter on the entry `description`. Maximum 200
            Unicode characters (not bytes); longer values return
            `InvalidArgument`.
        - name: newest_first
          in: query
          required: false
          schema:
            type: boolean
          description: >-
            Optional. If `true`, return entries in descending `update_time`
            order. Default is `false` (oldest first).
        - name: page_size
          in: query
          required: false
          schema:
            type: integer
            format: int32
            maximum: 1000
          description: >-
            Optional. Maximum entries to return per page (max 1000). Values
            above 1000 return `InvalidArgument`.
        - name: page_token
          in: query
          required: false
          schema:
            type: string
          description: >-
            Optional. Pagination token from a previous response's
            `nextPageToken`. Omit for the first request.
      responses:
        '200':
          description: A successful response.
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/GetAccountBalanceLedgerResponse'
components:
  schemas:
    LedgerEntryType:
      type: string
      enum:
        - DEPOSIT
        - WITHDRAWAL
        - ORDER_EXECUTION
        - CORRECTION
        - RESOLUTION
        - MANUAL_ADJUSTMENT
        - ACCOUNT_PROPERTY_ADJUSTMENT
        - COMMISSION
        - WITHDRAWAL_REJECTION
        - MANUAL_TRANSFER
        - PENDING_WITHDRAWAL_CREATION
      description: >-
        Allowlist of balance-ledger entry types that are exposed to clients.
        Internal exchange types (`NETTING`, `SECURITY_*`, `CONTRACT_EXPIRATION`,
        `BEGINNING_OF_DAY`, `INTEREST`, `SETTLEMENT_FEE`, etc.) are suppressed:
        requesting a suppressed type returns `Aborted` (409), and any suppressed
        types in upstream responses are silently filtered.
    GetAccountBalanceLedgerResponse:
      type: object
      properties:
        entries:
          type: array
          items:
            $ref: '#/components/schemas/BalanceLedgerEntry'
          description: >-
            Balance ledger entries for the requested account / time window.
            Suppressed entry types are filtered out.
        nextPageToken:
          type: string
          description: >-
            Pagination token to fetch the next page. Empty when there are no
            more results.
        eof:
          type: boolean
          description: '`true` when this response contains the final page of results.'
    BalanceLedgerEntry:
      type: object
      description: >-
        A single balance ledger entry. Each entry records the cash balance
        before and after a single change, along with the typed `entry_type` and
        human-readable `description`.
      properties:
        id:
          type: string
          description: Unique entry identifier.
        account:
          type: string
          description: >-
            Account this entry belongs to. Example:
            `firms/ISV-Alice/accounts/alice-trading`.
        currency:
          type: string
          description: ISO currency code (e.g., `USD`).
        beforeBalance:
          type: string
          description: Account balance immediately before this change (decimal string).
        afterBalance:
          type: string
          description: Account balance immediately after this change (decimal string).
        description:
          type: string
          description: Human-readable reason for the change.
        updateTime:
          type: string
          format: date-time
          description: Timestamp of the balance change.
        modifiedSecurityId:
          type: string
          description: >-
            Security ID associated with the change, if any (e.g., for
            `ORDER_EXECUTION` or `RESOLUTION` entries).
        entryType:
          $ref: '#/components/schemas/LedgerEntryType'
        symbol:
          type: string
          description: Instrument symbol associated with the change, if any.
        updateBusinessDate:
          type: string
          description: Business date for this change in `YYYY-MM-DD` format.

````