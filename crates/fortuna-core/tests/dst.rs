//! Deterministic simulation testing harness (T0.4; exec layer added T0.6).
//! Spec 5.1, PROMPT doctrine.
//!
//! Contract (scripts/run-dst.sh):
//!   dst [--nocapture] --seeds N        # corpus first, then N random seeds
//!   dst --replay-seed S                # one seed, verbose trace
//!
//! Every scenario builds a seeded world — sim clock, sim venue with a random
//! fault config, random markets/books, a REAL gate pipeline, and a REAL
//! order manager over an in-memory journal — and drives a seeded action
//! stream: gated placements, cancels, fill polls, ticks, clock advances,
//! public flow, book resets, settles, outages, TTL sweeps, and CRASHES
//! (drop the manager, rebuild from the journal, boot-reconcile). This
//! composes the spec 5.4 crash scenarios randomly: crash between persistence
//! and submission, crash between submission and ack, duplicate fill
//! delivery, fill after local cancel, partial fill then outage.
//!
//! Invariants at quiesce:
//!   I-money       venue cash == replay of the DEDUP'D fill stream + payouts
//!   I-reserve     reserved == 0 after cancel-all; cash never negative
//!   I-position    fill-derived positions == venue positions
//!   I-delivery    every fill the venue recorded is eventually delivered
//!   I-journal     no intent stuck Submitted after boot; every venue order's
//!                 client id is journaled (no orphans from our side); per
//!                 intent, cum_filled == the dedup'd fills for its order
//!   I-determinism every scenario runs twice; traces must be byte-identical
//!
//! A violation prints the seed and fails the run (exit 1). Reproduce with
//!   scripts/replay.sh --seed <S>
//! Minimize per dst-corpus/README.md, then commit the seed file.

use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentId, SplitMix64};
use fortuna_core::market::{Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::{ExecError, ExecPolicy, IntentStatus, MemoryJournal, OrderManager};
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline};
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{Fill, Market, MarketStatus, PriceLevel, SettlementMeta, Venue, VenueError};
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
            "--nocapture" => {}
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

const START_MS: i64 = 1_780_000_000_000;

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

fn permissive_gate_config() -> GateConfig {
    // Generous caps and huge rate buckets: DST exercises the gated PATH and
    // crash machinery; tight-limit behavior has its own suites in
    // fortuna-gates. Caps still real (i64), checks all run.
    let src = r#"
        [global]
        max_total_exposure_cents = 100000000
        max_daily_loss_cents = 50000
        min_order_contracts = 1
        max_order_contracts = 100000
        price_band_cents = 98
        max_cross_cents = 98
        per_market_exposure_cents = 100000000
        per_event_exposure_cents = 100000000
        require_event_mapping = false

        [per_strategy.dst]
        max_exposure_cents = 100000000
        max_order_notional_cents = 100000000
        min_net_edge_bps = 0

        [rate.sim]
        burst = 1000000
        sustained_per_min = 1000000
        market_burst = 1000000
        market_sustained_per_min = 1000000
    "#;
    toml::from_str(src).unwrap_or_else(|e| panic!("gate config literal: {e}"))
}

struct World {
    clock: Arc<SimClock>,
    venue: SimVenue,
    pipeline: GatePipeline,
    manager: Option<OrderManager<MemoryJournal>>,
    ids: IdGen,
    seen: BTreeMap<String, Fill>,
    cursor: fortuna_venues::Cursor,
    payouts: Cents,
    settled: BTreeSet<MarketId>,
    markets: Vec<MarketId>,
    coid_seq: u64,
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

    let manager = OrderManager::recover(
        MemoryJournal::default(),
        clock.clone(),
        ExecPolicy {
            default_ttl_ms: 120_000,
            ..ExecPolicy::default()
        },
    )
    .map_err(|e| format!("manager init: {e}"))?;

    let mut w = World {
        clock,
        venue,
        pipeline: GatePipeline::new(permissive_gate_config())
            .map_err(|e| format!("pipeline init: {e}"))?,
        manager: Some(manager),
        ids: IdGen::new(seed ^ 0xDEAD_BEEF),
        seen: BTreeMap::new(),
        cursor: fortuna_venues::Cursor::start(),
        payouts: Cents::ZERO,
        settled: BTreeSet::new(),
        markets,
        coid_seq: 0,
    };
    log(
        format!("world seed={seed} markets={n_markets} cash={initial_cash} faults={faults:?}"),
        &mut trace,
    );

