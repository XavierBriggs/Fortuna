-- T5.B7 slice 1b: scalar-belief storage (spec
-- docs/design/perp-strategies-and-scalar-claims.md §1.3/§1.4, §9.1). I5.
--
-- A scalar-belief path PARALLEL to the binary `beliefs` table (binary path
-- untouched). Two append-only tables:
--   scalar_beliefs : the durable, immutable scalar forecast claim; the
--                    realized value is written exactly once on resolution.
--   belief_scores  : derived, rule-tagged scores over the immutable claim;
--                    one row per (belief, rule), fully immutable.
--
-- Conventions (initial.sql): ids are ULID TEXT; timestamps are UTC ISO8601
-- TEXT (ms precision, lexically sortable); forecast quantities are DOUBLE
-- PRECISION (cognition-only floats, never money — quantiles ride as JSONB).
-- `fortuna_refuse_mutation()` is the shared blunt-refuse helper from the
-- initial migration (referenced, never redefined).

-- ---------- scalar_beliefs (§1.4) ----------
-- `producer` is a FIRST-CLASS column: the ROTA §9.1 scorecard groups by it
-- (funding_forecast now; aeolus/personas later with ZERO view change).
-- `event_key` is free-form with NO foreign key at rung-0 — scalar events
-- (funding windows, weather targets) need not map to the `events` table.
CREATE TABLE scalar_beliefs (
    belief_id      TEXT PRIMARY KEY,                 -- ULID
    producer       TEXT NOT NULL,                    -- groups the §9.1 scorecard
    event_key      TEXT NOT NULL,                    -- free-form; no FK at rung-0
    quantiles      JSONB NOT NULL,                   -- the prob_claims/v1 quantile fan
    unit           TEXT NOT NULL,                    -- e.g. "rate", "celsius"
    horizon        TEXT NOT NULL,
    provenance     JSONB NOT NULL,
    created_at     TEXT NOT NULL,
    realized_value DOUBLE PRECISION,                 -- NULL until resolved
    resolved_at    TEXT                              -- NULL until resolved
);
CREATE INDEX idx_scalar_beliefs_producer ON scalar_beliefs (producer, created_at);
CREATE INDEX idx_scalar_beliefs_event_key ON scalar_beliefs (event_key);
CREATE INDEX idx_scalar_beliefs_created ON scalar_beliefs (created_at);

-- Content immutability (mirrors fortuna_beliefs_guard, initial.sql:79-99):
-- DELETE is refused outright; an UPDATE is refused unless the ONLY changes are
-- realized_value and/or resolved_at transitioning FROM NULL (the one-time
-- resolution). Any content mutation, or a re-write of an already-set
-- resolution column, is refused. A UNIQUE name (not shared with `beliefs`).
CREATE OR REPLACE FUNCTION fortuna_scalar_beliefs_guard() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'scalar_beliefs is append-only (I5): DELETE refused';
    END IF;
    -- Every content column is frozen.
    IF NEW.belief_id  IS DISTINCT FROM OLD.belief_id
    OR NEW.producer   IS DISTINCT FROM OLD.producer
    OR NEW.event_key  IS DISTINCT FROM OLD.event_key
    OR NEW.quantiles  IS DISTINCT FROM OLD.quantiles
    OR NEW.unit       IS DISTINCT FROM OLD.unit
    OR NEW.horizon    IS DISTINCT FROM OLD.horizon
    OR NEW.provenance IS DISTINCT FROM OLD.provenance
    OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'scalar belief content is immutable; only the resolution columns may be set';
    END IF;
    -- The resolution columns may be set exactly once, FROM NULL only.
    IF OLD.realized_value IS NOT NULL
   AND NEW.realized_value IS DISTINCT FROM OLD.realized_value THEN
        RAISE EXCEPTION 'scalar belief realized_value is set once (already resolved)';
    END IF;
    IF OLD.resolved_at IS NOT NULL
   AND NEW.resolved_at IS DISTINCT FROM OLD.resolved_at THEN
        RAISE EXCEPTION 'scalar belief resolved_at is set once (already resolved)';
    END IF;
    -- A no-op UPDATE (NEW == OLD on every column) falls through and is allowed,
    -- exactly as fortuna_beliefs_guard (initial.sql) does: it mutates no data,
    -- only touches the WAL. Content mutation, DELETE, and re-resolution are all
    -- refused above; the binary `beliefs` table holds the identical posture.
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER scalar_beliefs_guard BEFORE UPDATE OR DELETE ON scalar_beliefs
    FOR EACH ROW EXECUTE FUNCTION fortuna_scalar_beliefs_guard();

-- ---------- belief_scores (§1.3) ----------
-- Derived, re-computable, rule-tagged scores over the immutable claim.
-- One row per (belief_id, rule_id) — exactly-once per rule; several scorers
-- run side by side. `score` is lower-is-better. Fully immutable (the blunt
-- refuse — a score is a deterministic function of the durable facts + a rule,
-- never edited; a correction is a new rule id, not a mutation).
CREATE TABLE belief_scores (
    score_id   TEXT PRIMARY KEY,          -- ULID
    belief_id  TEXT NOT NULL REFERENCES scalar_beliefs(belief_id),
    rule_id    TEXT NOT NULL,             -- the ScoringRule::id(), e.g. "crps_pinball"
    score      DOUBLE PRECISION NOT NULL, -- lower is better
    scored_at  TEXT NOT NULL,
    UNIQUE (belief_id, rule_id)
);
CREATE INDEX idx_belief_scores_belief ON belief_scores (belief_id);
CREATE INDEX idx_belief_scores_rule ON belief_scores (rule_id, scored_at);
CREATE TRIGGER belief_scores_append_only BEFORE UPDATE OR DELETE ON belief_scores
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
