-- Idempotency index for price_snapshots capture (WS1 slice 4).
-- The daemon persists a cadence snapshot per tracked market per segment;
-- at most ONE snapshot per (market_id, at) timestamp is needed for CLV.
-- ON CONFLICT (market_id, at) DO NOTHING in the INSERT ensures
-- idempotency without UPDATE/DELETE (which the append-only trigger forbids).
CREATE UNIQUE INDEX idx_price_snapshots_market_at
    ON price_snapshots (market_id, at);
