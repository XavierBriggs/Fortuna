//! `funding_forecast` unit tests (perp-strategies-and-scalar-claims §2.2/§2.3;
//! GAPS R1). Written from the design + adjudication BEFORE the strategy.
//!
//! Contract under test (the zero-capital scalar belief-producer):
//! - A `PerpTick` produces EXACTLY ONE `ScalarBeliefDraft` whose
//!   `predictive.validate()` is `Ok` (a valid quantile fan).
//! - The point forecast is the recorded estimate, finalized DIRECTLY (R1):
//!   `finalize_funding_rate(estimate)`; a near-zero estimate forecasts ~0
//!   (the venue zero-threshold).
//! - The dispersion band WIDENS with `remaining` candles: early-in-window the
//!   band strictly contains the near-close band; at `remaining == 0` the band
//!   collapses and all three quantile values equal the median (= the point
//!   forecast).
//! - A window roll (a new `next_funding_time`) RESETS the per-market state.
//! - `on_event` NEVER returns a non-empty `Vec<Proposal>` (zero-capital).
//! - DETERMINISM: the same `PerpTick` sequence yields byte-identical drafts.

use fortuna_cognition::scalar_beliefs::ScalarBeliefDraft;
use fortuna_cognition::scoring::{PredictiveDistribution, Quantile};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::funding_forecast::{FundingForecast, DISPERSION_SCALE};
use fortuna_runner::{CoreHandle, Proposal, Strategy};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;

// ── harness ────────────────────────────────────────────────────────────────

const MARKET: &str = "KXBTCPERP1";
/// The window finalization the estimate targets.
const NEXT_FUNDING: &str = "2026-06-12T20:00:00.000Z";

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn mkt() -> MarketId {
    MarketId::new(MARKET).unwrap()
}

fn ts(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(s).unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

/// A `CoreHandle` with no books/markets — funding_forecast never reads them
/// (it consumes `PerpTick` only). `now` is irrelevant to the strategy (it uses
/// `funding.obs_at`); set it to the observation time for tidiness.
struct World {
    books: BTreeMap<MarketId, fortuna_core::book::OrderBook>,
    markets: BTreeMap<MarketId, fortuna_venues::Market>,
    fees: ScheduleFeeModel,
    now: UtcTimestamp,
}

impl World {
    fn new(now: UtcTimestamp) -> World {
        World {
            books: BTreeMap::new(),
            markets: BTreeMap::new(),
            fees: fee_model(),
            now,
        }
    }
    fn handle(&self) -> CoreHandle<'_> {
        CoreHandle {
            now: self.now,
            books: &self.books,
            markets: &self.markets,
            fee_model: &self.fees,
        }
    }
}

/// Build a `PerpTick` bus event with the given estimate, observation time and
/// window finalization. `reference_price`/`marks` are present-but-unused by the
/// forecast (it uses the estimate directly, R1); they carry plausible values so
/// the event is well-formed.
fn perp_tick(estimate: Decimal, obs_at: &str, next_funding: &str) -> BusEvent {
    let marks = PerpMarks {
        venue_settlement: PerpPrice::from_dollars_exact(dec("6.3332")).unwrap(),
        conservative: None,
    };
    let funding = FundingObservation {
        estimate,
        next_funding_time: ts(next_funding),
        reference_price: PerpPrice::from_dollars_exact(dec("6.3000")).unwrap(),
        obs_at: ts(obs_at),
    };
    BusEvent {
        seq: 1,
        at: ts(obs_at),
        origin: EventOrigin::External,
        payload: EventPayload::PerpTick {
            venue: VenueId::new("kinetics").unwrap(),
            market: mkt(),
            marks,
            funding,
        },
    }
}

fn run(s: &mut FundingForecast, w: &World, ev: &BusEvent) -> Vec<Proposal> {
    futures::executor::block_on(s.on_event(ev, &w.handle())).unwrap()
}

/// The single quantile fan emitted by one tick (asserts exactly-one + valid).
fn emit_one(s: &mut FundingForecast, w: &World, ev: &BusEvent) -> Vec<Quantile> {
    let proposals = run(s, w, ev);
    assert!(proposals.is_empty(), "zero-capital: never a proposal");
    let drafts = s.drain_scalar_beliefs();
    assert_eq!(drafts.len(), 1, "exactly one scalar belief per PerpTick");
    let draft = &drafts[0];
    draft
        .predictive
        .validate()
        .expect("emitted predictive must validate");
    match &draft.predictive {
        PredictiveDistribution::Scalar { quantiles, unit } => {
            assert_eq!(unit, "rate");
            quantiles.clone()
        }
        other => panic!("expected Scalar, got {other:?}"),
    }
}

