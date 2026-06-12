//! T5.B3 tests: perp gate extensions (spec 5.15 "New gates", written from
//! spec text BEFORE implementation).
//!
//! Contract under test:
//! - Perp orders ride the SAME GatePipeline instance: same halt flags, same
//!   I3 rate buckets, same audit record stream, fail-closed everywhere; they
//!   seal into `GatedPerpOrder` (same private-constructor discipline as
//!   `GatedOrder` — a Cents-typed order cannot carry a PerpPrice by 5.15's
//!   type-separation rule).
//! - Worst-case exposure for any perp order is the LIQUIDATION loss, never
//!   the premium analogy: the margin ACCOUNT is the exposure unit; the
//!   maintenance-margin approximation comes from the configured risk curve
//!   (recorded venue leverage_estimates) with a safety multiplier, and any
//!   order whose worst case the curve cannot bound is REFUSED.
//! - New checks: margin headroom, liquidation-distance floor, per-asset
//!   leverage caps, per-venue/per-asset notional caps, funding-drag-in-edge.
//! - Fee trap (spec 5.15): edge floors evaluate at ASSUMED post-promo fees;
//!   config refuses assumed_fee_bps = 0.
//! - Valid reduce-only (exposure-reducing) orders pass the risk gates and
//!   edge floor with an exposure-reducing note (rejecting a de-risking close
//!   would force staying at higher risk); they still face halts, sanity,
//!   rate limits, idempotency, and netting.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpPosition, PerpPrice};
use fortuna_gates::perp::{
    GatedPerpOrder, PerpCandidateOrder, PerpGateInputs, PerpRestingOrderView, PERP_ALL,
};
use fortuna_gates::{GateCheck, GateConfig, GatePipeline, Verdict};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T08:00:00.000Z").unwrap()
}

fn intent(n: u64) -> IntentId {
    let mut g = IdGen::new(n);
    IntentId::new(g.next(t0()).unwrap())
}

/// Baseline permissive config: every check has room around the baseline
/// candidate (buy 100 @ $6.2600, fair $6.3500, equity $10,000).
fn perp_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 100000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 100000000
        per_event_exposure_cents = 100000000
        require_event_mapping = false

        [per_strategy.perp_s]
        max_exposure_cents = 100000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 10

        [rate.kinetics]
        burst = 100
        sustained_per_min = 600
        market_burst = 100
        market_sustained_per_min = 600

        [perp.venues.kinetics]
        max_total_notional_cents = 10000000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_bps = 1000
        assumed_fee_bps = 12
        funding_drag_bps_per_window = 4
        min_liquidation_distance_bps = 1000
        mm_safety_multiplier_pct = 130

        [perp.assets.KXBTCPERP]
        max_leverage_x10 = 50
        max_notional_cents = 5000000
        mm_curve = [[1000000, 500], [5000000, 800]]
        "#,
    )
    .unwrap()
}

/// Same config with one perp knob overridden (TOML re-parse keeps the test
/// surface declarative).
fn config_with(replace: &str, with: &str) -> GateConfig {
    let base = r#"
        [global]
        max_total_exposure_cents = 100000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 100000000
        per_event_exposure_cents = 100000000
        require_event_mapping = false

        [per_strategy.perp_s]
        max_exposure_cents = 100000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 10

        [rate.kinetics]
        burst = 100
        sustained_per_min = 600
        market_burst = 100
        market_sustained_per_min = 600

        [perp.venues.kinetics]
        max_total_notional_cents = 10000000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_bps = 1000
        assumed_fee_bps = 12
        funding_drag_bps_per_window = 4
        min_liquidation_distance_bps = 1000
        mm_safety_multiplier_pct = 130

        [perp.assets.KXBTCPERP]
        max_leverage_x10 = 50
        max_notional_cents = 5000000
        mm_curve = [[1000000, 500], [5000000, 800]]
        "#;
    toml::from_str(&base.replace(replace, with)).unwrap()
}

