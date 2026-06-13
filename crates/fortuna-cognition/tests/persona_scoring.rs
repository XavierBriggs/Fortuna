//! Track E E.5 (scoring & promotion §10/§11): per-(persona, version) scoring +
//! the beat-both-baselines promote/retire PROPOSAL (recommendation-only, I7).

use fortuna_cognition::persona_scoring::{
    propose_promotion, score_persona, Baseline, PersonaScope, PersonaScopeRecord, PersonaScorecard,
    PromotionVerdict,
};

fn scope() -> PersonaScope {
    PersonaScope {
        persona_id: "meteorologist".to_string(),
        persona_version: 3,
    }
}

#[test]
fn score_persona_computes_brier_quality_and_clv() {
    let record = PersonaScopeRecord {
        scope: scope(),
        samples: vec![(0.6, true), (0.6, false)],
        clv_bps: vec![100.0, 50.0],
    };
    let card = score_persona(&record);
    assert_eq!(card.n, 2);
    // Brier = ((0.6-1)^2 + (0.6-0)^2)/2 = (0.16 + 0.36)/2 = 0.26.
    assert!((card.brier_mean - 0.26).abs() < 1e-9);
    assert_eq!(card.clv_mean_bps, Some(75.0));
}

#[test]
fn an_empty_record_scores_to_zero_not_a_panic() {
    let card = score_persona(&PersonaScopeRecord {
        scope: scope(),
        samples: vec![],
        clv_bps: vec![],
    });
    assert_eq!(card.n, 0);
    assert_eq!(card.clv_mean_bps, None);
    assert!(
        card.quality.is_finite(),
        "quality must be finite, never NaN"
    );
}

fn card(n: usize, brier: f64, clv: Option<f64>) -> PersonaScorecard {
    PersonaScorecard {
        scope: scope(),
        n,
        brier_mean: brier,
        quality: 0.9,
        clv_mean_bps: clv,
    }
}

#[test]
fn below_the_floor_is_evaluating_zero_capital() {
    let p = propose_promotion(
        &card(30, 0.18, Some(42.0)),
        None,
        Baseline { brier_mean: 0.20 },
        Baseline { brier_mean: 0.19 },
        60,
    );
    assert_eq!(
        p.verdict,
        PromotionVerdict::Evaluating {
            resolved: 30,
            needed: 60
        }
    );
}

#[test]
fn beating_both_baselines_after_the_floor_is_promotable() {
    let p = propose_promotion(
        &card(74, 0.18, Some(42.0)),
        None,
        Baseline { brier_mean: 0.20 }, // no-persona raw baseline
        Baseline { brier_mean: 0.19 }, // market baseline
        60,
    );
    assert_eq!(p.verdict, PromotionVerdict::Promotable);
}

#[test]
fn exactly_at_the_floor_is_eligible_not_evaluating() {
    // §11: "after >= 60 resolved" — n == the floor is past it (strict `<`).
    let p = propose_promotion(
        &card(60, 0.18, Some(42.0)),
        None,
        Baseline { brier_mean: 0.20 },
        Baseline { brier_mean: 0.19 },
        60,
    );
    assert_eq!(p.verdict, PromotionVerdict::Promotable);
}

#[test]
fn worse_brier_than_market_is_a_retire_candidate() {
    let p = propose_promotion(
        &card(74, 0.21, Some(42.0)),
        None,
        Baseline { brier_mean: 0.20 },
        Baseline { brier_mean: 0.19 },
        60,
    );
    assert_eq!(p.verdict, PromotionVerdict::RetireCandidate);
}

#[test]
fn non_positive_clv_blocks_promotion_even_with_good_brier() {
    // Brier beats both, but CLV <= 0 means no edge net of fees -> retire.
    let p = propose_promotion(
        &card(74, 0.17, Some(-5.0)),
        None,
        Baseline { brier_mean: 0.20 },
        Baseline { brier_mean: 0.19 },
        60,
    );
    assert_eq!(p.verdict, PromotionVerdict::RetireCandidate);
}

#[test]
fn the_prior_version_comparison_is_reported() {
    let prior = card(60, 0.205, Some(10.0));
    let p = propose_promotion(
        &card(74, 0.18, Some(42.0)),
        Some(&prior),
        Baseline { brier_mean: 0.20 },
        Baseline { brier_mean: 0.19 },
        60,
    );
    assert_eq!(p.beats_prior_version, Some(true), "0.18 <= 0.205");
    assert_eq!(p.verdict, PromotionVerdict::Promotable);

    // A worse version does not beat its prior.
    let p2 = propose_promotion(
        &card(74, 0.22, Some(1.0)),
        Some(&prior),
        Baseline { brier_mean: 0.20 },
        Baseline { brier_mean: 0.19 },
        60,
    );
    assert_eq!(p2.beats_prior_version, Some(false));
}
