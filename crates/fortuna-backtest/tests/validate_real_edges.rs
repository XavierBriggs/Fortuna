//! W7 (WS4 MONEY-MATH): the `validate` real-edge seam.
//!
//! Written FROM the W7 spec (`docs/superpowers/plans/2026-06-21-ws4-demo-surface.md`)
//! BEFORE the implementation (TDD). These tests prove the seam that connects the
//! REAL replayed track record to `run_sweep`'s deflation toolkit:
//!
//! - the placeholder `EdgeProvider` (empty OOS series → `Insufficient`-by-
//!   construction) is replaced by a real `LedgerEdgeProvider` that assembles the
//!   per-period OOS **Brier-skill** series by scoring the replayed samples
//!   through the SAME `fortuna-scoring` path (G-PARITY);
//! - `run_sweep` threads the provider's per-row label windows + embargo into
//!   `pbo`, so the implemented + mutation-proven purged/embargoed CSCV finally has
//!   a reachable production path;
//! - the G-PIT leak guard at the harness layer drops future-dated beliefs before
//!   they can reach the scored series.
//!
//! The three tests, one requirement each (V&V-mandated shapes):
//!
//! 1. `validate_yields_honest_verdict` — a multi-config replay over a
//!    **pre-registered fixture**. Asserts `fortuna validate` reports WHATEVER
//!    verdict the real Brier math yields (`Go` OR `NoGo` — never a forced verdict)
//!    with `n_logits > 0` AND `effective_n >= 30` AND purge applied. Fails today
//!    (placeholder empty series → `Insufficient`).
//! 2. `purge_bites_directionally` — on a deliberately-leaky fixture, asserts
//!    `purged.pbo > nopurge.pbo + ε` (DIRECTIONAL — purge never UNDERstates
//!    overfitting; mirrors `purged_cscv_bites_on_known_overlap`).
//! 3. `leak_guard_rejects_future_belief` — plants a future-dated belief
//!    (`available_at >= decided_at`) in the seed; asserts `ReplayHarness::replay`'s
//!    strict-PIT rejection counter increments AND the leaked belief never reaches
//!    the scored series.
//!
//! The literal source-name strings used in `event_linkage` here are TEST data
//! (the grep gate only scans `crates/fortuna-backtest/src/`); the provider under
//! test in `src/edge_provider.rs` reads GENERIC scorecards by scope/config, never
//! a source name.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fortuna_backtest::edge_provider::LedgerEdgeProvider;
use fortuna_backtest::harness::{ReplayHarness, TimeRange};
use fortuna_backtest::manifest::{EngagedMarket, UniverseManifest};
use fortuna_backtest::records::{
    BeliefPayload, HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, HistoricalTrade,
    Provenance,
};
use fortuna_backtest::source::{HistoricalSource, SourceError};
use fortuna_backtest::sweep::{run_sweep, EdgeProvider, RecalMethod, SweepParams, TrialSpace};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::money::Cents;
use fortuna_scoring::deflation::{pbo, Duration};
use fortuna_scoring::GoDecision;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Shared fixture scaffolding
// ---------------------------------------------------------------------------

const SCOPE: &str = "weather:KNYC";
const PRODUCER: &str = "forecaster-A";
const CATEGORY: &str = "weather";

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).expect("valid timestamp")
}

/// One synthetic resolved event: a belief, its prior CLV-entry snapshot, and its
/// post-resolution outcome, all on one `event_linkage`.
#[derive(Clone)]
struct Ev {
    linkage: String,
    p: f64,
    available_ms: i64,
    decided_ms: i64,
    snapshot_price: i64,
    snapshot_ms: i64,
    outcome: f64,
    resolved_ms: i64,
}

/// A deterministic in-memory `HistoricalSource`. Performs NO scoring of its own —
/// the provider/harness do the as-of join + scoring, so any divergence in the
/// seam shows up against the pre-registered design.
struct MemSource {
    events: Vec<Ev>,
    /// When true, every engaged market is marked resolved in the manifest so
    /// G-DEAD is satisfied by the scored set. (Pending markets are exempt; we use
    /// resolved markets so the coverage check is exercised honestly.)
    mark_resolved: bool,
}

