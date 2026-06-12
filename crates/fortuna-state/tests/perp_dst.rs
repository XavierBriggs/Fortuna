//! T5.B6: seeded DST over the PERP margin/funding/liquidation plane
//! (spec 5.15, plan §B6). Drives the perp gate arm (fortuna-gates) and
//! the margin simulator (fortuna-state MarginSim) from seeds: funding-tick
//! chaos, liquidation under ack-delay/api-error, system-fill ingestion,
//! margin-call sequences, and demo-divergence (suffixed-ticker) confusion
//! — with per-arm hit accounting (a battery that never fired an arm
//! fails at the coverage floor).
//!
//! Per-seed invariants:
//!   1. No step ever panics; modeled errors only where the scenario
//!      designed them.
//!   2. IDEMPOTENCY: no client order id is ever applied to the margin
//!      account twice, across ack-delays and api-error retries.
//!   3. GATE-PASS => NO INSTANT LIQUIDATION: a risk-adding fill that
//!      passed the gates never liquidates at the unchanged marks it was
//!      gated against. (Holds because the harness config keeps
//!      min_liquidation_distance_bps >= price_band_bps — the buffer the
//!      distance floor guarantees covers the worst instant mark-to-limit
//!      loss the price band allows. Operator guidance ledgered in
//!      ASSUMPTIONS.)
//!   4. LIQUIDATION IS NEVER SILENT: every liquidation clears all
//!      positions (system-fill ingestion), and the halt evaluation that
//!      follows blocks every subsequent order at check 1 for the rest of
//!      the scenario (spec 5.15: trading on a wrong margin model is not
//!      allowed to continue silently).
//!   5. CONSERVATION: the sim balance always equals the harness's i128
//!      mirror of initial + realized - fees + funding + liquidation
//!      effects (exact, every step).
//!   6. DEMO-DIVERGENCE: an order for the demo-suffixed ticker
//!      (KXBTCPERP1, absent from config) fails closed at the first perp
//!      check; it never reaches the margin account.
//!   7. DETERMINISM: re-running a scenario from the same seed produces a
//!      byte-identical final-state digest.
//!
//! Conventions follow the other DST harnesses: master seed from
//! DST_MASTER_SEED or wall clock (printed), scenario count via
//! PERP_DST_SCENARIOS (default 20), failures print their seed.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IdGen, IntentId, SplitMix64};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{MarginAccountView, PerpMarks, PerpPrice};
use fortuna_gates::perp::{PerpCandidateOrder, PerpGateInputs};
use fortuna_gates::{GateCheck, GateConfig, GatePipeline, HaltScope};
use fortuna_state::{funding_times_between, MarginSim, MarginSimConfig, RiskCurve};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};

const MARKET: &str = "KXBTCPERP";
const DEMO_MARKET: &str = "KXBTCPERP1";

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T00:30:00.000Z").unwrap()
}

fn mkt(s: &str) -> MarketId {
    MarketId::new(s).unwrap()
}

/// Harness gate config. min_liquidation_distance_bps (500) >=
/// price_band_bps (300): see invariant 3.
fn gate_config() -> GateConfig {
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

        [per_strategy.perp_dst]
        max_exposure_cents = 100000000
        max_order_notional_cents = 100000000
        min_net_edge_bps = 0

        [rate.kinetics]
        burst = 1000000
        sustained_per_min = 1000000
        market_burst = 1000000
        market_sustained_per_min = 1000000

        [perp.venues.kinetics]
        max_total_notional_cents = 100000000
        min_order_contracts = 1
        max_order_contracts = 10000
        price_band_bps = 300
        assumed_fee_bps = 12
        funding_drag_bps_per_window = 4
        min_liquidation_distance_bps = 500
        mm_safety_multiplier_pct = 130

        [perp.assets.KXBTCPERP]
        max_leverage_x10 = 100
        max_notional_cents = 10000000
        mm_curve = [[2000000, 500], [10000000, 800]]
        "#,
    )
    .expect("harness gate config parses")
}

