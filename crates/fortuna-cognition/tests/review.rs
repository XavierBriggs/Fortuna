//! T3.1: the weekly and monthly review jobs (spec 5.8).
//!
//! Doctrine under test:
//! - The WEEKLY review's deterministic core (calibration audit per scope,
//!   GO/NO-GO recommendations against Section 11 thresholds) NEVER
//!   depends on the mind: commentary and lesson candidates are a layered
//!   extra; a failed or unparseable mind degrades to a report without
//!   them, never a lost report.
//! - Calibration refits happen at n >= 50 only, produce VERSIONED params
//!   (prior version + 1), and a degenerate record refuses to fit rather
//!   than lying.
//! - GO/NO-GO outputs are RECOMMENDATIONS with reasons (I7: promotion is
//!   a human action; nothing here mutates a stage).
//! - Lesson candidates are PROPOSALS for the operator review queue;
//!   promotion to semantic memory is an operator action. Monthly decay
//!   demotes lessons whose review date passed without confirmation.
//! - The MONTHLY review is fully deterministic: envelope allocation
//!   recommendations (advisory; never invents capital), the
//!   cost-of-cognition audit, and the operator checklist (kill-switch
//!   test, backup restore drill — reminders, never performed).
//!
//! Written BEFORE src/review.rs per the repository TDD doctrine.

use fortuna_cognition::calibration::CalibrationMethod;
use fortuna_cognition::mind::{MindOutput, StubMind};
use fortuna_cognition::review::{
    calibration_report, go_nogo, monthly_review, weekly_review, AllocationInput, GoNoGoThresholds,
    LessonStatusView, ScopeKey, ScopeRecord, StrategyKindView, StrategyRecord, Verdict,
};
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;
use std::collections::BTreeMap;

