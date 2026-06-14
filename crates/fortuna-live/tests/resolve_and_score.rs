//! A2d SLICE 3 part 3 tests: the scalar-belief resolve -> score loop
//! (`fortuna_live::daemon::resolve_and_score_funding_beliefs`; design
//! docs/design/perp-strategies-and-scalar-claims.md §2.6 A2d + §9.1).
//!
//! Written FROM the prompt/spec text BEFORE the implementation (TDD). They cover,
//! adversarially:
//!   - the HAPPY path: a due, captured `funding_forecast` belief resolves
//!     (realized_value set once) and writes the FIVE belief_scores legs
//!     (crps_pinball + the four A2d baselines), every score finite and ≥ 0;
//!   - IDEMPOTENCY: a SECOND run resolves 0 (the belief is no longer unresolved)
//!     and does NOT crash on the existing belief_scores rows (no dup-key panic);
//!   - the SKIP path: a belief whose realized rate is NOT captured stays
//!     unresolved AND unscored (left for a later run once the poller backfills);
//!   - the DUE gate: a belief whose window has NOT closed (`horizon > now`) is
//!     not touched;
//!   - the prior-window anchor: a captured PRIOR window (`funding_time − 8h`)
//!     feeds the last-rate / persistence-RW legs (vs the missing-prior fallback).
//!
//! Fixture-grounded: the realized rates are the REAL public capture at
//! docs/research/venue/kinetics-perps-2026-06-10/raw/live_prod_funding_hist_all.json
//! (KXBCHPERP @ 2026-06-11T04:00:00Z = -0.000_397_137_868_728_9; the prior 8h
//! window @ 2026-06-10T20:00:00Z = -0.000_179_146_600_442_7).
//!
//! ## Mutation-check note (for a reviewer)
//!
//! The two halves of the loop are pinned independently so a mutant cannot pass
//! vacuously: NEUTRALIZE the `beliefs.resolve(...)` call and
//! `belief_is_resolved` fails (realized_value stays NULL) — AND, because the
//! resolve is the gate the second run relies on, the idempotency assertion
//! (`count == 0` on rerun) also fails (the belief would re-resolve). DELETE any
//! of the five score legs and `score_count == 5` fails. SWAP `realized` for the
//! prior-window rate in the resolve and `realized_value == REALIZED` fails. So
//! every assertion has teeth.

use fortuna_ledger::{BeliefScoresRepo, FundingRatesHistoricalRepo, ScalarBeliefsRepo};
use fortuna_live::daemon::resolve_and_score_funding_beliefs;
use serde_json::json;
use sqlx::PgPool;

// ── real fixture values (KXBCHPERP) ──────────────────────────────────────────
const MARKET: &str = "KXBCHPERP";
/// The window the forecast resolves at (its `horizon` / `next_funding_time`).
const FUNDING_TIME: &str = "2026-06-11T04:00:00Z";
/// The realized funding rate finalized at FUNDING_TIME (public capture).
const REALIZED: f64 = -0.000_397_137_868_728_9;
const MARK: &str = "2.0115";
/// The PRIOR 8h window (`FUNDING_TIME − 8h`) and its realized rate.
const PRIOR_FUNDING_TIME: &str = "2026-06-10T20:00:00Z";
const PRIOR_REALIZED: f64 = -0.000_179_146_600_442_7;
const PRIOR_MARK: &str = "1.9540";

/// The standard-normal multipliers the funding_forecast producer uses for the
/// fixed §2.6 A2b seven-quantile set (so the test fan is byte-shaped like a real
/// belief, and the loop's `rw_band = (v@0.90 − v@0.50)/1.282` recovery is exact).
const FUNDING_QS: [(f64, f64); 7] = [
    (0.05, -1.645),
    (0.10, -1.282),
    (0.25, -0.674),
    (0.50, 0.0),
    (0.75, 0.674),
    (0.90, 1.282),
    (0.95, 1.645),
];

/// Build the producer's `v(q) = center + Zq·band` fan as the quantiles JSONB
/// (`[{"q":..,"v":..},..]`) the belief row stores. `band ≥ 0` ⇒ non-crossing ⇒
/// `validate_scalar`-clean, exactly like the live producer's `build_quantiles`.
fn fan_json(center: f64, band: f64) -> serde_json::Value {
    let qs: Vec<serde_json::Value> = FUNDING_QS
        .iter()
        .map(|&(q, z)| json!({"q": q, "v": center + z * band}))
        .collect();
    serde_json::Value::Array(qs)
}