impl MemSource {
    fn provenance(&self) -> Provenance {
        Provenance {
            producer_type: "forecast".to_string(),
            producer_id: PRODUCER.to_string(),
            mind_id: None,
            mind_version: None,
            strategy_id: "s1".to_string(),
            category: CATEGORY.to_string(),
            scope: SCOPE.to_string(),
        }
    }
}

impl HistoricalSource for MemSource {
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_> {
        let prov = self.provenance();
        Box::new(self.events.iter().map(move |e| {
            Ok(HistoricalBelief {
                provenance: prov.clone(),
                payload: BeliefPayload::Binary { p: e.p },
                event_linkage: e.linkage.clone(),
                available_at: ts(e.available_ms),
                decided_at: ts(e.decided_ms),
            })
        }))
    }

    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_> {
        Box::new(self.events.iter().map(|e| {
            Ok(HistoricalOutcome {
                event_linkage: e.linkage.clone(),
                outcome: e.outcome,
                resolved_at: ts(e.resolved_ms),
                resolution_source: "settlement".to_string(),
            })
        }))
    }

    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_> {
        Box::new(self.events.iter().map(|e| {
            Ok(HistoricalSnapshot {
                market: e.linkage.clone(),
                price: Cents::new(e.snapshot_price),
                at: ts(e.snapshot_ms),
            })
        }))
    }

    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_> {
        Box::new(std::iter::empty())
    }

    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError> {
        let engaged = if self.mark_resolved {
            self.events
                .iter()
                .map(|e| EngagedMarket {
                    event_linkage: e.linkage.clone(),
                    resolved: true,
                    voided: false,
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(UniverseManifest { engaged })
    }
}

/// The trial space the validate seam enumerates over a single scope. Two
/// calibration windows × two recal methods × one GO threshold = 4 configs.
fn trial_space() -> TrialSpace {
    TrialSpace {
        calibration_windows: vec![30, 60],
        recal_methods: vec![RecalMethod::Platt, RecalMethod::None],
        scopes: vec![SCOPE.to_string()],
        go_thresholds: vec![0.05],
    }
}

// ---------------------------------------------------------------------------
// PRE-REGISTERED FIXTURE — skilled vs noise (documented BEFORE running)
// ---------------------------------------------------------------------------
//
// 48 resolved events on one scope, spaced one day apart, every belief strictly
// `available_at < decided_at` (G-PIT-clean), every market resolved (G-DEAD-clean).
//
// The DESIGN (the verdict is a DERIVATION of this, not a knob):
//
//   - The producer is GENUINELY CALIBRATED on this scope: when it says p≈0.8 the
//     event happens; when it says p≈0.2 it does not. Its per-sample Brier loss
//     `(p−o)²` is consistently SMALL.
//   - The market baseline (the de-vigged snapshot-implied probability) is
//     systematically WORSE: it sits near 0.5 (an uninformative book) on every
//     event, so its Brier loss `(0.5−o)²` ≈ 0.25 every time.
//   - Therefore the per-period Brier-skill margin `baseline_loss − model_loss` is
//     POSITIVE and stable across all 48 periods → `brier_edge > 0`,
//     `effective_n ≈ 48 ≥ 30` (white-noise-like, no serial inflation), the
//     IS-best config stays OOS-best so `brier_pbo ≈ 0`, and the SPA on the
//     loss differential is significant (`p_c < α`).
//
//   EXPECTED VERDICT: **Go** (every Brier conjunct holds, N is sufficient).
//
// The test does NOT assert "Go" specifically — it asserts the seam produced a
// READABLE, non-`Insufficient` verdict (n_logits>0, effective_n≥30, purge
// applied) and prints the derived verdict. If the math yielded NoGo the test
// would still pass; the pre-registration documents why Go is the honest result.
fn skilled_fixture() -> MemSource {
    let base = 1_700_000_000_000_i64;
    let day = 86_400_000_i64;
    let mut events = Vec::new();
    for i in 0..48i64 {
        // Alternate YES/NO events; the producer is calibrated (confident + right).
        let yes = i % 2 == 0;
        let outcome = if yes { 1.0 } else { 0.0 };
        let p = if yes { 0.85 } else { 0.15 };
        // The market snapshot is an uninformative ~50c book on every event (the
        // de-vig baseline lands near 0.5 → Brier loss ≈ 0.25, worse than the
        // calibrated model on every period).
        let snapshot_price = 50;
        let decided = base + i * day;
        events.push(Ev {
            linkage: format!("event://forecast/station-KNYC/bracket-{i}/2026-day"),
            p,
            available_ms: decided - day / 2, // strictly before → G-PIT clean
            decided_ms: decided,
            snapshot_price,
            snapshot_ms: decided - 1_000, // strictly before → eligible CLV entry
            outcome,
            resolved_ms: decided + day, // resolves the next day (post-decision)
        });
    }
    MemSource {
        events,
        mark_resolved: true,
    }
}

// ---------------------------------------------------------------------------
// LEAKY FIXTURE — overlapping label windows make purge bite
// ---------------------------------------------------------------------------
//
// Mirrors `purged_cscv_bites_on_known_overlap` (deflation.rs) but at the
// edge-provider layer: sibling rows share an OVERLAPPING label window, so a
// "lucky" config wins in-sample purely because the leaking sibling row sits in
// both the train and the (overlapping) test fold. Purging drops the leaking
// train rows → the lucky config stops looking good OOS → PBO rises. No-purge
// UNDERSTATES overfitting.
//
// The construction (so the leak is a DERIVATION, not a knob):
//
//   - The provider's configs are a temperature-scaling family: config 0 is the
//     SHARPEST (τ<1, pushes confident probabilities toward 0/1); higher configs
//     SHRINK toward 0.5. So configs CROSS on heterogeneous events: the sharp
//     config wins on confident-CORRECT events, the shrink config wins on
//     confident-WRONG events.
//   - The fixture mixes the two event types by CSCV group (s=4 over 40 rows ⇒
//     groups {0..9},{10..19},{20..29},{30..39}). The matrix is sorted by
//     decided_at and decided times are monotonic in `i`, so a row's matrix index
//     equals `i`:
//       * rows 0..19  (groups 0,1): confident-WRONG  → shrink config best;
//       * rows 20..39 (groups 2,3): confident-CORRECT → sharp config best.
//     This makes the IS-best config depend on WHICH groups are the train half —
//     genuine CSCV structure (columns cross), not a degenerate single winner.
//   - The LEAK: ALL the CORRECT rows (20..39) share ONE overlapping label window
//     (a realistic "these brackets all resolved on the same station-day" cluster).
//     The WRONG rows (0..19) carry disjoint windows. Without purge, the sharp
//     config is IS-best whenever the correct cluster is in the train half (its
//     correct rows lift its IS mean) and OOS-best when correct rows are in test —
//     so it ranks consistently. WITH purge, the moment ANY correct row is in the
//     test fold, EVERY train correct row (same window) is purged, leaving the
//     train half all-WRONG → the IS-best flips to the shrink config, which is then
//     OOS-WORST on the held-out correct rows → its OOS rank collapses (λ < 0) far
//     more often → PBO RISES. No-purge UNDERSTATES the overfitting. The
//     provider-layer analogue of `purged_cscv_bites_on_known_overlap`.

fn leaky_fixture() -> MemSource {
    let base = 1_700_000_000_000_i64;
    let day = 86_400_000_i64;
    // The ONE shared label window every correct row resolves within (the
    // same-station-day leak). It is a wide span starting at `base`; every correct
    // row's `available_at` is `base` (strictly before its far-later decided time,
    // so G-PIT admits it) and `resolved_at` is the shared late instant.
    let leak_t0 = base;
    let leak_t1 = base + 50 * day;
    let mut events = Vec::new();
    for i in 0..40i64 {
        // First half MILDLY-WRONG (p=0.55, outcome=NO — sharpening barely hurts),
        // second half STRONGLY-CORRECT (p=0.85, outcome=YES — sharpening helps a
        // lot). The asymmetry makes the SHARP config IS-best on a MIXED train half
        // (so a leak-containing mixed combo selects it) while the SHRINK config is
        // IS-best on an all-WRONG train half (so purging the correct cluster flips
        // the IS-best — the lever that makes purge bite).
        let correct = i >= 20;
        let (p, outcome) = if correct { (0.85, 1.0) } else { (0.55, 0.0) };
        let snapshot_price = 50;
        // Decided times are MONOTONIC in i (so matrix row index == i after the
        // provider's decided_at sort).
        let decided_ms = base + 200 * day + i * day;
        let (available_ms, resolved_ms) = if correct {
            // Every correct row SHARES the one overlapping leak window.
            (leak_t0, leak_t1)
        } else {
            // Tiny, disjoint window straddling this WRONG row's own decided time
            // (far from the leak window and from every other row).
            (decided_ms - 1, decided_ms + 1)
        };
        events.push(Ev {
            linkage: format!("event://forecast/station-KNYC/leaky-{i}/2026-day"),
            p,
            available_ms,
            decided_ms,
            snapshot_price,
            snapshot_ms: available_ms,
            outcome,
            resolved_ms,
        });
    }
    MemSource {
        events,
        mark_resolved: true,
    }
}

// ---------------------------------------------------------------------------
// Test 1 — honest verdict (NOT a forced GO)
// ---------------------------------------------------------------------------

#[test]
fn validate_yields_honest_verdict() {
    // Build the real provider from the pre-registered skilled fixture (pure: the
    // as-of join drops any leak; no DB needed for the series assembly).
    let source = skilled_fixture();
    let provider = LedgerEdgeProvider::from_source(&source, full_range())
        .expect("provider builds from the resolved samples");

    let space = trial_space();
    let params = SweepParams::default();

    // Sanity: the provider's per-(scope,config) series are all the SAME length t,
    // and windows(scope) returns EXACTLY t windows (the length invariant the
    // matrix + purge depend on).
    let t = provider.edges(SCOPE, 0).brier_oos.len();
    assert!(t >= 30, "fixture must yield >= 30 periods; got t={t}");
    for c in 0..space.n_configs() {
        assert_eq!(
            provider.edges(SCOPE, c).brier_oos.len(),
            t,
            "every (scope, config) Brier series must be the SAME length t"
        );
    }
    let (windows, _embargo) = provider.windows(SCOPE);
    assert_eq!(
        windows.len(),
        t,
        "windows(scope) must return EXACTLY t windows (one per period)"
    );
    // A zero-window count would silently take the no-purge path; non-zero here
    // proves purge had a reachable path (the length invariant held, so `run_sweep`
    // passes these windows into `pbo` rather than the no-op no-purge baseline).
    assert!(
        !windows.is_empty(),
        "purge must have a reachable path (non-empty windows)"
    );

    // `run_sweep` consumes the provider by value (P: EdgeProvider); the pre-check
    // values above were captured before the move.
    let run = run_sweep(&space, &params, provider);

    // The seam produced a READABLE verdict over the real replayed track record:
    // NOT Insufficient (the placeholder's only possible output today).
    assert_ne!(
        run.verdict,
        GoDecision::Insufficient,
        "real edges over a 48-period track record must yield a READABLE verdict \
         (Go or NoGo), never Insufficient-by-construction; got brier_edge={} \
         effective_n={} brier_pbo={} brier_spa_p={}",
        run.brier_edge,
        run.effective_n,
        run.brier_pbo,
        run.brier_spa_p,
    );

    // The guard `decide` reads before any verdict: the sample is powered
    // (effective_n >= 30). (n_logits > 0 is implied by a non-Insufficient verdict,
    // since decide returns Insufficient when n_logits == 0.)
    assert!(
        run.effective_n >= 30.0,
        "effective_n must clear the GO floor of 30; got {}",
        run.effective_n
    );

    // The verdict must be one of the real two; print the DERIVED result so the
    // pre-registration is auditable.
    assert!(
        matches!(run.verdict, GoDecision::Go | GoDecision::NoGo),
        "verdict must be the real Brier-math result"
    );
    eprintln!(
        "[validate_yields_honest_verdict] DERIVED verdict = {:?} \
         (brier_edge={:.6}, brier_pbo={:.4}, brier_spa_p={:.4}, effective_n={:.2})",
        run.verdict, run.brier_edge, run.brier_pbo, run.brier_spa_p, run.effective_n
    );
}

// ---------------------------------------------------------------------------
// Test 2 — purge bites DIRECTIONALLY
// ---------------------------------------------------------------------------

#[test]
fn purge_bites_directionally() {
    // Build the real provider from the leaky fixture and assemble its Brier-skill
    // matrix + per-row windows exactly as run_sweep does.
    let source = leaky_fixture();
    let provider = LedgerEdgeProvider::from_source(&source, full_range())
        .expect("provider builds from the leaky samples");

    let space = trial_space();
    let n_configs = space.n_configs();
    let t = provider.edges(SCOPE, 0).brier_oos.len();

    // The T × n_configs Brier-skill matrix (same assembly as run_sweep).
    let matrix: Vec<Vec<f64>> = (0..t)
        .map(|row| {
            (0..n_configs)
                .map(|c| provider.edges(SCOPE, c).brier_oos[row])
                .collect()
        })
        .collect();

    let (windows, embargo) = provider.windows(SCOPE);
    assert_eq!(
        windows.len(),
        t,
        "windows must match the matrix row count (else pbo silently no-purges)"
    );

    let s = 4;
    let purged = pbo(&matrix, s, &windows, embargo);
    let nopurge = pbo(&matrix, s, &[], Duration::zero());

    // DIRECTIONAL: purging must RAISE PBO (expose the leak). A two-sided "differs"
    // is not acceptable — purge can never UNDERstate overfitting.
    assert!(
        purged.pbo > nopurge.pbo + 0.05,
        "purging must raise PBO (expose the same-window leak): purged={} nopurge={}",
        purged.pbo,
        nopurge.pbo
    );
}

// ---------------------------------------------------------------------------
// Test 3 — leak guard at the BELIEF/HARNESS layer (G-PIT)
// ---------------------------------------------------------------------------

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn leak_guard_rejects_future_belief(pool: PgPool) {
    // A clean fixture plus ONE future-dated belief (available_at >= decided_at):
    // the textbook look-ahead leak. The harness owns G-PIT, so its replay must
    // (a) increment look_ahead_rejected and (b) never let the leaked belief reach
    // the scored sample set.
    let mut source = skilled_fixture();
    // The leaked event: available_at AT decided_at (the equality boundary is a
    // leak under the STRICT `<` rule). Its outcome is the OPPOSITE of what its p
    // implies, so if it ever leaked into the scored set it would degrade Brier
    // visibly — a second, content-level guard on top of the counter.
    let decided = source.events[0].decided_ms + 1_000_000;
    let leaked = Ev {
        linkage: "event://forecast/station-KNYC/LEAKED/2026-day".to_string(),
        p: 0.99,
        available_ms: decided, // == decided_ms → look-ahead leak (strict `<`)
        decided_ms: decided,
        snapshot_price: 50,
        snapshot_ms: decided - 1_000,
        outcome: 0.0, // confident-wrong: would crater Brier if it leaked in
        resolved_ms: decided + 86_400_000,
    };
    source.events.push(leaked.clone());
    // The leaked market must be in the manifest too (else G-DEAD would fire on it
    // for an unrelated reason). It IS resolved, so it must appear in scored — but
    // because it is rejected by G-PIT it never produces a scored row. G-DEAD would
    // then flag it; so we mark it as NOT engaged by leaving mark_resolved to build
    // engaged from events — which WOULD include the leaked linkage. To keep this
    // test focused on G-PIT (not G-DEAD), drop the leaked market from the
    // manifest by marking it pending.
    // (Handled in the manifest override below.)

    let clock = SimClock::new(ts(source.events[0].decided_ms - 86_400_000));
    let harness = ReplayHarness::new(pool.clone(), clock, 3);

    let report = harness
        .replay(&LeakManifestSource(source), full_range())
        .await
        .expect("replay must succeed (the leak is rejected, not an error)");

    // (a) The strict-PIT rejection counter incremented for the one leaked belief.
    assert_eq!(
        report.look_ahead_rejected, 1,
        "the future-dated belief (available_at == decided_at) must be rejected + counted"
    );

    // (b) The leaked belief never reached the scored series: the parity scorecard
    // is assembled ONLY from the 48 clean resolved samples, so its n is 48 (the
    // leaked sample is absent). If the leak had reached the scored set, n would be
    // 49 and the confident-wrong sample would have degraded Brier.
    let card = report
        .scorecard
        .expect("clean resolved samples must produce a parity scorecard");
    assert_eq!(
        card.n, 48,
        "the leaked belief must NOT contribute to the scored sample set"
    );
}

/// Wraps a `MemSource` but reports the LEAKED market as pending (not resolved) in
/// the manifest, so the G-PIT-rejected market does not also trip G-DEAD coverage.
/// (G-DEAD is a SEPARATE gate; this test isolates G-PIT.)
struct LeakManifestSource(MemSource);

impl HistoricalSource for LeakManifestSource {
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_> {
        self.0.beliefs()
    }
    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_> {
        self.0.outcomes()
    }
    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_> {
        self.0.snapshots()
    }
    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_> {
        self.0.trades()
    }
    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError> {
        let engaged = self
            .0
            .events
            .iter()
            .map(|e| {
                let leaked = e.linkage.contains("LEAKED");
                EngagedMarket {
                    event_linkage: e.linkage.clone(),
                    // The leaked market is reported PENDING (exempt from G-DEAD
                    // coverage) so this test isolates the G-PIT guard.
                    resolved: !leaked,
                    voided: false,
                }
            })
            .collect();
        Ok(UniverseManifest { engaged })
    }
}

// ---------------------------------------------------------------------------
// Test 4 — noise → NoGo (Phase C Minor, W6b #6)
// ---------------------------------------------------------------------------
//
// PRE-REGISTERED FIXTURE — no-skill producer (documented BEFORE running).
//
// The producer emits p ≈ 0.5 on every event — identical to the baseline
// (the de-vigged snapshot-implied probability, also ~0.5). Therefore:
//
//   - model_loss   ≈ (0.5 − o)² ≈ 0.25 on every period
//   - baseline_loss ≈ (0.5 − o)² ≈ 0.25 on every period
//   - brier_skill  ≈ baseline_loss − model_loss ≈ 0
//
// Over 48 periods this yields `brier_edge ≈ 0` (no skill). With no positive
// edge `decide` returns **NoGo** (the SPA null is not rejected — the model
// is indistinguishable from the baseline).
//
// Why NOT Insufficient: N = 48 ≥ 30 (effective_n clears the threshold), so
// the sweeper has enough data to reach a verdict; it just finds no skill.
// Why NOT Go: brier_edge ≤ 0 fails the primary Brier-skill gate.
//
// The test runs the SAME `run_sweep(LedgerEdgeProvider)` real-provider path
// as `validate_yields_honest_verdict` (G-PARITY: the same code either scores
// skill or doesn't). It closes the loop the demo's central claim rests on:
// the gate refuses when there's no skill AND passes when there is.

fn noise_fixture() -> MemSource {
    // No-skill producer: p ≈ 0.5 on every event (same as the book baseline).
    // The market baseline snapshot is also ~50c, so model ≈ baseline on every
    // period → Brier-skill ≈ 0 throughout.
    let base = 1_700_000_000_000_i64;
    let day = 86_400_000_i64;
    let mut events = Vec::new();
    for i in 0..48i64 {
        let yes = i % 2 == 0;
        let outcome = if yes { 1.0 } else { 0.0 };
        // Producer probability ≈ 0.5 on every event (no-skill, matches baseline).
        let p = 0.5;
        // Snapshot also ~50c (the book baseline is ~uniform / uninformative).
        let snapshot_price = 50;
        let decided = base + i * day;
        events.push(Ev {
            linkage: format!("event://noise/station-KNYC/noise-{i}/2026-day"),
            p,
            available_ms: decided - day / 2,
            decided_ms: decided,
            snapshot_price,
            snapshot_ms: decided - 1_000,
            outcome,
            resolved_ms: decided + day,
        });
    }
    MemSource {
        events,
        mark_resolved: true,
    }
}

/// No-skill (noise) fixture through the FULL `run_sweep(LedgerEdgeProvider)`
/// real-provider path must yield `NoGo` — not `Insufficient` (N is sufficient),
/// not `Go` (brier_edge ≤ 0, no skill). Closes the demo's central claim: the
/// gate passes on skill AND refuses on noise, both via the same real path.
#[test]
fn noise_producer_yields_nogo_not_insufficient() {
    let source = noise_fixture();
    let provider = LedgerEdgeProvider::from_source(&source, full_range())
        .expect("provider builds from the resolved noise samples");

    let space = trial_space();
    let params = SweepParams::default();

    // Verify the fixture provides enough periods for the N guard.
    let t = provider.edges(SCOPE, 0).brier_oos.len();
    assert!(
        t >= 30,
        "noise fixture must yield >= 30 periods so the verdict can be NoGo (not Insufficient); got t={t}"
    );

    let run = run_sweep(&space, &params, provider);

    // The verdict must be NoGo — the gate fires correctly on a no-skill producer.
    assert_eq!(
        run.verdict,
        GoDecision::NoGo,
        "a no-skill producer (p≈0.5 = baseline) must yield NoGo through the real provider path; \
         got verdict={:?} brier_edge={:.6} effective_n={:.2} brier_pbo={:.4} brier_spa_p={:.4}",
        run.verdict,
        run.brier_edge,
        run.effective_n,
        run.brier_pbo,
        run.brier_spa_p,
    );

    // Corroborate: the sample was powered (not Insufficient due to thin N).
    assert!(
        run.effective_n >= 30.0,
        "effective_n must clear 30 so NoGo is a powered refusal, not a thin-data deferral; \
         got effective_n={}",
        run.effective_n,
    );

    eprintln!(
        "[noise_producer_yields_nogo_not_insufficient] DERIVED verdict = {:?} \
         (brier_edge={:.6}, brier_pbo={:.4}, brier_spa_p={:.4}, effective_n={:.2})",
        run.verdict, run.brier_edge, run.brier_pbo, run.brier_spa_p, run.effective_n
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn full_range() -> TimeRange {
    TimeRange {
        from: ts(0),
        to: ts(i64::from(u32::MAX) * 100_000),
    }
}

// ---------------------------------------------------------------------------
// Gate-proof: the run_sweep length-invariant assertion catches a ragged provider
// ---------------------------------------------------------------------------
//
// The whole point of the W7 length invariant: `pbo` only purges when
// `windows.len() == t`, so a provider that returns a NON-EMPTY but wrong-length
// window list would silently take the no-purge path and UNDERSTATE overfitting.
// `run_sweep` must FAIL LOUDLY (assert) rather than ship a no-purge run that
// masquerades as purged. This test plants exactly that violation and proves the
// assertion fires — a permanent guard against the assertion being weakened or
// dropped (verification-methodology: prove the gate catches a planted violation).
struct RaggedProvider;
impl EdgeProvider for RaggedProvider {
    fn edges(&self, _scope: &str, _c: usize) -> fortuna_backtest::sweep::ConfigEdges {
        fortuna_backtest::sweep::ConfigEdges {
            brier_oos: vec![0.1, 0.2, 0.3, 0.4],
            brier_loss_diff: vec![0.1, 0.2, 0.3, 0.4],
            clv_oos: vec![0.0; 4],
            sharpe_returns: vec![0.1, 0.2, 0.3, 0.4],
        }
    }
    fn windows(&self, _scope: &str) -> (Vec<fortuna_scoring::deflation::LabelWindow>, Duration) {
        // WRONG length: 2 windows for a 4-row matrix → must trip the assertion.
        (
            vec![fortuna_scoring::deflation::LabelWindow::new(0, 1); 2],
            Duration::zero(),
        )
    }
}

#[test]
#[should_panic(expected = "EXACTLY t windows")]
fn ragged_windows_trip_the_loud_assertion() {
    let space = trial_space();
    let params = SweepParams::default();
    let _ = run_sweep(&space, &params, RaggedProvider);
}
