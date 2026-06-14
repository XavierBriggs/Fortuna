//! F8 tests: emit propose-only beliefs from a parsed+validated Aeolus forecast.
//!
//! Driven from the RECORDED fixture (`fixtures/sources/aeolus/knyc_tmax.json`),
//! parsed through the F6 strict parser — no synthetic μ/σ where the recording
//! can drive it. The synthetic `lt` case is built via `parse_envelope` only to
//! exercise the `Comparison::Lt` arm the fixture (all-`ge`) cannot.

use fortuna_cognition::aeolus_beliefs::emit_aeolus_beliefs;
use fortuna_cognition::aeolus_forecast::{
    bracket_prob_ge, bracket_prob_lt, parse_envelope, parse_response,
};
use fortuna_cognition::scoring::PredictiveDistribution;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
));

fn fixture_forecast() -> fortuna_cognition::aeolus_forecast::AeolusForecast {
    let mut fcs = parse_response(FIXTURE).expect("fixture parses");
    assert_eq!(fcs.len(), 1, "fixture has exactly one forecast");
    fcs.remove(0)
}

#[test]
fn fourteen_binary_drafts_all_ge_none_skipped() {
    let fc = fixture_forecast();
    let out = emit_aeolus_beliefs(&fc);

    assert_eq!(out.binary.len(), 14, "fixture has 14 ge brackets");
    assert_eq!(
        out.skipped_in_bracket, 0,
        "fixture carries no in_bracket brackets"
    );
}

#[test]
fn binary_event_ids_and_probabilities_match_mu_sigma() {
    let fc = fixture_forecast();
    let mu = fc.mu();
    let sigma = fc.sigma();
    let out = emit_aeolus_beliefs(&fc);

    for (bracket, draft) in fc.brackets().iter().zip(out.binary.iter()) {
        // event_id is namespaced from the bracket hint.
        assert_eq!(draft.event_id, format!("aeolus:{}", bracket.event_hint));

        // p is FORTUNA's own μ/σ math (the same helper), strictly in (0,1).
        let expected = bracket_prob_ge(bracket.threshold_f, mu, sigma).expect("sigma>0 post-parse");
        assert_eq!(
            draft.p, expected,
            "p equals bracket_prob_ge(threshold,mu,sigma)"
        );
        assert!(draft.p > 0.0 && draft.p < 1.0, "p strictly inside (0,1)");

        // propose-only: no calibration, so p == p_raw.
        assert_eq!(draft.p, draft.p_raw, "propose-only: p == p_raw");

        // every draft is schema-valid.
        draft.validate().expect("draft validates");
    }

    // Spot-check a representative event_id: ge87 is FORTUNA's OWN
    // `bracket_prob_ge` value (exact), and it agrees with Aeolus's recorded
    // p≈0.6719 to the erf-approximation residual (~6.7e-8) the F6 module doc
    // pins — NOT byte-identical, since Aeolus used its own CDF.
    let ge87 = out
        .binary
        .iter()
        .find(|d| d.event_id == "aeolus:knyc-2026-06-13-tmax-ge87")
        .expect("ge87 draft present");
    assert_eq!(ge87.p, bracket_prob_ge(87, mu, sigma).expect("sigma>0"));
    assert!((ge87.p - 0.6719055375922601).abs() < 1e-3);
}

#[test]
fn binary_provenance_and_evidence_carry_the_right_data() {
    let fc = fixture_forecast();
    let out = emit_aeolus_beliefs(&fc);
    let draft = &out.binary[0];

    // provenance: model identity Aeolus knows about itself.
    let prov = &draft.provenance;
    assert_eq!(prov["model_id"], "aeolus");
    assert_eq!(prov["station"], "KNYC");
    assert_eq!(prov["variable"], "tmax");
    assert_eq!(prov["target_date"], "2026-06-13");
    assert_eq!(prov["model_version"], "sar-semos-v1");

    // evidence: the bracket cross-check rides as DATA.
    let ev = &draft.evidence;
    assert!(ev.is_array(), "evidence is a one-element array");
    let row = &ev[0];
    assert_eq!(row["source"], "aeolus");
    assert!(row.get("p_aeolus").is_some(), "evidence carries p_aeolus");
    assert!(row.get("p_fortuna").is_some(), "evidence carries p_fortuna");
    assert!(
        row.get("divergence").is_some(),
        "evidence carries divergence"
    );

    // divergence == |p_fortuna − p_aeolus|.
    let p_aeolus = row["p_aeolus"].as_f64().expect("p_aeolus is a number");
    let p_fortuna = row["p_fortuna"].as_f64().expect("p_fortuna is a number");
    let divergence = row["divergence"].as_f64().expect("divergence is a number");
    assert!((divergence - (p_fortuna - p_aeolus).abs()).abs() < 1e-12);

    // skill block rides in evidence.
    assert!(row.get("crps").is_some(), "evidence carries crps");
    assert!(
        row.get("crpss_vs_raw").is_some(),
        "evidence carries crpss key"
    );
    assert!(row.get("n_scored").is_some(), "evidence carries n_scored");
}

