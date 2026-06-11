//! T1.4: the settlement processor + watchdogs through the composed loop
//! (spec 5.13: "settlement is asynchronous and adversarial; FORTUNA never
//! assumes it, only reconciles it").
//!
//! Doctrine under test:
//! - Venue settlement notices (winner / void / correction) flow through
//!   the cursor stream into the books: entry chain pending -> posted ->
//!   confirmed, positions settled/refunded/reversed EXACTLY, every step
//!   audited. At-least-once delivery is deduped; a duplicate notice never
//!   double-applies.
//! - A venue CORRECTION reverses the original settlement to the cent and
//!   re-settles with the corrected winner (conservation through the
//!   chain).
//! - Watchdogs: settlement-overdue alerts (once, debounced); a books-vs-
//!   venue position mismatch that PERSISTS becomes a discrepancy record
//!   and a GLOBAL HALT (no silent corrections, spec 5.13); a Disputed
//!   market freezes its position (lifecycle Disputed, exposure retained).
//!
//! Written BEFORE the runner implementation per the TDD doctrine.

use fortuna_core::book::PriceLevel;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::GateConfig;
use fortuna_runner::{
    AuditSink, CoreHandle, MemoryAuditSink, Proposal, ProposedLeg, RunnerConfig, RunnerError,
    SimRunner, Stage, Strategy, StrategyKind, StrategyMetrics, Urgency,
};
use fortuna_state::{MarkPolicy, PositionLifecycle, SettlementStatus};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

fn lvl(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: Contracts::new(qty),
    }
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

fn settle_market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("settle {id}"),
        category: "test".into(),
        status: MarketStatus::Trading,
        // Close in 1h; settlement expected 1h after close.
        close_at: Some(t0().checked_add_millis(3_600_000).unwrap()),
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 1,
        },
        volume_contracts: Some(1_000),
        payout_per_contract: Cents::new(100),
    }
}

fn gate_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 800000
        max_daily_loss_cents = 500000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 45
        max_cross_cents = 10
        per_market_exposure_cents = 100000
        per_event_exposure_cents = 150000
        require_event_mapping = false

        [per_strategy.test_buyer]
        max_exposure_cents = 200000
        max_order_notional_cents = 100000
        min_net_edge_bps = 1

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap()
}

#[derive(Clone, Default)]
struct SharedAuditSink(Arc<Mutex<MemoryAuditSink>>);

impl AuditSink for SharedAuditSink {
    fn append(
        &mut self,
        kind: &str,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(kind, ref_id, payload)
    }
}

impl SharedAuditSink {
    fn rows_of_kind(&self, kind: &str) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .records
            .iter()
            .filter(|(k, _, _)| k == kind)
            .map(|(_, _, p)| p.clone())
            .collect()
    }
}

/// Buys 10 YES at the ask (taker) on its market, once.
struct TestBuyer {
    id: StrategyId,
    market: MarketId,
    proposed: bool,
    metrics: StrategyMetrics,
}

impl TestBuyer {
    fn new(market: &str) -> Self {
        TestBuyer {
            id: StrategyId::new("test_buyer").unwrap(),
            market: mkt(market),
            proposed: false,
            metrics: StrategyMetrics::default(),
        }
    }
}

#[async_trait::async_trait]
impl Strategy for TestBuyer {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }
    fn stage(&self) -> Stage {
        Stage::Sim
    }
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;
        if self.proposed {
            return Ok(Vec::new());
        }
        let EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        if book.market != self.market {
            return Ok(Vec::new());
        }
        let Some(ask) = book.yes_asks.first() else {
            return Ok(Vec::new());
        };
        self.proposed = true;
        Ok(vec![Proposal {
            legs: vec![ProposedLeg {
                market: self.market.clone(),
                side: Side::Yes,
                action: Action::Buy,
                limit_price: ask.price,
                fair_value: Cents::new(ask.price.raw() + 5),
                calibrated_p: None,
            }],
            group_policy: None,
            urgency: Urgency::Taker,
            manifest_hash: None,
            thesis: "test buyer".into(),
        }])
    }
    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}

struct World {
    runner: SimRunner,
    audit: SharedAuditSink,
}

