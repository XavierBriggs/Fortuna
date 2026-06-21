//! T0.5 tests: gate pipeline checks 1-10. Written from spec 5.3 before
//! implementation.
//!
//! Contract: ordered, fail-closed checks; each evaluated check emits an audit
//! record with verdict and reason; the first rejection stops evaluation.
//! Gate coverage is literal (PROMPT doctrine): every check has at least one
//! passing, one rejecting, and one boundary case.

use fortuna_core::book::{FeeModel, OrderBook, PriceLevel};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{EventId, IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_gates::{
    CandidateOrder, GateCheck, GateConfig, GateInputs, GatePipeline, HaltScope, RestingOrderView,
    Verdict,
};
use std::collections::BTreeSet;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-09T12:00:00.000Z").unwrap()
}

fn config_toml(min_edge_bps: i64) -> String {
    format!(
        r#"
        [global]
        max_total_exposure_cents = 100000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 20
        max_cross_cents = 5
        per_market_exposure_cents = 50000
        per_event_exposure_cents = 40000
        require_event_mapping = false

        [per_strategy.test_strat]
        max_exposure_cents = 50000
        max_order_notional_cents = 10000
        min_net_edge_bps = {min_edge_bps}

        [rate.sim]
        burst = 3
        sustained_per_min = 60
        market_burst = 2
        market_sustained_per_min = 30
        "#
    )
}

fn pipeline() -> GatePipeline {
    pipeline_with_edge_floor(100)
}

fn pipeline_with_edge_floor(bps: i64) -> GatePipeline {
    let cfg: GateConfig = toml::from_str(&config_toml(bps)).unwrap();
    GatePipeline::new(cfg).unwrap()
}

struct Fees;
impl FeeModel for Fees {
    fn fee(
        &self,
        role: fortuna_core::book::FillRole,
        price: Cents,
        qty: Contracts,
        category: Option<&str>,
        at: UtcTimestamp,
    ) -> Result<Cents, fortuna_core::book::FeeError> {
        // Kalshi-style quadratic 0.07 taker / 0.0175 maker, ceil.
        let model = test_fee_model();
        model.fee(role, price, qty, category, at)
    }
}

fn test_fee_model() -> fortuna_venues::fees::ScheduleFeeModel {
    let s: fortuna_venues::fees::FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            maker_coeff = "0.0175"
        "#,
    )
    .unwrap();
    fortuna_venues::fees::ScheduleFeeModel::new(vec![s]).unwrap()
}

fn book() -> OrderBook {
    OrderBook {
        market: MarketId::new("TEST-MKT").unwrap(),
        as_of: t0(),
        yes_bids: vec![PriceLevel {
            price: Cents::new(48),
            qty: Contracts::new(50),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(52),
            qty: Contracts::new(50),
        }],
    }
}

struct Setup {
    book: OrderBook,
    fees: Fees,
    recent: BTreeSet<String>,
    resting: Vec<RestingOrderView>,
    event: EventId,
}

impl Setup {
    fn new() -> Setup {
        let mut g = IdGen::new(99);
        Setup {
            book: book(),
            fees: Fees,
            recent: BTreeSet::new(),
            resting: Vec::new(),
            event: EventId::new(g.next(t0()).unwrap()),
        }
    }

    fn inputs(&self) -> GateInputs<'_> {
        GateInputs {
            now: t0(),
            open_exposure_cents: Cents::ZERO,
            market_exposure_cents: Cents::ZERO,
            strategy_exposure_cents: Cents::ZERO,
            event_exposure_cents: Cents::ZERO,
            event_id: Some(self.event),
            book: Some(&self.book),
            last_trade_price: None,
            fee_model: &self.fees,
            category: Some("test"),
            own_resting: &self.resting,
            recent_client_order_ids: &self.recent,
        }
    }
}

