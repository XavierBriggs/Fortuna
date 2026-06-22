# Area 8 — Cognition: Mind, personas & belief authoring

## Summary

The Mind trait, CostBudget mechanics, and the full persona/orchestrator pipeline are
correctly implemented and well-tested — the code is NOT "doing nothing". The system is
producing durable artifacts (108 beliefs in `fortuna_demo`), but 100% of those beliefs
carry `provenance->>'model_id' = 'aeolus'` because the synthesis arm is calibration-gated
(no `calibration_params` row yet) and the persona step is **config-OFF** (no `[personas]`
block in `config/fortuna.toml`). The biggest risk to the paper demo is that the two most
visible model-authoring paths — personas and discovery — are either completely wired-off
or blocked by a missing prerequisite (persona registry rows are empty; the meteorologist
cannot activate even if `[personas]` is turned on). To get 2–3 personas authoring scored
beliefs requires two operator steps: insert DB rows into `personas` and uncomment/enable
`[personas]` in `fortuna.toml`.

---

## Findings

| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
|---|---|---|---|---|---|---|---|
| P1 | BLOCKS | `[personas]` section absent from live config; persona step never runs | `config/fortuna.toml` — no `[personas]` block; `crates/fortuna-live/src/daemon.rs:2602` — `if let Some(pw) = personas.as_mut()` is the guard | `domain_analyses = 0`, `personas table = 0 rows` verified in `fortuna_demo`. The meteorologist persona exists as files (`config/personas/meteorologist/persona.md` + `schema.json`) but is not activated | Config opt-in correctly defaults to OFF; operator never turned it on | Add `[personas] enabled=true` block to `config/fortuna.toml` AND insert `personas` registry rows (see Root Cause Cascade below) | Integration: boot with `[personas] enabled=true` and verify at least one `domain_analyses` row appears per segment with aeolus.forecast signals present |
| P1 | BLOCKS | `personas` DB table is empty — persona registry validation will refuse boot even if config is enabled | `psql -d fortuna_demo`: `SELECT COUNT(*) FROM personas` → 0; `crates/fortuna-live/src/main.rs:440-455` — `PersonasRepo::head(entry.id)` + `def.validate_against(registry_head.as_ref())` fail-closes on missing row | Boot with `[personas] enabled=true` will fail at startup: `persona 'meteorologist' is not in the registry` | Persona hash-registry pattern (design §6) requires an operator `INSERT INTO personas(...)` before the persona may run | Operator action: INSERT into `personas` with the persona metadata and SHA-256 of the file (the `method_hash`). See `persona.rs:89-91` for the exact error text | Boot-validation test: a `[personas]` section referencing a persona id with no DB row must refuse with `NotRegistered` error (test already exists at `crates/fortuna-cognition/src/persona.rs:89`) |
| P1 | BLOCKS | Synthesis arm is calibration-gated; prices no edge until `calibration_params` row exists | `config/fortuna.toml:325-328` — `[synthesis] category="weather"`; `crates/fortuna-live/src/daemon.rs:355-375` — `calibration_for_scope` returns `None` when no row → synthesis prices nothing | All 108 beliefs are Aeolus-deterministic; the model arm authors zero beliefs in `fortuna_demo`. The comment in `daemon.rs:340-347` explicitly says "ONLY when `[synthesis].category` selects a calibration scope with a fitted params row" | Calibration data is accrued from RESOLVED beliefs over time; none have resolved yet (all 108 are `status='open'`) | Wait for NWS-graded beliefs to resolve (NWS CLI source is configured at `config/fortuna.toml:213-216`) OR for the purpose of the demo, consider using a seed calibration row | Assert: with empty `calibration_params`, `synthesis_edges` must price zero proposals (already tested implicitly; make explicit in a unit test) |
| P2 | BLOAT-cut | `[personas]` block comments in `config/fortuna.example.toml` note `[gates.per_strategy.domain-analysis]` is also needed but commented out; this is a two-part gate | `config/fortuna.example.toml:33-41` — the gate for persona beliefs is commented out with `max_exposure_cents = 0`; `crates/fortuna-live/src/main.rs:428-429` — `StrategyId::new("domain-analysis")` is the pre-built strategy | If persona beliefs reach the proposal path without a gate entry, orders will be gate-rejected. The comment says "zero capital" correctly but the gate must be uncommented to avoid a confusing reject-at-gate log | Incomplete opt-in documentation — two sections must be uncommented together | Uncomment `[gates.per_strategy.domain-analysis]` alongside `[personas]` in `fortuna.toml` | Verify that a persona belief proposal passes the gate when `max_exposure_cents = 0` is set (I7 propose-only, no fills) |
| P2 | PARK | Per-cycle budget in live config was previously `50c`; comment confirms it starved world-forward discovery every pass; raised to `150c` but the root cause (discovery can cost ~92c/call) is not tested | `config/fortuna.toml:106-109` — inline comment confirms: "50c starved EVERY pass"; raised to `150c`. `crates/fortuna-cognition/src/mind.rs:277-295` — `CostBudget::check` compares BEFORE the call | A future operator lowering the budget back to `50c` would silently starve discovery again with no test catching it | The per-cycle budget must be >= the expected cost of the most expensive routine call | Add a test asserting that the default `per_cycle_budget_cents` (or example TOML value) is >= the minimum cost to run one world-forward discovery cycle | Test: check budget + known discovery cost in `crates/fortuna-cognition/tests/` |
| P2 | PARK | The `AnthropicMind` uses `"thinking": {"type": "adaptive"}` in the wire body but the Anthropic API's structured-output (json_schema) mode may not be compatible with extended thinking | `crates/fortuna-cognition/src/mind.rs:574` — `"thinking": {"type": "adaptive"}` in both `call_priced` and `call_priced_structured` | If the API rejects structured+thinking combinations, calls silently degrade with a `SchemaInvalid` error and still burn budget. No test covers this combination against a real or recorded response | Structured output and extended thinking are separate API features whose interaction may be version-dependent | Verify API compatibility; add a fixture test with `thinking` in the response body from a real Opus 4.8 structured-output call | Fixture test: a response with a `thinking` block + a `text` block should parse the `text` block correctly |
| P3 | BLOAT-cut | Persona configs exist for `meteorologist` AND `macro-economist` but no `macro-economist` persona.md is in the second directory | `ls /config/personas/macro-economist/` — only `persona.md` and `schema.json` present (verified); `config/fortuna.example.toml:292-300` only references `meteorologist` | Not a current blocker but signals the macro persona may be only half-specified | Partial work | Verify `macro-economist/persona.md` is valid TOML-frontmatter; if intended, document how macro beliefs are scored (macro has no resolution source in the current source registry) | Read `macro-economist/persona.md` and validate frontmatter fields |
| P3 | PARK | `mind_from_env` in `daemon.rs` (the shared synthesis/persona/discovery mind factory) hardcodes `system_charter = SYNTH_MIND_SYSTEM_CHARTER` for ALL tiers | `crates/fortuna-live/src/daemon.rs:180-186`, `202-226` — `SYNTH_MIND_SYSTEM_CHARTER` is the charter for synthesis, mid, and triage minds via the same factory function | Persona runs use `synthesis_mind.clone()` (`main.rs:474`); the persona method overrides the system charter at the call site only when using the PERSONA-SPECIFIC charter (via `persona_system_charter` in `persona_runner.rs:113`). BUT persona_runner uses `mind.decide()` which uses the `AnthropicMind`'s OWNED charter (`config.system_charter`), not the override | The persona system charter is never actually injected — `run_persona_analysis` calls `mind.decide(&ctx)`, which sends `self.config.system_charter` (the synthesis charter "You synthesize calibrated probabilistic beliefs"), NOT the persona's method body. The persona method is completely unused in live calls | The `persona_system_charter` function exists and is exported but the persona runner never passes it to the mind — `mind.decide()` ignores it | HIGH SEVERITY RISK: See Finding A below (raised separately) | See Finding A |