#[test]
fn scalar_draft_is_a_well_formed_quantile_fan() {
    let fc = fixture_forecast();
    let mu = fc.mu();
    let out = emit_aeolus_beliefs(&fc);

    assert_eq!(out.scalar.event_key, "aeolus:KNYC:tmax:2026-06-13");
    assert_eq!(out.scalar.horizon, fc.resolution().settles_after);

    let PredictiveDistribution::Scalar { quantiles, unit } = &out.scalar.predictive else {
        panic!("scalar draft must carry a Scalar distribution");
    };
    assert_eq!(unit, "degF");
    assert_eq!(quantiles.len(), 7, "pinned 7-point fan");

    // strictly increasing q, strictly increasing v (σ>0).
    for w in quantiles.windows(2) {
        assert!(w[1].q > w[0].q, "q strictly increasing");
        assert!(w[1].v > w[0].v, "v strictly increasing");
    }

    // q=0.5 value is μ.
    let median = quantiles
        .iter()
        .find(|q| (q.q - 0.5).abs() < 1e-12)
        .expect("q=0.5 present");
    assert!((median.v - mu).abs() < 1e-9, "median == mu");
    assert!((median.v - 87.347).abs() < 1e-2, "median ≈ 87.347");

    // the distribution validates.
    out.scalar.predictive.validate().expect("scalar validates");
}

#[test]
fn binary_draft_is_propose_only_no_order_fields() {
    let fc = fixture_forecast();
    let out = emit_aeolus_beliefs(&fc);

    let json = serde_json::to_value(&out.binary[0]).expect("draft serializes");
    let obj = json.as_object().expect("draft is a JSON object");
    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();

    // EXACTLY the BeliefDraft surface — no order/size/price/side field (I6).
    let mut expected = vec![
        "event_id",
        "evidence",
        "horizon",
        "p",
        "p_raw",
        "provenance",
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected, "propose-only key set, no execution fields");
}

/// A synthetic single-`lt` envelope (the fixture is all-`ge`) exercises the
/// `Comparison::Lt` arm: it must use `bracket_prob_lt`, and ge(t)+lt(t)≈1.
#[test]
fn lt_bracket_uses_lt_helper_and_is_complementary() {
    let body = r#"{
        "schema":"aeolus.forecast/v2","station":"KNYC","nws_station_id":"NYC",
        "variable":"tmax","units":"degF","target_date":"2026-06-13",
        "run_at":"2026-06-13T00:00:00+00:00","next_run_at":"2026-06-13T06:00:00+00:00",
        "valid_until":"2026-06-13T04:00:00+00:00",
        "distribution":{"family":"normal","mu":87.0,"sigma":2.0,"model_version":"sar-semos-v1"},
        "skill":{"crps":1.2,"crpss_vs_raw":null,"n_scored":30,"window_days":30,"as_of":"2026-06-12T00:00:00+00:00"},
        "resolution":{"authority":"nws_observed_high","nws_station_id":"NYC","settles_after":"2026-06-14T10:00:00+00:00","note":"x"},
        "brackets":[{"event_hint":"knyc-2026-06-13-tmax-lt88","threshold_f":88,"comparison":"lt","p":0.5}]
    }"#;
    let fc = parse_envelope(body).expect("synthetic lt envelope parses");
    let out = emit_aeolus_beliefs(&fc);

    assert_eq!(out.binary.len(), 1);
    let draft = &out.binary[0];
    let expected_lt = bracket_prob_lt(88, 87.0, 2.0).expect("sigma>0");
    assert_eq!(draft.p, expected_lt, "lt arm uses bracket_prob_lt");
    assert_eq!(draft.p, draft.p_raw);

    // complementarity: ge(t) + lt(t) ≈ 1 (the two clamp halves of the CDF).
    let ge = bracket_prob_ge(88, 87.0, 2.0).expect("sigma>0");
    assert!((draft.p + ge - 1.0).abs() < 1e-9, "ge(t)+lt(t)≈1");

    draft.validate().expect("lt draft validates");
}

/// in_bracket comparisons are skipped (a single threshold can't define a range);
/// the count is surfaced. A mixed envelope with one in_bracket among ge yields
/// `skipped_in_bracket == 1` and one fewer binary draft.
#[test]
fn in_bracket_is_skipped_and_counted() {
    let body = r#"{
        "schema":"aeolus.forecast/v2","station":"KNYC","nws_station_id":"NYC",
        "variable":"tmax","units":"degF","target_date":"2026-06-13",
        "run_at":"2026-06-13T00:00:00+00:00","next_run_at":"2026-06-13T06:00:00+00:00",
        "valid_until":"2026-06-13T04:00:00+00:00",
        "distribution":{"family":"normal","mu":87.0,"sigma":2.0,"model_version":"sar-semos-v1"},
        "skill":{"crps":1.2,"crpss_vs_raw":null,"n_scored":30,"window_days":30,"as_of":"2026-06-12T00:00:00+00:00"},
        "resolution":{"authority":"nws_observed_high","nws_station_id":"NYC","settles_after":"2026-06-14T10:00:00+00:00","note":"x"},
        "brackets":[
            {"event_hint":"knyc-2026-06-13-tmax-ge87","threshold_f":87,"comparison":"ge","p":0.5},
            {"event_hint":"knyc-2026-06-13-tmax-in86","threshold_f":86,"comparison":"in_bracket","p":0.2}
        ]
    }"#;
    let fc = parse_envelope(body).expect("mixed envelope parses");
    let out = emit_aeolus_beliefs(&fc);

    assert_eq!(out.binary.len(), 1, "only the ge bracket becomes a draft");
    assert_eq!(
        out.skipped_in_bracket, 1,
        "the in_bracket bracket is counted"
    );
}
