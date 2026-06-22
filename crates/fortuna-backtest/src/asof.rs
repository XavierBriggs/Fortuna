//! As-of join — the G-PIT (point-in-time / no-look-ahead) enforcement point
//! (S2; spec §5 G-PIT, §6).
//!
//! Each replay decision assembles its inputs from the source's record pools via
//! an **as-of join** keyed on `event_linkage` and gated on time. The single
//! load-bearing rule is:
//!
//! > **A record enters a decision's context iff `available_at < decided_at`
//! > (STRICT `<`).**
//!
//! Strict `<` (never `<=`) is the conservative tie-break against coarse /
//! same-bucket timestamps: a daily forecast whose `available_at` and
//! `decided_at` land in the same calendar bucket must NOT admit same-bucket
//! future information. A record with `available_at >= decided_at` is a leak — it
//! is **rejected and counted** ([`AsOfDisposition::LookAheadRejected`]), never
//! silently joined.
//!
//! - **Belief:** admitted iff `belief.available_at < belief.decided_at`.
//! - **CLV-entry snapshot:** the LATEST snapshot with `snapshot.at < decided_at`
//!   (strict). A snapshot AT `decided_at` is ineligible even though it is the
//!   newest — that is exactly the leak strict `<` prevents.
//! - **Outcome:** a post-resolution LABEL (its `available_at == resolved_at`).
//!   It may LABEL but never DECIDE, so it is NOT subject to the `< decided_at`
//!   gate; it is attached to the joined context on `event_linkage` so the
//!   harness can score the resolved sample. Its resolution time being AFTER
//!   `decided_at` is expected (you resolve after you decide), not a leak.

use crate::records::{HistoricalBelief, HistoricalOutcome, HistoricalSnapshot};

/// The assembled, leak-free inputs for one replay decision.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionContext {
    /// The decision's belief (already verified `available_at < decided_at`).
    pub belief: HistoricalBelief,
    /// The CLV-entry snapshot: the latest with `at < decided_at`, or `None` when
    /// no eligible prior snapshot exists for this event.
    pub snapshot: Option<HistoricalSnapshot>,
    /// The post-resolution outcome label for this event, when present.
    pub outcome: Option<HistoricalOutcome>,
}

/// The outcome of as-of-joining one belief against the snapshot/outcome pools.
///
/// `Joined` boxes its [`DecisionContext`] so the two variants are similarly
/// sized (the context embeds full records; the rejected variant is a unit) —
/// otherwise every value would carry the large-variant footprint.
#[derive(Debug, Clone, PartialEq)]
pub enum AsOfDisposition {
    /// The belief passed G-PIT and was assembled into a [`DecisionContext`].
    Joined(Box<DecisionContext>),
    /// The belief is a look-ahead leak (`available_at >= decided_at`) and was
    /// rejected. The harness counts these in `ReplayReport::look_ahead_rejected`.
    LookAheadRejected,
}

/// As-of-join one belief against the candidate `snapshots` and `outcomes`,
/// enforcing G-PIT.
///
/// Returns [`AsOfDisposition::LookAheadRejected`] when the belief itself is a
/// leak (`available_at >= decided_at`, STRICT rule); otherwise
/// [`AsOfDisposition::Joined`] with the CLV-entry snapshot (latest with
/// `at < decided_at`) and the matching post-resolution outcome attached.
///
/// `snapshots` and `outcomes` may contain records for OTHER events; only those
/// matching `belief.event_linkage` are considered (the join key).
pub fn asof_join(
    belief: &HistoricalBelief,
    snapshots: &[HistoricalSnapshot],
    outcomes: &[HistoricalOutcome],
) -> AsOfDisposition {
    let decided_at = belief.decided_at;

    // G-PIT, the load-bearing comparison: a belief is ADMITTED iff its knowledge
    // time is STRICTLY before the decision instant (`available_at < decided_at`).
    // Strict `<` (never `<=`) is the conservative tie-break; the equality
    // boundary (`available_at == decided_at`) is a leak and is rejected. A `<` →
    // `<=` mutation here admits the tie and reds `gpit_strict_excludes_equal_timestamp`.
    if belief.available_at < decided_at {
        // CLV-entry snapshot: the latest eligible snapshot for THIS event with
        // `at < decided_at` (strict — a tie at decided_at is ineligible).
        // Same-event join only.
        let snapshot = snapshots
            .iter()
            .filter(|s| s.market == belief.event_linkage && s.at < decided_at)
            .max_by_key(|s| s.at)
            .cloned();

        // The post-resolution outcome label for this event (join key only; the
        // outcome's time is NOT gated — it labels, never decides).
        let outcome = outcomes
            .iter()
            .find(|o| o.event_linkage == belief.event_linkage)
            .cloned();

        AsOfDisposition::Joined(Box::new(DecisionContext {
            belief: belief.clone(),
            snapshot,
            outcome,
        }))
    } else {
        // `available_at >= decided_at` is a look-ahead leak (the equality
        // boundary included) → rejected and counted, never silently joined.
        AsOfDisposition::LookAheadRejected
    }
}