fn candidate(n: u64) -> PerpCandidateOrder {
    PerpCandidateOrder {
        intent_id: intent(n),
        strategy: StrategyId::new("perp_s").unwrap(),
        venue: VenueId::new("kinetics").unwrap(),
        market: MarketId::new("KXBTCPERP").unwrap(),
        action: Action::Buy,
        reduce_only: false,
        limit_price: PerpPrice::new(62600),
        qty: Contracts::new(100),
        fair_value: PerpPrice::new(63500),
        holding_windows: 1,
        client_order_id: ClientOrderId::new(format!("perp-{n}")).unwrap(),
    }
}

fn account(equity_cents: i64) -> MarginAccountView {
    // Balance-only view: no positions marked, no pending funding.
    MarginAccountView::compute(Cents::new(equity_cents), &[], Cents::ZERO).unwrap()
}

struct Ctx {
    account: MarginAccountView,
    position: Option<PerpPosition>,
    resting: Vec<PerpRestingOrderView>,
    recent: BTreeSet<String>,
    venue_open_notional: Cents,
    mark: PerpPrice,
}

impl Ctx {
    fn new(equity_cents: i64) -> Ctx {
        Ctx {
            account: account(equity_cents),
            position: None,
            resting: Vec::new(),
            recent: BTreeSet::new(),
            venue_open_notional: Cents::ZERO,
            mark: PerpPrice::new(62600),
        }
    }

    fn inputs(&self) -> PerpGateInputs<'_> {
        PerpGateInputs {
            now: t0(),
            account: &self.account,
            position: self.position.as_ref(),
            conservative_mark: self.mark,
            venue_open_notional_cents: self.venue_open_notional,
            own_resting: &self.resting,
            recent_client_order_ids: &self.recent,
        }
    }
}

fn short_pos(qty: i64) -> PerpPosition {
    PerpPosition {
        market: MarketId::new("KXBTCPERP").unwrap(),
        qty: Contracts::new(-qty),
        avg_entry: PerpPrice::new(62600),
    }
}

fn reject_check(p: &mut GatePipeline, c: &PerpCandidateOrder, ctx: &Ctx) -> Option<GateCheck> {
    match p.evaluate_perp(c, &ctx.inputs()).gated {
        Ok(_) => None,
        Err(r) => Some(r.check),
    }
}

// ---- pipeline shape ----

#[test]
fn full_pass_emits_perp_all_trail_and_seals() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000); // $10,000 equity
    let c = candidate(1);
    let out = p.evaluate_perp(&c, &ctx.inputs());
    let gated: GatedPerpOrder = out.gated.expect("baseline candidate must pass");
    assert_eq!(out.records.len(), PERP_ALL.len());
    for (record, expected) in out.records.iter().zip(PERP_ALL) {
        assert_eq!(record.check, expected);
        assert!(matches!(record.verdict, Verdict::Pass));
        assert_eq!(record.intent_id, c.intent_id);
    }
    // The sealed order carries the candidate's terms.
    assert_eq!(gated.limit_price(), c.limit_price);
    assert_eq!(gated.qty(), c.qty);
    assert_eq!(gated.market(), &c.market);
    assert_eq!(gated.venue(), &c.venue);
    assert_eq!(gated.action(), c.action);
    assert!(!gated.reduce_only());
    assert_eq!(gated.client_order_id(), &c.client_order_id);
}

#[test]
fn perp_all_has_eleven_checks_with_spec_numbering() {
    assert_eq!(PERP_ALL.len(), 11);
    assert_eq!(PERP_ALL[0], GateCheck::Halts);
    // The four 5.15 additions carry 1-based pipeline positions 11-14.
    assert_eq!(GateCheck::MarginHeadroom.index(), 11);
    assert_eq!(GateCheck::LiquidationDistance.index(), 12);
    assert_eq!(GateCheck::LeverageCap.index(), 13);
    assert_eq!(GateCheck::PerpNotionalCap.index(), 14);
    // The event-contract pipeline is untouched.
    assert_eq!(GateCheck::ALL.len(), 10);
}

#[test]
fn rejection_trail_ends_with_failing_check() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(4_094); // just below margin-headroom requirement
    let out = p.evaluate_perp(&candidate(1), &ctx.inputs());
    let rejection = out.gated.expect_err("must reject");
    let last = out.records.last().unwrap();
    assert_eq!(last.check, rejection.check);
    assert!(matches!(last.verdict, Verdict::Reject));
    // Every record before the failure is a pass.
    for r in &out.records[..out.records.len() - 1] {
        assert!(matches!(r.verdict, Verdict::Pass));
    }
}

