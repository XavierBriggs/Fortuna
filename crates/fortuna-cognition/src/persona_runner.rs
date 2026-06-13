//! Track E E.3a: the persona runner (design §8), modeled on `discovery.rs`.
//!
//! `run_persona_analysis` takes a loaded [`PersonaDef`](crate::persona::PersonaDef),
//! the untrusted signals to reason over, a `Mind`, and a daily cost budget, and
//! produces a `PersonaOutcome` — an order-free DRAFT the composition persists as a
//! `domain_analyses` row (cognition has no Postgres dependency; it returns drafts,
//! exactly like `run_reconciliation`).
//!
//! ## The trusted / untrusted firewall (design §4 — the headline)
//! The persona's trusted METHOD rides in the Mind transport's **system message**
//! (`AnthropicMindConfig.system_charter`, set by the composition from
//! `persona.method` — see [`persona_system_charter`]). This runner therefore
//! assembles **only the untrusted signals** into the [`AssembledContext`] data
//! path — the method is NEVER packed as a `<context-item>`. A poisoned signal's
//! worst case is a bad finding → a bad belief → still gated; it can never rewrite
//! the method.
//!
//! ## Degrade, never crash (design §8)
//! budget exhausted → throttle (no call, no artifact); no in-window signals →
//! skip; mind failure / non-JSON findings / schema-invalid findings → a counted
//! defect on the outcome, `Ok` returned, the loop survives. The only hard error is
//! a context-assembly failure. Determinism: all time via the injected `Clock`; a
//! scripted `StubMind` yields a byte-identical artifact + `content_hash`.

use crate::context::{assemble_context, content_hash_of, AssemblerConfig, ContextItem};
use crate::mind::Mind;
use crate::persona::PersonaDef;
use fortuna_core::clock::UtcTimestamp;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

/// Character budget for the assembled signal context (matches discovery's 100k).
const ASSEMBLER_BUDGET_CHARS: usize = 100_000;

/// One point-in-time input reference (design §5 `signal_manifest`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignalRef {
    pub signal_id: String,
    pub content_hash: String,
}

/// The persona runner's output — an order-free draft (mirrors
/// `ReconciliationOutcome`; I6: no order/size/price field can appear here). The
/// composition persists `findings`/`content_hash`/`signal_manifest` as one
/// append-only `domain_analyses` row + an audit row.
///
/// `Serialize` is derived so the composition can persist it AND so the I6
/// field-surface invariant pin (design §15, the `fortuna-invariants` slice E.3c)
/// can assert the key set carries no order/size field.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PersonaOutcome {
    pub persona_id: String,
    pub persona_version: i32,
    pub region_key: String,
    pub produced_at: UtcTimestamp,
    /// Point-in-time inputs (their ids + content hashes), strictly before the run.
    pub signal_manifest: Vec<SignalRef>,
    /// The schema-validated structured findings — `None` when no artifact was
    /// produced (throttled / skipped / degraded).
    pub findings: Option<Value>,
    /// SHA-256 over `{findings, signal_manifest}` — the replay anchor (5.7/I5).
    /// `Some` iff `findings` is `Some`.
    pub content_hash: Option<String>,
    /// The assembled-context manifest hash (`None` if no call was made).
    pub manifest_hash: Option<String>,
    pub cost_cents: i64,
    /// Budget breach → no call, no artifact (a degrade, never a crash).
    pub throttled: bool,
    /// No in-window signals → skip (no call).
    pub skipped_no_signals: bool,
    /// Counted defects (mind failure, non-JSON findings, schema violations).
    /// Surfaced for the caller to audit/alert on; never panics.
    pub defects: Vec<String>,
}

impl PersonaOutcome {
    fn empty(persona: &PersonaDef, region_key: &str, now: UtcTimestamp) -> PersonaOutcome {
        PersonaOutcome {
            persona_id: persona.meta.id.clone(),
            persona_version: persona.meta.version,
            region_key: region_key.to_string(),
            produced_at: now,
            signal_manifest: Vec::new(),
            findings: None,
            content_hash: None,
            manifest_hash: None,
            cost_cents: 0,
            throttled: false,
            skipped_no_signals: false,
            defects: Vec::new(),
        }
    }

    /// True iff a persisted artifact was produced (findings + content_hash present).
    pub fn produced_artifact(&self) -> bool {
        self.findings.is_some() && self.content_hash.is_some()
    }
}

