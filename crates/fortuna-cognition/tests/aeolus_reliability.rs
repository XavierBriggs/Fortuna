//! F9: Layer-3 reliability scoring tests, driven by the RECORDED fixture
//! (`knyc_tmax.json`, μ≈87.35/σ≈1.90) scored against a chosen realized daily high.
//! The realized value stands in for the NWS-CLI grader (F2, a seam): the math is
//! validated, the extraction is not this slice's concern.

use fortuna_cognition::aeolus_forecast::{bracket_prob_ge, parse_response, Comparison, Variable};
use fortuna_cognition::aeolus_reliability::score_reliability;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
));

fn forecast() -> fortuna_cognition::aeolus_forecast::AeolusForecast {
    parse_response(FIXTURE).expect("recorded fixture parses")[0].clone()
}

#[test]
fn scores_recorded_forecast_against_a_realized_high() {
    let fc = forecast();
    let realized = 88.0; // the official NWS daily high (°F)
    let r = score_reliability(&fc, realized);

    // Scope mirrors the F8 provenance the ROTA scorecard groups by.
    assert_eq!(r.scope.model_id, "aeolus");
    assert_eq!(r.scope.model_version, "sar-semos-v1");
    assert_eq!(r.scope.station, "KNYC");
    assert_eq!(r.scope.variable, Variable::Tmax);
    assert_eq!(r.scope.target_date, "2026-06-13");
    assert_eq!(r.realized_f, 88.0);

    // All 14 fixture brackets are `ge`, so all are scored.
    assert_eq!(r.n_brackets, 14);
    assert!(r.per_bracket.iter().all(|b| b.comparison == Comparison::Ge));

    // `ge t` outcome = realized ≥ t: 81..=88 satisfied (8), 89..=94 not (6).
    let trues = r.per_bracket.iter().filter(|b| b.outcome).count();
    assert_eq!(trues, 8, "thresholds 81..=88 are satisfied by a high of 88");
    assert_eq!(r.per_bracket.len() - trues, 6, "89..=94 are not");
    for b in &r.per_bracket {
        assert_eq!(b.outcome, realized >= b.threshold_f as f64);
        // brier is exactly (p − outcome)² of the belief's own μ/σ probability.
        let o = if b.outcome { 1.0 } else { 0.0 };
        assert!((b.brier - (b.p_fortuna - o).powi(2)).abs() < 1e-12);
    }

    // Spot-check the borderline bracket: ge87, p≈0.672, outcome true (88≥87).
    let ge87 = r.per_bracket.iter().find(|b| b.threshold_f == 87).unwrap();
    assert!(ge87.outcome);
    assert!((ge87.p_fortuna - bracket_prob_ge(87, fc.mu(), fc.sigma()).unwrap()).abs() < 1e-12);
    assert!((ge87.brier - (ge87.p_fortuna - 1.0).powi(2)).abs() < 1e-12);

    // Mean Brier is a sane, well-calibrated number (the confident-correct and
    // confident-wrong tails score ~0; only the few borderline brackets carry mass).
    assert!(
        r.brier_mean > 0.0 && r.brier_mean < 0.25,
        "brier_mean = {}",
        r.brier_mean
    );

    // CRPS of the μ/σ fan vs the realized value: measured, finite, σ-scale.
    let crps = r
        .crps
        .expect("a validated scalar fan + finite realized => Some");
    assert!(
        crps.is_finite() && crps > 0.0 && crps < 6.0,
        "crps = {crps}"
    );
}

#[test]
fn confident_and_correct_brackets_score_near_zero_brier() {
    let r = score_reliability(&forecast(), 88.0);
    // ge81 (p≈0.9998, satisfied) and ge94 (p≈0.0006, not satisfied) are both
    // confident AND correct => Brier ≈ 0.
    let ge81 = r.per_bracket.iter().find(|b| b.threshold_f == 81).unwrap();
    assert!(
        ge81.outcome && ge81.brier < 1e-3,
        "ge81 brier = {}",
        ge81.brier
    );
    let ge94 = r.per_bracket.iter().find(|b| b.threshold_f == 94).unwrap();
    assert!(
        !ge94.outcome && ge94.brier < 1e-3,
        "ge94 brier = {}",
        ge94.brier
    );
}

#[test]
fn outcome_is_evaluated_at_the_integer_boundary() {
    // A realized high of exactly 87: ge87 is satisfied (87 ≥ 87), ge88 is not.
    let r = score_reliability(&forecast(), 87.0);
    assert!(
        r.per_bracket
            .iter()
            .find(|b| b.threshold_f == 87)
            .unwrap()
            .outcome
    );
    assert!(
        !r.per_bracket
            .iter()
            .find(|b| b.threshold_f == 88)
            .unwrap()
            .outcome
    );
}

#[test]
fn a_colder_realized_flips_the_outcomes_and_crps_grows() {
    // Realized far below μ (a forecast miss): few/no `ge` brackets satisfied, and
    // the CRPS (distance of the fan from the realized value) is larger.
    let warm = score_reliability(&forecast(), 88.0);
    let cold = score_reliability(&forecast(), 80.0);
    let cold_trues = cold.per_bracket.iter().filter(|b| b.outcome).count();
    assert_eq!(cold_trues, 0, "a high of 80 satisfies no ge-81..94 bracket");
    assert!(
        cold.crps.unwrap() > warm.crps.unwrap(),
        "a realized far from μ scores a worse (larger) CRPS"
    );
}
