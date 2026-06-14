//! E4: seeded DST over the SETTLEMENT and WATCHDOG planes (spec 5.13 +
//! the PROMPT doctrine list). The composed settlement_loop tests pin
//! these paths deterministically; THIS harness drives them from seeds so
//! the standing battery (scripts/run-dst.sh) exercises discrepancies,
//! halts, reversals, voids, disputes, overdue alerts, orphan scans,
//! canonical divergences, audit death, and wide-spread marks — and
//! PROVES it did (per-arm hit accounting; a battery that never fired an
//! arm fails).
//!
//! Per-seed invariants:
//!   1. No tick ever errors, whatever the action schedule.
//!   2. A position-mismatch streak yields a discrepancy AND a global
//!      halt; the next tick submits zero orders.
//!   3. Audit death halts trading (no audit, no trading — I5).
//!   4. The discrepancy counter equals the audited discrepancy rows.
//!   5. The recording is byte-identical when the seed re-runs.
//!
//! Conventions follow the other DST harnesses: master seed from
//! DST_MASTER_SEED or wall clock (printed), scenario count via
//! SETTLE_DST_SCENARIOS (default 20), failures print their seed.

use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::SplitMix64;
use fortuna_core::market::{Action, MarketId, Side, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::ExecPolicy;
use fortuna_gates::GateConfig;
use fortuna_runner::{
    AuditSink, CoreHandle, MemoryAuditSink, Proposal, ProposedLeg, RunnerConfig, RunnerError,
    SimRunner, Stage, Strategy, StrategyKind, StrategyMetrics, Urgency,
};
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

fn lvl(price: i64, qty: i64) -> PriceLevel {
    PriceLevel {
        price: Cents::new(price),
        qty: fortuna_core::market::Contracts::new(qty),
    }
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

fn settle_market(id: &str) -> Market {
    Market {
        id: mkt(id),
        venue: VenueId::new("sim").unwrap(),
        title: format!("settle {id}"),
        category: "test".into(),
        status: MarketStatus::Trading,
        close_at: Some(t0().checked_add_millis(3_600_000).unwrap()),
        settlement: SettlementMeta {
            oracle_type: "t".into(),
            resolution_source: "t".into(),
            expected_lag_hours: 1,
        },
        volume_contracts: Some(1_000),
        payout_per_contract: Cents::new(100),
    }
}

fn gate_config() -> GateConfig {
    toml::from_str(
        r#"
        [global]
        max_total_exposure_cents = 800000
        max_daily_loss_cents = 500000
        min_order_contracts = 1
        max_order_contracts = 1000
        price_band_cents = 45
        max_cross_cents = 10
        per_market_exposure_cents = 100000
        per_event_exposure_cents = 150000
        require_event_mapping = false

        [per_strategy.dst_buyer]
        max_exposure_cents = 200000
        max_order_notional_cents = 100000
        min_net_edge_bps = 1

        [rate.sim]
        burst = 100
        sustained_per_min = 600
        market_burst = 50
        market_sustained_per_min = 300
        "#,
    )
    .unwrap()
}

#[derive(Clone, Default)]
struct SharedAuditSink(Arc<Mutex<MemoryAuditSink>>);

impl AuditSink for SharedAuditSink {
    fn append(
        &mut self,
        kind: &str,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<(), RunnerError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(kind, ref_id, payload)
    }
}

impl SharedAuditSink {
    fn with_fail_after(limit: Option<usize>) -> SharedAuditSink {
        SharedAuditSink(Arc::new(Mutex::new(MemoryAuditSink {
            records: Vec::new(),
            fail_after: limit,
        })))
    }
    fn rows_of_kind(&self, kind: &str) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .records
            .iter()
            .filter(|(k, _, _)| k == kind)
            .map(|(_, _, p)| p.clone())
            .collect()
    }
}

/// Buys 10 YES at the first ask it sees, once.
struct DstBuyer {
    proposed: bool,
    metrics: StrategyMetrics,
}

#[async_trait::async_trait]
impl Strategy for DstBuyer {
    fn id(&self) -> StrategyId {
        StrategyId::new("dst_buyer").unwrap()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }
    fn stage(&self) -> Stage {
        Stage::Sim
    }
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;
        if self.proposed {
            return Ok(Vec::new());
        }
        let EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        let Some(ask) = book.yes_asks.first() else {
            return Ok(Vec::new());
        };
        self.proposed = true;
        Ok(vec![Proposal {
            legs: vec![ProposedLeg {
                market: book.market.clone(),
                side: Side::Yes,
                action: Action::Buy,
                limit_price: ask.price,
                fair_value: Cents::new(ask.price.raw() + 5),
                calibrated_p: None,
            }],
            group_policy: None,
            urgency: Urgency::Taker,
            manifest_hash: None,
            thesis: "dst buyer".into(),
        }])
    }
    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}

