//! T1.3: mech_extremes (spec Section 6 item 2). Favorite-longshot fading
//! at price extremes in sub-$100k-volume markets, MAKER-ONLY.
//!
//! Doctrine under test:
//! - Only markets at a price extreme (favorite side at/above the
//!   configured threshold) produce proposals; the proposal BUYS the
//!   favorite (fading the overpriced longshot is buying the underpriced
//!   favorite in a binary market).
//! - Maker-only: the limit JOINS the own-side best bid and never crosses
//!   the counter ask; urgency is Passive.
//! - Volume cap: `volume_contracts` x $1/pair bounds dollar volume from
//!   above, so contracts <= 100_000 IS the spec's sub-$100k criterion.
//!   Unknown volume = SKIP (never assume small).
//! - Trading-status and time-to-close guards; one shot per
//!   (market, side, limit) book state.
//!
//! Written BEFORE src/mech_extremes.rs per the repository TDD doctrine.

use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_runner::mech_extremes::{MechExtremes, MechExtremesConfig};
use fortuna_runner::{CoreHandle, Strategy, Urgency};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::{Market, MarketStatus, SettlementMeta};
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap()
}

fn hours_later(h: i64) -> UtcTimestamp {
    t0().checked_add_millis(h * 3_600_000).unwrap()
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

fn market(id: &str, volume: Option<i64>) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("extreme {id}"),
        category: "politics".into(),
        status: MarketStatus::Trading,
        close_at: Some(hours_later(48)),
        settlement: SettlementMeta {
            oracle_type: "ap".into(),
            resolution_source: "ap".into(),
            expected_lag_hours: 2,
        },
        volume_contracts: volume,
        payout_per_contract: Cents::new(100),
    }
}

fn book(id: &str, bid: i64, ask: i64) -> OrderBook {
    OrderBook {
        market: mkt(id),
        as_of: t0(),
        yes_bids: vec![PriceLevel {
            price: Cents::new(bid),
            qty: Contracts::new(50),
        }],
        yes_asks: vec![PriceLevel {
            price: Cents::new(ask),
            qty: Contracts::new(50),
        }],
    }
}

fn config() -> MechExtremesConfig {
    MechExtremesConfig {
        extreme_min_cents: 90,
        bias_premium_cents: 2,
        max_volume_contracts: 100_000,
        min_ms_to_close: 3_600_000, // 1h
    }
}

fn snapshot_event(b: &OrderBook) -> BusEvent {
    BusEvent {
        seq: 1,
        at: t0(),
        origin: EventOrigin::External,
        payload: EventPayload::BookSnapshot {
            venue: VenueId::new("sim").unwrap(),
            book: b.clone(),
        },
    }
}

struct World {
    books: BTreeMap<MarketId, OrderBook>,
    markets: BTreeMap<MarketId, Market>,
    fees: ScheduleFeeModel,
}

impl World {
    fn new(market_def: Market, b: OrderBook) -> World {
        World {
            books: BTreeMap::from([(b.market.clone(), b)]),
            markets: BTreeMap::from([(market_def.id.clone(), market_def)]),
            fees: fee_model(),
        }
    }

    fn handle(&self) -> CoreHandle<'_> {
        CoreHandle {
            now: t0(),
            books: &self.books,
            markets: &self.markets,
            fee_model: &self.fees,
        }
    }
}

fn propose(strategy: &mut MechExtremes, w: &World, b: &OrderBook) -> Vec<fortuna_runner::Proposal> {
    futures::executor::block_on(strategy.on_event(&snapshot_event(b), &w.handle())).unwrap()
}

// ----------------------------------------------------------- the YES fade

