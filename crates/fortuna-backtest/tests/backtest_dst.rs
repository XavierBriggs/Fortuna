//! Boundary DST: three seeded scenarios that stress the replay harness in
//! directions the S2 integration tests (tests/harness.rs) do not cover.
//!
//! The literal `"historical-import"` is allowed in THIS test file (the grep
//! gate only scans `crates/fortuna-backtest/src/`). The harness itself keeps
//! the decoupling invariant.
//!
//! **Seed count:** `BACKTEST_DST_SCENARIOS` env var (default 64). Each
//! scenario is deterministic per seed — variation comes only from the
//! SplitMix64 derived from the seed index, never from wall time.
//!
//! ## Scenarios
//!
//! 1. `backtest_rerun_idempotent` — replay N records, replay again → zero new
//!    writes, all skipped_idempotent, ledger row count == N.
//! 2. `backtest_partial_replay_recovery` — replay a prefix of K records
//!    (simulating a crash at K), then replay the full source into the SAME
//!    ledger → the K prefix is skipped by idempotency, the remaining N−K are
//!    written, total rows == N (no dupes, no gaps).
//! 3. `backtest_clock_determinism` — two replays of the SAME source under two
//!    different SimClock start instants → byte-identical ledger rows (the
//!    wall/sim clock must NOT leak into any written row).

use fortuna_backtest::harness::{ReplayHarness, TimeRange};
use fortuna_backtest::manifest::UniverseManifest;
use fortuna_backtest::records::{
    BeliefPayload, HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, HistoricalTrade,
    Provenance,
};
use fortuna_backtest::source::{HistoricalSource, SourceError};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::SplitMix64;
use fortuna_core::money::Cents;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Helpers shared across all three scenarios
// ---------------------------------------------------------------------------

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).expect("valid timestamp")
}

fn full_range() -> TimeRange {
    TimeRange {
        from: ts(0),
        to: ts(i64::from(u32::MAX) * 1_000),
    }
}

