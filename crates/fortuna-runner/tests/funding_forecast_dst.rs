//! Seeded DST over the `funding_forecast` belief-producer (perp-strategies-
//! and-scalar-claims §2.2/§7; GAPS R1). Drives the strategy from seeds with
//! random PerpTick sequences: time gaps, window rolls, estimate oscillation,
//! and ±clamp extremes — with per-arm hit accounting (a battery that never
//! fired an arm fails the coverage floor).
//!
//! Per-seed invariants:
//!   1. NO PANIC: no PerpTick (any estimate, any window position) ever panics
//!      the strategy. `on_event` is total.
//!   2. ZERO PROPOSALS: a zero-capital belief-producer NEVER returns a
//!      Proposal, across every tick of every scenario (I6 vacuously; design
//!      §2.2).
//!   3. EVERY DRAFT VALIDATES: every emitted ScalarBeliefDraft's predictive
//!      `validate()`s (a well-formed quantile fan: q strictly increasing, v
//!      non-decreasing, finite, unit non-empty) — even at the ±clamp edge
//!      where the symmetric band can collapse.
//!   4. ONE-BELIEF-PER-TICK: exactly one draft per PerpTick (the producer is
//!      deterministic and emits on every tick).
//!   5. DETERMINISM: re-running a scenario from the same seed produces a
//!      byte-identical digest of all emitted drafts.
//!
//! Conventions follow the other DST harnesses (crates/fortuna-state/tests/
//! perp_dst.rs, crates/fortuna-runner/tests/synthesis_dst.rs): master seed
//! from DST_MASTER_SEED or wall clock (printed), per-scenario seeds via
//! SplitMix64, scenario count via FUNDING_FORECAST_DST_SCENARIOS (default 20),
//! failures print their seed, and the regression corpus in the canonical
//! crates/fortuna-core/dst-corpus/ replays first (tagged `harness:
//! funding-forecast-dst`).

use fortuna_cognition::scalar_beliefs::ScalarBeliefDraft;
use fortuna_cognition::scoring::PredictiveDistribution;
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::SplitMix64;
use fortuna_core::market::{MarketId, VenueId};
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::funding_forecast::FundingForecast;
use fortuna_runner::{CoreHandle, Strategy};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

const MARKET: &str = "KXBTCPERP1";
/// 8-hour funding windows start on the 04/12/20 UTC grid (research §4). The
/// scenario base; window rolls advance by 8h.
const WINDOW_MS: i64 = 8 * 3_600_000;

