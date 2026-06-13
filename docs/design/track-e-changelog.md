# Track E — domain-analysis personas: changelog

Track-owned changelog (newest first). Every entry = one gate-clean slice with its
commit, what landed, and how it was verified. Authoritative design:
`docs/design/domain-analysis-personas-design.md` (§18 = the six-slice plan).
Shared-doc touches are listed per entry so nothing goes stale silently.

Convention: one slice per iteration, tests-first, FULL workspace battery as the
commit gate, `fortuna-invariants` untouched except at E.3 (operator-waive-flagged).

---

## E.4b — SectionKind::DomainAnalysis context section (§9) (this commit)

Added the `DomainAnalysis` variant to the shared `SectionKind` enum
(`context.rs`), inserted just under `OpenBeliefs` (high priority, per §9) + its
`as_str` arm, and `persona_beliefs::domain_analysis_context_item` — builds a
high-priority context item from a persisted artifact so the synthesis Mind reads
the persona's pre-digested findings as ONE high-value item alongside the raw
signals. The item is DATA (the findings rendering + the artifact anchor in the
body), NOT the trusted method (which still rides only in the Mind system message,
§4). The item content_hash follows the assembler convention (hash of the rendered
body), so it passes the assembler's hash-verification; the item_id is the
analysis_id and the artifact anchor is in the body (replayable, 5.7).

