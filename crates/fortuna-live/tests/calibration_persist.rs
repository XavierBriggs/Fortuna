//! Task B1 (F0) tests: the count-triggered calibration persist that WAKES the
//! synthesis (model) arm in paper.
//!
//! Written FROM the brief BEFORE the implementation (TDD). The bug: the weekly
//! review computed `ScopeCalibration.fitted` but never persisted it, and only
//! fired on a Monday-aligned boundary — so a multi-day demo never warmed the
//! model arm (no `calibration_params` row ⇒ synthesis sizes ZERO forever). The
//! fix is a stage-gated, idempotent persist driven by a DAILY count trigger
//! (`persist_daily_calibration`) plus the shared `persist_fitted_calibration`
//! helper, gated on `ExecutionMode::PaperLedger` ONLY (I7).
//!
//! Adversarial coverage:
//!   (a) DAILY trigger: ≥50 resolved samples in a category ⇒ exactly ONE
//!       `calibration_params` row (version 1) WITHOUT any weekly/Monday
//!       boundary; <50 ⇒ no row.
//!   (b) idempotent + versioned: re-running on the SAME 50 stays version 1 (the
//!       `fitted_on_n` guard); adding 10 more resolved (n=60) advances to
//!       version 2 with `fitted_on_n = 60`.
//!   (c) I7: `auto_persist = false` (any non-PaperLedger mode) ⇒ NO row even at
//!       n≥50. Asserted as ExecutionMode MEMBERSHIP over all five variants —
//!       only PaperLedger persists.
//!
//! ## Mutation-check note (for a reviewer)
//!
//! Flip the `auto_persist` gate to ignore its argument and (c) REDs (a non-paper
//! mode would persist). Drop the `fitted_on_n` guard in
//! `persist_fitted_calibration` and (b)'s "same 50 ⇒ still version 1" REDs (a
//! re-run would issue a duplicate version). Break the `>50` count gate in
//! `calibration_report` (FULL_AUTONOMY_N) and (a)'s "<50 ⇒ no row" REDs.

use fortuna_ledger::{BeliefsRepo, EventsRepo};
use fortuna_live::boot::ExecutionMode;
use fortuna_live::daemon::{persist_daily_calibration, persist_fitted_calibration};
use sqlx::PgPool;

const CATEGORY: &str = "weather";
const SYNTH_MODEL: &str = "test-synth-model";
/// Mirrors `SYNTH_CALIBRATION_STRATEGY` / `SYNTH_CALIBRATION_KIND` in daemon.rs
/// (the scope the persist keys on). Kept here as the durable-row assertion key.
const STRATEGY: &str = "synth_events";
const KIND: &str = "platt";

fn now() -> fortuna_core::clock::UtcTimestamp {
    fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-17T12:00:00.000Z").unwrap()
}