/// Proposes ONE two-leg group (KXS + KXS2) once both books are seen —
/// drives submit_group_concurrent under the arm's seeded venue faults.
struct GroupBuyer {
    seen: std::collections::BTreeSet<MarketId>,
    proposed: bool,
    metrics: StrategyMetrics,
}

#[async_trait::async_trait]
impl Strategy for GroupBuyer {
    fn id(&self) -> StrategyId {
        StrategyId::new("dst_buyer").unwrap()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }
    fn stage(&self) -> Stage {
        Stage::Sim
    }
    async fn on_event(
        &mut self,
        ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;
        if self.proposed {
            return Ok(Vec::new());
        }
        let EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        if !book.yes_asks.is_empty() {
            self.seen.insert(book.market.clone());
        }
        if self.seen.len() < 2 {
            return Ok(Vec::new());
        }
        self.proposed = true;
        let leg = |m: &str| ProposedLeg {
            market: mkt(m),
            side: Side::Yes,
            action: Action::Buy,
            limit_price: Cents::new(60),
            fair_value: Cents::new(66),
            calibrated_p: None,
        };
        Ok(vec![Proposal {
            legs: vec![leg("KXS"), leg("KXS2")],
            group_policy: Some(fortuna_exec::GroupPolicy {
                max_unhedged_notional: Cents::new(5_000),
                max_leg_open_ms: 60_000,
                value_per_set: Cents::new(100),
                min_completion_edge_bps: 1,
            }),
            urgency: Urgency::Taker,
            manifest_hash: None,
            thesis: "dst group buyer".into(),
        }])
    }
    fn metrics(&self) -> StrategyMetrics {
        self.metrics
    }
}

fn runner_config(seed: u64) -> RunnerConfig {
    RunnerConfig {
        seed,
        gate_config: gate_config(),
        exec_policy: ExecPolicy::default(),
        envelopes: BTreeMap::from([("dst_buyer".to_string(), Cents::new(100_000))]),
        max_daily_loss: Cents::new(500_000),
        fee_model: fee_model(),
        markets: vec![settle_market("KXS"), settle_market("KXS2")],
        starting_cash: Cents::new(1_000_000),
        faults: Some(FaultConfig::none(seed)),
        mark_policy: MarkPolicy {
            max_book_age_ms: 86_400_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 10,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    }
}

/// The seeded arm vocabulary. One primary arm per scenario keeps per-arm
/// accounting honest; the battery's seed mix supplies the chaos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Arm {
    SettleClean,
    SettleThenCorrect,
    Void,
    Dispute,
    VenueMismatch,
    CanonicalDivergence,
    OrphanScan,
    AuditDeath,
    WideBook,
    Overdue,
    /// Two-leg group through submit_group_concurrent under seeded venue
    /// faults (concurrent-legs gate residue: the chaos battery must
    /// exercise the concurrent path, not only composed tests).
    MultiLegGroup,
}

const ARMS: [Arm; 11] = [
    Arm::SettleClean,
    Arm::SettleThenCorrect,
    Arm::Void,
    Arm::Dispute,
    Arm::VenueMismatch,
    Arm::CanonicalDivergence,
    Arm::OrphanScan,
    Arm::AuditDeath,
    Arm::WideBook,
    Arm::Overdue,
    Arm::MultiLegGroup,
];

struct ScenarioResult {
    recording: String,
    arm: Arm,
    discrepancies: u64,
    watchdog_rows: usize,
    halted: bool,
}

fn tick(runner: &mut SimRunner) -> Result<fortuna_runner::TickReport, String> {
    futures::executor::block_on(runner.tick()).map_err(|e| format!("tick errored: {e}"))
}

