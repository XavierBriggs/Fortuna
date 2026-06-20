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
use fortuna_cognition::persona::PersonaDef;
use fortuna_cognition::persona_beliefs::{belief_horizon, map_persona_analysis};
use fortuna_cognition::persona_orchestrator::fill_region_key;
use fortuna_cognition::scoring::PredictiveDistribution;
use fortuna_core::clock::UtcTimestamp;
use fortuna_ledger::{BeliefScoresRepo, BeliefsRepo, EventsRepo, ScalarBeliefsRepo, SignalsRepo};
use fortuna_live::daemon::{nws_cli_is_stale, resolve_and_score_weather_beliefs};
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
/// The recorded CLINYC (Central Park) NWS daily climate product.
/// AWIPS product id: CLINYC (CDUS41 KOKX). Realized MAXIMUM 91°F on 2026-06-13.
/// Provenance: realistic NWS-CLI-format product capturing the real NYC grading
/// station for 2026-06-13; MAXIMUM 91 is consistent with the knyc_tmax μ≈87.3
/// (91 >= 87 is TRUE for the ge87 bracket; Brier = (0.6719 − 1)² ≈ 0.1073).
const CLINYC_CLI: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/sources/nws_climate/cli_product_clinyc.json"
));
/// The SHIPPED meteorologist persona definition (persona.md).
/// Loaded at compile-time so ANY revert of the region_key template in that file
/// causes the production-path test (Test 6) to RED — the mutation proof is live,
/// not just described in a comment.
const METEOROLOGIST_PERSONA_MD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../config/personas/meteorologist/persona.md"
));
/// The companion output schema for the meteorologist persona (required by PersonaDef::parse).
const METEOROLOGIST_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../config/personas/meteorologist/schema.json"
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

// ── B2 (F3): resolution → resolved_stats feeds the B1 calibration chain ──────
//
// Proves: weather beliefs that resolve via resolve_and_score_weather_beliefs are
// immediately visible to BeliefsRepo::resolved_stats("weather"), which is
// exactly the query persist_daily_calibration (B1) reads. Without this pin a
// silent grading failure starves the model arm with no visible signal.

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn resolved_weather_beliefs_appear_in_resolved_stats(pool: PgPool) {
    // Seed open beliefs + a fresh CLI and resolve them.
    let drafts = seed_open_beliefs(&pool, "TTD").await;
    insert_cli_signal(&pool, "cli-ttd", &product_text(TROUTDALE_CLI)).await;

    let resolved = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .expect("resolver must succeed");
    assert_eq!(resolved, drafts.len() + 1, "all brackets + scalar resolved");

    // resolved_stats("weather") must now see those resolved rows — this is the
    // exact query that B1's persist_daily_calibration reads to count the warm
    // record. If resolution doesn't write `status='resolved'` + `brier` in the
    // right category the model arm starves invisibly.
    let stats = BeliefsRepo::new(pool.clone())
        .resolved_stats("weather")
        .await
        .expect("resolved_stats must not error");
    assert_eq!(
        stats.len(),
        drafts.len(),
        "resolved_stats sees every resolved bracket (scalar is in scalar_beliefs, not here)"
    );
    for stat in &stats {
        assert!(stat.brier.is_finite() && stat.brier >= 0.0, "brier ≥ 0");
        assert!(stat.brier <= 1.0, "Brier bounded [0,1]");
    }
}

// ── B2 (F3): nws_cli_is_stale unit tests (mutation-proof) ────────────────────
//
// The pure helper `nws_cli_is_stale(freshest, now, max_secs)` is the
// detection kernel for the daily-boundary ops alert. These tests pin its
// contract so that flipping the comparison or dropping the check turns at
// least one red.

#[test]
fn stale_when_cli_absent() {
    // No signal at all → always stale regardless of threshold.
    assert!(
        nws_cli_is_stale(None, now(), 36 * 3600),
        "absent CLI must be flagged stale"
    );
}

#[test]
fn stale_when_cli_too_old() {
    // CLI received 48h before `now` (well past the 36h threshold).
    let fresh_ts = "2026-06-13T00:00:00.000Z"; // now() = 2026-06-15T00:00:00Z → 48h gap
    assert!(
        nws_cli_is_stale(Some(fresh_ts), now(), 36 * 3600),
        "48h-old CLI must be flagged stale (threshold is 36h)"
    );
}

