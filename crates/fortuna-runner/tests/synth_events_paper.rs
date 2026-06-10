//! T3.5: the `synth_events` strategy at the PAPER parity boundary
//! (spec Section 6 item 4: "paper-only initially").
//!
//! Doctrine under test:
//! - synth_events is a named synthesis strategy (pure information-
//!   synthesis beliefs, low-attention markets). It DECLARES Paper as
//!   its stage cap; its EFFECTIVE stage starts at Sim and rises only
//!   through operator promotion records (I7 rails).
//! - At the paper boundary its UNSIZED proposal flows through the SAME
//!   gate pipeline into the PaperVenue. The harness owns sizing, timing
//!   and price-within-limit (I6): a Passive proposal with max price 62
//!   may be quoted maker at 59. The honest fill rules then hold: a
//!   public print AT the quote is a touch and must NOT fill; only a
//!   trade THROUGH it fills, haircut applied.
//! - Replaying recorded venue streams stays operator-blocked (GAPS);
//!   this test pushes synthetic books/prints through the same engine.

use fortuna_cognition::cycle::{EdgeView, TriageDecision};
use fortuna_cognition::events::{EdgeTier, MappingType};
use fortuna_cognition::mind::{Mind, MindOutput, StubMind};
use fortuna_core::book::OrderBook;
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_paper::{PaperConfig, PaperVenue};
use fortuna_runner::promotion::{effective_stage, PromotionRecord};
use fortuna_runner::synth_events::synth_events_config;
use fortuna_runner::synthesis::SynthesisStrategy;
use fortuna_runner::{CoreHandle, ProposedLeg, Stage, Strategy};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::{Cursor, Market, MarketStatus, PriceLevel, SettlementMeta, Venue};
use std::collections::BTreeMap;
use std::sync::Arc;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
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

fn market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("paper").unwrap(),
        title: format!("low-attention market {id}"),
        category: "weather".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "nws".into(),
            resolution_source: "nws".into(),
            expected_lag_hours: 2,
        },
        volume_contracts: Some(900),
        payout_per_contract: Cents::new(100),
    }
}

fn lvl(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: Contracts::new(qty),
    }
}

fn belief_output(p: f64) -> MindOutput {
    serde_json::from_value(serde_json::json!({
        "beliefs": [{
            "event_id": "evt-1",
            "p": p,
            "p_raw": p,
            "horizon": "2026-06-20T18:00:00.000Z",
            "evidence": [{"source": "synth", "ref": "sig-1"}]
        }],
        "proposals": [],
        "journal": null
    }))
    .unwrap()
}

fn near_identity_calibration() -> fortuna_cognition::cycle::CalibrationContext {
    use fortuna_cognition::calibration::{fit_platt, CalibrationMethod, CalibrationParams};
    let mut samples = Vec::new();
    for i in 0..100 {
        samples.push((0.7, i % 10 < 7));
        samples.push((0.3, i % 10 < 3));
        samples.push((0.5, i % 2 == 0));
    }
    fortuna_cognition::cycle::CalibrationContext {
        params: CalibrationParams {
            version: 1,
            method: CalibrationMethod::Platt(fit_platt(&samples).unwrap()),
            extremization_k: 1.0,
            fitted_on_n: 300,
        },
        resolved_n: 300,
    }
}

fn edge() -> EdgeView {
    EdgeView {
        market: "KX-A".to_string(),
        event_id: "evt-1".to_string(),
        mapping: MappingType::Direct,
        tier: EdgeTier::Confirmed,
    }
}

// --------------------------------------------------------- stage discipline

#[test]
fn synth_events_declares_paper_but_starts_at_sim() {
    let cfg = synth_events_config(
        vec![edge()],
        TriageDecision::AlwaysAccept,
        Some(near_identity_calibration()),
    )
    .unwrap();
    assert_eq!(cfg.id.to_string(), "synth_events");
    assert_eq!(cfg.stage, Stage::Paper, "spec: paper-only initially");
    assert_eq!(
        cfg.comparator.required_tier,
        EdgeTier::Confirmed,
        "synthesis trades confirmed edges only by default"
    );

    // I7: declared Paper is a CAP; with no operator record the strategy
    // runs at Sim. One operator record raises it to (and only to) Paper.
    assert_eq!(effective_stage(cfg.stage, &[]), Stage::Sim);
    let promoted = [PromotionRecord {
        strategy: "synth_events".to_string(),
        from: Stage::Sim,
        to: Stage::Paper,
        actor: "operator:xavier".to_string(),
        at: "2026-06-11T00:00:00.000Z".to_string(),
    }];
    assert_eq!(effective_stage(cfg.stage, &promoted), Stage::Paper);
}

// ------------------------------------------------------ paper fill honesty

