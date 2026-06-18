> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Markets

> Retrieve all markets



## OpenAPI

````yaml /api-reference/oapi-schemas/markets-schema.json get /v1/markets
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
  /v1/markets:
    get:
      tags:
        - Markets
      summary: Get Markets
      description: Retrieve all markets
      operationId: MarketService_GetMarkets
      parameters:
        - name: limit
          description: Page size
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: offset
          description: Page offset
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: orderBy
          description: Order by fields
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: orderDirection
          description: Order direction
          in: query
          required: false
          schema:
            type: string
        - name: id
          description: Market IDs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
        - name: slug
          description: Market slugs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: archived
          description: Whether market is archived
          in: query
          required: false
          schema:
            type: boolean
        - name: active
          description: Whether market is active
          in: query
          required: false
          schema:
            type: boolean
        - name: closed
          description: Whether market is closed
          in: query
          required: false
          schema:
            type: boolean
        - name: volumeNumMin
          description: Minimum volume number
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: volumeNumMax
          description: Maximum volume number
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: startDateMin
          description: Minimum start date
          in: query
          required: false
          schema:
            type: string
        - name: startDateMax
          description: Maximum start date
          in: query
          required: false
          schema:
            type: string
        - name: endDateMin
          description: Minimum end date
          in: query
          required: false
          schema:
            type: string
        - name: endDateMax
          description: Maximum end date
          in: query
          required: false
          schema:
            type: string
        - name: relatedTags
          description: Whether related tags are enabled
          in: query
          required: false
          schema:
            type: boolean
        - name: gameId
          description: Game ID
          in: query
          required: false
          schema:
            type: string
        - name: sportsMarketTypes
          description: Sports market types
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
              enum:
                - SPORTS_MARKET_TYPE_MONEYLINE
                - SPORTS_MARKET_TYPE_SPREAD
                - SPORTS_MARKET_TYPE_TOTAL
                - SPORTS_MARKET_TYPE_PROP
                - SPORTS_MARKET_TYPE_FUTURE
                - SPORTS_MARKET_TYPE_DRAWABLE_OUTCOME
        - name: includeTag
          description: Whether to include tag
          in: query
          required: false
          schema:
            type: boolean
        - name: categories
          description: Categories
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: marketTypes
          description: Market types
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: includeHidden
          description: 'Include hidden markets (default: false)'
          in: query
          required: false
          schema:
            type: boolean
        - name: tagIds
          description: Tag IDs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
      responses:
        '200':
          description: List of markets
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/v1GetMarketsResponse'
        '500':
          description: Internal server error
          content:
            application/json:
              schema: {}
