//! The Mind trait (spec 5.9, Artemis pattern): the model interface.
//!
//! `MindOutput` is PROPOSE-ONLY (I6): beliefs, unsized proposals, and an
//! optional journal draft. Sizing, timing, order type, and execution
//! belong to the harness; the model's `urgency` may select execution
//! style within policy, never size.
//!
//! Two implementations:
//! - `StubMind`: deterministic scripted outputs (DST and Phase 2 exit).
//! - `AnthropicMind`: the Claude API over raw HTTP behind a
//!   `MindTransport` trait (there is no official Rust SDK; the wire
//!   format follows the documented /v1/messages contract — see the
//!   claude-api reference consulted at T2.5). Structured output is
//!   enforced via `output_config.format` json_schema; any
//!   schema-invalid output is REJECTED and surfaced, never repaired
//!   silently. Per-cycle and per-day cost budgets are checked BEFORE the
//!   call; breach degrades to mechanical-only (the caller alerts).
//!
//! Secrets discipline: the API key lives in the TRANSPORT (from the
//! environment only); nothing in this module logs or stores it.

use crate::beliefs::BeliefDraft;
use crate::context::AssembledContext;
use async_trait::async_trait;
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::market::Side;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MindError {
    /// Transport/API failure (network, 4xx/5xx, overload). The decision
    /// cycle owns retry policy; the transport does not retry.
    #[error("mind provider error: {reason}")]
    Provider { reason: String },
    /// The model's output failed schema or domain validation. Rejected
    /// and logged by the caller, NEVER repaired silently (spec 5.9).
    #[error("schema-invalid mind output: {reason}")]
    SchemaInvalid { reason: String },
    /// The model refused (stop_reason = refusal). Surfaced, not retried.
    #[error("mind refused: {explanation}")]
    Refused { explanation: String },
    /// A cost ceiling was reached BEFORE the call. The cycle degrades to
    /// mechanical-only and alerts (spec 5.9).
    #[error("mind budget exhausted: {scope} (spent {spent_cents}c / cap {cap_cents}c)")]
    BudgetExhausted {
        scope: &'static str,
        spent_cents: i64,
        cap_cents: i64,
    },
}

/// Execution-style request (never size; spec 5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftUrgency {
    Passive,
    Taker,
}

/// One UNSIZED proposal draft (spec 5.9: market, side, max_price,
/// thesis, belief_ref, urgency). `thesis` is model text — data, never
/// instructions downstream. Unknown fields are REJECTED (I6): a model
/// that smuggles sizing or execution fields is schema-invalid and its
/// whole output is discarded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalDraft {
    pub market: String,
    pub side: Side,
    pub max_price_cents: i64,
    pub thesis: String,
    pub belief_ref: String,
    pub urgency: DraftUrgency,
}

impl ProposalDraft {
    fn validate(&self) -> Result<(), MindError> {
        if self.market.trim().is_empty() {
            return Err(MindError::SchemaInvalid {
                reason: "proposal market is empty".to_string(),
            });
        }
        if !(1..=99).contains(&self.max_price_cents) {
            return Err(MindError::SchemaInvalid {
                reason: format!(
                    "proposal max_price_cents {} outside [1, 99]",
                    self.max_price_cents
                ),
            });
        }
        if self.belief_ref.trim().is_empty() {
            return Err(MindError::SchemaInvalid {
                reason: "proposal belief_ref is empty (every proposal cites its belief)"
                    .to_string(),
            });
        }
        Ok(())
    }
}

/// Reconciliation-cycle journal draft (free text; episodic memory input).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalDraft {
    pub body: String,
}

/// What the mind emits (spec 5.9). `cost_cents` is what answering cost
/// (stub = 0; Anthropic = usage x configured prices). Unknown top-level
/// fields are REJECTED (I6): there is no escape hatch for orders, tool
/// calls, or commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MindOutput {
    pub beliefs: Vec<BeliefDraft>,
    pub proposals: Vec<ProposalDraft>,
    #[serde(default)]
    pub journal: Option<JournalDraft>,
    #[serde(default)]
    pub cost_cents: i64,
}

