# Fortuna Audit Profile

Use this profile when auditing Fortuna or a similar trading system.

## North star

Fortuna should become a safe, replayable, venue connected trading system that can close the loop from live market data to risk gated demo execution to auditable PnL, with a path toward a $50k PnL system.

## MVP loop

Live venue market data
to normalized market snapshot
to strategy evaluation
to proposed order
to risk approval or rejection
to demo execution
to position update
to PnL update
to append only audit log
to replayable decision record
to dashboard or report

## Highest risk areas

1. Demo versus live boundary
2. Kalshi coupling
3. Risk gate enforcement
4. Order lifecycle
5. PnL truth
6. Replayability
7. Websocket reliability
8. Market data staleness
9. Credential safety
10. Missing observability

## Required abstractions

1. VenueAdapter
2. MarketEvent
3. MarketSnapshot
4. Strategy
5. ProposedOrder
6. RiskDecision
7. ExecutionAdapter
8. DemoBroker
9. LiveBroker
10. OrderEvent
11. FillEvent
12. Position
13. PnLState
14. AuditEvent
15. ReplayHarness

## Red flags

1. Strategy imports Kalshi SDK
2. Risk logic reads raw Kalshi payloads
3. UI computes business critical PnL
4. Demo execution has a separate fake path unrelated to live execution
5. Live credentials loaded during demo mode
6. No kill switch
7. No max order or max daily loss
8. No idempotency key for order submission
9. No fill persistence
10. No replay fixtures
11. No recorded decision inputs
12. No test proving demo cannot call live execution
