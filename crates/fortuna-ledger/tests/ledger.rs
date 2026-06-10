//! T0.8 tests: audit writer (I5), Postgres intent journal, Phase-0 repos.
//! Each test gets an isolated, migrated database via #[sqlx::test].

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{
    Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId, VenueOrderId,
};
use fortuna_core::money::Cents;
use fortuna_exec::{ExecPolicy, IntentStatus, OrderManager, SubmitOutcome};
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_ledger::{AuditWriter, FillsRepo, HaltsRepo, PgIntentJournal, ReservationsRepo};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{Fill, Market, MarketStatus, PriceLevel, SettlementMeta, Venue};
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeSet;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn clock() -> Arc<SimClock> {
    Arc::new(SimClock::new(t0()))
}

// ---- audit writer (I5) ----

#[sqlx::test(migrations = "./migrations")]
async fn audit_appends_and_reads_back(pool: PgPool) {
    let w = AuditWriter::new(pool, clock(), 7);
    let id = w
        .append(
            "gate_decision",
            Some("system"),
            Some("ref-1"),
            json!({"verdict": "pass"}),
        )
        .await
        .unwrap();
    let rows = w.recent("gate_decision", 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].audit_id, id.to_string());
    assert_eq!(rows[0].payload, json!({"verdict": "pass"}));
    assert_eq!(rows[0].at, t0());
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_rows_refuse_mutation_at_the_database(pool: PgPool) {
    let w = AuditWriter::new(pool.clone(), clock(), 7);
    let id = w.append("halt", None, None, json!({})).await.unwrap();
    let update = sqlx::query("UPDATE audit SET kind = 'forged' WHERE audit_id = $1")
        .bind(id.to_string())
        .execute(&pool)
        .await;
    assert!(update.unwrap_err().to_string().contains("append-only"));
    let delete = sqlx::query("DELETE FROM audit WHERE audit_id = $1")
        .bind(id.to_string())
        .execute(&pool)
        .await;
    assert!(delete.unwrap_err().to_string().contains("append-only"));
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_write_failure_surfaces_as_an_error(pool: PgPool) {
    // I5: no audit, no trading. The runner halts on this Err; here we prove
    // the failure is LOUD (a dead pool can never silently "succeed").
    let w = AuditWriter::new(pool.clone(), clock(), 7);
    pool.close().await;
    let result = w.append("order", None, None, json!({})).await;
    assert!(result.is_err());
}

// ---- Postgres intent journal: the crash-recovery round trip ----

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

fn sim_venue(clock: Arc<SimClock>) -> SimVenue {
    let v = SimVenue::new(
        VenueId::new("sim").unwrap(),
        clock,
        fee_model(),
        FaultConfig::none(1),
        Cents::new(100_000),
    );
    v.add_market(Market {
        id: MarketId::new("M1").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        title: "ledger test market".into(),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 0,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    });
    v.set_book(
        &MarketId::new("M1").unwrap(),
        vec![PriceLevel {
            price: Cents::new(45),
            qty: Contracts::new(50),
        }],
        vec![PriceLevel {
            price: Cents::new(55),
            qty: Contracts::new(50),
        }],
    )
    .unwrap();
    v
}

fn gated(
    seed: u64,
    side: Side,
    price: i64,
    qty: i64,
    clock: &SimClock,
) -> fortuna_gates::GatedOrder {
    let cfg: GateConfig = toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 10000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 10000000
        per_event_exposure_cents = 10000000
        require_event_mapping = false

        [per_strategy.s1]
        max_exposure_cents = 10000000
        max_order_notional_cents = 10000000
        min_net_edge_bps = 0

        [rate.sim]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap();
    let mut pipeline = GatePipeline::new(cfg).unwrap();
    let mut g = IdGen::new(seed);
    let intent = IntentId::new(g.next(t0()).unwrap());
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("s1").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market: MarketId::new("M1").unwrap(),
        side,
        action: Action::Buy,
        limit_price: Cents::new(price),
        qty: Contracts::new(qty),
        fair_value: Cents::new(price + 5),
        client_order_id: ClientOrderId::from_intent(intent),
    };
    let fees = fee_model();
    let recent = BTreeSet::new();
    let inputs = GateInputs {
        now: clock.now(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: None,
        last_trade_price: Some(Cents::new(50)),
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    pipeline.evaluate(&candidate, &inputs).gated.unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn pg_journal_survives_crash_and_recovers_identically(pool: PgPool) {
    let clock = clock();
    let venue = sim_venue(clock.clone());

    // Life before the crash: submit a resting order and a crossing order,
    // ingest the crossing fill.
    let journal = PgIntentJournal::new(pool.clone(), "sim", clock.clone());
    let mut m = OrderManager::recover(journal, clock.clone(), ExecPolicy::default())
        .await
        .unwrap();
    let resting = gated(1, Side::Yes, 40, 5, &clock);
    let resting_intent = resting.intent_id();
    m.submit(resting, &venue).await.unwrap();

    // NO-side so the one-working-order rule (same strategy/market/side)
    // does not refuse it; buys NO at 55 against the yes bid 45 -> fills.
    let crossing = gated(2, Side::No, 55, 5, &clock);
    let crossing_intent = crossing.intent_id();
    let out = m.submit(crossing, &venue).await.unwrap();
    assert!(matches!(out, SubmitOutcome::Acked { .. }));
    let page = venue
        .fills_since(fortuna_venues::Cursor::start())
        .await
        .unwrap();
    assert_eq!(page.fills.len(), 1);
    m.ingest_fill(&page.fills[0]).await.unwrap();
    let before = format!("{:?}", m.intents());
    drop(m); // CRASH: only Postgres survives.

    // Recovery: a fresh journal handle over the same database.
    let journal2 = PgIntentJournal::new(pool.clone(), "sim", clock.clone());
    let mut m2 = OrderManager::recover(journal2, clock.clone(), ExecPolicy::default())
        .await
        .unwrap();
    assert_eq!(before, format!("{:?}", m2.intents()));
    assert_eq!(
        m2.intent(crossing_intent).unwrap().status,
        IntentStatus::Filled
    );
    assert_eq!(
        m2.intent(resting_intent).unwrap().status,
        IntentStatus::Acked
    );

    // Boot reconciliation completes cleanly against the venue: the resting
    // order is still there (consistent), nothing to adopt or close.
    let report = m2.boot_reconcile(&venue).await.unwrap();
    assert!(report.adopted.is_empty());
    assert!(report.closed_unsubmitted.is_empty());
    assert!(report.orphans_cancelled.is_empty());
    assert_eq!(
        m2.intent(resting_intent).unwrap().status,
        IntentStatus::Acked
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn intent_events_refuse_mutation(pool: PgPool) {
    let clock = clock();
    let venue = sim_venue(clock.clone());
    let journal = PgIntentJournal::new(pool.clone(), "sim", clock.clone());
    let mut m = OrderManager::recover(journal, clock.clone(), ExecPolicy::default())
        .await
        .unwrap();
    m.submit(gated(3, Side::Yes, 40, 5, &clock), &venue)
        .await
        .unwrap();
    let res = sqlx::query("DELETE FROM intent_events")
        .execute(&pool)
        .await;
    assert!(res.unwrap_err().to_string().contains("append-only"));
}

// ---- repos ----

fn a_fill(n: u64) -> Fill {
    Fill {
        fill_id: format!("f-{n}"),
        venue_order_id: VenueOrderId::new(format!("v-{n}")).unwrap(),
        client_order_id: ClientOrderId::new(format!("c-{n}")).unwrap(),
        market: MarketId::new("M1").unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        price: Cents::new(50),
        qty: Contracts::new(5),
        fee: Cents::new(9),
        is_maker: false,
        at: t0(),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn fills_repo_dedups_on_fill_id(pool: PgPool) {
    let repo = FillsRepo::new(pool);
    assert!(repo.insert("sim", &a_fill(1)).await.unwrap());
    assert!(!repo.insert("sim", &a_fill(1)).await.unwrap()); // duplicate
    assert!(repo.insert("sim", &a_fill(2)).await.unwrap());
    assert_eq!(repo.count().await.unwrap(), 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn halts_fold_restores_only_unrearmed_flags(pool: PgPool) {
    let repo = HaltsRepo::new(pool);
    repo.record_set(&HaltScope::Global, "drawdown", "system", t0())
        .await
        .unwrap();
    repo.record_set(
        &HaltScope::Venue("kalshi".into()),
        "rate breach",
        "system",
        t0(),
    )
    .await
    .unwrap();
    repo.record_rearm(&HaltScope::Global, "operator re-arm", "xavier", t0())
        .await
        .unwrap();
    let active = repo.active().await.unwrap();
    // Only the venue halt survives the fold: I2 restore-at-boot input.
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].0, HaltScope::Venue("kalshi".into()));
    assert_eq!(active[0].1, "rate breach");
}

#[sqlx::test(migrations = "./migrations")]
async fn reservations_fold_to_active_set(pool: PgPool) {
    let repo = ReservationsRepo::new(pool);
    let mut g = IdGen::new(5);
    let i1 = IntentId::new(g.next(t0()).unwrap());
    let i2 = IntentId::new(g.next(t0()).unwrap());
    repo.record_reserve(i1, "s1", Cents::new(500), t0())
        .await
        .unwrap();
    repo.record_reserve(i2, "s1", Cents::new(300), t0())
        .await
        .unwrap();
    repo.record_release(i1, "s1", Cents::new(500), t0())
        .await
        .unwrap();
    let active = repo.active().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0], (i2, "s1".to_string(), Cents::new(300)));
}
