//! C-next-2: paper-fill TRADE-THROUGH realism proven against REAL, recorded,
//! provenanced trade prints (NEVER synthetic).
//!
//! Spec 11: "maker fills in paper count ONLY when the market trades through the
//! limit price (not touches)." The companion unit tests in `paper.rs` prove this
//! with CONSTRUCTED prints; THIS test proves the SAME rule with the real price +
//! quantity parsed from a recorded public-trades capture
//! (`fixtures/kalshi/trades__public_recorded.json` — the PUBLIC, unauthenticated
//! `GET /trade-api/v2/markets/trades`; no credentials, no orders placed — see the
//! sibling `.meta.json` for provenance). The recorded market traded at a single
//! real price (3c), so the strict-through rule is shown by varying OUR resting
//! order's limit relative to that real print:
//!   - a 3c print is strictly THROUGH a buy at 4c  -> fills (haircut applied),
//!   - exactly AT a buy at 3c -> a touch, MUST NOT fill (the doctrine gate),
//!   - away from a buy at 2c -> MUST NOT fill.
//!
//! The trade price + quantity are read from the fixture (never hard-coded), so a
//! re-record with different real values still exercises the rule.

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_paper::{PaperConfig, PaperVenue};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::{Cursor, Market, MarketStatus, PriceLevel, SettlementMeta, Venue};
use std::sync::Arc;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/kalshi/trades__public_recorded.json"
);

/// The recorded market's real ticker (the fixture is its public trade prints).
const REAL_TICKER: &str = "KXPGATOUR-USO26-JDAY";

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-15T15:52:33.000Z").unwrap()
}

fn mkt() -> MarketId {
    MarketId::new(REAL_TICKER).unwrap()
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

fn test_market() -> Market {
    Market {
        id: mkt(),
        venue: VenueId::new("paper").unwrap(),
        title: format!("paper market {}", REAL_TICKER),
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
    }
}

fn lvl(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: Contracts::new(qty),
    }
}

/// A PaperVenue holding the recorded market, with a book straddling the real 3c
/// print (best bid 1c; asks 5c/6c) so resting buys at 2/3/4c are valid (non-
/// crossing) maker orders. `haircut_pct` is the configured quantity haircut.
fn paper(haircut_pct: u8) -> PaperVenue {
    let clock = Arc::new(SimClock::new(t0()));
    let venue = PaperVenue::new(
        VenueId::new("paper").unwrap(),
        clock,
        fee_model(),
        PaperConfig {
            maker_haircut_pct: haircut_pct,
        },
        Cents::new(100_000),
    )
    .unwrap();
    venue.add_market(test_market());
    venue
        .apply_book(&mkt(), vec![lvl(1, 50)], vec![lvl(5, 50), lvl(6, 50)])
        .unwrap();
    venue
}

/// Gate a resting maker BUY (yes) at `price` cents, size `qty`, through a
/// permissive real pipeline (I1 everywhere — even the test order is sealed).
fn gated_buy(seed: u64, price: i64, qty: i64) -> fortuna_gates::GatedOrder {
    use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};
    use std::collections::BTreeSet;
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

        [per_strategy.paper_test]
        max_exposure_cents = 10000000
        max_order_notional_cents = 10000000
        min_net_edge_bps = 0

        [rate.paper]
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
        strategy: StrategyId::new("paper_test").unwrap(),
        venue: VenueId::new("paper").unwrap(),
        market: mkt(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: Cents::new(price),
        qty: Contracts::new(qty),
        // A buy believes value above its limit (the gates verify the edge sign).
        fair_value: Cents::new((price + 5).min(99)),
        client_order_id: ClientOrderId::from_intent(intent),
    };
    let fees = fee_model();
    let recent = BTreeSet::new();
    let inputs = GateInputs {
        now: t0(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: None,
        last_trade_price: Some(Cents::new(3)),
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    pipeline.evaluate(&candidate, &inputs).gated.unwrap()
}

fn all_fills(v: &PaperVenue) -> Vec<fortuna_venues::Fill> {
    futures::executor::block_on(v.fills_since(Cursor::start()))
        .unwrap()
        .fills
}

/// The FIRST real recorded trade -> (yes_price_cents, whole_qty). Price is the
/// venue's `yes_price_dollars` string ("0.0300" -> 3c); quantity is `count_fp`
/// floored to whole contracts. REAL recorded values, never synthesized.
fn first_recorded_trade() -> (i64, i64) {
    let raw = std::fs::read_to_string(FIXTURE).expect("fixture readable");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("fixture parses");
    let t = &v["trades"][0];
    let px_dollars: f64 = t["yes_price_dollars"]
        .as_str()
        .expect("yes_price_dollars present")
        .parse()
        .expect("yes_price_dollars is numeric");
    let cents = (px_dollars * 100.0).round() as i64;
    let qty_fp: f64 = t["count_fp"]
        .as_str()
        .expect("count_fp present")
        .parse()
        .expect("count_fp is numeric");
    (cents, qty_fp.floor() as i64)
}

#[test]
fn recorded_real_print_fills_through_but_never_at_touch() {
    let (px, qty) = first_recorded_trade();
    // Pin the fixture's real shape (a 3c print, a usable whole quantity).
    assert_eq!(px, 3, "the recorded JDAY market traded at 3c");
    assert!(
        qty >= 2,
        "a usable real quantity for a haircut fill (got {qty})"
    );

    // THROUGH: a resting buy ABOVE the real print -> the market traded through us.
    let v = paper(50);
    futures::executor::block_on(v.place(gated_buy(1, px + 1, 1000))).unwrap();
    v.apply_public_trade(&mkt(), Cents::new(px), qty).unwrap();
    let fills = all_fills(&v);
    assert_eq!(
        fills.len(),
        1,
        "the REAL through-print fills our resting buy"
    );
    assert_eq!(
        fills[0].price,
        Cents::new(px + 1),
        "maker fills at OUR limit"
    );
    assert_eq!(
        fills[0].qty.raw(),
        qty * 50 / 100,
        "50% haircut of the real {qty}-lot print"
    );
    assert!(fills[0].is_maker);

    // TOUCH (the doctrine gate, against a REAL print): a resting buy EXACTLY AT the
    // real print price is a touch, never a fill -- the same spec-11 inflation guard
    // as paper.rs's `touch_prints_never_fill_resting_orders`.
    let v = paper(50);
    futures::executor::block_on(v.place(gated_buy(2, px, 1000))).unwrap();
    v.apply_public_trade(&mkt(), Cents::new(px), qty).unwrap();
    assert!(
        all_fills(&v).is_empty(),
        "TOUCH-FILL INFLATION: a real print AT our limit must never fill"
    );

    // AWAY: a resting buy BELOW the real print -> the market never reached us.
    let v = paper(50);
    futures::executor::block_on(v.place(gated_buy(3, px - 1, 1000))).unwrap();
    v.apply_public_trade(&mkt(), Cents::new(px), qty).unwrap();
    assert!(
        all_fills(&v).is_empty(),
        "a real print ABOVE our buy limit never fills"
    );
}