SHARED-ENUM SAFETY (the risk): verified there is NO exhaustive match on SectionKind
anywhere except `as_str` (updated), NO numeric discriminant cast, serde is
string-based (existing variants' wire form unchanged), and the Ord insertion
preserves every pre-existing variant's relative order. The existing `context` test
still passes. 3 new tests (the Ord priority chain, the item's fields + hash
convention + anchor-in-body, and that the artifact renders + packs before signals
via assemble_context). Full battery green. feature-dev:code-reviewer: confirmed all
shared-enum/content-hash/§4 checks CLEAN; two test-strengthening items (full Ord
chain incl. Lessons/Episodic + the specific data-wrapping assertion) applied.
fortuna-invariants UNTOUCHED.

This COMPLETES E.4 (E.4a fan-out + E.4b context section). The Track-E build is now
done end-to-end; what remains is operator/Track-A-gated (the §15 invariant pin, the
§10 ScopeKey + live daemon wiring) plus the macro-economist GENERALIZATION proof
(§17 — a second persona def, Track-E-buildable, proving one-mechanism-not-per-domain).

## E.6 — end-to-end meteorologist proof (the capstone) (commit ccdaeca)

New `crates/fortuna-ledger/tests/persona_e2e.rs` (design §9/§10/§11) — one
`#[sqlx::test]` wires the WHOLE persona pipeline on the real ledger DB:
register a `personas` row → load the SHIPPED `config/personas/meteorologist` def +
`validate_against` the registry head (the method_hash binds, round-tripped through the
DB) → `run_persona_analysis` with a scripted StubMind (the §12 spike findings) →
persist a `domain_analyses` row → `map_persona_analysis` fan-out to 3 BINARY beliefs →
persist events + beliefs → resolve + `resolved_stats` → `score_persona` +
`propose_promotion`. Asserts: every belief REPLAYS to the persisted artifact (its
provenance carries the analysis_id AND the content_hash anchor, and that
`domain_analyses` row round-trips the same hash); the §11 gate is `Evaluating`
(zero-capital) at low n; and the persist path never injects method text. Boundary-clean
— uses only the Track-E repos (Personas/DomainAnalyses/Events/Beliefs) + cognition
logic, NOT the daemon (mirrors the existing `aeolus_eval` test; `BeliefsRepo::insert`
directly).

Full battery green. feature-dev:code-reviewer: 1 Critical (the firewall assertion is
vacuous vs a StubMind → reframed as a persist-path sanity check pointing to E.3a's
SpyMind test for the firewall proper) + 1 Major (the content_hash replay anchor was
unasserted → now checked on all 3 beliefs) — both fixed; confirmed boundary-clean + the
§11 gate. fortuna-invariants UNTOUCHED.

This is the Track-E build CAPSTONE: the persona pipeline is proven end-to-end in code.
What remains is COORDINATION/operator work, ledgered in GAPS: E.4b (the
SectionKind::DomainAnalysis context-section for the synthesis-Mind path), the §15
PersonaOutcome invariant pin (operator-waive), the §10 ScopeKey + daemon weekly-review
wiring (Track-A coordination), and the live daemon wiring that runs personas on the
real loop (Track-A coordination; this slice proves the pieces connect).

## E.5a — persona scoring & promote/retire proposal (§10/§11) (commit 1009bb8)

New `fortuna_cognition::persona_scoring`: `PersonaScope{persona_id, persona_version}` +
`score_persona` (Brier / calibration-quality / CLV via the existing
`calibration_curve`/`calibration_quality` primitives) + `propose_promotion` — the §11
evaluation gate: below `min_resolved` → `Evaluating` (scored, ZERO capital); at/above →
`Promotable` iff it beats the no-persona AND the market baseline (Brier ≤ both) with
positive CLV, else `RetireCandidate`. RECOMMENDATION-ONLY (the I7 analog — the daemon
never self-promotes); compares against the prior version too.

BOUNDARY DECISION (Fit-validation §21): §10 says "extend the review ScopeKey", but
`ScopeKey` is a struct literal in Track A's `daemon.rs:1024` — mutating it breaks Track
A's composition (loop forbids touching it unilaterally). So this is an ADDITIVE parallel
`PersonaScope` reusing the SAME calibration arithmetic; folding persona dims into the
shared ScopeKey + the daemon wiring is a GATED Track-A coordination (GAPS). No arithmetic
loss; I7 preserved.

9 tests (incl. the exact-floor boundary + empty-record quality-finite). FULL workspace
battery green. feature-dev:code-reviewer: 2 Important — the CLV check was structurally
tied to the market check (→ refactored to three independent §11 booleans) and the
ScopeKey deferral needs a GAPS entry (→ added) — plus 2 Minors (exact-floor + quality
tests → added). fortuna-invariants UNTOUCHED. Shared-doc: design §21 Fit-validation note.

## E.4a — belief consumption: μ/σ→p backbone + artifact→belief fan-out (commit c1c1b55)

New `fortuna_cognition::persona_beliefs` (design §9):
- `normal_cdf` / `prob_at_least` — the μ/σ→p backbone (`1 − Φ((t−μ)/σ)`) via an
  A&S erf approximation, clamped to (ε, 1-ε) so deep-tail values stay valid belief
  probabilities. Deterministic Rust the runner FEEDS the persona (the LLM never does
  the arithmetic, §9); reproduces the §12 spike backbone (≥60≈0.92, ≥65≈0.41).
- `map_persona_analysis` — fans a persisted artifact's `findings` onto one BINARY
  `BeliefDraft` per `thresholds[]` (weather) / `outcomes[]` (macro), mirroring
  `map_aeolus_envelope`. Belief `p` = the persona's stated p (artifact authoritative);
  `evidence` cites `persona:<id>@<v>` + the analysis_id; `provenance` carries
  `{persona_id, persona_version, analysis_id, analysis_content_hash}` so the belief
  replays to the artifact (I5/5.7). event_ids are `ge…`/`out:…`-prefixed (no
  cross-branch collision) and de-duplicated. Builds on the existing BINARY belief
  ledger — independent of any scalar-claim type.

12 tests. FULL workspace battery green. feature-dev:code-reviewer (confirmed the
LLM-no-arithmetic separation is correctly implemented): two Major — deep-tail
saturation to exact 0/1 (→ clamp + test) and event_id collision risk (→ distinct
prefixes + raw labels + a DuplicateEvent dedup + tests) — both fixed. fortuna-invariants
UNTOUCHED. REMAINING in E.4: E.4b (SectionKind::DomainAnalysis — the artifact as a
high-priority context item for the synthesis-Mind path; the deterministic fan-out here
is the meteorologist's belief-consumption proof and needs no SectionKind).

## E.3 telemetry — persona-runner metrics (§19) (commit f65fd64)

New `fortuna_cognition::persona_metrics`: `PersonaCounters` folds `PersonaOutcome`s
into the operator funnel — `runs → analyses`, with the degrade counters
(`budget_skips`, `no_signal_skips`, `run_failures{reason}`, `triggers_coalesced`)
explaining every drop, the cumulative `cost_cents` counter, and the daily
`spend_today_cents` GAUGE (resets on the UTC-day roll, mirroring
`fortuna_mind_spend_today_cents`). `samples()` emits `PersonaMetricSample`s
shape-compatible with the runner's `MetricSample` (name/help/counter/labels/value),
so the composition drains them into fortuna-ops's integer-only registry through the
SAME loop — no new telemetry infra; persona-agnostic labels. Test-pinned accounting
identity: `runs == analyses + budget_skips + no_signal_skips + sum(failures)`.

10 tests. FULL workspace battery green. feature-dev:code-reviewer: two Major — the
§19 `reason` enum listed `context`, but context-assembly is the runner's ONE hard
error (not a counted defect) → design §19 reconciled (reason ∈ provider /
schema_invalid / other-defensive); and the `spend_today_cents` gauge was missing →
IMPLEMENTED (day-roll). Plus a Minor (added the "other" + "no findings journal"
tests). fortuna-invariants UNTOUCHED. Shared-doc touch (loop §8): design §19 row.
NOT-YET-WIRED: the composition (E.6 / a Track-A drive() seam) maps `samples()` into
the ops registry; this slice provides the fold + the names.

## E.3c — persona runner DST arm (seeded, under the cost budget) (commit 510ee8e)

New `crates/fortuna-cognition/tests/persona_dst.rs` (design §8/§15), wired into
`scripts/run-dst.sh` (`PERSONA_DST_SCENARIOS`, default 20; battery runs 2000). Each
seed builds a chaos world — 0..=4 point-in-time signals (0 exercises the skip path),
a random possibly-pre-exhausted `DiscoveryBudget`, and a call-counting `ChaosMind`
spanning all failure modes (provider error / unknown / missing-required / non-JSON
prose / empty journal). Per-seed invariants: never panics/errors (degrade); budget
throttle ⟹ no call/artifact/spend; no signals ⟹ skip; a reached run calls the mind
EXACTLY once and yields an artifact iff Valid (every anchor set) else a counted
defect; byte-identical content_hash on replay; and an INTEGRATION coalescing arm —
K+1 triggers through a gate threaded into the runner produce exactly ONE run (one
mind call). Passes at 2000 seeds (≈113 artifacts / 1130 throttled / 173 skipped).

feature-dev:code-reviewer (self-corrected several false positives): two confirmed —
the coalescing arm was a GATE-only unit test (didn't prove "one run") → reworked to
thread the gate through the runner with a counting mind; and the skip path was
unreachable (signals always ≥1) → 0..=4 signals now exercise it. Plus a clippy
`!= !` simplification fixed. fortuna-invariants UNTOUCHED.

Shared-doc touch (loop §8): `docs/verification.md` DST-harness count corrected
4→6 (it was already stale — omitted the perp arm; now lists perp + the persona arm).

## E.3b — persona trigger layer (declarative + schedulable) (commit 96cdb79)

`fortuna_cognition::persona_trigger` (design §7): the layer that decides WHEN a
`(persona, region)` run fires, decoupled from the persona's method.
- `Cadence` (EveryHours / DailyAtHourUtc) + `CadenceScheduler::due` — fire-once-per-period,
  generalizing the daemon's `DailyScheduler` (in-process state; not persisted across
  restarts — documented, GAPS-noted). `Cadence::validate()` rejects a never-fires config
  (DailyAtHourUtc hour ≥ 24) at config-load.
- `PersonaTriggerSpec::fires_on_signal` — signal-driven matching straight from the persona's
  `reads_signal_kinds` (config, not per-domain code).
- `PersonaTriggerGate` — REUSES the existing `signals::TriggerEngine` (unmodified) keyed by
  `persona_region_key` for per-`(persona, region)` serialization + debounce: duplicate/concurrent
  triggers coalesce into ONE in-flight run (the §8 "coalesced re-triggers → one run"), with the
  coalesced count reported. Key uses the 0x1F unit separator (collision-safe).

9 tests (tests/persona_trigger.rs). FULL workspace battery green. feature-dev:code-reviewer:
two Major (hour ≥ 24 silent-never-fire → `validate()` + test; in-process fire-once contract →
documented + GAPS) + a Minor (key-separator collision → 0x1F + test) + a Nit (fire-on-trigger
doc) — all applied. fortuna-invariants UNTOUCHED. No shared-doc edit needed (nothing made stale).

## E.3a — persona runner core + the trusted/untrusted firewall (commit 4e8b9e4)

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
