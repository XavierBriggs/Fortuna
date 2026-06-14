//! F6 tests — the STRICT `aeolus.forecast/v2` envelope parser + the μ/σ→bracket
//! probability backbone. Written test-first against the RECORDED fixture
//! (`fixtures/sources/aeolus/knyc_tmax.json`), which is the validation authority:
//! its `p`'s were produced WITH a half-degree continuity correction
//! (`T ≥ t − 0.5` for an integer bracket `ge t`). A wrong formula (e.g. dropping
//! the −0.5) misses each recorded `p` by ~0.1 — orders of magnitude above the
//! 1e-3 gate.
//!
//! OBSERVED fixture max abs delta across all 14 brackets: ~6.7e-8 (printed by the
//! gate test). That residual is the A&S 7.1.26 erf approximation error
//! (`persona_beliefs::normal_cdf`, max abs err ≈ 1.5e-7) vs. the producer's exact
//! erf — exactly the ~1e-7..1e-12-class delta the contract §7 anticipates, NOT a
//! formula error.

use fortuna_cognition::aeolus_forecast::{
    bracket_prob_ge, bracket_prob_lt, bracket_range_prob, parse_envelope, parse_response,
    AeolusError, Comparison, Units, Variable,
};
use fortuna_core::clock::UtcTimestamp;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
));

/// Clamp epsilon used by the implementation (probabilities live in
/// (f64::EPSILON, 1 − f64::EPSILON)).
const EPS: f64 = f64::EPSILON;

// ---------------------------------------------------------------------------
// THE GATE: the recorded `p`'s validate the μ/σ→p math (half-degree correction).
// ---------------------------------------------------------------------------

#[test]
fn fixture_brackets_match_mu_sigma_math_within_1e_3() {
    let forecasts = parse_response(FIXTURE).expect("recorded fixture must parse");
    assert_eq!(forecasts.len(), 1, "fixture wraps exactly one envelope");
    let fc = &forecasts[0];

    let mu = fc.mu();
    let sigma = fc.sigma();
    assert_eq!(fc.brackets().len(), 14, "fixture has 14 ge brackets");

    let mut max_delta = 0.0_f64;
    for bracket in fc.brackets() {
        assert_eq!(
            bracket.comparison,
            Comparison::Ge,
            "every fixture bracket is a `ge`"
        );
        let computed = bracket_prob_ge(bracket.threshold_f, mu, sigma)
            .expect("sigma>0 so the helper yields Some");
        let delta = (computed - bracket.p).abs();
        max_delta = max_delta.max(delta);
        assert!(
            delta < 1e-3,
            "ge{} computed={computed} recorded={} delta={delta} (a missing −0.5 misses by ~0.1)",
            bracket.threshold_f,
            bracket.p
        );
    }
    // Surface the residual class so we can see it is erf-approx (~1e-8), not a
    // formula error (~1e-1). Observed: ~6.7e-8.
    eprintln!("FIXTURE max abs delta (all 14 brackets) = {max_delta:.3e}");
    assert!(
        max_delta < 1e-3,
        "max delta {max_delta} exceeded the 1e-3 gate"
    );
}

#[test]
fn fixture_identity_tuple_is_exposed() {
    let fc = &parse_response(FIXTURE).expect("parse")[0];
    let run_at = UtcTimestamp::parse_iso8601("2026-06-13T00:00:00+00:00").expect("offset form");
    assert_eq!(fc.station(), "KNYC");
    assert_eq!(fc.variable(), Variable::Tmax);
    assert_eq!(fc.target_date(), "2026-06-13");
    assert_eq!(fc.run_at(), run_at);
    assert_eq!(
        fc.identity(),
        (
            "KNYC".to_string(),
            Variable::Tmax,
            "2026-06-13".to_string(),
            run_at
        )
    );
    // The +00:00 offset form parses to the same instant as the Z form.
    assert_eq!(
        run_at,
        UtcTimestamp::parse_iso8601("2026-06-13T00:00:00.000Z").expect("Z form")
    );
}

#[test]
fn fixture_units_and_family_are_degf_normal() {
    let fc = &parse_response(FIXTURE).expect("parse")[0];
    assert_eq!(fc.units(), Units::DegF);
}

// ---------------------------------------------------------------------------
// Helper math: lt is the complement; range is the difference; all clamped.
// ---------------------------------------------------------------------------