    let n_actions = 30 + (rng.next_u64() % 50) as usize;
    for step in 0..n_actions {
        let market = w.markets[(rng.next_u64() % w.markets.len() as u64) as usize].clone();
        match rng.next_u64() % 100 {
            // gated placement (38%)
            0..=37 => {
                let line = place_via_gates(&mut w, &mut rng, &market)?;
                log(format!("{step} {line}"), &mut trace);
            }
            // cancel a working intent (10%)
            38..=47 => {
                let manager = w.manager.as_mut().ok_or("manager missing")?;
                let working: Vec<IntentId> = manager
                    .intents()
                    .iter()
                    .filter(|(_, r)| r.status.is_working() && r.venue_order_id.is_some())
                    .map(|(id, _)| **id)
                    .collect();
                if working.is_empty() {
                    log(format!("{step} cancel: nothing working"), &mut trace);
                } else {
                    let target = working[(rng.next_u64() % working.len() as u64) as usize];
                    let res = futures::executor::block_on(manager.cancel_intent(target, &w.venue));
                    log(format!("{step} cancel {target} -> {res:?}"), &mut trace);
                }
            }
            // poll + ingest fills (15%)
            48..=62 => {
                let line = poll_and_ingest(&mut w)?;
                log(format!("{step} {line}"), &mut trace);
            }
            // tick (6%)
            63..=68 => {
                let res = w.venue.tick();
                log(format!("{step} tick -> {res:?}"), &mut trace);
            }
            // advance clock (8%)
            69..=76 => {
                let ms = 100 + rng.next_u64() % 60_000;
                w.clock.advance_millis(ms).map_err(|e| e.to_string())?;
                log(format!("{step} advance {ms}ms"), &mut trace);
            }
            // public flow (6%)
            77..=82 => {
                let res = w.venue.inject_public_order(
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
                    format!("{step} public -> {:?}", res.map(|f| f.len())),
                    &mut trace,
                );
            }
            // book reset (3%)
            83..=85 => {
                if w.settled.contains(&market) {
                    log(format!("{step} set_book skipped (settled)"), &mut trace);
                } else {
                    let (bids, asks) = random_book(&mut rng);
                    let res = w.venue.set_book(&market, bids, asks);
                    log(format!("{step} set_book -> {res:?}"), &mut trace);
                }
            }
            // CRASH: rebuild from journal + boot reconcile (6%)
            86..=91 => {
                let line = crash_and_recover(&mut w)?;
                log(format!("{step} {line}"), &mut trace);
            }
            // crash BETWEEN persistence and submission (2%)
            92..=93 => {
                let line = created_only_crash(&mut w, &mut rng, &market)?;
                log(format!("{step} {line}"), &mut trace);
            }
            // TTL sweep (3%)
            94..=96 => {
                let manager = w.manager.as_mut().ok_or("manager missing")?;
                let res = futures::executor::block_on(manager.sweep_ttl(&w.venue));
                log(format!("{step} ttl-sweep -> {res:?}"), &mut trace);
            }
            // outage window (2%)
            97..=98 => {
                let until = w
                    .clock
                    .now()
                    .checked_add_millis(1_000 + (rng.next_u64() % 5_000) as i64)
                    .map_err(|e| e.to_string())?;
                w.venue.set_outage_until(until);
                log(format!("{step} outage until {until}"), &mut trace);
            }
            // settle (1%)
            _ => {
                if !w.settled.contains(&market) {
                    let winner = if rng.next_u64().is_multiple_of(2) {
                        Side::Yes
                    } else {
                        Side::No
                    };
                    match w.venue.settle_market(&market, winner) {
                        Ok(p) => {
                            w.settled.insert(market.clone());
                            w.payouts = w.payouts.checked_add(p).map_err(|e| e.to_string())?;
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

        let (cash, reserved, _, _) = w.venue.inspect_totals();
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
    w.clock.advance_millis(600_000).map_err(|e| e.to_string())?;
    w.venue.tick().map_err(|e| format!("quiesce tick: {e}"))?;

    // Final crash + boot: resolves every Submitted-unknown intent.
    let boot_line = crash_and_recover(&mut w)?;
    log(format!("quiesce {boot_line}"), &mut trace);

    // I-journal (pre-cancel): every venue order's client id is ours.
    {
        let manager = w.manager.as_ref().ok_or("manager missing")?;
        let known = manager.known_client_order_ids();
        for _ in 0..100 {
            match futures::executor::block_on(w.venue.open_orders()) {
                Ok(open) => {
                    for o in &open {
                        if !known.contains(o.client_order_id.as_str()) {
                            return Err(format!(
                                "I-journal violated: venue order {} has unknown client id {}",
                                o.venue_order_id, o.client_order_id
                            ));
                        }
                    }
                    break;
                }
                Err(VenueError::Outage { .. }) => continue,
                Err(e) => return Err(format!("open_orders: {e:?}")),
            }
        }
    }

    // Cancel everything still working through the manager (bounded).
    for _ in 0..200 {
        let manager = w.manager.as_mut().ok_or("manager missing")?;
        let working: Vec<IntentId> = manager
            .intents()
            .iter()
            .filter(|(_, r)| r.status.is_working() && r.venue_order_id.is_some())
            .map(|(id, _)| **id)
            .collect();
        if working.is_empty() && w.venue.resting_orders().is_empty() {
            break;
        }
        for intent in working {
            let _ = futures::executor::block_on(manager.cancel_intent(intent, &w.venue));
        }
    }
    if !w.venue.resting_orders().is_empty() {
        return Err("quiesce: venue resting orders survived 200 cancel rounds".into());
    }

    // Drain fills to fixpoint.
    let (_, _, fill_count, pending_count) = w.venue.inspect_totals();
    if pending_count != 0 {
        return Err("quiesce: pending orders survived tick".into());
    }
    let mut stable = 0;
    for _ in 0..10_000 {
        let before = w.seen.len();
        let advanced = match poll_and_ingest(&mut w) {
            Ok(_) => true,
            Err(e) => return Err(format!("quiesce poll: {e}")),
        };
        let _ = advanced;
        if w.seen.len() == before {
            stable += 1;
            if stable >= 5 && w.seen.len() >= fill_count {
                break;
            }
        } else {
            stable = 0;
        }
    }

    // ---- invariants ----
    let (cash, reserved, fill_count, _) = w.venue.inspect_totals();

    if w.seen.len() != fill_count {
        return Err(format!(
            "I-delivery violated: venue recorded {fill_count} fills, actor saw {}",
            w.seen.len()
        ));
    }
    if reserved != Cents::ZERO {
        return Err(format!(
            "I-reserve violated: reserved {reserved} != $0.00 after cancel-all"
        ));
    }

    // I-money.
    let mut expected = initial_cash;
    for f in w.seen.values() {
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
    expected = expected.checked_add(w.payouts).map_err(|e| e.to_string())?;
    if cash != expected {
        return Err(format!(
            "I-money violated: venue cash {cash}, fill-stream replay expects {expected}"
        ));
    }

    // I-position (per side: YES and NO lots never net against each other).
    let mut derived: BTreeMap<MarketId, (i64, i64)> = BTreeMap::new();
    for f in w.seen.values() {
        let e = derived.entry(f.market.clone()).or_insert((0, 0));
        match (f.side, f.action) {
            (Side::Yes, Action::Buy) => e.0 += f.qty.raw(),
            (Side::Yes, Action::Sell) => e.0 -= f.qty.raw(),
            (Side::No, Action::Buy) => e.1 += f.qty.raw(),
            (Side::No, Action::Sell) => e.1 -= f.qty.raw(),
        }
    }
    derived.retain(|m, (y, n)| (*y != 0 || *n != 0) && !w.settled.contains(m));
    let venue_positions: BTreeMap<MarketId, (i64, i64)> = {
        let mut result = None;
        for _ in 0..100 {
            match futures::executor::block_on(w.venue.positions()) {
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
            .map(|p| (p.market, (p.yes, p.no)))
            .collect()
    };
    if derived != venue_positions {
        return Err(format!(
            "I-position violated: fill-derived {derived:?} != venue {venue_positions:?}"
        ));
    }

    // I-journal: no stuck Submitted; per-intent cum == dedup'd fills.
    let manager = w.manager.as_ref().ok_or("manager missing")?;
    let mut fills_by_coid: BTreeMap<String, i64> = BTreeMap::new();
    for f in w.seen.values() {
        *fills_by_coid
            .entry(f.client_order_id.as_str().to_string())
            .or_insert(0) += f.qty.raw();
    }
    for (id, rec) in manager.intents() {
        if rec.status == IntentStatus::Submitted {
            return Err(format!(
                "I-journal violated: intent {id} stuck Submitted after boot reconcile"
            ));
        }
        let seen_qty = fills_by_coid
            .get(rec.order.client_order_id.as_str())
            .copied()
            .unwrap_or(0);
        if rec.cum_filled.raw() != seen_qty {
            return Err(format!(
                "I-journal violated: intent {id} cum_filled {} != dedup'd fills {seen_qty}",
                rec.cum_filled
            ));
        }
    }

    log(
        format!(
            "quiesce ok: cash {cash}, {fill_count} fills, {} intents, {} settled",
            manager.intents().len(),
            w.settled.len()
        ),
        &mut trace,
    );
    Ok(trace)
}

/// Build a seeded candidate, run it through the REAL gate pipeline, submit
/// through the manager. Returns the trace line.
fn place_via_gates(
    w: &mut World,
    rng: &mut SplitMix64,
    market: &MarketId,
) -> Result<String, String> {
    w.coid_seq += 1;
    let intent = IntentId::new(
        w.ids
            .next(w.clock.now())
            .map_err(|e| format!("idgen: {e}"))?,
    );
    let limit = Cents::new(1 + (rng.next_u64() % 99) as i64);
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("dst").map_err(|e| e.to_string())?,
        venue: VenueId::new("sim").map_err(|e| e.to_string())?,
        market: market.clone(),
        side: if rng.next_u64().is_multiple_of(2) {
            Side::Yes
        } else {
            Side::No
        },
        action: if rng.next_u64() % 4 == 3 {
            Action::Sell
        } else {
            Action::Buy
        },
        limit_price: limit,
        qty: Contracts::new(1 + (rng.next_u64() % 20) as i64),
        fair_value: Cents::new((limit.raw() + 3).min(100)),
        client_order_id: ClientOrderId::from_intent(intent),
    };

    let book = futures::executor::block_on(w.venue.book(market)).ok();
    let manager = w.manager.as_mut().ok_or("manager missing")?;
    let known = manager.known_client_order_ids();
    let fees = kalshi_style_fees();
    let inputs = GateInputs {
        now: w.clock.now(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: book.as_ref(),
        last_trade_price: Some(Cents::new(50)),
        fee_model: &fees,
        category: Some("dst"),
        own_resting: &[],
        recent_client_order_ids: &known,
    };
    let outcome = w.pipeline.evaluate(&candidate, &inputs);
    match outcome.gated {
        Err(rej) => Ok(format!("gate-reject {:?}: {}", rej.check, rej.reason)),
        Ok(order) => {
            let res = futures::executor::block_on(manager.submit(order, &w.venue));
            match res {
                Ok(out) => Ok(format!("placed {intent} -> {out:?}")),
                Err(ExecError::WorkingOrderExists { existing, .. }) => {
                    Ok(format!("exec-refused (working order {existing})"))
                }
                Err(e) => Err(format!("submit failed unexpectedly: {e}")),
            }
        }
    }
}

/// Poll one page of fills and ingest through the manager (and the actor's
/// dedup view, which feeds the venue-level invariants).
fn poll_and_ingest(w: &mut World) -> Result<String, String> {
    let page = match futures::executor::block_on(w.venue.fills_since(w.cursor.clone())) {
        Ok(p) => p,
        Err(VenueError::Outage { .. }) => return Ok("poll -> outage".into()),
        Err(e) => return Err(format!("poll: {e:?}")),
    };
    let n = page.fills.len();
    let manager = w.manager.as_mut().ok_or("manager missing")?;
    for f in &page.fills {
        w.seen.insert(f.fill_id.clone(), f.clone());
        match manager.ingest_fill(f) {
            Ok(_) => {}
            Err(e) => return Err(format!("ingest_fill: {e}")),
        }
    }
    let line = format!(
        "poll {} -> {n} fill(s), cursor {}",
        w.cursor.0, page.next_cursor.0
    );
    w.cursor = page.next_cursor;
    Ok(line)
}

/// Drop the manager, rebuild from the journal, boot-reconcile (with bounded
/// retries across outage windows).
fn crash_and_recover(w: &mut World) -> Result<String, String> {
    let manager = w.manager.take().ok_or("manager missing")?;
    let journal = manager.into_journal();
    let mut rebuilt = OrderManager::recover(
        journal,
        w.clock.clone(),
        ExecPolicy {
            default_ttl_ms: 120_000,
            ..ExecPolicy::default()
        },
    )
    .map_err(|e| format!("recover: {e}"))?;
    let mut last_err = String::new();
    for _ in 0..10 {
        match futures::executor::block_on(rebuilt.boot_reconcile(&w.venue)) {
            Ok(report) => {
                // The manager's cursor checkpoint may differ from the
                // actor's; the actor keeps its own for venue invariants.
                let line = format!(
                    "crash+boot: adopted={} orphans={} closed={} missing={} fills={} orphan_fills={}",
                    report.adopted.len(),
                    report.orphans_cancelled.len(),
                    report.closed_unsubmitted.len(),
                    report.missing_at_venue.len(),
                    report.fills_applied,
                    report.orphan_fills.len()
                );
                // Fills the BOOT drained must also land in the actor's view.
                let refresh = {
                    w.manager = Some(rebuilt);
                    poll_to_fixpoint(w)?
                };
                return Ok(format!("{line}; refreshed {refresh}"));
            }
            Err(e) => {
                last_err = e.to_string();
                w.clock.advance_millis(10_000).map_err(|e| e.to_string())?;
            }
        }
    }
    Err(format!("boot_reconcile failed 10 times: {last_err}"))
}

/// Journal a Created row and "crash" before submitting (spec 5.4: crash
/// between intent persistence and submission). Boot closes it later.
fn created_only_crash(
    w: &mut World,
    rng: &mut SplitMix64,
    market: &MarketId,
) -> Result<String, String> {
    w.coid_seq += 1;
    let intent = IntentId::new(
        w.ids
            .next(w.clock.now())
            .map_err(|e| format!("idgen: {e}"))?,
    );
    let limit = Cents::new(1 + (rng.next_u64() % 99) as i64);
    let candidate = CandidateOrder {
        intent_id: intent,
        strategy: StrategyId::new("dst").map_err(|e| e.to_string())?,
        venue: VenueId::new("sim").map_err(|e| e.to_string())?,
        market: market.clone(),
        side: Side::Yes,
        action: Action::Buy,
        limit_price: limit,
        qty: Contracts::new(1),
        fair_value: Cents::new((limit.raw() + 3).min(100)),
        client_order_id: ClientOrderId::from_intent(intent),
    };
    let book = futures::executor::block_on(w.venue.book(market)).ok();
    let fees = kalshi_style_fees();
    let manager = w.manager.as_mut().ok_or("manager missing")?;
    let known = manager.known_client_order_ids();
    let inputs = GateInputs {
        now: w.clock.now(),
        open_exposure_cents: Cents::ZERO,
        market_exposure_cents: Cents::ZERO,
        strategy_exposure_cents: Cents::ZERO,
        event_exposure_cents: Cents::ZERO,
        event_id: None,
        book: book.as_ref(),
        last_trade_price: Some(Cents::new(50)),
        fee_model: &fees,
        category: Some("dst"),
        own_resting: &[],
        recent_client_order_ids: &known,
    };
    match w.pipeline.evaluate(&candidate, &inputs).gated {
        Err(rej) => Ok(format!("created-crash gate-reject {:?}", rej.check)),
        Ok(order) => {
            manager.journal_created_for_test(&order);
            let line = crash_and_recover(w)?;
            Ok(format!("created-then-{line}"))
        }
    }
}

fn poll_to_fixpoint(w: &mut World) -> Result<usize, String> {
    let mut total = 0;
    let mut stable = 0;
    for _ in 0..1_000 {
        let before = w.seen.len();
        poll_and_ingest(w)?;
        let gained = w.seen.len() - before;
        total += gained;
        if gained == 0 {
            stable += 1;
            if stable >= 3 {
                break;
            }
        } else {
            stable = 0;
        }
    }
    Ok(total)
}

fn random_faults(rng: &mut SplitMix64, seed: u64) -> FaultConfig {
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