/// The SAME curve and multiplier as the gate config: the sim must be at
/// least as strict as the gates assume.
fn sim_config() -> MarginSimConfig {
    let mut curves = BTreeMap::new();
    curves.insert(
        mkt(MARKET),
        RiskCurve {
            tiers: vec![(Cents::new(2_000_000), 500), (Cents::new(10_000_000), 800)],
        },
    );
    MarginSimConfig {
        mm_multiplier_pct: 130,
        liquidation_penalty_bps: 100,
        curves,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Arm {
    FundingTick,
    Liquidation,
    MarginReject,
    AckDelayFill,
    ApiErrorRetry,
    DemoDivergence,
}

#[derive(Default)]
struct ArmCounts(BTreeMap<&'static str, u64>);

impl ArmCounts {
    fn hit(&mut self, arm: Arm) {
        let key = match arm {
            Arm::FundingTick => "funding_tick",
            Arm::Liquidation => "liquidation",
            Arm::MarginReject => "margin_reject",
            Arm::AckDelayFill => "ack_delay_fill",
            Arm::ApiErrorRetry => "api_error_retry",
            Arm::DemoDivergence => "demo_divergence",
        };
        *self.0.entry(key).or_insert(0) += 1;
    }
}

/// A pending venue outcome for a gated order (ack-delay / retry modeling).
struct PendingFill {
    market: MarketId,
    action: Action,
    qty: Contracts,
    price: PerpPrice,
    client_order_id: String,
    /// Steps remaining before the fill lands.
    delay: u32,
    /// True when this fill came through the api-error retry path.
    via_retry: bool,
}

/// Final-state digest for the determinism invariant.
#[derive(Debug, PartialEq, Eq)]
struct Digest {
    balance: i64,
    position_qty: i64,
    position_entry: i64,
    funding_entries: usize,
    fills_applied: u64,
    liquidated: bool,
    gated_ok: u64,
    gated_rejected: u64,
}

fn pick(rng: &mut SplitMix64, n: u64) -> u64 {
    rng.next_u64() % n.max(1)
}

fn run_scenario(seed: u64, arms: &mut ArmCounts) -> Result<Digest, String> {
    let mut rng = SplitMix64::new(seed);
    let mut gates = GatePipeline::new(gate_config()).map_err(|e| format!("gate config: {e}"))?;
    // Volatility regime (seeded). Calm scenarios run a comfortable account
    // with small random-direction flow (funding/idempotency/demo arms);
    // WILD scenarios are margin-call sequences: a small account, large
    // directionally-biased orders that ride the gate's distance floor, and
    // hard walks with occasional 5-15% gaps — the path that actually
    // reaches liquidation.
    let wild = pick(&mut rng, 2) == 1;
    let initial_balance: i64 = if wild {
        20_000 + pick(&mut rng, 180_001) as i64
    } else {
        1_000_000
    };
    let bias_buy = pick(&mut rng, 2) == 0;
    let mut sim = MarginSim::new(Cents::new(initial_balance), sim_config())
        .map_err(|e| format!("sim config: {e}"))?;
    let mut idgen = IdGen::new(seed ^ 0x5eed);

    let mut now = t0();
    let mut venue_mark: i64 = 62_600;
    let mut applied_ids: BTreeSet<String> = BTreeSet::new();
    let mut recent_ids: BTreeSet<String> = BTreeSet::new();
    let mut pending: Vec<PendingFill> = Vec::new();
    let mut expected_balance: i128 = i128::from(initial_balance);
    let mut halted = false;
    let mut fills_applied: u64 = 0;
    let mut gated_ok: u64 = 0;
    let mut gated_rejected: u64 = 0;

    let steps = 30 + pick(&mut rng, 40);
    for step in 0..steps {
        // -- time advances 10min..6h; funding ticks fire on the schedule --
        let advance_ms = 600_000 + pick(&mut rng, 6 * 3_600_000 - 600_000) as i64;
        let next = now
            .checked_add_millis(advance_ms)
            .map_err(|e| format!("seed {seed}: clock overflow: {e}"))?;
        for tick in funding_times_between(now, next).map_err(|e| format!("seed {seed}: {e}"))? {
            // Rate within the venue's +/-2% cap, in 1e-5 steps.
            let r = pick(&mut rng, 4001) as i64 - 2000;
            let rate = Decimal::new(r, 5);
            let accrued = sim
                .apply_funding(&mkt(MARKET), rate, PerpPrice::new(venue_mark), tick)
                .map_err(|e| format!("seed {seed}: funding: {e}"))?;
            if let Some(acc) = accrued {
                expected_balance += i128::from(acc.amount.raw());
                arms.hit(Arm::FundingTick);
            }
        }
        now = next;

        // -- the mark random-walks; the conservative mark sits at or below --
        let walk = if wild {
            pick(&mut rng, 6_001) as i64 - 3_000
        } else {
            pick(&mut rng, 801) as i64 - 400
        };
        venue_mark = (venue_mark + walk).max(1_000);
        if wild && pick(&mut rng, 100) < 5 {
            // Gap move: +/- 5-15% of the mark in one step.
            let gap_bps = 500 + pick(&mut rng, 1_001) as i64;
            let gap = venue_mark * gap_bps / 10_000;
            venue_mark = if pick(&mut rng, 2) == 0 {
                (venue_mark - gap).max(1_000)
            } else {
                venue_mark + gap
            };
        }
        let cons_gap = pick(&mut rng, 101) as i64;
        let cons_mark = (venue_mark - cons_gap).max(1);
        let marks: BTreeMap<MarketId, PerpMarks> = {
            let mut m = BTreeMap::new();
            m.insert(
                mkt(MARKET),
                PerpMarks {
                    venue_settlement: PerpPrice::new(venue_mark),
                    conservative: Some(PerpPrice::new(cons_mark)),
                },
            );
            m
        };

        // -- pending venue outcomes land (ack-delay / retry) --
        let mut landing = Vec::new();
        for mut p in pending.drain(..) {
            if p.delay == 0 {
                landing.push(p);
            } else {
                p.delay -= 1;
                landing.push(p);
                continue;
            }
        }
        let (due, still_pending): (Vec<_>, Vec<_>) =
            landing.into_iter().partition(|p| p.delay == 0);
        pending = still_pending;
        for p in due {
            // IDEMPOTENCY: a client id lands at most once, ever.
            if !applied_ids.insert(p.client_order_id.clone()) {
                return Err(format!(
                    "seed {seed}: client id {} applied twice",
                    p.client_order_id
                ));
            }
            let out = sim
                .apply_fill(&p.market, p.action, p.qty, p.price, Cents::ZERO)
                .map_err(|e| format!("seed {seed}: delayed fill: {e}"))?;
            expected_balance += i128::from(out.realized_pnl.raw());
            fills_applied += 1;
            if p.via_retry {
                arms.hit(Arm::ApiErrorRetry);
            } else {
                arms.hit(Arm::AckDelayFill);
            }
        }

        // -- maybe submit an order --
        if pick(&mut rng, 100) < 70 {
            let demo_divergence = pick(&mut rng, 100) < 5;
            let market = if demo_divergence { DEMO_MARKET } else { MARKET };
            let follow_bias = wild && pick(&mut rng, 100) < 75;
            let action = if follow_bias {
                if bias_buy {
                    Action::Buy
                } else {
                    Action::Sell
                }
            } else if pick(&mut rng, 2) == 0 {
                Action::Buy
            } else {
                Action::Sell
            };
            let qty = if wild {
                1 + pick(&mut rng, 1_500) as i64
            } else {
                1 + pick(&mut rng, 200) as i64
            };
            // Limit within the price band of the conservative mark the
            // gate sees (some land outside and reject: also fine).
            let off = pick(&mut rng, 401) as i64 - 200;
            let limit = (cons_mark + off).max(1);
            // Fair value favorable to the action so the edge floor mostly
            // passes (occasionally unfavorable: rejects are part of chaos).
            let edge_off = pick(&mut rng, 700) as i64;
            let fair = match action {
                Action::Buy => limit + edge_off,
                Action::Sell => (limit - edge_off).max(1),
            };
            let reduce_only = pick(&mut rng, 10) == 0;
            let client_order_id = format!("perp-dst-{seed}-{step}");
            let candidate = PerpCandidateOrder {
                intent_id: IntentId::new(
                    idgen
                        .next(now)
                        .map_err(|e| format!("seed {seed}: id: {e}"))?,
                ),
                strategy: StrategyId::new("perp_dst").map_err(|e| format!("{e}"))?,
                venue: VenueId::new("kinetics").map_err(|e| format!("{e}"))?,
                market: mkt(market),
                action,
                reduce_only,
                limit_price: PerpPrice::new(limit),
                qty: Contracts::new(qty),
                fair_value: PerpPrice::new(fair),
                holding_windows: 1 + pick(&mut rng, 5) as u32,
                client_order_id: ClientOrderId::new(&client_order_id)
                    .map_err(|e| format!("{e}"))?,
            };
            let account: MarginAccountView = sim
                .account_view(&marks, Cents::ZERO)
                .map_err(|e| format!("seed {seed}: account view: {e}"))?;
            let venue_open = match sim.position(&mkt(MARKET)) {
                Some(p) => p
                    .notional_at(PerpPrice::new(venue_mark))
                    .map_err(|e| format!("seed {seed}: notional: {e}"))?,
                None => Cents::ZERO,
            };
            let inputs = PerpGateInputs {
                now,
                account: &account,
                position: sim.position(&mkt(market)),
                conservative_mark: PerpPrice::new(cons_mark),
                venue_open_notional_cents: venue_open,
                own_resting: &[],
                recent_client_order_ids: &recent_ids,
            };
            let outcome = gates.evaluate_perp(&candidate, &inputs);
            match outcome.gated {
                Err(rejection) => {
                    gated_rejected += 1;
                    if halted {
                        // Invariant 4: after a liquidation, EVERYTHING
                        // rejects at check 1.
                        if rejection.check != GateCheck::Halts {
                            return Err(format!(
                                "seed {seed}: post-liquidation order rejected at {:?}, \
                                 not Halts",
                                rejection.check
                            ));
                        }
                    } else if demo_divergence {
                        // Invariant 6: suffixed ticker fails closed at the
                        // first perp check.
                        if rejection.check != GateCheck::MarginHeadroom {
                            return Err(format!(
                                "seed {seed}: demo-suffixed ticker rejected at {:?}, \
                                 not fail-closed MarginHeadroom",
                                rejection.check
                            ));
                        }
                        arms.hit(Arm::DemoDivergence);
                    } else if matches!(
                        rejection.check,
                        GateCheck::MarginHeadroom
                            | GateCheck::LiquidationDistance
                            | GateCheck::LeverageCap
                            | GateCheck::PerpNotionalCap
                    ) {
                        arms.hit(Arm::MarginReject);
                    }
                }
                Ok(gated) => {
                    gated_ok += 1;
                    if halted {
                        return Err(format!(
                            "seed {seed}: order passed the gates after a liquidation halt"
                        ));
                    }
                    if demo_divergence {
                        return Err(format!(
                            "seed {seed}: demo-suffixed ticker passed the gates"
                        ));
                    }
                    recent_ids.insert(client_order_id.clone());
                    // -- venue outcome roll --
                    let roll = pick(&mut rng, 100);
                    if roll < 60 {
                        // Immediate fill.
                        if !applied_ids.insert(client_order_id.clone()) {
                            return Err(format!(
                                "seed {seed}: client id {client_order_id} applied twice"
                            ));
                        }
                        let out = sim
                            .apply_fill(
                                gated.market(),
                                gated.action(),
                                gated.qty(),
                                gated.limit_price(),
                                Cents::ZERO,
                            )
                            .map_err(|e| format!("seed {seed}: fill: {e}"))?;
                        expected_balance += i128::from(out.realized_pnl.raw());
                        fills_applied += 1;
                        // Invariant 3: gate-pass => no instant liquidation
                        // at the marks the order was gated against.
                        if !gated.reduce_only() {
                            let liq = sim
                                .check_liquidation(&marks, Cents::ZERO)
                                .map_err(|e| format!("seed {seed}: liq check: {e}"))?;
                            if let Some(event) = liq {
                                return Err(format!(
                                    "seed {seed}: instant liquidation after a gated fill \
                                     (balance_after {})",
                                    event.balance_after
                                ));
                            }
                        }
                    } else if roll < 75 {
                        // Ack-delay: the fill lands 1-3 steps later.
                        pending.push(PendingFill {
                            market: gated.market().clone(),
                            action: gated.action(),
                            qty: gated.qty(),
                            price: gated.limit_price(),
                            client_order_id: client_order_id.clone(),
                            delay: 1 + pick(&mut rng, 3) as u32,
                            via_retry: false,
                        });
                    } else if roll < 90 {
                        // Api-error on submit; exec retries the SAME client
                        // id once; the retry fills half the time.
                        if pick(&mut rng, 2) == 0 {
                            pending.push(PendingFill {
                                market: gated.market().clone(),
                                action: gated.action(),
                                qty: gated.qty(),
                                price: gated.limit_price(),
                                client_order_id: client_order_id.clone(),
                                delay: 1,
                                via_retry: true,
                            });
                        }
                        // else: abandoned after the failed retry; no fill.
                    }
                    // else (roll 90..100): dropped outright; no fill.
                }
            }
        }

        // -- end-of-step margin check: liquidation is never silent --
        if !halted {
            if std::env::var("PERP_DST_DEBUG").is_ok() {
                if let Some(p) = sim.position(&mkt(MARKET)) {
                    let view = sim
                        .account_view(&marks, Cents::ZERO)
                        .map_err(|e| e.to_string())?;
                    eprintln!(
                        "dbg seed {seed} step {step} wild {wild} qty {} entry {} mark {venue_mark} equity {} balance {}",
                        p.qty, p.avg_entry, view.equity, sim.balance()
                    );
                }
            }
            let liq = sim
                .check_liquidation(&marks, Cents::ZERO)
                .map_err(|e| format!("seed {seed}: liq check: {e}"))?;
            if let Some(event) = liq {
                arms.hit(Arm::Liquidation);
                // System-fill ingestion: every position was closed.
                if !event.closed.is_empty() && sim.position(&mkt(MARKET)).is_some() {
                    return Err(format!("seed {seed}: liquidation left a position standing"));
                }
                expected_balance = i128::from(event.balance_after.raw());
                // Spec 5.15: mandatory halt evaluation follows.
                gates.set_halt(HaltScope::Global, "liquidation: margin model was wrong");
                halted = true;
            }
        }

        // -- invariant 5: exact balance conservation, every step --
        if i128::from(sim.balance().raw()) != expected_balance {
            return Err(format!(
                "seed {seed}: balance {} diverged from mirror {expected_balance}",
                sim.balance()
            ));
        }
    }

    let (position_qty, position_entry) = match sim.position(&mkt(MARKET)) {
        Some(p) => (p.qty.raw(), p.avg_entry.raw()),
        None => (0, 0),
    };
    Ok(Digest {
        balance: sim.balance().raw(),
        position_qty,
        position_entry,
        funding_entries: sim.funding_log().len(),
        fills_applied,
        liquidated: halted,
        gated_ok,
        gated_rejected,
    })
}

#[test]
fn perp_plane_survives_seeded_chaos() {
    let scenarios: u64 = std::env::var("PERP_DST_SCENARIOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let master: u64 = std::env::var("DST_MASTER_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(1)
        });
    println!("[perp-dst] master seed {master} -> {scenarios} scenario(s)");

    let mut master_rng = SplitMix64::new(master);
    let mut arms = ArmCounts::default();
    let mut failures: Vec<(u64, String)> = Vec::new();
    for _ in 0..scenarios {
        let seed = master_rng.next_u64();
        match run_scenario(seed, &mut arms) {
            Ok(first) => {
                // Invariant 7: same seed, same digest.
                let mut rerun_arms = ArmCounts::default();
                match run_scenario(seed, &mut rerun_arms) {
                    Ok(second) => {
                        if first != second {
                            failures.push((
                                seed,
                                format!("non-deterministic: {first:?} vs {second:?}"),
                            ));
                        }
                    }
                    Err(e) => failures.push((seed, format!("rerun failed: {e}"))),
                }
            }
            Err(e) => failures.push((seed, e)),
        }
    }

    println!("[perp-dst] arms {:?}", arms.0);
    assert!(
        failures.is_empty(),
        "[perp-dst] master {master}: {} failing seed(s): {:?}",
        failures.len(),
        failures
    );
    // Coverage floor (mirrors settlement_dst): small draws can
    // legitimately miss rare arms; at 100+ scenarios every arm must fire.
    if scenarios >= 100 {
        for arm in [
            "funding_tick",
            "liquidation",
            "margin_reject",
            "ack_delay_fill",
            "api_error_retry",
            "demo_divergence",
        ] {
            assert!(
                arms.0.get(arm).copied().unwrap_or(0) > 0,
                "[perp-dst] master {master}: arm {arm} never fired across {scenarios} scenarios"
            );
        }
    }
}