/// Per-arm hit counts (coverage floor mirrors perp_dst/settlement_dst).
#[derive(Default)]
struct ArmCounts(BTreeMap<&'static str, u64>);

impl ArmCounts {
    fn hit(&mut self, arm: &'static str) {
        *self.0.entry(arm).or_default() += 1;
    }
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

/// A seed-derived estimate in a wide range that includes both the in-band and
/// the ±clamp-exceeding regimes. Returned as a `Decimal` with bounded scale so
/// the value is exactly representable (the venue-payload rate domain).
fn seeded_estimate(rng: &mut SplitMix64, arms: &mut ArmCounts) -> Decimal {
    // 5 regimes, equally likely.
    match rng.next_u64() % 5 {
        0 => {
            // Sub-zero-threshold (|rate| < 0.01%) -> finalizes to 0.
            arms.hit("near_zero");
            let micro = (rng.next_u64() % 99) as i64; // 0..98 of 1e-6
            Decimal::new(micro, 6) // up to 0.000098 < 0.0001
        }
        1 => {
            // Normal in-band negative.
            arms.hit("in_band");
            let v = -((rng.next_u64() % 199 + 1) as i64); // -1..-199 of 1e-4
            Decimal::new(v, 4)
        }
        2 => {
            // Normal in-band positive.
            arms.hit("in_band");
            let v = (rng.next_u64() % 199 + 1) as i64; // 1..199 of 1e-4
            Decimal::new(v, 4)
        }
        3 => {
            // Above the +2% clamp -> finalize pins to +0.02.
            arms.hit("clamp_extreme");
            let over = (rng.next_u64() % 500 + 201) as i64; // 2.01%..7.00%
            Decimal::new(over, 4)
        }
        _ => {
            // Below the -2% clamp -> finalize pins to -0.02.
            arms.hit("clamp_extreme");
            let over = -((rng.next_u64() % 500 + 201) as i64);
            Decimal::new(over, 4)
        }
    }
}

/// A seed-derived per-scenario digest of every emitted draft (determinism
/// anchor, invariant 5).
fn run_scenario(seed: u64, arms: &mut ArmCounts) -> Result<String, String> {
    let mut rng = SplitMix64::new(seed);
    let mut strat = FundingForecast::new().map_err(|e| format!("ctor: {e}"))?;

    let books = BTreeMap::new();
    let markets = BTreeMap::new();
    let fees = fee_model();

    // Base window: a 04/12/20 grid finalization. Walk a window cursor forward.
    let base = UtcTimestamp::parse_iso8601("2026-06-12T20:00:00.000Z")
        .map_err(|e| format!("base ts: {e}"))?;
    let mut next_funding = base;
    // observation cursor starts one window before the first finalization.
    let mut obs_at = base
        .checked_add_millis(-WINDOW_MS)
        .map_err(|e| format!("obs base: {e}"))?;

    let n_ticks = 4 + (rng.next_u64() % 28) as usize; // 4..=31 ticks
    let mut emitted: Vec<ScalarBeliefDraft> = Vec::new();

    for _ in 0..n_ticks {
        // --- advance the observation time by a random gap (minutes) ---
        let gap_min = match rng.next_u64() % 4 {
            0 => 1,
            1 => 5,
            2 => 30,
            _ => 1 + (rng.next_u64() % 120) as i64, // up to a 2h gap
        };
        if gap_min > 1 {
            arms.hit("time_gap");
        }
        obs_at = obs_at
            .checked_add_millis(gap_min * 60_000)
            .map_err(|e| format!("obs advance: {e}"))?;

        // --- maybe roll the window (a new next_funding_time) ---
        if rng.next_u64().is_multiple_of(5) {
            arms.hit("window_roll");
            next_funding = next_funding
                .checked_add_millis(WINDOW_MS)
                .map_err(|e| format!("window roll: {e}"))?;
        }
        // If the observation cursor passed the current finalization, advance the
        // window so next_funding_time stays in the future (the normal lifecycle)
        // — but DST also deliberately exercises the past-due (collapsed) path,
        // so only roll forward part of the time.
        while obs_at.epoch_millis() > next_funding.epoch_millis()
            && rng.next_u64().is_multiple_of(2)
        {
            next_funding = next_funding
                .checked_add_millis(WINDOW_MS)
                .map_err(|e| format!("window catch-up: {e}"))?;
        }
        if obs_at.epoch_millis() > next_funding.epoch_millis() {
            arms.hit("past_due");
        }

        // --- a seed-derived estimate (oscillating across regimes) ---
        let estimate = seeded_estimate(&mut rng, arms);

        let core = CoreHandle {
            now: obs_at,
            books: &books,
            markets: &markets,
            fee_model: &fees,
        };
        let ev = BusEvent {
            seq: 1,
            at: obs_at,
            origin: EventOrigin::External,
            payload: EventPayload::PerpTick {
                venue: VenueId::new("kinetics").map_err(|e| format!("venue: {e}"))?,
                market: MarketId::new(MARKET).map_err(|e| format!("market: {e}"))?,
                marks: PerpMarks {
                    venue_settlement: PerpPrice::from_dollars_exact(
                        Decimal::from_str("6.3332").map_err(|e| format!("mark: {e}"))?,
                    )
                    .map_err(|e| format!("mark px: {e}"))?,
                    conservative: None,
                },
                funding: FundingObservation {
                    estimate,
                    next_funding_time: next_funding,
                    reference_price: PerpPrice::from_dollars_exact(
                        Decimal::from_str("6.3000").map_err(|e| format!("ref: {e}"))?,
                    )
                    .map_err(|e| format!("ref px: {e}"))?,
                    obs_at,
                },
            },
        };

        // Invariant 1 (no panic) is enforced by the harness running at all;
        // invariant 2 (zero proposals) is asserted here.
        let proposals = futures::executor::block_on(strat.on_event(&ev, &core))
            .map_err(|e| format!("on_event: {e}"))?;
        if !proposals.is_empty() {
            return Err(format!(
                "ZERO-CAPITAL VIOLATED: {} proposal(s) emitted",
                proposals.len()
            ));
        }

        // Invariant 4: exactly one belief per tick.
        let drafts = strat.drain_scalar_beliefs();
        if drafts.len() != 1 {
            return Err(format!("expected 1 draft per tick, got {}", drafts.len()));
        }
        for d in &drafts {
            // Invariant 3: every emitted draft validates.
            d.predictive
                .validate()
                .map_err(|e| format!("emitted draft failed validate(): {e}"))?;
            // It is a Scalar fan (the only thing a scalar producer emits).
            if !matches!(d.predictive, PredictiveDistribution::Scalar { .. }) {
                return Err("emitted a non-Scalar predictive".to_string());
            }
        }
        emitted.extend(drafts);
    }

    // The digest: the JSON of every emitted draft, in order (invariant 5).
    let digest = serde_json::to_string(&emitted).map_err(|e| format!("digest serialize: {e}"))?;
    Ok(digest)
}

#[test]
fn funding_forecast_survives_seeded_chaos() {
    let scenarios: u64 = std::env::var("FUNDING_FORECAST_DST_SCENARIOS")
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
    println!("[funding-forecast-dst] master seed {master} -> {scenarios} scenario(s)");

    let mut master_rng = SplitMix64::new(master);
    let mut arms = ArmCounts::default();
    let mut failures: Vec<(u64, String)> = Vec::new();
    for _ in 0..scenarios {
        let seed = master_rng.next_u64();
        match run_scenario(seed, &mut arms) {
            Ok(first) => {
                // Invariant 5: same seed, same digest.
                let mut rerun_arms = ArmCounts::default();
                match run_scenario(seed, &mut rerun_arms) {
                    Ok(second) => {
                        if first != second {
                            failures.push((seed, "non-deterministic digest".to_string()));
                        }
                    }
                    Err(e) => failures.push((seed, format!("rerun failed: {e}"))),
                }
            }
            Err(e) => failures.push((seed, e)),
        }
    }

    println!("[funding-forecast-dst] arms {:?}", arms.0);
    assert!(
        failures.is_empty(),
        "[funding-forecast-dst] master {master}: {} failing seed(s): {:?}",
        failures.len(),
        failures
    );
    // Coverage floor: small draws can miss rare arms; at 100+ scenarios every
    // arm must fire (the chaos actually exercised each regime).
    if scenarios >= 100 {
        for arm in [
            "near_zero",
            "in_band",
            "clamp_extreme",
            "time_gap",
            "window_roll",
        ] {
            assert!(
                arms.0.get(arm).copied().unwrap_or(0) > 0,
                "[funding-forecast-dst] master {master}: arm {arm} never fired across {scenarios} scenarios"
            );
        }
    }
}

/// Regression corpus (definition-of-done #3): funding-forecast-tagged seeds in
/// the canonical crates/fortuna-core/dst-corpus/ directory. NEVER delete corpus
/// seeds. Selected for THIS harness by a `# harness: funding-forecast-dst` line.
fn load_corpus() -> Vec<(u64, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fortuna-core/dst-corpus");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "seed"))
        .collect();
    paths.sort();
    for path in paths {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !content.contains("harness: funding-forecast-dst") {
            continue;
        }
        let label = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Ok(seed) = line.parse::<u64>() {
                out.push((seed, label.clone()));
            }
        }
    }
    out
}

#[test]
fn regression_corpus_replays_green() {
    // The corpus MAY be empty for this harness (no red seed has been found —
    // the discipline is in place; an empty corpus is not a failure, matching
    // the GAPS "regression-seed corpus is empty" disclosed Minor). Any tagged
    // seed present must replay green + deterministically.
    let corpus = load_corpus();
    let mut seen = BTreeSet::new();
    for (seed, label) in corpus {
        if !seen.insert(seed) {
            continue;
        }
        let mut arms = ArmCounts::default();
        let first = run_scenario(seed, &mut arms)
            .unwrap_or_else(|e| panic!("corpus {label} (seed {seed}): {e}"));
        let mut rerun_arms = ArmCounts::default();
        let second = run_scenario(seed, &mut rerun_arms)
            .unwrap_or_else(|e| panic!("corpus {label} (seed {seed}) rerun: {e}"));
        assert_eq!(
            first, second,
            "corpus {label} (seed {seed}): non-deterministic digest"
        );
    }
}
