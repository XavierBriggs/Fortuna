//! TDD (A3): `drain_applied_settlements` buffers the NET realized-PnL delta of
//! every settlement the processor applies to a HELD market, and a second drain
//! returns empty (mem::take semantics). A market we hold no position in produces
//! no buffered settlement.
//!
//! Written RED first against the not-yet-implemented `pending_settlements`
//! field + `SettlementApplied` struct + `drain_applied_settlements` method;
//! will fail until the A3 implementation lands.
//!
//! Mirrors `fill_drain.rs` (A2) for the buffer/drain contract and
//! `settlement_loop.rs` for the fill-then-settle position setup.

use fortuna_core::book::PriceLevel;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::GateConfig;
use fortuna_runner::{
    CoreHandle, MemoryAuditSink, Proposal, ProposedLeg, RunnerConfig, RunnerError, SimRunner,
    Stage, Strategy, StrategyKind, StrategyMetrics, Urgency,
};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-18T12:00:00.000Z").unwrap()
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
            taker_coeff = "0"
            maker_coeff = "0"
        "#,
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn settle_market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: fortuna_core::market::VenueId::new("sim").unwrap(),
        title: format!("settle {id}"),
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
        min_net_edge_bps = 0

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap()
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

fn runner() -> SimRunner {
    let config = RunnerConfig {
        seed: 41,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("test_buyer".to_string(), Cents::new(100_000))]),
        max_daily_loss: Cents::new(500_000),
        fee_model: fee_model(),
        markets: vec![settle_market("KXS"), settle_market("KXOTHER")],
        starting_cash: Cents::new(1_000_000),
        faults: Some(FaultConfig::none(41)),
        mark_policy: MarkPolicy {
            max_book_age_ms: 86_400_000,
            max_spread_cents: 90,
        },
        max_sets_per_proposal: 10,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    };
    let r = SimRunner::new(
        config,
        vec![Box::new(TestBuyer::new("KXS"))],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    r.venue()
        .set_book(&mkt("KXS"), vec![lvl(55, 50)], vec![lvl(60, 50)])
        .unwrap();
    r
}

fn tick(r: &mut SimRunner) -> fortuna_runner::TickReport {
    futures::executor::block_on(r.tick()).unwrap()
}

/// A settled HELD market buffers ONE `SettlementApplied` carrying the net
/// realized-PnL delta (payout - basis); the second drain is empty.
#[test]
fn drain_applied_settlements_buffers_net_pnl_and_empties_on_second_drain() {
    let mut r = runner();

    // Tick 1: the buyer crosses the 60c ask for 10 YES -> cost basis 600c.
    let report = tick(&mut r);
    assert!(report.fills_applied >= 1, "the buyer must fill");

    // Settle YES at the venue, then tick so the processor reconciles it.
    r.venue().settle_market(&mkt("KXS"), Side::Yes).unwrap();
    tick(&mut r);

    // Books truth: 10 YES @ 60 -> payout 1000, realized pnl 1000 - 600 = 400.
    let pos = r.positions().position(&mkt("KXS")).unwrap();
    assert_eq!(pos.realized_pnl, Cents::new(400));

    // First drain: exactly one settlement for KXS with the net delta.
    let drained = r.drain_applied_settlements();
    assert_eq!(
        drained.len(),
        1,
        "one settlement buffered for the held market"
    );
    let s = &drained[0];
    assert_eq!(s.market, mkt("KXS"));
    assert_eq!(
        s.notice_id, "stl-KXS-0",
        "notice_id is the venue settlement notice id (set-once dedup key)"
    );
    assert_eq!(
        s.realized_pnl_cents, 400,
        "amount is the NET realized-PnL delta (payout - basis)"
    );
    assert_eq!(s.outcome, "Winner(Yes)");

    // Second drain: empty (mem::take guarantees one-shot delivery).
    assert!(
        r.drain_applied_settlements().is_empty(),
        "second drain returns empty"
    );
}

/// Settling a market we never held buffers NOTHING (no position, no PnL delta).
#[test]
fn unheld_market_settlement_buffers_nothing() {
    let mut r = runner();

    // Tick 1: open a position only on KXS.
    let report = tick(&mut r);
    assert!(report.fills_applied >= 1, "the buyer must fill on KXS");
    // Drain the KXS-only settlement noise (none yet — not settled).
    let _ = r.drain_applied_settlements();

    // Settle KXOTHER, which we never traded.
    r.venue().settle_market(&mkt("KXOTHER"), Side::Yes).unwrap();
    tick(&mut r);

    let drained = r.drain_applied_settlements();
    assert!(
        drained.iter().all(|s| s.market != mkt("KXOTHER")),
        "an unheld market's settlement is not buffered: {drained:?}"
    );
}
