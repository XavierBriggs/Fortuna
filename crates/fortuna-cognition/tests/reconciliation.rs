//! T2.7: the daily reconciliation cycle (spec 5.8) + the aeolus_eval
//! ingestion contract (spec Section 6, item 3).
//!
//! Doctrine under test:
//! - The reconciliation cycle reads the day's fills, open positions, and
//!   originating beliefs; it produces a JOURNAL draft and STRUCTURALLY
//!   ZERO trade candidates ("No orders are placed from this loop").
//! - aeolus_eval is a SIGNAL-under-evaluation with ZERO capital: every
//!   forecast becomes a belief draft (scored like any belief), and the
//!   mapping produces NO proposals — by type, not by configuration.
//! - The envelope contract is FORTUNA's interface definition: malformed
//!   payloads (missing fields, p outside (0,1), bad dates) are REJECTED
//!   loudly; the operator fixture validates the real exporter conforms.
//!
//! Written BEFORE src/reconciliation.rs per the repository TDD doctrine.

use fortuna_cognition::mind::{MindOutput, StubMind};
use fortuna_cognition::reconciliation::{
    map_aeolus_envelope, run_reconciliation, AeolusEnvelope, ReconError,
};
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;

fn t(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

// ----------------------------------------------------------- aeolus_eval

fn envelope_json() -> serde_json::Value {
    json!({
        "station": "KNYC",
        "target_date": "2026-06-12",
        "run_at": "2026-06-11T10:00:00.000Z",
        "brackets": [
            {"event_hint": "highny-2026-06-12-t60", "p": 0.18},
            {"event_hint": "highny-2026-06-12-t65", "p": 0.55},
            {"event_hint": "highny-2026-06-12-t70", "p": 0.27}
        ]
    })
}

#[test]
fn aeolus_envelope_maps_to_zero_capital_belief_drafts() {
    let env: AeolusEnvelope = serde_json::from_value(envelope_json()).unwrap();
    let drafts = map_aeolus_envelope(&env, t("2026-06-12T23:00:00.000Z")).unwrap();

    assert_eq!(drafts.len(), 3);
    let d = &drafts[1];
    assert_eq!(d.event_id, "aeolus:highny-2026-06-12-t65");
    assert!((d.p - 0.55).abs() < 1e-9);
    assert!((d.p_raw - 0.55).abs() < 1e-9, "raw forecast preserved");
    // Evidence cites the run; provenance marks the source model.
    assert_eq!(d.evidence[0]["source"], "aeolus");
    assert_eq!(d.provenance["model_id"], "aeolus");
    // ZERO CAPITAL is structural: the mapper returns BeliefDrafts only —
    // there is no proposal type anywhere in its signature.
}

#[test]
fn malformed_envelopes_are_rejected_loudly() {
    // p outside (0,1).
    let mut bad = envelope_json();
    bad["brackets"][0]["p"] = json!(1.2);
    let env: AeolusEnvelope = serde_json::from_value(bad).unwrap();
    assert!(matches!(
        map_aeolus_envelope(&env, t("2026-06-12T23:00:00.000Z")),
        Err(ReconError::BadEnvelope { .. })
    ));

    // Empty brackets: an empty forecast is a broken export, not a no-op.
    let mut empty = envelope_json();
    empty["brackets"] = json!([]);
    let env: AeolusEnvelope = serde_json::from_value(empty).unwrap();
    assert!(map_aeolus_envelope(&env, t("2026-06-12T23:00:00.000Z")).is_err());

    // Missing fields fail at deserialization (the contract is strict).
    let missing: Result<AeolusEnvelope, _> = serde_json::from_value(json!({"station": "KNYC"}));
    assert!(missing.is_err());
}

// --------------------------------------------------------- reconciliation

fn journal_output() -> MindOutput {
    serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [{
            "market": "KXSNEAK",
            "side": "yes",
            "max_price_cents": 50,
            "thesis": "the model tries to trade from the journal loop",
            "belief_ref": "evt-x",
            "urgency": "taker"
        }],
        "journal": {"body": "Today: 3 fills, 1 settlement. Tomorrow: watch KXHIGHNY."}
    }))
    .unwrap()
}

#[tokio::test]
async fn reconciliation_yields_a_journal_and_structurally_no_orders() {
    let mind = StubMind::scripted(vec![journal_output()]);
    let outcome = run_reconciliation(&mind, &[], t("2026-06-12T00:00:00.000Z"))
        .await
        .unwrap();

    assert!(
        outcome.journal.is_some(),
        "the journal draft is the product"
    );
    assert!(
        outcome.journal.as_ref().unwrap().body.contains("Tomorrow"),
        "tomorrow's plan rides in the journal"
    );
    // "No orders are placed from this loop": even though the mind EMITTED
    // a proposal, the reconciliation outcome carries no field that could
    // hold one — discarded proposals are COUNTED for the audit row.
    assert_eq!(outcome.discarded_proposals, 1);
    assert_eq!(outcome.cycle_kind, "reconciliation");
    assert!(!outcome.manifest_hash.is_empty());
}

#[tokio::test]
async fn reconciliation_without_a_journal_is_an_error() {
    // A reconciliation run that produces no journal failed its one job.
    let empty: MindOutput = serde_json::from_value(json!({
        "beliefs": [], "proposals": [], "journal": null
    }))
    .unwrap();
    let mind = StubMind::scripted(vec![empty]);
    let err = run_reconciliation(&mind, &[], t("2026-06-12T00:00:00.000Z"))
        .await
        .unwrap_err();
    assert!(matches!(err, ReconError::NoJournal));
}
