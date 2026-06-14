-- A2d slice 3 part 1: realized-funding store (spec
-- docs/design/perp-strategies-and-scalar-claims.md §9.1; I5).
--
-- The durable record of FINALIZED 8h funding rates pulled from the PUBLIC
-- `GET /margin/funding_rates/historical` (no auth). The resolve/score loop
-- reads `realized_rate(market, funding_time)` to settle a scalar funding
-- belief against ground truth; the poller reads `latest_funding_time(market)`
-- for incremental backfill.
--
-- Conventions (initial.sql): timestamps are UTC ISO8601 TEXT (lexically
-- sortable); `funding_rate` is DOUBLE PRECISION (a cognition-only finalized
-- decimal fraction per 8h, NOT money); `mark_price` is the venue's
-- per-contract dollar STRING, stored VERBATIM (no float round-trip).
-- `fortuna_refuse_mutation()` is the shared blunt-refuse helper from the
-- initial migration (referenced, never redefined).
--
-- Append-only posture (I5): a finalized funding rate NEVER changes, so a
-- re-poll of the same (market_ticker, funding_time) is an idempotent no-op via
-- `ON CONFLICT DO NOTHING` at the app layer — it inserts nothing and the
-- append-only trigger never fires. UPDATE and DELETE are refused outright.
CREATE TABLE funding_rates_historical (
    market_ticker TEXT NOT NULL,             -- the perp ticker, e.g. "KXBTCPERP"
    funding_time  TEXT NOT NULL,             -- ISO8601, the exact 8h boundary
    funding_rate  DOUBLE PRECISION NOT NULL, -- finalized decimal fraction per 8h
    mark_price    TEXT NOT NULL,             -- per-contract dollar string, verbatim
    captured_at   TEXT NOT NULL,             -- ISO8601, when we polled it
    UNIQUE (market_ticker, funding_time)
);
CREATE INDEX idx_funding_rates_hist_market_time
    ON funding_rates_historical (market_ticker, funding_time);
CREATE TRIGGER funding_rates_historical_append_only
    BEFORE UPDATE OR DELETE ON funding_rates_historical
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