fn candidate(coid: &str) -> CandidateOrder {
    let mut g = IdGen::new(7);
    CandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("test_strat").unwrap(),
        venue: VenueId::new("sim").unwrap(),
        market: MarketId::new("TEST-MKT").unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(50),
        qty: Contracts::new(10),
        fair_value: Cents::new(55),
        client_order_id: ClientOrderId::new(coid).unwrap(),
    }
}

// ---- happy path + audit completeness ----

#[test]
fn clean_order_passes_all_eleven_checks_with_full_audit_trail() {
    let mut p = pipeline();
    let s = Setup::new();
    let out = p.evaluate(&candidate("c1"), &s.inputs());
    let gated = out.gated.expect("clean order must pass");
    assert_eq!(gated.limit_price(), Cents::new(50));
    assert_eq!(gated.qty().raw(), 10);
    assert_eq!(gated.client_order_id().as_str(), "c1");
    // One record per check, in pipeline order, all Pass.
    assert_eq!(out.records.len(), GateCheck::ALL.len());
    for (i, r) in out.records.iter().enumerate() {
        assert_eq!(r.check.index(), i + 1, "records must be in check order");
        assert_eq!(r.verdict, Verdict::Pass);
    }
}

#[test]
fn rejection_stops_evaluation_at_the_failing_check() {
    let mut p = pipeline();
    p.set_halt(HaltScope::Global, "test halt");
    let s = Setup::new();
    // Also make the size invalid: the halt (check 1) must win.
    let mut c = candidate("c1");
    c.qty = Contracts::new(0);
    let out = p.evaluate(&c, &s.inputs());
    let rejection = out.gated.unwrap_err();
    assert_eq!(rejection.check, GateCheck::Halts);
    assert_eq!(out.records.len(), 1);
    assert_eq!(out.records[0].verdict, Verdict::Reject);
    assert!(out.records[0].reason.contains("test halt"));
}

// ---- check 1: halts ----

#[test]
fn halts_global_strategy_and_venue_each_block() {
    let s = Setup::new();
    for scope in [
        HaltScope::Global,
        HaltScope::Strategy("test_strat".into()),
        HaltScope::Venue("sim".into()),
    ] {
        let mut p = pipeline();
        p.set_halt(scope, "halted for test");
        let out = p.evaluate(&candidate("c1"), &s.inputs());
        assert_eq!(out.gated.unwrap_err().check, GateCheck::Halts);
    }
}

#[test]
fn unrelated_strategy_or_venue_halt_does_not_block() {
    let s = Setup::new();
    let mut p = pipeline();
    p.set_halt(HaltScope::Strategy("other_strat".into()), "other");
    p.set_halt(HaltScope::Venue("kalshi".into()), "other venue");
    assert!(p.evaluate(&candidate("c1"), &s.inputs()).gated.is_ok());
}

#[test]
fn rearm_clears_a_halt() {
    let s = Setup::new();
    let mut p = pipeline();
    p.set_halt(HaltScope::Global, "halted");
    assert!(p.evaluate(&candidate("c1"), &s.inputs()).gated.is_err());
    p.rearm(HaltScope::Global).unwrap();
    assert!(p.evaluate(&candidate("c2"), &s.inputs()).gated.is_ok());
    // Re-arming a clear scope is an error (no silent no-ops on operator paths).
    assert!(p.rearm(HaltScope::Global).is_err());
}

// ---- check 2: account capital threshold ----
// Worst-case order cost = notional + max(maker, taker) fee at the limit:
// 10 x 50c = 500 + ceil(0.07*10*0.25*100)=18 -> 518.

#[test]
fn capital_threshold_boundary() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut inputs = s.inputs();
    inputs.open_exposure_cents = Cents::new(100_000 - 518); // exactly at cap
    assert!(p.evaluate(&candidate("c1"), &inputs).gated.is_ok());
    inputs.open_exposure_cents = Cents::new(100_000 - 517); // one cent over
    let out = p.evaluate(&candidate("c2"), &inputs);
    assert_eq!(out.gated.unwrap_err().check, GateCheck::Capital);
}

// ---- check 3: per-market and per-strategy position caps ----

