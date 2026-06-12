//! I1 extension (spec 5.15; T5.B3 ADDITION — this file only adds pins,
//! it weakens nothing): perp orders ride the same universal gate.
//!
//! Property to encode: every sealed perp order carries a complete
//! PERP_ALL pass trail; every rejection carries a trail ending in the
//! failing check; `GatedPerpOrder` is unconstructible outside
//! fortuna-gates (the compile-fail half lives as doc-tests on
//! fortuna-invariants/src/lib.rs, mirroring `GatedOrder`'s).
//!
//! The venue-side binding (a kinetics `place` accepting only
//! `GatedPerpOrder`) is pinned when the adapter lands (T5.B4); the seal
//! and trail properties are pinned here, now.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpPrice};
use fortuna_gates::perp::{PerpCandidateOrder, PerpGateInputs, PERP_ALL};
use fortuna_gates::{GateConfig, GatePipeline, Verdict};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T08:00:00.000Z").unwrap()
}

fn pipeline() -> GatePipeline {
    let cfg: GateConfig = toml::from_str(
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

        [per_strategy.perp_i1]
        max_exposure_cents = 100000000
        max_order_notional_cents = 1000000
        min_net_edge_bps = 0

        [rate.kinetics]
        burst = 100000
        sustained_per_min = 100000
        market_burst = 100000
        market_sustained_per_min = 100000

        [perp.venues.kinetics]
        max_total_notional_cents = 10000000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_bps = 2000
        assumed_fee_bps = 12
        funding_drag_bps_per_window = 4
        min_liquidation_distance_bps = 100
        mm_safety_multiplier_pct = 130

        [perp.assets.KXBTCPERP]
        max_leverage_x10 = 50
        max_notional_cents = 5000000
        mm_curve = [[1000000, 500], [5000000, 800]]
        "#,
    )
    .unwrap();
    GatePipeline::new(cfg).unwrap()
}

fn candidate(n: u64, qty: i64, limit: i64, fair: i64) -> PerpCandidateOrder {
    let mut g = IdGen::new(n + 1);
    PerpCandidateOrder {
        intent_id: IntentId::new(g.next(t0()).unwrap()),
        strategy: StrategyId::new("perp_i1").unwrap(),
        venue: VenueId::new("kinetics").unwrap(),
        market: MarketId::new("KXBTCPERP").unwrap(),
        action: Action::Buy,
        reduce_only: false,
        limit_price: PerpPrice::new(limit),
        qty: Contracts::new(qty),
        fair_value: PerpPrice::new(fair),
        holding_windows: 1,
        client_order_id: ClientOrderId::new(format!("perp-i1-{n}")).unwrap(),
    }
}

proptest! {
    /// For arbitrary candidates: a sealed order exists ONLY behind a
    /// complete PERP_ALL pass trail; a rejection's trail is a PERP_ALL
    /// prefix ending at the failing check. No third outcome exists.
    #[test]
    fn perp_i1_every_outcome_carries_a_coherent_trail(
        n in 0u64..10_000,
        qty in 0i64..5_000,
        limit in 1i64..150_000,
        fair in 1i64..150_000,
        equity in 0i64..10_000_000,
    ) {
        let mut gates = pipeline();
        let account =
            MarginAccountView::compute(Cents::new(equity), &[], Cents::ZERO).unwrap();
        let recent = BTreeSet::new();
        let inputs = PerpGateInputs {
            now: t0(),
            account: &account,
            position: None,
            conservative_mark: PerpPrice::new(62_600),
            venue_open_notional_cents: Cents::ZERO,
            own_resting: &[],
            recent_client_order_ids: &recent,
        };
        let out = gates.evaluate_perp(&candidate(n, qty, limit, fair), &inputs);
        match out.gated {
            Ok(sealed) => {
                prop_assert_eq!(out.records.len(), PERP_ALL.len());
                for (record, expected) in out.records.iter().zip(PERP_ALL) {
                    prop_assert_eq!(record.check, expected);
                    prop_assert!(matches!(record.verdict, Verdict::Pass));
                }
                // The sealed order's terms are the candidate's, unaltered.
                prop_assert_eq!(sealed.qty(), Contracts::new(qty));
                prop_assert_eq!(sealed.limit_price(), PerpPrice::new(limit));
            }
            Err(rejection) => {
                prop_assert!(!out.records.is_empty());
                prop_assert!(out.records.len() <= PERP_ALL.len());
                let last = out.records.last().unwrap();
                prop_assert_eq!(last.check, rejection.check);
                prop_assert!(matches!(last.verdict, Verdict::Reject));
                for (record, expected) in out.records.iter().zip(PERP_ALL) {
                    prop_assert_eq!(record.check, expected);
                }
                for record in &out.records[..out.records.len() - 1] {
                    prop_assert!(matches!(record.verdict, Verdict::Pass));
                }
            }
        }
    }
}
