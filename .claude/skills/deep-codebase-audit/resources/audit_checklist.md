# Deep Codebase Audit Checklist

## Repository inventory

1. README and docs exist
2. Entrypoints identified
3. Package manager identified
4. Runtime processes identified
5. Tests identified
6. CI identified
7. Deployment path identified
8. Config and secrets path identified
9. External systems identified
10. Data stores identified

## Architecture

1. Clear domain model
2. Clear adapter boundaries
3. No vendor leakage into core logic
4. No UI owned business logic
5. No scripts acting as hidden production paths
6. No circular dependency risk
7. No hidden global state
8. Configuration is centralized and validated
9. Side effects are isolated
10. State can be rebuilt or reconciled

## Trading system specific

1. Live data ingestion works
2. Market data normalized
3. Staleness handled
4. Strategy is testable
5. Proposed orders are explicit objects
6. Risk gate is mandatory
7. Demo broker and live broker share interface
8. Live execution requires explicit arming
9. Kill switch exists
10. Order lifecycle is modeled
11. Fills are persisted
12. Positions derive from fills
13. PnL derives from fills, marks, settlements, and fees
14. Replay exists
15. Strategy attribution exists

## Tests

1. Unit tests
2. Integration tests
3. Contract tests
4. End to end tests
5. Replay tests
6. Safety tests
7. Regression tests
8. Property tests
9. Load or soak tests

## Operations

1. Structured logs
2. Metrics
3. Alerts
4. Runbooks
5. Health checks
6. Rate limit handling
7. Retry policy
8. Timeout policy
9. Backpressure handling
10. Rollback path
11. Environment separation
12. Audit log retention

## Security

1. No committed secrets
2. No secret values in logs
3. Least privilege credentials
4. Dependency scan
5. CI secret safety
6. Safe config defaults
7. Auth boundaries
8. Supply chain risk reviewed
