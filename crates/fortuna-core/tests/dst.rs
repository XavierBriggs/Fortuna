//! Deterministic simulation testing harness (T0.4). Spec 5.1, PROMPT doctrine.
//!
//! Contract (scripts/run-dst.sh):
//!   dst [--nocapture] --seeds N        # corpus first, then N random seeds
//!   dst --replay-seed S                # one seed, verbose trace
//!
//! Every scenario: build a seeded world (sim clock, sim venue with a random
//! fault config, random markets/books), drive a seeded actor through a random
//! action stream (places, cancels, ticks, clock advances, public flow, fill
//! polls with crash-amnesia, book resets, settles, outages), then quiesce and
//! assert the cross-cutting invariants:
//!
//!   I-money     venue cash == initial - sum(buy gross+fee) + sum(sell
//!               gross-fee) + sum(settle payouts), derived from the DEDUP'D
//!               fill stream (at-least-once delivery must reconcile exactly).
//!   I-reserve   after cancel-all + flush, reserved == 0 (no leaked
//!               reservations) and no negative cash ever.
//!   I-position  dedup'd fill-derived positions == venue positions
//!               (settled markets absent).
//!   I-delivery  every fill the venue recorded is eventually delivered
//!               (cursor mechanics lose nothing).
//!   I-determinism  running the same seed twice produces byte-identical
//!               traces.
//!
//! A violation prints the seed and fails the run (exit 1). Reproduce with
//!   scripts/replay.sh --seed <S>
//! Minimize per dst-corpus/README.md, then commit the seed file.

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::SplitMix64;
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, VenueId};
use fortuna_core::money::Cents;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, PlaceOrder, SimVenue};
use fortuna_venues::{
    Cursor, Fill, Market, MarketStatus, PriceLevel, SettlementMeta, Venue, VenueError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::panic::AssertUnwindSafe;
use std::process::ExitCode;
use std::sync::Arc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut seeds: u64 = 2000;
    let mut replay: Option<u64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seeds" => {
                i += 1;
                seeds = match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(n) => n,
                    None => return usage(),
                };
            }
            "--replay-seed" => {
                i += 1;
                replay = match args.get(i).and_then(|s| s.parse().ok()) {
                    Some(n) => Some(n),
                    None => return usage(),
                };
            }
            "--nocapture" => {} // libtest compat; we always print
            other => {
                eprintln!("dst: unknown arg {other:?}");
                return usage();
            }
        }
        i += 1;
    }

    if let Some(seed) = replay {
        println!("[dst] replaying seed {seed} (verbose)");
        return match run_scenario(seed, true) {
            Ok(()) => {
                println!("[dst] seed {seed}: OK");
                ExitCode::SUCCESS
            }
            Err(msg) => {
                eprintln!("[dst] seed {seed}: FAIL\n{msg}");
                ExitCode::from(1)
            }
        };
    }

    let mut failures: Vec<(u64, String)> = Vec::new();
    let corpus = load_corpus();
    println!("[dst] regression corpus: {} seed(s)", corpus.len());
    for (seed, label) in &corpus {
        run_one(*seed, &format!("corpus ({label})"), &mut failures);
    }

    // Fresh randomized seeds. Master entropy comes from the real clock (the
    // one legal wall-time source) unless pinned via DST_MASTER_SEED.
    let master = std::env::var("DST_MASTER_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            use fortuna_core::clock::RealClock;
            RealClock.now().epoch_millis() as u64
        });
    println!("[dst] master seed {master} -> {seeds} random scenario(s)");
    let mut master_rng = SplitMix64::new(master);
    for _ in 0..seeds {
        let seed = master_rng.next_u64();
        run_one(seed, "random", &mut failures);
    }

    if failures.is_empty() {
        println!(
            "[dst] OK: {} corpus + {} random seeds, zero invariant violations",
            corpus.len(),
            seeds
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("[dst] {} FAILURE(S):", failures.len());
        for (seed, msg) in &failures {
            eprintln!("  seed {seed}: {}", msg.lines().next().unwrap_or(""));
        }
        eprintln!(
            "[dst] reproduce: scripts/replay.sh --seed <S>; minimize per dst-corpus/README.md"
        );
        ExitCode::from(1)
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: dst [--seeds N] [--replay-seed S]");
    ExitCode::from(2)
}

fn run_one(seed: u64, kind: &str, failures: &mut Vec<(u64, String)>) {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| run_scenario(seed, false)));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(msg)) => {
            eprintln!("[dst] FAIL seed {seed} ({kind}): {msg}");
            failures.push((seed, msg));
        }
        Err(panic) => {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic (no message)".to_string());
            eprintln!("[dst] PANIC seed {seed} ({kind}): {msg}");
            failures.push((seed, format!("panic: {msg}")));
        }
    }
}