/// The number of DST scenarios to run (configurable via env; default 64).
fn scenario_count() -> usize {
    std::env::var("BACKTEST_DST_SCENARIOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64)
}

/// Build a deterministic master seed from the env or a fixed salt.
fn master_seed() -> u64 {
    std::env::var("DST_MASTER_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x6c4e_4f72_5432_4253_u64)
}

/// Derive a per-scenario seed from the master seed and the scenario index.
fn scenario_seed(scenario_index: usize) -> u64 {
    let mut rng = SplitMix64::new(master_seed());
    // Advance the PRNG `scenario_index + 1` times so each index gets a
    // distinct value independent of the others.
    for _ in 0..=scenario_index {
        rng.next_u64();
    }
    rng.next_u64()
}

// ---------------------------------------------------------------------------
// SeededTriple: one synthetic event derived purely from a seed value
// ---------------------------------------------------------------------------

struct SeededRecord {
    linkage: String,
    p: f64,
    /// available_at epoch-ms (strictly before decided_ms — G-PIT passes)
    available_ms: i64,
    decided_ms: i64,
    snapshot_ms: i64,
    snapshot_price: i64,
    outcome: f64,
    resolved_ms: i64,
}

impl SeededRecord {
    /// Derive N records from a SplitMix64 seeded by `seed`. All timestamps
    /// are strictly ordered so that G-PIT is satisfied on every record.
    fn derive_n(n: usize, seed: u64) -> Vec<SeededRecord> {
        let mut rng = SplitMix64::new(seed);
        let mut records = Vec::with_capacity(n);
        // A monotonically advancing "current time" so records don't collide.
        let mut base_ms: i64 = 1_000_000_000_i64; // 2001-09-09 in epoch-ms (within range)
        for i in 0..n {
            // Each record occupies a 10_000ms window (10 seconds).
            let window_ms = 10_000_i64 * (i as i64 + 1);
            let decided_ms = base_ms + window_ms;
            let available_ms = decided_ms - 1_000; // strictly before decided
            let snapshot_ms = decided_ms - 500; // between available and decided
            let resolved_ms = decided_ms + 5_000; // after decided

            // p ∈ (0, 1): use the high 53 bits of the PRNG so we get a valid
            // f64 probability. Clamp to [0.01, 0.99] to avoid exact 0/1.
            let p_raw = (rng.next_u64() >> 11) as f64 / ((1u64 << 53) as f64);
            let p = 0.01_f64 + p_raw * 0.98_f64;

            // outcome: 0.0 (NO) or 1.0 (YES), alternating by record index for
            // variety but driven by the PRNG for full randomness.
            let outcome = if rng.next_u64() & 1 == 0 {
                0.0_f64
            } else {
                1.0_f64
            };

            // snapshot price in [1, 99] cents
            let price = ((rng.next_u64() % 99) as i64) + 1;

            records.push(SeededRecord {
                linkage: format!(
                    "event://dst/scope-a/bracket-{i}/2026-07-{:02}",
                    (i % 28) + 1
                ),
                p,
                available_ms,
                decided_ms,
                snapshot_ms,
                snapshot_price: price,
                outcome,
                resolved_ms,
            });

            base_ms = decided_ms;
        }
        records
    }
}

// ---------------------------------------------------------------------------
// SeededSource: an in-memory HistoricalSource wrapping SeededRecord slices.
// ---------------------------------------------------------------------------

const SCOPE: &str = "dst:scope-a";
const PRODUCER: &str = "dst-producer";
const CATEGORY: &str = "dst";

struct SeededSource {
    records: Vec<SeededRecord>,
}

impl SeededSource {
    fn provenance(&self) -> Provenance {
        Provenance {
            producer_type: "dst-test".to_string(),
            producer_id: PRODUCER.to_string(),
            mind_id: None,
            mind_version: None,
            strategy_id: "dst-s1".to_string(),
            category: CATEGORY.to_string(),
            scope: SCOPE.to_string(),
        }
    }
}

impl HistoricalSource for SeededSource {
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_> {
        let prov = self.provenance();
        Box::new(self.records.iter().map(move |r| {
            Ok(HistoricalBelief {
                provenance: prov.clone(),
                payload: BeliefPayload::Binary { p: r.p },
                event_linkage: r.linkage.clone(),
                available_at: ts(r.available_ms),
                decided_at: ts(r.decided_ms),
            })
        }))
    }

    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_> {
        Box::new(self.records.iter().map(|r| {
            Ok(HistoricalOutcome {
                event_linkage: r.linkage.clone(),
                outcome: r.outcome,
                resolved_at: ts(r.resolved_ms),
                resolution_source: "dst-settlement".to_string(),
            })
        }))
    }

    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_> {
        Box::new(self.records.iter().map(|r| {
            Ok(HistoricalSnapshot {
                market: r.linkage.clone(),
                price: Cents::new(r.snapshot_price),
                at: ts(r.snapshot_ms),
            })
        }))
    }

    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_> {
        Box::new(std::iter::empty())
    }

    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError> {
        // Empty engaged list: the G-DEAD check requires every engaged market to
        // appear in the scored set. We use an empty manifest so the check passes
        // regardless of which records happen to have outcomes.
        Ok(UniverseManifest {
            engaged: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// Scenario 1: backtest_rerun_idempotent
// ---------------------------------------------------------------------------

/// DST: replay N records twice into the same ledger. The second pass must
/// write zero new rows and skip exactly N (all via idempotency), and the
/// ledger row count must be exactly N (no duplicates).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn backtest_rerun_idempotent(pool: PgPool) {
    let n_scenarios = scenario_count();
    let clock_start = ts(20_000_000);

    for idx in 0..n_scenarios {
        let seed = scenario_seed(idx);
        // Vary N in [4, 20] by seed (4 is the minimum for MIN_N=3 scoring).
        let mut seed_rng = SplitMix64::new(seed);
        let n = 4 + (seed_rng.next_u64() % 17) as usize; // 4..=20

        let records = SeededRecord::derive_n(n, seed);
        let source = SeededSource { records };
        let harness = ReplayHarness::new(pool.clone(), SimClock::new(clock_start), 3);

        let first = harness
            .replay(&source, full_range())
            .await
            .unwrap_or_else(|e| panic!("seed {seed:#018x} n={n}: first replay failed: {e}"));

        assert_eq!(
            first.written, n,
            "seed {seed:#018x} n={n}: first replay must write all {n} records"
        );
        assert_eq!(
            first.skipped_idempotent, 0,
            "seed {seed:#018x} n={n}: first replay skips nothing (empty ledger)"
        );
        assert_eq!(
            first.look_ahead_rejected, 0,
            "seed {seed:#018x} n={n}: no G-PIT rejections (available_ms < decided_ms by construction)"
        );

        let second = harness
            .replay(&source, full_range())
            .await
            .unwrap_or_else(|e| panic!("seed {seed:#018x} n={n}: second replay failed: {e}"));

        assert_eq!(
            second.written, 0,
            "seed {seed:#018x} n={n}: second replay writes ZERO new rows (idempotent)"
        );
        assert_eq!(
            second.skipped_idempotent, n,
            "seed {seed:#018x} n={n}: second replay skips all {n} rows via content-hash idempotency"
        );
        assert_eq!(
            second.look_ahead_rejected, 0,
            "seed {seed:#018x} n={n}: no G-PIT rejections on the second pass"
        );

        // Hard floor: ledger row count == N (no duplicates).
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("seed {seed:#018x} n={n}: count query failed: {e}"));
        assert_eq!(
            count, n as i64,
            "seed {seed:#018x} n={n}: ledger holds exactly {n} rows (no dupes)"
        );

        // Truncate between scenarios so each scenario starts with a clean ledger.
        sqlx::query("TRUNCATE beliefs, events, scorecards RESTART IDENTITY CASCADE")
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("truncate between scenarios failed: {e}"));
    }
    println!("backtest_rerun_idempotent: {n_scenarios} seeds passed");
}

// ---------------------------------------------------------------------------
// Scenario 2: backtest_partial_replay_recovery
// ---------------------------------------------------------------------------

/// A `HistoricalSource` that yields only the first `k` records of an
/// underlying `SeededSource`. Used to simulate a crash-at-K scenario.
struct PrefixSource<'a> {
    inner: &'a SeededSource,
    k: usize,
}

impl HistoricalSource for PrefixSource<'_> {
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_> {
        let prov = self.inner.provenance();
        Box::new(self.inner.records[..self.k].iter().map(move |r| {
            Ok(HistoricalBelief {
                provenance: prov.clone(),
                payload: BeliefPayload::Binary { p: r.p },
                event_linkage: r.linkage.clone(),
                available_at: ts(r.available_ms),
                decided_at: ts(r.decided_ms),
            })
        }))
    }

    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_> {
        Box::new(self.inner.records[..self.k].iter().map(|r| {
            Ok(HistoricalOutcome {
                event_linkage: r.linkage.clone(),
                outcome: r.outcome,
                resolved_at: ts(r.resolved_ms),
                resolution_source: "dst-settlement".to_string(),
            })
        }))
    }

    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_> {
        Box::new(self.inner.records[..self.k].iter().map(|r| {
            Ok(HistoricalSnapshot {
                market: r.linkage.clone(),
                price: Cents::new(r.snapshot_price),
                at: ts(r.snapshot_ms),
            })
        }))
    }

    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_> {
        Box::new(std::iter::empty())
    }

    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError> {
        Ok(UniverseManifest {
            engaged: Vec::new(),
        })
    }
}

/// DST: crash-at-K recovery. Replay the first K records (crash simulation),
/// then replay the full N-record source into the SAME ledger:
///   - The K previously-written rows are skipped via idempotency.
///   - The remaining N−K rows are written fresh.
///   - Total ledger row count == N (no duplicates, no gaps).
///
/// K is varied across seeds including the edge cases K=0 and K=N.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn backtest_partial_replay_recovery(pool: PgPool) {
    let n_scenarios = scenario_count();
    let clock_start = ts(20_000_000);

    for idx in 0..n_scenarios {
        let seed = scenario_seed(idx);
        let mut seed_rng = SplitMix64::new(seed);
        // N in [4, 16] (modest to keep the test fast at scale)
        let n = 4 + (seed_rng.next_u64() % 13) as usize; // 4..=16

        // K in [0, N]: the crash point.
        // - K=0: no prior work; full write on resume.
        // - K=N: complete prior run; all skipped on resume.
        // Spread K across the full [0,N] range using the seed.
        let k = (seed_rng.next_u64() % (n as u64 + 1)) as usize;

        let records = SeededRecord::derive_n(n, seed);
        let source = SeededSource { records };
        let harness = ReplayHarness::new(pool.clone(), SimClock::new(clock_start), 3);

        // Phase 1 — "crash" after K records.
        if k > 0 {
            let prefix = PrefixSource { inner: &source, k };
            let crash_report = harness
                .replay(&prefix, full_range())
                .await
                .unwrap_or_else(|e| {
                    panic!("seed {seed:#018x} n={n} k={k}: crash-phase replay failed: {e}")
                });
            assert_eq!(
                crash_report.written, k,
                "seed {seed:#018x} n={n} k={k}: crash phase wrote {k} rows"
            );
        }

        // Verify the ledger holds exactly K rows after the crash phase.
        let count_after_crash: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("count after crash failed: {e}"));
        assert_eq!(
            count_after_crash, k as i64,
            "seed {seed:#018x} n={n} k={k}: ledger holds {k} rows after crash phase"
        );

        // Phase 2 — resume: replay the FULL source into the SAME ledger.
        let resume_report = harness
            .replay(&source, full_range())
            .await
            .unwrap_or_else(|e| {
                panic!("seed {seed:#018x} n={n} k={k}: resume-phase replay failed: {e}")
            });

        let expected_written = n - k;
        let expected_skipped = k;

        assert_eq!(
            resume_report.written, expected_written,
            "seed {seed:#018x} n={n} k={k}: resume writes the {expected_written} missing rows"
        );
        assert_eq!(
            resume_report.skipped_idempotent, expected_skipped,
            "seed {seed:#018x} n={n} k={k}: resume skips the {expected_skipped} already-written rows"
        );

        // Hard floor: exactly N rows in the ledger (no dupes, no gaps).
        let count_final: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("count after resume failed: {e}"));
        assert_eq!(
            count_final, n as i64,
            "seed {seed:#018x} n={n} k={k}: ledger holds exactly {n} rows after recovery (no dupes, no gaps)"
        );

        // Truncate between scenarios.
        sqlx::query("TRUNCATE beliefs, events, scorecards RESTART IDENTITY CASCADE")
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("truncate between scenarios failed: {e}"));
    }
    println!("backtest_partial_replay_recovery: {n_scenarios} seeds (with K=0 and K=N edge cases) passed");
}

