-- FORTUNA L1 schema (T0.8). Spec Section 7 + 5.5/5.12/5.13/5.14, I5.
--
-- Conventions: ids are ULID TEXT; timestamps are UTC ISO8601 TEXT with fixed
-- millisecond precision (lexically sortable; matches the in-process
-- UtcTimestamp wire form, spec DDL uses TEXT); money is BIGINT integer cents;
-- probabilities are DOUBLE PRECISION (cognition-only floats).
--
-- Append-only enforcement (I5): tables marked APPEND-ONLY get triggers that
-- reject UPDATE and DELETE outright. "Updates" are superseding rows linked
-- via supersedes columns. beliefs is content-immutable: only the scoring
-- columns (status, outcome, brier, clv_bps) may ever change.

-- ---------- helper: refuse mutation ----------
CREATE OR REPLACE FUNCTION fortuna_refuse_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'table % is append-only (I5): % refused', TG_TABLE_NAME, TG_OP;
END;
$$ LANGUAGE plpgsql;

-- ---------- canonical events (5.12) ----------
CREATE TABLE events (
    event_id            TEXT PRIMARY KEY,
    statement           TEXT NOT NULL,
    resolution_criteria TEXT NOT NULL,
    resolution_source   TEXT NOT NULL,
    horizon             TEXT,
    benchmark_at        TEXT NOT NULL,
    category            TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'created'
        CHECK (status IN ('created','active','resolution_pending',
                          'resolved_provisional','disputed','resolved_final','dead')),
    dead_reason         TEXT
        CHECK (dead_reason IS NULL OR dead_reason IN ('voided','source_lost','mutated')),
    unscoreable         BOOLEAN NOT NULL DEFAULT FALSE,
    created_at          TEXT NOT NULL
);