#[test]
fn per_market_cap_boundary() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut inputs = s.inputs();
    inputs.market_exposure_cents = Cents::new(50_000 - 518);
    assert!(p.evaluate(&candidate("c1"), &inputs).gated.is_ok());
    inputs.market_exposure_cents = Cents::new(50_000 - 517);
    assert_eq!(
        p.evaluate(&candidate("c2"), &inputs)
            .gated
            .unwrap_err()
            .check,
        GateCheck::PositionCaps
    );
}

#[test]
fn per_strategy_cap_boundary() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut inputs = s.inputs();
    inputs.strategy_exposure_cents = Cents::new(50_000 - 518);
    assert!(p.evaluate(&candidate("c1"), &inputs).gated.is_ok());
    inputs.strategy_exposure_cents = Cents::new(50_000 - 517);
    assert_eq!(
        p.evaluate(&candidate("c2"), &inputs)
            .gated
            .unwrap_err()
            .check,
        GateCheck::PositionCaps
    );
}

#[test]
fn unknown_strategy_is_fail_closed() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.strategy = StrategyId::new("never_configured").unwrap();
    let out = p.evaluate(&c, &s.inputs());
    assert_eq!(out.gated.unwrap_err().check, GateCheck::PositionCaps);
}

// ---- check 4: price sanity ----

#[test]
fn price_band_vs_mid_boundary() {
    // Mid = (48 + 52) / 2 = 50; band 20 -> [30, 70].
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.limit_price = Cents::new(30);
    c.fair_value = Cents::new(45); // keep the edge floor satisfied
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());
    let mut c = candidate("c2");
    c.limit_price = Cents::new(29);
    c.fair_value = Cents::new(45);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::PriceSanity
    );
}

#[test]
fn buy_crossing_beyond_max_slippage_rejected() {
    // Best ask 52, max_cross 5: buy limit 57 ok, 58 rejected.
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.limit_price = Cents::new(57);
    c.fair_value = Cents::new(70);
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());
    let mut c = candidate("c2");
    c.limit_price = Cents::new(58);
    c.fair_value = Cents::new(70);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::PriceSanity
    );
}

#[test]
fn sell_crossing_beyond_max_slippage_rejected() {
    // Best bid 48, max_cross 5: sell limit 43 ok, 42 rejected.
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.action = Action::Sell;
    c.limit_price = Cents::new(43);
    c.fair_value = Cents::new(30);
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());
    let mut c = candidate("c2");
    c.action = Action::Sell;
    c.limit_price = Cents::new(42);
    c.fair_value = Cents::new(30);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::PriceSanity
    );
}

#[test]
fn no_price_reference_is_fail_closed() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut inputs = s.inputs();
    inputs.book = None;
    inputs.last_trade_price = None;
    assert_eq!(
        p.evaluate(&candidate("c1"), &inputs)
            .gated
            .unwrap_err()
            .check,
        GateCheck::PriceSanity
    );
    // Last trade alone is an acceptable reference.
    inputs.last_trade_price = Some(Cents::new(50));
    assert!(p.evaluate(&candidate("c2"), &inputs).gated.is_ok());
}

#[test]
fn no_side_prices_are_checked_in_yes_space() {
    // Buy NO at 53 == YES-space 47; mid 50, band 20 -> fine. At NO 85,
    // YES-space 15 is within [30,70]? No: 15 < 30 -> reject.
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.side = Side::No;
    c.limit_price = Cents::new(53);
    c.fair_value = Cents::new(60); // NO-space fair: edge = (60-53)*10
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());
    let mut c = candidate("c2");
    c.side = Side::No;
    c.limit_price = Cents::new(85);
    c.fair_value = Cents::new(95);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::PriceSanity
    );
}

// ---- check 5: size sanity ----