/// Persona-runner errors. Only context assembly is a hard error; every other
/// failure degrades to a counted defect on the `Ok(PersonaOutcome)` (design §8).
#[derive(Debug, Error)]
pub enum PersonaRunError {
    #[error("context assembly failed: {0}")]
    Context(#[from] crate::context::ContextError),
}

/// The trusted system message for a persona run: the persona's method body, which
/// the composition sets as the Mind transport's `system_charter` (`mind.rs:495`).
/// This is the ONLY place the method enters a model call — never the context data
/// path (design §4). Exposed so the wiring is explicit and testable.
pub fn persona_system_charter(persona: &PersonaDef) -> &str {
    &persona.method
}

/// Validate parsed `findings` against a persona `schema.json`, strictly (design
/// §4(c)): findings must be a JSON object; every `required` key present; and when
/// the schema sets `additionalProperties:false`, no key outside `properties`.
/// Returns the list of violations (empty = valid). Config-driven — no per-domain
/// Rust shape, so a new persona's schema is honored without code.
pub fn validate_findings(findings: &Value, schema: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    let Some(obj) = findings.as_object() else {
        violations.push("findings is not a JSON object (free prose is never executed)".to_string());
        return violations;
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !obj.contains_key(key) {
                violations.push(format!("missing required field '{key}'"));
            }
        }
    }
    let additional_allowed = schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !additional_allowed {
        // additionalProperties:false with NO `properties` means every key is
        // forbidden (per JSON Schema); a missing `properties` must not silently
        // disable the check.
        let props = schema.get("properties").and_then(Value::as_object);
        for key in obj.keys() {
            let allowed = props.map(|p| p.contains_key(key)).unwrap_or(false);
            if !allowed {
                violations.push(format!(
                    "unknown field '{key}' (schema forbids additionalProperties)"
                ));
            }
        }
    }
    violations
}

/// The replay anchor: SHA-256 over a deterministic `{findings, signal_manifest}`
/// serialization (design §5 `content_hash`).
fn anchor_hash(findings: &Value, manifest: &[SignalRef]) -> Result<String, PersonaRunError> {
    // serde_json::to_string is deterministic for identical inputs; this never
    // fails for a Value + a Vec of plain structs, but we degrade rather than
    // unwrap to honor the no-panic rule.
    let anchor = serde_json::json!({ "findings": findings, "signal_manifest": manifest });
    let serialized = serde_json::to_string(&anchor).unwrap_or_default();
    Ok(content_hash_of(&serialized))
}

/// Run one persona analysis (design §8). Budget-first, assemble ONLY untrusted
/// signals (the method is the Mind's system charter, never the context), one
/// `Mind.decide`, parse + strictly validate findings from the journal body,
/// stamp the `content_hash` anchor. Degrades on every failure mode; the only hard
/// error is context assembly.
pub async fn run_persona_analysis(
    persona: &PersonaDef,
    region_key: &str,
    signals: &[ContextItem],
    mind: &dyn Mind,
    budget: &mut crate::discovery::DiscoveryBudget,
    now: UtcTimestamp,
) -> Result<PersonaOutcome, PersonaRunError> {
    let mut outcome = PersonaOutcome::empty(persona, region_key, now);

    // 1. Budget check FIRST (throttle before spend).
    if !budget.allows(now) {
        outcome.throttled = true;
        return Ok(outcome);
    }
    // 2. No in-window signals → skip (no call).
    if signals.is_empty() {
        outcome.skipped_no_signals = true;
        return Ok(outcome);
    }

    // Point-in-time input manifest (strictly before the call).
    outcome.signal_manifest = signals
        .iter()
        .map(|item| SignalRef {
            signal_id: item.item_id.clone(),
            content_hash: item.content_hash.clone(),
        })
        .collect();

    // 3. Assemble ONLY the untrusted signals as delimited data. The trusted
    //    method is the Mind's system charter, NOT a context item (the firewall).
    let assembler = AssemblerConfig {
        budget_chars: ASSEMBLER_BUDGET_CHARS,
        anonymize: false,
    };
    let cycle_kind = format!("persona:{}", persona.meta.id);
    let ctx = assemble_context(signals, now, &cycle_kind, &assembler)?;
    outcome.manifest_hash = Some(ctx.manifest_hash.clone());

    // 4. One Mind call; degrade on failure (counted defect, never crash).
    let output = match mind.decide(&ctx).await {
        Ok(output) => output,
        Err(e) => {
            outcome
                .defects
                .push(format!("mind failed: {e} (persona run degraded to none)"));
            return Ok(outcome);
        }
    };
    budget.record_spend(output.cost_cents, now);
    outcome.cost_cents = output.cost_cents;

    // 5. Findings ride in the journal body as strict JSON (like discovery).
    let Some(journal) = output.journal else {
        outcome
            .defects
            .push("persona produced no findings journal".to_string());
        return Ok(outcome);
    };
    let findings: Value = match serde_json::from_str(&journal.body) {
        Ok(value) => value,
        Err(e) => {
            outcome.defects.push(format!(
                "findings body violated the contract (never repaired): {e}"
            ));
            return Ok(outcome);
        }
    };
    let violations = validate_findings(&findings, &persona.schema);
    if !violations.is_empty() {
        for v in violations {
            outcome
                .defects
                .push(format!("findings schema violation: {v}"));
        }
        return Ok(outcome);
    }

    // 6. Stamp the replay anchor and the artifact.
    outcome.content_hash = Some(anchor_hash(&findings, &outcome.signal_manifest)?);
    outcome.findings = Some(findings);
    Ok(outcome)
}
