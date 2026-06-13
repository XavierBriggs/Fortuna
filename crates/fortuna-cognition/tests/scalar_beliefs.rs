//! Tests for `scalar_beliefs::ScalarBeliefDraft` (perp-strategies-and-scalar-
//! claims design §1, §2.5). Written from the spec text BEFORE implementation.
//!
//! Contract under test:
//! - `ScalarBeliefDraft` is the SCALAR mirror of `beliefs::BeliefDraft`: it
//!   carries a `PredictiveDistribution` (the scalar claim), an `event_key`,
//!   `horizon`, `evidence`, and a `#[serde(default)]` `provenance`.
//! - It is `deny_unknown_fields` (I6 schema discipline), exactly like
//!   `BeliefDraft`.
//! - It round-trips through serde BYTE-STABLY — it rides the same bus/replay
//!   and persist paths whose byte-compare is load-bearing (design §2.5).
//!   `PredictiveDistribution::Scalar` embeds `f64` quantile values; this pins
//!   that they survive a JSON round trip unchanged.

use fortuna_cognition::scalar_beliefs::ScalarBeliefDraft;
use fortuna_cognition::scoring::{PredictiveDistribution, Quantile};
use fortuna_core::clock::UtcTimestamp;

fn ts(millis: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(millis).unwrap()
}

/// A valid scalar claim: ≥2 quantiles, q strictly increasing in (0,1), v
/// non-decreasing (design §1.1 validation).
fn scalar_pred() -> PredictiveDistribution {
    PredictiveDistribution::Scalar {
        quantiles: vec![
            Quantile { q: 0.1, v: -0.0003 },
            Quantile { q: 0.5, v: 0.0001 },
            Quantile { q: 0.9, v: 0.0007 },
        ],
        unit: "rate".to_string(),
    }
}

fn draft() -> ScalarBeliefDraft {
    ScalarBeliefDraft {
        event_key: "perp:KXBTCPERP:funding:2026-06-13T16:00:00Z".to_string(),
        predictive: scalar_pred(),
        horizon: ts(1_718_294_400_000),
        evidence: serde_json::json!([{"source": "funding_estimate", "candles": 312}]),
        provenance: serde_json::json!({"producer": "funding_forecast"}),
    }
}

#[test]
fn scalar_belief_draft_holds_a_scalar_predictive() {
    let d = draft();
    // The carrier holds a Scalar distribution (the harness validates the
    // shape at the persist boundary; the type itself stays a plain carrier).
    assert!(matches!(
        d.predictive,
        PredictiveDistribution::Scalar { .. }
    ));
}

#[test]
fn scalar_belief_draft_serde_round_trips_byte_stable() {
    let d = draft();
    // to_string -> from_str -> to_string must be byte-identical: the replay
    // recorder and the scalar-belief persist path both byte-compare, and the
    // embedded f64 quantile values must survive unchanged (design §2.5).
    let once = serde_json::to_string(&d).unwrap();
    let back: ScalarBeliefDraft = serde_json::from_str(&once).unwrap();
    let twice = serde_json::to_string(&back).unwrap();
    assert_eq!(once, twice, "scalar-belief JSON is not byte-stable");
    assert_eq!(d, back, "scalar-belief did not survive the round trip");
}

#[test]
fn scalar_belief_draft_rejects_unknown_fields() {
    // deny_unknown_fields (I6): an extra field is a HARD error, never silently
    // ignored — exactly the BeliefDraft discipline.
    let json = r#"{
        "event_key": "perp:KXBTCPERP:funding:2026-06-13T16:00:00Z",
        "predictive": {"kind":"scalar","quantiles":[{"q":0.1,"v":-0.0003},{"q":0.9,"v":0.0007}],"unit":"rate"},
        "horizon": "2026-06-13T16:00:00.000Z",
        "evidence": [],
        "provenance": {},
        "surprise_field": true
    }"#;
    let parsed: Result<ScalarBeliefDraft, _> = serde_json::from_str(json);
    assert!(
        parsed.is_err(),
        "deny_unknown_fields must reject an unknown field"
    );
}

#[test]
fn scalar_belief_draft_provenance_defaults_when_absent() {
    // provenance is `#[serde(default)]` (mirrors BeliefDraft): a producer that
    // omits it parses, with provenance = null (the harness stamps it later).
    let json = r#"{
        "event_key": "perp:KXBTCPERP:funding:2026-06-13T16:00:00Z",
        "predictive": {"kind":"scalar","quantiles":[{"q":0.1,"v":-0.0003},{"q":0.9,"v":0.0007}],"unit":"rate"},
        "horizon": "2026-06-13T16:00:00.000Z",
        "evidence": []
    }"#;
    let d: ScalarBeliefDraft = serde_json::from_str(json).unwrap();
    assert_eq!(d.provenance, serde_json::Value::Null);
}