CREATE TABLE market_event_edges (
    edge_id      TEXT PRIMARY KEY,
    market_id    TEXT NOT NULL,
    venue        TEXT NOT NULL,
    event_id     TEXT NOT NULL REFERENCES events(event_id),
    mapping_type TEXT NOT NULL
        CHECK (mapping_type IN ('direct','negation','bracket_component','conditional_on')),
    confidence   DOUBLE PRECISION NOT NULL,
    proposed_by  TEXT NOT NULL,
    confirmed_by TEXT,
    -- superseding rows: a confidence hit or correction inserts a new edge
    -- row pointing at the one it replaces (append-only discipline).
    supersedes   TEXT REFERENCES market_event_edges(edge_id),
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_edges_market ON market_event_edges (venue, market_id);
CREATE INDEX idx_edges_event ON market_event_edges (event_id);
CREATE TRIGGER edges_append_only BEFORE UPDATE OR DELETE ON market_event_edges
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- beliefs (5.5; DDL follows the spec) ----------
CREATE TABLE beliefs (
    belief_id  TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    event_id   TEXT NOT NULL REFERENCES events(event_id),
    p          DOUBLE PRECISION NOT NULL,
    p_raw      DOUBLE PRECISION NOT NULL,
    horizon    TEXT NOT NULL,
    evidence   JSONB NOT NULL,
    provenance JSONB NOT NULL,
    supersedes TEXT REFERENCES beliefs(belief_id),
    status     TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open','resolved','superseded','abandoned')),
    outcome    INTEGER CHECK (outcome IS NULL OR outcome IN (0,1)),
    brier      DOUBLE PRECISION,
    clv_bps    DOUBLE PRECISION
);
CREATE INDEX idx_beliefs_event ON beliefs (event_id);
CREATE INDEX idx_beliefs_status ON beliefs (status);

-- Content immutability: only scoring fields may change, exactly once from NULL.
CREATE OR REPLACE FUNCTION fortuna_beliefs_guard() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'beliefs is append-only (I5): DELETE refused';
    END IF;
    IF NEW.belief_id  IS DISTINCT FROM OLD.belief_id
    OR NEW.created_at IS DISTINCT FROM OLD.created_at
    OR NEW.event_id   IS DISTINCT FROM OLD.event_id
    OR NEW.p          IS DISTINCT FROM OLD.p
    OR NEW.p_raw      IS DISTINCT FROM OLD.p_raw
    OR NEW.horizon    IS DISTINCT FROM OLD.horizon
    OR NEW.evidence   IS DISTINCT FROM OLD.evidence
    OR NEW.provenance IS DISTINCT FROM OLD.provenance
    OR NEW.supersedes IS DISTINCT FROM OLD.supersedes THEN
        RAISE EXCEPTION 'belief content is immutable; only scoring fields may be set';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER beliefs_guard BEFORE UPDATE OR DELETE ON beliefs
    FOR EACH ROW EXECUTE FUNCTION fortuna_beliefs_guard();

-- ---------- audit (I5) ----------
-- Partitioned monthly per Section 7; a DEFAULT partition makes it work out
-- of the box, monthly partitions attach via ops jobs as volume demands.
CREATE TABLE audit (
    audit_id TEXT NOT NULL,
    at       TEXT NOT NULL,
    kind     TEXT NOT NULL,
    actor    TEXT,
    ref_id   TEXT,
    payload  JSONB NOT NULL,
    PRIMARY KEY (audit_id, at)
) PARTITION BY RANGE (at);
CREATE TABLE audit_default PARTITION OF audit DEFAULT;
CREATE INDEX idx_audit_at ON audit (at);
CREATE INDEX idx_audit_kind ON audit (kind, at);
CREATE INDEX idx_audit_ref ON audit (ref_id);
CREATE TRIGGER audit_append_only BEFORE UPDATE OR DELETE ON audit
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- signals (5.11) ----------
CREATE TABLE signals (
    signal_id    TEXT NOT NULL,
    source       TEXT NOT NULL,
    type         TEXT NOT NULL,
    received_at  TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    payload      JSONB NOT NULL,
    PRIMARY KEY (signal_id, received_at),
    UNIQUE (source, content_hash, received_at)
) PARTITION BY RANGE (received_at);
CREATE TABLE signals_default PARTITION OF signals DEFAULT;
CREATE INDEX idx_signals_source ON signals (source, received_at);
CREATE TRIGGER signals_append_only BEFORE UPDATE OR DELETE ON signals
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

CREATE TABLE source_registry (
    source_id   TEXT PRIMARY KEY,
    trust_tier  INTEGER NOT NULL CHECK (trust_tier BETWEEN 0 AND 10),
    domain_tags JSONB NOT NULL DEFAULT '[]',
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- ---------- execution mirror (5.4) ----------
-- intent_events is THE durable intent journal (fortuna-exec's
-- IntentJournal); the fold is application logic.
CREATE TABLE intent_events (
    seq       BIGSERIAL PRIMARY KEY,
    intent_id TEXT NOT NULL,
    event     JSONB NOT NULL,
    at        TEXT NOT NULL
);
CREATE INDEX idx_intent_events_intent ON intent_events (intent_id, seq);
CREATE TRIGGER intent_events_append_only BEFORE UPDATE OR DELETE ON intent_events
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- Venue fill cursor checkpoints (derived state, updatable).
CREATE TABLE exec_cursors (
    venue      TEXT PRIMARY KEY,
    cursor     TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE fills (
    fill_id         TEXT PRIMARY KEY,
    venue           TEXT NOT NULL,
    venue_order_id  TEXT NOT NULL,
    client_order_id TEXT NOT NULL,
    market_id       TEXT NOT NULL,
    side            TEXT NOT NULL CHECK (side IN ('yes','no')),
    action          TEXT NOT NULL CHECK (action IN ('buy','sell')),
    price_cents     BIGINT NOT NULL,
    qty             BIGINT NOT NULL,
    fee_cents       BIGINT NOT NULL,
    is_maker        BOOLEAN NOT NULL,
    at              TEXT NOT NULL
);
CREATE INDEX idx_fills_market ON fills (market_id, at);
CREATE INDEX idx_fills_coid ON fills (client_order_id);
CREATE TRIGGER fills_append_only BEFORE UPDATE OR DELETE ON fills
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- market snapshots (Section 7: point-in-time) ----------
CREATE TABLE market_snapshots (
    snapshot_id        TEXT PRIMARY KEY,
    venue              TEXT NOT NULL,
    market_id          TEXT NOT NULL,
    title              TEXT NOT NULL,   -- UNTRUSTED external text (5.11)
    category           TEXT NOT NULL,
    status             TEXT NOT NULL,
    close_at           TEXT,
    payout_cents       BIGINT NOT NULL,
    oracle_type        TEXT NOT NULL,
    resolution_source  TEXT NOT NULL,
    expected_lag_hours INTEGER NOT NULL,
    as_of              TEXT NOT NULL
);
CREATE INDEX idx_market_snapshots ON market_snapshots (venue, market_id, as_of);
CREATE TRIGGER market_snapshots_append_only BEFORE UPDATE OR DELETE ON market_snapshots
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- price snapshots for CLV (5.5) ----------
CREATE TABLE price_snapshots (
    snapshot_id   TEXT PRIMARY KEY,
    market_id     TEXT NOT NULL,
    venue         TEXT NOT NULL,
    event_id      TEXT REFERENCES events(event_id),
    kind          TEXT NOT NULL CHECK (kind IN ('t24h','t1h','t5m','on_trade','other')),
    best_bid_cents BIGINT,
    best_ask_cents BIGINT,
    bid_qty       BIGINT,
    ask_qty       BIGINT,
    liquidity_ok  BOOLEAN NOT NULL,
    at            TEXT NOT NULL
);
CREATE INDEX idx_price_snapshots ON price_snapshots (event_id, at);
CREATE TRIGGER price_snapshots_append_only BEFORE UPDATE OR DELETE ON price_snapshots
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- settlements + discrepancies (5.13) ----------
CREATE TABLE settlement_entries (
    settlement_id TEXT PRIMARY KEY,
    market_id     TEXT NOT NULL,
    venue         TEXT NOT NULL,
    amount_cents  BIGINT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('pending','posted','confirmed','reversed')),
    -- a correction INSERTS a new row pointing at what it supersedes
    supersedes    TEXT REFERENCES settlement_entries(settlement_id),
    detail        JSONB NOT NULL DEFAULT '{}',
    at            TEXT NOT NULL
);
CREATE INDEX idx_settlements_market ON settlement_entries (market_id, at);
CREATE TRIGGER settlements_append_only BEFORE UPDATE OR DELETE ON settlement_entries
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

CREATE TABLE discrepancies (
    discrepancy_id TEXT PRIMARY KEY,
    kind           TEXT NOT NULL,
    detail         JSONB NOT NULL,
    opened_at      TEXT NOT NULL
);
CREATE TRIGGER discrepancies_append_only BEFORE UPDATE OR DELETE ON discrepancies
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- Resolutions are their own INSERT-only entries referencing the error.
CREATE TABLE discrepancy_resolutions (
    resolution_id  TEXT PRIMARY KEY,
    discrepancy_id TEXT NOT NULL REFERENCES discrepancies(discrepancy_id),
    disposition    TEXT NOT NULL CHECK (disposition IN ('matching_entry','adjustment','escalated')),
    reason         TEXT NOT NULL,
    ref_id         TEXT,
    at             TEXT NOT NULL
);
CREATE TRIGGER discrepancy_resolutions_append_only BEFORE UPDATE OR DELETE ON discrepancy_resolutions
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- memory (5.6) ----------
CREATE TABLE journal (
    journal_id TEXT PRIMARY KEY,
    day        TEXT NOT NULL,
    body       JSONB NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_journal_day ON journal (day);
CREATE TRIGGER journal_append_only BEFORE UPDATE OR DELETE ON journal
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

CREATE TABLE lessons (
    lesson_id  TEXT PRIMARY KEY,
    body       TEXT NOT NULL,
    provenance JSONB NOT NULL,
    status     TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','demoted')),
    review_at  TEXT NOT NULL,
    supersedes TEXT REFERENCES lessons(lesson_id),
    created_at TEXT NOT NULL
);
CREATE TRIGGER lessons_append_only BEFORE UPDATE OR DELETE ON lessons
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- calibration (5.10) ----------
CREATE TABLE calibration_params (
    param_id   TEXT PRIMARY KEY,
    model_id   TEXT NOT NULL,
    strategy   TEXT NOT NULL,
    category   TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('platt','isotonic','shrinkage','extremization')),
    params     JSONB NOT NULL,
    version    INTEGER NOT NULL,
    effective_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (model_id, strategy, category, kind, version)
);
CREATE TRIGGER calibration_append_only BEFORE UPDATE OR DELETE ON calibration_params
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- reservations (5.14; derived, event-sourced) ----------
CREATE TABLE reservation_events (
    seq       BIGSERIAL PRIMARY KEY,
    intent_id TEXT NOT NULL,
    strategy  TEXT NOT NULL,
    kind      TEXT NOT NULL CHECK (kind IN ('reserve','release')),
    amount_cents BIGINT NOT NULL,
    at        TEXT NOT NULL
);
CREATE INDEX idx_reservation_events_intent ON reservation_events (intent_id);
CREATE TRIGGER reservation_events_append_only BEFORE UPDATE OR DELETE ON reservation_events
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- halt persistence (I2 must survive restarts) ----------
CREATE TABLE halt_events (
    seq    BIGSERIAL PRIMARY KEY,
    scope  TEXT NOT NULL,           -- 'global' | 'strategy:<id>' | 'venue:<id>'
    kind   TEXT NOT NULL CHECK (kind IN ('set','rearm')),
    reason TEXT NOT NULL,
    actor  TEXT NOT NULL,           -- 'system' or operator identity for rearm
    at     TEXT NOT NULL
);
CREATE TRIGGER halt_events_append_only BEFORE UPDATE OR DELETE ON halt_events
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