/// Corpus format: one file per seed in dst-corpus/, lines starting with '#'
/// describe the failure mode; exactly one non-comment line holds the seed.
fn load_corpus() -> Vec<(u64, String)> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/dst-corpus");
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

// ---- scenario ----

const START_MS: i64 = 1_780_000_000_000; // 2026-06-08T16:26:40Z; arbitrary fixed epoch

/// Run a scenario twice and byte-compare traces (I-determinism), returning
/// the first run's invariant verdict.
fn run_scenario(seed: u64, verbose: bool) -> Result<(), String> {
    let trace1 = run_world(seed, verbose)?;
    let trace2 = run_world(seed, false)?;
    if trace1 != trace2 {
        let divergence = trace1
            .lines()
            .zip(trace2.lines())
            .position(|(a, b)| a != b)
            .map(|i| format!("first divergent line {i}"))
            .unwrap_or_else(|| "length mismatch".to_string());
        return Err(format!(
            "I-determinism violated: same seed produced different traces ({divergence})"
        ));
    }
    Ok(())
}

struct ActorState {
    cursor: Cursor,
    checkpoint_cursor: Cursor,
    seen: BTreeMap<String, Fill>, // dedup by fill_id
    known_orders: Vec<fortuna_core::market::VenueOrderId>,
    payouts: Cents,
}

