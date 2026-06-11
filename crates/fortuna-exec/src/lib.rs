//! fortuna-exec: order manager and execution. Spec 5.4.
//!
//! Intent journal state machine (created -> submitted -> acked ->
//! partially_filled -> filled | cancelled | rejected), persisted BEFORE any
//! network call; client ids derived from intent ids (idempotent resubmission).
//! Boot reconciliation before any strategy wakes. IntentGroup multi-leg
//! completion policy (max unhedged notional, max leg-open duration,
//! complete-or-unwind). Execution policy: maker-first, TTL, re-quote on belief
//! or signal update, one working order per (strategy, market, side). Flatten
//! planner (book-walk estimate; kill-switch path is planner-exempt per spec).

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

mod flatten;
mod group;
mod journal;
mod manager;

pub use flatten::{plan_flatten, FlattenDecision, FlattenLeg, FlattenPlan};
pub use group::{
    decide_complete_or_unwind, CompleteOrUnwind, GroupDecision, GroupPolicy, GroupState,
    GroupStatus, GroupTracker, RemainingLeg,
};
pub use journal::{IntentEvent, IntentJournal, JournalRow, MemoryJournal, OrderSnapshot};
pub use manager::{
    BootReport, CancelOutcome, ExecError, ExecPolicy, FillApplication, IntentRecord, IntentStatus,
    IntentView, LegOutcome, OrderManager, SubmitOutcome,
};
