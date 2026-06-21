-- Phase-C persistence: settlement idempotency keys, producer columns on fills,
-- scalar_beliefs UNIQUE constraint, and append-only bus_recordings table.
-- (Task A1 — sequences after 20260617000001_event_source_evidence.sql)

-- ---------- 1. fills: add producer + strategy columns ----------
-- Nullable — legacy rows keep NULL; A2 forward will populate them.
ALTER TABLE fills ADD COLUMN IF NOT EXISTS producer TEXT;
ALTER TABLE fills ADD COLUMN IF NOT EXISTS strategy TEXT;

-- ---------- 2. settlement_entries: add intent_id + partial unique index ----------
-- Nullable so legacy rows (no intent) stay NULL and are never deduplicated.
ALTER TABLE settlement_entries ADD COLUMN IF NOT EXISTS intent_id TEXT;

-- Partial unique index: only initial rows (supersedes IS NULL) with a
-- populated intent_id are deduplicated. Correction rows (supersedes IS NOT NULL)
-- and legacy rows (intent_id IS NULL) are exempt — preserving the supersede chain
-- and backward compatibility without silent loss of operator corrections.
CREATE UNIQUE INDEX IF NOT EXISTS settlement_entries_intent_uniq
    ON settlement_entries (market_id, intent_id)
    WHERE supersedes IS NULL AND intent_id IS NOT NULL;

-- ---------- 3. scalar_beliefs: UNIQUE (producer, event_key) ----------
-- producer + event_key columns already exist (20260613000002_scalar_beliefs.sql).
-- This constraint makes the window-dedup safe under at-least-once delivery.
ALTER TABLE scalar_beliefs
    ADD CONSTRAINT scalar_beliefs_producer_event_key_uniq
    UNIQUE (producer, event_key);

-- ---------- 4. bus_recordings: new append-only table ----------
-- Stores binary-bus event segments as JSONL text for replay / audit (I5).
-- recording_id: ULID (caller-supplied); segment_seq orders segments within
-- a recording session.
CREATE TABLE IF NOT EXISTS bus_recordings (
    recording_id TEXT   PRIMARY KEY,
    segment_seq  BIGINT NOT NULL,
    jsonl        TEXT   NOT NULL,
    created_at   TEXT   NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_bus_recordings_created ON bus_recordings (created_at);

-- Reuse the existing fortuna_refuse_mutation() — never redefine it here.
CREATE TRIGGER bus_recordings_append_only
    BEFORE UPDATE OR DELETE ON bus_recordings
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
