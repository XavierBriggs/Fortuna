-- Track E slice 1 (domain-analysis personas): the persona registry and the
-- persisted, append-only domain-analysis artifact. Design
-- docs/design/domain-analysis-personas-design.md §5; spec 5.5/5.7/5.9/5.11, I5/I6.
--
-- Two append-only tables, mirroring existing idioms:
--   personas         -> the calibration_params/lessons pattern (versioned,
--                       supersedes-chained registry; UNIQUE (persona_id, version)
--                       refuses a version re-issue; fortuna_refuse_mutation guard).
--   domain_analyses  -> the beliefs pattern (content-immutable artifact; only the
--                       supersession marker `status` may ever change; a dedicated
--                       content-guard trigger refuses content edits and DELETE).
-- The replay anchor (5.7/I5) is domain_analyses.content_hash over
-- findings + signal_manifest; a consuming belief's provenance points at
-- {analysis_id, content_hash} so the decision replays to the exact artifact.

-- ---------- personas (registry; append-only; supersedes-chained) ----------
CREATE TABLE personas (
    persona_row_id        TEXT PRIMARY KEY,                 -- ULID
    persona_id            TEXT NOT NULL,                    -- e.g. 'meteorologist'
    version               INTEGER NOT NULL,                 -- bumps per method change
    domain                TEXT NOT NULL,
    domain_tags           JSONB NOT NULL,
    reads_signal_kinds    JSONB NOT NULL,                   -- signal kinds this persona may read
    tier                  TEXT NOT NULL CHECK (tier IN ('cheap','synthesis')),
    method_hash           TEXT NOT NULL,                    -- SHA-256 of the trusted method file
    output_schema_version TEXT NOT NULL,
    status                TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active','retired')),
    -- superseding rows: a method change inserts a new (persona_id, version) row
    -- pointing at the one it replaces (append-only discipline).
    supersedes            TEXT REFERENCES personas(persona_row_id),
    effective_at          TEXT NOT NULL,
    created_at            TEXT NOT NULL,
    UNIQUE (persona_id, version)
);
CREATE INDEX idx_personas_id_version ON personas (persona_id, version);
CREATE TRIGGER personas_append_only BEFORE UPDATE OR DELETE ON personas
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();

-- ---------- domain_analyses (artifact; append-only; content-immutable) ----------
CREATE TABLE domain_analyses (
    analysis_id     TEXT PRIMARY KEY,                       -- ULID
    persona_id      TEXT NOT NULL,                          -- the producing persona
    persona_version INTEGER NOT NULL,
    domain          TEXT NOT NULL,
    region_key      TEXT NOT NULL,                          -- dedup/serialization key
    produced_at     TEXT NOT NULL,                          -- from the injected Clock
    signal_manifest JSONB NOT NULL,                         -- [{signal_id, content_hash}] (5.7)
    findings        JSONB NOT NULL,                         -- schema-validated structured output
    content_hash    TEXT NOT NULL,                          -- SHA-256 over findings + signal_manifest
    manifest_hash   TEXT NOT NULL,                          -- the assembled-context manifest hash
    cost_cents      BIGINT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open','superseded')),
    supersedes      TEXT REFERENCES domain_analyses(analysis_id),
    created_at      TEXT NOT NULL
);
CREATE INDEX idx_domain_analyses_region ON domain_analyses (domain, region_key, produced_at);
CREATE INDEX idx_domain_analyses_persona ON domain_analyses (persona_id, persona_version);

-- Content immutability (mirrors fortuna_beliefs_guard): the artifact is the
-- replay anchor, so findings/signal_manifest/content_hash and every other field
-- are frozen at insert; only `status` may flip (open -> superseded). DELETE is
-- refused outright.
CREATE OR REPLACE FUNCTION fortuna_domain_analyses_guard() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'domain_analyses is append-only (I5): DELETE refused';
    END IF;
    IF NEW.analysis_id     IS DISTINCT FROM OLD.analysis_id
    OR NEW.persona_id      IS DISTINCT FROM OLD.persona_id
    OR NEW.persona_version IS DISTINCT FROM OLD.persona_version
    OR NEW.domain          IS DISTINCT FROM OLD.domain
    OR NEW.region_key      IS DISTINCT FROM OLD.region_key
    OR NEW.produced_at     IS DISTINCT FROM OLD.produced_at
    OR NEW.signal_manifest IS DISTINCT FROM OLD.signal_manifest
    OR NEW.findings        IS DISTINCT FROM OLD.findings
    OR NEW.content_hash    IS DISTINCT FROM OLD.content_hash
    OR NEW.manifest_hash   IS DISTINCT FROM OLD.manifest_hash
    OR NEW.cost_cents      IS DISTINCT FROM OLD.cost_cents
    OR NEW.supersedes      IS DISTINCT FROM OLD.supersedes
    OR NEW.created_at      IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'domain_analysis content is immutable; only status may be set';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER domain_analyses_guard BEFORE UPDATE OR DELETE ON domain_analyses
    FOR EACH ROW EXECUTE FUNCTION fortuna_domain_analyses_guard();
