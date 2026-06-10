//! Phase 2 EXIT: seeded DST over the COMPOSED decision loop.
//!
//! Every scenario builds a seeded world — two edge-mapped markets, random
//! book schedules, seeded venue faults — and a CHAOS MIND whose seeded
//! script mixes healthy beliefs with every cognition failure mode
//! (provider error, schema-invalid, refusal, budget exhaustion, empty
//! output, beliefs for the wrong event). Invariants checked on every
//! seed:
//!   1. Cognition failure NEVER errors a tick, never panics the loop.
//!   2. Declined-triage scenarios submit ZERO orders (shadow runs and
//!      plain declines never trade).
//!   3. Submitted orders never exceed emitted proposals (one leg each;
//!      nothing invents orders the strategy did not propose).
//!   4. The run stays reportable (money plane consistent) and the
//!      recording is byte-identical when the seed re-runs (replay).
//!
//! Conventions follow crates/fortuna-core/tests/dst.rs: master seed from
//! DST_MASTER_SEED or wall clock (printed), per-scenario seeds derived
//! via SplitMix64, failures print the offending seed. Scenario count via
//! SYNTH_DST_SCENARIOS (default 20).

use fortuna_cognition::context::AssembledContext;
use fortuna_cognition::cycle::{ComparatorConfig, EdgeView, TriageDecision};
use fortuna_cognition::events::{EdgeTier, MappingType};
use fortuna_cognition::mind::{Mind, MindError, MindOutput};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::SplitMix64;
use fortuna_core::market::{MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_runner::synthesis::{SynthesisConfig, SynthesisStrategy};
use fortuna_runner::{MemoryAuditSink, RunnerConfig, SimRunner};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use fortuna_venues::{Market, MarketStatus, PriceLevel, SettlementMeta};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-10T12:00:00.000Z").unwrap()
}

fn mkt(id: &str) -> MarketId {
    MarketId::new(id).unwrap()
}

const MARKETS: [&str; 2] = ["KX-A", "KX-B"];
const EVENTS: [&str; 2] = ["evt-1", "evt-2"];

// ------------------------------------------------------------- chaos mind

/// A mind whose entire behavior is a seed-derived script: deterministic
/// for replay, hostile in distribution.
struct ChaosMind {
    script: Mutex<Vec<Result<MindOutput, MindError>>>,
}

impl ChaosMind {
    /// Pre-generate `calls` scripted responses from the seed.
    fn seeded(seed: u64, calls: usize) -> ChaosMind {
        let mut rng = SplitMix64::new(seed);
        let mut script = Vec::with_capacity(calls);
        for _ in 0..calls {
            let roll = rng.next_u64() % 100;
            let event = EVENTS[(rng.next_u64() % 2) as usize];
            let p = 0.05 + (rng.next_u64() % 90) as f64 / 100.0;
            let entry: Result<MindOutput, MindError> = if roll < 40 {
                Ok(serde_json::from_value(serde_json::json!({
                    "beliefs": [{
                        "event_id": event,
                        "p": p,
                        "p_raw": p,
                        "horizon": "2026-06-20T18:00:00.000Z",
                        "evidence": [{"source": "chaos", "ref": "dst"}]
                    }],
                    "proposals": [],
                    "journal": null
                }))
                .unwrap())
            } else if roll < 55 {
                Ok(MindOutput::empty())
            } else if roll < 70 {
                Err(MindError::Provider {
                    reason: "529 overloaded (dst)".to_string(),
                })
            } else if roll < 80 {
                Err(MindError::SchemaInvalid {
                    reason: "model emitted prose (dst)".to_string(),
                })
            } else if roll < 90 {
                Err(MindError::Refused {
                    explanation: "declined (dst)".to_string(),
                })
            } else {
                Err(MindError::BudgetExhausted {
                    scope: "day",
                    spent_cents: 500,
                    cap_cents: 500,
                })
            };
            script.push(entry);
        }
        script.reverse(); // pop() consumes front-first
        ChaosMind {
            script: Mutex::new(script),
        }
    }
}