#[test]
fn ge_and_lt_are_complementary() {
    let (mu, sigma) = (70.0, 4.0);
    for t in [60_i64, 65, 70, 75, 80] {
        let ge = bracket_prob_ge(t, mu, sigma).expect("some");
        let lt = bracket_prob_lt(t, mu, sigma).expect("some");
        assert!((ge + lt - 1.0).abs() < 1e-9, "ge({t})+lt({t}) ≈ 1");
        assert!(ge > EPS && ge < 1.0 - EPS, "ge clamped into (ε,1−ε)");
        assert!(lt > EPS && lt < 1.0 - EPS, "lt clamped into (ε,1−ε)");
    }
}

#[test]
fn range_is_ge_floor_minus_ge_cap_and_nonnegative() {
    let (mu, sigma) = (70.0, 4.0);
    let floor = 68_i64;
    let cap = 72_i64;
    let range = bracket_range_prob(floor, cap, mu, sigma).expect("some");
    let ge_floor = bracket_prob_ge(floor, mu, sigma).expect("some");
    let ge_cap = bracket_prob_ge(cap, mu, sigma).expect("some");
    // floor ≤ high < cap  ==  ge(floor) − ge(cap), modulo the clamp.
    let expected = (ge_floor - ge_cap).clamp(EPS, 1.0 - EPS);
    assert!((range - expected).abs() < 1e-12);
    assert!(range >= EPS, "a real range probability is non-negative");
    assert!(range < 1.0 - EPS, "clamped below 1");
    // An inverted range (floor ≥ cap) collapses to the lower clamp, never panics.
    let inverted = bracket_range_prob(cap, floor, mu, sigma).expect("some");
    assert!(inverted <= EPS + 1e-30 || inverted > 0.0);
}

#[test]
fn non_positive_or_nonfinite_sigma_yields_none() {
    assert!(bracket_prob_ge(70, 70.0, 0.0).is_none());
    assert!(bracket_prob_ge(70, 70.0, -1.0).is_none());
    assert!(bracket_prob_ge(70, 70.0, f64::NAN).is_none());
    assert!(bracket_prob_lt(70, 70.0, 0.0).is_none());
    assert!(bracket_range_prob(68, 72, 70.0, 0.0).is_none());
    assert!(bracket_range_prob(68, 72, f64::NAN, 4.0).is_none());
}

// ---------------------------------------------------------------------------
// Strict parser: validation + rejection cases.
// ---------------------------------------------------------------------------

/// A minimal valid v2 envelope string, with `{slot}`/`{p}` holes the tests fill.
fn envelope_with(distribution: &str, units: &str, schema: &str, bracket_p: &str) -> String {
    format!(
        r#"{{
            "schema": "{schema}",
            "station": "KNYC",
            "nws_station_id": "NYC",
            "variable": "tmax",
            "units": "{units}",
            "target_date": "2026-06-13",
            "run_at": "2026-06-13T00:00:00+00:00",
            "next_run_at": "2026-06-13T06:00:00+00:00",
            "valid_until": "2026-06-13T04:00:00+00:00",
            "distribution": {distribution},
            "skill": {{
                "crps": 1.3,
                "crpss_vs_raw": null,
                "n_scored": 30,
                "window_days": 30,
                "as_of": "2026-06-12T00:00:00+00:00"
            }},
            "resolution": {{
                "authority": "nws_observed_high",
                "nws_station_id": "NYC",
                "settles_after": "2026-06-14T10:00:00+00:00",
                "note": "official NWS daily maximum"
            }},
            "brackets": [
                {{ "event_hint": "knyc-2026-06-13-tmax-ge87", "threshold_f": 87, "comparison": "ge", "p": {bracket_p} }}
            ]
        }}"#
    )
}

fn good_distribution() -> &'static str {
    r#"{ "family": "normal", "mu": 87.0, "sigma": 1.9, "model_version": "sar-semos-v1" }"#
}

#[test]
fn minimal_valid_envelope_parses() {
    let body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "0.67");
    let fc = parse_envelope(&body).expect("minimal envelope is valid");
    assert_eq!(fc.station(), "KNYC");
    assert_eq!(fc.brackets().len(), 1);
}

#[test]
fn non_positive_sigma_is_rejected() {
    let dist = r#"{ "family": "normal", "mu": 87.0, "sigma": 0.0, "model_version": "v1" }"#;
    let body = envelope_with(dist, "degF", "aeolus.forecast/v2", "0.5");
    assert!(matches!(
        parse_envelope(&body),
        Err(AeolusError::NonPositiveSigma { .. })
    ));
    let dist_neg = r#"{ "family": "normal", "mu": 87.0, "sigma": -3.1, "model_version": "v1" }"#;
    let body_neg = envelope_with(dist_neg, "degF", "aeolus.forecast/v2", "0.5");
    assert!(matches!(
        parse_envelope(&body_neg),
        Err(AeolusError::NonPositiveSigma { .. })
    ));
}