fn run_world(seed: u64, verbose: bool) -> Result<String, String> {
    let mut rng = SplitMix64::new(seed);
    let mut trace = String::new();
    let log = |line: String, trace: &mut String| {
        if verbose {
            println!("  {line}");
        }
        trace.push_str(&line);
        trace.push('\n');
    };

    // World setup.
    let clock = Arc::new(SimClock::new(
        UtcTimestamp::from_epoch_millis(START_MS).map_err(|e| e.to_string())?,
    ));
    let faults = random_faults(&mut rng, seed);
    let initial_cash = Cents::new(50_000 + (rng.next_u64() % 50_000) as i64);
    let venue = SimVenue::new(
        VenueId::new("sim").map_err(|e| e.to_string())?,
        clock.clone(),
        kalshi_style_fees(),
        faults.clone(),
        initial_cash,
    );
    let n_markets = 1 + (rng.next_u64() % 3) as usize;
    let mut markets = Vec::new();
    for m in 0..n_markets {
        let id = MarketId::new(format!("DST-MKT-{m}")).map_err(|e| e.to_string())?;
        venue.add_market(Market {
            id: id.clone(),
            venue: VenueId::new("sim").map_err(|e| e.to_string())?,
            title: format!("DST market {m}"),
            category: "dst".to_string(),
            status: MarketStatus::Trading,
            close_at: None,
            settlement: SettlementMeta {
                oracle_type: "dst".into(),
                resolution_source: "dst".into(),
                expected_lag_hours: 0,
            },
            payout_per_contract: Cents::new(100),
        });
        let (bids, asks) = random_book(&mut rng);
        venue
            .set_book(&id, bids, asks)
            .map_err(|e| format!("setup book: {e}"))?;
        markets.push(id);
    }
    log(
        format!("world seed={seed} markets={n_markets} cash={initial_cash} faults={faults:?}"),
        &mut trace,
    );

    let mut actor = ActorState {
        cursor: Cursor::start(),
        checkpoint_cursor: Cursor::start(),
        seen: BTreeMap::new(),
        known_orders: Vec::new(),
        payouts: Cents::ZERO,
    };
    let mut settled: BTreeSet<MarketId> = BTreeSet::new();
    let mut coid_seq = 0u64;

    // Action stream.
    let n_actions = 30 + (rng.next_u64() % 50) as usize;
    for step in 0..n_actions {
        let market = markets[(rng.next_u64() % markets.len() as u64) as usize].clone();
        match rng.next_u64() % 100 {
            // place (40%)
            0..=39 => {
                coid_seq += 1;
                let req = PlaceOrder {
                    market: market.clone(),
                    side: if rng.next_u64().is_multiple_of(2) {
                        Side::Yes
                    } else {
                        Side::No
                    },
                    action: if rng.next_u64().is_multiple_of(4) {
                        Action::Sell
                    } else {
                        Action::Buy
                    },
                    limit_price: Cents::new(1 + (rng.next_u64() % 99) as i64),
                    qty: Contracts::new(1 + (rng.next_u64() % 20) as i64),
                    client_order_id: ClientOrderId::new(format!("dst-{seed}-{coid_seq}"))
                        .map_err(|e| e.to_string())?,
                };
                let res = venue.place_raw(req.clone());
                if let Ok(id) = &res {
                    actor.known_orders.push(id.clone());
                }
                log(format!("{step} place {req:?} -> {res:?}"), &mut trace);
            }
            // cancel (12%)
            40..=51 => {
                let target = if !actor.known_orders.is_empty() && !rng.next_u64().is_multiple_of(4)
                {
                    let i = (rng.next_u64() % actor.known_orders.len() as u64) as usize;
                    actor.known_orders[i].clone()
                } else {
                    fortuna_core::market::VenueOrderId::new(format!("sim-{}", rng.next_u64() % 200))
                        .map_err(|e| e.to_string())?
                };
                let res = futures::executor::block_on(venue.cancel(&target));
                log(format!("{step} cancel {target} -> {res:?}"), &mut trace);
            }
            // poll fills (18%)
            52..=69 => {
                let res = futures::executor::block_on(venue.fills_since(actor.cursor.clone()));
                match res {
                    Ok(page) => {
                        for f in &page.fills {
                            actor.seen.insert(f.fill_id.clone(), f.clone());
                        }
                        log(
                            format!(
                                "{step} poll {} -> {} fill(s), cursor {}",
                                actor.cursor.0,
                                page.fills.len(),
                                page.next_cursor.0
                            ),
                            &mut trace,
                        );
                        actor.cursor = page.next_cursor;
                        if rng.next_u64().is_multiple_of(3) {
                            actor.checkpoint_cursor = actor.cursor.clone();
                        }
                    }
                    Err(e) => log(format!("{step} poll -> {e:?}"), &mut trace),
                }
            }
            // tick (8%)
            70..=77 => {
                let res = venue.tick();
                log(format!("{step} tick -> {res:?}"), &mut trace);
            }
            // advance clock (8%)
            78..=85 => {
                let ms = 100 + rng.next_u64() % 60_000;
                clock.advance_millis(ms).map_err(|e| e.to_string())?;
                log(format!("{step} advance {ms}ms"), &mut trace);
            }
            // inject public flow (6%)
            86..=91 => {
                let res = venue.inject_public_order(
                    &market,
                    if rng.next_u64().is_multiple_of(2) {
                        Side::Yes
                    } else {
                        Side::No
                    },
                    if rng.next_u64().is_multiple_of(2) {
                        Action::Buy
                    } else {
                        Action::Sell
                    },
                    Cents::new(1 + (rng.next_u64() % 99) as i64),
                    1 + (rng.next_u64() % 15) as i64,
                );
                log(
                    format!("{step} public -> {:?}", res.map(|fills| fills.len())),
                    &mut trace,
                );
            }
            // reset book (3%)
            92..=94 => {
                let (bids, asks) = random_book(&mut rng);
                if settled.contains(&market) {
                    log(format!("{step} set_book skipped (settled)"), &mut trace);
                } else {
                    let res = venue.set_book(&market, bids, asks);
                    log(format!("{step} set_book -> {res:?}"), &mut trace);
                }
            }
            // actor crash: forget everything since last checkpoint (3%)
            95..=97 => {
                actor.cursor = actor.checkpoint_cursor.clone();
                log(format!("{step} actor-crash, cursor rewound"), &mut trace);
            }
            // outage window (1%)
            98 => {
                let until = clock
                    .now()
                    .checked_add_millis(1_000 + (rng.next_u64() % 5_000) as i64)
                    .map_err(|e| e.to_string())?;
                venue.set_outage_until(until);
                log(format!("{step} outage until {until}"), &mut trace);
            }
            // settle (1%)
            _ => {
                if !settled.contains(&market) {
                    let winner = if rng.next_u64().is_multiple_of(2) {
                        Side::Yes
                    } else {
                        Side::No
                    };
                    match venue.settle_market(&market, winner) {
                        Ok(p) => {
                            settled.insert(market.clone());
                            actor.payouts =
                                actor.payouts.checked_add(p).map_err(|e| e.to_string())?;
                            log(
                                format!("{step} settle {market} {winner:?} -> {p}"),
                                &mut trace,
                            );
                        }
                        Err(e) => log(format!("{step} settle -> {e:?}"), &mut trace),
                    }
                }
            }
        }

        // Continuous invariant: cash never negative, reservations never
        // exceed cash (worst-case reservation discipline holds).
        let (cash, reserved, _, _) = venue.inspect_totals();
        if cash < Cents::ZERO {
            return Err(format!(
                "I-reserve violated at step {step}: negative cash {cash}"
            ));
        }
        if reserved > cash {
            return Err(format!(
                "I-reserve violated at step {step}: reserved {reserved} > cash {cash}"
            ));
        }
        if reserved < Cents::ZERO {
            return Err(format!(
                "I-reserve violated at step {step}: negative reserved {reserved}"
            ));
        }
    }

    // ---- quiesce ----
    // Clear any outage so cleanup can proceed deterministically.
    clock.advance_millis(600_000).map_err(|e| e.to_string())?;
    venue.tick().map_err(|e| format!("quiesce tick: {e}"))?;
    // Cancel everything still resting (bounded retries; cancel faults may
    // time out, but every retry re-rolls).
    for _ in 0..200 {
        let resting = venue.resting_orders();
        if resting.is_empty() {
            break;
        }
        for (id, _) in resting {
            let _ = futures::executor::block_on(venue.cancel(&id));
        }
    }
    if !venue.resting_orders().is_empty() {
        return Err("quiesce: resting orders survived 200 cancel rounds".into());
    }

    // Drain fills to fixpoint (dedup absorbs duplicates; withheld fills are
    // delivered on subsequent polls by construction).
    let (_, _, fill_count, pending_count) = venue.inspect_totals();
    if pending_count != 0 {
        return Err("quiesce: pending orders survived tick".into());
    }
    let mut stable = 0;
    for _ in 0..10_000 {
        // Injected transient API errors are expected here; retry through them.
        let page = match futures::executor::block_on(venue.fills_since(actor.cursor.clone())) {
            Ok(p) => p,
            Err(VenueError::Outage { .. }) => continue,
            Err(e) => return Err(format!("quiesce poll: {e:?}")),
        };
        for f in &page.fills {
            actor.seen.insert(f.fill_id.clone(), f.clone());
        }
        let advanced = page.next_cursor != actor.cursor;
        actor.cursor = page.next_cursor;
        if !advanced && page.fills.is_empty() {
            stable += 1;
            if stable >= 3 && actor.seen.len() >= fill_count {
                break;
            }
        } else {
            stable = 0;
        }
    }

    // ---- invariants ----
    let (cash, reserved, fill_count, _) = venue.inspect_totals();

    // I-delivery: nothing the venue recorded is lost to cursor mechanics.
    if actor.seen.len() != fill_count {
        return Err(format!(
            "I-delivery violated: venue recorded {fill_count} fills, actor saw {}",
            actor.seen.len()
        ));
    }

    // I-reserve: no leaked reservations after cancel-all.
    if reserved != Cents::ZERO {
        return Err(format!(
            "I-reserve violated: reserved {reserved} != $0.00 after cancel-all"
        ));
    }

    // I-money: replay the dedup'd fill stream against initial cash.
    let mut expected = initial_cash;
    for f in actor.seen.values() {
        let gross = f
            .price
            .checked_mul(f.qty.raw())
            .map_err(|e| e.to_string())?;
        expected = match f.action {
            Action::Buy => expected
                .checked_sub(gross)
                .and_then(|c| c.checked_sub(f.fee))
                .map_err(|e| e.to_string())?,
            Action::Sell => expected
                .checked_add(gross)
                .and_then(|c| c.checked_sub(f.fee))
                .map_err(|e| e.to_string())?,
        };
    }
    expected = expected
        .checked_add(actor.payouts)
        .map_err(|e| e.to_string())?;
    if cash != expected {
        return Err(format!(
            "I-money violated: venue cash {cash}, fill-stream replay expects {expected} \
             (initial {initial_cash}, payouts {})",
            actor.payouts
        ));
    }

    // I-position: dedup'd fill-derived net-yes per market == venue positions.
    let mut derived: BTreeMap<MarketId, i64> = BTreeMap::new();
    for f in actor.seen.values() {
        let delta = match (f.side, f.action) {
            (Side::Yes, Action::Buy) => f.qty.raw(),
            (Side::Yes, Action::Sell) => -f.qty.raw(),
            (Side::No, Action::Buy) => -f.qty.raw(),
            (Side::No, Action::Sell) => f.qty.raw(),
        };
        *derived.entry(f.market.clone()).or_insert(0) += delta;
    }
    derived.retain(|m, n| *n != 0 && !settled.contains(m));
    let venue_positions: BTreeMap<MarketId, i64> = {
        // Retry through injected transient errors (bounded).
        let mut result = None;
        for _ in 0..100 {
            match futures::executor::block_on(venue.positions()) {
                Ok(p) => {
                    result = Some(p);
                    break;
                }
                Err(VenueError::Outage { .. }) => continue,
                Err(e) => return Err(format!("positions: {e:?}")),
            }
        }
        result
            .ok_or("positions: 100 transient errors in a row")?
            .into_iter()
            .map(|p| (p.market, p.net_yes))
            .collect()
    };
    if derived != venue_positions {
        return Err(format!(
            "I-position violated: fill-derived {derived:?} != venue {venue_positions:?}"
        ));
    }

    log(
        format!(
            "quiesce ok: cash {cash}, {} fills recorded, {} seen, {} settled",
            fill_count,
            actor.seen.len(),
            settled.len()
        ),
        &mut trace,
    );
    Ok(trace)
}

