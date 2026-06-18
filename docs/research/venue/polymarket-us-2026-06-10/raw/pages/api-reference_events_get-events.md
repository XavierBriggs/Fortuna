> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Get Events

> Retrieve all events



## OpenAPI

````yaml /api-reference/oapi-schemas/events-schema.json get /v1/events
openapi: 3.0.3
info:
  title: protos/gateway/events/v1/events.proto
  version: 1.0.0
servers:
  - url: https://gateway.polymarket.us
    description: Production server
security: []
tags:
  - name: EventsService
paths:
  /v1/events:
    get:
      tags:
        - Events
      summary: Get Events
      description: Retrieve all events
      operationId: EventsService_GetEvents
      parameters:
        - name: limit
          description: Maximum number of events to return
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: offset
          description: Number of events to skip for pagination
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: orderBy
          description: Fields to order results by
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: orderDirection
          description: Order direction (asc or desc)
          in: query
          required: false
          schema:
            type: string
        - name: id
          description: Filter by event IDs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
        - name: slug
          description: Filter by event slugs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: archived
          description: Filter by archived status
          in: query
          required: false
          schema:
            type: boolean
        - name: active
          description: Filter by active status
          in: query
          required: false
          schema:
            type: boolean
        - name: closed
          description: Filter by closed status
          in: query
          required: false
          schema:
            type: boolean
        - name: liquidityMin
          description: Minimum liquidity filter
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: liquidityMax
          description: Maximum liquidity filter
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: volumeMin
          description: Minimum volume filter
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: volumeMax
          description: Maximum volume filter
          in: query
          required: false
          schema:
            type: number
            format: double
        - name: startDateMin
          description: Minimum start date filter
          in: query
          required: false
          schema:
            type: string
        - name: startDateMax
          description: Maximum start date filter
          in: query
          required: false
          schema:
            type: string
        - name: endDateMin
          description: Minimum end date filter
          in: query
          required: false
          schema:
            type: string
        - name: endDateMax
          description: Maximum end date filter
          in: query
          required: false
          schema:
            type: string
        - name: relatedTags
          description: Include related tags
          in: query
          required: false
          schema:
            type: boolean
        - name: tagSlug
          description: Filter by tag slug
          in: query
          required: false
          schema:
            type: string
        - name: userId
          description: Filter by user ID
          in: query
          required: false
          schema:
            type: string
        - name: featured
          description: Filter by featured status
          in: query
          required: false
          schema:
            type: boolean
        - name: seriesId
          description: Filter by series IDs
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
        - name: eventDate
          description: Filter by event date
          in: query
          required: false
          schema:
            type: string
        - name: startTimeMin
          description: Minimum start time filter
          in: query
          required: false
          schema:
            type: string
        - name: startTimeMax
          description: Maximum start time filter
          in: query
          required: false
          schema:
            type: string
        - name: excludeTagId
          description: Tag IDs to exclude
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
        - name: featuredOrder
          description: Order by featured position
          in: query
          required: false
          schema:
            type: boolean
        - name: includeTemplate
          description: Include template events
          in: query
          required: false
          schema:
            type: boolean
        - name: recurrence
          description: Filter by recurrence type
          in: query
          required: false
          schema:
            type: string
        - name: gameId
          description: Filter by game ID
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: rescheduledFromGameId
          description: Filter by rescheduled from game ID
          in: query
          required: false
          schema:
            type: integer
            format: int32
        - name: period
          description: Filter by event period
          in: query
          required: false
          schema:
            type: string
        - name: finishedTimestampMin
          description: Minimum finished timestamp filter
          in: query
          required: false
          schema:
            type: string
        - name: finishedTimestampMax
          description: Maximum finished timestamp filter
          in: query
          required: false
          schema:
            type: string
        - name: ended
          description: Filter by ended status
          in: query
          required: false
          schema:
            type: boolean
        - name: sportradarGameId
          description: Filter by Sportradar game ID
          in: query
          required: false
          schema:
            type: string
        - name: excludeSeriesId
          description: Series IDs to exclude
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
        - name: resolution
          description: Filter by resolution status
          in: query
          required: false
          schema:
            type: boolean
        - name: categories
          description: Filter by categories
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: marketTypes
          description: Filter by market types
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: string
        - name: live
          description: Filter by live status
          in: query
          required: false
          schema:
            type: boolean
        - name: excludeEventId
          description: Event IDs to exclude
          in: query
          required: false
          explode: true
          schema:
            type: array
            items:
              type: integer
              format: int32
        - name: includeHidden
          description: 'Include hidden events (default: false)'
          in: query
          required: false
          schema:
            type: boolean
        - name: tagIds
          description: Filter by tag IDs
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
          description: List of events
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/v1GetEventsResponse'
        '500':
          description: Internal server error
          content:
            application/json:
              schema: {}
