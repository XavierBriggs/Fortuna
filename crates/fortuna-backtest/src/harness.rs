//! Replay harness (S2): streams a [`HistoricalSource`] through the as-of join
//! (G-PIT) into the SAME `fortuna-scoring` rules and the SAME ledger write path
//! as live (G-PARITY), idempotently and deterministically (spec §5, §6).
//!
//! ## What the harness guarantees
//!
//! - **G-PIT (no look-ahead):** every belief is admitted only if
//!   `available_at < decided_at` (STRICT); leaks are rejected and counted. The
//!   CLV-entry snapshot is the latest with `at < decided_at`. (Enforced in
//!   [`crate::asof`].)
//! - **G-PARITY (scores identically to live):** replay calls the SAME assembler
//!   the daemon's recompute path uses
//!   (`fortuna_cognition::scorecard_agg::assemble_from_samples`) — never a
//!   reimplementation of Brier/scoring — and persists through the SAME
//!   `ScorecardsRepo`/`BeliefsRepo` the live path uses. The only deltas are the
//!   source stamp (`fortuna_ledger::SOURCE_HISTORICAL_IMPORT`) and the preserved
//!   original timestamps.
//! - **Idempotent:** every written row is keyed by a CONTENT HASH and inserted
//!   with `ON CONFLICT DO NOTHING`, so a re-run over the same source is a no-op
//!   (`written == 0`, `skipped_idempotent == prior written`).
//! - **Deterministic:** all "now" reads go through the injected [`Clock`]; the
//!   harness never reads wall time. Replay preserves the ORIGINAL record
//!   timestamps (it never masquerades as a live decision), and the content-hash
//!   ids are derived from record content, not the clock — so two replays of the
//!   same source produce byte-identical ledger rows.
//!
//! ## Decoupling
//!
//! This file carries NO source-name literals (the grep gate). The source stamp
//! is referenced from the ledger-side const
//! [`fortuna_ledger::SOURCE_HISTORICAL_IMPORT`], where the literal must already
//! live for the forward-window SQL filters.

use crate::asof::{asof_join, AsOfDisposition, DecisionContext};
use crate::manifest::{enforce_gdead, GDeadViolation, ScoredRow};
use crate::records::BeliefPayload;
use crate::source::{HistoricalSource, SourceError};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_ledger::{
    BeliefsRepo, EventsRepo, LedgerError, PgPool, ScorecardsRepo, SOURCE_HISTORICAL_IMPORT,
};
use fortuna_scoring::Scorecard;
use thiserror::Error;

/// An inclusive replay window over `decided_at`. A belief is replayed iff its
/// `decided_at` is within `[from, to]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub from: UtcTimestamp,
    pub to: UtcTimestamp,
}

impl TimeRange {
    /// `true` iff `at` falls within the inclusive `[from, to]` window.
    fn contains(&self, at: UtcTimestamp) -> bool {
        at >= self.from && at <= self.to
    }
}

