//! T4.1 hard requirement 2 (kickoff): Postgres wiring through the DAEMON
//! path — the same SimRunner composition the daemon boots must journal
//! intents into Postgres (PgIntentJournal), and the journal must feed
//! recovery. Written from the kickoff text BEFORE the journal-generic
//! runner existed; runs on the throwaway sqlx test database (NEVER the
//! operator db — .cargo/config.toml [env] routes to fortuna_dev's server).

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::market::{Contracts, MarketId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::{ExecPolicy, IntentJournal, OrderManager};
use fortuna_gates::GateConfig;
use fortuna_ledger::PgIntentJournal;
use fortuna_runner::mech_structural::{MechStructural, MechStructuralConfig};
use fortuna_runner::{MemoryAuditSink, RunnerConfig, SimRunner, Strategy};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            maker_coeff = "0.0175"
        "#,
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn bracket_market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("bracket {id}"),
        category: "weather".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "nws".into(),
            resolution_source: "nws".into(),
            expected_lag_hours: 2,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    }
}

fn gate_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 800000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 45
        max_cross_cents = 10
        per_market_exposure_cents = 100000
        per_event_exposure_cents = 150000
        require_event_mapping = false

        [per_strategy.mech_structural]
        max_exposure_cents = 200000
        max_order_notional_cents = 10000
        min_net_edge_bps = 100

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap()
}

fn runner_config(seed: u64) -> RunnerConfig {
    RunnerConfig {
        seed,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("mech_structural".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![
            bracket_market("BKT-LO"),
            bracket_market("BKT-MID"),
            bracket_market("BKT-HI"),
        ],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(seed),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    }
}

fn strategy() -> Box<dyn Strategy> {
    Box::new(
        MechStructural::new(MechStructuralConfig {
            bracket_sets: vec![vec![mkt("BKT-LO"), mkt("BKT-MID"), mkt("BKT-HI")]],
            min_edge_cents_per_set: 2,
            max_unhedged_notional: Cents::new(5_000),
            max_leg_open_ms: 60_000,
            min_completion_edge_bps: 100,
        })
        .unwrap(),
    )
}

fn set_arb_books<J: IntentJournal + Send>(runner: &SimRunner<J>) {
    let lvl = |p: i64, q: i64| PriceLevel {
        price: Cents::new(p),
        qty: Contracts::new(q),
    };
    for (m, bid, ask) in [("BKT-LO", 20, 25), ("BKT-MID", 23, 28), ("BKT-HI", 25, 30)] {
        runner
            .venue()
            .set_book(&mkt(m), vec![lvl(bid, 80)], vec![lvl(ask, 80)])
            .unwrap();
    }
}

/// The daemon composition's journal is Postgres: a tick that submits
/// orders must leave the intent trail in the DATABASE (journal-before-
/// network is the exec contract; here we prove the daemon path persists
/// it durably), and a fresh journal handle must recover from it.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn daemon_composition_journals_intents_in_postgres(pool: PgPool) {
    let journal_clock: Arc<dyn Clock> = Arc::new(SimClock::new(t0()));
    let journal = PgIntentJournal::new(pool.clone(), "sim", journal_clock.clone());

    let mut r = SimRunner::new_with_journal(
        runner_config(42),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
        journal,
    )
    .await
    .unwrap();
    set_arb_books(&r);

    let report = r.tick().await.unwrap();
    assert_eq!(report.orders_submitted, 3, "three legs submitted");

    // The trail is IN POSTGRES: three intents, each with at least
    // Created + SubmitAttempted (journal-before-network), plus outcomes.
    let distinct: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT intent_id) FROM intent_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(distinct, 3, "one journal lineage per leg");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM intent_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        events >= 6,
        "each leg journals Created before SubmitAttempted (got {events})"
    );

    // Crash-recovery path: a FRESH journal handle on the same database
    // sees the full trail and the order manager recovers from it.
    let fresh = PgIntentJournal::new(pool.clone(), "sim", journal_clock.clone());
    let rows = fresh.load_all().await.unwrap();
    assert!(rows.len() >= 6, "recovery fold input present");
    let recovered = OrderManager::recover(fresh, journal_clock, ExecPolicy::default()).await;
    assert!(
        recovered.is_ok(),
        "recovery from the Pg journal must fold cleanly"
    );
}

/// The default-journal constructor still composes identically (the
/// widening must not change existing behavior): same seed, same books,
/// same submission count, no Postgres anywhere.
#[test]
fn memory_journal_default_unchanged() {
    let mut r = SimRunner::new(
        runner_config(42),
        vec![strategy()],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    set_arb_books(&r);
    let report = futures::executor::block_on(r.tick()).unwrap();
    assert_eq!(report.orders_submitted, 3);
}
