//! WS2 S6d test: the daemon WEEKLY CADENCE fires the `recompute_scorecards`
//! driver, so a populated `scorecards` snapshot is durably persisted on-cadence
//! (milestone D-E "recompute on cadence"). This is the coverage S6c could not
//! close: S6c tested the DRIVER in isolation; this proves the CALL-SITE fires
//! when `drive()` crosses the week boundary.
//!
//! Written FROM the brief BEFORE the wiring (TDD). RED before the call-site
//! exists (no scorecard row after the week boundary); GREEN after.
//!
//! Mirrors `daemon_smoke::drive_runs_the_weekly_review_at_the_week_boundary`
//! (the sibling that proves the weekly REVIEW fires) — same `drive()` scaffold,
//! same `ReviewWiring`, same `StopAtCadence` boundary tick — but seeds a scored
//! `weather` scope (forward-resolved binary beliefs + outcomes + confirmed
//! direct edges + liquid pre-benchmark snapshots) and asserts the cadence
//! PERSISTS a populated scorecard the repo reads back.
//!
//! Coverage (black-box, via the `ScorecardsRepo` read-back):
//!   - RED: no `latest_scorecard("weather", Some("aeolus"), "forward")` before
//!     the wiring; GREEN: a populated GO card after the week-boundary tick.
//!   - Mixed CLV: a scope where SOME resolved beliefs carry `clv_bps` and some
//!     are NULL → the card's `clv_mean_bps` is the mean over the PRESENT values,
//!     no panic, and `n`/baseline stay index-aligned (the S6c-verifier flag).

use fortuna_cognition::cycle::TriageDecision;
use fortuna_cognition::mind::{Mind, StubMind};
use fortuna_core::clock::SimClock;
use fortuna_core::market::{Contracts, MarketId};
use fortuna_core::money::Cents;
use fortuna_ledger::PgIntentJournal;
use fortuna_live::boot::DaemonToml;
use fortuna_live::compose::DegradeScrape;
use fortuna_live::daemon::{compose_runner, default_degrade_thresholds, drive};
use fortuna_live::run_loop::{CadenceDriver, HaltPoller, LoopConfig};
use fortuna_ops::FortunaConfig;
use fortuna_runner::SimRunner;
use fortuna_venues::sim::SimVenue;
use fortuna_venues::PriceLevel;
use sqlx::PgPool;
use std::sync::Arc;

/// Daemon boot time (mirrors daemon_smoke::t0): a Thursday, so the first
/// `WeeklyScheduler.due()` fires on boot, exactly like the daily digest.
fn t0() -> fortuna_core::clock::UtcTimestamp {
    fortuna_core::clock::UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
}

/// The inert mind every smoke passes (no beliefs => no synthesis proposals).
fn stub_mind() -> Arc<dyn Mind> {
    Arc::new(StubMind::scripted(Vec::new()))
}

/// Sim cadence that FIRES THE STOP CHANNEL at a chosen wake (mirrors
/// daemon_smoke::StopAtCadence) — the deterministic week-boundary tick.
struct StopAtCadence {
    clock: Arc<SimClock>,
    sleeps: u64,
    fire_at: u64,
    tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl CadenceDriver for StopAtCadence {
    async fn sleep_ms(&mut self, ms: u64) {
        self.clock.advance_millis(ms).expect("sim clock advances");
        self.sleeps += 1;
        if self.sleeps == self.fire_at {
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }
    }
}

struct NeverHalted;

impl HaltPoller for NeverHalted {
    async fn poll(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }
}

/// Arbitrage books so the synthesis arm has live quotes (mirrors
/// daemon_smoke::arb_books); inert here (stub mind drafts no proposals) but
/// required for a clean boot.
fn arb_books(r: &SimRunner<SimVenue, PgIntentJournal>) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: Contracts::new(q),
    };
    for (m, bid, ask) in [
        ("SIM-BKT-LO", 20, 25),
        ("SIM-BKT-MID", 23, 28),
        ("SIM-BKT-HI", 25, 30),
    ] {
        r.venue()
            .set_book(
                &MarketId::new(m).unwrap(),
                vec![lvl(bid, 80)],
                vec![lvl(ask, 80)],
            )
            .unwrap();
    }
}

