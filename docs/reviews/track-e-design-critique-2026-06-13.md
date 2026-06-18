# Track E persona-system — adversarial DESIGN critique (design-gate evidence)

Date: 2026-06-13. Target: `docs/design/domain-analysis-personas-design.md` (407 lines)
on `track-e` @ 7c7ee7c (worktree fortuna-wt-e). Read-only critique; no code exists
yet, so this is a DESIGN gate, not a code gate. Rubric fixed before reading.
Verifier subagent + main-loop corroboration of the decision-critical finding.

## VERDICT: ACCEPT-WITH-CONDITIONS — design is sound; 3 precision corrections before build

The design is structurally sound, grounded in real code, and its single
decision-critical question resolves cleanly in its favor. The three conditions are
corrections to WHERE a mechanism attaches (not structural flaws) — an implementer
must not code to a structure that does not exist. None is a BLOCK. The operator
still owns the build-approval decision (track-E brief §3 RALPH STOP); this critique
is the evidence for it.

Branch is DOCS-ONLY (zero `.rs`/`.sql`/`.toml` changes; protected crate untouched;
no test weakening possible) — verified.

## DECISION-CRITICAL: Track E is INDEPENDENT of `prob_claims/v1` (the scalar-claims pass)

Unlike T5.B7 (perp funding_forecast) and the Aeolus weather signal — both blocked on
the unbuilt scalar claim type because `BeliefDraft` is binary-probability-only — Track
E's personas DO NOT hit that wall and can build now:

- `BeliefDraft` is binary-only: one `event_id`, one `p`, one `p_raw`, each validated
  strictly inside (0,1) (`crates/fortuna-cognition/src/beliefs.rs:53-74`).
- The existing Aeolus mapper already handles "per-bracket probabilities" by fanning one
  envelope into N independent binary drafts, one per bracket keyed
  `event_id="aeolus:{event_hint}"` (`reconciliation.rs:65-104`); discovery returns
  `Vec<BeliefDraft>` keyed per event (`discovery.rs:461,563-577`).
- Track E personas emit exactly this fan-out: meteorologist ≥60/≥65/≥70°F → three
  binary drafts; macro `outcomes[].p` per threshold → one binary belief per threshold
  (design §9, §13). The multi-outcome `findings.outcomes[].p` blob is the ARTIFACT (a
  `domain_analyses` row), never itself a belief — it FEEDS the binary fan-out.
- Nothing requires a single belief to carry a probability vector / continuous
  distribution / scalar-quantile claim. That is precisely B7's missing capability that
  Track E does not need.

Main-loop corroboration: the binary `BeliefDraft` shape and the Aeolus per-bracket
fan-out were independently known from prior grounding; the design's §9/§13 outputs match.

Corollary watch item: keep persona artifacts strictly as the artifact feeding the
binary fan-out; the moment a multi-outcome blob leaks into a single `BeliefDraft` is the
moment Track E would accidentally need `prob_claims/v1`.

## Rubric findings (evidence before verdict; real file:line)

**A. Invariant fit — MIXED.**
- (i) I6 no-order-field claim — RISK/overstated. The existing I6 dependency-direction
  check (`crates/fortuna-invariants/tests/i6_propose_only_mind.rs:131-162`) only reads
  `fortuna-cognition/Cargo.toml` deps; it does NOT inspect artifact/`PersonaOutcome`
  fields. The field guarantee comes from the field-surface tests pinning exact key-sets
  (same file, 99-127) — and there is no such test for a persona artifact yet. → Condition 3.
- (ii) I5 / §5.7 replay chain — CONFIRMED, genuinely closed. Real anchor is
  `content_hash_of` + `ManifestItem{item_id,section,content_hash}` → `manifest_hash`,
  and `assemble_context` verifies every item's hash fail-closed (`ContextError::HashMismatch`)
  before assembling (`context.rs:80-87,99-105,137-148,206-226`). The `beliefs_guard`
  content-immutability trigger (`migrations/20260609000001_initial.sql:78-98`) is the
  precise template §5 cites for `domain_analyses`.

**B. Trust firewall — structurally REAL but MISLOCATED in the doc. → Condition 1.**
The separation IS structural, but NOT where §4 says. The real firewall is the Mind
transport's system-vs-user split: charter goes as `"system": config.system_charter`,
context as the user message (`mind.rs:491-498`, `system_charter` field 374-377). §4
claims the method renders "on the charter side of the context assembler, never a
ContextItem" — but `SectionKind::Charter` is ITSELF a `ContextItem` in the same
`<context-item>` block as everything else (`context.rs:183-204`; test corpus
`tests/context.rs:62-65`). The persona method (per-persona, dynamic — unlike the single
static `system_charter`) must be a per-call system message; the §12 spike already did
exactly this ("method as the trusted system prompt"), so the MECHANISM is validated —
only the §4 prose + assertions (a)/(b) must re-anchor to the transport/system-message
layer. (c)/(d) — strict `deny_unknown_fields` findings + method-hash gating — testable
as written.

