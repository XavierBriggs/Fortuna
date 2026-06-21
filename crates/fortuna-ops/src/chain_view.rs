//! WS4 E3 — the chain-view contract (the serialized per-event chain the UI session renders).
//!
//! The backend owns the data + serialization; the separate UI session renders against this
//! committed shape. This module is **W1**: pure `Serialize`/`Deserialize` types + golden-JSON
//! tests, no endpoint yet (the endpoint is W2). It is the artifact the UI session unblocks on.
//!
//! Composition (reuse, never fork):
//! - `scorecard` is the literal WS2 [`fortuna_scoring::Scorecard`] (Brier-vs-baseline, CORP, DM,
//!   reliability, the GO whole-truth).
//! - `validation` is WS3's deflated view (PBO, SPA p_c, `family_n_trials`, verdict). It is
//!   **forward-declared** as raw `serde_json::Value` until WS3 commits `ValidationRun: Serialize`
//!   — W1 must not reuse an unbuilt type; reconcile to the real type when WS3 merges (WS3→WS4 dep).
//!
//! Conventions: cents are `i64`, probabilities are `f64` (matching the `Scorecard` DTO style);
//! timestamps are ISO-8601 `String`. Every chain stage is `Option` so the view renders at any
//! maturity (a freshly-tagged event has signals+beliefs but no fill/settle/score yet).

use fortuna_scoring::Scorecard;
use serde::{Deserialize, Serialize};

/// One event's full chain, serialized for the UI render.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChainView {
    pub event: EventRef,
    /// Safety pills (always present — they gate whether the demo is showing live or paper).
    pub safety: SafetyPills,
    /// What triggered the analysis (Aeolus envelope, NWS AFD, …).
    pub signals: Vec<SignalRef>,
    /// The head-to-head: one entry per producer (Aeolus, meteorologist, …).
    pub producers: Vec<ProducerBelief>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ProposalRef>,
    /// The I1 universal-gate TRACE (render-only; never a bypass or re-run of the gate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<FillRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement: Option<SettlementRef>,
    /// WS2 GO whole-truth surface for this scope/producer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorecard: Option<Scorecard>,
    /// WS3 deflated view — forward-declared raw JSON until WS3 commits `ValidationRun: Serialize`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EventRef {
    pub event_linkage: String,
    pub category: String,
    pub scope: String,
    pub target_date: String,
    pub market_ticker: String,
}

/// `execution_mode` is a `String` filled at the endpoint via `ExecutionMode::as_str()` (the enum is
/// `Deserialize`-only); `order_mutation_enabled` via `allows_order_mutation()`. Keeping these as
/// primitives keeps the contract decoupled from `fortuna-live`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SafetyPills {
    pub execution_mode: String,
    pub order_mutation_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book_freshness_secs: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SignalRef {
    pub source: String,
    pub kind: String,
    pub at: String,
    pub summary: String,
}

/// A producer's belief on the event + (post-resolution) its scores. The head-to-head row.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProducerBelief {
    pub producer_id: String,
    pub producer_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mind_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mind_version: Option<i64>,
    /// The emitted probability.
    pub p_raw: f64,
    /// The calibrated probability — `None` until the producer has a persisted calibration set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p_cal: Option<f64>,
    /// The reasoning drill-in (the model's verbatim free-text). Append-only display; NEVER executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub belief_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<BeliefScore>,
}

/// Post-resolution scores. NOTE: `brier` is the per-producer differentiator; `clv_bps` is a
/// MARKET-LEVEL drift quantity — shared/identical across producers who share the same bracket
/// (the resolver computes CLV from the earliest fill on the edge-market), not an independent
/// per-producer confirmation. The render must present CLV as market-level.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct BeliefScore {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brier: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clv_bps: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProposalRef {
    pub market: String,
    pub side: String,
    pub max_price_cents: i64,
    pub size: i64,
    pub thesis: String,
    pub belief_ref: String,
    pub urgency: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GateResult {
    pub decision: String,
    pub checks: Vec<GateCheck>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GateCheck {
    pub name: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A paper fill. `orders` is always `0` (paper_ledger; the `i_paper_live_no_real_order` wall).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FillRef {
    pub price_cents: i64,
    pub qty: i64,
    pub orders: u32,
    pub at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SettlementRef {
    pub outcome: f64,
    pub realized_pnl_cents: i64,
    pub settled_at: String,
    pub resolution_source: String,
}