// ---- check 1: halts (shared flags) ----

#[test]
fn venue_halt_blocks_perp_orders() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    p.set_halt(
        fortuna_gates::HaltScope::Venue("kinetics".into()),
        "drawdown",
    );
    let ctx = Ctx::new(1_000_000);
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::Halts)
    );
}

// ---- check 11: margin headroom ----

#[test]
fn margin_headroom_rejects_when_required_exceeds_available() {
    // Baseline order: post-notional 62,600c, mm 500 bps -> 3,130c, x130%
    // -> 4,069c required; funding drag 26c. Equity 4,094c -> available
    // 4,068c < 4,069c: reject. Equity 4,095c -> available 4,069c: pass
    // (headroom check specifically; later checks may still reject).
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(4_094);
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::MarginHeadroom)
    );

    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(4_095);
    let rejected_at = reject_check(&mut p, &candidate(2), &ctx);
    assert_ne!(rejected_at, Some(GateCheck::MarginHeadroom));
}

#[test]
fn margin_headroom_refuses_notional_beyond_risk_curve() {
    // qty 80,000 -> post-notional 50,080,000c, beyond the last curve tier
    // (5,000,000c): the approximation cannot bound the worst case -> REFUSE,
    // regardless of equity.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000_000);
    let mut c = candidate(1);
    c.qty = Contracts::new(80_000);
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

/// Variant config for the curve-tier boundary test: distance floor
/// disabled, leverage cap 20x, strategy per-order notional widened — so
/// margin headroom is the deciding check around the tier edge.
fn tier_boundary_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 100000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 49
        max_cross_cents = 98
        per_market_exposure_cents = 100000000
        per_event_exposure_cents = 100000000
        require_event_mapping = false

        [per_strategy.perp_s]
        max_exposure_cents = 100000000
        max_order_notional_cents = 2000000
        min_net_edge_bps = 10

        [rate.kinetics]
        burst = 100
        sustained_per_min = 600
        market_burst = 100
        market_sustained_per_min = 600

        [perp.venues.kinetics]
        max_total_notional_cents = 10000000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_bps = 1000
        assumed_fee_bps = 12
        funding_drag_bps_per_window = 4
        min_liquidation_distance_bps = 0
        mm_safety_multiplier_pct = 130

        [perp.assets.KXBTCPERP]
        max_leverage_x10 = 200
        max_notional_cents = 5000000
        mm_curve = [[1000000, 500], [5000000, 800]]
        "#,
    )
    .unwrap()
}