**C. Code-reality grounding — (1) CONFIRMED · (2) REFUTED · (3) CONFIRMED · (4) CONFIRMED.**
- (1) "extends, doesn't change the Mind/belief interface" — CONFIRMED. `Mind` returns
  `MindOutput{beliefs,proposals,journal,cost_cents}` (`mind.rs:115-168`); adding
  `SectionKind::DomainAnalysis` touches only the enum + `as_str` match (`context.rs:43-64`),
  additive. Track A's cycle calls `assemble_context` then `mind.decide` generically
  (`cycle.rs:325-350`). Watch: §9 inserts the variant "just under OpenBeliefs" — manifest
  serializes the snake_case string (not the int discriminant) and the priority golden test
  asserts by string position (`tests/context.rs:88-94`), so it holds; re-run the
  manifest-determinism test (`tests/context.rs:99-113`) after insertion.
- (2) "extends the review `ScopeKey`" — REFUTED (drops a spec dimension). `ScopeKey` is
  `{model_id, strategy, category}` today (`review.rs:37-41`); `strategy` is spec-mandated
  (5.10) and load-bearing across the GO/NO-GO machinery. Design §10's
  `{model_id, persona_id, persona_version, category}` REPLACES `strategy`. Blast radius
  is contained (`ScopeKey` is constructed only in review.rs) but as written it regresses
  strategy-scoped calibration. → Condition 2.
- (3) "mirrors lessons/calibration_params supersession + append-only repos" — CONFIRMED
  (`CalibrationParamsRepo` repos.rs:1262, `LessonsRepo` 1397, `BeliefsRepo.insert` flips
  prior status in-txn 957-990; the append-only triggers exist migration lines 278,294,98).
- (4) runner "modeled on discovery.rs" (budget-first, degrade-not-crash) — CONFIRMED 1:1
  (`discovery.rs:282-285,296-302,312-320,492-495,506-511`).

**D. Scalar-claims independence — see the DECISION-CRITICAL section above. INDEPENDENT.**

**E. Coordination/ownership — CONFIRMED low-collision, one watch item.**
"One migration, two tables" is realistic (the initial migration defines ~15 tables in one
file). Does not touch `fortuna-sources` (consumes the `signals` table; no fetch). ROTA is
data-only (Track B implements panels). Watch: Track A is mid-flight on T4.2 and owns the
belief-composition path; Track E must edit `context.rs` (`SectionKind`) and `review.rs`
(`ScopeKey`), in Track A's neighborhood. T4.2's named scope is the WS dial (not those
files), so hard conflict is unlikely — but SEQUENCE those edits into a clean-main window
relative to Track A's cycle/belief commits.

## Must-fix before build (the 3 conditions)
1. (B) Re-anchor §4 firewall + assertions (a)/(b) to the transport system-message
   (`AnthropicMindConfig.system_charter` / `"system"` field), not "the charter side of the
   context assembler" (which does not exist). Mechanism is real + spike-validated; only the
   description is wrong.
2. (C2) Fix the `ScopeKey` change to ADD persona fields while KEEPING the spec-mandated
   `strategy` (`review.rs:37-41`, spec 5.10), or introduce a distinct `PersonaScopeKey`.
3. (A.i) Reword §3/§15 so the artifact/`PersonaOutcome` no-order-field guarantee is owned
   by the NEW ADD-only field-surface I6 test (mirroring `i6_propose_only_mind.rs:99-127`),
   not the dependency-direction check (which does not inspect fields).

## Watch during build
- Insert `SectionKind::DomainAnalysis` carefully; re-run the manifest-determinism golden
  test (`tests/context.rs:99-113`).
- Land `context.rs`/`review.rs` edits in a clean window vs Track A's in-flight work.
- Keep persona artifacts strictly as the artifact feeding the binary fan-out (D corollary).

## Where the design is STRONGER than required (honest credit, both directions)
- The §2 "ephemeral vs persisted" argument is a correct, non-obvious I5/§5.7 derivation:
  a persona call is non-deterministic, every belief must replay byte-identically, so the
  reasoning must be persisted EITHER way → the shared content-hashed artifact strictly
  dominates. Faithful to the real `HashMismatch` fail-closed verification.
- The §12 live spike already exercised the exact transport firewall (method-as-system-
  prompt, signals as `<context-item>` data, injection ignored, PWNED=0), de-risking the one
  claim (B) the prose otherwise mislocates. No spike artifact/key was committed (diff is
  docs-only) — clean on the secrets discipline.
- The degrade-not-crash + budget-first + strict-findings runner contract (§8) precisely
  mirrors the proven `discovery.rs` shape.

## Note for the operator (out of band, not a track-E defect)
The design correctly cites spec v0.9 (confirmed real on main: "Version 0.9 (build-ready
draft), June 11, 2026"). CLAUDE.md still says "docs/spec.md (v0.8)" — CLAUDE.md is the
stale one; housekeeping, not a track-E issue.