/// One market, one strategy that crosses the 60c ask for 10 contracts on
/// the first tick (envelope 700 / cost-per-set ~61c => sets = 10 capped).
fn world(seed: u64) -> World {
    let audit = SharedAuditSink::default();
    let config = RunnerConfig {
        seed,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("test_buyer".to_string(), Cents::new(100_000))]),
        max_daily_loss: Cents::new(500_000),
        fee_model: fee_model(),
        markets: vec![settle_market("KXS")],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(seed),
        mark_policy: MarkPolicy {
            max_book_age_ms: 86_400_000,
            max_spread_cents: 90,
        },
        max_sets_per_proposal: 10,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    };
    let runner = SimRunner::new(
        config,
        vec![Box::new(TestBuyer::new("KXS"))],
        Box::new(audit.clone()),
        t0(),
    )
    .unwrap();
    runner
        .venue()
        .set_book(&mkt("KXS"), vec![lvl(55, 50)], vec![lvl(60, 50)])
        .unwrap();
    World { runner, audit }
}

fn tick(w: &mut World) -> fortuna_runner::TickReport {
    futures::executor::block_on(w.runner.tick()).unwrap()
}

/// Fill the position (tick 1), then settle at the venue and let the
/// PROCESSOR reconcile it (tick 2).
fn fill_then_settle(w: &mut World, winner: Side) {
    let r = tick(w);
    assert!(r.fills_applied >= 1, "the buyer must fill");
    w.runner.venue().settle_market(&mkt("KXS"), winner).unwrap();
    tick(w);
}

// ------------------------------------------------------------ happy path

#[test]
fn venue_settlement_flows_through_the_processor_into_the_books() {
    let mut w = world(41);
    fill_then_settle(&mut w, Side::Yes);

    // Position settled: 10 YES @ 60 -> payout 1000, pnl 1000 - 600 = 400.
    let pos = w.runner.positions().position(&mkt("KXS")).unwrap();
    assert_eq!(pos.yes.qty, 0);
    assert_eq!(pos.realized_pnl, Cents::new(400));

    // Entry chain reached Confirmed (venue shows no residual position).
    let head = w.runner.settlements().head(&mkt("KXS")).unwrap();
    assert_eq!(head.status, SettlementStatus::Confirmed);
    assert_eq!(head.amount_cents, Cents::new(1_000));

    // Audited: a settlement row exists and no discrepancy rows.
    assert!(!w.audit.rows_of_kind("settlement").is_empty());
    assert!(w.audit.rows_of_kind("discrepancy").is_empty());
}

#[test]
fn duplicate_notices_never_double_apply() {
    let mut w = world(43);
    fill_then_settle(&mut w, Side::Yes);
    let pnl_after = w
        .runner
        .positions()
        .position(&mkt("KXS"))
        .unwrap()
        .realized_pnl;
    // More ticks re-poll the stream; the notice must not re-apply.
    tick(&mut w);
    tick(&mut w);
    assert_eq!(
        w.runner
            .positions()
            .position(&mkt("KXS"))
            .unwrap()
            .realized_pnl,
        pnl_after
    );
    assert_eq!(w.runner.settlements().chain(&mkt("KXS")).len(), 3);
}

// ------------------------------------------------------------ correction

#[test]
fn a_venue_correction_reverses_and_resettles_to_the_cent() {
    let mut w = world(47);
    fill_then_settle(&mut w, Side::Yes);
    assert_eq!(
        w.runner
            .positions()
            .position(&mkt("KXS"))
            .unwrap()
            .realized_pnl,
        Cents::new(400)
    );

    // The venue corrects: NO actually won. 10 YES @ 60 -> -600.
    w.runner
        .venue()
        .reverse_settlement(&mkt("KXS"), Side::No)
        .unwrap();
    tick(&mut w);

    let pos = w.runner.positions().position(&mkt("KXS")).unwrap();
    assert_eq!(
        pos.realized_pnl,
        Cents::new(-600),
        "corrected PnL must equal what a direct NO settlement would produce"
    );
    // Chain: pending, posted, confirmed, REVERSED, pending, posted, confirmed.
    let chain = w.runner.settlements().chain(&mkt("KXS"));
    assert!(chain.iter().any(|e| e.status == SettlementStatus::Reversed));
    assert_eq!(
        w.runner.settlements().head(&mkt("KXS")).unwrap().status,
        SettlementStatus::Confirmed
    );
    // The reversal is auditable.
    assert!(!w.audit.rows_of_kind("settlement_reversal").is_empty());
}

// ------------------------------------------------------------------ void