#[test]
fn margin_headroom_uses_higher_tier_above_threshold() {
    // mark $5.0000: qty 2,000 -> exactly 1,000,000c notional (tier 1,
    // 500 bps -> required ceil(50,000 x 1.3) = 65,000c; drag 400c;
    // available 69,600c: pass). qty 2,001 -> 1,000,500c (tier 2, 800 bps
    // -> required 104,052c: reject). Equity 70,000c either way.
    let ctx = Ctx {
        mark: PerpPrice::new(50_000),
        ..Ctx::new(70_000)
    };

    let mut p = GatePipeline::new(tier_boundary_config()).unwrap();
    let mut pass = candidate(1);
    pass.limit_price = PerpPrice::new(50_000);
    pass.fair_value = PerpPrice::new(51_000);
    pass.qty = Contracts::new(2_000);
    assert_ne!(
        reject_check(&mut p, &pass, &ctx),
        Some(GateCheck::MarginHeadroom)
    );

    let mut p = GatePipeline::new(tier_boundary_config()).unwrap();
    let mut reject = candidate(2);
    reject.limit_price = PerpPrice::new(50_000);
    reject.fair_value = PerpPrice::new(51_000);
    reject.qty = Contracts::new(2_001);
    assert_eq!(
        reject_check(&mut p, &reject, &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

#[test]
fn margin_headroom_fail_closed_on_missing_perp_config() {
    // Venue or asset absent from [perp.*]: reject at the first perp check.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.market = MarketId::new("KXETHPERP").unwrap(); // no [perp.assets] entry
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::MarginHeadroom)
    );

    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut c = candidate(2);
    c.venue = VenueId::new("unknown_venue").unwrap();
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

#[test]
fn margin_headroom_rejects_position_for_wrong_market() {
    // Fail-closed: certifying margin against the wrong market's position
    // would be worse than no position view at all.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(1_000_000);
    ctx.position = Some(PerpPosition {
        market: MarketId::new("KXETHPERP").unwrap(),
        qty: Contracts::new(-10),
        avg_entry: PerpPrice::new(62600),
    });
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

// ---- check 12: liquidation-distance floor ----

#[test]
fn liquidation_distance_floor_rejects_thin_buffer() {
    // Equity 4,200c: headroom passes (available 4,174 >= 4,069) but the
    // remaining buffer (105c over 100 contracts at mark $6.2600) is ~16 bps
    // of mark — far below the 1,000 bps floor.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(4_200);
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::LiquidationDistance)
    );
}

#[test]
fn liquidation_distance_floor_is_monotone_in_config() {
    // The baseline (equity $10,000) passes at floor 1,000 bps; the same
    // order rejects when the operator demands a wider floor than the
    // buffer provides.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    assert_eq!(reject_check(&mut p, &candidate(1), &ctx), None);

    let strict = config_with(
        "min_liquidation_distance_bps = 1000",
        "min_liquidation_distance_bps = 10000000",
    );
    let mut p = GatePipeline::new(strict).unwrap();
    assert_eq!(
        reject_check(&mut p, &candidate(2), &ctx),
        Some(GateCheck::LiquidationDistance)
    );
}

// ---- check 13: per-asset leverage cap ----

#[test]
fn leverage_cap_rejects_when_notional_exceeds_equity_times_cap() {
    // Cap 0.5x (max_leverage_x10 = 5): equity 1,000,000c allows 500,000c.
    // qty 800 -> 500,800c > 500,000c: reject. qty 798 -> 499,548c: passes
    // this check.
    let cfg = config_with("max_leverage_x10 = 50", "max_leverage_x10 = 5");
    let mut p = GatePipeline::new(cfg).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.qty = Contracts::new(800);
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::LeverageCap));

    let cfg = config_with("max_leverage_x10 = 50", "max_leverage_x10 = 5");
    let mut p = GatePipeline::new(cfg).unwrap();
    let mut c = candidate(2);
    c.qty = Contracts::new(798);
    assert_ne!(reject_check(&mut p, &c, &ctx), Some(GateCheck::LeverageCap));
}

// ---- check 14: notional caps ----

#[test]
fn venue_notional_cap_counts_existing_open_notional() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(1_000_000);
    ctx.venue_open_notional = Cents::new(9_990_000);
    // 9,990,000 + 62,600 > 10,000,000: reject.
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::PerpNotionalCap)
    );
    ctx.venue_open_notional = Cents::new(9_900_000);
    let mut p = GatePipeline::new(perp_config()).unwrap();
    // 9,900,000 + 62,600 <= 10,000,000: passes this check.
    assert_ne!(
        reject_check(&mut p, &candidate(2), &ctx),
        Some(GateCheck::PerpNotionalCap)
    );
}

#[test]
fn asset_notional_cap_uses_worst_case_post_position() {
    // Short 10, non-reduce-only BUY 12: naive post-fill |pos| would be 2
    // (1,252c), but worst case while the order rests is max(|10|, |2|) = 10
    // plus the order's own adding side -> the gate must price the LARGER
    // exposure. With asset cap 5,000c, the worst-case 6,260c rejects; the
    // naive 1,252c would have passed — this pins the max() conservatism.
    let cfg = config_with("max_notional_cents = 5000000", "max_notional_cents = 5000");
    let mut p = GatePipeline::new(cfg).unwrap();
    let mut ctx = Ctx::new(1_000_000);
    ctx.position = Some(short_pos(10));
    let mut c = candidate(1);
    c.qty = Contracts::new(12);
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::PerpNotionalCap)
    );
}

// ---- check 4 (perp arm): price sanity vs conservative mark ----