fn q_at(quantiles: &[Quantile], q: f64) -> f64 {
    quantiles
        .iter()
        .find(|x| (x.q - q).abs() < 1e-12)
        .unwrap_or_else(|| panic!("no quantile at q={q}"))
        .v
}

/// Half-band width (high − low) of the fan.
fn band_width(quantiles: &[Quantile]) -> f64 {
    q_at(quantiles, 0.9) - q_at(quantiles, 0.1)
}

// ── tests ──────────────────────────────────────────────────────────────────

#[test]
fn one_perp_tick_emits_one_valid_scalar_belief() {
    let mut s = FundingForecast::new().unwrap();
    // Observe mid-window (8h window opened 12:00, obs at 16:00 -> 240 min left).
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    let ev = perp_tick(dec("0.0005"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    // The median IS the point forecast (estimate finalized directly, R1).
    assert_eq!(q_at(&fan, 0.5), 0.0005, "median == finalize(estimate)");
    // A non-degenerate band (remaining > 0).
    assert!(band_width(&fan) > 0.0, "band is open mid-window");
}

#[test]
fn point_forecast_is_the_estimate_finalized_directly() {
    // R1: the estimate is the point forecast (finalized), NOT fed into a
    // FundingWindow. A 0.4% estimate passes finalize unchanged -> median 0.004.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    let ev = perp_tick(dec("0.0040"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    assert_eq!(q_at(&fan, 0.5), 0.004);
}

#[test]
fn near_zero_estimate_forecasts_about_zero() {
    // The venue zero-threshold: |rate| < 0.01% -> 0. An estimate of 0.00005
    // (half the threshold) finalizes to exactly 0, so the median is 0.0 and the
    // fan is a small symmetric band around 0.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    let ev = perp_tick(dec("0.00005"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    assert_eq!(q_at(&fan, 0.5), 0.0, "sub-threshold estimate -> median 0");
    // Symmetric around 0 (within float tolerance).
    let lo = q_at(&fan, 0.1);
    let hi = q_at(&fan, 0.9);
    assert!(lo < 0.0 && hi > 0.0, "band straddles 0");
    assert!((lo + hi).abs() < 1e-12, "symmetric around the zero median");
}

#[test]
fn exact_zero_estimate_forecasts_zero() {
    // The recorded estimate fixture carries funding_rate == 0; finalize(0) = 0.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    let ev = perp_tick(dec("0"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    assert_eq!(q_at(&fan, 0.5), 0.0);
}

#[test]
fn a2b_emits_exactly_the_seven_fixed_quantiles() {
    // Design §2.6 A2b: funding_forecast's `PredictiveDistribution::Scalar` carries
    // EXACTLY the seven quantiles {0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95} — the
    // fixed set characterizing the body + both tails for CRPS and band-coverage
    // (supersedes the prior unfixed 3-point fan). MUTATION GUARD: drop/add/reorder
    // a level, or revert to the 3-point set, and this reds.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    let ev = perp_tick(dec("0.0005"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    let qs: Vec<f64> = fan.iter().map(|x| x.q).collect();
    assert_eq!(
        qs,
        vec![0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95],
        "the fixed §2.6 A2b seven-quantile set, in strictly-increasing order"
    );
    // The values stay non-decreasing across the wider set (validate_scalar
    // guarantees it; pin it so a multiplier sign/order slip reds here).
    for pair in fan.windows(2) {
        assert!(
            pair[1].v >= pair[0].v,
            "quantile values non-decreasing across the 7-fan: {fan:?}"
        );
    }
}

#[test]
fn dispersion_widens_with_remaining_candles() {
    // Earlier in the window (more remaining) => a STRICTLY WIDER band than near
    // the close. The band shrinks as sqrt(remaining/window).
    let estimate = dec("0.0005");

    // Early: window opens 12:00, observe 12:30 -> 450 min remaining.
    let mut s_early = FundingForecast::new().unwrap();
    let w_early = World::new(ts("2026-06-12T12:30:00.000Z"));
    let early = emit_one(
        &mut s_early,
        &w_early,
        &perp_tick(estimate, "2026-06-12T12:30:00.000Z", NEXT_FUNDING),
    );

    // Near close: observe 19:30 -> 30 min remaining.
    let mut s_late = FundingForecast::new().unwrap();
    let w_late = World::new(ts("2026-06-12T19:30:00.000Z"));
    let late = emit_one(
        &mut s_late,
        &w_late,
        &perp_tick(estimate, "2026-06-12T19:30:00.000Z", NEXT_FUNDING),
    );

    assert!(
        band_width(&early) > band_width(&late),
        "early band {} must exceed near-close band {}",
        band_width(&early),
        band_width(&late)
    );
    // The early fan strictly CONTAINS the late fan (proper superset of the
    // central interval): lower-low and higher-high.
    assert!(q_at(&early, 0.1) < q_at(&late, 0.1), "early low is lower");
    assert!(q_at(&early, 0.9) > q_at(&late, 0.9), "early high is higher");
}

#[test]
fn band_collapses_to_the_point_at_window_close() {
    // remaining == 0 (obs_at == next_funding_time): the band is 0; all three
    // quantile values equal the median (= the point forecast). Still a valid
    // distribution (equal v is non-decreasing; >=2 quantiles).
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts(NEXT_FUNDING));
    let ev = perp_tick(dec("0.0005"), NEXT_FUNDING, NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    assert_eq!(q_at(&fan, 0.1), 0.0005);
    assert_eq!(q_at(&fan, 0.5), 0.0005);
    assert_eq!(q_at(&fan, 0.9), 0.0005);
    assert_eq!(band_width(&fan), 0.0, "band fully collapsed at close");
}

#[test]
fn band_is_largest_at_window_open() {
    // At remaining == FUNDING_CANDLES_PER_WINDOW (480 min before close) the
    // half-band is exactly DISPERSION_SCALE * 1.282 (sqrt(1) = 1).
    let mut s = FundingForecast::new().unwrap();
    // 480 minutes (8h) before the 20:00 close -> 12:00 open.
    let w = World::new(ts("2026-06-12T12:00:00.000Z"));
    let ev = perp_tick(dec("0"), "2026-06-12T12:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    let hi = q_at(&fan, 0.9);
    let expected = DISPERSION_SCALE * 1.282;
    assert!(
        (hi - expected).abs() < 1e-9,
        "open half-band {hi} != DISPERSION_SCALE*1.282 {expected}"
    );
    // The dispersion scale is 0.2% (a conservative rung-0 width), and even the
    // widest tail spread (1.282 * 0.002) stays well inside the venue's ±2%
    // (0.02) finalization clamp — the band never needs the clamp at p == 0.
    assert!((DISPERSION_SCALE - 0.002).abs() < 1e-12, "scale is 0.2%");
    assert!(
        expected < 0.02,
        "widest open spread stays inside the ±2% clamp"
    );
}

#[test]
fn quantile_fan_never_crosses_even_near_the_clamp() {
    // A point forecast pinned at the +2% clamp: the upper quantile clamps back
    // to +0.02 (collapsing the upper half-band) while the lower stays open. The
    // values must remain NON-DECREASING (v_low <= median <= v_high) so
    // validate() passes — the symmetric-clamp edge case.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T12:00:00.000Z"));
    // Estimate above the clamp -> finalize pins it to +0.02.
    let ev = perp_tick(dec("0.05"), "2026-06-12T12:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    assert_eq!(q_at(&fan, 0.5), 0.02, "median pinned at +clamp");
    assert_eq!(q_at(&fan, 0.9), 0.02, "upper clamps to +clamp (collapsed)");
    assert!(q_at(&fan, 0.1) < 0.02, "lower half-band stays open");
    // validate() inside emit_one already proved non-crossing; assert order too.
    assert!(q_at(&fan, 0.1) <= q_at(&fan, 0.5));
    assert!(q_at(&fan, 0.5) <= q_at(&fan, 0.9));
}

#[test]
fn negative_clamp_edge_also_validates() {
    // Mirror of the +clamp case at the -2% floor: lower clamps to -0.02, upper
    // stays open, still non-decreasing.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T12:00:00.000Z"));
    let ev = perp_tick(dec("-0.05"), "2026-06-12T12:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    assert_eq!(q_at(&fan, 0.5), -0.02);
    assert_eq!(q_at(&fan, 0.1), -0.02);
    assert!(q_at(&fan, 0.9) > -0.02);
}

#[test]
fn event_key_pins_market_and_resolution_window() {
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    run(
        &mut s,
        &w,
        &perp_tick(dec("0.0005"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING),
    );
    let drafts = s.drain_scalar_beliefs();
    assert_eq!(drafts.len(), 1);
    let draft = &drafts[0];
    assert_eq!(draft.event_key, format!("{MARKET}:{NEXT_FUNDING}"));
    assert_eq!(draft.horizon, ts(NEXT_FUNDING));
    // Evidence is DATA, carrying the estimate + remaining (spec 5.11 — never
    // instructions). provenance is left default for the harness to stamp.
    assert!(draft.evidence.get("estimate").is_some());
    assert!(draft.evidence.get("remaining_candles").is_some());
    assert_eq!(draft.provenance, serde_json::Value::Null);
}

#[test]
fn window_roll_resets_state_and_emits_for_the_new_window() {
    // Two PerpTicks with DIFFERENT next_funding_time = two windows. The second
    // is keyed to the new window and the per-market state rolled (last estimate
    // does not bleed across).
    let mut s = FundingForecast::new().unwrap();

    let w1 = World::new(ts("2026-06-12T16:00:00.000Z"));
    run(
        &mut s,
        &w1,
        &perp_tick(dec("0.0005"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING),
    );
    let first = s.drain_scalar_beliefs();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].event_key, format!("{MARKET}:{NEXT_FUNDING}"));

    // New window finalizing at 2026-06-13T04:00:00Z.
    let next2 = "2026-06-13T04:00:00.000Z";
    let w2 = World::new(ts("2026-06-12T22:00:00.000Z"));
    run(
        &mut s,
        &w2,
        &perp_tick(dec("0.0009"), "2026-06-12T22:00:00.000Z", next2),
    );
    let second = s.drain_scalar_beliefs();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].event_key, format!("{MARKET}:{next2}"));
    assert_eq!(
        second[0].horizon,
        ts(next2),
        "the rolled window's horizon is the new next_funding_time"
    );
}

#[test]
fn on_event_never_proposes_across_many_ticks() {
    // Zero-capital: NO PerpTick (any estimate, any window position) yields a
    // Proposal. Drive a sweep including the clamp extremes.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    for est in ["-0.05", "-0.0005", "0", "0.00005", "0.0005", "0.05"] {
        let ev = perp_tick(dec(est), "2026-06-12T16:00:00.000Z", NEXT_FUNDING);
        let proposals = run(&mut s, &w, &ev);
        assert!(
            proposals.is_empty(),
            "estimate {est} must not produce a proposal"
        );
    }
}

#[test]
fn non_perp_events_are_ignored() {
    // A non-PerpTick event produces neither a proposal nor a belief.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T16:00:00.000Z"));
    let settled = BusEvent {
        seq: 1,
        at: ts("2026-06-12T16:00:00.000Z"),
        origin: EventOrigin::External,
        payload: EventPayload::Settled {
            venue: VenueId::new("kinetics").unwrap(),
            market: mkt(),
            payout_cents: 100,
        },
    };
    let proposals = run(&mut s, &w, &settled);
    assert!(proposals.is_empty());
    assert!(
        s.drain_scalar_beliefs().is_empty(),
        "a non-PerpTick event produces no belief"
    );
}

#[test]
fn identical_tick_sequence_yields_byte_identical_drafts() {
    // DETERMINISM: two independent strategy instances fed the same PerpTick
    // sequence emit byte-identical drafts (JSON-serialized).
    fn run_sequence() -> Vec<ScalarBeliefDraft> {
        let mut s = FundingForecast::new().unwrap();
        let mut out = Vec::new();
        let ticks = [
            (dec("0.0005"), "2026-06-12T12:30:00.000Z", NEXT_FUNDING),
            (dec("0.0007"), "2026-06-12T16:00:00.000Z", NEXT_FUNDING),
            (
                dec("0.0003"),
                "2026-06-13T01:00:00.000Z",
                "2026-06-13T04:00:00.000Z",
            ),
        ];
        for (est, obs, nxt) in ticks {
            let w = World::new(ts(obs));
            run(&mut s, &w, &perp_tick(est, obs, nxt));
            out.extend(s.drain_scalar_beliefs());
        }
        out
    }
    let a = run_sequence();
    let b = run_sequence();
    let aj = serde_json::to_string(&a).unwrap();
    let bj = serde_json::to_string(&b).unwrap();
    assert_eq!(aj, bj, "same PerpTick sequence -> byte-identical drafts");
    assert_eq!(a.len(), 3);
}

#[test]
fn past_due_window_degrades_to_collapsed_band_not_panic() {
    // A next_funding_time already in the PAST relative to obs_at (stale frame /
    // clock skew): remaining clamps to 0, the band collapses, no panic, still a
    // valid distribution.
    let mut s = FundingForecast::new().unwrap();
    let w = World::new(ts("2026-06-12T21:00:00.000Z"));
    // obs_at AFTER next_funding_time.
    let ev = perp_tick(dec("0.0005"), "2026-06-12T21:00:00.000Z", NEXT_FUNDING);
    let fan = emit_one(&mut s, &w, &ev);
    assert_eq!(band_width(&fan), 0.0, "past-due window -> collapsed band");
    assert_eq!(q_at(&fan, 0.5), 0.0005);
}

// Silence the unused-import lint for Cents (kept for parity with the suite's
// other harnesses; PerpPrice math here uses Decimal directly).
#[allow(dead_code)]
fn _cents_is_in_scope() -> Cents {
    Cents::ZERO
}
