//! fortuna-cognition: everything probabilistic. Spec 5.7-5.12. I6.
//!
//! Source trait + ingestion funnel (envelope, dedup, point-in-time, source
//! registry trust tiers). Trigger engine (rules + cheap triage; triage is
//! scored via declined-trigger shadow sampling; per-event serialization with
//! debounce). Context assembler (budgeted, manifest-hashed, replayable;
//! computed views snapshotted). Mind trait: StubMind (deterministic, DST) and
//! AnthropicMind (structured output, budgets; schema-invalid => reject, log,
//! retry once, skip). MindOutput is propose-only (I6): beliefs, proposals,
//! journal drafts. Comparator + shared Kelly sizing lib (envelope reservation,
//! calibration haircut). Calibration layer (Platt/isotonic, shrinkage prior).
//! Loops: decision, daily reconciliation (00:00 UTC), weekly, monthly.
//!
//! Phase 1 (T1.3) ships the `veto` module only: the reduce-only model veto
//! for mech_extremes with a deterministic stub mind. The rest lands in
//! Phase 2.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod beliefs;
pub mod calibration;
pub mod context;
pub mod cycle;
pub mod discovery;
pub mod events;
pub mod mind;
pub mod persona;
pub mod persona_metrics;
pub mod persona_runner;
pub mod persona_trigger;
pub mod reconciliation;
pub mod review;
pub mod shadow;
pub mod signals;
pub mod veto;
