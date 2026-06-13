//! I2 extension (spec 5.15; T5.B3 ADDITION — this file only adds pins,
//! it weakens nothing): drawdown math includes funding paid/received and
//! margin unrealized PnL, marked conservatively.
//!
//! Property to encode: an equity decline coming PURELY from perp
//! components — an adverse mark move on a margined position, or funding
//! paid — drives the same breach => halt => human-re-arm lifecycle as an
//! event-contract loss; and when the venue mark and our conservative mark
//! disagree, the WORSE-FOR-US number governs the breach (a venue mark that
//! says "fine" cannot mask a loss our conservative mark sees).

use fortuna_core::book::{FeeError, FeeModel, FillRole, OrderBook, PriceLevel};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpMarks, PerpPosition, PerpPrice};
use fortuna_gates::{CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_state::{equity_with_margin, DrawdownMonitor, DrawdownVerdict};
use std::collections::BTreeSet;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T00:00:00.000Z").unwrap()
}

fn at_ms(offset: i64) -> UtcTimestamp {
    t0().checked_add_millis(offset).unwrap()
}

struct ZeroFees;
impl FeeModel for ZeroFees {
    fn fee(
        &self,
        _role: FillRole,
        _price: Cents,
        _qty: Contracts,
        _category: Option<&str>,
        _at: UtcTimestamp,
    ) -> Result<Cents, FeeError> {
        Ok(Cents::ZERO)
    }
}

fn pipeline() -> GatePipeline {
    let cfg: GateConfig = toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 1000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 1000000
        per_event_exposure_cents = 1000000
        require_event_mapping = false

        [per_strategy.perp_i2]
        max_exposure_cents = 1000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.sim]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000
        "#,
    )
    .unwrap();
    GatePipeline::new(cfg).unwrap()
}

fn book(market: &MarketId, as_of: UtcTimestamp) -> OrderBook {
    OrderBook {
        market: market.clone(),
        as_of,
        yes_bids: vec![PriceLevel {
            price: Cents::new(49),
            qty: Contracts::new(1000),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(51),
            qty: Contracts::new(1000),
        }],
    }
}