fn t(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

fn scope(category: &str) -> ScopeKey {
    ScopeKey {
        model_id: "claude-fable-5".to_string(),
        strategy: "synth_events".to_string(),
        category: category.to_string(),
    }
}

fn overconfident_samples(n_pairs: usize) -> Vec<(f64, bool)> {
    let mut samples = Vec::new();
    for i in 0..n_pairs {
        samples.push((0.9, i % 10 < 7));
        samples.push((0.1, i % 10 < 3));
    }
    samples
}

// ------------------------------------------------------- calibration report

#[test]
fn calibration_report_refits_at_50_and_versions_forward() {
    let records = vec![
        ScopeRecord {
            key: scope("weather"),
            samples: overconfident_samples(30), // n = 60: refit
            clv_bps: vec![120.0, -40.0, 220.0],
        },
        ScopeRecord {
            key: scope("politics"),
            samples: overconfident_samples(10), // n = 20: below threshold
            clv_bps: vec![],
        },
    ];
    let mut priors = BTreeMap::new();
    priors.insert(scope("weather"), 3u32);

    let report = calibration_report(&records, &priors);
    assert_eq!(report.len(), 2);

    let weather = &report[0];
    assert_eq!(weather.n, 60);
    let fitted = weather.fitted.as_ref().expect("n=60 refits");
    assert_eq!(fitted.version, 4, "prior version 3 -> 4");
    assert!(matches!(fitted.method, CalibrationMethod::Platt(_)));
    assert_eq!(fitted.fitted_on_n, 60);
    assert!(weather.brier_mean > 0.0 && weather.brier_mean < 1.0);
    assert!(weather.quality > 0.0);
    assert!(!weather.curve.is_empty());
    let clv = weather.clv_mean_bps.expect("clv measured");
    assert!((clv - 100.0).abs() < 1e-9, "mean of 120/-40/220 = 100");

    let politics = &report[1];
    assert!(politics.fitted.is_none(), "n=20 must NOT fit (spec 5.10)");
    assert_eq!(
        politics.fitted_version_would_be, 1,
        "no prior -> v1 when it earns one"
    );
    assert!(politics.clv_mean_bps.is_none());
}

#[test]
fn calibration_report_refuses_degenerate_records_without_lying() {
    // 60 resolved beliefs that ALL hit: a logistic cannot be identified.
    let all_hit: Vec<(f64, bool)> = (0..60).map(|_| (0.8, true)).collect();
    let records = vec![ScopeRecord {
        key: scope("weather"),
        samples: all_hit,
        clv_bps: vec![],
    }];
    let report = calibration_report(&records, &BTreeMap::new());
    assert!(report[0].fitted.is_none(), "degenerate record: no fit");
    assert!(
        report[0]
            .fit_defect
            .as_deref()
            .unwrap_or("")
            .contains("degenerate"),
        "the refusal is explained, not silent"
    );
}

// ------------------------------------------------------------- go / no-go

fn thresholds() -> GoNoGoThresholds {
    GoNoGoThresholds {
        min_paper_days_mechanical: 30,
        min_resolved_beliefs_synthesis: 60,
        max_fee_pnl_ratio: 0.35,
    }
}

#[test]
fn go_nogo_recommends_with_reasons_and_never_promotes() {
    let records = vec![
        // Mechanical, enough days, profitable net of fees: GO.
        StrategyRecord {
            strategy: "mech_structural".to_string(),
            kind: StrategyKindView::Mechanical,
            paper_days: 45,
            resolved_beliefs: 0,
            realized_pnl_cents: 90_000,
            fees_cents: 20_000,
            clv_mean_bps: None,
            invariant_violations: 0,
        },
        // Synthesis, too few resolved beliefs: INSUFFICIENT DATA.
        StrategyRecord {
            strategy: "synth_events".to_string(),
            kind: StrategyKindView::Synthesis,
            paper_days: 45,
            resolved_beliefs: 20,
            realized_pnl_cents: 5_000,
            fees_cents: 1_000,
            clv_mean_bps: Some(80.0),
            invariant_violations: 0,
        },
        // Mechanical, fee ratio breach: NO-GO with the reason named.
        StrategyRecord {
            strategy: "mech_extremes".to_string(),
            kind: StrategyKindView::Mechanical,
            paper_days: 60,
            resolved_beliefs: 0,
            realized_pnl_cents: 10_000,
            fees_cents: 9_000,
            clv_mean_bps: None,
            invariant_violations: 0,
        },
        // Any invariant violation is an unconditional NO-GO.
        StrategyRecord {
            strategy: "mech_violator".to_string(),
            kind: StrategyKindView::Mechanical,
            paper_days: 60,
            resolved_beliefs: 0,
            realized_pnl_cents: 99_000,
            fees_cents: 1_000,
            clv_mean_bps: None,
            invariant_violations: 1,
        },
    ];

    let recs = go_nogo(&records, &thresholds());
    assert_eq!(recs.len(), 4);

    assert_eq!(recs[0].verdict, Verdict::Go);
    assert_eq!(recs[1].verdict, Verdict::InsufficientData);
    assert!(recs[1].reasons.iter().any(|r| r.contains("resolved")));
    assert_eq!(recs[2].verdict, Verdict::NoGo);
    assert!(recs[2].reasons.iter().any(|r| r.contains("fee")));
    assert_eq!(recs[3].verdict, Verdict::NoGo);
    assert!(recs[3].reasons.iter().any(|r| r.contains("invariant")));
    // The recommendation type carries NO stage mutation surface: verdict
    // + reasons + strategy name only (I7 is the operator's).
}

#[test]
fn synthesis_needs_positive_clv_for_go() {
    let mut record = StrategyRecord {
        strategy: "synth_events".to_string(),
        kind: StrategyKindView::Synthesis,
        paper_days: 90,
        resolved_beliefs: 80,
        realized_pnl_cents: 50_000,
        fees_cents: 5_000,
        clv_mean_bps: Some(45.0),
        invariant_violations: 0,
    };
    let go = go_nogo(std::slice::from_ref(&record), &thresholds());
    assert_eq!(go[0].verdict, Verdict::Go);

    record.clv_mean_bps = Some(-10.0);
    let nogo = go_nogo(std::slice::from_ref(&record), &thresholds());
    assert_eq!(nogo[0].verdict, Verdict::NoGo);
    assert!(nogo[0].reasons.iter().any(|r| r.contains("CLV")));

    record.clv_mean_bps = None;
    let insufficient = go_nogo(std::slice::from_ref(&record), &thresholds());
    assert_eq!(insufficient[0].verdict, Verdict::InsufficientData);
}

// ----------------------------------------------------------- weekly review

fn commentary_output(valid: bool) -> MindOutput {
    let body = if valid {
        json!({
            "commentary": "Weather scope tempering improved; politics still thin.",
            "lesson_candidates": [{
                "body": "NWS discussion updates before 06Z lead Kalshi high-temp markets",
                "provenance": {"journal_days": ["2026-06-08", "2026-06-09"]}
            }]
        })
        .to_string()
    } else {
        "free prose, not the contract".to_string()
    };
    serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": body}
    }))
    .unwrap()
}