fn run_scenario(seed: u64) -> Result<ScenarioResult, String> {
    let mut rng = SplitMix64::new(seed);
    let arm = ARMS[(rng.next_u64() % ARMS.len() as u64) as usize];

    // The multi-leg arm seeds venue faults so the CONCURRENT submission
    // path sees ack delays, transient API errors, and rejects.
    let mut faults = FaultConfig::none(seed);
    if arm == Arm::MultiLegGroup {
        faults.ack_delay_pm = (rng.next_u64() % 300) as u32;
        faults.api_error_pm = (rng.next_u64() % 150) as u32;
        faults.place_reject_pm = (rng.next_u64() % 150) as u32;
    }

    // Audit death plants a fuse early enough to fire mid-run.
    let audit = match arm {
        Arm::AuditDeath => {
            SharedAuditSink::with_fail_after(Some(3 + (rng.next_u64() % 5) as usize))
        }
        _ => SharedAuditSink::default(),
    };
    let strategy: Box<dyn Strategy> = if arm == Arm::MultiLegGroup {
        Box::new(GroupBuyer {
            seen: std::collections::BTreeSet::new(),
            proposed: false,
            metrics: StrategyMetrics::default(),
        })
    } else {
        Box::new(DstBuyer {
            proposed: false,
            metrics: StrategyMetrics::default(),
        })
    };
    let mut config = runner_config(seed);
    config.faults = Some(faults);
    let mut runner = SimRunner::new(config, vec![strategy], Box::new(audit.clone()), t0())
        .map_err(|e| format!("construction: {e}"))?;

    runner
        .venue()
        .set_book(&mkt("KXS"), vec![lvl(55, 50)], vec![lvl(60, 50)])
        .map_err(|e| e.to_string())?;
    runner
        .venue()
        .set_book(&mkt("KXS2"), vec![lvl(55, 50)], vec![lvl(60, 50)])
        .map_err(|e| e.to_string())?;

    // Tick 1: the buyer fills (except under early audit death, where the
    // halt may land first — both are legal worlds).
    let first = tick(&mut runner)?;
    let filled = first.fills_applied >= 1;

    // The primary arm.
    match arm {
        Arm::SettleClean => {
            let winner = if rng.next_u64().is_multiple_of(2) {
                Side::Yes
            } else {
                Side::No
            };
            runner
                .venue()
                .settle_market(&mkt("KXS"), winner)
                .map_err(|e| e.to_string())?;
        }
        Arm::SettleThenCorrect => {
            runner
                .venue()
                .settle_market(&mkt("KXS"), Side::Yes)
                .map_err(|e| e.to_string())?;
            tick(&mut runner)?;
            runner
                .venue()
                .reverse_settlement(&mkt("KXS"), Side::No)
                .map_err(|e| e.to_string())?;
        }
        Arm::Void => {
            // A void REPLACES settlement (the market dies unresolved);
            // voiding an already-settled market is venue-rejected, which
            // the first draft of this arm learned the hard way.
            runner
                .venue()
                .void_market(&mkt("KXS"))
                .map_err(|e| e.to_string())?;
        }
        Arm::Dispute => {
            runner
                .venue()
                .set_market_status(&mkt("KXS"), MarketStatus::Disputed);
        }
        Arm::VenueMismatch => {
            // The venue grows a position our books never saw.
            runner
                .venue()
                .seed_position(&mkt("KXS"), 12, 0, Cents::new(720));
        }
        Arm::CanonicalDivergence => {
            runner.set_canonical_resolution(mkt("KXS"), Side::No, "edge-dst");
            runner
                .venue()
                .settle_market(&mkt("KXS"), Side::Yes)
                .map_err(|e| e.to_string())?;
        }
        Arm::OrphanScan => {
            runner.set_position_coverage(std::collections::BTreeSet::new());
        }
        Arm::AuditDeath => { /* the fuse is in the sink */ }
        Arm::WideBook => {
            runner
                .venue()
                .set_book(&mkt("KXS"), vec![lvl(2, 5)], vec![lvl(97, 5)])
                .map_err(|e| e.to_string())?;
        }
        Arm::Overdue => {
            // Jump past close + expected lag + grace: 1h + 1h + 1h + slack.
            runner
                .clock
                .advance_millis(4 * 3_600_000)
                .map_err(|e| e.to_string())?;
        }
        Arm::MultiLegGroup => {
            // The faults ARE the arm; ticks below drive the group's
            // concurrent submission, fills, and TTL machinery. Process
            // any ack-delayed orders at the venue between ticks.
            runner.venue().tick().map_err(|e| e.to_string())?;
        }
    }

    // Settle-side processing + watchdogs need a few ticks (mismatch
    // needs 3 consecutive).
    let extra = 4 + (rng.next_u64() % 3);
    for _ in 0..extra {
        runner
            .clock
            .advance_millis(500 + rng.next_u64() % 1_500)
            .map_err(|e| e.to_string())?;
        if arm == Arm::MultiLegGroup {
            runner.venue().tick().map_err(|e| e.to_string())?;
        }
        tick(&mut runner)?;
    }
    if arm == Arm::MultiLegGroup {
        // Whatever the fault dice did, accounting must reconcile: orders
        // never exceed the two proposed legs, and the money plane holds
        // (checked via report() below like every arm).
        let c = runner.counters();
        if c.orders_submitted > 2 {
            return Err(format!(
                "group of 2 legs submitted {} orders",
                c.orders_submitted
            ));
        }
    }

    let counters = runner.counters();
    let halted = runner.gates().halts().global_halted().is_some();

    // Arm-specific invariants.
    match arm {
        Arm::VenueMismatch => {
            if filled {
                if counters.discrepancies == 0 {
                    return Err("mismatch arm produced no discrepancy".into());
                }
                if !halted {
                    return Err("mismatch discrepancy must global-halt".into());
                }
                let post = tick(&mut runner)?;
                if post.orders_submitted != 0 {
                    return Err("orders submitted after a global halt".into());
                }
            }
        }
        Arm::AuditDeath => {
            if !halted {
                return Err("audit death must halt trading (I5)".into());
            }
        }
        Arm::CanonicalDivergence => {
            if filled
                && !audit
                    .rows_of_kind("discrepancy")
                    .iter()
                    .any(|r| r["kind"] == "settlement_divergence")
            {
                return Err("divergence arm recorded no settlement_divergence".into());
            }
        }
        Arm::OrphanScan => {
            if filled
                && !audit
                    .rows_of_kind("watchdog")
                    .iter()
                    .any(|r| r["kind"] == "orphaned_position")
            {
                return Err("orphan arm raised no orphaned_position".into());
            }
        }
        Arm::Dispute => {
            if filled
                && !audit
                    .rows_of_kind("watchdog")
                    .iter()
                    .any(|r| r["kind"] == "dispute_freeze")
            {
                return Err("dispute arm froze nothing".into());
            }
        }
        Arm::Overdue => {
            if filled
                && !audit
                    .rows_of_kind("watchdog")
                    .iter()
                    .any(|r| r["kind"] == "settlement_overdue")
            {
                return Err("overdue arm alerted nothing".into());
            }
        }
        _ => {}
    }

    // Cross-arm consistency: the counter agrees with the audit rows
    // (audit-death runs lose rows by design — counters only there).
    if arm != Arm::AuditDeath {
        let rows = audit.rows_of_kind("discrepancy").len() as u64;
        if counters.discrepancies != rows {
            return Err(format!(
                "discrepancy counter {} != audited rows {rows}",
                counters.discrepancies
            ));
        }
    }

    let recording = runner
        .recording()
        .to_jsonl()
        .map_err(|e| format!("recording: {e}"))?;
    Ok(ScenarioResult {
        recording,
        arm,
        discrepancies: counters.discrepancies,
        watchdog_rows: audit.rows_of_kind("watchdog").len(),
        halted,
    })
}