/// Seed one forward-resolved binary belief on its own event, with a confirmed
/// direct edge and a liquid pre-benchmark snapshot so a de-vigged market
/// baseline exists (mirrors `scorecard_driver::seed_scored_belief`). `bid`/`ask`
/// are the YES cents of the benchmark snapshot. `clv` is the per-belief CLV in
/// bps recorded at resolution (`None` => the belief has NULL clv_bps).
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
    clv: Option<f64>,
) {
    let benchmark = "2026-06-14T10:00:00.000Z";
    let event_id = format!("sc6d-evt-{suffix}");

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

    let provenance = serde_json::json!({"producer": "aeolus"});
    let beliefs = fortuna_ledger::BeliefsRepo::new(pool.clone());
    let belief_id = format!("sc6d-belief-{suffix}");
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
    beliefs
        .resolve_and_score(&belief_id, outcome, brier, clv)
        .await
        .unwrap();

    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            &format!("sc6d-edge-{suffix}"),
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
            &format!("sc6d-snap-{suffix}"),
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

/// Build the `ReviewWiring` the daemon needs for the weekly cadence, keyed on the
/// seeded `weather` scope. Sim smoke (not PaperLedger): never auto-persist
/// calibration (I7) — the SCORECARD recompute is a snapshot, not a promotion.
fn review_wiring(
    pool: &PgPool,
    full: &FortunaConfig,
    dcfg: &DaemonToml,
) -> fortuna_live::daemon::ReviewWiring {
    let review = dcfg.review.clone().expect("the example ships [review]");
    fortuna_live::daemon::ReviewWiring {
        pool: pool.clone(),
        mind: stub_mind(),
        review,
        synth_category: Some("weather".to_string()),
        auto_persist_calibration: false,
        start: t0(),
        weekly: fortuna_live::daemon::WeeklyScheduler::new(),
        monthly: fortuna_live::daemon::MonthlyScheduler::new(),
        envelopes: full.envelopes.clone(),
    }
}

/// Drive `drive()` across the week boundary (the first `WeeklyScheduler.due()`
/// fires on boot) with the given review wiring. Shared by both cadence tests.
async fn drive_one_week_boundary(
    pool: &PgPool,
    full: &FortunaConfig,
    dcfg: &DaemonToml,
    reviews: fortuna_live::daemon::ReviewWiring,
) {
    let runner = compose_runner(
        pool.clone(),
        full,
        dcfg,
        t0(),
        91,
        stub_mind(),
        TriageDecision::AlwaysAccept,
    )
    .await
    .unwrap();
    arb_books(&runner);

    let (tx, mut stop) = tokio::sync::oneshot::channel::<()>();
    let mut cadence = StopAtCadence {
        clock: runner.clock.clone(),
        sleeps: 0,
        fire_at: 6,
        tx: Some(tx),
    };
    let mut poller = NeverHalted;
    let loop_cfg = LoopConfig {
        tick_interval_ms: 1000,
        halt_poll_ms: 500,
        clv_min_touch_qty: 1,
        clv_max_spread_cents: 10,
    };
    let mut scrape = DegradeScrape::new(default_degrade_thresholds());
    let mut daily = fortuna_live::daemon::DailyScheduler::new();

    let mut runner = fortuna_live::daemon::ActiveRunner::Sim(runner);
    let (_stats, _shutdown) = drive(
        &mut runner,
        &mut cadence,
        &mut poller,
        &loop_cfg,
        4,
        &mut stop,
        |_r, _s| {},
        &mut scrape,
        None,
        &mut daily,
        None,              // synthesis_refresh
        None,              // scalar producer
        None,              // reconciliation
        Some(reviews),     // M2 + S6d: weekly review + scorecard cadence wiring
        "claude-opus-4-8", // S5b: the configured synthesis model id
        None,              // perp feed
        None,              // personas
        None,              // discovery
        None,              // resolution_pool
        None,              // live PerpTick channel
        None,              // fills persist
        None,              // recording persist
        None,              // snapshot persist
    )
    .await
    .expect("daemon drive");
}

/// The weekly cadence FIRES `recompute_scorecards` and persists a populated GO
/// scorecard the repo reads back. RED before the call-site exists; GREEN after.
///
/// Seeds 32 forward-resolved beliefs (≥ the spec §11 forward-volume floor of 30)
/// where the producer beats the de-vigged market baseline, so the verdict is GO
/// (not Insufficient). Each pair is one confident-right and one confident-wrong-
/// but-still-better-than-market sample, giving a deterministic mean Brier.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn weekly_cadence_persists_a_populated_scorecard(pool: PgPool) {
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&format!(
        "{text}\n[synthesis]\nvenue = \"sim\"\ncategory = \"weather\"\n"
    ))
    .unwrap();

    // Seed 32 forward-resolved scored beliefs in scope "weather", producer
    // "aeolus", each with a confirmed direct edge + liquid pre-benchmark snapshot.
    // All identical: p=0.70 outcome=true → producer Brier (0.70−1)²=0.09; market
    // (30+40)/200=0.35 → baseline (0.35−1)²=0.4225. Producer beats baseline → GO.
    let seeded = 32usize;
    for i in 0..seeded {
        seed_scored_belief(
            &pool,
            &format!("{i:03}"),
            &format!("SC6D-BKT-{i:03}"),
            0.70,
            true,
            0.09,
            30,
            40,
            Some(7.5),
        )
        .await;
    }

    // RED-BEFORE invariant: no scorecard exists until the cadence fires it.
    let before = fortuna_ledger::ScorecardsRepo::new(pool.clone())
        .latest_scorecard("weather", Some("aeolus"), "forward")
        .await
        .unwrap();
    assert!(
        before.is_none(),
        "no scorecard before the week boundary fires recompute_scorecards"
    );

    let reviews = review_wiring(&pool, &full, &dcfg);
    drive_one_week_boundary(&pool, &full, &dcfg, reviews).await;

    // The cadence persisted a populated GO card for the producer-attributed scope.
    let card = fortuna_ledger::ScorecardsRepo::new(pool.clone())
        .latest_scorecard("weather", Some("aeolus"), "forward")
        .await
        .expect("latest_scorecard read")
        .expect("the weekly cadence persisted a producer scorecard");
    assert_eq!(card.scope, "weather");
    assert_eq!(card.producer.as_deref(), Some("aeolus"));
    assert_eq!(card.window, "forward");
    assert_eq!(
        card.n as usize, seeded,
        "exactly the seeded forward beliefs"
    );
    assert!(
        (card.brier - 0.09).abs() < 1e-9,
        "mean producer Brier = 0.09 (all p=0.70, outcome=true); got {}",
        card.brier
    );
    assert!(
        (card.brier_baseline - 0.4225).abs() < 1e-9,
        "mean de-vigged market baseline = 0.4225; got {}",
        card.brier_baseline
    );
    assert_eq!(
        card.go.decision,
        fortuna_cognition::scoring::GoDecision::Go,
        "0.09 < 0.4225 with n=32 ≥ 30 (spec §11 forward floor) → Go"
    );

    // The cadence ALSO persisted the merged-scope (producer = NULL) card — both
    // buckets the dashboard reads come from the same recompute pass.
    let merged = fortuna_ledger::ScorecardsRepo::new(pool.clone())
        .latest_scorecard("weather", None, "forward")
        .await
        .expect("latest_scorecard merged read")
        .expect("the weekly cadence persisted the merged-scope scorecard");
    assert_eq!(merged.producer, None);
    assert_eq!(
        merged.n as usize, seeded,
        "the merged scope sees the same forward beliefs"
    );
}

