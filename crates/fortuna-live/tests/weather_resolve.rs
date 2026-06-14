//! `fortuna_live::daemon::resolve_and_score_weather_beliefs` — the weather
//! "close-the-loop" resolver (source contract §5 Layer 3). Written FROM the task
//! text BEFORE the implementation (TDD), adversarially:
//!   - the HAPPY path: due open Aeolus bracket beliefs + the scalar μ/σ belief
//!     resolve against the INDEPENDENT NWS grade (Brier of the persisted `p` /
//!     CRPS of the persisted fan), each outcome the realized `ge`/`lt` truth;
//!   - IDEMPOTENCY: a second run resolves 0 and writes no duplicate score row;
//!   - the SKIP path: a belief whose CLI product cannot be ROUTED (its grading
//!     station has no matching recorded product) stays OPEN and unscored;
//!   - the AMBIGUOUS-grade path: a jammed CLI (`nws_cli_realized` → None) leaves
//!     the belief OPEN — never a fabricated temperature.
//!
//! ## Recorded-data note (honesty)
//!
//! Both inputs are REAL recorded captures: the μ/σ probabilities come from the
//! recorded `knyc_tmax` Aeolus forecast (`fixtures/sources/aeolus/`), and the
//! realized daily high (91°F) is graded from the recorded Troutdale NWS CLI
//! product (`fixtures/sources/nws_climate/cli_product_troutdale.json`, AWIPS
//! `CLITTD`). The forecast's grading station is LABELLED `TTD` in these tests so
//! the recorded Troutdale product routes to it — the same recorded-forecast ×
//! recorded-CLI pairing the F9 e2e uses to prove the math. It does NOT assert TTD
//! is NYC's grader; the production NYC CLI (`CLINYC`) fixture is a ledgered seam
//! (GAPS). Nothing here is fabricated: the probabilities and the realized value
//! are both recorded.

use fortuna_cognition::aeolus_beliefs::emit_aeolus_beliefs;
use fortuna_cognition::aeolus_forecast::parse_response;
use fortuna_cognition::scoring::PredictiveDistribution;
use fortuna_core::clock::UtcTimestamp;
use fortuna_ledger::{BeliefScoresRepo, BeliefsRepo, EventsRepo, ScalarBeliefsRepo, SignalsRepo};
use fortuna_live::daemon::resolve_and_score_weather_beliefs;
use serde_json::json;
use sqlx::PgPool;

const KNYC_TMAX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/aeolus/knyc_tmax.json"
));
const TROUTDALE_CLI: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/nws_climate/cli_product_troutdale.json"
));
const JAMMED_CLI: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/nws_climate/cli_product.json"
));

/// The recorded Troutdale daily high (`MAXIMUM 91`) the grader extracts.
const TROUTDALE_HIGH: i64 = 91;
const TARGET_DATE: &str = "2026-06-13";
/// The forecast horizon (settles_after); `now()` is strictly after it.
const HORIZON: &str = "2026-06-14T10:00:00.000Z";

fn now() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-15T00:00:00.000Z").unwrap()
}

/// Provenance routing the belief to a CLI by `grading_station` (see the module
/// note: TTD pairs the recorded knyc forecast with the recorded Troutdale CLI).
fn provenance(grading_station: &str) -> serde_json::Value {
    json!({
        "model_id": "aeolus",
        "station": "KNYC",
        "nws_station_id": grading_station,
        "variable": "tmax",
        "target_date": TARGET_DATE,
        "run_at": "2026-06-13T00:00:00.000Z",
        "model_version": "sar-semos-v1",
    })
}

fn product_text(fixture: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(fixture).unwrap();
    v["productText"].as_str().unwrap().to_string()
}

/// Insert a recorded `nws.cli` signal carrying `productText`.
async fn insert_cli_signal(pool: &PgPool, signal_id: &str, text: &str) {
    SignalsRepo::new(pool.clone())
        .insert(
            signal_id,
            "nws_cli",
            "nws.cli",
            "2026-06-14T11:00:00.000Z",
            signal_id, // content_hash stand-in (unique per signal)
            &json!({ "productText": text }),
        )
        .await
        .unwrap();
}

/// Persist the recorded knyc forecast's beliefs (14 binary brackets + 1 scalar)
/// as OPEN+DUE, routed to `grading_station`. Returns the binary drafts (for
/// per-bracket assertions). Both belief kinds carry the same routing provenance.
async fn seed_open_beliefs(
    pool: &PgPool,
    grading_station: &str,
) -> Vec<fortuna_cognition::beliefs::BeliefDraft> {
    let fc = parse_response(KNYC_TMAX).expect("recorded forecast parses")[0].clone();
    let beliefs = emit_aeolus_beliefs(&fc);
    let prov = provenance(grading_station);
    let events = EventsRepo::new(pool.clone());
    let beliefs_repo = BeliefsRepo::new(pool.clone());

    for (i, draft) in beliefs.binary.iter().enumerate() {
        events
            .create(
                &draft.event_id,
                "aeolus weather bracket",
                "official NWS daily maximum",
                "nws_observed_high",
                Some(HORIZON),
                HORIZON,
                "weather",
                "2026-06-13T01:00:00.000Z",
            )
            .await
            .unwrap();
        beliefs_repo
            .insert(
                &format!("wx-bin-{i}"),
                "2026-06-13T01:00:00.000Z",
                &draft.event_id,
                draft.p,
                draft.p_raw,
                HORIZON,
                &draft.evidence,
                &prov,
                None,
            )
            .await
            .unwrap();
    }

    let quantiles = match &beliefs.scalar.predictive {
        PredictiveDistribution::Scalar { quantiles, .. } => {
            serde_json::to_value(quantiles).unwrap()
        }
        _ => unreachable!("F8 scalar is a Scalar predictive"),
    };
    ScalarBeliefsRepo::new(pool.clone())
        .insert(
            "wx-scalar-0",
            "aeolus",
            &beliefs.scalar.event_key,
            &quantiles,
            "degF",
            HORIZON,
            &prov,
            "2026-06-13T01:00:00.000Z",
        )
        .await
        .unwrap();

    beliefs.binary.clone()
}

