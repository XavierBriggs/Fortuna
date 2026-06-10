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

// ---- settlements + discrepancies repos (T1.4, spec 5.13) ----

#[sqlx::test(migrations = "./migrations")]
async fn settlement_chain_persists_as_superseding_rows(pool: PgPool) {
    let repo = fortuna_ledger::SettlementsRepo::new(pool);
    repo.insert_entry(
        "e-1",
        "KXS",
        "sim",
        1_000,
        "pending",
        None,
        &serde_json::json!({"winner": "Yes"}),
        "2026-06-10T13:00:00.000Z",
    )
    .await
    .unwrap();
    repo.insert_entry(
        "e-2",
        "KXS",
        "sim",
        1_000,
        "posted",
        Some("e-1"),
        &serde_json::json!({"winner": "Yes"}),
        "2026-06-10T13:00:01.000Z",
    )
    .await
    .unwrap();
    repo.insert_entry(
        "e-3",
        "KXS",
        "sim",
        1_000,
        "confirmed",
        Some("e-2"),
        &serde_json::json!({"winner": "Yes"}),
        "2026-06-10T13:00:02.000Z",
    )
    .await
    .unwrap();

    let chain = repo.chain("KXS").await.unwrap();
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0].status, "pending");
    assert_eq!(chain[2].status, "confirmed");
    assert_eq!(chain[2].supersedes.as_deref(), Some("e-2"));

    // Duplicate entry ids are refused (append-only, exactly-once rows).
    assert!(repo
        .insert_entry(
            "e-3",
            "KXS",
            "sim",
            1_000,
            "confirmed",
            Some("e-2"),
            &serde_json::json!({}),
            "2026-06-10T13:00:03.000Z",
        )
        .await
        .is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn discrepancies_open_and_resolve_append_only(pool: PgPool) {
    let repo = fortuna_ledger::DiscrepanciesRepo::new(pool);
    repo.open(
        "d-1",
        "position_mismatch",
        &serde_json::json!({"market": "KXS"}),
        "2026-06-10T13:00:00.000Z",
    )
    .await
    .unwrap();
    assert_eq!(repo.open_count().await.unwrap(), 1);

    repo.resolve(
        "r-1",
        "d-1",
        "adjustment",
        "books corrected after missed fill",
        Some("fill-99"),
        "2026-06-10T14:00:00.000Z",
    )
    .await
    .unwrap();
    assert_eq!(repo.open_count().await.unwrap(), 0);

    // Unknown disposition is refused by the schema CHECK.
    repo.open(
        "d-2",
        "balance_drift",
        &serde_json::json!({}),
        "2026-06-10T15:00:00.000Z",
    )
    .await
    .unwrap();
    assert!(repo
        .resolve(
            "r-2",
            "d-2",
            "wished_away",
            "no",
            None,
            "2026-06-10T15:01:00.000Z"
        )
        .await
        .is_err());
}

// ---- events + edges + price snapshots repos (T2.1, spec 5.12 + 5.5) ----

#[sqlx::test(migrations = "./migrations")]
async fn events_create_transition_and_dead_states_persist(pool: PgPool) {
    let repo = fortuna_ledger::EventsRepo::new(pool);
    repo.create(
        "evt-1",
        "Team A beats Team B",
        "official final score",
        "league.example",
        Some("2026-06-21T00:00:00.000Z"),
        "2026-06-20T18:00:00.000Z",
        "sports",
        "2026-06-10T12:00:00.000Z",
    )
    .await
    .unwrap();

    repo.set_status("evt-1", "active").await.unwrap();
    repo.set_status("evt-1", "resolution_pending")
        .await
        .unwrap();
    let row = repo.get("evt-1").await.unwrap();
    assert_eq!(row.status, "resolution_pending");
    assert!(!row.unscoreable);

    // Unknown status refused by the schema CHECK.
    assert!(repo.set_status("evt-1", "vibing").await.is_err());

    repo.mark_dead("evt-1", "source_lost").await.unwrap();
    let row = repo.get("evt-1").await.unwrap();
    assert_eq!(row.status, "dead");
    assert_eq!(row.dead_reason.as_deref(), Some("source_lost"));

    repo.mark_unscoreable("evt-1").await.unwrap();
    assert!(repo.get("evt-1").await.unwrap().unscoreable);
}

#[sqlx::test(migrations = "./migrations")]
async fn edges_propose_confirm_by_superseding_row(pool: PgPool) {
    let events = fortuna_ledger::EventsRepo::new(pool.clone());
    events
        .create(
            "evt-1",
            "s",
            "c",
            "src",
            None,
            "2026-06-20T18:00:00.000Z",
            "sports",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    let repo = fortuna_ledger::EdgesRepo::new(pool);

    repo.insert_edge(
        "edge-1",
        "KXTEAM-A",
        "kalshi",
        "evt-1",
        "direct",
        0.9,
        "model:stub",
        None,
        None,
        "2026-06-10T12:01:00.000Z",
    )
    .await
    .unwrap();

    // Confirmation INSERTS a superseding row (append-only discipline).
    repo.insert_edge(
        "edge-2",
        "KXTEAM-A",
        "kalshi",
        "evt-1",
        "direct",
        0.9,
        "model:stub",
        Some("operator:xavier"),
        Some("edge-1"),
        "2026-06-10T13:00:00.000Z",
    )
    .await
    .unwrap();

    // current_edges resolves heads: superseded rows drop out.
    let heads = repo.current_edges_for_event("evt-1").await.unwrap();
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].edge_id, "edge-2");
    assert_eq!(heads[0].confirmed_by.as_deref(), Some("operator:xavier"));
    assert_eq!(heads[0].supersedes.as_deref(), Some("edge-1"));
}

