-- WS3 S5: the append-only `validation_runs` store — the deflated G-TRUTH GO
-- surface (plan docs/superpowers/plans/2026-06-21-ws3-generic-backtest.md S5;
-- spec §7). I5.
--
-- One IMMUTABLE row per sweep, for one (scope, producer) at one `computed_at`.
-- A re-run is a NEW row, never an edit — exactly the posture of `scorecards`
-- (20260621000001). The whole-truth, overfitting-deflated GO surface rides in the
-- `payload` JSONB verbatim: {run_id, scope, producer, trial_space, n_trials,
-- family_n_trials, selected_config, brier_edge, brier_pbo, brier_spa_p, clv_edge,
-- clv_pbo, clv_spa_p, effective_n, mintrl_ok, sharpe_dsr, verdict, computed_at}.
-- Brier is the gated headline; the CLV columns are corroborating; `family_n_trials`
-- is the JOINT scope × config grid (the deflation N, BLOCK-2) — never a single
-- flattering number.
--
-- Conventions (initial.sql): ids are caller-supplied ULID TEXT; timestamps are
-- UTC ISO8601 TEXT (ms precision, lexically sortable so `ORDER BY computed_at`
-- == chronological). `producer` is NULLABLE — a merged-scope run has no producer;
-- NULL is its own bucket in the read path. Sequences strictly after
-- 20260621000001_scorecards.sql.
--
-- `fortuna_refuse_mutation()` is the shared blunt-refuse helper from the initial
-- migration (referenced, NEVER redefined here).

CREATE TABLE validation_runs (
    run_id      TEXT  PRIMARY KEY,           -- ULID (caller-supplied)
    scope       TEXT  NOT NULL,              -- opaque scope label (e.g. "weather:KNYC")
    producer    TEXT,                        -- opaque producer; NULL = merged scope
    computed_at TEXT  NOT NULL,              -- UTC ISO8601 (ms); newest wins on read
    payload     JSONB NOT NULL,              -- the full serialized ValidationRun (G-TRUTH surface)
    -- One run per (scope, producer, computed_at). NULL producer is a distinct
    -- bucket (Postgres treats NULLs as distinct in UNIQUE), which is fine for an
    -- append-only store: re-runs carry distinct run_ids/computed_at and the read
    -- path takes the newest.
    UNIQUE (scope, producer, computed_at)
);

-- Newest-run-per-scope read path: (scope, producer) prefix then computed_at DESC.
-- Indexed for the `latest` lookup.
CREATE INDEX idx_validation_runs_scope_latest
    ON validation_runs (scope, producer, computed_at DESC);

-- Append-only (I5): UPDATE and DELETE are refused outright. A correction is a new
-- row with a later computed_at, never a mutation of an existing run.
CREATE TRIGGER validation_runs_append_only
    BEFORE UPDATE OR DELETE ON validation_runs
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