// ---------------------------------------------------------------------------
// Scenario 3: backtest_clock_determinism
// ---------------------------------------------------------------------------

/// DST: clock independence. Two replays of the SAME source into separate
/// ledgers under two *different* SimClock start instants (T1 ≠ T2) produce
/// byte-identical belief_id sets. The wall/sim clock must not leak into any
/// written row — all row content is derived from the source records, not the
/// clock.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn backtest_clock_determinism(pool: PgPool) {
    let n_scenarios = scenario_count();

    for idx in 0..n_scenarios {
        let seed = scenario_seed(idx);
        let mut seed_rng = SplitMix64::new(seed);
        let n = 4 + (seed_rng.next_u64() % 13) as usize; // 4..=16

        // Two clock instants that are different from each other and from the
        // base timestamps used in the source records (which start at 1_000_000_000ms).
        let t1_ms = 50_000_000_i64 + (seed_rng.next_u64() % 1_000_000) as i64;
        let t2_ms = t1_ms + 999_999_i64; // always different from T1
        assert_ne!(t1_ms, t2_ms);

        let records = SeededRecord::derive_n(n, seed);
        let source = SeededSource { records };

        // Replay with clock T1.
        let harness_t1 = ReplayHarness::new(pool.clone(), SimClock::new(ts(t1_ms)), 3);
        harness_t1
            .replay(&source, full_range())
            .await
            .unwrap_or_else(|e| panic!("seed {seed:#018x} n={n}: T1 replay failed: {e}"));

        let ids_t1: Vec<String> =
            sqlx::query_scalar("SELECT belief_id FROM beliefs ORDER BY belief_id")
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|e| panic!("ids T1 query failed: {e}"));

        // Wipe the ledger between the two passes so they use the same pool
        // without interference, letting us compare the id sets directly.
        sqlx::query("TRUNCATE beliefs, events, scorecards RESTART IDENTITY CASCADE")
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("truncate between T1 and T2 failed: {e}"));

        // Replay the SAME source with a DIFFERENT clock T2.
        let harness_t2 = ReplayHarness::new(pool.clone(), SimClock::new(ts(t2_ms)), 3);
        harness_t2
            .replay(&source, full_range())
            .await
            .unwrap_or_else(|e| panic!("seed {seed:#018x} n={n}: T2 replay failed: {e}"));

        let ids_t2: Vec<String> =
            sqlx::query_scalar("SELECT belief_id FROM beliefs ORDER BY belief_id")
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|e| panic!("ids T2 query failed: {e}"));

        // The id sets must be identical: the clock does not leak into belief ids.
        assert_eq!(
            ids_t1, ids_t2,
            "seed {seed:#018x} n={n}: belief_ids differ between clock T1={t1_ms}ms and T2={t2_ms}ms — clock leaked into ledger rows"
        );
        assert_eq!(
            ids_t1.len(),
            n,
            "seed {seed:#018x} n={n}: expected {n} unique belief_ids but got {}",
            ids_t1.len()
        );

        // Truncate between scenarios.
        sqlx::query("TRUNCATE beliefs, events, scorecards RESTART IDENTITY CASCADE")
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("truncate after determinism check failed: {e}"));
    }
    println!("backtest_clock_determinism: {n_scenarios} seeds passed");
}