impl MindOutput {
    /// Domain validation over the parsed shape: every belief and proposal
    /// must be individually valid; failure rejects the WHOLE output.
    fn validate(&self) -> Result<(), MindError> {
        for b in &self.beliefs {
            b.validate().map_err(|e| MindError::SchemaInvalid {
                reason: e.to_string(),
            })?;
        }
        for p in &self.proposals {
            p.validate()?;
        }
        Ok(())
    }

    pub fn empty() -> MindOutput {
        MindOutput {
            beliefs: Vec::new(),
            proposals: Vec::new(),
            journal: None,
            cost_cents: 0,
        }
    }
}

/// The model interface (spec 5.9).
#[async_trait]
pub trait Mind: Send + Sync {
    fn id(&self) -> &str;
    async fn decide(&self, ctx: &AssembledContext) -> Result<MindOutput, MindError>;
    /// Cycle boundary, called by the CYCLE OWNER (decision cycle, veto
    /// loop) before its first call: resets any per-cycle budget so the
    /// cap spans all calls of one cycle (a retry shares the allowance).
    /// Free-running minds (stub) need no notion of a cycle.
    fn begin_cycle(&self) {}
    /// The mind's own running spend today, in cents — the BUDGET-TRUE
    /// number, which includes tokens consumed by failed calls (refusals,
    /// schema-invalid outputs) that never produced a usable decision.
    /// Stub minds spend nothing.
    fn spent_today_cents(&self) -> i64 {
        0
    }
}

/// Deterministic scripted mind (DST and Phase 2 exit). Outputs replay in
/// order; an exhausted script yields the empty (null) decision.
pub struct StubMind {
    script: Mutex<Vec<MindOutput>>,
}

impl StubMind {
    pub fn scripted(outputs: Vec<MindOutput>) -> StubMind {
        StubMind {
            script: Mutex::new(outputs),
        }
    }
}

#[async_trait]
impl Mind for StubMind {
    fn id(&self) -> &str {
        "stub-mind"
    }

    async fn decide(&self, _ctx: &AssembledContext) -> Result<MindOutput, MindError> {
        let mut script = self.script.lock().unwrap_or_else(|e| e.into_inner());
        if script.is_empty() {
            Ok(MindOutput::empty())
        } else {
            Ok(script.remove(0))
        }
    }
}

/// Per-cycle and per-day cost ceilings (spec 5.9: "Per-cycle and per-day
/// cost budgets in config; budget breach degrades to mechanical-only and
/// alerts"). Day boundary is 00:00 UTC (house rule). Checked BEFORE any
/// call: a breach never spends another cent finding out.
#[derive(Debug, Clone)]
pub struct CostBudget {
    per_cycle_cap_cents: i64,
    per_day_cap_cents: i64,
    spent_today_cents: i64,
    spent_this_cycle_cents: i64,
    day_epoch: i64,
}

const DAY_MS: i64 = 86_400_000;

impl CostBudget {
    pub fn new(per_cycle_cap_cents: i64, per_day_cap_cents: i64) -> CostBudget {
        CostBudget {
            per_cycle_cap_cents,
            per_day_cap_cents,
            spent_today_cents: 0,
            spent_this_cycle_cents: 0,
            day_epoch: -1,
        }
    }

    /// Start a new decision cycle: the per-cycle allowance resets; the
    /// day total carries.
    pub fn begin_cycle(&mut self) {
        self.spent_this_cycle_cents = 0;
    }

    fn roll(&mut self, now: UtcTimestamp) {
        let day = now.epoch_millis().div_euclid(DAY_MS);
        if day != self.day_epoch {
            self.day_epoch = day;
            self.spent_today_cents = 0;
        }
    }

    /// Refuses when THIS CYCLE's spend has reached the per-cycle cap
    /// (a positive cap binds: the call that would exceed it is refused)
    /// or the day cap is reached. A non-positive cycle cap is degenerate
    /// and refuses everything.
    pub fn check(&mut self, now: UtcTimestamp) -> Result<(), MindError> {
        self.roll(now);
        if self.per_cycle_cap_cents <= 0 || self.spent_this_cycle_cents >= self.per_cycle_cap_cents
        {
            return Err(MindError::BudgetExhausted {
                scope: "per_cycle",
                spent_cents: self.spent_this_cycle_cents,
                cap_cents: self.per_cycle_cap_cents,
            });
        }
        if self.spent_today_cents >= self.per_day_cap_cents {
            return Err(MindError::BudgetExhausted {
                scope: "per_day",
                spent_cents: self.spent_today_cents,
                cap_cents: self.per_day_cap_cents,
            });
        }
        Ok(())
    }