#[test]
fn price_sanity_rejects_limit_outside_band_of_mark() {
    // Band 1,000 bps of mark 62,600 -> max distance 6,260 tt.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.limit_price = PerpPrice::new(70_000); // 7,400 tt away
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::PriceSanity));

    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut c = candidate(2);
    c.limit_price = PerpPrice::new(68_000); // 5,400 tt away: inside band
    assert_ne!(reject_check(&mut p, &c, &ctx), Some(GateCheck::PriceSanity));
}

#[test]
fn price_sanity_rejects_non_positive_limit() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.limit_price = PerpPrice::ZERO;
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::PriceSanity));
}

// ---- check 5 (perp arm): size sanity ----

#[test]
fn size_sanity_rejects_quantity_outside_venue_bounds() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.qty = Contracts::new(0);
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::SizeSanity));

    // Variant with max_order_contracts = 150: qty 200 rejects on size.
    let cfg = config_with("max_order_contracts = 10000", "max_order_contracts = 150");
    let mut p = GatePipeline::new(cfg).unwrap();
    let mut c = candidate(2);
    c.qty = Contracts::new(200);
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::SizeSanity));
}

#[test]
fn size_sanity_enforces_strategy_per_order_notional_cap() {
    // Strategy cap 1,000,000c; qty 9,000 -> 5,634,000c... but that exceeds
    // the risk curve too. Use a variant strategy cap below the baseline
    // order notional (62,600c) instead.
    let cfg = config_with(
        "max_order_notional_cents = 1000000",
        "max_order_notional_cents = 50000",
    );
    let mut p = GatePipeline::new(cfg).unwrap();
    let ctx = Ctx::new(1_000_000);
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::SizeSanity)
    );
}

#[test]
fn unconfigured_strategy_fails_closed() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.strategy = StrategyId::new("nobody").unwrap();
    let out = p.evaluate_perp(&c, &ctx.inputs());
    assert!(out.gated.is_err());
}

// ---- check 6 (perp arm): edge floor with funding drag + assumed fees ----

#[test]
fn edge_floor_subtracts_assumed_fee_and_funding_drag() {
    // Baseline: gross (63500-62600)x100 = 900c; assumed fee
    // ceil(62,600 x 12bps) = 76c; drag 1 window ceil(62,600 x 4bps) = 26c;
    // net 798c = 127 bps >= 10 bps floor: passes.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    assert_eq!(reject_check(&mut p, &candidate(1), &ctx), None);

    // Zero gross edge: fee + drag make net negative -> reject.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut c = candidate(2);
    c.fair_value = c.limit_price;
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::EdgeFloor));
}

#[test]
fn funding_drag_scales_with_holding_windows() {
    // 200 windows: drag = ceil(62,600 x 4bps x 200) = 5,008c > gross 900c
    // -> reject at the edge floor. The same order with 1 window passes.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.holding_windows = 200;
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::EdgeFloor));
}

#[test]
fn zero_holding_windows_rejects() {
    // Every position can cross a funding tick; an intended holding of zero
    // windows is not a coherent declaration.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.holding_windows = 0;
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::EdgeFloor));
}

#[test]
fn sell_side_edge_mirrors() {
    // Sell above fair: gross = (limit - fair) x qty.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.action = Action::Sell;
    c.limit_price = PerpPrice::new(63_500);
    c.fair_value = PerpPrice::new(62_600);
    assert_eq!(reject_check(&mut p, &c, &ctx), None);

    // Sell below fair: negative gross -> reject.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut c = candidate(2);
    c.action = Action::Sell;
    c.limit_price = PerpPrice::new(62_600);
    c.fair_value = PerpPrice::new(63_500);
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::EdgeFloor));
}

// ---- fee trap (spec 5.15) ----

#[test]
fn config_rejects_zero_assumed_fee() {
    // Promo-$0 fees must never reach edge math: assumed_fee_bps = 0 is an
    // invalid config, not a permissive one.
    let cfg = config_with("assumed_fee_bps = 12", "assumed_fee_bps = 0");
    assert!(GatePipeline::new(cfg).is_err());
}

// ---- checks 7/8 (shared): rate limits + idempotency ----

