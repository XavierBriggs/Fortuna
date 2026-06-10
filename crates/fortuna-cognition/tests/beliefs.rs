//! T2.3: belief ledger logic (spec 5.5) — validation, supersession,
//! Brier scoring, abandonment, freshness policy, calibration curves.
//!
//! Doctrine under test:
//! - Probabilities are STRICTLY inside (0,1): a model claiming certainty
//!   is schema-invalid output, rejected, never repaired silently (5.9).
//! - Beliefs are immutable; an update is a NEW belief superseding the
//!   old; abandonment (event died) excludes the belief from calibration
//!   ENTIRELY — scored neither right nor wrong.
//! - Brier = (p - outcome)^2, scored exactly once on resolution.
//! - Freshness: category max age; a relevant signal arriving after
//!   creation forces refresh; the pre-benchmark window tightens the
//!   cadence. Stale beliefs are EXCLUDED from the comparator (the
//!   caller's duty; here we pin the verdict).
//! - Calibration curves bucket resolved beliefs only and report
//!   (mean predicted p, observed frequency, n) per bucket.
//!
//! Written BEFORE src/beliefs.rs per the repository TDD doctrine.

use fortuna_cognition::beliefs::{
    brier_score, calibration_curve, BeliefDraft, BeliefStatus, Freshness, FreshnessPolicy,
};
use fortuna_core::clock::UtcTimestamp;

fn t(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(1_790_000_000_000 + ms).unwrap()
}

fn draft(p: f64) -> BeliefDraft {
    BeliefDraft {
        event_id: "evt-1".to_string(),
        p,
        p_raw: p,
        horizon: t(86_400_000),
        evidence: serde_json::json!([{"source": "aeolus", "ref": "run-1", "weight_note": "fresh"}]),
        provenance: serde_json::json!({"model_id": "stub", "prompt_hash": "h", "context_manifest_hash": "m", "cost_cents": 0}),
    }
}

// ------------------------------------------------------------ validation

#[test]
fn probabilities_strictly_inside_unit_interval() {
    assert!(draft(0.5).validate().is_ok());
    assert!(draft(0.001).validate().is_ok());
    assert!(
        draft(0.0).validate().is_err(),
        "certainty is not a probability"
    );
    assert!(draft(1.0).validate().is_err());
    assert!(draft(-0.1).validate().is_err());
    assert!(draft(1.7).validate().is_err());
    assert!(draft(f64::NAN).validate().is_err());

    let mut d = draft(0.5);
    d.p_raw = 1.2;
    assert!(d.validate().is_err(), "p_raw is validated too");
}

// ----------------------------------------------------------------- brier

#[test]
fn brier_is_squared_error_against_outcome() {
    assert!((brier_score(0.7, true) - 0.09).abs() < 1e-12);
    assert!((brier_score(0.7, false) - 0.49).abs() < 1e-12);
    assert!((brier_score(0.5, true) - 0.25).abs() < 1e-12);
}

// ------------------------------------------------------------- freshness

fn policy() -> FreshnessPolicy {
    let mut p = FreshnessPolicy::new(3_600_000, 7_200_000, 300_000);
    p.set_category_max_age("weather", 1_800_000); // 30m for weather
    p
}

#[test]
fn freshness_age_rules_per_category() {
    let p = policy();
    let created = t(0);
    let benchmark = t(100 * 3_600_000); // far away: window inactive

    // Default category: 1h max age.
    assert_eq!(
        p.assess("politics", created, benchmark, t(3_000_000), None),
        Freshness::Fresh
    );
    assert!(matches!(
        p.assess("politics", created, benchmark, t(3_700_000), None),
        Freshness::Stale { .. }
    ));
    // Weather override: 30m.
    assert!(matches!(
        p.assess("weather", created, benchmark, t(2_000_000), None),
        Freshness::Stale { .. }
    ));
}

#[test]
fn relevant_signal_after_creation_forces_refresh() {
    let p = policy();
    let created = t(0);
    let benchmark = t(100 * 3_600_000);
    // Young belief, but a relevant signal arrived after it was formed.
    assert!(matches!(
        p.assess("politics", created, benchmark, t(60_000), Some(t(30_000))),
        Freshness::Stale { .. }
    ));
    // A signal from BEFORE the belief was formed is already incorporated.
    assert_eq!(
        p.assess("politics", created, benchmark, t(60_000), Some(t(-1_000))),
        Freshness::Fresh
    );
}

#[test]
fn pre_benchmark_window_tightens_the_cadence() {
    let p = policy();
    let benchmark = t(10_000_000);
    // Inside the 2h pre-benchmark window the max age is 5m, overriding
    // the category's 1h.
    let created = t(9_000_000 - 400_000); // 400s before "now"
    let now = t(9_000_000);
    assert!(matches!(
        p.assess("politics", created, benchmark, now, None),
        Freshness::Stale { .. }
    ));
    // The same age OUTSIDE the window is fresh.
    let created = t(0);
    let now = t(400_000);
    assert_eq!(
        p.assess("politics", created, t(100 * 3_600_000), now, None),
        Freshness::Fresh
    );
}

// -------------------------------------------------------------- statuses

#[test]
fn status_vocabulary_matches_the_schema() {
    for (s, name) in [
        (BeliefStatus::Open, "open"),
        (BeliefStatus::Resolved, "resolved"),
        (BeliefStatus::Superseded, "superseded"),
        (BeliefStatus::Abandoned, "abandoned"),
    ] {
        assert_eq!(s.as_str(), name);
    }
}

// ------------------------------------------------------------ calibration

#[test]
fn calibration_curve_buckets_resolved_beliefs() {
    // 10 beliefs at p~0.8 of which 7 hit; 5 at p~0.2 of which 1 hits.
    let mut samples: Vec<(f64, bool)> = Vec::new();
    for i in 0..10 {
        samples.push((0.81 + (i as f64) * 0.004, i < 7));
    }
    for i in 0..5 {
        samples.push((0.18 + (i as f64) * 0.004, i < 1));
    }

    let curve = calibration_curve(&samples, 10);
    let hot = curve
        .iter()
        .find(|b| b.lo <= 0.85 && 0.85 < b.hi)
        .expect("0.8 bucket");
    assert_eq!(hot.n, 10);
    assert!((hot.observed_frequency - 0.7).abs() < 1e-9);
    assert!(hot.mean_p > 0.80 && hot.mean_p < 0.85);

    let cold = curve
        .iter()
        .find(|b| b.lo <= 0.19 && 0.19 < b.hi)
        .expect("0.2 bucket");
    assert_eq!(cold.n, 5);
    assert!((cold.observed_frequency - 0.2).abs() < 1e-9);

    // Empty buckets are omitted (no fake calibration points).
    assert!(curve.iter().all(|b| b.n > 0));
}

#[test]
fn calibration_curve_rejects_degenerate_bucket_counts() {
    assert!(calibration_curve(&[(0.5, true)], 0).is_empty());
}
