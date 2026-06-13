# Track M — Pluggable model providers / per-tier model selection (PARKED brief)

Status: **PARKED** (operator, 2026-06-13: "store that up as a design doc, we will do
that later"). Not active. This is a self-contained session brief to hand to a fresh
Claude Code session when the operator activates Track M. It is adjacent to Track E
(domain-analysis personas): personas declare a **tier**; Track M is what resolves a
tier to "whatever model the operator chose". Track E keeps the `Mind` trait surface
stable so the two compose.

Authored by Track E during the 2026-06-13 design session, from a verified Explore map
of the model layer. The session that owns this MUST verify the code afresh (the spec's
"investigate fresh, don't inherit conclusions" rule).

---

> **Session brief — own FORTUNA's pluggable model-provider / per-tier model-selection system ("Track M")**
>
> Hand this entire file to a fresh Claude Code session. It is the complete brief for designing and building ONE feature end to end: the ability for the operator to choose **which model runs the system, globally or per tier** — not just Anthropic (Fable 5 / Haiku), but open and local models (e.g. Nous **Hermes** served over an OpenAI-compatible endpoint) and other operator-named providers (e.g. "openclaw" — confirm with the operator what this is) — all behind FORTUNA's existing `Mind` trait, with cost, audit, structured-output, and promotion discipline fully preserved. You own it brainstorm → spec → implementation → gate.
>
> **0. First moves (before anything else).**
> 1. You're in the FORTUNA repo. Read, in order: `CLAUDE.md`, then `docs/spec.md` §3 (invariants), §5.9 (model interface — THE core section), §5.10 (calibration), §5.11 (signal trust), §11 (validation pipeline + shadow-mode model swaps), then `GAPS.md` and `ASSUMPTIONS.md` (search for `T2.5`, `T3.3`, `mind`, `budget`, `shadow`). Invoke the `fortuna` skill and the `superpowers:brainstorming` skill at the start. The `claude-api` skill is REQUIRED reading before you touch any model wire format (model ids, pricing, structured output) — do not answer from memory. The user runs local models; the `local-model-benchmark` and `huggingface-skills:huggingface-local-models` skills are relevant for the llama.cpp / OpenAI-compatible serving path.
> 2. DESIGN-FIRST. Write no code until the operator approves a committed design doc (the brainstorming skill gates this). The spec wins on every conflict; where it's silent, record the gap in `GAPS.md` and choose the conservative option.
> 3. Use the **Explore** subagent to map the real model layer before designing — see §4. Do not inherit this brief's description of the code; verify it.
>
> **1. What you are building (one paragraph).** Today FORTUNA's cognition layer talks to models through a `Mind` trait, with one concrete `AnthropicMind<T: MindTransport>` that speaks the Anthropic `/v1/messages` wire format, plus a `StubMind`. Model tiers (synthesis = Fable 5, triage = Haiku) are config defaults. We want the operator to be able to point any tier at any model/provider — a commercial Anthropic/other API, or a local/open model (Hermes, Llama-class, etc.) served over an OpenAI-compatible endpoint — chosen in config, with per-model pricing feeding the existing cost budgets, per-provider structured-output enforcement that preserves the "reject schema-invalid output, never repair it" discipline, and full provenance/audit. The result: swapping the brain (or running a cheaper/local brain for the cheap tier) is a config + (for live) a shadow-comparison decision, not a code change.
>
> **2. Hard boundaries (non-negotiable).**
> - **I6 (propose-only) holds for EVERY provider.** Any model, local or remote, has zero tools that mutate external state; output is structured beliefs/proposals only; sizing/timing/execution stay in the harness. A local model is not more trusted than a remote one.
> - **Structured-output discipline (5.9):** schema-invalid output is rejected and logged, **never silently repaired**. Anthropic enforces this via tool-use/`json_schema`; an OpenAI-compatible/local endpoint may use `response_format` json-schema, a GBNF grammar (llama.cpp), or prompted-JSON — whatever the transport uses, the harness MUST strictly validate and reject on mismatch (numeric range re-validation in code, as `AnthropicMind` already does for p∈(0,1), price∈[1,99]). Designing this portability is the heart of the feature.
> - **I5 (audit):** every model call writes prompt hash, context-manifest hash, `model_id`, and cost. **`model_id` is load-bearing** — it is the calibration scope (5.10, per `model_id`), the belief provenance stamp, and the shadow pairing key. New models = new cold calibration scopes (shrink-to-market until n≥50).
> - **I7 (promotion) governs LIVE model swaps.** Selecting a model for Sim/shadow is free; **promoting a model into the live decision flow requires the shadow-mode comparison** (`shadow.rs::evaluate_model_swap`, ≥30 paired resolutions per active category, Brier/CLV ≥ incumbent) and is an operator action. Your config surface must make "what runs live" vs "what runs in shadow" explicit and must not let a config edit silently swap the live brain past the gate.
> - **5.9 budgets:** per-cycle and per-day cost caps, checked BEFORE the call; pricing is per-model config (cents/MTok in/out, or $0 for a local model — but a local model still has a token/latency budget). Budget breach degrades to mechanical-only + alert.
> - **Determinism / Clock:** all time via the injected `Clock`; no wall-time. The model call is the non-deterministic edge — tests use `StubMind`/scripted transports; never hit a live endpoint in tests or DST.
> - **Secrets:** API keys/endpoints via env only — never in repo, config files, logs, or audit payloads (the existing `ReqwestMindTransport` redacts the key in Debug; preserve that for every transport). Local endpoint URLs are config; any auth token is env.
> - **House style (CLAUDE.md):** Rust 2021; no `panic!`/`unwrap`/`expect` in the cognition path; `thiserror`; structured output via serde with `deny_unknown_fields`; `cargo fmt` + `clippy -D warnings` + full workspace tests + `scripts/run-dst.sh` all green; tests written from the spec BEFORE implementation; **never weaken `crates/fortuna-invariants/`.**
>
> **3. The shape to design (a starting point, improve it).**
> - A **per-tier (and optional per-persona / global) model config**: `{ tier → { provider, model_id, base_url?, max_tokens, input_price_cents_per_mtok, output_price_cents_per_mtok, structured_output_mode } }`. Decide the granularity (global default + per-tier override + maybe per-persona) and justify.
> - A **provider abstraction.** `MindTransport` already isolates the wire call (`post_messages(Value) -> (u16, Value)`). Likely you add an **OpenAI-compatible transport** (covers Hermes/local llama.cpp/vLLM and many commercial APIs) alongside the Anthropic one, plus a per-provider request/response **adapter** (prompt format, system-vs-user role mapping, structured-output mechanism, token-usage extraction for pricing). Keep `Mind`'s trait surface stable — the persona track (Track E) and the decision cycle depend on it.
> - A **capability descriptor** per provider/model: does it support native json-schema/tool-use structured output, or must the harness fall back to grammar/prompted-JSON + strict validation? This drives the structured-output mode and is the load-bearing portability question.
> - A **factory** (`mind_from_config`/extend `mind_from_env`) that builds the right `Mind` per tier from config + env, fails loud on a misconfigured/unreachable provider (degrade to mechanical-only is explicit, never accidental), and stamps the correct `model_id`.
>
> **4. Explore before you design (use the Explore subagent).** Map the real model layer — verify, don't trust this brief:
> - `crates/fortuna-cognition/src/mind.rs`: the `Mind` trait (`decide`, `id`, `begin_cycle`, `spent_today_cents`), `AnthropicMind<T: MindTransport>`, `AnthropicMindConfig` (model, max_tokens, prices, `system_charter`), `MindTransport` + `ReqwestMindTransport` (env key, Debug redaction), `CostBudget` (per-cycle/per-day, check-before-call, record-spend-even-on-reject), `mind_from_env()` factory, `output_schema()` + the post-parse domain validation.
> - `crates/fortuna-cognition/src/shadow.rs`: `ShadowHarness`, `evaluate_model_swap`, the manifest-hash pairing key — the I7 model-swap gate your config must respect.
> - `crates/fortuna-cognition/src/review.rs`: monthly model-version evaluation (shadow results).
> - The `[cognition]` config (FortunaConfig + `config/fortuna.example.toml`): how tiers, prices, and `allow_stub_mind` are wired today; how the daemon composes the mind (`fortuna-live`).
> - How `model_id` flows into belief `provenance` and the calibration scope (`calibration_params`, `ScopeKey`).
>
> **5. Definition of done.** A committed, operator-approved design doc under `docs/design/`; then an implementation that (a) ships the per-tier model config + the provider abstraction (incl. at least one OpenAI-compatible transport) + the capability-driven structured-output enforcement + the factory, (b) is proven by routing a tier to a non-Anthropic model behind a scripted transport in tests (no live endpoint), (c) has tests written from the spec first — incl. structured-output rejection across providers, per-model budget/pricing, secrets-never-logged, and `model_id`/provenance correctness — plus a DST scenario for a provider failure degrading to mechanical-only, and (d) passes the full battery (fmt, clippy -D warnings, workspace tests, run-dst) with `fortuna-invariants` untouched. Update `GAPS.md`/`ASSUMPTIONS.md` for every spec-silence you resolve.
>
> **6. Coordination.**
> - Track E (domain-analysis personas) owns `crates/fortuna-cognition` persona/analysis code; personas declare a **tier**, and your factory is what resolves a tier to a model. **Do not change the `Mind` trait signature** without coordinating with Track E and the decision cycle — both depend on `decide(&AssembledContext) -> Result<MindOutput, MindError>`.
> - Track D owns `crates/fortuna-sources`; not your concern.
> - Work on your own branch/worktree; rebase on main; never weaken the invariant crate; never push without being asked.
>
> **7. The opening question to bring to the operator (after exploring, before finalizing the design).** Surface the load-bearing decision: **how is structured output enforced for providers that lack Anthropic-style tool-use/json-schema** (native `response_format` json-schema where supported; GBNF/grammar for local llama.cpp; or prompted-JSON + strict reject as the universal fallback) — and the **granularity of model selection** (global default only, vs per-tier, vs per-tier + per-persona override). Recommend: per-tier override on a global default (matches the existing tier model), with prompted-JSON-+-strict-validation as the portable floor and native json-schema used where the provider supports it. Confirm what "openclaw" refers to so the first non-Anthropic target is concrete.