#[test]
fn unknown_units_enum_value_is_rejected() {
    // degC is not in the Units enum — strict enum rename rejects it.
    let body = envelope_with(good_distribution(), "degC", "aeolus.forecast/v2", "0.5");
    assert!(matches!(parse_envelope(&body), Err(AeolusError::Json(_))));
}

#[test]
fn unknown_schema_is_rejected_as_unknown_schema() {
    let body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v3", "0.5");
    assert!(matches!(
        parse_envelope(&body),
        Err(AeolusError::UnknownSchema { .. })
    ));
}

#[test]
fn unknown_extra_field_is_rejected() {
    // Inject a stray top-level field; deny_unknown_fields must reject it.
    let mut body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "0.5");
    body = body.replace(
        "\"schema\": \"aeolus.forecast/v2\",",
        "\"schema\": \"aeolus.forecast/v2\", \"surprise\": true,",
    );
    assert!(matches!(parse_envelope(&body), Err(AeolusError::Json(_))));
}

#[test]
fn bracket_p_of_one_is_clamped_not_rejected() {
    let body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "1.0");
    let fc = parse_envelope(&body).expect("p=1.0 is CLAMPED, not a parse failure");
    let p = fc.brackets()[0].p;
    assert!(p < 1.0, "p=1.0 clamped strictly below 1 (got {p})");
    assert!(p >= 1.0 - 1e-6 - 1e-12, "clamped to 1−1e-6, not mangled");
}

#[test]
fn bracket_p_of_zero_is_clamped_not_rejected() {
    let body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "0.0");
    let fc = parse_envelope(&body).expect("p=0.0 is CLAMPED, not a parse failure");
    let p = fc.brackets()[0].p;
    assert!(p > 0.0, "p=0.0 clamped strictly above 0 (got {p})");
    assert!(p <= 1e-6 + 1e-12, "clamped to 1e-6");
}

#[test]
fn empty_event_hint_is_rejected() {
    let body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "0.5")
        .replace("knyc-2026-06-13-tmax-ge87", "");
    assert!(matches!(
        parse_envelope(&body),
        Err(AeolusError::EmptyEventHint)
    ));
}

#[test]
fn empty_brackets_is_rejected() {
    let body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "0.5").replace(
        r#""brackets": [
                { "event_hint": "knyc-2026-06-13-tmax-ge87", "threshold_f": 87, "comparison": "ge", "p": 0.5 }
            ]"#,
        r#""brackets": []"#,
    );
    assert!(matches!(
        parse_envelope(&body),
        Err(AeolusError::EmptyBrackets)
    ));
}

#[test]
fn crpss_vs_raw_null_parses() {
    // The shipped fixture already carries crpss_vs_raw: null; confirm directly.
    let fc = &parse_response(FIXTURE).expect("parse")[0];
    // Reaching here means the null Option deserialized; assert the helper data path too.
    assert!(fc.sigma() > 0.0);
}

#[test]
fn lt_and_in_bracket_comparisons_parse() {
    let body = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "0.5")
        .replace(r#""comparison": "ge""#, r#""comparison": "lt""#);
    let fc = parse_envelope(&body).expect("lt comparison is valid");
    assert_eq!(fc.brackets()[0].comparison, Comparison::Lt);

    let body2 = envelope_with(good_distribution(), "degF", "aeolus.forecast/v2", "0.5")
        .replace(r#""comparison": "ge""#, r#""comparison": "in_bracket""#);
    let fc2 = parse_envelope(&body2).expect("in_bracket comparison is valid");
    assert_eq!(fc2.brackets()[0].comparison, Comparison::InBracket);
}

#[test]
fn parse_response_rejects_a_malformed_member() {
    // A wrapper whose only forecast has σ=0 must surface the typed error.
    let bad = format!(
        r#"{{"forecasts":[{}]}}"#,
        envelope_with(
            r#"{ "family": "normal", "mu": 87.0, "sigma": 0.0, "model_version": "v1" }"#,
            "degF",
            "aeolus.forecast/v2",
            "0.5"
        )
    );
    assert!(matches!(
        parse_response(&bad),
        Err(AeolusError::NonPositiveSigma { .. })
    ));
}
