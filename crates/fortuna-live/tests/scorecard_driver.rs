//! WS2 S6c test: the daemon-cadence DRIVER that populates the `scorecards`
//! snapshot from the ledger (milestone D-E "recompute on cadence").
//!
//! Written FROM the brief BEFORE the driver (TDD). The pure aggregation
//! (`fortuna_cognition::scorecard_agg::assemble_from_samples`), the
//! `ScorecardsRepo`, and the endpoint already exist (S6a/S6b). This slice ships
//! the thin connecting wire: `recompute_scorecards` COLLECTS the per-sample
//! vectors the existing `run_weekly_review` Brier-baseline path already computes
//! — `(belief.p, belief.outcome)` and the de-vigged market baseline loss
//! `mb = (market_p − outcome)²` with `market_p = (bid + ask) / 200.0` — then
//! `assemble_from_samples(...)` and persists via `ScorecardsRepo`.
//!
//! Coverage (black-box, via the repo read-back):
//!   - RED before the driver, GREEN after.
//!   - Seed a `weather` scope: two forward-resolved binary beliefs + their
//!     outcomes + confirmed direct edges + liquid pre-benchmark price snapshots
//!     (so a de-vigged market baseline EXISTS), then run the driver, then assert
//!     `latest_scorecard("weather", Some("aeolus"), "forward")` returns a row
//!     with the expected `go.decision == Go` and `brier`/`brier_baseline`/`n`
//!     matching the seeded samples.
//!   - The hand-computed values, mirroring the daemon_smoke Brier-baseline test:
//!
//! ```text
//! Belief A: p=0.70, outcome=true  -> producer (0.70-1)^2=0.09;
//!           market_p=(30+40)/200=0.35 -> baseline (0.35-1)^2=0.4225
//! Belief B: p=0.40, outcome=false -> producer (0.40-0)^2=0.16;
//!           market_p=(50+70)/200=0.60 -> baseline (0.60-0)^2=0.36
//! mean producer Brier  = (0.09+0.16)/2 = 0.125
//! mean market baseline = (0.4225+0.36)/2 = 0.39125
//! ```
//!
//!     Producer beats baseline (0.125 < 0.39125) with n=2 >= min_n -> `Go`.
//!   - `window = "forward"` excludes `source='historical-import'`: a seeded
//!     historical-import belief MUST NOT change `n` or the Brier.
//!
//! Each test gets an isolated, migrated database via #[sqlx::test].

use fortuna_cognition::scoring::GoDecision;
use fortuna_core::clock::UtcTimestamp;
use sqlx::PgPool;

/// Seed one forward-resolved binary belief on its own event, with a confirmed
/// direct edge and a liquid pre-benchmark snapshot so a de-vigged market
/// baseline exists. `bid`/`ask` are the YES cents of the benchmark snapshot.
#[allow(clippy::too_many_arguments)]
async fn seed_scored_belief(
    pool: &PgPool,
    suffix: &str,
    market_id: &str,
    p: f64,
    outcome: bool,
    brier: f64,
    bid: i64,
    ask: i64,
    source_historical: bool,
) {
    let benchmark = "2026-06-14T10:00:00.000Z";
    let event_id = format!("scd-evt-{suffix}");

    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            &event_id,
            "Will it happen?",
            "official",
            "nws",
            Some(benchmark),
            benchmark,
            "weather",
            "2026-06-13T00:00:00.000Z",
        )
        .await
        .unwrap();

    let provenance = if source_historical {
        serde_json::json!({"producer": "aeolus", "source": "historical-import"})
    } else {
        serde_json::json!({"producer": "aeolus"})
    };
    let beliefs = fortuna_ledger::BeliefsRepo::new(pool.clone());
    let belief_id = format!("scd-belief-{suffix}");
    beliefs
        .insert(
            &belief_id,
            "2026-06-13T01:00:00.000Z",
            &event_id,
            p,
            p,
            benchmark,
            &serde_json::json!([]),
            &provenance,
            None,
        )
        .await
        .unwrap();
    // CLV is recorded on the belief so the driver has a non-empty CLV series.
    beliefs
        .resolve_and_score(&belief_id, outcome, brier, Some(7.5))
        .await
        .unwrap();

    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            &format!("scd-edge-{suffix}"),
            market_id,
            "sim",
            &event_id,
            "direct",
            0.9,
            "model:stub",
            Some("op"),
            None,
            "2026-06-13T00:01:00.000Z",
        )
        .await
        .unwrap();

    // Liquid snapshot strictly before benchmark_at (2026-06-14T10:00:00Z).
    fortuna_ledger::SnapshotsRepo::new(pool.clone())
        .insert(
            &format!("scd-snap-{suffix}"),
            market_id,
            "sim",
            Some(&event_id),
            "t24h",
            Some(bid),
            Some(ask),
            Some(50),
            Some(50),
            true,
            "2026-06-14T09:00:00.000Z",
        )
        .await
        .unwrap();
}