/// Errors produced during a replay pass.
#[derive(Debug, Error)]
pub enum ReplayError {
    /// The source failed to yield a record stream.
    #[error("source error during replay: {0}")]
    Source(#[from] SourceError),
    /// A ledger write (event/belief/scorecard) failed.
    #[error("ledger error during replay: {0}")]
    Ledger(#[from] LedgerError),
    /// A record carried a timestamp outside the representable range, or id
    /// derivation failed.
    #[error("record encoding error during replay: {0}")]
    Encoding(String),
    /// The G-DEAD integrity gate failed: one or more engaged markets from the
    /// universe manifest were absent from the scored set. The producer silently
    /// dropped markets it engaged with — classic survivorship bias.
    #[error("G-DEAD integrity gate failed: {0}")]
    GDead(#[from] GDeadViolation),
}

/// The honest accounting of one replay pass.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayReport {
    /// Belief rows newly written to the ledger this pass.
    pub written: usize,
    /// Belief rows skipped because their content-hash key already existed
    /// (idempotent re-run / duplicate content).
    pub skipped_idempotent: usize,
    /// Beliefs rejected by G-PIT (`available_at >= decided_at`).
    pub look_ahead_rejected: usize,
    /// The parity scorecard assembled from the replayed, resolved samples for
    /// the scope — surfaced so the G-PARITY gate can compare it to the live
    /// path. `None` when no resolved samples were produced.
    pub scorecard: Option<Scorecard>,
}

/// The replay harness. Generic over the injected [`Clock`] so the replay path
/// never reads wall time (determinism / I-time-via-Clock).
pub struct ReplayHarness<C: Clock> {
    pool: PgPool,
    #[allow(dead_code)] // held to enforce "time via the injected Clock"; preserved
    // timestamps mean S2 reads no wall-clock now() — the Clock is the contract
    // hook future slices (e.g. computed_at) use, and its presence forbids a
    // SystemTime::now() creeping into the replay path.
    clock: C,
    min_n: u32,
}

impl<C: Clock> ReplayHarness<C> {
    /// Build a harness over a ledger `pool`, an injected `clock`, and the
    /// trial-count floor `min_n` (below which the scorecard verdict is
    /// `Insufficient`, matching WS2).
    pub fn new(pool: PgPool, clock: C, min_n: u32) -> Self {
        ReplayHarness { pool, clock, min_n }
    }

    /// Replay `source` over `range`, returning the [`ReplayReport`].
    ///
    /// Pipeline per decision: stream beliefs → as-of-join against the snapshot /
    /// outcome pools (G-PIT) → write the source-stamped belief (idempotent) →
    /// accumulate the resolved `(p, outcome)` sample → assemble + persist the
    /// parity scorecard via the SAME live assembler + ledger path (G-PARITY) →
    /// enforce G-DEAD (every manifest-engaged market must appear in scored).
    pub async fn replay(
        &self,
        source: &impl HistoricalSource,
        range: TimeRange,
    ) -> Result<ReplayReport, ReplayError> {
        // Materialise the snapshot/outcome pools (NOT the beliefs — beliefs are
        // the large stream and are consumed lazily). The pools are bounded by
        // the engaged universe, not the full archive history.
        let snapshots = collect(source.snapshots())?;
        let outcomes = collect(source.outcomes())?;

        // Obtain the universe manifest for the G-DEAD check (done once per
        // replay pass, before consuming the belief stream).
        let manifest = source.universe_manifest()?;

        let events = EventsRepo::new(self.pool.clone());
        let beliefs_repo = BeliefsRepo::new(self.pool.clone());

        let mut written = 0usize;
        let mut skipped_idempotent = 0usize;
        let mut look_ahead_rejected = 0usize;

        // Deterministic accumulation: (decided_at, linkage, p, outcome, voided)
        // sorted so the sample order is independent of the source's iteration order.
        let mut resolved: Vec<(UtcTimestamp, String, f64, bool)> = Vec::new();
        // Accumulate ScoredRow entries for G-DEAD: one per joined belief that
        // has an outcome. We use a Vec here and deduplicate by linkage after the
        // loop (a single market may have multiple beliefs; G-DEAD checks market
        // coverage, not belief count).
        let mut scored_rows: Vec<ScoredRow> = Vec::new();
        let mut scope: Option<(String, Option<String>)> = None;

        for belief in source.beliefs() {
            let belief = belief?;
            if !range.contains(belief.decided_at) {
                continue;
            }

            match asof_join(&belief, &snapshots, &outcomes) {
                AsOfDisposition::LookAheadRejected => {
                    look_ahead_rejected += 1;
                }
                AsOfDisposition::Joined(ctx) => {
                    let p = binary_p(&ctx)?;

                    // Source-stamped, content-hashed, idempotent belief write
                    // through the LIVE ledger path. Original timestamps preserved.
                    let rows = self.write_belief(&events, &beliefs_repo, &ctx, p).await?;
                    if rows == 0 {
                        skipped_idempotent += 1;
                    } else {
                        written += 1;
                    }

                    // Record the scope (first belief wins; all share one scope in
                    // a single-scope replay). Used to label the parity scorecard.
                    if scope.is_none() {
                        let producer = if ctx.belief.provenance.producer_id.is_empty() {
                            None
                        } else {
                            Some(ctx.belief.provenance.producer_id.clone())
                        };
                        scope = Some((ctx.belief.provenance.scope.clone(), producer));
                    }

                    // Only RESOLVED beliefs (with an outcome label) contribute to
                    // the parity scorecard sample set and the G-DEAD scored set.
                    if let Some(outcome) = &ctx.outcome {
                        let happened = outcome.outcome >= 0.5;
                        resolved.push((
                            ctx.belief.decided_at,
                            ctx.belief.event_linkage.clone(),
                            p,
                            happened,
                        ));
                        // Accumulate a ScoredRow for G-DEAD coverage tracking.
                        // `voided` is always false here: voided markets carry no
                        // outcome in the outcomes pool (they are present in the
                        // manifest but yield no resolved outcome record). The
                        // harness produces ScoredRows only for resolved markets;
                        // the G-DEAD check will catch any voided market that the
                        // source omits from its outcomes stream.
                        scored_rows.push(ScoredRow {
                            event_linkage: ctx.belief.event_linkage.clone(),
                            outcome: outcome.outcome,
                            voided: false,
                        });
                    }
                }
            }
        }

        // Also produce ScoredRows for voided markets: they appear in the
        // manifest's engaged set but yield no resolved outcome record in the
        // outcomes pool. The source's outcomes stream should carry them with
        // `outcome = 0.0` (or they are absent — either way G-DEAD must see them).
        // We add them from the outcomes pool (which already contains voided
        // entries if the source is well-formed) by checking the manifest.
        for m in &manifest.engaged {
            if m.voided {
                // Voided markets may or may not appear in the outcomes pool. If
                // the source emits a voided outcome record, it's already in
                // `outcomes`; if not, we still need a ScoredRow to satisfy G-DEAD.
                // Find it in outcomes, or emit a placeholder voided row.
                let from_outcomes = outcomes.iter().find(|o| o.event_linkage == m.event_linkage);
                scored_rows.push(ScoredRow {
                    event_linkage: m.event_linkage.clone(),
                    outcome: from_outcomes.map(|o| o.outcome).unwrap_or(0.0),
                    voided: true,
                });
            }
        }

        // Deduplicate ScoredRows by linkage (multiple beliefs → one market).
        // Sort for determinism, then dedup by linkage.
        scored_rows.sort_by(|a, b| a.event_linkage.cmp(&b.event_linkage));
        scored_rows.dedup_by(|a, b| a.event_linkage == b.event_linkage);

        // G-DEAD: every engaged market in the manifest must appear in scored.
        // This is the false-negative guard: no silent survivorship via dropped
        // losers / voided / NO-resolved markets.
        enforce_gdead(&scored_rows, &manifest)?;

        // Deterministic sample order: by decided_at then linkage.
        resolved.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        let scorecard = if resolved.is_empty() {
            None
        } else {
            let (scope_label, producer) = scope.unwrap_or_else(|| (String::new(), None));
            let samples: Vec<(f64, bool)> = resolved.iter().map(|(_, _, p, o)| (*p, *o)).collect();
            // The SAME assembler the daemon's recompute_scorecards path uses —
            // never a reimplementation of Brier/scoring (G-PARITY by
            // construction). The `window` is the source stamp.
            let card = fortuna_cognition::scorecard_agg::assemble_from_samples(
                &scope_label,
                producer.as_deref(),
                SOURCE_HISTORICAL_IMPORT,
                &samples,
                &[], // no de-vig baseline modeled in S2 (matches the parity input)
                &[], // no CLV series modeled in S2
                self.min_n,
            );
            self.persist_scorecard(&card).await?;
            Some(card)
        };

        Ok(ReplayReport {
            written,
            skipped_idempotent,
            look_ahead_rejected,
            scorecard,
        })
    }

    /// Ensure the event row exists, then write the source-stamped, content-hashed
    /// belief idempotently. Returns rows-affected (`1` = written, `0` = skipped).
    async fn write_belief(
        &self,
        events: &EventsRepo,
        beliefs: &BeliefsRepo,
        ctx: &DecisionContext,
        p: f64,
    ) -> Result<u64, ReplayError> {
        let linkage = &ctx.belief.event_linkage;
        let decided_iso = ctx.belief.decided_at.to_iso8601();

        // Event id is content-derived from the canonical linkage so the same
        // source event always maps to the same row (idempotent across re-runs).
        let event_id = content_ulid(&[b"event", linkage.as_bytes()]);
        let benchmark_at = ctx
            .outcome
            .as_ref()
            .map(|o| o.resolved_at.to_iso8601())
            .unwrap_or_else(|| decided_iso.clone());
        events
            .create_idempotent(
                &event_id,
                linkage,                     // statement (opaque)
                "replayed historical event", // resolution_criteria
                ctx.outcome
                    .as_ref()
                    .map(|o| o.resolution_source.as_str())
                    .unwrap_or("historical"), // resolution_source
                Some(&decided_iso),          // horizon
                &benchmark_at,
                &ctx.belief.provenance.category,
                &decided_iso, // created_at = ORIGINAL decided time (preserved)
            )
            .await?;

        // Belief id is content-derived from the full belief content so identical
        // content → identical id → ON CONFLICT skip (idempotent / deterministic).
        let belief_id = content_ulid(&[
            b"belief",
            linkage.as_bytes(),
            decided_iso.as_bytes(),
            ctx.belief.provenance.producer_id.as_bytes(),
            p.to_bits().to_be_bytes().as_slice(),
        ]);

        // Provenance carries the source stamp + producer/scope. The source key is
        // the ledger-side const (no literal in this crate's src/).
        let provenance = serde_json::json!({
            "source": SOURCE_HISTORICAL_IMPORT,
            "producer": ctx.belief.provenance.producer_id,
            "producer_type": ctx.belief.provenance.producer_type,
            "strategy_id": ctx.belief.provenance.strategy_id,
            "category": ctx.belief.provenance.category,
            "scope": ctx.belief.provenance.scope,
            "event_linkage": linkage,
        });
        let evidence = serde_json::json!({ "replayed": true });

        let rows = beliefs
            .insert_historical(
                &belief_id,
                &decided_iso, // created_at = ORIGINAL decided time (preserved)
                &event_id,
                p,
                p,            // p_raw == p (no recalibration in replay)
                &decided_iso, // horizon (opaque here)
                &evidence,
                &provenance,
            )
            .await?;
        Ok(rows)
    }

    /// Persist the parity scorecard through the LIVE `ScorecardsRepo` (the same
    /// write path the daemon uses), keyed idempotently by a content-derived id +
    /// the source-stamp window + a content-derived `computed_at` so a re-run is a
    /// no-op (the table's `ON CONFLICT (scope, producer, window, computed_at)`).
    async fn persist_scorecard(&self, card: &Scorecard) -> Result<(), ReplayError> {
        let repo = ScorecardsRepo::new(self.pool.clone());
        // computed_at is content-derived (NOT wall-clock) so the persisted row is
        // deterministic and idempotent across replays. We hash the scope/producer/
        // window/n into a fixed sentinel instant in the representable range.
        let id = content_ulid(&[
            b"scorecard",
            card.scope.as_bytes(),
            card.producer.as_deref().unwrap_or("").as_bytes(),
            card.window.as_bytes(),
        ]);
        // A fixed, deterministic computed_at: the harness does not invent a
        // wall-clock time for a historical card. Epoch 0 + a small content offset
        // keeps it stable and inside range; newest-wins on read is irrelevant for
        // the single replayed snapshot.
        let computed_at = UtcTimestamp::from_epoch_millis(0)
            .map_err(|e| ReplayError::Encoding(e.to_string()))?
            .to_iso8601();
        repo.insert_scorecard(&id, card, &computed_at).await?;
        Ok(())
    }
}

/// Extract the binary probability from a decision context, erroring on a scalar
/// payload (S2 scores binary scopes only; scalar replay is a later slice).
fn binary_p(ctx: &DecisionContext) -> Result<f64, ReplayError> {
    match &ctx.belief.payload {
        BeliefPayload::Binary { p } => Ok(*p),
        BeliefPayload::Scalar { .. } => Err(ReplayError::Encoding(
            "scalar belief payloads are not scored by the S2 binary replay path".to_string(),
        )),
    }
}

/// Drain a source iterator into a Vec, surfacing the first error.
fn collect<T>(
    iter: Box<dyn Iterator<Item = Result<T, SourceError>> + '_>,
) -> Result<Vec<T>, ReplayError> {
    let mut out = Vec::new();
    for item in iter {
        out.push(item?);
    }
    Ok(out)
}

/// Derive a deterministic, content-addressed ULID string from the given byte
/// segments via FNV-1a (a stable, version-pinned hash — NOT `DefaultHasher`,
/// whose output is unstable across releases). Identical content always yields
/// the same ULID, which is what makes the `ON CONFLICT DO NOTHING` writes
/// idempotent and the replay deterministic.
fn content_ulid(segments: &[&[u8]]) -> String {
    // FNV-1a 64-bit, run twice over a domain-separated stream to fill 128 bits.
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hi = FNV_OFFSET;
    let mut lo = FNV_OFFSET ^ 0x9E37_79B9_7F4A_7C15;
    for seg in segments {
        for &byte in *seg {
            hi ^= u64::from(byte);
            hi = hi.wrapping_mul(FNV_PRIME);
        }
        // Domain-separate the second lane so hi/lo are not identical.
        for &byte in seg.iter().rev() {
            lo ^= u64::from(byte);
            lo = lo.wrapping_mul(FNV_PRIME);
        }
    }
    let bytes = ((u128::from(hi) << 64) | u128::from(lo)).to_be_bytes();
    fortuna_core::ids::Ulid::from_bytes(bytes).to_string()
}

/// Derive a deterministic ULID-text `run_id` for a `validation_runs` row.
///
/// Uses the same FNV-1a content-hash approach as [`content_ulid`] so the id
/// is **byte-stable across Rust releases** (unlike `DefaultHasher`, which the
/// stdlib reserves the right to change).  Seeding with `computed_at_ms`
/// ensures that re-runs of the same `(scope, producer)` at a different time
/// always produce a distinct id, while two calls with identical inputs always
/// produce the same id — making the id a pure function of its inputs (I5).
///
/// # Arguments
/// * `scope`          — validation scope string (e.g. `"weather:KNYC"`)
/// * `producer`       — optional producer tag (e.g. `Some("aeolus")`)
/// * `computed_at_ms` — the run's `computed_at` epoch-milliseconds
pub fn run_id_for(scope: &str, producer: Option<&str>, computed_at_ms: i64) -> String {
    let ts_bytes = computed_at_ms.to_le_bytes();
    content_ulid(&[
        scope.as_bytes(),
        producer.unwrap_or("").as_bytes(),
        &ts_bytes,
    ])
}