#[test]
fn a_void_refunds_basis_and_leaves_realized_pnl_untouched() {
    let mut w = world(53);
    let r = tick(&mut w);
    assert!(r.fills_applied >= 1);
    let fees_before = w
        .runner
        .positions()
        .position(&mkt("KXS"))
        .unwrap()
        .fees_paid;

    w.runner.venue().void_market(&mkt("KXS")).unwrap();
    let voids_before = w.runner.counters().settlement_voids;
    assert_eq!(voids_before, 0, "void not yet processed");
    tick(&mut w);

    let pos = w.runner.positions().position(&mkt("KXS")).unwrap();
    assert_eq!(pos.yes.qty, 0, "voided lots clear");
    assert_eq!(
        pos.realized_pnl,
        Cents::ZERO,
        "a void is the world breaking the question, not a trading outcome"
    );
    assert_eq!(pos.fees_paid, fees_before, "fees are sunk, not refunded");
    let head = w.runner.settlements().head(&mkt("KXS")).unwrap();
    assert_eq!(head.status, SettlementStatus::Confirmed);
    assert_eq!(head.amount_cents, Cents::new(600), "refund = cost basis");
}

// -------------------------------------------------------------- watchdogs

#[test]
fn overdue_settlement_alerts_once() {
    let mut w = world(59);
    let r = tick(&mut w);
    assert!(r.fills_applied >= 1);

    // close (1h) + expected lag (1h) + grace (1h default) = 3h. At 4h with
    // no settlement the watchdog must fire — exactly once.
    w.runner.clock.advance_millis(4 * 3_600_000).unwrap();
    tick(&mut w);
    assert_eq!(w.audit.rows_of_kind("watchdog").len(), 1);
    let row = &w.audit.rows_of_kind("watchdog")[0];
    assert_eq!(row["kind"], "settlement_overdue");

    tick(&mut w);
    assert_eq!(
        w.audit.rows_of_kind("watchdog").len(),
        1,
        "debounced: one alert per market"
    );
}

#[test]
fn persistent_position_mismatch_writes_discrepancy_and_halts() {
    let mut w = world(61);
    let r = tick(&mut w);
    assert!(r.fills_applied >= 1);

    // Books-vs-venue drift: venue says 12 YES, we say 10.
    w.runner
        .venue()
        .seed_position(&mkt("KXS"), 12, 0, Cents::new(720));

    // One mismatched tick is NOT a discrepancy (an in-flight fill could
    // explain it); a persistent one is.
    tick(&mut w);
    assert!(w.audit.rows_of_kind("discrepancy").is_empty());
    tick(&mut w);
    let report = tick(&mut w);
    assert!(
        !w.audit.rows_of_kind("discrepancy").is_empty(),
        "3 consecutive mismatched ticks must write a discrepancy"
    );
    assert!(report.halted, "books-vs-venue divergence is a GLOBAL HALT");
}

#[test]
fn disputed_market_freezes_the_position() {
    let mut w = world(67);
    let r = tick(&mut w);
    assert!(r.fills_applied >= 1);

    w.runner
        .venue()
        .set_market_status(&mkt("KXS"), MarketStatus::Disputed);
    tick(&mut w);

    let pos = w.runner.positions().position(&mkt("KXS")).unwrap();
    assert_eq!(
        pos.lifecycle,
        PositionLifecycle::Disputed,
        "a disputed market's position freezes (out of bankroll, in exposure)"
    );
    let rows = w.audit.rows_of_kind("watchdog");
    assert!(rows.iter().any(|r| r["kind"] == "dispute_freeze"));
}

// ------------------------------------------------------------ determinism

#[test]
fn settlement_processing_is_byte_deterministic() {
    let run = || {
        let mut w = world(71);
        fill_then_settle(&mut w, Side::Yes);
        w.runner
            .venue()
            .reverse_settlement(&mkt("KXS"), Side::No)
            .unwrap();
        tick(&mut w);
        w.runner.report().unwrap().recording_jsonl
    };
    assert_eq!(run(), run());
}

// ---- T3.6: divergence detector + orphan watchdog (spec 5.13) ----

