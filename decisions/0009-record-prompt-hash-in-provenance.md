# 0009. Record prompt_hash in belief provenance

Status: Accepted. Date: 2026-06-22.

## Context

Spec 5.5 mandates that belief provenance carry `{model_id, prompt_hash, context_manifest_hash, cost_cents}`, and I5 requires the audit log be "sufficient to replay any decision." As-built, the harness stamped only three of the four fields: `prompt_hash` was omitted at all three production stamp sites (synthesis `mind.rs`, shadow `shadow.rs`, world-forward discovery `discovery.rs`). `context_manifest_hash` captured which context items the model saw, but nothing captured the system charter or the exact rendered prompt. A charter or prompt-template edit between two decisions was invisible to the audit trail, leaving I5's replay guarantee partial. The 2026-06-22 canon review flagged this as the top accountability gap before live capital.

## Options

1. **Implement `prompt_hash`** (chosen). Hash the exact prompt material the provider was sent (system charter + rendered context) and stamp it into provenance at every site. Closes the I5 replay gap; small, bounded change.
2. **Amend the spec to drop `prompt_hash`.** Argue the prompt is reconstructable from `context_manifest_hash` plus the code version (the charter lives in git). Rejected: only valid if the rendered prompt is provably deterministic from stored data plus the matching checkout, which is harder to argue than to just hash the bytes, and it makes the trail reconstructable only with the source in hand rather than self-contained.
3. **Leave it as a defect in GAPS.** Rejected: the spec and code would stay in disagreement on the audit guarantee, which is exactly the drift the canon exists to remove.

## Decision

The harness computes `prompt_hash = Sha256(system_charter ⏎ rendered_context)` (the same content-hash primitive used for `context_manifest_hash`, joined by a unit separator) and stamps it into belief provenance:
- Synthesis path: inside `AnthropicMind::call_priced` (it knows both the charter and the rendered context).
- Discovery path: carried on the internal `StructuredDecision` from `call_priced_structured` and stamped in `discovery.rs`.
- Shadow path: the challenger mind stamps its own `prompt_hash` first; `shadow.rs` carries it forward when it overwrites provenance with the challenger identity and pairing key.

Minds with no real provider prompt (the `StubMind` / default structured channel) stamp an empty `prompt_hash` — there is nothing to reconstruct. Covered by `anthropic_stamps_charter_sensitive_prompt_hash` (asserts the hash is present, deterministic for identical inputs, and changes when the charter changes).

## Trade-offs

Adds a hash computation per model call and a field to the internal `StructuredDecision` (not to the I6-guarded `MindOutput`, so the model surface is unchanged). The hash covers charter + rendered context, not the JSON schema or model-tier string; those are captured by `model_id` and the stored schema. Acceptable: the material reconstruction risk was the charter/template, which is now covered.

## Consequences

I5's "replay any decision" is now honored for the prompt as well as the context. The spec and code agree. The `prompt_hash`-not-recorded defect is retired from the gap list. Remaining audit-replay items (a decision-replay tool, `actor` on daemon rows) are separate and tracked in GAPS.md.