async fn seed_event(pool: &PgPool, id: &str) {
    EventsRepo::new(pool.clone())
        .create(
            id,
            "s",
            "c",
            "src",
            None,
            "2026-06-20T18:00:00.000Z",
            CATEGORY,
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .expect("event create");
}

/// Seed `count` RESOLVED beliefs in CATEGORY whose (p, outcome) samples carry
/// real spread AND both outcomes — so `fit_platt` is identifiable (a degenerate
/// record would refuse the fit and persist nothing, which would mask the path).
/// Index `i` runs in `[start, start+count)` so successive calls extend the
/// resolved record without colliding on belief/event ids.
async fn seed_resolved(pool: &PgPool, start: usize, count: usize) {
    let beliefs = BeliefsRepo::new(pool.clone());
    for i in start..start + count {
        let eid = format!("evt-{i:04}");
        let bid = format!("b-{i:04}");
        seed_event(pool, &eid).await;
        // The fit must be IDENTIFIABLE: several distinct claimed-p values (spread,
        // so the Hessian is non-singular) AND both outcomes appearing at EACH p
        // value (so it is not perfectly separated). p cycles over 7 buckets while
        // outcome cycles over 3 — gcd(7,3)=1, so across the run every p-bucket
        // sees both outcomes (fit_platt would refuse a degenerate record).
        let p = 0.1 + 0.8 * ((i % 7) as f64 / 6.0);
        let outcome = i % 3 != 0;
        beliefs
            .insert(
                &bid,
                "2026-06-10T12:00:00.000Z",
                &eid,
                p,
                p,
                "2026-06-20T18:00:00.000Z",
                &serde_json::json!([]),
                &serde_json::json!({"model_id": SYNTH_MODEL}),
                None,
            )
            .await
            .expect("belief insert");
        beliefs
            .resolve_and_score(&bid, outcome, 0.1, Some(50.0))
            .await
            .expect("resolve_and_score");
    }
}

async fn param_versions(pool: &PgPool) -> Vec<i32> {
    sqlx::query_scalar::<_, i32>(
        r#"SELECT version FROM calibration_params
           WHERE model_id = $1 AND strategy = $2 AND category = $3 AND kind = $4
           ORDER BY version"#,
    )
    .bind(SYNTH_MODEL)
    .bind(STRATEGY)
    .bind(CATEGORY)
    .bind(KIND)
    .fetch_all(pool)
    .await
    .expect("param_versions query")
}

async fn latest_fitted_on_n(pool: &PgPool) -> i64 {
    let params: serde_json::Value = sqlx::query_scalar(
        r#"SELECT params FROM calibration_params
           WHERE model_id = $1 AND strategy = $2 AND category = $3 AND kind = $4
           ORDER BY version DESC LIMIT 1"#,
    )
    .bind(SYNTH_MODEL)
    .bind(STRATEGY)
    .bind(CATEGORY)
    .bind(KIND)
    .fetch_one(pool)
    .await
    .expect("latest params query");
    params
        .get("fitted_on_n")
        .and_then(|v| v.as_i64())
        .expect("params JSON carries fitted_on_n")
}

// ── (a) the DAILY trigger persists at n≥50 with NO weekly/Monday boundary ─────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daily_trigger_persists_one_version_at_fifty(pool: PgPool) {
    seed_resolved(&pool, 0, 50).await;

    // The DAILY path (not the weekly/Monday boundary): build the scope from
    // resolved_stats, fit, and persist. auto_persist=true (paper).
    let persisted = persist_daily_calibration(
        &pool,
        SYNTH_MODEL,
        Some(CATEGORY),
        true,
        now(),
        now().epoch_millis().max(0) as u64,
    )
    .await
    .expect("daily calibration persist");
    assert_eq!(persisted, 1, "exactly one scope fitted+persisted at n=50");

    let versions = param_versions(&pool).await;
    assert_eq!(
        versions,
        vec![1],
        "exactly ONE calibration_params row (version 1) — the model arm is warm"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daily_trigger_persists_nothing_below_fifty(pool: PgPool) {
    seed_resolved(&pool, 0, 49).await;

    let persisted = persist_daily_calibration(
        &pool,
        SYNTH_MODEL,
        Some(CATEGORY),
        true,
        now(),
        now().epoch_millis().max(0) as u64,
    )
    .await
    .expect("daily calibration persist");
    assert_eq!(persisted, 0, "below FULL_AUTONOMY_N (50) nothing is fitted");
    assert!(
        param_versions(&pool).await.is_empty(),
        "no calibration_params row below the threshold"
    );
}

// ── (b) idempotent on unchanged data; versioned on new resolved data ──────────

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn re_run_on_same_data_is_idempotent_then_versions_on_new_data(pool: PgPool) {
    seed_resolved(&pool, 0, 50).await;
    let base = now().epoch_millis().max(0) as u64;

    // First run → version 1, fitted_on_n = 50.
    let first = persist_daily_calibration(&pool, SYNTH_MODEL, Some(CATEGORY), true, now(), base)
        .await
        .expect("first persist");
    assert_eq!(first, 1, "first run persists version 1");
    assert_eq!(param_versions(&pool).await, vec![1]);
    assert_eq!(latest_fitted_on_n(&pool).await, 50, "fitted on 50");

    // Re-run on the SAME 50 → the fitted_on_n guard makes it a NO-OP (no v2).
    let second =
        persist_daily_calibration(&pool, SYNTH_MODEL, Some(CATEGORY), true, now(), base + 1)
            .await
            .expect("idempotent re-run");
    assert_eq!(second, 0, "unchanged resolved data ⇒ no new version");
    assert_eq!(
        param_versions(&pool).await,
        vec![1],
        "still exactly version 1 — re-running a boundary is a clean no-op"
    );

    // Add 10 more resolved (n=60) → the guard releases, version 2 at n=60.
    seed_resolved(&pool, 50, 10).await;
    let third =
        persist_daily_calibration(&pool, SYNTH_MODEL, Some(CATEGORY), true, now(), base + 2)
            .await
            .expect("versioned re-run");
    assert_eq!(third, 1, "new resolved data ⇒ exactly one new version");
    assert_eq!(
        param_versions(&pool).await,
        vec![1, 2],
        "the ladder advanced to version 2"
    );
    assert_eq!(
        latest_fitted_on_n(&pool).await,
        60,
        "version 2 was fitted on the new resolved count (60)"
    );
}

// ── (c) I7: ONLY ExecutionMode::PaperLedger persists (membership, not strings) ─

/// The single gate main.rs uses to set `ReviewWiring.auto_persist_calibration`.
/// The test references the SAME mapping (no test-only re-derivation): persist is
/// allowed iff the mode is PaperLedger.
fn auto_persist_for(mode: ExecutionMode) -> bool {
    mode.auto_persist_calibration()
}

#[test]
fn only_paper_ledger_allows_auto_persist() {
    use ExecutionMode::*;
    for mode in [LiveDataOnly, DryRun, DemoOrders, ProductionOrders] {
        assert!(
            !auto_persist_for(mode),
            "{mode:?} must NEVER auto-persist calibration (I7)",
        );
    }
    assert!(
        auto_persist_for(PaperLedger),
        "PaperLedger is the ONLY mode that auto-persists"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn no_persist_in_non_paper_modes_even_at_threshold(pool: PgPool) {
    use ExecutionMode::*;
    seed_resolved(&pool, 0, 60).await;
    let base = now().epoch_millis().max(0) as u64;

    // Every non-paper mode: the gate is false ⇒ the daily path persists NOTHING
    // even though the resolved record is well past the fit threshold.
    for (idx, mode) in [LiveDataOnly, DryRun, DemoOrders, ProductionOrders]
        .into_iter()
        .enumerate()
    {
        let persisted = persist_daily_calibration(
            &pool,
            SYNTH_MODEL,
            Some(CATEGORY),
            auto_persist_for(mode),
            now(),
            base + idx as u64,
        )
        .await
        .expect("daily persist");
        assert_eq!(persisted, 0, "{mode:?} persists nothing (I7 wall)");
    }
    assert!(
        param_versions(&pool).await.is_empty(),
        "no calibration_params row written under any non-paper mode"
    );

    // PaperLedger on the SAME data DOES persist — proving the data was fit-ready
    // and only the gate held the others back.
    let paper = persist_daily_calibration(
        &pool,
        SYNTH_MODEL,
        Some(CATEGORY),
        auto_persist_for(PaperLedger),
        now(),
        base + 100,
    )
    .await
    .expect("paper persist");
    assert_eq!(paper, 1, "PaperLedger persists exactly one version");
    assert_eq!(param_versions(&pool).await, vec![1]);
}

// ── the shared helper's gate, exercised directly (mutation-proof on auto_persist) ─

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn persist_helper_returns_zero_when_gate_is_false(pool: PgPool) {
    use fortuna_cognition::review::{calibration_report, ScopeKey, ScopeRecord};
    use std::collections::BTreeMap;

    seed_resolved(&pool, 0, 55).await;
    let stats = BeliefsRepo::new(pool.clone())
        .resolved_stats(CATEGORY)
        .await
        .expect("resolved_stats");
    let key = ScopeKey {
        model_id: SYNTH_MODEL.to_string(),
        strategy: STRATEGY.to_string(),
        category: CATEGORY.to_string(),
    };
    let record = ScopeRecord {
        key,
        samples: stats.iter().map(|s| (s.p, s.outcome)).collect(),
        clv_bps: stats.iter().filter_map(|s| s.clv_bps).collect(),
    };
    let scopes = calibration_report(&[record], &BTreeMap::new());
    assert!(
        scopes[0].fitted.is_some(),
        "precondition: the scope IS fit-ready at n=55"
    );

    // auto_persist=false ⇒ the helper returns 0 and writes nothing, even though
    // the scope carries a fitted set.
    let n = persist_fitted_calibration(&pool, &scopes, false, now(), 1)
        .await
        .expect("persist helper");
    assert_eq!(n, 0, "auto_persist=false ⇒ persist nothing");
    assert!(param_versions(&pool).await.is_empty(), "no row written");

    // auto_persist=true ⇒ it persists exactly the one fitted scope.
    let n = persist_fitted_calibration(&pool, &scopes, true, now(), 2)
        .await
        .expect("persist helper");
    assert_eq!(n, 1, "auto_persist=true ⇒ the fitted scope persists");
    assert_eq!(param_versions(&pool).await, vec![1]);
}