#[test]
fn proposes_yes_fade_at_extreme_under_volume_cap() {
    let b = book("KXEXT", 92, 93);
    let w = World::new(market("KXEXT", Some(50_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();

    let out = propose(&mut s, &w, &b);
    assert_eq!(out.len(), 1);
    let p = &out[0];
    assert_eq!(p.legs.len(), 1, "single-leg by construction");
    assert_eq!(p.urgency, Urgency::Passive, "maker-only");
    let leg = &p.legs[0];
    assert_eq!(leg.market, mkt("KXEXT"));
    assert_eq!(leg.side, Side::Yes, "favorite side is YES at 92 bid");
    assert_eq!(leg.action, Action::Buy);
    assert_eq!(
        leg.limit_price,
        Cents::new(92),
        "JOINS the bid, never crosses"
    );
    assert_eq!(
        leg.fair_value,
        Cents::new(94),
        "fair = limit + bias premium (honest deterministic claim)"
    );
}

// ------------------------------------------------------------ the NO fade

#[test]
fn proposes_no_fade_when_yes_is_cheap() {
    // YES at 5/8 => the favorite is NO: no-space bid = 100-8 = 92.
    let b = book("KXEXT", 5, 8);
    let w = World::new(market("KXEXT", Some(50_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();

    let out = propose(&mut s, &w, &b);
    assert_eq!(out.len(), 1);
    let leg = &out[0].legs[0];
    assert_eq!(leg.side, Side::No);
    assert_eq!(leg.action, Action::Buy);
    assert_eq!(
        leg.limit_price,
        Cents::new(92),
        "joins the NO bid (100-ask)"
    );
    assert_eq!(leg.fair_value, Cents::new(94));
}

// ----------------------------------------------------------------- guards

#[test]
fn skips_market_over_the_volume_cap() {
    let b = book("KXEXT", 92, 93);
    let w = World::new(market("KXEXT", Some(150_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(propose(&mut s, &w, &b).is_empty());
}

#[test]
fn skips_market_with_unknown_volume() {
    let b = book("KXEXT", 92, 93);
    let w = World::new(market("KXEXT", None), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(
        propose(&mut s, &w, &b).is_empty(),
        "unknown volume must SKIP, never assume small"
    );
}

#[test]
fn skips_non_extreme_prices() {
    let b = book("KXEXT", 50, 52);
    let w = World::new(market("KXEXT", Some(50_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(propose(&mut s, &w, &b).is_empty());
}

#[test]
fn skips_markets_close_to_expiry() {
    let b = book("KXEXT", 92, 93);
    let mut m = market("KXEXT", Some(50_000));
    m.close_at = Some(t0().checked_add_millis(60_000).unwrap()); // 1 min out
    let w = World::new(m, b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(propose(&mut s, &w, &b).is_empty());
}

#[test]
fn skips_unknown_close_time() {
    let b = book("KXEXT", 92, 93);
    let mut m = market("KXEXT", Some(50_000));
    m.close_at = None;
    let w = World::new(m, b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(propose(&mut s, &w, &b).is_empty());
}

#[test]
fn skips_non_trading_status() {
    let b = book("KXEXT", 92, 93);
    let mut m = market("KXEXT", Some(50_000));
    m.status = MarketStatus::Halted;
    let w = World::new(m, b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(propose(&mut s, &w, &b).is_empty());
}

#[test]
fn skips_unknown_market_metadata() {
    // Book for a market the catalog has never described: no volume, no
    // status, no close time => no trade.
    let b = book("KXGHOST", 92, 93);
    let w = World::new(market("KXEXT", Some(50_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(propose(&mut s, &w, &b).is_empty());
}

#[test]
fn skips_one_sided_books() {
    let mut b = book("KXEXT", 92, 93);
    b.yes_asks.clear();
    let w = World::new(market("KXEXT", Some(50_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    assert!(
        propose(&mut s, &w, &b).is_empty(),
        "no counter ask => no non-cross guarantee => no quote"
    );
}

/// Fair value clamps at 99c; if clamping leaves no honest edge claim over
/// the limit, there is nothing to propose.
#[test]
fn skips_when_premium_cannot_clear_the_99c_clamp() {
    let b = book("KXEXT", 98, 99);
    let w = World::new(market("KXEXT", Some(50_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();
    let out = propose(&mut s, &w, &b);
    assert_eq!(out.len(), 1, "98+2 clamps to 99 which still exceeds 98");
    assert_eq!(out[0].legs[0].fair_value, Cents::new(99));

    let b2 = book("KXEXT2", 99, 100);
    // 99 bid: fair would clamp to 99 == limit, no honest edge -> skip.
    let mut w2 = World::new(market("KXEXT2", Some(50_000)), b2.clone());
    w2.markets
        .insert(mkt("KXEXT2"), market("KXEXT2", Some(50_000)));
    let mut s2 = MechExtremes::new(config()).unwrap();
    assert!(propose(&mut s2, &w2, &b2).is_empty());
}

// --------------------------------------------------------------- one-shot

#[test]
fn one_shot_per_market_side_and_limit() {
    let b = book("KXEXT", 92, 93);
    let w = World::new(market("KXEXT", Some(50_000)), b.clone());
    let mut s = MechExtremes::new(config()).unwrap();

    assert_eq!(propose(&mut s, &w, &b).len(), 1);
    assert!(
        propose(&mut s, &w, &b).is_empty(),
        "same book state must not re-propose"
    );

    // The bid moves: a NEW (market, side, limit) key may fire again.
    let b2 = book("KXEXT", 93, 94);
    let mut w2 = World::new(market("KXEXT", Some(50_000)), b2.clone());
    w2.markets
        .insert(mkt("KXEXT"), market("KXEXT", Some(50_000)));
    let out = propose(&mut s, &w2, &b2);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].legs[0].limit_price, Cents::new(93));
}

// ------------------------------------------------------------ validation

#[test]
fn config_validation_rejects_nonsense() {
    let ok = config();
    assert!(MechExtremes::new(ok.clone()).is_ok());

    let mut c = ok.clone();
    c.extreme_min_cents = 50; // not an extreme
    assert!(MechExtremes::new(c).is_err());

    let mut c = ok.clone();
    c.extreme_min_cents = 100; // unreachable price
    assert!(MechExtremes::new(c).is_err());

    let mut c = ok.clone();
    c.bias_premium_cents = 0; // no edge claim
    assert!(MechExtremes::new(c).is_err());

    let mut c = ok.clone();
    c.max_volume_contracts = 0; // filters everything
    assert!(MechExtremes::new(c).is_err());

    let mut c = ok;
    c.min_ms_to_close = -1;
    assert!(MechExtremes::new(c).is_err());
}

// ------------------------------------------------- maker-only invariant

/// Across a sweep of extreme books, the proposed limit NEVER crosses the
/// counter ask in own-side space (maker-only is structural, not luck).
#[test]
fn proposed_limits_never_cross() {
    for (bid, ask) in [(90, 91), (92, 93), (95, 99), (90, 99), (97, 98)] {
        let b = book("KXEXT", bid, ask);
        let w = World::new(market("KXEXT", Some(50_000)), b.clone());
        let mut s = MechExtremes::new(config()).unwrap();
        for p in propose(&mut s, &w, &b) {
            let leg = &p.legs[0];
            match leg.side {
                Side::Yes => assert!(
                    leg.limit_price.raw() < ask,
                    "YES limit {} must stay under ask {ask}",
                    leg.limit_price
                ),
                Side::No => assert!(
                    leg.limit_price.raw() < 100 - bid,
                    "NO limit {} must stay under no-ask {}",
                    leg.limit_price,
                    100 - bid
                ),
            }
        }
        // And the mirrored cheap-side books.
        let b = book("KXEXT", 100 - ask, 100 - bid);
        let w = World::new(market("KXEXT", Some(50_000)), b.clone());
        let mut s = MechExtremes::new(config()).unwrap();
        for p in propose(&mut s, &w, &b) {
            let leg = &p.legs[0];
            assert_eq!(leg.side, Side::No);
            assert!(leg.limit_price.raw() < 100 - (100 - ask));
        }
    }
}

// ------------------------------------------- the composed loop with veto

/// The spec'd composition (Section 6: mech_extremes ships WITH the model
/// veto): the fade flows sizing -> veto -> gates -> manager and RESTS
/// maker-side (joining the bid never crosses the sim book).
#[test]
fn fade_rests_maker_side_through_the_full_loop_with_veto() {
    use fortuna_cognition::veto::StubVetoMind;
    use fortuna_core::market::StrategyId;
    use fortuna_exec::ExecPolicy;
    use fortuna_runner::{MemoryAuditSink, RunnerConfig, SimRunner};
    use fortuna_state::MarkPolicy;
    use fortuna_venues::sim::FaultConfig;
    use std::sync::Arc;

    let gate_config: fortuna_gates::GateConfig = toml::from_str(
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

        [per_strategy.mech_extremes]
        max_exposure_cents = 200000
        max_order_notional_cents = 10000
        min_net_edge_bps = 150

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap();
    let config = RunnerConfig {
        seed: 37,
        gate_config,
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("mech_extremes".to_string(), Cents::new(100_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![market("KXEXT", Some(50_000))],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(37),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 10,
        kelly_fraction: 0.25,
        veto_mind: Some(Arc::new(StubVetoMind::allow_all())),
        veto_strategies: vec![StrategyId::new("mech_extremes").unwrap()],
    };
    let mut runner = SimRunner::new(
        config,
        vec![Box::new(MechExtremes::new(self::config()).unwrap())],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();
    runner
        .venue()
        .set_book(
            &mkt("KXEXT"),
            vec![PriceLevel {
                price: Cents::new(92),
                qty: Contracts::new(50),
            }],
            vec![PriceLevel {
                price: Cents::new(93),
                qty: Contracts::new(50),
            }],
        )
        .unwrap();

    let report = futures::executor::block_on(runner.tick()).unwrap();
    assert!(report.proposals >= 1, "the fade must fire");
    assert_eq!(report.orders_submitted, 1, "one maker order through gates");
    assert_eq!(
        report.fills_applied, 0,
        "joining the bid must REST, never cross (maker-only)"
    );
    assert_eq!(report.gate_rejections, 0);

    // The resting intent is the fade: buy YES at 92.
    let intents: Vec<_> = runner
        .manager()
        .intents()
        .iter()
        .map(|(_, r)| {
            (
                r.order.market.as_str().to_string(),
                r.order.side,
                r.order.limit_price.raw(),
            )
        })
        .collect();
    assert_eq!(intents, vec![("KXEXT".to_string(), Side::Yes, 92)]);
}
