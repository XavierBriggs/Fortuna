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