components:
  schemas:
    v1GetEventsResponse:
      type: object
      properties:
        events:
          type: array
          items:
            $ref: '#/components/schemas/v1Event'
          description: List of events
      description: Response containing list of events
    v1Event:
      type: object
      properties:
        id:
          type: string
          description: Unique event identifier
        ticker:
          type: string
          description: Event ticker symbol
          nullable: true
        slug:
          type: string
          description: Event slug for URL
          nullable: true
        title:
          type: string
          description: Event title
          nullable: true
        subtitle:
          type: string
          description: Event subtitle
          nullable: true
        description:
          type: string
          description: Event description
          nullable: true
        resolutionSource:
          type: string
          description: Source for event resolution
          nullable: true
        startDate:
          type: string
          description: Event start date
          nullable: true
        creationDate:
          type: string
          description: Event creation date
          nullable: true
        endDate:
          type: string
          description: Event end date
          nullable: true
        image:
          type: string
          description: Event image URL
          nullable: true
        active:
          type: boolean
          description: Whether event is active
          nullable: true
        closed:
          type: boolean
          description: Whether event is closed
          nullable: true
        archived:
          type: boolean
          description: Whether event is archived
          nullable: true
        liquidity:
          type: number
          format: decimal
          description: Event liquidity
          nullable: true
        volume:
          type: number
          format: decimal
          description: Event volume
          nullable: true
        openInterest:
          type: number
          format: decimal
          description: Open interest
          nullable: true
        category:
          type: string
          description: Event category
          nullable: true
        subcategory:
          type: string
          description: Event subcategory
          nullable: true
        createdAt:
          type: string
          description: Creation timestamp
          nullable: true
        updatedAt:
          type: string
          description: Last update timestamp
          nullable: true
        volume24hr:
          type: number
          format: decimal
          description: 24-hour volume
          nullable: true
        volume1wk:
          type: number
          format: decimal
          description: One week volume
          nullable: true
        volume1mo:
          type: number
          format: decimal
          description: One month volume
          nullable: true
        volume1yr:
          type: number
          format: decimal
          description: One year volume
          nullable: true
        closedTime:
          type: string
          description: Event close time
          nullable: true
        eventDate:
          type: string
          description: Event date
          nullable: true
        startTime:
          type: string
          description: Event start time
          nullable: true
        seriesSlug:
          type: string
          description: Series slug
          nullable: true
        score:
          type: string
          description: Event score
          nullable: true
        elapsed:
          type: string
          description: Elapsed time
          nullable: true
        period:
          type: string
          description: Event period
          nullable: true
        live:
          type: boolean
          description: Whether event is live
          nullable: true
        ended:
          type: boolean
          description: Whether event has ended
          nullable: true
        finishedTimestamp:
          type: string
          description: Finished timestamp
          nullable: true
        featuredOrder:
          type: integer
          format: int32
          description: Featured order
          nullable: true
        markets:
          type: array
          items:
            $ref: '#/components/schemas/v1Market'
          description: Associated markets
        gameId:
          type: integer
          format: int32
          description: ID taken from our sports provider
          nullable: true
        rescheduledFromGameId:
          type: integer
          format: int32
          description: Rescheduled ID taken from our sports provider
          nullable: true
        sportradarGameId:
          type: string
          description: Sportradar game ID
          nullable: true
        eventState:
          $ref: '#/components/schemas/v1EventState'
        participants:
          type: array
          items:
            $ref: '#/components/schemas/v1Participant'
          description: Event participants
        hidden:
          type: boolean
          description: Whether event is hidden
          nullable: true
        tags:
          type: array
          items:
            $ref: '#/components/schemas/tagsv1Tag'
          description: Associated tags
        teams:
          type: array
          items:
            $ref: '#/components/schemas/v1Team'
          description: Associated teams
        metadata:
          $ref: '#/components/schemas/v1EventMetadata'
      description: Event information and configuration
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
    v1EventState:
      type: object
      properties:
        id:
          type: integer
          format: int32
        gameId:
          type: integer
          format: int32
          nullable: true
        sportradarGameId:
          type: string
          nullable: true
        type:
          type: string
        createdAt:
          type: string
          format: date-time
          nullable: true
        updatedAt:
          type: string
          format: date-time
          nullable: true
        score:
          type: string
          nullable: true
        elapsed:
          type: string
          nullable: true
        period:
          type: string
          nullable: true
        live:
          type: boolean
          nullable: true
        ended:
          type: boolean
          nullable: true
        finishedTimestamp:
          type: string
          format: date-time
          nullable: true
        footballState:
          $ref: '#/components/schemas/v1FootballState'
        ufcState:
          $ref: '#/components/schemas/v1UFCState'
        tennisState:
          $ref: '#/components/schemas/v1TennisState'
        baseballState:
          $ref: '#/components/schemas/v1BaseballState'
        mainSpreadLine:
          type: number
          format: double
          description: Main spread line
          title: Lines
          nullable: true
        mainTotalLine:
          type: number
          format: double
          description: Main total line
          nullable: true
    v1Participant:
      type: object
      properties:
        id:
          type: string
        type:
          $ref: '#/components/schemas/v1ParticipantType'
        nominee:
          $ref: '#/components/schemas/v1Nominee'
        player:
          $ref: '#/components/schemas/v1Player'
        team:
          $ref: '#/components/schemas/v1Team'
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
    v1EventMetadata:
      type: object
      properties:
        gameState:
          $ref: '#/components/schemas/v1EventState'
        latestGameUpdate:
          $ref: '#/components/schemas/v1GameUpdate'
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
    v1FootballState:
      type: object
      properties:
        down:
          type: integer
          format: int32
          nullable: true
        yard:
          type: integer
          format: int32
          nullable: true
        possessionProviderId:
          type: string
          nullable: true
    v1UFCState:
      type: object
      properties:
        weightClass:
          type: string
          nullable: true
        cardSegment:
          type: string
          nullable: true
        eventLogo:
          type: string
          nullable: true
        rounds:
          type: integer
          format: int32
          nullable: true
        order:
          type: integer
          format: int32
          nullable: true
    v1TennisState:
      type: object
      properties:
        tournamentName:
          type: string
          nullable: true
        round:
          type: string
          nullable: true
    v1BaseballState:
      type: object
      properties:
        balls:
          type: integer
          format: int32
          nullable: true
        strikes:
          type: integer
          format: int32
          nullable: true
        outs:
          type: integer
          format: int32
          nullable: true
        onFirst:
          type: boolean
          nullable: true
        onSecond:
          type: boolean
          nullable: true
        onThird:
          type: boolean
          nullable: true
        inningHalf:
          type: string
          nullable: true
    v1ParticipantType:
      type: string
      enum:
        - PARTICIPANT_TYPE_NOMINEE
        - PARTICIPANT_TYPE_PLAYER
        - PARTICIPANT_TYPE_TEAM
    v1Nominee:
      type: object
      properties:
        id:
          type: string
        name:
          type: string
    v1Player:
      type: object
      properties:
        id:
          type: string
        name:
          type: string
        team:
          $ref: '#/components/schemas/v1Team'
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
    v1GameUpdate:
      type: object
      properties:
        id:
          type: integer
          format: int32
          description: Game update ID
        eventId:
          type: integer
          format: int32
          description: Event ID
        sportradarGameId:
          type: string
          description: Sportradar game ID
        type:
          type: string
          description: Game update type
        title:
          type: string
          description: Game update title
        subtitle:
          type: string
          description: Game update subtitle
          nullable: true
        subject:
          type: string
          description: Game update subject
          nullable: true
        team:
          type: string
          description: Team name
          nullable: true
        period:
          type: string
          description: Game period
          nullable: true
        clock:
          type: string
          description: Game clock
          nullable: true
        highlightAt:
          type: string
          format: date-time
          description: Highlight timestamp
          nullable: true
        createdAt:
          type: string
          format: date-time
          description: Creation timestamp
          nullable: true
    v1MarketSideType:
      type: string
      enum:
        - MARKET_SIDE_TYPE_ERC1155
        - MARKET_SIDE_TYPE_INSTRUMENT
    v1Provider:
      type: string
      enum:
        - PROVIDER_SPORTSDATAIO
        - PROVIDER_SPORTRADAR

````