#[test]
fn not_stale_when_cli_fresh() {
    // CLI received 12h before `now` — within the 36h window.
    let fresh_ts = "2026-06-14T12:00:00.000Z"; // now() = 2026-06-15T00:00:00Z → 12h gap
    assert!(
        !nws_cli_is_stale(Some(fresh_ts), now(), 36 * 3600),
        "12h-old CLI must NOT be flagged stale (threshold is 36h)"
    );
}

#[test]
fn stale_exactly_at_threshold_boundary() {
    // Staleness uses STRICTLY-GREATER: `age_secs > max_secs`. So at exactly the
    // threshold (age == max_secs) the CLI is NOT yet stale; one second past it IS.
    // now() = 2026-06-15T00:00:00.000Z, threshold 36h = 129600s.
    // Exactly 36h before now = 2026-06-13T12:00:00.000Z (age == threshold → NOT stale).
    let exactly_at_boundary = "2026-06-13T12:00:00.000Z";
    assert!(
        !nws_cli_is_stale(Some(exactly_at_boundary), now(), 36 * 3600),
        "CLI exactly at the 36h boundary is NOT yet stale (age == threshold, not exceeding it)"
    );
    let one_second_over = "2026-06-13T11:59:59.000Z"; // 36h+1s old
    assert!(
        nws_cli_is_stale(Some(one_second_over), now(), 36 * 3600),
        "CLI 1s past the 36h boundary IS stale"
    );
}