#[test]
fn size_bounds_and_notional_cap_boundaries() {
    let s = Setup::new();
    let mut p = pipeline();

    let mut c = candidate("c0");
    c.qty = Contracts::new(0);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::SizeSanity
    );

    // Notional cap 10_000: 200 x 50c == 10_000 exactly -> pass.
    let mut c = candidate("c1");
    c.qty = Contracts::new(200);
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());

    // 201 x 50c = 10_050 -> reject.
    let mut c = candidate("c2");
    c.qty = Contracts::new(201);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::SizeSanity
    );

    // Max contracts 1000: need a tiny price so the notional cap allows it.
    let mut c = candidate("c3");
    c.qty = Contracts::new(1001);
    c.limit_price = Cents::new(31); // within band
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::SizeSanity
    );
}

// ---- check 6: fee-adjusted edge floor ----
// qty 10 @ 50c: worst fee 18c; fair 52 -> gross 20c, net 2c, notional 500c
// -> exactly 40 bps.

#[test]
fn edge_floor_boundary() {
    let s = Setup::new();
    let mut p = pipeline_with_edge_floor(40);
    let mut c = candidate("c1");
    c.fair_value = Cents::new(52);
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());

    let mut p = pipeline_with_edge_floor(41);
    let mut c = candidate("c2");
    c.fair_value = Cents::new(52);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::EdgeFloor
    );
}

#[test]
fn negative_edge_rejected_even_with_zero_floor() {
    let s = Setup::new();
    let mut p = pipeline_with_edge_floor(0);
    let mut c = candidate("c1");
    c.fair_value = Cents::new(49); // buying above fair
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::EdgeFloor
    );
}

#[test]
fn sell_edge_is_limit_minus_fair() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.action = Action::Sell;
    c.limit_price = Cents::new(50);
    c.fair_value = Cents::new(45); // selling above fair: gross 50c on 10
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());
}

#[test]
fn insane_fair_value_rejected() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.fair_value = Cents::new(101);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::EdgeFloor
    );
}

// ---- check 7: rate limits (I3: breach is a halt, not a throttle) ----

#[test]
fn market_bucket_breach_sets_venue_halt() {
    let s = Setup::new();
    let mut p = pipeline();
    assert!(p.evaluate(&candidate("c1"), &s.inputs()).gated.is_ok());
    assert!(p.evaluate(&candidate("c2"), &s.inputs()).gated.is_ok());
    // market_burst = 2: the third order on the same market breaches.
    let out = p.evaluate(&candidate("c3"), &s.inputs());
    assert_eq!(out.gated.unwrap_err().check, GateCheck::RateLimits);
    // The breach HALTED the venue: the next order dies at check 1.
    let out = p.evaluate(&candidate("c4"), &s.inputs());
    assert_eq!(out.gated.unwrap_err().check, GateCheck::Halts);
}

#[test]
fn venue_bucket_breach_across_markets_sets_venue_halt() {
    let s = Setup::new();
    let mut p = pipeline();
    for (i, mkt) in ["M1", "M2", "M3"].iter().enumerate() {
        let mut c = candidate(&format!("c{i}"));
        c.market = MarketId::new(*mkt).unwrap();
        let mut book = book();
        book.market = c.market.clone();
        let mut inputs = s.inputs();
        inputs.book = Some(&book);
        assert!(p.evaluate(&c, &inputs).gated.is_ok(), "order {i}");
    }
    // venue burst = 3: the fourth order (fresh market) breaches the venue bucket.
    let mut c = candidate("c4");
    c.market = MarketId::new("M4").unwrap();
    let mut book = book();
    book.market = c.market.clone();
    let mut inputs = s.inputs();
    inputs.book = Some(&book);
    let out = p.evaluate(&c, &inputs);
    assert_eq!(out.gated.unwrap_err().check, GateCheck::RateLimits);
    assert!(p.halts().venue_halted("sim").is_some());
}

