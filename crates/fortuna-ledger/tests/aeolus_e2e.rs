//! F5–F9 end-to-end (the assignment gate): a RECORDED Aeolus forecast drives the
//! whole weather pipeline to a PERSISTED, SCORED bracket belief.
//!
//! recorded fixture → F6 strict parse + μ/σ→p → F5 dedup → F7 world-forward match
//! → F8 propose-only beliefs (binary brackets + scalar fan) PERSIST → F9 Brier/CRPS
//! vs a RECORDED realized temperature → persist the scores. The chain is asserted
//! to produce a scored bracket belief (a pipeline that parses but never scores a
//! bracket belief is NOT done) and the belief's p is re-validated against the
//! pinned μ/σ math (calibration validated, not asserted).
//!
//! The realized temperature stands in for the NWS-CLI grader (F2, a ledgered seam);
//! the value used here is a chosen daily high — the math is what is proven.

use fortuna_cognition::aeolus_beliefs::emit_aeolus_beliefs;
use fortuna_cognition::aeolus_dedup::dedup_forecasts;
use fortuna_cognition::aeolus_forecast::{bracket_prob_ge, parse_response};
use fortuna_cognition::aeolus_match::match_forecast;
use fortuna_cognition::aeolus_reliability::score_reliability;
use fortuna_cognition::scoring::PredictiveDistribution;
use fortuna_ledger::{BeliefScoresRepo, BeliefsRepo, EventsRepo, ScalarBeliefsRepo};
use sqlx::PgPool;
use std::collections::BTreeMap;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
));

#[sqlx::test(migrations = "./migrations")]
async fn recorded_forecast_drives_a_persisted_scored_bracket_belief(pool: PgPool) {
    // ---- F6 parse + F5 dedup (a duplicate run collapses to one). --------------
    let fc = parse_response(FIXTURE).expect("recorded fixture parses")[0].clone();
    let deduped = dedup_forecasts(vec![fc.clone(), fc.clone()]);
    assert_eq!(deduped.len(), 1, "F5 collapses the duplicate run");
    let fc = &deduped[0];

    // ---- F7 world-forward match. ----------------------------------------------
    let family = match_forecast(fc);
    assert_eq!(family.events.len(), 14);
    assert_eq!(family.nws_station_id, "NYC"); // grading station, distinct from KNYC

    // ---- F8 propose-only beliefs. ---------------------------------------------
    let beliefs = emit_aeolus_beliefs(fc);
    assert_eq!(beliefs.binary.len(), 14);
    assert_eq!(beliefs.skipped_in_bracket, 0);

    // ---- F9 score vs a RECORDED realized daily high. --------------------------
    let realized = 88.0_f64;
    let reliability = score_reliability(fc, realized);
    // event_id -> (outcome, brier) for resolving the persisted beliefs.
    let scored: BTreeMap<&str, (bool, f64)> = reliability
        .per_bracket
        .iter()
        .map(|b| (b.event_id.as_str(), (b.outcome, b.brier)))
        .collect();

    // ---- Persist the binary bracket beliefs, then resolve+score each. ---------
    let events = EventsRepo::new(pool.clone());
    let beliefs_repo = BeliefsRepo::new(pool.clone());
    let horizon = fc.resolution().settles_after.to_iso8601();
    let created = "2026-06-13T01:00:00.000Z";

    for (i, draft) in beliefs.binary.iter().enumerate() {
        // The event the belief resolves against (declares its NWS grader).
        events
            .create(
                &draft.event_id,
                "aeolus weather bracket",
                "official NWS daily maximum",
                "nws_observed_high",
                Some(&horizon),
                &horizon,
                "weather",
                created,
            )
            .await
            .unwrap();
        let belief_id = format!("aeolus-bin-{i}");
        beliefs_repo
            .insert(
                &belief_id,
                created,
                &draft.event_id,
                draft.p,
                draft.p_raw,
                &horizon,
                &draft.evidence,
                &draft.provenance,
                None,
            )
            .await
            .unwrap();
        let (outcome, brier) = scored[draft.event_id.as_str()];
        beliefs_repo
            .resolve_and_score(&belief_id, outcome, brier, None)
            .await
            .unwrap();
    }

    // ---- Persist the scalar μ/σ belief + its CRPS score. ----------------------
    let quantiles = match &beliefs.scalar.predictive {
        PredictiveDistribution::Scalar { quantiles, .. } => {
            serde_json::to_value(quantiles).unwrap()
        }
        _ => panic!("F8 scalar must be a Scalar predictive"),
    };
    ScalarBeliefsRepo::new(pool.clone())
        .insert(
            "aeolus-scalar-1",
            "aeolus",
            &beliefs.scalar.event_key,
            &quantiles,
            "degF",
            &horizon,
            &beliefs.scalar.provenance,
            created,
        )
        .await
        .unwrap();
    let crps = reliability.crps.expect("F9 produced a CRPS");
    BeliefScoresRepo::new(pool.clone())
        .insert(
            "aeolus-crps-1",
            "aeolus-scalar-1",
            "crps_pinball",
            crps,
            created,
        )
        .await
        .unwrap();

    // ---- THE GATE: a scored bracket belief exists, replaying to the μ/σ math. --
    // ge87 is the borderline bracket: p≈0.672, outcome true (88≥87).
    let ge87_event = "aeolus:knyc-2026-06-13-tmax-ge87";
    let idx = beliefs
        .binary
        .iter()
        .position(|d| d.event_id == ge87_event)
        .expect("ge87 belief emitted");
    let row = beliefs_repo
        .get(&format!("aeolus-bin-{idx}"))
        .await
        .unwrap();
    assert_eq!(
        row.status, "resolved",
        "the bracket belief is SCORED, not merely parsed"
    );
    assert_eq!(row.outcome, Some(1), "88 ≥ 87 ⇒ outcome true");
    // Calibration VALIDATED, not asserted: the persisted p is exactly the pinned
    // μ/σ probability (the same number F6's fixture test pinned to 6.9e-8 of Aeolus).
    let expected_p = bracket_prob_ge(87, fc.mu(), fc.sigma()).unwrap();
    assert!(
        (row.p - expected_p).abs() < 1e-12,
        "persisted p == μ/σ math"
    );
    let brier = row.brier.expect("brier persisted");
    assert!(
        (brier - (expected_p - 1.0).powi(2)).abs() < 1e-9,
        "brier = (p−1)²"
    );

    // The scalar CRPS landed under its rule (the §9.1 / Layer-3 scorecard feed).
    let crps_scores = BeliefScoresRepo::new(pool.clone())
        .scores_for_rule("crps_pinball", 10)
        .await
        .unwrap();
    assert_eq!(crps_scores.len(), 1);
    assert!((crps_scores[0].score - crps).abs() < 1e-12);

    // The whole family scored: 14 brier-scored bracket beliefs + one CRPS.
    assert_eq!(reliability.n_brackets, 14);
}
