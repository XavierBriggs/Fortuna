-- WS2 S6b: the append-only `scorecards` snapshot store (plan
-- docs/superpowers/plans/2026-06-20-ws2-proof-layer.md Task 6 Step 5; spec 5.5,
-- §11). I5.
--
-- One IMMUTABLE row per recomputed `fortuna_scoring::Scorecard`, for one
-- (scope, producer, window) triple at one `computed_at`. A recompute is a NEW
-- row, never an edit — exactly the posture of belief_scores / bus_recordings.
-- The full pure Scorecard (Brier + baseline + the recorded Log/RPS/CRPS, CLV,
-- the CORP decomposition, PIT bins, the DM test, and the GO surface) rides in
-- the `payload` JSONB verbatim, so WS3 reuses the identical assembly without any
-- schema reshape.
--
-- Conventions (initial.sql): ids are caller-supplied ULID TEXT; timestamps are
-- UTC ISO8601 TEXT (ms precision, lexically sortable so `ORDER BY computed_at`
-- == chronological). `producer` is NULLABLE — the merged-scope card has no
-- producer; NULL is its own bucket in the read path. Sequences strictly after
-- 20260619000001_price_snapshots_market_at_unique.sql.
--
-- `fortuna_refuse_mutation()` is the shared blunt-refuse helper from the initial
-- migration (referenced, NEVER redefined here).

-- NOTE: `window` is a reserved SQL keyword (window functions), so the column is
-- quoted everywhere it appears (here, the index, and the repo queries).
CREATE TABLE scorecards (
    id          TEXT  PRIMARY KEY,           -- ULID (caller-supplied)
    scope       TEXT  NOT NULL,              -- opaque scope label (e.g. "weather:KNYC")
    producer    TEXT,                        -- opaque producer; NULL = merged scope
    "window"    TEXT  NOT NULL,              -- e.g. "forward" / "historical"
    computed_at TEXT  NOT NULL,              -- UTC ISO8601 (ms); newest wins on read
    payload     JSONB NOT NULL,              -- the full serialized Scorecard
    -- One snapshot per (scope, producer, window, computed_at). NULL producer is a
    -- distinct bucket (Postgres treats NULLs as distinct in UNIQUE), which is fine
    -- for an append-only snapshot store: re-runs carry distinct ids/computed_at and
    -- the read path takes the newest.
    UNIQUE (scope, producer, "window", computed_at)
);

-- Newest-snapshot-per-scope read path: (scope, producer, window) prefix then
-- computed_at DESC. Indexed for the `latest_scorecard` lookup.
CREATE INDEX idx_scorecards_scope_latest
    ON scorecards (scope, producer, "window", computed_at DESC);

-- Append-only (I5): UPDATE and DELETE are refused outright. A correction is a
-- new row with a later computed_at, never a mutation of an existing snapshot.
CREATE TRIGGER scorecards_append_only
    BEFORE UPDATE OR DELETE ON scorecards
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