components:
  schemas:
    v1GetMarketsResponse:
      type: object
      properties:
        markets:
          type: array
          items:
            $ref: '#/components/schemas/v1Market'
          description: List of markets
      description: Response containing list of markets
    v1Market:
      type: object
      properties:
        id:
          type: string
          description: Unique market identifier
        question:
          type: string
          description: Market question
          nullable: true
        slug:
          type: string
          description: Market slug for URL
          nullable: true
        endDate:
          type: string
          description: Market end date
          nullable: true
        category:
          type: string
          description: Market category
          nullable: true
        startDate:
          type: string
          description: Market start date
          nullable: true
        image:
          type: string
          description: Market image URL
          nullable: true
        description:
          type: string
          description: Market description
          nullable: true
        active:
          type: boolean
          description: Whether market is active
          nullable: true
        marketType:
          type: string
          description: Type of market
          nullable: true
        closed:
          type: boolean
          description: Whether market is closed
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        archived:
          type: boolean
          description: Whether market is archived
          nullable: true
        orderPriceMinTickSize:
          type: number
          format: decimal
          description: Minimum tick size for order price
          nullable: true
        gameStartTime:
          type: string
          description: Game start time
          nullable: true
        bestBid:
          type: number
          format: decimal
          description: Best bid price
          nullable: true
        bestAsk:
          type: number
          format: decimal
          description: Best ask price
          nullable: true
        manualActivation:
          type: boolean
          description: Whether manual activation is required
          nullable: true
        sportsMarketType:
          type: string
          description: Sports market type
          nullable: true
        line:
          type: number
          format: decimal
          description: Line value
          nullable: true
        marketSides:
          type: array
          items:
            $ref: '#/components/schemas/v1MarketSide'
          description: Market sides
        outcomes:
          type: string
          description: Outcomes JSON
          nullable: true
        outcomePrices:
          type: string
          description: Outcome prices JSON
          nullable: true
        ep3Status:
          type: string
          description: EP3 status
          nullable: true
        sportsMarketTypeV2:
          $ref: '#/components/schemas/v1SportsMarketType'
        hidden:
          type: boolean
          description: Whether market is hidden
          nullable: true
        tags:
          type: array
          items:
            $ref: '#/components/schemas/tagsv1Tag'
          description: Associated tags
        title:
          type: string
          description: Market title
          nullable: true
        subtitle:
          type: string
          description: Market subtitle
          nullable: true
        color:
          type: string
          description: Market color
          nullable: true
        darkColor:
          type: string
          description: Market dark mode color
          nullable: true
        subjectId:
          type: integer
          format: int32
          description: Subject ID
          nullable: true
        subject:
          $ref: '#/components/schemas/v1Subject'
        feeCoefficient:
          type: number
          format: decimal
          description: Fee coefficient
          nullable: true
        spreadTotalSuffix:
          type: string
          description: Spread/total suffix for UI display (e.g. points, goals, runs)
          nullable: true
        minimumTradeQty:
          type: number
          format: decimal
          description: >-
            Minimum order quantity in contracts (e.g. 0.01 = 1% of a contract,
            1.0 = one whole contract).
          nullable: true
      description: Market information and configuration
    v1MarketSide:
      type: object
      properties:
        id:
          type: string
          description: Market side ID
        marketSideType:
          $ref: '#/components/schemas/v1MarketSideType'
        identifier:
          type: string
          description: Market side identifier
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        description:
          type: string
          description: Market side description
          nullable: true
        price:
          type: string
          description: Market side price
          nullable: true
        marketId:
          type: integer
          format: int32
          description: Market ID
        long:
          type: boolean
          description: Whether market side is the long or short side of the market
          nullable: true
        teamId:
          type: integer
          format: int32
          description: Team ID - deprecated
          nullable: true
          deprecated: true
        team:
          $ref: '#/components/schemas/v1Team'
          description: Team information - deprecated, use participant instead
          deprecated: true
        participantId:
          type: string
          description: Participant ID (references participant on the event)
          nullable: true
      description: Market position information
    v1SportsMarketType:
      type: string
      enum:
        - SPORTS_MARKET_TYPE_MONEYLINE
        - SPORTS_MARKET_TYPE_SPREAD
        - SPORTS_MARKET_TYPE_TOTAL
        - SPORTS_MARKET_TYPE_PROP
        - SPORTS_MARKET_TYPE_FUTURE
        - SPORTS_MARKET_TYPE_DRAWABLE_OUTCOME
    tagsv1Tag:
      type: object
      properties:
        id:
          type: string
          description: Unique tag identifier
        label:
          type: string
          description: Tag label
          nullable: true
        slug:
          type: string
          description: Tag slug for URL
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        image:
          type: string
          description: Tag image URL
          nullable: true
        tradable:
          type: boolean
          description: Whether the tag is tradable
          nullable: true
        league:
          $ref: '#/components/schemas/v1TagLeague'
        sport:
          $ref: '#/components/schemas/v1TagSport'
        parentId:
          type: integer
          format: int32
          description: Parent tag ID
          nullable: true
        subtags:
          type: array
          items:
            type: object
            description: Nested object (recursive)
          description: Child subtags
    v1Subject:
      type: object
      properties:
        id:
          type: integer
          format: int32
          description: Subject ID
        name:
          type: string
          description: Subject name
        displayName:
          type: string
          description: Subject display name
          nullable: true
        description:
          type: string
          description: Subject description
          nullable: true
        subjectType:
          type: string
          description: Subject type (nominee, player, team, candidate, golf_player)
        image:
          type: string
          description: Subject image URL
          nullable: true
        color:
          type: string
          description: Subject color
          nullable: true
        darkColor:
          type: string
          description: Subject dark mode color
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        slug:
          type: string
          description: Subject slug for URL
          nullable: true
      description: Subject information
    v1MarketSideType:
      type: string
      enum:
        - MARKET_SIDE_TYPE_ERC1155
        - MARKET_SIDE_TYPE_INSTRUMENT
    v1Team:
      type: object
      properties:
        id:
          type: integer
          format: int32
        name:
          type: string
        abbreviation:
          type: string
        league:
          type: string
        record:
          type: string
        logo:
          type: string
        alias:
          type: string
        safeName:
          type: string
        homeIcon:
          type: string
          nullable: true
        awayIcon:
          type: string
          nullable: true
        colorPrimary:
          type: string
          nullable: true
        providerId:
          type: integer
          format: int32
          nullable: true
        ordering:
          type: string
          nullable: true
        longIcon:
          type: string
          nullable: true
        shortIcon:
          type: string
          nullable: true
        displayAbbreviation:
          type: string
          nullable: true
        ranking:
          type: string
          format: int64
          nullable: true
        conference:
          type: string
          nullable: true
        providerIds:
          type: array
          items:
            $ref: '#/components/schemas/v1SportsTeamProvider'
        longIconDark:
          type: string
          nullable: true
        shortIconDark:
          type: string
          nullable: true
    v1TagLeague:
      type: object
      properties:
        id:
          type: integer
          format: int32
        name:
          type: string
        sportId:
          type: integer
          format: int32
        tagId:
          type: integer
          format: int32
          nullable: true
        image:
          type: string
          nullable: true
        resolution:
          type: string
          nullable: true
        ordering:
          type: string
          nullable: true
        activeSeriesId:
          type: integer
          format: int32
          nullable: true
        isOperational:
          type: boolean
          nullable: true
        automaticResolution:
          type: boolean
          nullable: true
        createdAt:
          type: string
          nullable: true
        slug:
          type: string
        abbreviation:
          type: string
          nullable: true
    v1TagSport:
      type: object
      properties:
        id:
          type: integer
          format: int32
        name:
          type: string
        tagId:
          type: integer
          format: int32
          nullable: true
        createdAt:
          type: string
          nullable: true
        slug:
          type: string
        image:
          type: string
          nullable: true
    v1SportsTeamProvider:
      type: object
      properties:
        provider:
          $ref: '#/components/schemas/v1Provider'
        providerId:
          type: string
          description: The provider id
    v1Provider:
      type: string
      enum:
        - PROVIDER_SPORTSDATAIO
        - PROVIDER_SPORTRADAR

````