#[test]
fn settlement_and_watchdog_paths_survive_seeded_chaos() {
    let scenarios: u64 = std::env::var("SETTLE_DST_SCENARIOS")
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
    println!("[settlement-dst] master seed {master} -> {scenarios} scenario(s)");

    let mut master_rng = SplitMix64::new(master);
    let mut failures = Vec::new();
    let mut arm_hits: BTreeMap<Arm, u64> = BTreeMap::new();
    let mut total_discrepancies = 0u64;
    let mut total_watchdog_rows = 0usize;
    let mut total_halts = 0u64;

    for _ in 0..scenarios {
        let seed = master_rng.next_u64();
        match run_scenario(seed) {
            Ok(first) => match run_scenario(seed) {
                Ok(second) if second.recording == first.recording => {
                    *arm_hits.entry(first.arm).or_insert(0) += 1;
                    total_discrepancies += first.discrepancies;
                    total_watchdog_rows += first.watchdog_rows;
                    total_halts += u64::from(first.halted);
                }
                Ok(_) => failures.push((seed, "recording differs on replay".to_string())),
                Err(e) => failures.push((seed, format!("replay errored: {e}"))),
            },
            Err(e) => failures.push((seed, e)),
        }
    }

    println!(
        "[settlement-dst] arms {arm_hits:?}; {total_discrepancies} discrepancies, \
         {total_watchdog_rows} watchdog rows, {total_halts} halts"
    );
    assert!(
        failures.is_empty(),
        "[settlement-dst] master {master}: {} failing seed(s): {:?}",
        failures.len(),
        failures
    );
    // The battery must EXERCISE the paths it claims to cover. Aggregate
    // and per-arm coverage asserts are gated on a scenario floor: at the
    // default 20 scenarios over 10 arms, a seed draw can legitimately
    // miss the halting arms (~7% of masters), and a coverage assert that
    // intermittently reds HEALTHY code erodes trust in real reds. The
    // full run-dst.sh battery always runs far above the floor.
    if scenarios >= 100 {
        assert!(
            total_discrepancies > 0,
            "battery never produced a discrepancy (distribution bug)"
        );
        assert!(
            total_watchdog_rows > 0,
            "battery never fired a watchdog (distribution bug)"
        );
        assert!(total_halts > 0, "battery never halted (distribution bug)");
    }
    if scenarios >= 200 {
        for arm in ARMS {
            assert!(
                arm_hits.get(&arm).copied().unwrap_or(0) > 0,
                "arm {arm:?} never ran in {scenarios} scenarios"
            );
        }
    }
}