fn random_faults(rng: &mut SplitMix64, seed: u64) -> FaultConfig {
    // ~1/4 of scenarios run clean (faithful venue); the rest mix faults.
    if rng.next_u64().is_multiple_of(4) {
        return FaultConfig::none(seed);
    }
    let mut pm = |max: u64| (rng.next_u64() % max) as u32;
    FaultConfig {
        seed,
        api_error_pm: pm(150),
        place_timeout_but_placed_pm: pm(250),
        place_reject_pm: pm(200),
        ack_delay_pm: pm(300),
        drop_fill_pm: pm(300),
        dup_fill_pm: pm(300),
        cancel_timeout_cancelled_pm: pm(250),
        cancel_timeout_not_cancelled_pm: pm(250),
    }
}

fn random_book(rng: &mut SplitMix64) -> (Vec<PriceLevel>, Vec<PriceLevel>) {
    // Mid in [20, 80], spread >= 2, up to 3 levels per side, qty 1..50.
    // Prices step CUMULATIVELY away from mid so sides stay strictly sorted.
    let mid = 20 + (rng.next_u64() % 61) as i64;
    let half_spread = 1 + (rng.next_u64() % 5) as i64;
    let mut bids = Vec::new();
    let mut asks = Vec::new();
    let n_bids = rng.next_u64() % 4;
    let n_asks = rng.next_u64() % 4;
    let mut bid_price = mid - half_spread;
    for _ in 0..n_bids {
        if bid_price < 1 {
            break;
        }
        bids.push(PriceLevel {
            price: Cents::new(bid_price),
            qty: Contracts::new(1 + (rng.next_u64() % 50) as i64),
        });
        bid_price -= 1 + (rng.next_u64() % 3) as i64;
    }
    let mut ask_price = mid + half_spread;
    for _ in 0..n_asks {
        if ask_price > 99 {
            break;
        }
        asks.push(PriceLevel {
            price: Cents::new(ask_price),
            qty: Contracts::new(1 + (rng.next_u64() % 50) as i64),
        });
        ask_price += 1 + (rng.next_u64() % 3) as i64;
    }
    (bids, asks)
}

fn kalshi_style_fees() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        r#"
            formula = "quadratic"
            effective_date = "2026-01-01"
            taker_coeff = "0.07"
            maker_coeff = "0.0175"
        "#,
    )
    .unwrap_or_else(|e| panic!("fee schedule literal: {e}"));
    ScheduleFeeModel::new(vec![s]).unwrap_or_else(|e| panic!("fee model: {e}"))
}

// Suppress the unused-error-variant lint surface: VenueError is matched via
// Debug formatting in traces; this keeps the import honest.
#[allow(dead_code)]
fn _venue_error_witness(e: VenueError) -> String {
    format!("{e:?}")
}