/// The canonical millisecond ISO form the loop queries the realized store with
/// (`UtcTimestamp::to_iso8601()` round-trips the venue's `…Z` to `….000Z`); the
/// store and the loop AGREE on this form, so the test inserts realized rows under
/// it. Mirrors the production poller's normalization.
fn canon(iso: &str) -> String {
    fortuna_core::clock::UtcTimestamp::parse_iso8601(iso)
        .expect("fixture funding_time parses")
        .to_iso8601()
}

fn now() -> fortuna_core::clock::UtcTimestamp {
    // Strictly AFTER FUNDING_TIME, so the window has closed and the belief is due.
    fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-11T04:05:00.000Z").unwrap()
}

/// Insert one unresolved funding_forecast belief with the given id/event_key/fan
/// and a past `horizon` (so it is due at `now()`).
#[allow(clippy::too_many_arguments)]
async fn insert_belief(
    beliefs: &ScalarBeliefsRepo,
    belief_id: &str,
    event_key: &str,
    fan: &serde_json::Value,
    horizon_iso: &str,
) {
    beliefs
        .insert(
            belief_id,
            "funding_forecast",
            event_key,
            fan,
            "rate",
            horizon_iso,
            &json!({"strategy": "funding_forecast"}),
            "2026-06-11T03:00:00.000Z",
        )
        .await
        .expect("belief insert");
}

async fn belief_is_resolved(pool: &PgPool, belief_id: &str) -> (bool, Option<f64>) {
    let row = ScalarBeliefsRepo::new(pool.clone())
        .get(belief_id)
        .await
        .expect("get belief");
    (row.realized_value.is_some(), row.realized_value)
}

async fn score_count(pool: &PgPool, belief_id: &str) -> usize {
    BeliefScoresRepo::new(pool.clone())
        .scores_for_belief(belief_id)
        .await
        .expect("scores_for_belief")
        .len()
}

// ── happy path: resolve + five scored legs, with a captured prior window ──────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn due_captured_belief_resolves_and_scores_five_legs(pool: PgPool) {
    let beliefs = ScalarBeliefsRepo::new(pool.clone());
    let funding = FundingRatesHistoricalRepo::new(pool.clone());

    // The realized rate for THIS window AND the prior 8h window (both captured).
    funding
        .insert(
            MARKET,
            &canon(FUNDING_TIME),
            REALIZED,
            MARK,
            &canon(FUNDING_TIME),
        )
        .await
        .expect("insert realized");
    funding
        .insert(
            MARKET,
            &canon(PRIOR_FUNDING_TIME),
            PRIOR_REALIZED,
            PRIOR_MARK,
            &canon(PRIOR_FUNDING_TIME),
        )
        .await
        .expect("insert prior realized");

    // A forecast fan: median (estimate) a touch off the realized rate, a real
    // dispersion band (so rw_band recovers > 0). event_key uses the CANONICAL
    // horizon form (what the live producer emits via to_iso8601()).
    let horizon = canon(FUNDING_TIME);
    let event_key = format!("{MARKET}:{horizon}");
    let fan = fan_json(-0.000_30, 0.000_20);
    insert_belief(&beliefs, "sb-due", &event_key, &fan, &horizon).await;

    let resolved = resolve_and_score_funding_beliefs(&pool, now(), 1)
        .await
        .expect("resolve_and_score");
    assert_eq!(resolved, 1, "the one due, captured belief resolved");

    // It is resolved against the CURRENT window's realized rate (set once).
    let (is_resolved, value) = belief_is_resolved(&pool, "sb-due").await;
    assert!(is_resolved, "realized_value is set after resolution");
    assert_eq!(
        value,
        Some(REALIZED),
        "resolved against the CURRENT window rate, not the prior"
    );

    // Exactly the five legs, each a proper-rule CRPS: finite and ≥ 0.
    let rows = BeliefScoresRepo::new(pool.clone())
        .scores_for_belief("sb-due")
        .await
        .expect("scores");
    assert_eq!(rows.len(), 5, "crps_pinball + the four A2d baselines");
    let mut rule_ids: Vec<&str> = rows.iter().map(|r| r.rule_id.as_str()).collect();
    rule_ids.sort_unstable();
    assert_eq!(
        rule_ids,
        vec![
            "crps_pinball",
            "crps_pinball:carry_forward",
            "crps_pinball:last_rate",
            "crps_pinball:rw_estimate",
            "crps_pinball:rw_persistence",
        ],
        "the forecast leg plus the four baseline legs, named per the gate"
    );
    for r in &rows {
        assert!(
            r.score.is_finite() && r.score >= 0.0,
            "leg {} CRPS must be finite & ≥ 0: {}",
            r.rule_id,
            r.score
        );
    }
}