#[test]
fn synth_events_maker_quote_fills_only_on_trade_through_in_paper() {
    futures::executor::block_on(async {
        // The mind believes 0.70; the book shows 55/62. Fair 70 vs ask
        // 62: the comparator proposes (max acceptable price 62).
        let mind: Arc<dyn Mind> = Arc::new(StubMind::scripted(vec![belief_output(0.70)]));
        let mut cfg = synth_events_config(
            vec![edge()],
            TriageDecision::AlwaysAccept,
            Some(near_identity_calibration()),
        )
        .unwrap();
        cfg.stage = Stage::Sim; // boundary test runs at the unpromoted stage
        let mut strategy = SynthesisStrategy::new(cfg, mind);

        let book = OrderBook {
            market: mkt("KX-A"),
            as_of: t0(),
            yes_bids: vec![lvl(55, 50)],
            yes_asks: vec![lvl(62, 50)],
        };
        let books = BTreeMap::from([(mkt("KX-A"), book.clone())]);
        let markets = BTreeMap::from([(mkt("KX-A"), market("KX-A"))]);
        let fees = fee_model();
        let core = CoreHandle {
            now: t0(),
            books: &books,
            markets: &markets,
            fee_model: &fees,
        };
        let event = BusEvent {
            seq: 0,
            at: t0(),
            origin: EventOrigin::External,
            payload: EventPayload::BookSnapshot {
                venue: VenueId::new("paper").unwrap(),
                book,
            },
        };
        let proposals = strategy.on_event(&event, &core).await.unwrap();
        assert_eq!(proposals.len(), 1, "fair 70 vs ask 62: propose");
        let leg = &proposals[0].legs[0];
        assert_eq!(leg.side, Side::Yes);
        assert_eq!(leg.action, Action::Buy);
        assert_eq!(leg.limit_price, Cents::new(62), "max acceptable price");

        // The HARNESS owns execution (I6): a Passive proposal may be
        // quoted maker inside the spread at any price <= the limit.
        let quote_price = Cents::new(59);
        assert!(quote_price <= leg.limit_price);

        let clock = Arc::new(SimClock::new(t0()));
        let paper = PaperVenue::new(
            VenueId::new("paper").unwrap(),
            clock,
            fee_model(),
            PaperConfig {
                maker_haircut_pct: 50,
            },
            Cents::new(1_000_000),
        )
        .unwrap();
        paper.add_market(market("KX-A"));
        paper
            .apply_book(&mkt("KX-A"), vec![lvl(55, 50)], vec![lvl(62, 50)])
            .unwrap();

        // Size minimally and gate through the SAME pipeline (I1).
        let order = gate_leg(leg, quote_price, 10);
        paper.place(order).await.unwrap();

        // A print AT the quote is a TOUCH: never a paper fill.
        paper
            .apply_public_trade(&mkt("KX-A"), Cents::new(59), 30)
            .unwrap();
        let fills = paper.fills_since(Cursor::start()).await.unwrap();
        assert!(
            fills.fills.is_empty(),
            "touch at the quote must NOT fill in paper"
        );

        // A trade THROUGH the quote fills, haircut applied (50% of 30
        // public volume = 15, capped at our 10).
        paper
            .apply_public_trade(&mkt("KX-A"), Cents::new(58), 30)
            .unwrap();
        let fills = paper.fills_since(Cursor::start()).await.unwrap();
        assert_eq!(fills.fills.len(), 1, "trade-through fills");
        let fill = &fills.fills[0];
        assert!(fill.is_maker);
        assert_eq!(fill.qty.raw(), 10);
        assert_eq!(fill.price, Cents::new(59), "filled at OUR quote");

        // Settlement realizes the belief: YES pays 100c/contract.
        let pnl = paper.settle_market(&mkt("KX-A"), Side::Yes).unwrap();
        assert!(pnl > Cents::ZERO, "winning settlement positive: {pnl}");
    });
}

/// Minimal sizing + the real gate pipeline (the T0.9 paper boundary
/// idiom), at the harness's chosen quote price within the leg's limit.
fn gate_leg(leg: &ProposedLeg, quote_price: Cents, qty: i64) -> fortuna_gates::GatedOrder {
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

        [per_strategy.synth_events]
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
    let mut ids = IdGen::new(7);
    let intent = IntentId::new(ids.next(t0()).unwrap());
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("synth_events").unwrap(),
        venue: VenueId::new("paper").unwrap(),
        market: leg.market.clone(),
        side: leg.side,
        action: leg.action,
        limit_price: quote_price,
        qty: Contracts::new(qty),
        fair_value: leg.fair_value,
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
        last_trade_price: Some(Cents::new(58)),
        fee_model: &fees,
        category: None,
        own_resting: &[],
        recent_client_order_ids: &recent,
    };
    pipeline.evaluate(&candidate, &inputs).gated.unwrap()
}