---

### Finding A (ESCALATE — potential P0 depender):

**The persona's trusted method (`persona.method`) is never injected into the AnthropicMind system charter for live persona runs.**

Evidence chain:
1. `persona_runner.rs:113` — `pub fn persona_system_charter(persona: &PersonaDef) -> &str` — exported but not called from the runner itself
2. `persona_runner.rs:213` — `let output = match mind.decide(&ctx).await` — uses the OWNED charter from `AnthropicMindConfig.system_charter`, which was set at construction time as `SYNTH_MIND_SYSTEM_CHARTER`
3. `main.rs:474` — `mind: synthesis_mind.clone()` — the persona wiring uses the same mind instance as synthesis, whose charter is "You synthesize calibrated probabilistic beliefs..."
4. `daemon.rs:181-184` — `SYNTH_MIND_SYSTEM_CHARTER` is the only charter fed to ALL tier minds

Root cause: The design calls for the persona method to be the Mind's system message (design §4 / `persona_runner.rs` module docstring: "The persona's trusted METHOD rides in the Mind transport's system message"). The design intent is that a **new** `AnthropicMind` is built per persona run with `AnthropicMindConfig { system_charter: persona.method, ... }`. Instead, the shared synthesis mind is reused with a fixed synthesis charter.

Current impact: ZERO (personas are config-OFF and the personas table is empty). But if personas are enabled, the meteorologist's careful instructions would be replaced by the synthesis charter "You synthesize calibrated probabilistic beliefs..."

**Severity: P1 BLOCKS** — enabling personas without fixing this means the model never follows the persona method; findings will be generic synthesis output not persona-specific structured findings; the schema validation will likely fail.