/// The driver collects the de-vigged samples + baseline from the ledger and
/// persists a Scorecard the repo can read back, with the expected GO verdict and
/// Brier values. RED before `recompute_scorecards` exists; GREEN after.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn recompute_scorecards_persists_a_go_card_for_the_seeded_scope(pool: PgPool) {
    // A: p=0.70 outcome=true  brier=0.09; market (30+40)/200=0.35 → baseline 0.4225
    seed_scored_belief(&pool, "A", "SCD-BKT-LO", 0.70, true, 0.09, 30, 40, false).await;
    // B: p=0.40 outcome=false brier=0.16; market (50+70)/200=0.60 → baseline 0.36
    seed_scored_belief(&pool, "B", "SCD-BKT-MID", 0.40, false, 0.16, 50, 70, false).await;
    // A historical-import belief that MUST be excluded from window="forward".
    seed_scored_belief(&pool, "IMP", "SCD-BKT-IMP", 0.80, true, 0.04, 10, 20, true).await;

    let now = UtcTimestamp::parse_iso8601("2026-06-21T00:00:00.000Z").unwrap();

    // The driver under test: one scorecard for (scope="weather", producer="aeolus",
    // window="forward"), min_n=2 so the seeded n=2 is judged (not Insufficient).
    let card =
        fortuna_live::daemon::recompute_scorecards(&pool, "weather", Some("aeolus"), 2, 1, now)
            .await
            .expect("recompute_scorecards runs")
            .expect("a scorecard was produced for the seeded scope");

    // The returned card matches the seeded samples (forward-only).
    assert_eq!(card.n, 2, "exactly the 2 forward beliefs (IMP excluded)");
    assert!(
        (card.brier - 0.125).abs() < 1e-9,
        "mean producer Brier = (0.09+0.16)/2 = 0.125; got {}",
        card.brier
    );
    assert!(
        (card.brier_baseline - 0.39125).abs() < 1e-9,
        "mean de-vigged market baseline = (0.4225+0.36)/2 = 0.39125; got {}",
        card.brier_baseline
    );
    assert_eq!(
        card.go.decision,
        GoDecision::Go,
        "0.125 < 0.39125 with n=2 ≥ min_n → Go"
    );

    // And the row is durably in the ledger (the cadence-persisted snapshot).
    let stored = fortuna_ledger::ScorecardsRepo::new(pool.clone())
        .latest_scorecard("weather", Some("aeolus"), "forward")
        .await
        .expect("latest_scorecard")
        .expect("the driver persisted a row");
    assert_eq!(
        stored, card,
        "the persisted scorecard round-trips to the returned card"
    );
    assert_eq!(stored.scope, "weather");
    assert_eq!(stored.producer.as_deref(), Some("aeolus"));
    assert_eq!(stored.window, "forward");
}

/// A scope with no resolved-with-snapshot beliefs yields an `Insufficient` card
/// (n=0 < min_n) — never a panic, and still persisted as the honest snapshot.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn recompute_scorecards_empty_scope_is_insufficient(pool: PgPool) {
    let now = UtcTimestamp::parse_iso8601("2026-06-21T00:00:00.000Z").unwrap();
    let card =
        fortuna_live::daemon::recompute_scorecards(&pool, "weather", Some("aeolus"), 2, 1, now)
            .await
            .expect("recompute_scorecards runs on an empty scope")
            .expect("a (possibly insufficient) card is still produced + persisted");
    assert_eq!(card.n, 0);
    assert_eq!(card.go.decision, GoDecision::Insufficient);
}