    pub fn record_spend(&mut self, cents: i64, now: UtcTimestamp) {
        self.roll(now);
        self.spent_today_cents += cents.max(0);
        self.spent_this_cycle_cents += cents.max(0);
    }

    pub fn spent_today_cents(&self) -> i64 {
        self.spent_today_cents
    }

    pub fn spent_this_cycle_cents(&self) -> i64 {
        self.spent_this_cycle_cents
    }
}

/// The wire transport: POST /v1/messages with the documented headers
/// (x-api-key from the environment, anthropic-version 2023-06-01). NO
/// retries here — the decision cycle owns retry policy.
#[async_trait]
pub trait MindTransport: Send + Sync {
    async fn post_messages(
        &self,
        body: serde_json::Value,
    ) -> Result<(u16, serde_json::Value), MindError>;
}

/// Live transport over reqwest. Constructed ONLY when the operator
/// provides ANTHROPIC_API_KEY (env; never config, never logged) — this
/// is the runtime feature gate the Phase 2 exit names.
pub struct ReqwestMindTransport {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const ENV_ANTHROPIC_API_KEY: &str = "ANTHROPIC_API_KEY";

impl ReqwestMindTransport {
    /// Fails loudly without the key: a mind that silently can't think is
    /// worse than no mind (the composition degrades to mechanical-only
    /// EXPLICITLY, never by accident).
    pub fn from_env(timeout: std::time::Duration) -> Result<Self, MindError> {
        let api_key = std::env::var(ENV_ANTHROPIC_API_KEY).map_err(|_| MindError::Provider {
            reason: format!("{ENV_ANTHROPIC_API_KEY} is not set (operator-provisioned)"),
        })?;
        if api_key.trim().is_empty() {
            return Err(MindError::Provider {
                reason: format!("{ENV_ANTHROPIC_API_KEY} is empty"),
            });
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| MindError::Provider {
                reason: format!("http client: {e}"),
            })?;
        Ok(ReqwestMindTransport {
            client,
            api_key,
            base_url: ANTHROPIC_API_URL.to_string(),
        })
    }
}

impl std::fmt::Debug for ReqwestMindTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReqwestMindTransport")
            .field("base_url", &self.base_url)
            .field("api_key", &"<REDACTED>")
            .finish()
    }
}

#[async_trait]
impl MindTransport for ReqwestMindTransport {
    async fn post_messages(
        &self,
        body: serde_json::Value,
    ) -> Result<(u16, serde_json::Value), MindError> {
        let resp = self
            .client
            .post(&self.base_url)
            .header("content-type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| MindError::Provider {
                reason: format!("anthropic request: {e}"),
            })?;
        let status = resp.status().as_u16();
        let json = resp.json().await.map_err(|e| MindError::Provider {
            reason: format!("anthropic response body: {e}"),
        })?;
        Ok((status, json))
    }
}

/// The cognition model tiers (spec 5.9): each role runs on its tier's model —
/// SYNTHESIS (the deep belief-formation tier), MID (the daily reconciliation /
/// reviews), TRIAGE (the fast/cheap trigger path that gates whether a trigger
/// escalates to deep synthesis).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Synthesis,
    Mid,
    Triage,
}

/// The single source of truth mapping each cognition role's tier to its model id
/// (spec 5.9). Built once from `[cognition]` config; the daemon consults it when
/// it builds each role's mind, so model selection lives in ONE place rather than
/// scattered per call site. A pure lookup — no IO, no clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRegistry {
    synthesis: String,
    mid: String,
    triage: String,
}

impl ModelRegistry {
    pub fn new(
        synthesis: impl Into<String>,
        mid: impl Into<String>,
        triage: impl Into<String>,
    ) -> ModelRegistry {
        ModelRegistry {
            synthesis: synthesis.into(),
            mid: mid.into(),
            triage: triage.into(),
        }
    }