#[test]
fn perp_rate_breach_halts_venue_for_event_orders_too() {
    // I3 on the perp path: breach is a HALT, not a throttle — and the halt
    // is SHARED: a subsequent perp order on the same venue rejects at
    // check 1.
    // All orders land at the same instant, so no sustained refill occurs:
    // a venue burst of 2 trips on the third order.
    let cfg = config_with(
        "burst = 100\n        sustained_per_min = 600",
        "burst = 2\n        sustained_per_min = 1",
    );
    let mut p = GatePipeline::new(cfg).unwrap();
    let ctx = Ctx::new(1_000_000);
    assert_eq!(reject_check(&mut p, &candidate(1), &ctx), None);
    assert_eq!(reject_check(&mut p, &candidate(2), &ctx), None);
    let third = p.evaluate_perp(&candidate(3), &ctx.inputs());
    let rejection = third.gated.unwrap_err();
    assert_eq!(rejection.check, GateCheck::RateLimits);
    // The venue is now halted: the NEXT order dies at check 1.
    assert_eq!(
        reject_check(&mut p, &candidate(4), &ctx),
        Some(GateCheck::Halts)
    );
}

#[test]
fn missing_rate_config_fails_closed() {
    let cfg = config_with("[rate.kinetics]", "[rate.other_venue]");
    let mut p = GatePipeline::new(cfg).unwrap();
    let ctx = Ctx::new(1_000_000);
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::RateLimits)
    );
}

#[test]
fn duplicate_client_order_id_rejects() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(1_000_000);
    ctx.recent.insert("perp-1".into());
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::Idempotency)
    );
}

// ---- check 10 (perp arm): internal netting ----

#[test]
fn crossing_own_resting_perp_order_rejects() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(1_000_000);
    ctx.resting.push(PerpRestingOrderView {
        market: MarketId::new("KXBTCPERP").unwrap(),
        action: Action::Sell,
        price: PerpPrice::new(62_500),
    });
    // New buy at 62,600 >= our own resting sell at 62,500: self-cross.
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::InternalNetting)
    );

    // New buy strictly below our resting sell: no cross.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut c = candidate(2);
    c.limit_price = PerpPrice::new(62_400);
    c.fair_value = PerpPrice::new(63_300);
    assert_eq!(reject_check(&mut p, &c, &ctx), None);
}

// ---- reduce-only semantics ----

#[test]
fn valid_reduce_only_passes_where_risk_adding_fails() {
    // Short 200 @ entry 62,600, equity 6,000c. Risk-adding buy 100 prices
    // worst case max(200, 100) = 200 contracts -> required 8,138c > 5,974c
    // available: reject. The same order reduce_only prices the close
    // (exposure-reducing) and passes the whole pipeline.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(6_000);
    ctx.position = Some(short_pos(200));
    let c = candidate(1);
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::MarginHeadroom)
    );

    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut c = candidate(2);
    c.reduce_only = true;
    let out = p.evaluate_perp(&c, &ctx.inputs());
    let gated = out.gated.expect("reduce-only close must pass");
    assert!(gated.reduce_only());
}

