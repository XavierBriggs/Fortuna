//! Seeded DST over the `perp_event_basis` propose-only basis strategy (perp-
//! strategies-and-scalar-claims §3/§7; GAPS "TRACK C — slice 3b"). Drives the
//! strategy from seeds with random PerpTicks (varying perp mark) + book
//! perturbations across a FIXED catalog, with per-arm hit accounting (a battery
//! that never fires an arm fails the coverage floor).
//!
//! Per-seed invariants:
//!   1. NO PANIC: no PerpTick (any mark, any book state) ever panics the
//!      strategy. `on_event` is total.
//!   2. STRUCTURAL UNSIZED (I6): every emitted proposal is exactly one
//!      `Passive`/`Buy`/`Yes` leg with no quantity (the type has no qty field —
//!      this is asserted structurally by the shape it can take).
//!   3. PROPOSALS ONLY WHEN TRADEABLE: a proposal appears ONLY when the basis
//!      cleared the fee-trap AND the perp mark fell in a bin with a joinable
//!      bid. The harness independently recomputes the basis verdict and refuses
//!      any proposal emitted on a non-tradeable signal.
//!   4. DETERMINISM: re-running a scenario from the same seed produces a
//!      byte-identical digest of all emitted proposals.
//!
//! Conventions follow funding_forecast_dst / perp_dst: master seed from
//! DST_MASTER_SEED or wall clock (printed), per-scenario seeds via SplitMix64,
//! scenario count via PERP_EVENT_BASIS_DST_SCENARIOS (default 20), failures
//! print their seed, and the canonical crates/fortuna-core/dst-corpus/ replays
//! first (tagged `harness: perp-event-basis-dst`).

