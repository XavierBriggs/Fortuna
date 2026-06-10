//! The daily reconciliation cycle (spec 5.8) and the aeolus_eval
//! ingestion contract (spec Section 6, item 3).
//!
//! Reconciliation runs at 00:00 UTC: the mind reads the day's fills,
//! open positions, and originating beliefs (assembled as context items
//! by the composition), writes the journal entry and tomorrow's plan.
//! "No orders are placed from this loop" is STRUCTURAL here: the
//! outcome type has no field that can carry a trade; proposals the mind
//! emits anyway are counted (audited) and discarded.
//!
//! aeolus_eval: Aeolus is a signal under evaluation with ZERO capital.
//! Every forecast becomes a belief draft scored like any other belief
//! (Brier vs market-implied, CLV vs benchmark snapshots); the mapper's
//! signature returns BeliefDrafts only — no proposal type exists in this
//! path. The envelope shape below is FORTUNA's interface definition; the
//! operator-recorded fixture validates that Aeolus's exporter conforms
//! (GAPS).

use crate::beliefs::BeliefDraft;
use crate::context::{assemble_context, AssemblerConfig, ContextItem};
use crate::mind::{JournalDraft, Mind, MindError};
use fortuna_core::clock::UtcTimestamp;
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReconError {
    #[error(transparent)]
    Mind(#[from] MindError),
    #[error("context assembly failed: {0}")]
    Context(#[from] crate::context::ContextError),
    #[error("aeolus envelope rejected: {reason}")]
    BadEnvelope { reason: String },
    #[error("reconciliation produced no journal (its one job)")]
    NoJournal,
}

/// One Aeolus bracket forecast (probability for one bracket event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AeolusBracket {
    /// Stable hint joining the bracket to a canonical event
    /// (`aeolus:{event_hint}` becomes the event id namespace).
    pub event_hint: String,
    pub p: f64,
}

/// The aeolus_eval envelope contract (FORTUNA-defined; strict — unknown
/// fields are rejected so contract drift surfaces immediately).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AeolusEnvelope {
    pub station: String,
    /// Local target date of the forecast (YYYY-MM-DD).
    pub target_date: String,
    /// When Aeolus produced the run (point-in-time authority for the
    /// belief's evidence).
    pub run_at: UtcTimestamp,
    pub brackets: Vec<AeolusBracket>,
}

/// Map one envelope into ZERO-CAPITAL belief drafts. The signature is
/// the discipline: there is no proposal type here at all.
pub fn map_aeolus_envelope(
    envelope: &AeolusEnvelope,
    horizon: UtcTimestamp,
) -> Result<Vec<BeliefDraft>, ReconError> {
    if envelope.brackets.is_empty() {
        return Err(ReconError::BadEnvelope {
            reason: "empty brackets (a broken export, not a no-op)".to_string(),
        });
    }
    let mut drafts = Vec::with_capacity(envelope.brackets.len());
    for bracket in &envelope.brackets {
        if bracket.event_hint.trim().is_empty() {
            return Err(ReconError::BadEnvelope {
                reason: "bracket with empty event_hint".to_string(),
            });
        }
        let draft = BeliefDraft {
            event_id: format!("aeolus:{}", bracket.event_hint),
            p: bracket.p,
            p_raw: bracket.p,
            horizon,
            evidence: json!([{
                "source": "aeolus",
                "ref": format!("{}@{}", envelope.station, envelope.run_at),
                "weight_note": "raw aeolus run (signal under evaluation)",
            }]),
            provenance: json!({
                "model_id": "aeolus",
                "station": envelope.station,
                "target_date": envelope.target_date,
                "run_at": envelope.run_at,
            }),
        };
        draft.validate().map_err(|e| ReconError::BadEnvelope {
            reason: e.to_string(),
        })?;
        drafts.push(draft);
    }
    Ok(drafts)
}

/// The reconciliation outcome: a journal (the product), discarded
/// proposal COUNT (the mind tried to trade; we audit, we never obey),
/// and replay provenance. No field can carry an order.
#[derive(Debug)]
pub struct ReconciliationOutcome {
    pub cycle_kind: &'static str,
    pub journal: Option<JournalDraft>,
    pub beliefs: Vec<BeliefDraft>,
    pub discarded_proposals: usize,
    pub manifest_hash: String,
    pub cost_cents: i64,
}

/// Run the daily reconciliation (00:00 UTC; the caller schedules).
/// Context items: the day's fills, open positions, originating beliefs —
/// assembled by the composition, point-in-time as of `now`.
pub async fn run_reconciliation(
    mind: &dyn Mind,
    context_items: &[ContextItem],
    now: UtcTimestamp,
) -> Result<ReconciliationOutcome, ReconError> {
    let assembler = AssemblerConfig {
        budget_chars: 200_000,
        anonymize: false,
    };
    let ctx = assemble_context(context_items, now, "reconciliation", &assembler)?;
    let output = mind.decide(&ctx).await?;

    let journal = output.journal.clone();
    if journal.is_none() {
        return Err(ReconError::NoJournal);
    }
    Ok(ReconciliationOutcome {
        cycle_kind: "reconciliation",
        journal,
        beliefs: output.beliefs,
        discarded_proposals: output.proposals.len(),
        manifest_hash: ctx.manifest_hash,
        cost_cents: output.cost_cents,
    })
}
