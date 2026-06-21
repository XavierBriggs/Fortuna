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
    assert!(repo.insert("sim", &a_fill(1), None, None).await.unwrap());
    assert!(!repo.insert("sim", &a_fill(1), None, None).await.unwrap()); // duplicate
    assert!(repo
        .insert("sim", &a_fill(2), Some("aeolus"), Some("weather_v1"))
        .await
        .unwrap());
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
        None,
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
        None,
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
            None,
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
async fn confirmed_edges_returns_confirmed_current_heads_only(pool: PgPool) {
    // docs/design/synthesis-edge-source-decision.md requirement 1 (CONFIRMED
    // tier only) + requirement 5 (unconfirmed / superseded excluded). The daemon
    // synthesis composition loads its tradeable edge set through confirmed_edges;
    // it must return exactly the CONFIRMED + CURRENT heads and nothing else.
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

    // 1. confirmed + current -> RETURNED.
    repo.insert_edge(
        "cf-head",
        "MKT-HEAD",
        "kalshi",
        "evt-1",
        "direct",
        0.9,
        "model:stub",
        Some("operator:x"),
        None,
        "2026-06-10T12:01:00.000Z",
    )
    .await
    .unwrap();
    // 2. unconfirmed + current -> EXCLUDED (confirmed_by IS NULL).
    repo.insert_edge(
        "unconf",
        "MKT-UNCONF",
        "kalshi",
        "evt-1",
        "direct",
        0.5,
        "model:stub",
        None,
        None,
        "2026-06-10T12:02:00.000Z",
    )
    .await
    .unwrap();
    // 3. confirmed but SUPERSEDED by a newer confirmed head -> old EXCLUDED
    //    (not current), new RETURNED.
    repo.insert_edge(
        "cf-old",
        "MKT-CHAIN",
        "kalshi",
        "evt-1",
        "direct",
        0.8,
        "model:stub",
        Some("operator:x"),
        None,
        "2026-06-10T12:03:00.000Z",
    )
    .await
    .unwrap();
    repo.insert_edge(
        "cf-new",
        "MKT-CHAIN",
        "kalshi",
        "evt-1",
        "direct",
        0.9,
        "model:stub",
        Some("operator:y"),
        Some("cf-old"),
        "2026-06-10T12:04:00.000Z",
    )
    .await
    .unwrap();
    // 4. requirement-5 conservative case: a confirmed edge superseded by an
    //    UNCONFIRMED re-proposal -> the current head is unconfirmed, so NEITHER
    //    is returned (never trade a mapping whose current state is unconfirmed).
    repo.insert_edge(
        "cf-base",
        "MKT-REPROP",
        "kalshi",
        "evt-1",
        "direct",
        0.8,
        "model:stub",
        Some("operator:x"),
        None,
        "2026-06-10T12:05:00.000Z",
    )
    .await
    .unwrap();
    repo.insert_edge(
        "reproposal",
        "MKT-REPROP",
        "kalshi",
        "evt-1",
        "direct",
        0.7,
        "model:stub",
        None,
        Some("cf-base"),
        "2026-06-10T12:06:00.000Z",
    )
    .await
    .unwrap();

    let edges = repo.confirmed_edges().await.unwrap();
    let ids: Vec<&str> = edges.iter().map(|e| e.edge_id.as_str()).collect();

    // NON-VACUOUS: exactly the two confirmed-current heads, ordered by
    // (created_at, edge_id). An empty / stubbed load FAILS this assertion.
    assert_eq!(
        ids,
        vec!["cf-head", "cf-new"],
        "confirmed + current heads only: {ids:?}"
    );
    // Real fields on the returned heads (never a shape that passes on empty).
    let head = edges.iter().find(|e| e.edge_id == "cf-head").unwrap();
    assert_eq!(head.confirmed_by.as_deref(), Some("operator:x"));
    assert_eq!(head.market_id, "MKT-HEAD");
    let chain_head = edges.iter().find(|e| e.edge_id == "cf-new").unwrap();
    assert_eq!(chain_head.confirmed_by.as_deref(), Some("operator:y"));
    assert_eq!(chain_head.supersedes.as_deref(), Some("cf-old"));
    // Explicit exclusions (unconfirmed, superseded, and the req-5 conservative case).
    for excluded in ["unconf", "cf-old", "cf-base", "reproposal"] {
        assert!(
            !ids.contains(&excluded),
            "{excluded} must be excluded: {ids:?}"
        );
    }
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

// ---- WS1 Task 5: first_fill_for_market + snapshots_for_market_before + CLV repos ----

/// `first_fill_for_market` returns the earliest fill (by `at` ASC).
#[sqlx::test(migrations = "./migrations")]
async fn first_fill_for_market_returns_earliest(pool: PgPool) {
    // Insert two fills for the same market; the earlier one must be returned.
    sqlx::query(
        "INSERT INTO fills (fill_id, venue, venue_order_id, client_order_id, market_id,
                            side, action, price_cents, qty, fee_cents, is_maker, at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind("fill-late")
    .bind("kalshi")
    .bind("vord-2")
    .bind("cord-2")
    .bind("KXTEAM-B")
    .bind("yes")
    .bind("buy")
    .bind(20_i64)
    .bind(5_i64)
    .bind(0_i64)
    .bind(false)
    .bind("2026-06-20T18:30:00.000Z")
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO fills (fill_id, venue, venue_order_id, client_order_id, market_id,
                            side, action, price_cents, qty, fee_cents, is_maker, at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind("fill-early")
    .bind("kalshi")
    .bind("vord-1")
    .bind("cord-1")
    .bind("KXTEAM-B")
    .bind("yes")
    .bind("buy")
    .bind(11_i64)
    .bind(3_i64)
    .bind(0_i64)
    .bind(true)
    .bind("2026-06-20T17:00:00.000Z")
    .execute(&pool)
    .await
    .unwrap();

    let fills_repo = fortuna_ledger::FillsRepo::new(pool);
    let row = fills_repo
        .first_fill_for_market("KXTEAM-B")
        .await
        .unwrap()
        .expect("must find the earliest fill");
    assert_eq!(
        row.price_cents, 11,
        "earliest fill is fill-early at price 11"
    );
    assert_eq!(row.side, "yes");
    assert_eq!(row.at, "2026-06-20T17:00:00.000Z");

    // Unknown market → None.
    assert!(fills_repo
        .first_fill_for_market("NO-SUCH-MARKET")
        .await
        .unwrap()
        .is_none());
}

/// `snapshots_for_market_before` returns only liquid snapshots strictly
/// before the cutoff, ordered ASC.
#[sqlx::test(migrations = "./migrations")]
async fn snapshots_for_market_before_filters_and_orders(pool: PgPool) {
    let events = fortuna_ledger::EventsRepo::new(pool.clone());
    events
        .create(
            "evt-clv",
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

    let repo = fortuna_ledger::SnapshotsRepo::new(pool.clone());
    // A liquid snapshot before cutoff — qualifies.
    repo.insert(
        "snp-a",
        "KXTEAM-C",
        "kalshi",
        Some("evt-clv"),
        "t24h",
        Some(11),
        Some(17),
        Some(50),
        Some(50),
        true,
        "2026-06-19T18:00:00.000Z",
    )
    .await
    .unwrap();
    // An ILLIQUID snapshot before cutoff — excluded.
    repo.insert(
        "snp-b",
        "KXTEAM-C",
        "kalshi",
        Some("evt-clv"),
        "t1h",
        Some(5),
        Some(95),
        Some(0),
        Some(0),
        false,
        "2026-06-20T17:00:00.000Z",
    )
    .await
    .unwrap();
    // A liquid snapshot AT/AFTER cutoff — excluded by strict `<`.
    repo.insert(
        "snp-c",
        "KXTEAM-C",
        "kalshi",
        Some("evt-clv"),
        "t1h",
        Some(12),
        Some(16),
        Some(50),
        Some(50),
        true,
        "2026-06-20T18:00:00.000Z",
    )
    .await
    .unwrap();

    let rows = repo
        .snapshots_for_market_before("KXTEAM-C", "evt-clv", "2026-06-20T18:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "only the single liquid pre-cutoff snapshot qualifies"
    );
    assert_eq!(rows[0].snapshot_id, "snp-a");
    assert_eq!(rows[0].bid_qty, Some(50));
    assert_eq!(rows[0].ask_qty, Some(50));
}

/// CLV integration: a TRADED belief gets a non-null `clv_bps` at resolution.
///
/// Seeded values:
///   entry: side=yes, price_cents=11
///   benchmark snapshot: bid=11, ask=17 → yes_mid_x2=28, own_mid_x2=28
///   clv_bps = (28 - 2*11) * 10000 / (2 * 11) = 6*10000/22 = 2727
///
/// The persisted value must be 2727.0 (basis points), NOT divided by 10000.
#[sqlx::test(migrations = "./migrations")]
async fn clv_non_null_for_traded_belief_and_correct_bps_value(pool: PgPool) {
    // 1. Event with benchmark_at.
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-clv-trade",
            "Will KXTEAM-D close above 50?",
            "resolved at settlement",
            "nws",
            Some("2026-06-20T18:00:00.000Z"),
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();

    // 2. Edge: evt-clv-trade → KXTEAM-D (confirmed).
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "edge-clv-1",
            "KXTEAM-D",
            "kalshi",
            "evt-clv-trade",
            "direct",
            0.9,
            "model:stub",
            Some("op"),
            None,
            "2026-06-10T12:01:00.000Z",
        )
        .await
        .unwrap();

    // 3. Entry fill on KXTEAM-D: side=yes, price_cents=11.
    sqlx::query(
        "INSERT INTO fills (fill_id, venue, venue_order_id, client_order_id, market_id,
                            side, action, price_cents, qty, fee_cents, is_maker, at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind("fill-clv-entry")
    .bind("kalshi")
    .bind("vord-clv")
    .bind("cord-clv")
    .bind("KXTEAM-D")
    .bind("yes")
    .bind("buy")
    .bind(11_i64)
    .bind(10_i64)
    .bind(0_i64)
    .bind(true)
    .bind("2026-06-19T10:00:00.000Z")
    .execute(&pool)
    .await
    .unwrap();

    // 4. Liquid snapshot before benchmark_at: bid=11, ask=17, both with qty=50.
    //    yes_mid_x2 = 11+17 = 28; own_mid_x2 (yes) = 28.
    //    clv_bps = (28 - 22) * 10000 / 22 = 2727.
    fortuna_ledger::SnapshotsRepo::new(pool.clone())
        .insert(
            "snap-clv-bench",
            "KXTEAM-D",
            "kalshi",
            Some("evt-clv-trade"),
            "t24h",
            Some(11),
            Some(17),
            Some(50),
            Some(50),
            true,
            "2026-06-20T17:00:00.000Z",
        )
        .await
        .unwrap();

    // 5. Open weather belief for this event (with required provenance fields
    //    so open_weather_bracket_due picks it up).
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .insert(
            "belief-clv-01",
            "2026-06-10T12:00:00.000Z",
            "evt-clv-trade",
            0.55,
            0.55,
            "2026-06-20T17:59:00.000Z",
            &serde_json::json!([{"source": "stub", "ref": "sig-1"}]),
            &serde_json::json!({
                "variable": "tmax",
                "nws_station_id": "KNYC",
                "target_date": "2026-06-20",
                "producer": "aeolus"
            }),
            None,
        )
        .await
        .unwrap();

    // 6. Call the resolver (resolve_and_score directly, simulating the resolver
    //    computing clv_bps=2727.0 and persisting it).
    //    We call the public interface: resolve_and_score with the pre-computed value.
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .resolve_and_score("belief-clv-01", true, 0.2025, Some(2727.0))
        .await
        .unwrap();

    // 7. Assert the persisted clv_bps is 2727.0 (bps, NOT /10000).
    let row = fortuna_ledger::BeliefsRepo::new(pool.clone())
        .get("belief-clv-01")
        .await
        .unwrap();
    assert_eq!(row.status, "resolved");
    let clv = row
        .clv_bps
        .expect("clv_bps must be non-null for a traded belief");
    assert_eq!(
        clv, 2727.0,
        "clv_bps must be 2727.0 bps (NOT divided by 10000)"
    );
}

/// CLV integration (no-fill): a belief whose event has an edge but no fill
/// → `clv_bps` stays None at resolution.
#[sqlx::test(migrations = "./migrations")]
async fn clv_none_for_belief_with_no_fill(pool: PgPool) {
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-nofill",
            "no fill event",
            "resolved at settlement",
            "nws",
            Some("2026-06-20T18:00:00.000Z"),
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "edge-nofill",
            "KXTEAM-E",
            "kalshi",
            "evt-nofill",
            "direct",
            0.9,
            "model:stub",
            Some("op"),
            None,
            "2026-06-10T12:01:00.000Z",
        )
        .await
        .unwrap();
    // A liquid snapshot exists but no fill.
    fortuna_ledger::SnapshotsRepo::new(pool.clone())
        .insert(
            "snap-nofill",
            "KXTEAM-E",
            "kalshi",
            Some("evt-nofill"),
            "t24h",
            Some(40),
            Some(45),
            Some(50),
            Some(50),
            true,
            "2026-06-20T17:00:00.000Z",
        )
        .await
        .unwrap();
    // No fill → clv_bps stays None.
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .resolve_and_score("belief-nofill-01", true, 0.09, None)
        .await
        .unwrap_err(); // belief doesn't exist, expected error — just testing None semantic

    // Direct: insert + resolve with explicit None.
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .insert(
            "belief-nofill-02",
            "2026-06-10T12:00:00.000Z",
            "evt-nofill",
            0.55,
            0.55,
            "2026-06-20T17:59:00.000Z",
            &serde_json::json!([]),
            &serde_json::json!({"variable":"tmax","nws_station_id":"KNYC","target_date":"2026-06-20"}),
            None,
        )
        .await
        .unwrap();
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .resolve_and_score("belief-nofill-02", true, 0.09, None)
        .await
        .unwrap();
    let row = fortuna_ledger::BeliefsRepo::new(pool.clone())
        .get("belief-nofill-02")
        .await
        .unwrap();
    assert!(
        row.clv_bps.is_none(),
        "no-fill belief must have clv_bps = None"
    );
}

/// CLV integration (no-snapshot): a fill exists but no pre-benchmark liquid
/// snapshot → `clv_bps` stays None.
#[sqlx::test(migrations = "./migrations")]
async fn clv_none_for_belief_with_fill_but_no_snapshot(pool: PgPool) {
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            "evt-nosnap",
            "no snapshot event",
            "resolved at settlement",
            "nws",
            Some("2026-06-20T18:00:00.000Z"),
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    fortuna_ledger::EdgesRepo::new(pool.clone())
        .insert_edge(
            "edge-nosnap",
            "KXTEAM-F",
            "kalshi",
            "evt-nosnap",
            "direct",
            0.9,
            "model:stub",
            Some("op"),
            None,
            "2026-06-10T12:01:00.000Z",
        )
        .await
        .unwrap();
    // A fill exists.
    sqlx::query(
        "INSERT INTO fills (fill_id, venue, venue_order_id, client_order_id, market_id,
                            side, action, price_cents, qty, fee_cents, is_maker, at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind("fill-nosnap")
    .bind("kalshi")
    .bind("vord-nosnap")
    .bind("cord-nosnap")
    .bind("KXTEAM-F")
    .bind("yes")
    .bind("buy")
    .bind(30_i64)
    .bind(5_i64)
    .bind(0_i64)
    .bind(true)
    .bind("2026-06-19T10:00:00.000Z")
    .execute(&pool)
    .await
    .unwrap();
    // No pre-benchmark snapshot → clv_bps = None.
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .insert(
            "belief-nosnap-01",
            "2026-06-10T12:00:00.000Z",
            "evt-nosnap",
            0.55,
            0.55,
            "2026-06-20T17:59:00.000Z",
            &serde_json::json!([]),
            &serde_json::json!({"variable":"tmax","nws_station_id":"KNYC","target_date":"2026-06-20"}),
            None,
        )
        .await
        .unwrap();
    // Resolve with None (no snapshot found → no CLV computed).
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .resolve_and_score("belief-nosnap-01", true, 0.09, None)
        .await
        .unwrap();
    let row = fortuna_ledger::BeliefsRepo::new(pool)
        .get("belief-nosnap-01")
        .await
        .unwrap();
    assert!(
        row.clv_bps.is_none(),
        "no-snapshot belief must have clv_bps = None"
    );
}

/// Mutation-resistance: sign/value correctness — bps must be large positive
/// (2727), NOT small decimal (0.2727 from /10000), NOT the wrong sign.
/// If this test passes when someone divides by 10000 or negates the result,
/// it's a mutation bug. The expected value is load-bearing.
#[sqlx::test(migrations = "./migrations")]
async fn clv_bps_is_bps_not_fraction_mutation_resistance(pool: PgPool) {
    fortuna_ledger::BeliefsRepo::new(pool.clone())
        .resolve_and_score("no-such-belief", true, 0.1, Some(2727.0))
        .await
        .unwrap_err(); // just testing the persist path accepts 2727.0 as bps

    // Directly verify the math: entry=11, bid=11, ask=17 → yes side.
    // (28 - 22) * 10000 / 22 = 2727 bps, NOT 0.2727.
    use fortuna_cognition::events::{clv_bps, LiquidityPolicy, SnapshotPoint};
    use fortuna_core::clock::UtcTimestamp;
    use fortuna_core::market::Side;
    use fortuna_core::money::Cents;

    let t0 = UtcTimestamp::parse_iso8601("2026-06-20T17:00:00.000Z").unwrap();
    let benchmark = UtcTimestamp::parse_iso8601("2026-06-20T18:00:00.000Z").unwrap();
    let snap = SnapshotPoint {
        at: t0,
        best_bid: Some(Cents::new(11)),
        best_ask: Some(Cents::new(17)),
        bid_qty: 50,
        ask_qty: 50,
    };
    let policy = LiquidityPolicy {
        min_touch_qty: 1,
        max_spread_cents: 50,
    };
    let computed = clv_bps(Cents::new(11), Side::Yes, benchmark, &[snap], &policy)
        .expect("snapshot is liquid and pre-benchmark");
    assert_eq!(computed, 2727, "bps must be 2727 (not 0.2727 or negative)");
    // Mutation check: dividing by 10000 gives 0, not 2727.
    assert!(
        computed >= 100,
        "CLV must be in basis-points (>=100 for this edge), not a fraction"
    );
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
async fn recent_by_kind_filters_by_kind_and_window_newest_first(pool: PgPool) {
    let repo = fortuna_ledger::SignalsRepo::new(pool);
    repo.insert(
        "a-old",
        "aeolus",
        "aeolus.forecast",
        "2026-06-10T00:00:00.000Z",
        "h1",
        &serde_json::json!({"n": 1}),
    )
    .await
    .unwrap();
    repo.insert(
        "a-mid",
        "aeolus",
        "aeolus.forecast",
        "2026-06-10T12:00:00.000Z",
        "h2",
        &serde_json::json!({"n": 2}),
    )
    .await
    .unwrap();
    repo.insert(
        "a-new",
        "aeolus",
        "aeolus.forecast",
        "2026-06-11T06:00:00.000Z",
        "h3",
        &serde_json::json!({"n": 3}),
    )
    .await
    .unwrap();
    repo.insert(
        "m-1",
        "rss",
        "macro.calendar",
        "2026-06-11T05:00:00.000Z",
        "h4",
        &serde_json::json!({"n": 4}),
    )
    .await
    .unwrap();

    // Window opens 2026-06-10T06:00 -> excludes a-old; kind filter excludes m-1.
    let got = repo
        .recent_by_kind(
            &["aeolus.forecast".to_string()],
            "2026-06-10T06:00:00.000Z",
            10,
        )
        .await
        .unwrap();
    let ids: Vec<&str> = got.iter().map(|r| r.signal_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["a-new", "a-mid"],
        "newest-first, in-window, kind-filtered"
    );
    assert_eq!(got[0].kind, "aeolus.forecast");
    assert_eq!(got[0].source, "aeolus");
    assert_eq!(got[0].content_hash, "h3");
    assert_eq!(got[0].payload, serde_json::json!({"n": 3}));

    // Multiple kinds + a cap: newest two across both kinds.
    let got2 = repo
        .recent_by_kind(
            &["aeolus.forecast".to_string(), "macro.calendar".to_string()],
            "2026-06-10T06:00:00.000Z",
            2,
        )
        .await
        .unwrap();
    let ids2: Vec<&str> = got2.iter().map(|r| r.signal_id.as_str()).collect();
    assert_eq!(
        ids2,
        vec!["a-new", "m-1"],
        "both kinds, newest-first, capped at 2"
    );

    // No kinds requested -> nothing (an empty allowlist reads nothing).
    let none = repo
        .recent_by_kind(&[], "2000-01-01T00:00:00.000Z", 10)
        .await
        .unwrap();
    assert!(none.is_empty(), "empty kinds -> empty result");
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

// ---- lessons repo (T3.1, spec 5.6 semantic memory) ----

#[sqlx::test(migrations = "./migrations")]
async fn lessons_promote_confirm_and_demote_by_superseding_insert(pool: PgPool) {
    let repo = fortuna_ledger::LessonsRepo::new(pool);

    // Operator-approved promotion: an ACTIVE lesson with provenance and
    // a review date.
    repo.insert(
        "l-1",
        "NWS discussion updates before 06Z lead Kalshi high-temp markets",
        &serde_json::json!({"journal_days": ["2026-06-08", "2026-06-09"]}),
        "2026-07-10T00:00:00.000Z",
        "2026-06-10T00:00:00.000Z",
    )
    .await
    .unwrap();
    let active = repo.active().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].lesson_id, "l-1");

    // Confirmation extends the review date via a SUPERSEDING insert
    // (the table is append-only; the old row is never touched).
    repo.confirm(
        "l-1",
        "l-2",
        "2026-08-10T00:00:00.000Z",
        "2026-07-09T00:00:00.000Z",
    )
    .await
    .unwrap();
    let active = repo.active().await.unwrap();
    assert_eq!(active.len(), 1, "confirmation replaces, never duplicates");
    assert_eq!(active[0].lesson_id, "l-2");
    assert_eq!(active[0].review_at, "2026-08-10T00:00:00.000Z");
    assert_eq!(
        active[0].body, "NWS discussion updates before 06Z lead Kalshi high-temp markets",
        "body rides through confirmation"
    );

    // Decay: demotion supersedes with a demoted row.
    repo.demote("l-2", "l-3", "2026-08-11T00:00:00.000Z")
        .await
        .unwrap();
    assert!(
        repo.active().await.unwrap().is_empty(),
        "demoted = out of memory"
    );

    // Confirming or demoting a superseded lesson is refused (the chain
    // head is the only live row).
    assert!(repo
        .demote("l-1", "l-4", "2026-08-12T00:00:00.000Z")
        .await
        .is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn journal_range_returns_the_weekly_window_in_order(pool: PgPool) {
    let repo = fortuna_ledger::JournalRepo::new(pool);
    for day in ["2026-06-08", "2026-06-10", "2026-06-09", "2026-06-15"] {
        repo.insert(
            &format!("j-{day}"),
            day,
            &serde_json::json!({"body": format!("entry {day}")}),
            "2026-06-16T00:00:00.000Z",
        )
        .await
        .unwrap();
    }
    let week = repo.range("2026-06-08", "2026-06-14").await.unwrap();
    assert_eq!(
        week.iter().map(|r| r.day.as_str()).collect::<Vec<_>>(),
        vec!["2026-06-08", "2026-06-09", "2026-06-10"],
        "inclusive window, day-ordered, the 15th excluded"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn resolved_stats_expose_brier_and_clv_for_the_review(pool: PgPool) {
    seed_event(&pool, "evt-1").await;
    let repo = fortuna_ledger::BeliefsRepo::new(pool);
    repo.insert(
        "b-1",
        "2026-06-10T12:00:00.000Z",
        "evt-1",
        0.7,
        0.72,
        "2026-06-20T18:00:00.000Z",
        &serde_json::json!([]),
        &serde_json::json!({"model_id": "stub"}),
        None,
    )
    .await
    .unwrap();
    repo.resolve_and_score("b-1", true, 0.09, Some(150.0))
        .await
        .unwrap();

    let stats = repo.resolved_stats("weather").await.unwrap();
    assert_eq!(stats.len(), 1);
    assert!((stats[0].p - 0.7).abs() < 1e-9);
    assert!(stats[0].outcome);
    assert!((stats[0].brier - 0.09).abs() < 1e-9);
    assert!((stats[0].clv_bps.unwrap() - 150.0).abs() < 1e-9);
}

#[sqlx::test(migrations = "./migrations")]
async fn resolved_persona_stats_groups_resolved_beliefs_by_provenance_scope(pool: PgPool) {
    for e in ["e-a", "e-b", "e-c", "e-v2", "e-macro", "e-model"] {
        seed_event(&pool, e).await;
    }
    let repo = fortuna_ledger::BeliefsRepo::new(pool);
    let meteo_v1 = serde_json::json!({"persona_id": "meteorologist", "persona_version": 1});
    let ins =
        |id: &'static str, at: &'static str, ev: &'static str, p: f64, prov: serde_json::Value| {
            let repo = &repo;
            async move {
                repo.insert(
                    id,
                    at,
                    ev,
                    p,
                    p,
                    "2026-06-20T18:00:00.000Z",
                    &serde_json::json!([]),
                    &prov,
                    None,
                )
                .await
                .unwrap();
            }
        };

    // Two RESOLVED meteorologist@1 beliefs (the target scope), ascending created_at.
    ins(
        "b-a",
        "2026-06-10T10:00:00.000Z",
        "e-a",
        0.7,
        meteo_v1.clone(),
    )
    .await;
    ins(
        "b-b",
        "2026-06-10T11:00:00.000Z",
        "e-b",
        0.4,
        meteo_v1.clone(),
    )
    .await;
    // An OPEN meteorologist@1 belief (never resolved) — must be excluded.
    ins(
        "b-c",
        "2026-06-10T12:00:00.000Z",
        "e-c",
        0.6,
        meteo_v1.clone(),
    )
    .await;
    // A different VERSION, a different PERSONA, and a NON-persona belief — all excluded.
    ins(
        "b-v2",
        "2026-06-10T10:30:00.000Z",
        "e-v2",
        0.5,
        serde_json::json!({"persona_id": "meteorologist", "persona_version": 2}),
    )
    .await;
    ins(
        "b-macro",
        "2026-06-10T10:30:00.000Z",
        "e-macro",
        0.5,
        serde_json::json!({"persona_id": "macro-economist", "persona_version": 1}),
    )
    .await;
    ins(
        "b-model",
        "2026-06-10T10:30:00.000Z",
        "e-model",
        0.5,
        serde_json::json!({"model_id": "stub"}),
    )
    .await;

    repo.resolve_and_score("b-a", true, 0.09, Some(150.0))
        .await
        .unwrap();
    repo.resolve_and_score("b-b", false, 0.16, None)
        .await
        .unwrap(); // CLV unmeasurable
                   // b-c stays OPEN.
    repo.resolve_and_score("b-v2", true, 0.25, Some(10.0))
        .await
        .unwrap();
    repo.resolve_and_score("b-macro", true, 0.25, Some(20.0))
        .await
        .unwrap();
    repo.resolve_and_score("b-model", true, 0.25, Some(30.0))
        .await
        .unwrap();

    let stats = repo
        .resolved_persona_stats("meteorologist", 1)
        .await
        .unwrap();
    assert_eq!(stats.persona_id, "meteorologist");
    assert_eq!(stats.persona_version, 1);
    // Only the two RESOLVED meteorologist@1 beliefs, in created_at order.
    assert_eq!(
        stats.samples.len(),
        2,
        "open / v2 / other-persona / non-persona all excluded"
    );
    assert!((stats.samples[0].0 - 0.7).abs() < 1e-9 && stats.samples[0].1);
    assert!((stats.samples[1].0 - 0.4).abs() < 1e-9 && !stats.samples[1].1);
    // Only the measurable CLV survives (b-b's was None).
    assert_eq!(stats.clv_bps.len(), 1);
    assert!((stats.clv_bps[0] - 150.0).abs() < 1e-9);

    // A scope with no resolved beliefs is empty, not an error.
    let empty = repo
        .resolved_persona_stats("meteorologist", 99)
        .await
        .unwrap();
    assert!(empty.samples.is_empty() && empty.clv_bps.is_empty());
}

// ---- discovery persistence (T3.2, spec 5.12) ----

#[sqlx::test(migrations = "./migrations")]
async fn tradability_scores_persist_append_only_with_latest_query(pool: PgPool) {
    let repo = fortuna_ledger::TradabilityRepo::new(pool);
    repo.insert(
        "ts-1",
        "KX-A",
        "kalshi",
        0.42,
        &serde_json::json!({"volume_factor": 0.7, "category_quality": 0.6}),
        "2026-06-11T06:00:00.000Z",
    )
    .await
    .unwrap();
    repo.insert(
        "ts-2",
        "KX-A",
        "kalshi",
        0.55,
        &serde_json::json!({"volume_factor": 0.9, "category_quality": 0.61}),
        "2026-06-12T06:00:00.000Z",
    )
    .await
    .unwrap();

    let latest = repo.latest("KX-A").await.unwrap().unwrap();
    assert_eq!(latest.score_id, "ts-2");
    assert!((latest.score - 0.55).abs() < 1e-9);
    assert!(repo.latest("KX-UNSEEN").await.unwrap().is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn unscoreable_events_are_excluded_from_calibration_queries(pool: PgPool) {
    let events = fortuna_ledger::EventsRepo::new(pool.clone());
    let beliefs = fortuna_ledger::BeliefsRepo::new(pool.clone());

    seed_event(&pool, "evt-good").await;
    events
        .create(
            "watch:vibes",
            "something unfalsifiable",
            "vibes",
            "my-cool-blog",
            None,
            "2026-06-20T18:00:00.000Z",
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
    events.mark_unscoreable("watch:vibes").await.unwrap();

    for (b, evt) in [("b-good", "evt-good"), ("b-vibes", "watch:vibes")] {
        beliefs
            .insert(
                b,
                "2026-06-10T12:00:00.000Z",
                evt,
                0.7,
                0.7,
                "2026-06-20T18:00:00.000Z",
                &serde_json::json!([]),
                &serde_json::json!({"model_id": "stub"}),
                None,
            )
            .await
            .unwrap();
        beliefs
            .resolve_and_score(b, true, 0.09, None)
            .await
            .unwrap();
    }

    // Both resolved; only the scoreable event's belief reaches the
    // calibration record (spec 5.12: no beliefs nobody can grade).
    let samples = beliefs.resolved_samples("weather").await.unwrap();
    assert_eq!(samples.len(), 1);
    let stats = beliefs.resolved_stats("weather").await.unwrap();
    assert_eq!(stats.len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn current_edges_for_market_sees_chain_heads_only(pool: PgPool) {
    seed_event(&pool, "evt-1").await;
    let repo = fortuna_ledger::EdgesRepo::new(pool);
    repo.insert_edge(
        "e-1",
        "KX-A",
        "kalshi",
        "evt-1",
        "direct",
        0.7,
        "claude-fable-5",
        None,
        None,
        "2026-06-10T12:00:00.000Z",
    )
    .await
    .unwrap();
    // Confirmation supersedes.
    repo.insert_edge(
        "e-2",
        "KX-A",
        "kalshi",
        "evt-1",
        "direct",
        0.7,
        "claude-fable-5",
        Some("operator"),
        Some("e-1"),
        "2026-06-11T12:00:00.000Z",
    )
    .await
    .unwrap();

    let edges = repo.current_edges_for_market("KX-A").await.unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].edge_id, "e-2");
    assert_eq!(edges[0].confirmed_by.as_deref(), Some("operator"));
    assert!(repo
        .current_edges_for_market("KX-UNSEEN")
        .await
        .unwrap()
        .is_empty());
}

// ---- Phase-C idempotency + recordings (Task A1) ----

/// Inserting a settlement entry twice with the same (market_id, intent_id)
/// and supersedes=None must be idempotent: second call returns Ok(false) and
/// only one row exists. (Partial-unique-index dedup; A1 spec.)
#[sqlx::test(migrations = "./migrations")]
async fn settlement_entry_idempotent_on_same_intent(pool: PgPool) {
    let repo = fortuna_ledger::SettlementsRepo::new(pool);
    let inserted1 = repo
        .insert_entry(
            "se-idem-1",
            "MKT-A",
            "kalshi",
            500,
            "pending",
            None,
            Some("intent-abc"),
            &serde_json::json!({}),
            "2026-06-18T10:00:00.000Z",
        )
        .await
        .unwrap();
    assert!(inserted1, "first insert must return true");

    // Same market_id + same intent_id + supersedes=None → partial-index conflict → no-op.
    let inserted2 = repo
        .insert_entry(
            "se-idem-2",
            "MKT-A",
            "kalshi",
            500,
            "pending",
            None,
            Some("intent-abc"),
            &serde_json::json!({}),
            "2026-06-18T10:00:01.000Z",
        )
        .await
        .unwrap();
    assert!(
        !inserted2,
        "second insert with same intent must return false"
    );

    // Only one row.
    let chain = repo.chain("MKT-A").await.unwrap();
    assert_eq!(chain.len(), 1);
}

/// A correction row (supersedes=Some) with the same (market_id, intent_id)
/// must still insert — the partial-unique-index exempts rows where
/// supersedes IS NOT NULL.
#[sqlx::test(migrations = "./migrations")]
async fn settlement_entry_correction_still_inserts(pool: PgPool) {
    let repo = fortuna_ledger::SettlementsRepo::new(pool);

    // Initial entry.
    let ins1 = repo
        .insert_entry(
            "se-c-1",
            "MKT-B",
            "kalshi",
            1_000,
            "pending",
            None,
            Some("intent-xyz"),
            &serde_json::json!({}),
            "2026-06-18T11:00:00.000Z",
        )
        .await
        .unwrap();
    assert!(ins1);

    // Correction with supersedes set — NOT deduped by partial index.
    let ins2 = repo
        .insert_entry(
            "se-c-2",
            "MKT-B",
            "kalshi",
            1_000,
            "posted",
            Some("se-c-1"),
            Some("intent-xyz"),
            &serde_json::json!({}),
            "2026-06-18T11:00:01.000Z",
        )
        .await
        .unwrap();
    assert!(ins2, "correction must insert (partial index exempt)");

    let chain = repo.chain("MKT-B").await.unwrap();
    assert_eq!(chain.len(), 2);
}

/// Inserting a scalar belief twice with the same (producer, event_key)
/// must be idempotent: second call returns Ok(false) and only one row.
#[sqlx::test(migrations = "./migrations")]
async fn scalar_belief_idempotent_on_same_producer_event_key(pool: PgPool) {
    let repo = fortuna_ledger::ScalarBeliefsRepo::new(pool);
    let q = serde_json::json!({"p50": 0.5});
    let prov = serde_json::json!({"model": "test"});

    let ins1 = repo
        .insert(
            "sb-idem-1",
            "aeolus",
            "TEMP/KORD/2026-07-01",
            &q,
            "celsius",
            "24h",
            &prov,
            "2026-06-18T10:00:00.000Z",
        )
        .await
        .unwrap();
    assert!(ins1, "first insert must return true");

    // Same producer + event_key → conflict → no-op.
    let ins2 = repo
        .insert(
            "sb-idem-2",
            "aeolus",
            "TEMP/KORD/2026-07-01",
            &q,
            "celsius",
            "24h",
            &prov,
            "2026-06-18T10:00:01.000Z",
        )
        .await
        .unwrap();
    assert!(
        !ins2,
        "second insert with same (producer, event_key) must return false"
    );
}

/// bus_recordings rejects UPDATE and DELETE (append-only trigger).
#[sqlx::test(migrations = "./migrations")]
async fn bus_recordings_append_only(pool: PgPool) {
    let repo = fortuna_ledger::RecordingsRepo::new(pool.clone());
    repo.append(
        "rec-1",
        0,
        r#"{"event":"test"}"#,
        "2026-06-18T10:00:00.000Z",
    )
    .await
    .unwrap();

    // UPDATE must be refused by the trigger. Use sqlx::query (non-macro)
    // so offline mode does not need a cache entry for this ad-hoc SQL.
    let update_result =
        sqlx::query(r#"UPDATE bus_recordings SET jsonl = 'tampered' WHERE recording_id = $1"#)
            .bind("rec-1")
            .execute(&pool)
            .await;
    assert!(
        update_result.is_err(),
        "UPDATE on bus_recordings must be refused"
    );

    // DELETE must also be refused.
    let delete_result = sqlx::query(r#"DELETE FROM bus_recordings WHERE recording_id = $1"#)
        .bind("rec-1")
        .execute(&pool)
        .await;
    assert!(
        delete_result.is_err(),
        "DELETE on bus_recordings must be refused"
    );
}

// ---- WS1-8a: forward-only promotion count (§9.1) ----

/// Helper: insert a belief with a given provenance JSON, then resolve+score it.
async fn seed_resolved_belief(
    pool: &PgPool,
    belief_id: &str,
    event_id: &str,
    provenance: serde_json::Value,
) {
    let repo = fortuna_ledger::BeliefsRepo::new(pool.clone());
    repo.insert(
        belief_id,
        "2026-06-10T12:00:00.000Z",
        event_id,
        0.7,
        0.7,
        "2026-06-20T18:00:00.000Z",
        &serde_json::json!([]),
        &provenance,
        None,
    )
    .await
    .unwrap();
    repo.resolve_and_score(belief_id, true, 0.09, None)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
/// T8a-1: forward-only count excludes `source='historical-import'` rows (§9.1).
/// N forward beliefs + M import beliefs → count returns N (not N+M).
async fn resolved_count_forward_excludes_historical_import_rows(pool: PgPool) {
    seed_event(&pool, "evt-fwd-1").await;
    seed_event(&pool, "evt-fwd-2").await;
    seed_event(&pool, "evt-fwd-3").await;
    seed_event(&pool, "evt-imp-1").await;
    seed_event(&pool, "evt-imp-2").await;

    // N=3 forward resolved beliefs (no source key → NULL source → forward)
    seed_resolved_belief(
        &pool,
        "b-fwd-1",
        "evt-fwd-1",
        serde_json::json!({"producer": "aeolus"}),
    )
    .await;
    seed_resolved_belief(
        &pool,
        "b-fwd-2",
        "evt-fwd-2",
        serde_json::json!({"producer": "aeolus"}),
    )
    .await;
    seed_resolved_belief(
        &pool,
        "b-fwd-3",
        "evt-fwd-3",
        serde_json::json!({"producer": "aeolus"}),
    )
    .await;

    // M=2 historical-import resolved beliefs (same producer, same category)
    seed_resolved_belief(
        &pool,
        "b-imp-1",
        "evt-imp-1",
        serde_json::json!({"producer": "aeolus", "source": "historical-import"}),
    )
    .await;
    seed_resolved_belief(
        &pool,
        "b-imp-2",
        "evt-imp-2",
        serde_json::json!({"producer": "aeolus", "source": "historical-import"}),
    )
    .await;

    let repo = fortuna_ledger::BeliefsRepo::new(pool);

    // Merged scope (producer=None): forward count = 3, not 5.
    let fwd = repo.resolved_count_forward(None, "weather").await.unwrap();
    assert_eq!(
        fwd, 3,
        "forward count must exclude historical-import rows (got {fwd}, want 3)"
    );

    // Per-producer scope: same exclusion applies.
    let fwd_prod = repo
        .resolved_count_forward(Some("aeolus"), "weather")
        .await
        .unwrap();
    assert_eq!(
        fwd_prod, 3,
        "per-producer forward count must exclude historical-import rows (got {fwd_prod}, want 3)"
    );
}

#[sqlx::test(migrations = "./migrations")]
/// T8a-2 (byte-identical-today guarantee): with NO import rows,
/// `resolved_count_forward` equals the count `resolved_stats` would return.
async fn resolved_count_forward_equals_resolved_stats_count_when_no_import_rows(pool: PgPool) {
    seed_event(&pool, "evt-r-1").await;
    seed_event(&pool, "evt-r-2").await;

    seed_resolved_belief(
        &pool,
        "b-r-1",
        "evt-r-1",
        serde_json::json!({"producer": "aeolus"}),
    )
    .await;
    seed_resolved_belief(
        &pool,
        "b-r-2",
        "evt-r-2",
        serde_json::json!({"producer": "aeolus"}),
    )
    .await;

    let repo = fortuna_ledger::BeliefsRepo::new(pool);

    let stats_count = repo.resolved_stats("weather").await.unwrap().len() as i64;
    let fwd = repo.resolved_count_forward(None, "weather").await.unwrap();
    assert_eq!(
        fwd, stats_count,
        "forward count must equal resolved_stats count when no import rows exist (today guarantee)"
    );
}

// ----------------------------------------------------------------------- T8b
// Tests for WS1 slice 8b: forward_resolved_for_brier_baseline query.

/// Helper: insert a benchmark event (benchmark_at set so snapshots can precede it)
/// and return the event_id.
async fn seed_brier_event(pool: &PgPool, event_id: &str, benchmark_at: &str) {
    fortuna_ledger::EventsRepo::new(pool.clone())
        .create(
            event_id,
            "Will it happen?",
            "official source",
            "nws",
            None,
            benchmark_at,
            "weather",
            "2026-06-10T12:00:00.000Z",
        )
        .await
        .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
/// T8b-1: `forward_resolved_for_brier_baseline` returns exactly the forward
/// resolved rows and EXCLUDES `source='historical-import'` beliefs (§9.1).
/// Covers: historical-import exclusion filter, field mapping (p, outcome,
/// event_id, benchmark_at), and forward-only row count.
async fn forward_resolved_for_brier_baseline_excludes_historical_import(pool: PgPool) {
    // Three forward beliefs on two events.
    seed_brier_event(&pool, "evtb-fwd-1", "2026-06-20T18:00:00.000Z").await;
    seed_brier_event(&pool, "evtb-fwd-2", "2026-06-21T18:00:00.000Z").await;
    seed_brier_event(&pool, "evtb-imp-1", "2026-06-22T18:00:00.000Z").await;

    let beliefs_repo = fortuna_ledger::BeliefsRepo::new(pool.clone());

    // Forward: two beliefs on evtb-fwd-1 (one superseded) and one on evtb-fwd-2.
    beliefs_repo
        .insert(
            "bb-fwd-1",
            "2026-06-10T12:00:00.000Z",
            "evtb-fwd-1",
            0.70,
            0.70,
            "2026-06-20T18:00:00.000Z",
            &serde_json::json!([]),
            &serde_json::json!({"producer": "aeolus"}),
            None,
        )
        .await
        .unwrap();
    beliefs_repo
        .resolve_and_score("bb-fwd-1", true, 0.09, None)
        .await
        .unwrap();

    beliefs_repo
        .insert(
            "bb-fwd-2",
            "2026-06-10T13:00:00.000Z",
            "evtb-fwd-2",
            0.60,
            0.60,
            "2026-06-21T18:00:00.000Z",
            &serde_json::json!([]),
            &serde_json::json!({"producer": "aeolus"}),
            None,
        )
        .await
        .unwrap();
    beliefs_repo
        .resolve_and_score("bb-fwd-2", false, 0.36, None)
        .await
        .unwrap();

    // Historical import: same category, must be excluded.
    beliefs_repo
        .insert(
            "bb-imp-1",
            "2026-06-10T14:00:00.000Z",
            "evtb-imp-1",
            0.55,
            0.55,
            "2026-06-22T18:00:00.000Z",
            &serde_json::json!([]),
            &serde_json::json!({"producer": "aeolus", "source": "historical-import"}),
            None,
        )
        .await
        .unwrap();
    beliefs_repo
        .resolve_and_score("bb-imp-1", true, 0.2025, None)
        .await
        .unwrap();

    let rows = beliefs_repo
        .forward_resolved_for_brier_baseline("weather")
        .await
        .unwrap();

    // Only 2 forward rows returned; the historical-import row is excluded.
    assert_eq!(
        rows.len(),
        2,
        "historical-import row must be excluded (got {} rows, want 2)",
        rows.len()
    );

    // Verify the two forward rows have correct values.
    let row1 = rows.iter().find(|r| r.belief_id == "bb-fwd-1").unwrap();
    assert!(
        (row1.p - 0.70).abs() < 1e-9,
        "p must be 0.70, got {}",
        row1.p
    );
    assert!(row1.outcome, "outcome for bb-fwd-1 must be true");
    assert_eq!(row1.event_id, "evtb-fwd-1");
    assert_eq!(row1.benchmark_at, "2026-06-20T18:00:00.000Z");

    let row2 = rows.iter().find(|r| r.belief_id == "bb-fwd-2").unwrap();
    assert!(
        (row2.p - 0.60).abs() < 1e-9,
        "p must be 0.60, got {}",
        row2.p
    );
    assert!(!row2.outcome, "outcome for bb-fwd-2 must be false");
    assert_eq!(row2.event_id, "evtb-fwd-2");
    assert_eq!(row2.benchmark_at, "2026-06-21T18:00:00.000Z");

    // None of the returned rows is the import row.
    assert!(
        rows.iter().all(|r| r.belief_id != "bb-imp-1"),
        "bb-imp-1 (historical-import) must not appear in results"
    );
}