use fortuna_cognition::basis::{compute_basis, BracketBin, BracketStrike};
use fortuna_core::book::{OrderBook, PriceLevel};
use fortuna_core::bus::{BusEvent, EventOrigin, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::SplitMix64;
use fortuna_core::market::{Action, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
use fortuna_runner::perp_event_basis::{PerpEventBasis, PerpEventBasisConfig};
use fortuna_runner::{CoreHandle, Proposal, Strategy, Urgency};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use rust_decimal::prelude::ToPrimitive; // Decimal::to_f64 (mirror the strategy's mark round-trip)
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

const PERP: &str = "KXBTCPERP";
/// fee_floor + min_basis combined (must clear, strict >) — matches the cfg.
const FEE_FLOOR: f64 = 10.0;
const MIN_BASIS: f64 = 5.0;
const PREMIUM: i64 = 2;

/// The fixed catalog the DST perturbs books over: a `less` bottom tail (≤ $60k),
/// six `between` $1,000 bins spanning $60k–$66k, and a `greater` top tail
/// (≥ $66k). The between bins partition $60,000–66,000 contiguously, so a perp
/// mark in that span always lands in a between bin; below/above lands in a tail.
fn catalog() -> Vec<(MarketId, BracketStrike)> {
    let mut v = vec![(mkt("KXBTC-LESS60K"), BracketStrike::Less { cap: 60_000.0 })];
    let mut floor = 60_000.0;
    for i in 0..6 {
        let cap = floor + 1_000.0;
        v.push((
            mkt(&format!("KXBTC-B{i}")),
            BracketStrike::Between { floor, cap },
        ));
        floor = cap;
    }
    v.push((
        mkt("KXBTC-GT66K"),
        BracketStrike::Greater { floor: 66_000.0 },
    ));
    v
}

fn mkt(s: &str) -> MarketId {
    MarketId::new(s).expect("market id")
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .expect("fee schedule toml");
    ScheduleFeeModel::new(vec![s]).expect("fee model")
}

/// Per-arm hit counts (coverage floor mirrors perp_dst/funding_forecast_dst).
#[derive(Default)]
struct ArmCounts(BTreeMap<&'static str, u64>);

impl ArmCounts {
    fn hit(&mut self, arm: &'static str) {
        *self.0.entry(arm).or_default() += 1;
    }
}

/// One YES level pair as a book (prices in cents). `bid == 0` ⇒ bid-empty (an
/// illiquid bin: no joinable bid, but still `ask/2` implied mass per `bin_prob`).
/// Always keeps bid < ask and prices in (0,100) so `OrderBook::validate` accepts.
fn book(market: &MarketId, bid: i64, ask: i64) -> OrderBook {
    let as_of = UtcTimestamp::from_epoch_millis(0).expect("ts");
    let yes_bids = if bid <= 0 {
        vec![]
    } else {
        vec![PriceLevel {
            price: Cents::new(bid),
            qty: Contracts::new(100),
        }]
    };
    OrderBook {
        market: market.clone(),
        as_of,
        yes_bids,
        yes_asks: vec![PriceLevel {
            price: Cents::new(ask.clamp(bid + 1, 99)),
            qty: Contracts::new(100),
        }],
    }
}

/// A seed-derived (bid, ask) for one bin. Regimes: illiquid (bid 0), thin,
/// rich. The ask stays a couple cents above the bid.
fn seeded_quote(rng: &mut SplitMix64) -> (i64, i64) {
    match rng.next_u64() % 4 {
        0 => (0, 2),                                 // illiquid: bid-empty, ask/2 mass
        1 => (1 + (rng.next_u64() % 8) as i64, 0),   // thin (1..8)
        2 => (40 + (rng.next_u64() % 20) as i64, 0), // mid (40..59)
        _ => (88 + (rng.next_u64() % 10) as i64, 0), // rich (88..97)
    }
}

/// Build the books for one tick: each catalog market gets a seed-derived quote.
/// Returns the books AND the parallel `BracketBin`s (mass) the kernel would see,
/// so the harness can recompute the verdict independently.
fn seeded_books(
    cat: &[(MarketId, BracketStrike)],
    rng: &mut SplitMix64,
    arms: &mut ArmCounts,
) -> (BTreeMap<MarketId, OrderBook>, Vec<BracketBin>) {
    let mut books = BTreeMap::new();
    let mut bins = Vec::new();
    for (id, strike) in cat {
        let (mut bid, ask_seed) = seeded_quote(rng);
        // ask sits just above bid (or a fixed 2 for the illiquid case).
        let ask = if bid <= 0 {
            2
        } else {
            (bid + 1 + (ask_seed % 2)).min(99)
        };
        if bid > 0 {
            arms.hit("liquid_bin");
        } else {
            arms.hit("illiquid_bin");
            bid = 0;
        }
        // Mirror the strategy's `bin_prob` EXACTLY: an absent bid counts as the
        // 0c floor, so a bid-empty (illiquid) bin still carries `ask/2` implied
        // mass — NOT zero. (The only zero-mass shape is no quote on EITHER side,
        // which this builder never produces — every bin has an ask.) Keeping
        // this oracle in lockstep with `bin_prob` is what makes its independent
        // verdict match the strategy; the prior `bid<=0 ⇒ 0.0` rule diverged
        // once `bin_prob` was corrected to value the ask-only low tail.
        let prob = ((bid.max(0) + ask) as f64 / 2.0) / 100.0;
        bins.push(BracketBin {
            kind: *strike,
            prob,
        });
        books.insert(id.clone(), book(id, bid, ask));
    }
    (books, bins)
}

/// A seed-derived perp per-contract mark in dollars (BTC/10000). The BTC value
/// spans $58k–$68k so it lands below the bottom tail, inside the between span,
/// or above the top tail across draws. Returned as a whole-tick Decimal string.
fn seeded_perp(rng: &mut SplitMix64, arms: &mut ArmCounts) -> (Decimal, f64) {
    // BTC dollars in [58_000, 68_000), to whole dollars (per-contract = /10000,
    // exact to 4dp).
    let btc = 58_000 + (rng.next_u64() % 10_000) as i64;
    if btc < 60_000 {
        arms.hit("mark_low_tail");
    } else if btc >= 66_000 {
        arms.hit("mark_high_tail");
    } else {
        arms.hit("mark_in_between");
    }
    let per_contract = Decimal::new(btc, 4); // btc/10000, exact
                                             // The oracle MUST feed compute_basis the SAME f64 mark the STRATEGY does, or the
                                             // two straddle the strict fee-trap `>` at the boundary (signed_basis == fee+margin)
                                             // and disagree. The strategy reads the mark as a PerpPrice and converts
                                             // per_contract -> f64 -> ×10000 (a LOSSY round-trip); `btc as f64` (the exact int)
                                             // diverges by a sub-ulp ε. Mirror the strategy's path EXACTLY so the independent
                                             // verdict uses the strategy's actual mark. (Regression: perp-event-basis-fee-trap-boundary.seed.)
    let perp_btc = per_contract.to_f64().unwrap_or(btc as f64) * 10_000.0; // = strategy's PERP_CONTRACT_BTC_DIVISOR
    (per_contract, perp_btc)
}

/// Run one seeded scenario; return a determinism digest of every emitted
/// proposal. Enforces invariants 2/3 inline (and 1 by running at all).
fn run_scenario(seed: u64, arms: &mut ArmCounts) -> Result<String, String> {
    let mut rng = SplitMix64::new(seed);
    let cat = catalog();
    let ladder: BTreeMap<MarketId, BracketStrike> = cat.iter().cloned().collect();
    let cfg = PerpEventBasisConfig {
        perp_market: mkt(PERP),
        ladder,
        fee_floor_dollars: FEE_FLOOR,
        min_basis_dollars: MIN_BASIS,
        edge_premium_cents: PREMIUM,
    };
    let mut strat = PerpEventBasis::new(cfg).map_err(|e| format!("ctor: {e}"))?;
    let fees = fee_model();

    let n_ticks = 4 + (rng.next_u64() % 28) as usize; // 4..=31 ticks
    let mut emitted: Vec<Proposal> = Vec::new();

    for _ in 0..n_ticks {
        let (books, bins) = seeded_books(&cat, &mut rng, arms);
        let (per_contract, perp_btc) = seeded_perp(&mut rng, arms);

        // Independent verdict: what the kernel says about THIS (bins, mark).
        let sig = compute_basis(&bins, perp_btc, FEE_FLOOR, MIN_BASIS);
        let tradeable = sig.map(|s| s.is_tradeable).unwrap_or(false);
        match sig {
            None => arms.hit("none_median"),
            Some(s) if s.is_tradeable => arms.hit("tradeable"),
            Some(_) => arms.hit("not_tradeable"),
        }

        let core = CoreHandle {
            now: UtcTimestamp::from_epoch_millis(0).map_err(|e| format!("now: {e}"))?,
            books: &books,
            markets: &BTreeMap::new(),
            fee_model: &fees,
        };
        let ev = BusEvent {
            seq: 1,
            at: UtcTimestamp::from_epoch_millis(0).map_err(|e| format!("at: {e}"))?,
            origin: EventOrigin::External,
            payload: EventPayload::PerpTick {
                venue: VenueId::new("kinetics").map_err(|e| format!("venue: {e}"))?,
                market: mkt(PERP),
                marks: PerpMarks {
                    venue_settlement: PerpPrice::from_dollars_exact(per_contract)
                        .map_err(|e| format!("mark px: {e}"))?,
                    conservative: None,
                },
                funding: FundingObservation {
                    estimate: Decimal::from_str("0.0005").map_err(|e| format!("est: {e}"))?,
                    next_funding_time: UtcTimestamp::from_epoch_millis(8 * 3_600_000)
                        .map_err(|e| format!("nft: {e}"))?,
                    reference_price: PerpPrice::from_dollars_exact(
                        Decimal::from_str("6.0000").map_err(|e| format!("ref: {e}"))?,
                    )
                    .map_err(|e| format!("ref px: {e}"))?,
                    obs_at: UtcTimestamp::from_epoch_millis(0).map_err(|e| format!("obs: {e}"))?,
                },
            },
        };

        // Invariant 1 (no panic) holds by running. Collect proposals.
        let proposals = futures::executor::block_on(strat.on_event(&ev, &core))
            .map_err(|e| format!("on_event: {e}"))?;

        for p in &proposals {
            // Invariant 3: a proposal ONLY when the kernel said tradeable.
            if !tradeable {
                return Err(format!(
                    "PROPOSAL ON NON-TRADEABLE BASIS: mark {perp_btc}, oracle sig {sig:?}; \
                     strategy thesis: {}",
                    p.thesis
                ));
            }
            // Invariant 2 (structural unsized): exactly one Passive/Buy/Yes leg.
            // ProposedLeg has NO qty field — the shape itself proves unsized; we
            // assert the rest of the shape here.
            if p.urgency != Urgency::Passive {
                return Err("a basis proposal must be Passive (maker-only)".to_string());
            }
            if p.group_policy.is_some() {
                return Err("a single-leg proposal must have no group_policy".to_string());
            }
            if p.legs.len() != 1 {
                return Err(format!("expected one leg, got {}", p.legs.len()));
            }
            let leg = &p.legs[0];
            if leg.side != Side::Yes || leg.action != Action::Buy {
                return Err("a basis leg must be Buy/Yes".to_string());
            }
            if leg.calibrated_p.is_some() {
                return Err("a mechanical leg must carry no calibrated_p".to_string());
            }
            // The leg targets a catalog market and joins a real bid (limit <
            // fair ≤ 99), and the limit equals that market's best bid.
            let target_book = books
                .get(&leg.market)
                .ok_or_else(|| format!("leg market {} not in books", leg.market))?;
            let best_bid = target_book
                .yes_bids
                .first()
                .ok_or_else(|| "leg joined a bid-empty book".to_string())?;
            if leg.limit_price != best_bid.price {
                return Err(format!(
                    "limit {} != target best bid {}",
                    leg.limit_price, best_bid.price
                ));
            }
            if leg.fair_value.raw() <= leg.limit_price.raw() {
                return Err("fair must exceed the limit (an edge claim)".to_string());
            }
            if leg.fair_value.raw() > 99 {
                return Err("fair must be clamped ≤ 99".to_string());
            }
        }
        // At most one proposal per tick (one target bin).
        if proposals.len() > 1 {
            return Err(format!(
                "expected ≤1 proposal per tick, got {}",
                proposals.len()
            ));
        }
        emitted.extend(proposals);
    }

    // Digest the load-bearing proposal fields, in order (invariant 4). Proposal
    // is not Serialize; build a stable string.
    let mut digest = String::new();
    for p in &emitted {
        for leg in &p.legs {
            digest.push_str(&format!(
                "{}|{:?}|{:?}|{}|{}|{:?};",
                leg.market, leg.side, leg.action, leg.limit_price, leg.fair_value, leg.calibrated_p
            ));
        }
        digest.push_str(&format!("[{:?}]\n", p.urgency));
    }
    Ok(digest)
}

#[test]
fn perp_event_basis_survives_seeded_chaos() {
    let scenarios: u64 = std::env::var("PERP_EVENT_BASIS_DST_SCENARIOS")
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
    println!("[perp-event-basis-dst] master seed {master} -> {scenarios} scenario(s)");

    // The tagged corpus replays first (definition-of-done #3).
    let mut failures: Vec<(u64, String)> = Vec::new();
    let mut corpus_arms = ArmCounts::default();
    let mut seen = BTreeSet::new();
    for (seed, label) in load_corpus() {
        if !seen.insert(seed) {
            continue;
        }
        match run_scenario(seed, &mut corpus_arms) {
            Ok(first) => match run_scenario(seed, &mut ArmCounts::default()) {
                Ok(second) if first == second => {}
                Ok(_) => failures.push((seed, format!("corpus {label}: non-deterministic"))),
                Err(e) => failures.push((seed, format!("corpus {label} rerun: {e}"))),
            },
            Err(e) => failures.push((seed, format!("corpus {label}: {e}"))),
        }
    }

    let mut master_rng = SplitMix64::new(master);
    let mut arms = ArmCounts::default();
    for _ in 0..scenarios {
        let seed = master_rng.next_u64();
        match run_scenario(seed, &mut arms) {
            Ok(first) => {
                // Invariant 4: same seed → same digest.
                match run_scenario(seed, &mut ArmCounts::default()) {
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

    println!("[perp-event-basis-dst] arms {:?}", arms.0);
    assert!(
        failures.is_empty(),
        "[perp-event-basis-dst] master {master}: {} failing seed(s): {:?}",
        failures.len(),
        failures
    );
    // Coverage floor: at 100+ scenarios every regime must have fired (the chaos
    // actually exercised tradeable/up/down, the None-median path, and the
    // no-joinable-bid path).
    if scenarios >= 100 {
        for arm in [
            "tradeable",
            "not_tradeable",
            "none_median",
            "mark_low_tail",
            "mark_high_tail",
            "mark_in_between",
            "illiquid_bin",
            "liquid_bin",
        ] {
            assert!(
                arms.0.get(arm).copied().unwrap_or(0) > 0,
                "[perp-event-basis-dst] master {master}: arm {arm} never fired across {scenarios} scenarios"
            );
        }
    }
}

/// Regression corpus loader (definition-of-done #3): seeds tagged
/// `# harness: perp-event-basis-dst` in the canonical dst-corpus directory.
/// NEVER delete corpus seeds. An empty corpus is not a failure (no red seed yet).
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
        if !content.contains("harness: perp-event-basis-dst") {
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
    let corpus = load_corpus();
    let mut seen = BTreeSet::new();
    for (seed, label) in corpus {
        if !seen.insert(seed) {
            continue;
        }
        let mut arms = ArmCounts::default();
        let first = run_scenario(seed, &mut arms)
            .unwrap_or_else(|e| panic!("corpus {label} (seed {seed}): {e}"));
        let second = run_scenario(seed, &mut ArmCounts::default())
            .unwrap_or_else(|e| panic!("corpus {label} (seed {seed}) rerun: {e}"));
        assert_eq!(
            first, second,
            "corpus {label} (seed {seed}): non-deterministic digest"
        );
    }
}
