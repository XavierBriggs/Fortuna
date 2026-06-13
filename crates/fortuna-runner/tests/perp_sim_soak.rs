//! Slice 4b — the perp INGESTION seam (`SimRunner::inject_perp_tick`) drives a
//! REAL perp strategy through a runner tick. Written from the design text
//! (perp-strategies §2.1/§3.2; §5 "confirm they run in a Sim soak") BEFORE the
//! seam.
//!
//! `EventPayload::PerpTick` has no producer in the deterministic `tick()` loop
//! (which only sources `BookSnapshot`s) — so the two perp strategies would be
//! INERT in the daemon without this seam. Here: build a `PerpTick`'s perp-domain
//! components, `inject_perp_tick` them, `tick()`, and assert the live
//! `funding_forecast` strategy FIRED — it produced a scalar belief that drained
//! through the runner (`drain_pending_scalar_beliefs`). The existing
//! `scalar_belief_drain.rs` proved the drain SEAM with a MOCK producer that
//! emits unconditionally; this proves a REAL strategy emits BECAUSE it saw an
//! injected `PerpTick`.

use fortuna_cognition::scoring::PredictiveDistribution;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::funding_forecast::FundingForecast;
use fortuna_runner::{MemoryAuditSink, RunnerConfig, SimRunner};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-13T12:00:00.000Z").unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn runner_config() -> RunnerConfig {
    RunnerConfig {
        seed: 1,
        gate_config: toml::from_str(
            "[global]\nmax_total_exposure_cents=800000\nmax_daily_loss_cents=50000\nmin_order_contracts=1\nmax_order_contracts=1000\nprice_band_cents=45\nmax_cross_cents=10\nper_market_exposure_cents=100000\nper_event_exposure_cents=150000\nrequire_event_mapping=false\n[rate.sim]\nburst=100\nsustained_per_min=600\nmarket_burst=50\nmarket_sustained_per_min=300\n",
        )
        .unwrap(),
        exec_policy: fortuna_exec::ExecPolicy::default(),
        envelopes: BTreeMap::new(),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![],
        starting_cash: Cents::new(1_000_000),
        faults: FaultConfig::none(1),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    }
}

/// A `PerpTick`'s perp-domain components for `inject_perp_tick`: a $63,100
/// settlement mark, a $63,000 reference, a small funding estimate, and a
/// funding window that finalizes 4h after the observation (so the in-progress
/// window has remaining time for the dispersion model).
fn perp_components() -> (VenueId, MarketId, PerpMarks, FundingObservation) {
    (
        VenueId::new("kinetics").unwrap(),
        MarketId::new("KXBTCPERP").unwrap(),
        PerpMarks {
            venue_settlement: PerpPrice::new(631_000),
            conservative: None,
        },
        FundingObservation {
            estimate: Decimal::from_str("0.0001").unwrap(),
            next_funding_time: UtcTimestamp::parse_iso8601("2026-06-13T16:00:00.000Z").unwrap(),
            reference_price: PerpPrice::new(630_000),
            obs_at: t0(),
        },
    )
}

#[test]
fn funding_forecast_fires_on_an_injected_perp_tick() {
    let mut r = SimRunner::new(
        runner_config(),
        vec![Box::new(FundingForecast::new().unwrap())],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .unwrap();

    // A tick with NO injected perp data → funding_forecast sees nothing → no
    // scalar belief (the strategy is inert without a PerpTick — the very gap
    // this seam closes).
    futures::executor::block_on(r.tick()).unwrap();
    assert!(
        r.drain_pending_scalar_beliefs().is_empty(),
        "no PerpTick → funding_forecast produces nothing"
    );

    // Inject a PerpTick through the seam, then tick: the strategy now sees it
    // and forecasts.
    let (venue, market, marks, funding) = perp_components();
    r.inject_perp_tick(venue, market, marks, funding);
    futures::executor::block_on(r.tick()).unwrap();

    let drained = r.drain_pending_scalar_beliefs();
    assert_eq!(
        drained.len(),
        1,
        "funding_forecast produced exactly one scalar belief from the injected tick"
    );
    let (sid, draft) = &drained[0];
    assert_eq!(
        sid.as_str(),
        "funding_forecast",
        "tagged with the producing strategy"
    );
    assert!(
        matches!(draft.predictive, PredictiveDistribution::Scalar { .. }),
        "a scalar funding forecast (a quantile fan)"
    );
    assert!(
        draft.event_key.contains("KXBTCPERP"),
        "the belief keys the perp market it forecast, got {:?}",
        draft.event_key
    );

    // Drains once: a tick with no new PerpTick yields nothing more.
    futures::executor::block_on(r.tick()).unwrap();
    assert!(
        r.drain_pending_scalar_beliefs().is_empty(),
        "no new PerpTick → no new belief (drains once)"
    );
}
