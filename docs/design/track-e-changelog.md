# Track E — domain-analysis personas: changelog

Track-owned changelog (newest first). Every entry = one gate-clean slice with its
commit, what landed, and how it was verified. Authoritative design:
`docs/design/domain-analysis-personas-design.md` (§18 = the six-slice plan).
Shared-doc touches are listed per entry so nothing goes stale silently.

Convention: one slice per iteration, tests-first, FULL workspace battery as the
commit gate, `fortuna-invariants` untouched except at E.3 (operator-waive-flagged).

---

## E.3a — persona runner core + the trusted/untrusted firewall (this commit)

`fortuna_cognition::persona_runner` (design §8a): `run_persona_analysis(persona,
region_key, signals, mind, budget, now) -> PersonaOutcome`. Budget-first
(throttle-before-spend, `DiscoveryBudget`), assembles ONLY the untrusted signals
into the context (the trusted method is the Mind's system charter, never a
`<context-item>` — the §4 firewall), one `Mind.decide`, parses findings from the
journal body and strictly validates them against the persona's `schema.json`
(config-driven: required keys + `additionalProperties:false`), stamps the
`content_hash` replay anchor over `{findings, signal_manifest}`. `PersonaOutcome` is
order-free (mirrors `ReconciliationOutcome`, I6) — a draft the composition persists.

Degrade arms (never crash): budget exhausted → throttle; no in-window signals →
skip; mind failure / non-JSON findings / schema-violating findings → a counted
defect, `Ok` returned. Determinism: a scripted `StubMind` → byte-identical artifact
+ `content_hash` (no live endpoint in any test).

Tests (11, tests/persona_runner.rs): the headline firewall (a planted injection in
an untrusted signal is rendered AS DATA; the method marker never appears in the
context), determinism, the strict-findings degrade arms, the budget/skip arms, and
the shipped meteorologist running end to end against a scripted finding.

Review (feature-dev:code-reviewer): one Major — `validate_findings` skipped the
unknown-key check when `additionalProperties:false` but `properties` was absent →
FIXED (every key forbidden) + a regression test; and the Critical §15 invariant pin
deferred to E.3c (operator-waive, see GAPS). `PersonaOutcome` gained `#[derive(Serialize)]`.

Shared-doc touches (per loop §8): `docs/architecture.md` §3 (cognition crate-map entry
gains the persona-layer paragraph); this changelog (new); `docs/design/implementer-loop-track-e.md`
§8 (the operator's documentation-discipline directive added as a standing loop rule).
Deferred to E.3b/E.3c: the trigger layer (§7), the DST-under-budget arm, persona telemetry
(§19), and the `PersonaOutcome` no-order/size field-surface invariant pin (§15, the first
`fortuna-invariants` touch, operator-waive-gated).

## E.2 — persona skill-file loader + method_hash registry validation (commit d6e8c23)

`fortuna_cognition::persona`: `PersonaDef::parse(persona_md, schema_json)` parses
TOML `+++` frontmatter + the trusted method body, computes `method_hash` = SHA-256 of
the whole `persona.md`, loads `schema.json`. `validate_against(Option<&RegistryHead>)`
is fail-closed (only `status=="active"` runs) and refuses NotRegistered / Inactive /
VersionMismatch / HashMismatch — the §4(d)/§6 headline. Pure core (no fs IO);
`RegistryHead` is a pure cognition input (cognition has no ledger dep). Shipped the
meteorologist persona on disk (`config/personas/meteorologist/`). 14 tests; full
battery green; feature-dev review applied (status fail-closed + `split_frontmatter`
`.get()` hardening).

## E.1 — personas + domain_analyses ledger (commit dfdf3e0)

Migration `20260613000001_personas.sql`: the append-only `personas` registry
(supersedes-chained, `UNIQUE(persona_id,version)`, `fortuna_refuse_mutation`) + the
content-immutable `domain_analyses` artifact (a dedicated guard freezes all 12
content columns; only `status` flips; `content_hash` over findings+signal_manifest is
the I5/5.7 replay anchor). `PersonasRepo` + `DomainAnalysesRepo`. 6 mutation-proven
`#[sqlx::test]`s; full battery green. Also committed the operator-requested telemetry
(§19) + detailed ROTA view contracts (§20).

## Design phase — committed + operator-approved (commit b4eaae3)

`docs/design/domain-analysis-personas-design.md` (§2 artifact-model = persisted,
operator-endorsed; the trusted/untrusted firewall as the heart; the six-slice §18
plan). Spike-validated 2026-06-13 (§12). Track M (per-tier model providers) parked.
