-- A4: per-(market,strategy) trade score from settled fills.
-- Sequences after 20260618000001_phase_c_persistence.sql.
-- Mirrors the belief_scores append-only pattern.

CREATE TABLE trade_scores (
    trade_score_id        TEXT PRIMARY KEY,   -- ULID
    market_id             TEXT NOT NULL,
    venue                 TEXT NOT NULL,
    strategy              TEXT,               -- from fills; nullable
    producer              TEXT,               -- belief-originated; NULL until D4
    realized_pnl_cents    BIGINT NOT NULL,    -- NET of basis (= the settlement delta)
    fees_cents            BIGINT NOT NULL,
    pnl_after_fees_cents  BIGINT NOT NULL,
    n_fills               BIGINT NOT NULL,
    maker_fills           BIGINT NOT NULL,    -- fill realism (count of is_maker)
    settled_at            TEXT NOT NULL,
    scored_at             TEXT NOT NULL,
    UNIQUE (market_id, strategy)              -- one score per settled market+strategy (idempotent)
);
CREATE INDEX idx_trade_scores_strategy ON trade_scores (strategy, scored_at);
CREATE TRIGGER trade_scores_append_only BEFORE UPDATE OR DELETE ON trade_scores
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