#[test]
fn reduce_only_without_position_rejects() {
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let ctx = Ctx::new(1_000_000);
    let mut c = candidate(1);
    c.reduce_only = true;
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

#[test]
fn reduce_only_same_direction_rejects() {
    // Long position + reduce-only BUY is incoherent.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(1_000_000);
    ctx.position = Some(PerpPosition {
        market: MarketId::new("KXBTCPERP").unwrap(),
        qty: Contracts::new(50),
        avg_entry: PerpPrice::new(62_600),
    });
    let mut c = candidate(1);
    c.reduce_only = true;
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

#[test]
fn reduce_only_oversized_rejects() {
    // Closing 300 against a 200-contract position would flip, not reduce.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(1_000_000);
    ctx.position = Some(short_pos(200));
    let mut c = candidate(1);
    c.reduce_only = true;
    c.qty = Contracts::new(300);
    assert_eq!(
        reject_check(&mut p, &c, &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

#[test]
fn reduce_only_still_faces_sanity_and_idempotency() {
    // De-risking skips the risk gates, not the fat-finger gates.
    let mut p = GatePipeline::new(perp_config()).unwrap();
    let mut ctx = Ctx::new(6_000);
    ctx.position = Some(short_pos(200));
    let mut c = candidate(1);
    c.reduce_only = true;
    c.limit_price = PerpPrice::new(70_000); // outside the price band
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::PriceSanity));

    let mut p = GatePipeline::new(perp_config()).unwrap();
    ctx.recent.insert("perp-2".into());
    let mut c = candidate(2);
    c.reduce_only = true;
    assert_eq!(reject_check(&mut p, &c, &ctx), Some(GateCheck::Idempotency));
}

// ---- config validation ----

#[test]
fn perp_config_validation_rejects_bad_shapes() {
    // Descending risk-curve thresholds.
    let cfg = config_with(
        "mm_curve = [[1000000, 500], [5000000, 800]]",
        "mm_curve = [[5000000, 500], [1000000, 800]]",
    );
    assert!(GatePipeline::new(cfg).is_err());

    // Zero leverage cap.
    let cfg = config_with("max_leverage_x10 = 50", "max_leverage_x10 = 0");
    assert!(GatePipeline::new(cfg).is_err());

    // Safety multiplier below 100% would WEAKEN the venue's own margin.
    let cfg = config_with(
        "mm_safety_multiplier_pct = 130",
        "mm_safety_multiplier_pct = 99",
    );
    assert!(GatePipeline::new(cfg).is_err());

    // Empty curve.
    let cfg = config_with(
        "mm_curve = [[1000000, 500], [5000000, 800]]",
        "mm_curve = []",
    );
    assert!(GatePipeline::new(cfg).is_err());
}

#[test]
fn config_without_perp_section_still_validates_for_event_trading() {
    // The perp section is additive: existing event-contract configs parse
    // and validate unchanged. Perp orders then fail closed.
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

        [per_strategy.perp_s]
        max_exposure_cents = 1000000
        max_order_notional_cents = 100000
        min_net_edge_bps = 10

        [rate.kinetics]
        burst = 100
        sustained_per_min = 600
        market_burst = 100
        market_sustained_per_min = 600
        "#,
    )
    .unwrap();
    let mut p = GatePipeline::new(cfg).unwrap();
    let ctx = Ctx::new(1_000_000);
    assert_eq!(
        reject_check(&mut p, &candidate(1), &ctx),
        Some(GateCheck::MarginHeadroom)
    );
}

// ---- properties ----

proptest! {
    #[test]
    fn prop_evaluate_perp_never_panics_and_trail_is_coherent(
        equity in -1_000_000i64..100_000_000,
        qty in 0i64..20_000,
        limit in 0i64..200_000,
        fair in 0i64..200_000,
        mark in 1i64..200_000,
        windows in 0u32..50,
        reduce in any::<bool>(),
        pos_qty in -500i64..500,
    ) {
        let mut p = GatePipeline::new(perp_config()).unwrap();
        let mut ctx = Ctx::new(equity);
        ctx.mark = PerpPrice::new(mark);
        if pos_qty != 0 {
            ctx.position = Some(PerpPosition {
                market: MarketId::new("KXBTCPERP").unwrap(),
                qty: Contracts::new(pos_qty),
                avg_entry: PerpPrice::new(62_600),
            });
        }
        let mut c = candidate(99);
        c.qty = Contracts::new(qty);
        c.limit_price = PerpPrice::new(limit);
        c.fair_value = PerpPrice::new(fair);
        c.holding_windows = windows;
        c.reduce_only = reduce;
        let out = p.evaluate_perp(&c, &ctx.inputs());
        match out.gated {
            Ok(_) => {
                // A full pass emits exactly the PERP_ALL trail, all Pass.
                prop_assert_eq!(out.records.len(), PERP_ALL.len());
                for (r, expected) in out.records.iter().zip(PERP_ALL) {
                    prop_assert_eq!(r.check, expected);
                    prop_assert!(matches!(r.verdict, Verdict::Pass));
                }
            }
            Err(rej) => {
                // The trail ends at the failing check; everything before
                // passed; the trail is a prefix of PERP_ALL.
                let last = out.records.last().unwrap();
                prop_assert_eq!(last.check, rej.check);
                prop_assert!(matches!(last.verdict, Verdict::Reject));
                prop_assert!(out.records.len() <= PERP_ALL.len());
                for (r, expected) in out.records.iter().zip(PERP_ALL) {
                    prop_assert_eq!(r.check, expected);
                }
            }
        }
    }
}