#[test]
fn rate_halt_does_not_clear_with_time() {
    // I3: a halt, not a throttle. Refill must never un-halt.
    let s = Setup::new();
    let mut p = pipeline();
    for i in 0..2 {
        assert!(p
            .evaluate(&candidate(&format!("c{i}")), &s.inputs())
            .gated
            .is_ok());
    }
    assert!(p.evaluate(&candidate("c2"), &s.inputs()).gated.is_err()); // breach
    let mut inputs = s.inputs();
    inputs.now = t0().checked_add_millis(3_600_000).unwrap(); // an hour later
    let out = p.evaluate(&candidate("c5"), &inputs);
    assert_eq!(out.gated.unwrap_err().check, GateCheck::Halts);
}

#[test]
fn sustained_refill_grants_tokens_below_breach() {
    let s = Setup::new();
    let mut p = pipeline();
    // Drain the venue bucket exactly (3 orders across 3 markets, market
    // buckets stay at 1/2 each).
    for (i, mkt) in ["M1", "M2", "M3"].iter().enumerate() {
        let mut c = candidate(&format!("c{i}"));
        c.market = MarketId::new(*mkt).unwrap();
        let mut book = book();
        book.market = c.market.clone();
        let mut inputs = s.inputs();
        inputs.book = Some(&book);
        assert!(p.evaluate(&c, &inputs).gated.is_ok());
    }
    // 60/min sustained = 1 token/sec: one second later one more order fits.
    let mut c = candidate("c-later");
    c.market = MarketId::new("M1").unwrap();
    let mut book = book();
    book.market = c.market.clone();
    let mut inputs = s.inputs();
    inputs.book = Some(&book);
    inputs.now = t0().checked_add_millis(1_000).unwrap();
    assert!(p.evaluate(&c, &inputs).gated.is_ok());
    assert!(p.halts().venue_halted("sim").is_none());
}

#[test]
fn orders_rejected_before_check_7_do_not_consume_tokens() {
    let s = Setup::new();
    let mut p = pipeline();
    for i in 0..5 {
        let mut c = candidate(&format!("bad{i}"));
        c.qty = Contracts::new(0); // dies at size sanity
        assert!(p.evaluate(&c, &s.inputs()).gated.is_err());
    }
    // Bucket untouched: two good orders still pass.
    assert!(p.evaluate(&candidate("g1"), &s.inputs()).gated.is_ok());
    assert!(p.evaluate(&candidate("g2"), &s.inputs()).gated.is_ok());
}

#[test]
fn unknown_venue_rate_config_is_fail_closed() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.venue = VenueId::new("unconfigured_venue").unwrap();
    let out = p.evaluate(&c, &s.inputs());
    assert_eq!(out.gated.unwrap_err().check, GateCheck::RateLimits);
}

// ---- check 8: idempotency ----

#[test]
fn duplicate_client_order_id_rejected() {
    let mut s = Setup::new();
    s.recent.insert("dup-coid".to_string());
    let mut p = pipeline();
    let out = p.evaluate(&candidate("dup-coid"), &s.inputs());
    assert_eq!(out.gated.unwrap_err().check, GateCheck::Idempotency);
    assert!(p.evaluate(&candidate("fresh"), &s.inputs()).gated.is_ok());
}

// ---- check 9: same-event exposure cap ----

#[test]
fn event_exposure_boundary() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut inputs = s.inputs();
    inputs.event_exposure_cents = Cents::new(40_000 - 518);
    assert!(p.evaluate(&candidate("c1"), &inputs).gated.is_ok());
    inputs.event_exposure_cents = Cents::new(40_000 - 517);
    assert_eq!(
        p.evaluate(&candidate("c2"), &inputs)
            .gated
            .unwrap_err()
            .check,
        GateCheck::EventExposure
    );
}

#[test]
fn missing_event_mapping_passes_when_not_required() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut inputs = s.inputs();
    inputs.event_id = None;
    let out = p.evaluate(&candidate("c1"), &inputs);
    assert!(out.gated.is_ok());
    // The audit record documents that the cap could not bind.
    let rec = &out.records[GateCheck::EventExposure.index() - 1];
    assert!(rec.reason.contains("no event mapping"));
}