// ── Test 5 (WS1 Task 3): meteorologist belief is RESOLVED (not skipped) ──────
//
// Before this slice the daemon.rs:4723 `strip_prefix("aeolus:")` line SKIPPED any
// belief whose event_id lacked the "aeolus:" prefix — specifically the meteorologist
// persona's `weather:KNYC:tmax:DATE#ge87` format. This test seeds ONE meteorologist
// belief and ONE Aeolus belief for the SAME bracket + grading station, runs the
// resolver, and asserts BOTH are scored.
//
// Mutation proof: reverting daemon.rs:4723 to `strip_prefix("aeolus:")` leaves the
// meteorologist belief OPEN (resolved count drops to 1) → RED.

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn meteorologist_belief_is_resolved_not_skipped(pool: PgPool) {
    // Shared setup: the Troutdale CLI (CLITTD, realized high 91°F) is the grader.
    insert_cli_signal(&pool, "cli-ttd-meteo", &product_text(TROUTDALE_CLI)).await;

    // --- Seed an Aeolus binary belief (event_id = "aeolus:knyc-2026-06-13-tmax-ge87") ---
    // p = 0.6719… (the recorded knyc forecast's ge87 probability).
    let p_ge87 = 0.6719055375922601_f64;
    let prov_aeolus = json!({
        "model_id": "aeolus",
        "station": "KNYC",
        "nws_station_id": "TTD",   // routes to Troutdale (CLITTD)
        "variable": "tmax",
        "target_date": TARGET_DATE,
        "run_at": "2026-06-13T00:00:00.000Z",
        "model_version": "sar-semos-v1",
    });
    EventsRepo::new(pool.clone())
        .create(
            "aeolus:knyc-2026-06-13-tmax-ge87",
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
    BeliefsRepo::new(pool.clone())
        .insert(
            "ws13-aeolus-ge87",
            "2026-06-13T01:00:00.000Z",
            "aeolus:knyc-2026-06-13-tmax-ge87",
            p_ge87,
            p_ge87,
            HORIZON,
            &json!([{"source": "aeolus", "ref": "sig-knyc"}]),
            &prov_aeolus,
            None,
        )
        .await
        .unwrap();

    // --- Seed a meteorologist belief for the SAME bracket ---
    // event_id uses the persona grammar: `weather:KNYC:tmax:DATE#ge87`
    // The provenance carries the same grading keys so open_weather_bracket_due picks it up.
    let prov_meteo = json!({
        "producer": "meteorologist",
        "persona_id": "meteorologist",
        "persona_version": 1,
        "analysis_id": "01METEOANALYSIS",
        "analysis_content_hash": "ch-meteo-1",
        "nws_station_id": "TTD",   // same grading station
        "variable": "tmax",
        "target_date": TARGET_DATE,
    });
    // The persona grammar event_id: `{region_key}#{bracket_token}`.
    // region_key = "weather:KNYC:tmax:2026-06-13", bracket = "ge87".
    let meteo_event_id = "weather:KNYC:tmax:2026-06-13#ge87";
    EventsRepo::new(pool.clone())
        .create(
            meteo_event_id,
            "meteorologist weather bracket",
            "official NWS daily maximum",
            "nws_observed_high",
            Some(HORIZON),
            HORIZON,
            "weather",
            "2026-06-13T01:30:00.000Z",
        )
        .await
        .unwrap();
    BeliefsRepo::new(pool.clone())
        .insert(
            "ws13-meteo-ge87",
            "2026-06-13T01:30:00.000Z",
            meteo_event_id,
            p_ge87, // same p — both should score identically
            p_ge87,
            HORIZON,
            &json!([{"source": "persona:meteorologist@1", "ref": "01METEOANALYSIS"}]),
            &prov_meteo,
            None,
        )
        .await
        .unwrap();

    // --- Run the resolver ---
    let resolved = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .expect("resolve_and_score_weather_beliefs");

    // BOTH beliefs must be resolved. Before this slice, only 1 resolved (Aeolus)
    // and the meteorologist was silently skipped.
    assert_eq!(
        resolved, 2,
        "BOTH Aeolus + meteorologist bracket beliefs must be resolved; \
         if this is 1 the strip_prefix(\"aeolus:\") skip bug is back"
    );

    let beliefs_repo = BeliefsRepo::new(pool.clone());

    // Aeolus belief: resolved, outcome=true (91 >= 87).
    let aeolus_row = beliefs_repo.get("ws13-aeolus-ge87").await.unwrap();
    assert_eq!(aeolus_row.status, "resolved", "Aeolus belief resolved");
    assert_eq!(aeolus_row.outcome, Some(1), "91 >= 87 => true");

    // Meteorologist belief: also resolved, same outcome + brier.
    let meteo_row = beliefs_repo.get("ws13-meteo-ge87").await.unwrap();
    assert_eq!(
        meteo_row.status, "resolved",
        "meteorologist belief must be resolved — this is the thesis of the slice"
    );
    assert_eq!(meteo_row.outcome, Some(1), "91 >= 87 => true");

    // Both briors must be identical (same p, same outcome).
    let aeolus_brier = aeolus_row.brier.expect("Aeolus brier set");
    let meteo_brier = meteo_row.brier.expect("meteorologist brier set");
    assert!(
        (aeolus_brier - meteo_brier).abs() < 1e-12,
        "brier must be identical for same p+outcome: aeolus={aeolus_brier}, meteo={meteo_brier}"
    );
}

// ── Test 6 (WS1 Critical boundary fix): production-path authoring → CLINYC grader ─
//
// This is the MISSING cross-slice coverage that allowed the grading-station mismatch
// to hide in production. Previous tests hand-set `nws_station_id` directly in the
// belief provenance (bypassing the real authoring path). This test drives the
// PRODUCTION AUTHORING PATH end-to-end:
//
//   1. Real `aeolus.forecast` signal (knyc_tmax.json) — station=KNYC, nws_station_id=NYC
//   2. fill_region_key with the SHIPPED persona.md template
//      ("weather:{nws_station_id}:tmax:{target_date}") → "weather:NYC:tmax:2026-06-13"
//   3. map_persona_analysis on that region_key → belief with nws_station_id="NYC" in provenance
//   4. resolve_and_score_weather_beliefs + the real CLINYC fixture (MAXIMUM 91)
//   5. Both the Aeolus belief AND the meteorologist belief RESOLVE + Brier-score
//      (the head-to-head both produce scores)
//
// MUTATION PROOF: reverting the persona.md region_key template to
// "weather:{station}:tmax:{target_date}" (old station field) causes fill_region_key
// to produce "weather:KNYC:tmax:2026-06-13" → map_persona_analysis stamps
// nws_station_id="KNYC" → the resolver looks for CLIKNYC (no match in CLINYC) →
// the meteorologist belief stays OPEN → resolved count drops from 2 to 1 (the Aeolus
// belief still scores because it carries nws_station_id="NYC" from its own provenance).
// If the template is reverted to "weather:{station}:tmax:{date}" (missing {date} field),
// fill_region_key returns None (the real payload has target_date, not date) → no
// meteorologist belief is created → 0 meteorologist beliefs resolved → count stays 1.
// Either mutation → RED.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn production_authoring_path_meteorologist_resolves_against_clinyc(pool: PgPool) {
    // Load the real Aeolus knyc forecast and the real CLINYC CLI product.
    let payload: serde_json::Value = {
        let root: serde_json::Value = serde_json::from_str(KNYC_TMAX).expect("knyc_tmax parses");
        root["forecasts"][0].clone()
    };
    // Verify the fixture carries the production fields (honesty check).
    assert_eq!(
        payload["station"].as_str().unwrap_or(""),
        "KNYC",
        "fixture station field"
    );
    assert_eq!(
        payload["nws_station_id"].as_str().unwrap_or(""),
        "NYC",
        "fixture nws_station_id field"
    );
    assert_eq!(
        payload["target_date"].as_str().unwrap_or(""),
        TARGET_DATE,
        "fixture target_date field"
    );

    // 1. Drive the PRODUCTION region_key derivation (the shipped persona.md template).
    //    We load the template from the ACTUAL FILE (METEOROLOGIST_PERSONA_MD, baked in
    //    at compile-time). If the template in that file is wrong (e.g. reverted to
    //    {station} or {date}), fill_region_key returns None or "weather:KNYC:..." →
    //    the assertion below REDs. This is the LIVE mutation proof: reverting persona.md
    //    breaks this test, not just the comment.
    let persona_def = PersonaDef::parse(METEOROLOGIST_PERSONA_MD, METEOROLOGIST_SCHEMA)
        .expect("shipped meteorologist persona.md must parse");
    assert_eq!(
        persona_def.meta.region_key, "weather:{nws_station_id}:tmax:{target_date}",
        "MUTATION PROOF (persona.md): the shipped region_key template must use \
         {{nws_station_id}} (NWS-CLI grading station) and {{target_date}} (real field name). \
         Reverting to {{station}} or {{date}} here causes this assertion to RED."
    );
    let region_key = fill_region_key(&persona_def.meta.region_key, &payload)
        .expect("real Aeolus payload must fill the shipped template");
    assert_eq!(
        region_key, "weather:NYC:tmax:2026-06-13",
        "production region_key must use the NWS-CLI grading station (NYC), \
         not the forecast station (KNYC)"
    );

    // 2. Drive map_persona_analysis on the production region_key → belief provenance
    //    must carry nws_station_id="NYC" (the resolver routes via this field).
    let horizon = belief_horizon(&region_key).expect("date-bearing region_key has a horizon");
    let findings = json!({
        "thresholds": [{"ge": 87, "p": 0.6719055375922601}],
        "sigma_trend": "steady",
        "confidence": "high",
        "regime": "stagnant upper ridge",
        "key_risk": "none significant"
    });
    let drafts = map_persona_analysis(
        "meteorologist",
        3,
        "01PRODPATHANALYSIS00000001",
        "prod-path-content-hash",
        &region_key,
        &findings,
        horizon,
    )
    .expect("map_persona_analysis must succeed on the production region_key");
    assert_eq!(drafts.len(), 1, "one threshold → one belief draft");
    let meteo_draft = &drafts[0];
    // Verify the grading station in provenance — THE CORE OF THE BUG FIX.
    // Mutation proof: reverting region_key template to {station} → provenance
    // nws_station_id becomes "KNYC" → this assertion REDs.
    assert_eq!(
        meteo_draft.provenance["nws_station_id"]
            .as_str()
            .unwrap_or(""),
        "NYC",
        "MUTATION PROOF: production-path meteorologist belief must carry \
         nws_station_id='NYC' (the NWS-CLI grading station), not 'KNYC' \
         (the forecast station). If this is 'KNYC', the region_key template \
         is wrong — revert to Option A fix."
    );
    let meteo_event_id = &meteo_draft.event_id;
    assert_eq!(
        meteo_event_id, "weather:NYC:tmax:2026-06-13#ge87",
        "production event_id must use NYC grading station"
    );

    // 3. Seed the CLINYC CLI signal (the real NYC grader, MAXIMUM 91°F).
    insert_cli_signal(&pool, "cli-nyc-prod", &product_text(CLINYC_CLI)).await;

    // 4. Seed the Aeolus belief for the SAME bracket (for the head-to-head).
    let p_ge87 = 0.6719055375922601_f64;
    let prov_aeolus = json!({
        "model_id": "aeolus",
        "station": "KNYC",
        "nws_station_id": "NYC",
        "variable": "tmax",
        "target_date": TARGET_DATE,
        "run_at": "2026-06-13T00:00:00.000Z",
        "model_version": "sar-semos-v1",
    });
    EventsRepo::new(pool.clone())
        .create(
            "aeolus:knyc-2026-06-13-tmax-ge87",
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
    BeliefsRepo::new(pool.clone())
        .insert(
            "prod-path-aeolus-ge87",
            "2026-06-13T01:00:00.000Z",
            "aeolus:knyc-2026-06-13-tmax-ge87",
            p_ge87,
            p_ge87,
            HORIZON,
            &json!([{"source": "aeolus", "ref": "knyc-tmax"}]),
            &prov_aeolus,
            None,
        )
        .await
        .unwrap();

    // 5. Seed the meteorologist belief (from the production-path draft above).
    EventsRepo::new(pool.clone())
        .create(
            meteo_event_id,
            "meteorologist weather bracket",
            "official NWS daily maximum",
            "nws_observed_high",
            Some(HORIZON),
            HORIZON,
            "weather",
            "2026-06-13T01:30:00.000Z",
        )
        .await
        .unwrap();
    BeliefsRepo::new(pool.clone())
        .insert(
            "prod-path-meteo-ge87",
            "2026-06-13T01:30:00.000Z",
            meteo_event_id,
            meteo_draft.p,
            meteo_draft.p_raw,
            HORIZON,
            &meteo_draft.evidence,
            &meteo_draft.provenance,
            None,
        )
        .await
        .unwrap();

    // 6. Run the resolver. BOTH beliefs must resolve.
    let resolved = resolve_and_score_weather_beliefs(&pool, now(), 0)
        .await
        .expect("resolve_and_score_weather_beliefs");
    assert_eq!(
        resolved, 2,
        "BOTH Aeolus AND meteorologist beliefs must resolve against CLINYC; \
         if this is 1, the meteorologist's nws_station_id is wrong (bug is back). \
         If 0, the CLINYC fixture is not being found."
    );

    let beliefs_repo = BeliefsRepo::new(pool.clone());

    // Aeolus belief: resolved, outcome=true (91 >= 87).
    let aeolus_row = beliefs_repo.get("prod-path-aeolus-ge87").await.unwrap();
    assert_eq!(aeolus_row.status, "resolved", "Aeolus belief resolved");
    assert_eq!(aeolus_row.outcome, Some(1), "91 >= 87 → true");

    // Meteorologist belief: also resolved, same outcome.
    let meteo_row = beliefs_repo.get("prod-path-meteo-ge87").await.unwrap();
    assert_eq!(
        meteo_row.status, "resolved",
        "METEOROLOGIST BELIEF MUST RESOLVE — this is the thesis of WS1 and the \
         exact bug this fix addresses. If 'open', nws_station_id='KNYC' is still \
         stamped (template reverted) and the resolver hunts CLIKNYC not CLINYC."
    );
    assert_eq!(meteo_row.outcome, Some(1), "91 >= 87 → true");

    // Both Briers must be set and equal (same p, same realized high, same bracket).
    let aeolus_brier = aeolus_row
        .brier
        .expect("Aeolus belief brier must be set at resolution");
    let meteo_brier = meteo_row
        .brier
        .expect("meteorologist belief brier must be set at resolution");
    assert!(
        (aeolus_brier - meteo_brier).abs() < 1e-12,
        "both producers use same p+outcome → identical Brier; \
         aeolus={aeolus_brier}, meteo={meteo_brier}"
    );
}