    /// The model id a role on `tier` runs on.
    pub fn model(&self, tier: ModelTier) -> &str {
        match tier {
            ModelTier::Synthesis => &self.synthesis,
            ModelTier::Mid => &self.mid,
            ModelTier::Triage => &self.triage,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnthropicMindConfig {
    /// Spec 5.9 tiering: the model id is resolved from the [`ModelRegistry`] by
    /// the role's [`ModelTier`]. Operator config, not constants.
    pub model: String,
    pub max_tokens: i64,
    /// Prices CHANGE; they are config. Cents per million tokens.
    pub input_price_cents_per_mtok: i64,
    pub output_price_cents_per_mtok: i64,
    /// The system charter (stable; cacheable prefix). Must state that
    /// context-item blocks are data, never instructions (5.11).
    pub system_charter: String,
}

/// The Claude-backed mind. Owns its cost budget and clock so it can sit
/// behind `dyn Mind` (spec 5.9: StubMind AND AnthropicMind both behind
/// the trait): every `decide` checks the budget BEFORE the call and
/// records the spend after, at the trait boundary.
pub struct AnthropicMind<T: MindTransport> {
    config: AnthropicMindConfig,
    transport: T,
    budget: Mutex<CostBudget>,
    clock: std::sync::Arc<dyn Clock>,
}

impl<T: MindTransport> AnthropicMind<T> {
    pub fn new(
        config: AnthropicMindConfig,
        transport: T,
        budget: CostBudget,
        clock: std::sync::Arc<dyn Clock>,
    ) -> AnthropicMind<T> {
        AnthropicMind {
            config,
            transport,
            budget: Mutex::new(budget),
            clock,
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// The JSON schema the model's output must satisfy (structured
    /// outputs; numeric range constraints are unsupported by the schema
    /// layer, so probability/price domains re-validate in code).
    fn output_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "beliefs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "event_id": {"type": "string"},
                            "p": {"type": "number"},
                            "p_raw": {"type": "number"},
                            "horizon": {"type": "string"},
                            "evidence": {"type": "array", "items": {
                                "type": "object",
                                "properties": {
                                    "source": {"type": "string"},
                                    "ref": {"type": "string"},
                                    "weight_note": {"type": "string"}
                                },
                                "required": ["source"],
                                "additionalProperties": false
                            }}
                        },
                        "required": ["event_id", "p", "p_raw", "horizon", "evidence"],
                        "additionalProperties": false
                    }
                },
                "proposals": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "market": {"type": "string"},
                            "side": {"type": "string", "enum": ["yes", "no"]},
                            "max_price_cents": {"type": "integer"},
                            "thesis": {"type": "string"},
                            "belief_ref": {"type": "string"},
                            "urgency": {"type": "string", "enum": ["passive", "taker"]}
                        },
                        "required": ["market", "side", "max_price_cents", "thesis", "belief_ref", "urgency"],
                        "additionalProperties": false
                    }
                },
                "journal": {
                    "anyOf": [
                        {"type": "null"},
                        {"type": "object", "properties": {"body": {"type": "string"}},
                         "required": ["body"], "additionalProperties": false}
                    ]
                }
            },
            "required": ["beliefs", "proposals", "journal"],
            "additionalProperties": false
        })
    }

    /// One decision: budget check FIRST, then the call, then validation
    /// and cost accounting. `now` comes from the injected clock. The
    /// trait-level `decide` wraps this over the OWNED budget and clock;
    /// this explicit form exists for compositions that share one budget
    /// across minds and for budget-mechanics tests.
    pub async fn decide_with_budget(
        &self,
        ctx: &AssembledContext,
        budget: &mut CostBudget,
        now: UtcTimestamp,
    ) -> Result<MindOutput, MindError> {
        budget.check(now)?;
        let (result, cost_cents) = self.call_priced(ctx).await;
        budget.record_spend(cost_cents, now);
        result
    }

    /// The transport call + validation, returning the cost SEPARATELY so
    /// callers can record spend even when the output is rejected (tokens
    /// were consumed whether or not the output parses). Transport-level
    /// failures cost zero (no usage was returned).
    async fn call_priced(&self, ctx: &AssembledContext) -> (Result<MindOutput, MindError>, i64) {
        let body = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "thinking": {"type": "adaptive"},
            "system": self.config.system_charter,
            "output_config": {"format": {"type": "json_schema", "schema": Self::output_schema()}},
            "messages": [{"role": "user", "content": ctx.rendered}],
        });

        let (status, resp) = match self.transport.post_messages(body).await {
            Ok(pair) => pair,
            Err(e) => return (Err(e), 0),
        };
        if !(200..300).contains(&status) {
            let reason = resp["error"]["message"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string();
            return (
                Err(MindError::Provider {
                    reason: format!("HTTP {status}: {reason}"),
                }),
                0,
            );
        }

        // Cost first: tokens were spent whether or not the output parses.
        let input_tokens = resp["usage"]["input_tokens"].as_i64().unwrap_or(0);
        let output_tokens = resp["usage"]["output_tokens"].as_i64().unwrap_or(0);
        let cost_cents = ceil_div(
            input_tokens * self.config.input_price_cents_per_mtok,
            1_000_000,
        ) + ceil_div(
            output_tokens * self.config.output_price_cents_per_mtok,
            1_000_000,
        );

        if resp["stop_reason"] == "refusal" {
            return (
                Err(MindError::Refused {
                    explanation: resp["stop_details"]["explanation"]
                        .as_str()
                        .unwrap_or("no explanation")
                        .to_string(),
                }),
                cost_cents,
            );
        }

        let Some(text) = resp["content"].as_array().and_then(|blocks| {
            blocks
                .iter()
                .find(|b| b["type"] == "text")
                .and_then(|b| b["text"].as_str())
        }) else {
            return (
                Err(MindError::SchemaInvalid {
                    reason: "response carries no text block".to_string(),
                }),
                cost_cents,
            );
        };

        let mut output: MindOutput = match serde_json::from_str(text) {
            Ok(output) => output,
            Err(e) => {
                return (
                    Err(MindError::SchemaInvalid {
                        reason: format!("output is not valid MindOutput JSON: {e}"),
                    }),
                    cost_cents,
                )
            }
        };
        if let Err(e) = output.validate() {
            return (Err(e), cost_cents);
        }
        output.cost_cents = cost_cents;
        // Provenance is HARNESS knowledge (spec 5.5): stamp it here —
        // the model never writes its own provenance.
        for belief in &mut output.beliefs {
            belief.provenance = json!({
                "model_id": self.config.model,
                "context_manifest_hash": ctx.manifest_hash,
                "cost_cents": cost_cents,
            });
        }
        (Ok(output), cost_cents)
    }
}