#[tokio::test]
async fn weekly_review_layers_commentary_over_a_deterministic_core() {
    let records = vec![ScopeRecord {
        key: scope("weather"),
        samples: overconfident_samples(30),
        clv_bps: vec![100.0],
    }];
    let strategies = vec![StrategyRecord {
        strategy: "synth_events".to_string(),
        kind: StrategyKindView::Synthesis,
        paper_days: 45,
        resolved_beliefs: 60,
        realized_pnl_cents: 10_000,
        fees_cents: 2_000,
        clv_mean_bps: Some(100.0),
        invariant_violations: 0,
    }];

    let mind = StubMind::scripted(vec![commentary_output(true)]);
    let review = weekly_review(
        &mind,
        &[],
        &records,
        &BTreeMap::new(),
        &strategies,
        &thresholds(),
        t("2026-06-14T00:00:00.000Z"),
    )
    .await
    .unwrap();

    assert_eq!(review.calibration.len(), 1);
    assert_eq!(review.recommendations.len(), 1);
    assert!(review
        .commentary
        .as_deref()
        .unwrap_or("")
        .contains("Weather"));
    assert_eq!(review.lesson_candidates.len(), 1);
    assert!(review.lesson_candidates[0].body.contains("NWS"));
    assert!(review.commentary_defect.is_none());
    assert!(!review.manifest_hash.is_empty());
}

#[tokio::test]
async fn weekly_review_survives_mind_failure_and_unparseable_commentary() {
    let records = vec![ScopeRecord {
        key: scope("weather"),
        samples: overconfident_samples(30),
        clv_bps: vec![],
    }];

    // Unparseable commentary: deterministic core intact, defect recorded,
    // ZERO lesson candidates (never guess lessons out of prose).
    let mind = StubMind::scripted(vec![commentary_output(false)]);
    let review = weekly_review(
        &mind,
        &[],
        &records,
        &BTreeMap::new(),
        &[],
        &thresholds(),
        t("2026-06-14T00:00:00.000Z"),
    )
    .await
    .unwrap();
    assert_eq!(review.calibration.len(), 1, "core survives");
    assert!(review.commentary.is_none());
    assert!(review.lesson_candidates.is_empty());
    assert!(review.commentary_defect.is_some());

    // Mind exhausted (empty decision = no journal): same degrade.
    let mind = StubMind::scripted(vec![]);
    let review = weekly_review(
        &mind,
        &[],
        &records,
        &BTreeMap::new(),
        &[],
        &thresholds(),
        t("2026-06-14T00:00:00.000Z"),
    )
    .await
    .unwrap();
    assert_eq!(review.calibration.len(), 1);
    assert!(review.commentary_defect.is_some());
}

// ---------------------------------------------------------- monthly review

#[test]
fn monthly_review_allocates_conservatively_and_decays_lessons() {
    let strategies = vec![
        AllocationInput {
            strategy: "mech_structural".to_string(),
            envelope_cents: 300_000,
            realized_pnl_cents: 50_000,
            fees_cents: 10_000,
            cognition_cost_cents: 0,
        },
        AllocationInput {
            strategy: "synth_events".to_string(),
            envelope_cents: 200_000,
            realized_pnl_cents: -30_000,
            fees_cents: 8_000,
            cognition_cost_cents: 12_000,
        },
    ];
    let lessons = vec![
        LessonStatusView {
            lesson_id: "l-1".to_string(),
            review_at: t("2026-06-20T00:00:00.000Z"), // future: keeps
        },
        LessonStatusView {
            lesson_id: "l-2".to_string(),
            review_at: t("2026-06-10T00:00:00.000Z"), // due: demote
        },
    ];

    let review = monthly_review(&strategies, &lessons, t("2026-06-15T00:00:00.000Z"));

    // Net-negative strategy: recommend shrink; never an increase that
    // invents capital (sum of recommendations <= sum of envelopes).
    let alloc = &review.allocations;
    assert_eq!(alloc.len(), 2);
    assert_eq!(alloc[0].strategy, "mech_structural");
    assert!(alloc[0].recommended_envelope_cents >= alloc[0].current_envelope_cents);
    assert_eq!(alloc[1].strategy, "synth_events");
    assert!(
        alloc[1].recommended_envelope_cents < alloc[1].current_envelope_cents,
        "net-negative (pnl - fees - cognition < 0) shrinks"
    );
    let current_total: i64 = alloc.iter().map(|a| a.current_envelope_cents).sum();
    let recommended_total: i64 = alloc.iter().map(|a| a.recommended_envelope_cents).sum();
    assert!(
        recommended_total <= current_total,
        "recommendations never invent capital"
    );
    assert!(
        !alloc[1].rationale.is_empty(),
        "every recommendation explains itself"
    );

    // Cost-of-cognition audit totals.
    assert_eq!(review.cost_audit.total_cognition_cost_cents, 12_000);
    assert_eq!(review.cost_audit.total_realized_pnl_cents, 20_000);
    assert_eq!(review.cost_audit.total_fees_cents, 18_000);

    // Lesson decay: due lessons demoted, future ones kept.
    assert_eq!(review.lessons_due_demotion, vec!["l-2".to_string()]);

    // Operator checklist: reminders only (kill-switch test, backup drill).
    assert!(review
        .operator_checklist
        .iter()
        .any(|i| i.contains("kill-switch")));
    assert!(review
        .operator_checklist
        .iter()
        .any(|i| i.contains("backup")));
}