// ── idempotency: a second run resolves nothing and does not dup-key crash ─────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_second_run_is_idempotent(pool: PgPool) {
    let beliefs = ScalarBeliefsRepo::new(pool.clone());
    let funding = FundingRatesHistoricalRepo::new(pool.clone());
    funding
        .insert(
            MARKET,
            &canon(FUNDING_TIME),
            REALIZED,
            MARK,
            &canon(FUNDING_TIME),
        )
        .await
        .expect("insert realized");

    let horizon = canon(FUNDING_TIME);
    let event_key = format!("{MARKET}:{horizon}");
    let fan = fan_json(-0.000_30, 0.000_20);
    insert_belief(&beliefs, "sb-idem", &event_key, &fan, &horizon).await;

    // First run resolves + scores it.
    let first = resolve_and_score_funding_beliefs(&pool, now(), 1)
        .await
        .expect("first run");
    assert_eq!(first, 1, "first run resolves the belief");
    assert_eq!(
        score_count(&pool, "sb-idem").await,
        5,
        "five legs after run 1"
    );

    // Second run: the belief is no longer unresolved, so 0 are resolved — and
    // crucially NO dup-key panic on the existing five belief_scores rows.
    let second = resolve_and_score_funding_beliefs(&pool, now(), 1_000)
        .await
        .expect("second run must not error on already-scored rows");
    assert_eq!(second, 0, "nothing newly resolved on the second run");
    assert_eq!(
        score_count(&pool, "sb-idem").await,
        5,
        "still exactly five legs — no duplicate rows"
    );
}

// ── skip path: an uncaptured realized rate leaves the belief untouched ────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn an_uncaptured_belief_stays_unresolved_and_unscored(pool: PgPool) {
    let beliefs = ScalarBeliefsRepo::new(pool.clone());
    // NOTE: no funding_rates_historical row inserted — the realized rate is not
    // captured yet.
    let horizon = canon(FUNDING_TIME);
    let event_key = format!("{MARKET}:{horizon}");
    let fan = fan_json(-0.000_30, 0.000_20);
    insert_belief(&beliefs, "sb-uncaptured", &event_key, &fan, &horizon).await;

    let resolved = resolve_and_score_funding_beliefs(&pool, now(), 1)
        .await
        .expect("resolve_and_score");
    assert_eq!(resolved, 0, "no captured rate ⇒ nothing resolved");

    let (is_resolved, _) = belief_is_resolved(&pool, "sb-uncaptured").await;
    assert!(!is_resolved, "belief stays UNRESOLVED for a later run");
    assert_eq!(
        score_count(&pool, "sb-uncaptured").await,
        0,
        "and is UNSCORED (the skip path writes nothing)"
    );
}

// ── due gate: a belief whose window has not closed is not touched ─────────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn a_belief_whose_window_is_open_is_not_resolved(pool: PgPool) {
    let beliefs = ScalarBeliefsRepo::new(pool.clone());
    let funding = FundingRatesHistoricalRepo::new(pool.clone());
    // The rate is captured, but the horizon is in the FUTURE relative to `now()`.
    funding
        .insert(
            MARKET,
            &canon(FUNDING_TIME),
            REALIZED,
            MARK,
            &canon(FUNDING_TIME),
        )
        .await
        .expect("insert realized");

    let future_horizon = "2026-06-11T12:00:00.000Z"; // strictly after now()
    let event_key = format!("{MARKET}:{future_horizon}");
    let fan = fan_json(-0.000_30, 0.000_20);
    insert_belief(&beliefs, "sb-open", &event_key, &fan, future_horizon).await;

    let resolved = resolve_and_score_funding_beliefs(&pool, now(), 1)
        .await
        .expect("resolve_and_score");
    assert_eq!(
        resolved, 0,
        "an open-window belief is not due ⇒ not resolved"
    );
    let (is_resolved, _) = belief_is_resolved(&pool, "sb-open").await;
    assert!(!is_resolved, "open-window belief stays unresolved");
    assert_eq!(score_count(&pool, "sb-open").await, 0, "and unscored");
}