#[test]
fn missing_event_mapping_rejects_when_required() {
    let cfg_src = config_toml(100).replace(
        "require_event_mapping = false",
        "require_event_mapping = true",
    );
    let cfg: GateConfig = toml::from_str(&cfg_src).unwrap();
    let mut p = GatePipeline::new(cfg).unwrap();
    let s = Setup::new();
    let mut inputs = s.inputs();
    inputs.event_id = None;
    assert_eq!(
        p.evaluate(&candidate("c1"), &inputs)
            .gated
            .unwrap_err()
            .check,
        GateCheck::EventExposure
    );
}

// ---- check 10: internal netting ----

#[test]
fn order_crossing_own_resting_ask_rejected() {
    let mut s = Setup::new();
    s.resting.push(RestingOrderView {
        market: MarketId::new("TEST-MKT").unwrap(),
        side: Side::Yes,
        action: Action::Sell,
        price: Cents::new(52),
    });
    let mut p = pipeline();
    // Buy at 52 would cross our own ask at 52.
    let mut c = candidate("c1");
    c.limit_price = Cents::new(52);
    c.fair_value = Cents::new(60);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::InternalNetting
    );
    // Buy at 51 rests below our ask: fine.
    let mut c = candidate("c2");
    c.limit_price = Cents::new(51);
    c.fair_value = Cents::new(60);
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());
}

#[test]
fn no_side_order_crossing_own_yes_bid_rejected() {
    // Our resting yes-bid at 48. Buying NO at 53 is a yes-ask at 47 <= 48:
    // we would trade against ourselves.
    let mut s = Setup::new();
    s.resting.push(RestingOrderView {
        market: MarketId::new("TEST-MKT").unwrap(),
        side: Side::Yes,
        action: Action::Buy,
        price: Cents::new(48),
    });
    let mut p = pipeline();
    let mut c = candidate("c1");
    c.side = Side::No;
    c.limit_price = Cents::new(53);
    c.fair_value = Cents::new(60);
    assert_eq!(
        p.evaluate(&c, &s.inputs()).gated.unwrap_err().check,
        GateCheck::InternalNetting
    );
    // NO at 51 == yes-ask 49 > 48: rests without crossing.
    let mut c = candidate("c2");
    c.side = Side::No;
    c.limit_price = Cents::new(51);
    c.fair_value = Cents::new(60);
    assert!(p.evaluate(&c, &s.inputs()).gated.is_ok());
}

#[test]
fn own_resting_on_other_markets_is_ignored() {
    let mut s = Setup::new();
    s.resting.push(RestingOrderView {
        market: MarketId::new("OTHER-MKT").unwrap(),
        side: Side::Yes,
        action: Action::Sell,
        price: Cents::new(50),
    });
    let mut p = pipeline();
    assert!(p.evaluate(&candidate("c1"), &s.inputs()).gated.is_ok());
}

#[test]
fn mismatched_book_market_is_fail_closed() {
    let s = Setup::new();
    let mut p = pipeline();
    let mut wrong_book = book();
    wrong_book.market = MarketId::new("SOME-OTHER-MKT").unwrap();
    let mut inputs = s.inputs();
    inputs.book = Some(&wrong_book);
    assert_eq!(
        p.evaluate(&candidate("c1"), &inputs)
            .gated
            .unwrap_err()
            .check,
        GateCheck::PriceSanity
    );
}

// ---- config hot-reload (operator path) ----

#[test]
fn reload_config_applies_new_limits() {
    let s = Setup::new();
    let mut p = pipeline();
    assert!(p.evaluate(&candidate("c1"), &s.inputs()).gated.is_ok());
    // Tighten the notional cap below the candidate's 500c.
    let cfg: GateConfig = toml::from_str(&config_toml(100).replace(
        "max_order_notional_cents = 10000",
        "max_order_notional_cents = 400",
    ))
    .unwrap();
    p.reload_config(cfg).unwrap();
    assert_eq!(
        p.evaluate(&candidate("c2"), &s.inputs())
            .gated
            .unwrap_err()
            .check,
        GateCheck::SizeSanity
    );
}

