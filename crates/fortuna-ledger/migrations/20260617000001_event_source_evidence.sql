-- World-forward evidence links (spec 5.11/5.12).
--
-- A watch event can be synthesized from many fresh signal rows. The event row
-- keeps the canonical resolution source; this append-only link table records
-- the concrete signal ids that were present in the model context when the event
-- was proposed, so replay can walk event -> evidence signals without parsing
-- free-form model text.

CREATE TABLE event_source_evidence (
    event_id           TEXT NOT NULL REFERENCES events(event_id),
    signal_id          TEXT NOT NULL,
    signal_received_at TEXT NOT NULL,
    source             TEXT NOT NULL,
    signal_type        TEXT NOT NULL,
    content_hash       TEXT NOT NULL,
    relation           TEXT NOT NULL
        CHECK (relation IN ('model_context','belief_evidence','operator_annotation')),
    created_at         TEXT NOT NULL,
    PRIMARY KEY (event_id, signal_id, signal_received_at),
    FOREIGN KEY (signal_id, signal_received_at)
        REFERENCES signals(signal_id, received_at)
);
CREATE INDEX idx_event_source_evidence_event ON event_source_evidence (event_id, created_at);
CREATE INDEX idx_event_source_evidence_signal ON event_source_evidence (signal_id, signal_received_at);
CREATE INDEX idx_event_source_evidence_source ON event_source_evidence (source, created_at);
CREATE TRIGGER event_source_evidence_append_only BEFORE UPDATE OR DELETE ON event_source_evidence
    FOR EACH ROW EXECUTE FUNCTION fortuna_refuse_mutation();