#[test]
fn settlement_divergence_records_and_pnl_follows_venue_truth() {
    let mut w = world(91);
    // The composition wires the canonical resolution (events pipeline):
    // canon says NO for KXS's mapped event.
    w.runner
        .set_canonical_resolution(mkt("KXS"), Side::No, "edge-kxs-evt");

    // The venue settles YES anyway (two truths coexist deliberately).
    fill_then_settle(&mut w, Side::Yes);

    // PnL FOLLOWS VENUE TRUTH: we held YES, the venue paid.
    let pos = w.runner.positions().position(&mkt("KXS")).unwrap();
    assert_eq!(pos.realized_pnl, Cents::new(400));

    // The divergence is RECORDED, never reconciled away: a discrepancy
    // names both truths and the edge whose confidence takes the hit.
    let rows = w.audit.rows_of_kind("discrepancy");
    let div: Vec<&serde_json::Value> = rows
        .iter()
        .filter(|r| r["kind"] == "settlement_divergence")
        .collect();
    assert_eq!(div.len(), 1, "exactly one divergence record: {rows:?}");
    assert_eq!(div[0]["detail"]["venue_outcome"], "yes");
    assert_eq!(div[0]["detail"]["canonical_outcome"], "no");
    assert_eq!(div[0]["detail"]["edge"], "edge-kxs-evt");
    assert!(
        div[0]["detail"]["edge_confidence_hit"].is_string(),
        "the documented confidence hit rides in the record"
    );

    // Counted for ops.
    assert!(w.runner.counters().discrepancies >= 1);
}

#[test]
fn agreeing_settlement_records_no_divergence() {
    let mut w = world(92);
    w.runner
        .set_canonical_resolution(mkt("KXS"), Side::Yes, "edge-kxs-evt");
    fill_then_settle(&mut w, Side::Yes);
    assert!(w
        .audit
        .rows_of_kind("discrepancy")
        .iter()
        .all(|r| r["kind"] != "settlement_divergence"));
}

#[test]
fn orphaned_position_alerts_once_only_when_coverage_is_wired() {
    // Coverage NOT wired: the watchdog stays silent (the composition has
    // not provided the fresh-belief/mechanical-owner view yet).
    let mut w = world(93);
    let r = tick(&mut w);
    assert!(r.fills_applied >= 1);
    tick(&mut w);
    assert!(w
        .audit
        .rows_of_kind("watchdog")
        .iter()
        .all(|r| r["kind"] != "orphaned_position"));

    // Coverage wired and KXS is NOT covered: the held position is an
    // orphan (no fresh belief, no mechanical owner) — alert ONCE.
    w.runner
        .set_position_coverage(std::collections::BTreeSet::new());
    tick(&mut w);
    tick(&mut w);
    let orphans: Vec<serde_json::Value> = w
        .audit
        .rows_of_kind("watchdog")
        .into_iter()
        .filter(|r| r["kind"] == "orphaned_position")
        .collect();
    assert_eq!(orphans.len(), 1, "alert once, not every tick");
    assert_eq!(orphans[0]["market"], "KXS");

    // A COVERED position is never an orphan.
    let mut w2 = world(94);
    let r = tick(&mut w2);
    assert!(r.fills_applied >= 1);
    w2.runner
        .set_position_coverage(std::collections::BTreeSet::from([mkt("KXS")]));
    tick(&mut w2);
    assert!(w2
        .audit
        .rows_of_kind("watchdog")
        .iter()
        .all(|r| r["kind"] != "orphaned_position"));
}

// ---- E5: venue outage must not starve venue-independent watchdogs ----

#[test]
fn venue_independent_watchdogs_run_through_a_venue_outage() {
    // A dark venue rightly stalls the venue-DEPENDENT checks (posted->
    // confirmed, books-vs-venue mismatch) and any NEW catalog knowledge
    // (a dispute flag the venue never delivered cannot be acted on).
    // But overdue (clock + last-known meta) and the orphan scan (local
    // books + composition coverage) need no venue call — an outage must
    // not starve them.
    let mut w = world(95);
    let r = tick(&mut w);
    assert!(r.fills_applied >= 1);

    // The venue goes dark for the rest of the scenario.
    w.runner
        .venue()
        .set_outage_until(t0().checked_add_millis(86_400_000).unwrap());
    w.runner
        .set_position_coverage(std::collections::BTreeSet::new());

    // Overdue: close (1h) + lag (1h) + grace (1h) passed during the
    // outage — the alert must fire anyway.
    w.runner.clock.advance_millis(4 * 3_600_000).unwrap();
    tick(&mut w);
    assert!(
        w.audit
            .rows_of_kind("watchdog")
            .iter()
            .any(|r| r["kind"] == "settlement_overdue"),
        "overdue watchdog starved by the outage"
    );
    // The orphan scan ran dark too.
    assert!(
        w.audit
            .rows_of_kind("watchdog")
            .iter()
            .any(|r| r["kind"] == "orphaned_position"),
        "orphan watchdog starved by the outage"
    );
}