#[sqlx::test(migrations = "./migrations")]
async fn snapshots_insert_and_latest_liquid_pre_benchmark_query(pool: PgPool) {
    let events = fortuna_ledger::EventsRepo::new(pool.clone());
    events
        .create(
            "evt-1",
            "s",
            "c",
            "src",
            None,
            "2026-06-20T18:00:00.000Z",
            "sports",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    let repo = fortuna_ledger::SnapshotsRepo::new(pool);

    // Older liquid, newer ILLIQUID, post-benchmark liquid.
    repo.insert(
        "snap-1",
        "KXTEAM-A",
        "kalshi",
        Some("evt-1"),
        "t24h",
        Some(40),
        Some(43),
        Some(50),
        Some(50),
        true,
        "2026-06-19T18:00:00.000Z",
    )
    .await
    .unwrap();
    repo.insert(
        "snap-2",
        "KXTEAM-A",
        "kalshi",
        Some("evt-1"),
        "t1h",
        Some(60),
        Some(95),
        Some(50),
        Some(50),
        false,
        "2026-06-20T17:00:00.000Z",
    )
    .await
    .unwrap();
    repo.insert(
        "snap-3",
        "KXTEAM-A",
        "kalshi",
        Some("evt-1"),
        "other",
        Some(70),
        Some(72),
        Some(50),
        Some(50),
        true,
        "2026-06-20T19:00:00.000Z",
    )
    .await
    .unwrap();

    let best = repo
        .latest_liquid_before("KXTEAM-A", "evt-1", "2026-06-20T18:00:00.000Z")
        .await
        .unwrap()
        .expect("the t24h snapshot qualifies");
    assert_eq!(best.snapshot_id, "snap-1");
    assert_eq!(best.best_bid_cents, Some(40));

    // Nothing qualifies before a too-early cutoff.
    assert!(repo
        .latest_liquid_before("KXTEAM-A", "evt-1", "2026-06-19T00:00:00.000Z")
        .await
        .unwrap()
        .is_none());
}

// ---- signals + source registry repos (T2.2, spec 5.11) ----

#[sqlx::test(migrations = "./migrations")]
async fn signals_append_only_with_per_source_dedup_index_rebuild(pool: PgPool) {
    let repo = fortuna_ledger::SignalsRepo::new(pool);
    repo.insert(
        "sig-1",
        "aeolus",
        "aeolus_run",
        "2026-06-10T12:00:00.000Z",
        "hash-a",
        &serde_json::json!({"station": "KNYC"}),
    )
    .await
    .unwrap();
    repo.insert(
        "sig-2",
        "aeolus",
        "aeolus_run",
        "2026-06-10T13:00:00.000Z",
        "hash-b",
        &serde_json::json!({"station": "KBOS"}),
    )
    .await
    .unwrap();
    repo.insert(
        "sig-3",
        "rss-nws",
        "news_item",
        "2026-06-10T13:00:00.000Z",
        "hash-a",
        &serde_json::json!({"t": "x"}),
    )
    .await
    .unwrap();

    assert_eq!(repo.count().await.unwrap(), 3);

    // The boot-time dedup index: (source, content_hash) pairs.
    let pairs = repo.dedup_pairs().await.unwrap();
    assert_eq!(pairs.len(), 3);
    assert!(pairs.contains(&("aeolus".to_string(), "hash-a".to_string())));
    assert!(pairs.contains(&("rss-nws".to_string(), "hash-a".to_string())));
}

#[sqlx::test(migrations = "./migrations")]
async fn source_registry_upserts_and_loads_allowlist(pool: PgPool) {
    let repo = fortuna_ledger::SourceRegistryRepo::new(pool);
    repo.upsert(
        "aeolus",
        7,
        &["weather".to_string()],
        true,
        "2026-06-10T12:00:00.000Z",
    )
    .await
    .unwrap();
    repo.upsert("sketchy-blog", 1, &[], false, "2026-06-10T12:00:00.000Z")
        .await
        .unwrap();
    // Demotion on the record: an upsert updates tier + updated_at.
    repo.upsert(
        "aeolus",
        4,
        &["weather".to_string()],
        true,
        "2026-06-11T12:00:00.000Z",
    )
    .await
    .unwrap();

    let entries = repo.load_all().await.unwrap();
    assert_eq!(entries.len(), 2);
    let aeolus = entries.iter().find(|e| e.source_id == "aeolus").unwrap();
    assert_eq!(aeolus.trust_tier, 4, "demotion persisted");
    assert!(aeolus.enabled);
    let blog = entries
        .iter()
        .find(|e| e.source_id == "sketchy-blog")
        .unwrap();
    assert!(!blog.enabled);

    // Tier outside the schema CHECK is refused by the database.
    assert!(repo
        .upsert("aeolus", 11, &[], true, "2026-06-12T12:00:00.000Z")
        .await
        .is_err());
}

// ---- beliefs repo (T2.3, spec 5.5) ----

async fn seed_event(pool: &PgPool, id: &str) {
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            id,
            "s",
            "c",
            "src",
            None,
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn beliefs_insert_supersede_and_score_exactly_once(pool: PgPool) {
    seed_event(&pool, "evt-1").await;
    let repo = fortuna_ledger::BeliefsRepo::new(pool);

    repo.insert(
        "b-1",
        "2026-06-10T12:00:00.000Z",
        "evt-1",
        0.62,
        0.65,
        "2026-06-20T18:00:00.000Z",
        &serde_json::json!([{"source": "aeolus"}]),
        &serde_json::json!({"model_id": "stub"}),
        None,
    )
    .await
    .unwrap();

    // An update is a NEW row superseding the old; the old flips status.
    repo.insert(
        "b-2",
        "2026-06-10T13:00:00.000Z",
        "evt-1",
        0.70,
        0.72,
        "2026-06-20T18:00:00.000Z",
        &serde_json::json!([{"source": "aeolus"}]),
        &serde_json::json!({"model_id": "stub"}),
        Some("b-1"),
    )
    .await
    .unwrap();
    let b1 = repo.get("b-1").await.unwrap();
    assert_eq!(b1.status, "superseded");
    let b2 = repo.get("b-2").await.unwrap();
    assert_eq!(b2.status, "open");
    assert_eq!(b2.supersedes.as_deref(), Some("b-1"));

    // Content is immutable at the DATABASE (the T0.8 guard).
    assert!(repo.try_mutate_content_for_test("b-2", 0.99).await.is_err());

    // Scoring fills outcome/brier/clv exactly once.
    repo.resolve_and_score("b-2", true, 0.09, Some(1_250.0))
        .await
        .unwrap();
    let b2 = repo.get("b-2").await.unwrap();
    assert_eq!(b2.status, "resolved");
    assert_eq!(b2.outcome, Some(1));
    assert!((b2.brier.unwrap() - 0.09).abs() < 1e-9);
    assert!((b2.clv_bps.unwrap() - 1_250.0).abs() < 1e-9);

    // A second scoring attempt is refused (score-once).
    assert!(repo
        .resolve_and_score("b-2", false, 0.49, None)
        .await
        .is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn beliefs_abandon_on_event_death_excludes_from_calibration(pool: PgPool) {
    seed_event(&pool, "evt-1").await;
    let repo = fortuna_ledger::BeliefsRepo::new(pool);
    repo.insert(
        "b-1",
        "2026-06-10T12:00:00.000Z",
        "evt-1",
        0.62,
        0.65,
        "2026-06-20T18:00:00.000Z",
        &serde_json::json!([]),
        &serde_json::json!({"model_id": "stub"}),
        None,
    )
    .await
    .unwrap();

    repo.abandon_open_for_event("evt-1").await.unwrap();
    let b1 = repo.get("b-1").await.unwrap();
    assert_eq!(b1.status, "abandoned");
    assert!(
        b1.outcome.is_none(),
        "abandoned is scored neither right nor wrong"
    );

    // Calibration samples = RESOLVED beliefs only.
    let samples = repo.resolved_samples("weather").await.unwrap();
    assert!(samples.is_empty());
}

// ---- journal repo (T2.7, spec 5.6/5.8) ----

#[sqlx::test(migrations = "./migrations")]
async fn journal_one_entry_per_day_append_only(pool: PgPool) {
    let repo = fortuna_ledger::JournalRepo::new(pool);
    repo.insert(
        "j-1",
        "2026-06-11",
        &serde_json::json!({"body": "3 fills; tomorrow watch KXHIGHNY", "manifest_hash": "abc"}),
        "2026-06-12T00:00:05.000Z",
    )
    .await
    .unwrap();

    // One journal per day (the unique index): a second insert refuses.
    assert!(repo
        .insert(
            "j-2",
            "2026-06-11",
            &serde_json::json!({"body": "dup"}),
            "2026-06-12T00:00:06.000Z",
        )
        .await
        .is_err());

    let row = repo.get_day("2026-06-11").await.unwrap().unwrap();
    assert_eq!(row.journal_id, "j-1");
    assert!(row.body["body"].as_str().unwrap().contains("KXHIGHNY"));
    assert!(repo.get_day("2026-06-10").await.unwrap().is_none());
}

// ---- calibration params repo (T2.8, spec 5.10) ----

#[sqlx::test(migrations = "./migrations")]
async fn calibration_params_are_versioned_append_only_config(pool: PgPool) {
    let repo = fortuna_ledger::CalibrationParamsRepo::new(pool);
    let scope = ("claude-fable-5", "synthesis", "weather");

    // Version 1 lands.
    repo.insert(
        "cp-1",
        scope.0,
        scope.1,
        scope.2,
        "platt",
        &serde_json::json!({"version": 1, "method": {"Platt": {"a": 0.39, "b": 0.0}},
                            "extremization_k": 1.0, "fitted_on_n": 80}),
        1,
        "2026-06-11T00:00:00.000Z",
        "2026-06-11T00:00:00.000Z",
    )
    .await
    .unwrap();

    // A parameter UPDATE is a new VERSION row, never a mutation.
    repo.insert(
        "cp-2",
        scope.0,
        scope.1,
        scope.2,
        "platt",
        &serde_json::json!({"version": 2, "method": {"Platt": {"a": 0.46, "b": 0.02}},
                            "extremization_k": 1.0, "fitted_on_n": 140}),
        2,
        "2026-06-18T00:00:00.000Z",
        "2026-06-18T00:00:00.000Z",
    )
    .await
    .unwrap();

    let latest = repo
        .latest(scope.0, scope.1, scope.2, "platt")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.param_id, "cp-2");
    assert_eq!(latest.version, 2);
    assert!((latest.params["method"]["Platt"]["a"].as_f64().unwrap() - 0.46).abs() < 1e-12);

    // Re-issuing an existing (scope, version) is refused (UNIQUE).
    assert!(repo
        .insert(
            "cp-3",
            scope.0,
            scope.1,
            scope.2,
            "platt",
            &serde_json::json!({}),
            2,
            "2026-06-19T00:00:00.000Z",
            "2026-06-19T00:00:00.000Z",
        )
        .await
        .is_err());

    // Unknown kinds are refused by the schema CHECK.
    assert!(repo
        .insert(
            "cp-4",
            scope.0,
            scope.1,
            scope.2,
            "voodoo",
            &serde_json::json!({}),
            1,
            "2026-06-19T00:00:00.000Z",
            "2026-06-19T00:00:00.000Z",
        )
        .await
        .is_err());

    // Scopes are independent; an unseen scope has no params.
    repo.insert(
        "cp-5",
        scope.0,
        scope.1,
        "sports",
        "isotonic",
        &serde_json::json!({"steps": []}),
        1,
        "2026-06-19T00:00:00.000Z",
        "2026-06-19T00:00:00.000Z",
    )
    .await
    .unwrap();
    let weather = repo
        .latest(scope.0, scope.1, "weather", "platt")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(weather.version, 2, "other scopes do not bleed in");
    assert!(repo
        .latest("other-model", scope.1, scope.2, "platt")
        .await
        .unwrap()
        .is_none());
}

// ---- aeolus_eval end to end (Phase 2 EXIT, spec Section 6 item 3) ----

/// The full aeolus_eval path: fixture envelope -> strict contract parse
/// -> ZERO-CAPITAL belief drafts -> persisted -> resolved & scored ->
/// the calibration record sees them. No order type exists anywhere in
/// this flow; the signal is evaluated, never traded.
#[sqlx::test(migrations = "./migrations")]
async fn aeolus_eval_writes_scored_beliefs_from_the_fixture_envelope(pool: PgPool) {
    use fortuna_cognition::beliefs::calibration_curve;
    use fortuna_cognition::calibration::calibration_quality;
    use fortuna_cognition::reconciliation::{map_aeolus_envelope, AeolusEnvelope};
    use fortuna_core::clock::UtcTimestamp;

    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/aeolus/sample_envelope.json"
    ))
    .unwrap();
    let envelope: AeolusEnvelope = serde_json::from_str(&fixture).unwrap();
    let horizon = UtcTimestamp::parse_iso8601("2026-06-12T23:00:00.000Z").unwrap();
    let drafts = map_aeolus_envelope(&envelope, horizon).unwrap();
    assert_eq!(drafts.len(), 3, "every bracket becomes a belief draft");

    // The composition persists drafts as beliefs on aeolus-namespaced
    // events (created by the events pipeline in the live composition).
    let events = fortuna_ledger::EventsRepo::new(pool.clone());
    let beliefs = fortuna_ledger::BeliefsRepo::new(pool.clone());
    for (i, draft) in drafts.iter().enumerate() {
        events
            .create(
                &draft.event_id,
                &format!("NYC high in bracket {i}"),
                "NWS daily climate report",
                "nws",
                Some("2026-06-12T23:00:00.000Z"),
                "2026-06-11T10:00:00.000Z",
                "weather",
                "2026-06-11T10:00:00.000Z",
            )
            .await
            .unwrap();
        beliefs
            .insert(
                &format!("b-aeolus-{i}"),
                "2026-06-11T10:00:00.000Z",
                &draft.event_id,
                draft.p,
                draft.p_raw,
                "2026-06-12T23:00:00.000Z",
                &draft.evidence,
                &draft.provenance,
                None,
            )
            .await
            .unwrap();
    }

    // Settlement day: the t65 bracket verified; the others did not.
    // Brier = (p - outcome)^2, computed by the scoring composition.
    let outcomes = [false, true, false];
    for (i, (draft, outcome)) in drafts.iter().zip(outcomes).enumerate() {
        let target = f64::from(u8::from(outcome));
        let brier = (draft.p - target) * (draft.p - target);
        beliefs
            .resolve_and_score(&format!("b-aeolus-{i}"), outcome, brier, None)
            .await
            .unwrap();
    }

    // The scored record is queryable as calibration input and the
    // quality factor sees it (tiny n => heavily ramped down: honesty).
    let samples = beliefs.resolved_samples("weather").await.unwrap();
    assert_eq!(samples.len(), 3);
    let curve = calibration_curve(&samples, 10);
    assert!(!curve.is_empty());
    let quality = calibration_quality(&curve, samples.len());
    assert!(quality > 0.0, "scored record must register");
    assert!(
        quality <= 3.0 / 50.0 + 1e-9,
        "n=3 stays ramped down (low-data honesty), got {quality}"
    );

    // Zero capital is structural: nothing in this test ever touched an
    // order, an intent, or a venue — there is no API surface for it.
    let b1 = beliefs.get("b-aeolus-1").await.unwrap();
    assert_eq!(b1.status, "resolved");
    assert_eq!(b1.outcome, Some(1));
}