// ── happy path: every bracket + the scalar resolve against the NWS grade ──────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn due_weather_beliefs_resolve_against_the_recorded_grade(pool: PgPool) {
    let drafts = seed_open_beliefs(&pool, "TTD").await;
    insert_cli_signal(&pool, "cli-ttd", &product_text(TROUTDALE_CLI)).await;

    let resolved = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .expect("resolve_and_score_weather_beliefs");
    assert_eq!(
        resolved,
        drafts.len() + 1,
        "14 brackets + 1 scalar resolved"
    );

    let beliefs_repo = BeliefsRepo::new(pool.clone());
    // Every bracket belief is now SCORED, with the realized `ge`/`lt` outcome and
    // brier = (persisted p − outcome)².
    for (i, draft) in drafts.iter().enumerate() {
        let row = beliefs_repo.get(&format!("wx-bin-{i}")).await.unwrap();
        assert_eq!(row.status, "resolved", "{} scored", draft.event_id);
        // The bracket threshold lives in the event_hint suffix (geNN); 91 ≥ N.
        let n: i64 = draft
            .event_id
            .rsplit("-ge")
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let expect_true = TROUTDALE_HIGH >= n;
        assert_eq!(
            row.outcome,
            Some(i32::from(expect_true)),
            "{}",
            draft.event_id
        );
        let brier = row.brier.unwrap();
        let outcome_f = if expect_true { 1.0 } else { 0.0 };
        assert!(
            (brier - (draft.p - outcome_f).powi(2)).abs() < 1e-12,
            "brier = (p−outcome)² for {}",
            draft.event_id
        );
    }

    // Spot-check the borderline brackets explicitly.
    let ge87 = beliefs_repo.get("wx-bin-6").await.unwrap(); // ge87
    assert_eq!(ge87.event_id, "aeolus:knyc-2026-06-13-tmax-ge87");
    assert_eq!(ge87.outcome, Some(1), "91 ≥ 87 ⇒ true");

    // The scalar μ/σ belief resolved against the SAME realized high, with a CRPS row.
    let scalar = ScalarBeliefsRepo::new(pool.clone())
        .get("wx-scalar-0")
        .await
        .unwrap();
    assert_eq!(scalar.realized_value, Some(TROUTDALE_HIGH as f64));
    let crps_rows = BeliefScoresRepo::new(pool.clone())
        .scores_for_belief("wx-scalar-0")
        .await
        .unwrap();
    assert_eq!(crps_rows.len(), 1, "one crps_pinball score row");
    assert_eq!(crps_rows[0].rule_id, "crps_pinball");
    assert!(crps_rows[0].score.is_finite() && crps_rows[0].score >= 0.0);
}

// ── idempotency: a second run resolves nothing and adds no duplicate score ────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_second_run_is_idempotent(pool: PgPool) {
    let drafts = seed_open_beliefs(&pool, "TTD").await;
    insert_cli_signal(&pool, "cli-ttd", &product_text(TROUTDALE_CLI)).await;

    let first = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .unwrap();
    assert_eq!(first, drafts.len() + 1);

    let second = resolve_and_score_weather_beliefs(&pool, now(), 1_000)
        .await
        .expect("second run must not error on already-scored rows");
    assert_eq!(second, 0, "nothing newly resolved");
    assert_eq!(
        BeliefScoresRepo::new(pool.clone())
            .scores_for_belief("wx-scalar-0")
            .await
            .unwrap()
            .len(),
        1,
        "still exactly one CRPS row — no duplicate"
    );
}

// ── skip path: a belief whose CLI product cannot be routed stays OPEN ─────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn unroutable_station_leaves_beliefs_open(pool: PgPool) {
    // Beliefs graded by "NYC", but the only recorded CLI is Troutdale (CLITTD) —
    // no CLINYC product ⇒ no route ⇒ nothing resolved (the missing-fixture seam).
    seed_open_beliefs(&pool, "NYC").await;
    insert_cli_signal(&pool, "cli-ttd", &product_text(TROUTDALE_CLI)).await;

    let resolved = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .unwrap();
    assert_eq!(resolved, 0, "no matching CLI product ⇒ nothing graded");

    let row = BeliefsRepo::new(pool.clone())
        .get("wx-bin-6")
        .await
        .unwrap();
    assert_eq!(row.status, "open", "belief stays OPEN for a later run");
    assert!(row.outcome.is_none());
}

// ── ambiguous grade: a jammed CLI leaves the belief OPEN (never fabricated) ───

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_jammed_cli_does_not_grade(pool: PgPool) {
    // The recorded PTKR product (AWIPS CLITKR) jams MINIMUM as `7676`. Route the
    // beliefs to TKR so the product is FOUND, but the grader returns None ⇒ the
    // beliefs stay OPEN, never graded against a fabricated value.
    seed_open_beliefs(&pool, "TKR").await;
    insert_cli_signal(&pool, "cli-tkr", &product_text(JAMMED_CLI)).await;

    let resolved = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .unwrap();
    assert_eq!(
        resolved, 0,
        "a jammed CLI grades to None ⇒ nothing resolved"
    );
    let row = BeliefsRepo::new(pool.clone())
        .get("wx-bin-6")
        .await
        .unwrap();
    assert_eq!(row.status, "open");
}