/// MIXED CLV (the S6c-verifier flag): a scope where SOME resolved beliefs carry
/// `clv_bps` and some are NULL. The cadence-persisted card's `clv_mean_bps` is
/// the mean over the PRESENT values only — no panic, and `n`/baseline stay
/// index-aligned (every belief still contributes its sample + baseline loss).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn weekly_cadence_handles_mixed_clv_without_panic(pool: PgPool) {
    let example_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/fortuna.example.toml"
    );
    let text = std::fs::read_to_string(example_path).unwrap();
    let full = FortunaConfig::load_file(example_path).unwrap();
    let dcfg = DaemonToml::parse(&format!(
        "{text}\n[synthesis]\nvenue = \"sim\"\ncategory = \"weather\"\n"
    ))
    .unwrap();

    // 30 forward beliefs (≥ the §11 floor). Identical samples (p=0.70,
    // outcome=true) so `n`/Brier are deterministic, BUT only every other belief
    // carries a CLV (the rest are NULL). The present CLVs alternate 10.0 / 20.0
    // → 15 present values, all of mean 15.0. `clv_mean_bps` MUST equal 15.0 (the
    // mean over the 15 PRESENT values, NOT over all 30 with NULLs counted as 0).
    let total = 30usize;
    let mut present_clv_sum = 0.0_f64;
    let mut present_clv_n = 0usize;
    for i in 0..total {
        let clv = if i % 2 == 0 {
            let v = if (i / 2) % 2 == 0 { 10.0 } else { 20.0 };
            present_clv_sum += v;
            present_clv_n += 1;
            Some(v)
        } else {
            None // NULL clv_bps — present in the scope, absent from the CLV series
        };
        seed_scored_belief(
            &pool,
            &format!("{i:03}"),
            &format!("SC6D-MIX-{i:03}"),
            0.70,
            true,
            0.09,
            30,
            40,
            clv,
        )
        .await;
    }
    let expected_clv_mean = present_clv_sum / present_clv_n as f64;

    let reviews = review_wiring(&pool, &full, &dcfg);
    drive_one_week_boundary(&pool, &full, &dcfg, reviews).await;

    let card = fortuna_ledger::ScorecardsRepo::new(pool.clone())
        .latest_scorecard("weather", Some("aeolus"), "forward")
        .await
        .expect("latest_scorecard read")
        .expect("the weekly cadence persisted a scorecard for the mixed-CLV scope");

    // Every belief still contributed its sample + baseline loss (index-aligned):
    // the NULL CLVs did NOT drop any sample from the Brier/baseline series.
    assert_eq!(
        card.n as usize, total,
        "all {total} forward beliefs contribute samples; NULL clv drops none"
    );
    assert!(
        (card.brier - 0.09).abs() < 1e-9,
        "mean producer Brier unaffected by NULL clv; got {}",
        card.brier
    );

    // clv_mean_bps is the mean over the PRESENT values only.
    let clv_mean = card
        .clv_mean_bps
        .expect("some beliefs carry CLV → a mean exists");
    assert!(
        (clv_mean - expected_clv_mean).abs() < 1e-9,
        "clv_mean_bps must be the mean over the {present_clv_n} PRESENT values \
         ({expected_clv_mean}), not diluted by the NULLs; got {clv_mean}"
    );
}