#[async_trait::async_trait]
impl Mind for ChaosMind {
    fn id(&self) -> &str {
        "chaos-mind"
    }
    async fn decide(&self, _ctx: &AssembledContext) -> Result<MindOutput, MindError> {
        let Ok(mut script) = self.script.lock() else {
            return Ok(MindOutput::empty());
        };
        script.pop().unwrap_or_else(|| Ok(MindOutput::empty()))
    }
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

// --------------------------------------------------------------- scenario

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
        venue: VenueId::new("sim").unwrap(),
        title: format!("market {id}"),
        category: "weather".into(),
        status: MarketStatus::Trading,
        close_at: None,
        settlement: SettlementMeta {
            oracle_type: "nws".into(),
            resolution_source: "nws".into(),
            expected_lag_hours: 2,
        },
        volume_contracts: None,
        payout_per_contract: Cents::new(100),
    }
}

fn runner_config(seed: u64, faults: FaultConfig) -> RunnerConfig {
    let gate_config = toml::from_str(
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

        [per_strategy.synth_dst]
        max_exposure_cents = 200000
        max_order_notional_cents = 10000
        min_net_edge_bps = 100

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap();
    RunnerConfig {
        seed,
        gate_config,
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("synth_dst".to_string(), Cents::new(300_000))]),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: MARKETS.iter().map(|m| market(m)).collect(),
        starting_cash: Cents::new(1_000_000),
        faults,
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

struct ScenarioResult {
    recording: String,
    orders_submitted: u64,
    proposals_emitted: u64,
    cognition_failures: u64,
    beliefs_drafted: u64,
}

/// One seeded scenario. Deterministic: every random draw comes from the
/// seed, so the same seed replays byte-identically.
fn run_scenario(seed: u64) -> Result<ScenarioResult, String> {
    let mut rng = SplitMix64::new(seed);
    let declined = rng.next_u64().is_multiple_of(4); // 25% of scenarios never accept
    let triage = if declined {
        TriageDecision::AlwaysDecline
    } else {
        TriageDecision::AlwaysAccept
    };
    let shadow_quota = (rng.next_u64() % 3) as u32;
    let ticks = 10 + (rng.next_u64() % 20) as usize;

    let mut faults = FaultConfig::none(rng.next_u64());
    faults.ack_delay_pm = (rng.next_u64() % 150) as u32;
    faults.drop_fill_pm = (rng.next_u64() % 100) as u32;
    faults.dup_fill_pm = (rng.next_u64() % 100) as u32;

    // Seeded calibration chaos: most scenarios run a near-identity
    // fitted scope; some run UNWIRED (beliefs shrink fully to market and
    // price no edge — also a valid world). Quality is seeded too.
    let calibration = if rng.next_u64().is_multiple_of(5) {
        None
    } else {
        Some(near_identity_calibration())
    };
    let quality = (rng.next_u64() % 101) as f64 / 100.0;

    // Worst case two cycles per tick (one per mapped market/event).
    let mind: Arc<dyn Mind> = Arc::new(ChaosMind::seeded(rng.next_u64(), ticks * 2));
    let strategy = SynthesisStrategy::new(
        SynthesisConfig {
            id: StrategyId::new("synth_dst").map_err(|e| e.to_string())?,
            edges: MARKETS
                .iter()
                .zip(EVENTS.iter())
                .map(|(m, e)| EdgeView {
                    market: (*m).to_string(),
                    event_id: (*e).to_string(),
                    mapping: MappingType::Direct,
                    tier: EdgeTier::Confirmed,
                })
                .collect(),
            comparator: ComparatorConfig {
                min_edge_cents: 5,
                required_tier: EdgeTier::Proposed,
            },
            triage,
            shadow_quota,
            calibration,
            stage: fortuna_runner::Stage::Sim,
        },
        mind,
    );

    let mut runner = SimRunner::new(
        runner_config(seed, faults),
        vec![Box::new(strategy)],
        Box::new(MemoryAuditSink::default()),
        t0(),
    )
    .map_err(|e| format!("construction failed: {e}"))?;
    runner.set_calibration_quality("synth_dst", quality);

    let mut proposals_seen: u64 = 0;
    for tick_no in 0..ticks {
        // Random legal two-sided books each tick.
        for m in MARKETS {
            let bid = 1 + (rng.next_u64() % 96) as i64;
            let ask = bid + 1 + (rng.next_u64() % 3) as i64;
            let qty = 10 + (rng.next_u64() % 90) as i64;
            let lvl = |p: i64, q: i64| PriceLevel {
                price: Cents::new(p),
                qty: fortuna_core::market::Contracts::new(q),
            };
            runner
                .venue()
                .set_book(&mkt(m), vec![lvl(bid, qty)], vec![lvl(ask.min(99), qty)])
                .map_err(|e| format!("set_book: {e}"))?;
        }
        // Invariant 1: cognition chaos NEVER errors the tick.
        let report = futures::executor::block_on(runner.tick())
            .map_err(|e| format!("tick {tick_no} errored under cognition chaos: {e}"))?;
        proposals_seen += report.proposals as u64;
        runner
            .clock
            .advance_millis(200 + rng.next_u64() % 1800)
            .map_err(|e| e.to_string())?;
    }

    let counters = runner.counters();
    // Invariant 2: declined scenarios never trade (shadow included).
    if declined && counters.orders_submitted > 0 {
        return Err(format!(
            "declined-triage scenario submitted {} orders",
            counters.orders_submitted
        ));
    }
    // Invariant 3: no order without a proposal behind it (synthesis legs
    // are single-leg, so orders can never exceed proposals).
    if counters.orders_submitted > proposals_seen {
        return Err(format!(
            "orders ({}) exceed proposals ({proposals_seen})",
            counters.orders_submitted
        ));
    }
    // Invariant 4: the money plane stays reportable.
    let report = runner.report().map_err(|e| format!("report failed: {e}"))?;

    Ok(ScenarioResult {
        recording: report.recording_jsonl,
        orders_submitted: counters.orders_submitted,
        proposals_emitted: proposals_seen,
        cognition_failures: counters.cognition_failures,
        beliefs_drafted: counters.beliefs_drafted,
    })
}

#[test]
fn synthesis_loop_survives_cognition_chaos_on_every_seed() {
    let scenarios: u64 = std::env::var("SYNTH_DST_SCENARIOS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let master: u64 = std::env::var("DST_MASTER_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            use fortuna_core::clock::Clock;
            fortuna_core::clock::RealClock.now().epoch_millis() as u64
        });
    println!("[synthesis-dst] master seed {master} -> {scenarios} scenario(s)");

    let mut master_rng = SplitMix64::new(master);
    let mut failures = Vec::new();
    let mut totals = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..scenarios {
        let seed = master_rng.next_u64();
        match run_scenario(seed) {
            Ok(first) => {
                // Replay criterion: the same seed is byte-identical.
                match run_scenario(seed) {
                    Ok(second) if second.recording == first.recording => {
                        totals.0 += first.orders_submitted;
                        totals.1 += first.proposals_emitted;
                        totals.2 += first.cognition_failures;
                        totals.3 += first.beliefs_drafted;
                    }
                    Ok(_) => failures.push((seed, "recording differs on replay".to_string())),
                    Err(e) => failures.push((seed, format!("replay errored: {e}"))),
                }
            }
            Err(e) => failures.push((seed, e)),
        }
    }

    println!(
        "[synthesis-dst] totals: {} orders, {} proposals, {} cognition failures, {} beliefs",
        totals.0, totals.1, totals.2, totals.3
    );
    assert!(
        failures.is_empty(),
        "[synthesis-dst] master {master}: {} failing seed(s): {:?}",
        failures.len(),
        failures
    );
    // The chaos distribution must actually exercise the failure plane —
    // a battery that never failed cognition proved nothing.
    assert!(
        totals.2 > 0,
        "chaos mind never failed across {scenarios} scenarios (distribution bug)"
    );
}