fn try_order(p: &mut GatePipeline, n: u64, now: UtcTimestamp) -> Result<(), GateCheck> {
    let market = MarketId::new("M").unwrap();
    let b = book(&market, now);
    let fees = ZeroFees;
    let recent = BTreeSet::new();
    let mut g = IdGen::new(n + 1);
    let candidate = CandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("perp_i2").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market: market.clone(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(1),
        fair_value: Cents::new(60),
        client_order_id: ClientOrderId::new(format!("perp-i2-{n}")).unwrap(),
    };
    let inputs = GateInputs {
        now,
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: Some(&b),
        last_trade_price: None,
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    match p.evaluate(&candidate, &inputs).gated {
        Ok(_) => Ok(()),
        Err(rej) => Err(rej.check),
    }
}

fn long_pos(qty: i64, entry: i64) -> PerpPosition {
    PerpPosition {
        market: MarketId::new("KXBTCPERP").unwrap(),
        qty: Contracts::new(qty),
        avg_entry: PerpPrice::new(entry),
    }
}

fn margin_account(balance: i64, mark_pair: PerpMarks, pending_funding: i64) -> MarginAccountView {
    MarginAccountView::compute(
        Cents::new(balance),
        &[(long_pos(1_000, 62_600), mark_pair)],
        Cents::new(pending_funding),
    )
    .unwrap()
}

fn both(mark: i64) -> PerpMarks {
    PerpMarks {
        venue_settlement: PerpPrice::new(mark),
        conservative: Some(PerpPrice::new(mark)),
    }
}

/// An adverse perp mark move alone — no event-contract loss anywhere —
/// must drive the full breach => halt => human-re-arm lifecycle.
#[test]
fn perp_i2_mark_loss_breaches_and_locks_until_rearm() {
    let limit = Cents::new(50_000);
    let mut monitor = DrawdownMonitor::new(limit);
    let mut gates = pipeline();
    let event_equity = Cents::new(1_000_000);

    // Day start: position marked at entry, uPnL 0.
    let healthy = margin_account(100_000, both(62_600), 0);
    let start = equity_with_margin(event_equity, &[healthy]).unwrap();
    assert_eq!(start.total, Cents::new(1_100_000));
    assert_eq!(
        monitor.check(at_ms(0), start.total).unwrap(),
        DrawdownVerdict::Ok
    );
    assert!(try_order(&mut gates, 1, at_ms(0)).is_ok());

    // The mark drops $0.50: uPnL -5,000 tt x 1,000 = -50,000c. Event
    // equity is UNCHANGED; the loss is pure margin unrealized PnL.
    let bleeding = margin_account(100_000, both(57_600), 0);
    let now_equity = equity_with_margin(event_equity, &[bleeding]).unwrap();
    assert_eq!(now_equity.total, Cents::new(1_050_000));
    let verdict = monitor.check(at_ms(3_600_000), now_equity.total).unwrap();
    let DrawdownVerdict::Breach { loss, .. } = verdict else {
        panic!("perp mark loss at the limit must breach, got {verdict:?}");
    };
    assert_eq!(loss, limit);

    // Breach reaches the gates as a halt; nothing passes; recovery does
    // not clear it; only the operator re-arm does (I2 unchanged).
    gates.set_halt(HaltScope::Global, format!("drawdown breach: {loss}"));
    assert_eq!(
        try_order(&mut gates, 2, at_ms(3_700_000)),
        Err(GateCheck::Halts)
    );
    let recovered = margin_account(100_000, both(62_600), 0);
    let rec_equity = equity_with_margin(event_equity, &[recovered]).unwrap();
    assert!(matches!(
        monitor.check(at_ms(4_000_000), rec_equity.total).unwrap(),
        DrawdownVerdict::Breach { .. }
    ));
    assert_eq!(
        try_order(&mut gates, 3, at_ms(4_000_000)),
        Err(GateCheck::Halts)
    );
    gates.rearm(HaltScope::Global).unwrap();
    assert!(try_order(&mut gates, 4, at_ms(4_100_000)).is_ok());
}

/// Funding paid is drawdown (spec 5.15): a pending funding debit alone
/// reaches the breach threshold.
#[test]
fn perp_i2_funding_paid_breaches() {
    let mut monitor = DrawdownMonitor::new(Cents::new(50_000));
    let event_equity = Cents::new(1_000_000);

    let start =
        equity_with_margin(event_equity, &[margin_account(100_000, both(62_600), 0)]).unwrap();
    assert_eq!(
        monitor.check(at_ms(0), start.total).unwrap(),
        DrawdownVerdict::Ok
    );

    // Funding debit of exactly the daily limit, mark unchanged.
    let after_funding = equity_with_margin(
        event_equity,
        &[margin_account(100_000, both(62_600), -50_000)],
    )
    .unwrap();
    assert!(matches!(
        monitor.check(at_ms(1_000), after_funding.total).unwrap(),
        DrawdownVerdict::Breach { .. }
    ));
}

/// The worse-for-us mark governs halt math (spec 5.15): a venue settlement
/// mark that still says "whole" cannot mask the loss our conservative mark
/// sees. The breach must fire from the conservative number.
#[test]
fn perp_i2_worse_for_us_mark_governs_breach() {
    let mut monitor = DrawdownMonitor::new(Cents::new(50_000));
    let event_equity = Cents::new(1_000_000);

    let start =
        equity_with_margin(event_equity, &[margin_account(100_000, both(62_600), 0)]).unwrap();
    monitor.check(at_ms(0), start.total).unwrap();

    // Venue mark unchanged (uPnL 0 by the venue's lights); our
    // conservative mark sees -50,000c.
    let disagreeing = margin_account(
        100_000,
        PerpMarks {
            venue_settlement: PerpPrice::new(62_600),
            conservative: Some(PerpPrice::new(57_600)),
        },
        0,
    );
    let composed = equity_with_margin(event_equity, &[disagreeing]).unwrap();
    assert_eq!(composed.total, Cents::new(1_050_000));
    assert!(matches!(
        monitor.check(at_ms(1_000), composed.total).unwrap(),
        DrawdownVerdict::Breach { .. }
    ));
}