#[async_trait]
impl<T: MindTransport> Mind for AnthropicMind<T> {
    fn id(&self) -> &str {
        &self.config.model
    }

    fn begin_cycle(&self) {
        self.budget
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .begin_cycle();
    }

    fn spent_today_cents(&self) -> i64 {
        self.budget
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .spent_today_cents()
    }

    /// The spec 5.9 trait boundary: budget checked BEFORE the call,
    /// spend recorded after — against the mind's OWNED budget. The lock
    /// is never held across the transport await.
    async fn decide(&self, ctx: &AssembledContext) -> Result<MindOutput, MindError> {
        let now = self.clock.now();
        {
            let mut budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
            budget.check(now)?;
        }
        let (result, cost_cents) = self.call_priced(ctx).await;
        {
            let mut budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
            budget.record_spend(cost_cents, now);
        }
        result
    }
}

/// The composition's mind factory (spec 5.9 / GAPS: the env-key gate IS
/// the feature flag). ANTHROPIC_API_KEY present => the Claude-backed
/// mind over the reqwest transport; absent => the STUB, whose empty
/// decisions hold zero beliefs and zero proposals — a keyless
/// composition can never trade on a live provider by accident.
pub fn mind_from_env(
    config: AnthropicMindConfig,
    budget: CostBudget,
    clock: std::sync::Arc<dyn Clock>,
    timeout: std::time::Duration,
) -> Box<dyn Mind> {
    match ReqwestMindTransport::from_env(timeout) {
        Ok(transport) => Box::new(AnthropicMind::new(config, transport, budget, clock)),
        Err(_) => Box::new(StubMind::scripted(Vec::new())),
    }
}

fn ceil_div(num: i64, den: i64) -> i64 {
    (num + den - 1) / den
}
