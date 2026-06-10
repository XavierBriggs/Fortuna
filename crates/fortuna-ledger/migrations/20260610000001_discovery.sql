-- T3.2: market-back discovery persistence (spec 5.12).
-- Tradability scores: one append-only row per scoring run per market.

CREATE TABLE tradability_scores (
    score_id   TEXT PRIMARY KEY,
    market_id  TEXT NOT NULL,
    venue      TEXT NOT NULL,
    score      DOUBLE PRECISION NOT NULL CHECK (score >= 0.0 AND score <= 1.0),
    components JSONB NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX tradability_market_idx ON tradability_scores (market_id, created_at DESC);
CREATE TRIGGER tradability_append_only BEFORE UPDATE OR DELETE ON tradability_scores
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