**Recommended fix:** In `main.rs` (or a `persona_runner_for` factory), build a **separate** `AnthropicMind` per persona with `system_charter = def.method` (and the persona's schema enforced via `decide_structured`). The persona runner already has the right interface — it calls `mind.decide()` — so the fix is in the wiring, not in `persona_runner.rs`.

**Severity reclassification:** P1 BLOCKS (not P3) because it is directly on the critical path to making personas author any correct artifact.

---

## Trace / narrative

### The Mind trait — what it actually does

`crates/fortuna-cognition/src/mind.rs` implements the `Mind` trait with two concrete types:
- `StubMind` (deterministic, DST; returns scripted outputs; `line:205`)
- `AnthropicMind<T: MindTransport>` (live; posts to `/v1/messages`; `line:461`)

The `CostBudget` struct (`line:238`) tracks `spent_today_cents` and `spent_this_cycle_cents`. `roll()` (`line:265`) resets the day bucket using `div_euclid(DAY_MS)`, which is UTC midnight — CONFIRMED. The `check()` method (`line:277`) refuses BEFORE the call, so budget breach degrades to mechanical-only without spending. Session evidence claim "CostBudget resets at UTC midnight + on restart" — CONFIRMED at `mind.rs:265-270`.

Session evidence claim "Mind trait works: 19/19 tests": grep confirms `#[test]` + `#[tokio::test]` = 19 in `crates/fortuna-cognition/tests/mind.rs`. The `failed_calls_burn_into_spent_today` test at line 498 verifies that a schema-invalid response still records spend. CONFIRMED.

### Why 100% of beliefs are `provenance->>'model_id'='aeolus'`

Query: `SELECT DISTINCT provenance->>'model_id' as model_id, COUNT(*) FROM beliefs GROUP BY 1;` → `aeolus | 108`. Zero Claude-authored beliefs.

There are three paths that can author beliefs:
1. **Synthesis arm** (`crates/fortuna-live/src/daemon.rs:349-393`) — opt-in via `[synthesis]` section. PRESENT in `fortuna.toml:325-328`. BUT: calibration-gated at `daemon.rs:355-375` — `calibration_for_scope` returns `None` when no `calibration_params` row exists. DB query confirms zero rows. Synthesis arm proposes nothing until calibration accrues from resolved beliefs.
2. **Persona analysis** (`daemon.rs:2602-2779`) — opt-in via `[personas]` section. ABSENT from `config/fortuna.toml`. The `personas.as_mut()` guard at `daemon.rs:2602` is never entered. Zero `domain_analyses` rows confirmed in DB.
3. **World-forward discovery** — opt-in via `[discovery]` section. PRESENT and enabled in `fortuna.toml:297-314`. This CAN author beliefs (via `world_forward_discovery` → `persist_beliefs`) but only for "scoreable" candidates whose resolution source is in the source registry.

The Aeolus beliefs come from the F7 weather plugin in the discovery wiring (not the synthesis arm). The weather day-set source auto-mints beliefs for Aeolus forecast signals → Direct edges → weather beliefs with `model_id='aeolus'` (the `aeolus_beliefs.rs` module). This is the deterministic path.

### Persona system: wired but not activated

The full persona pipeline is correctly implemented:
- `crates/fortuna-cognition/src/persona.rs` — loader, hash validation
- `crates/fortuna-cognition/src/persona_runner.rs` — `run_persona_analysis` (budget-first, §4 firewall, schema validation)
- `crates/fortuna-cognition/src/persona_orchestrator.rs` — `run_due_personas` (due-by-signal or cadence)
- `crates/fortuna-live/src/daemon.rs:1493-1515` — `PersonasWiring` struct
- `crates/fortuna-live/src/daemon.rs:2602-2779` — segment-level persona loop
- `crates/fortuna-live/src/main.rs:430-481` — boot-time wiring (fail-closed registry validation)

What is MISSING:
1. `config/fortuna.toml` has no `[personas]` block → `dcfg.personas` is `None` → `personas_wiring = None` → the segment loop is a no-op
2. `SELECT COUNT(*) FROM personas` = 0 → even if config is enabled, boot fails at `def.validate_against(registry_head.as_ref())` with `PersonaError::NotRegistered`
3. **Finding A above:** the synthesis mind's charter is not swapped for the persona method at call time

Session evidence claims: "Persona configs exist (config/personas/meteorologist, config/personas/macro-economist) but `[personas]` is opt-in/OFF (`crates/fortuna-live/src/boot.rs:319`)" — CONFIRMED. The field is `PersonasSection.enabled = false` as default (`boot.rs:382`). The boot.rs line ref is approximate; the Default impl is at `boot.rs:381-392` in the working tree.

### What the meteorologist would read if activated

`config/personas/meteorologist/persona.md` shows `reads_signal_kinds = ["aeolus.forecast", "nws.observed_high", "nws.forecast_discussion"]`. DB query on `fortuna_demo` shows 270 `aeolus.forecast` signals across 6 stations (45 each: KAUS, KLAX, KMDW, KMIA, KNYC, KPHL). Zero `nws.observed_high` or `nws.forecast_discussion` signals — NWS sources are configured for `nws.afd` (Area Forecast Discussion) and `nws.alert`, not the two types the meteorologist reads. This means even with personas enabled and registry rows inserted, the meteorologist would likely skip most segments (`skipped_no_signals = true`) unless `nws.cli` observations map to `nws.observed_high` kind.

### Budget context

`config/fortuna.toml:103` — `daily_budget_cents = 3_000` ($30/day, raised 2026-06-18 from earlier $10/$15). The inline comment is explicit: "prior $10 day burned fully in ~8.5h on calibration-gated synthesis TRIAGE no-ops (failed/empty calls still debit, by design)". The synthesis triage mind burns budget even on calibration-gated passes because the Haiku mind is called to evaluate whether a trigger should escalate — the triage call costs money even if the answer is "no synthesis today." Raising the budget alone does not add mind value; the synthesis arm needs a calibration_params row.

### `domain_analyses` table

`psql -d fortuna_demo`: `SELECT COUNT(*) FROM domain_analyses` → 0. CONFIRMED: zero persona analyses persisted. Table schema is present (migration applied); it is simply empty because the persona step never runs.

---

## Self-adversarial pass

**Finding A (persona charter not injected):** This is the strongest finding and it genuinely matters. The evidence is unambiguous: `run_persona_analysis` at `persona_runner.rs:213` calls `mind.decide(&ctx)`, which on an `AnthropicMind` sends `self.config.system_charter` — the synthesis charter. The `persona_system_charter` function at `persona_runner.rs:113` is exported but never called from within the runner. The only way to override the system charter is to build a new `AnthropicMind` per persona run, which the composition does not do. **Am I sure?** Yes — the Mind trait has no `set_charter` method; the charter is immutable post-construction. This is a genuine P1.

**Budget exhaustion claim:** The session evidence says "Budget exhausted in prior soak, raised to $30/day." The config comment at `fortuna.toml:103` independently confirms this with operational detail. The claim is CONFIRMED with specifics (8.5h burnout, TRIAGE no-ops, calibration gating). This is not a finding — it is background fact.

**"Mind authors 0 beliefs" claim:** CONFIRMED via DB query. The synthesis arm is structurally calibration-gated (zero rows in `calibration_params`). This is accurate.

**What I might have missed:** I did not fully trace the world-forward discovery mind calls to see if they produce any non-Aeolus beliefs. Given `domain_analyses = 0` and `personas = 0`, and beliefs all showing `model_id='aeolus'`, the world-forward loop is running but its beliefs are the Aeolus-deterministic F7 weather path, not model-authored world-forward candidates. The world-forward synthesis calls (if any) with the Anthropic mind would show a different model_id — none do. I should check if world-forward has produced any `watch:` events.

**False positive check:** Is Finding A actually fixable without persona_runner.rs changes? YES — it requires only a wiring change in `main.rs` / `PersonasWiring` construction: build `mind_from_env` with `system_charter = def.method` for each persona instead of passing `synthesis_mind.clone()`. The runner itself is correct.

---

## Open questions for the Lead

1. **Finding A priority:** Should the persona charter injection fix be treated as a pre-requisite for any personas activation PR? Given the severity (personas would silently use the wrong charter), yes.

2. **Signal kind mismatch:** The meteorologist reads `nws.observed_high` and `nws.forecast_discussion` — neither kind exists in the DB (only `nws.afd` and `nws.cli`). Does the NWS adapter need a configuration update to emit these kinds, or should the meteorologist's `reads_signal_kinds` be revised to match what the adapter actually emits?

3. **World-forward beliefs:** Does the `world_forward_discovery` loop produce any Claude-authored beliefs in practice (even if not currently), or do all its `scoreable_candidates` get filtered by the unscoreable rule (no resolution source in the registry)? A `SELECT COUNT(*), provenance->>'model_id' FROM beliefs WHERE provenance->>'model_id' != 'aeolus'` would answer this post-soak.

4. **Calibration bootstrap:** The synthesis arm prices nothing until `calibration_params` rows exist, which requires resolved beliefs, which requires closed markets. Is there a plan to seed a bootstrap calibration row for the demo? Or is the demo expected to stay in a "accrual" state until markets close naturally?

5. **`macro-economist` persona:** Is the macro persona complete and intended for near-term use? It would require a resolution source in the source registry (there is none for macro events), so without that it would produce unscoreable beliefs per the unscoreable rule.