#[test]
fn reload_config_never_clears_halts() {
    // A config push must not be a re-arm path (I2): halts survive reload.
    let s = Setup::new();
    let mut p = pipeline();
    p.set_halt(HaltScope::Global, "halted before reload");
    let cfg: GateConfig = toml::from_str(&config_toml(100)).unwrap();
    p.reload_config(cfg).unwrap();
    assert_eq!(
        p.evaluate(&candidate("c1"), &s.inputs())
            .gated
            .unwrap_err()
            .check,
        GateCheck::Halts
    );
}

// ---- check 11: book-age freshness ----

fn pipeline_with_book_age(max_ms: i64) -> GatePipeline {
    // Prepend max_book_age_ms before the first section header so it belongs
    // to the root GateConfig (not to [rate.sim] or any other sub-table).
    let src = format!("max_book_age_ms = {}\n{}", max_ms, config_toml(100));
    let cfg: GateConfig = toml::from_str(&src).unwrap();
    GatePipeline::new(cfg).unwrap()
}

#[test]
fn stale_book_is_rejected_when_max_book_age_configured() {
    // Book as_of is 2000ms before now; max_book_age_ms = 1000 → reject.
    let s = Setup::new();
    let mut p = pipeline_with_book_age(1000);
    let mut stale_book = book();
    stale_book.as_of = t0().checked_add_millis(-2000).unwrap();
    let mut inputs = s.inputs();
    inputs.book = Some(&stale_book);
    let out = p.evaluate(&candidate("c1"), &inputs);
    let rejection = out.gated.unwrap_err();
    assert_eq!(rejection.check, GateCheck::BookAge);
    assert!(
        rejection.reason.contains("stale"),
        "reason must mention 'stale': {}",
        rejection.reason
    );
}

#[test]
fn fresh_book_passes_when_max_book_age_configured() {
    // Book as_of is 500ms before now; max_book_age_ms = 1000 → pass.
    let s = Setup::new();
    let mut p = pipeline_with_book_age(1000);
    let mut fresh_book = book();
    fresh_book.as_of = t0().checked_add_millis(-500).unwrap();
    let mut inputs = s.inputs();
    inputs.book = Some(&fresh_book);
    assert!(p.evaluate(&candidate("c1"), &inputs).gated.is_ok());
}

#[test]
fn book_age_disabled_when_max_book_age_not_set() {
    // Default config (no max_book_age_ms) → check is a no-op even for ancient book.
    let s = Setup::new();
    let mut p = pipeline(); // no max_book_age_ms
    let mut ancient_book = book();
    ancient_book.as_of = t0().checked_add_millis(-86_400_000).unwrap(); // 24h ago
    let mut inputs = s.inputs();
    inputs.book = Some(&ancient_book);
    assert!(p.evaluate(&candidate("c1"), &inputs).gated.is_ok());
}

#[test]
fn book_age_passes_when_no_book_provided() {
    // book=None → check passes (book absence is PriceSanity's concern, not BookAge's).
    // Give a last_trade_price so PriceSanity doesn't block us.
    let s = Setup::new();
    let mut p = pipeline_with_book_age(1000);
    let mut inputs = s.inputs();
    inputs.book = None;
    inputs.last_trade_price = Some(Cents::new(50));
    assert!(p.evaluate(&candidate("c1"), &inputs).gated.is_ok());
}

#[test]
fn book_age_check_attributed_correctly_in_rejection_record() {
    // Pipeline-level: rejection is attributed to BookAge in the audit trail.
    let s = Setup::new();
    let mut p = pipeline_with_book_age(500);
    let mut stale_book = book();
    stale_book.as_of = t0().checked_add_millis(-2000).unwrap();
    let mut inputs = s.inputs();
    inputs.book = Some(&stale_book);
    let out = p.evaluate(&candidate("c1"), &inputs);
    let last = out.records.last().unwrap();
    assert_eq!(last.verdict, Verdict::Reject);
    assert_eq!(last.check, GateCheck::BookAge);
}
