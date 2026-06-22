# Area h — Existing-doc inventory and drift map

Audit area: documentation inventory + drift map for FORTUNA at
`/Users/xavierbriggs/fortuna-wt-ws3` (branch `feature/ws3-generic-backtest`).
READ-ONLY pass. Every contradiction is cited `path:line` on both sides.

**Totals:** 379 markdown/doc files in scope (excluding `target/`,
`node_modules/`, `.git/`). Of those: **167** are scraped venue API reference
pages (`docs/research/venue/*/raw/pages/`), **60** are dated gate verdicts
(`docs/reviews/`), **18** are prior-audit artifacts (`docs/audit/2026-06-18/`).
The remaining ~134 are the canonical/operational/design/research/fixture set
inventoried below.

**Workspace ground truth (as-built):** `Cargo.toml` lists **17** crate members;
`ls crates/` shows **17** dirs. This is the single fact that the most docs get
wrong (see Drift #1).

---

## 1. Inventory table (classification)

Classifications: **keep-as-canonical** (authoritative, maintain) ·
**fold-into-canon** (deep topic doc that should be merged into
ARCHITECTURE/STANDARDS/canonical owner) · **archive** (superseded/historical;
move to `docs/archive/`) · **reference** (kept as-is: research, fixtures, MVPs)
· **delete** (no value).

### 1a. Root + top-level canonical ledgers

| Path | Summary | Class |
|---|---|---|
| `CLAUDE.md` | Repository constitution: the 7 invariants (normative), house conventions, definition of done, session rules. Binding. | keep-as-canonical |
| `docs/spec.md` | Design authority v0.9; §1–13 (purpose, principles, invariants, architecture, every component spec). Spec wins on disagreement. | keep-as-canonical |
| `AGENTS.md` | Agent front door: non-negotiables (brief) + "where verified truth lives" pointer table + multi-agent protocol summary. | keep-as-canonical |
| `README.md` | Project overview, 7-invariant summary table, status-as-of-2026-06-14, doc map, layout. | keep-as-canonical |
| `PROMPT.md` | Master build instruction + acceptance checklist. | keep-as-canonical |
| `BUILD_PLAN.md` (108KB) | Phased task list; ticks carry commit hashes + phase EXIT evidence. | keep-as-canonical |
| `GAPS.md` (16KB, pruned 5858→39L per CHANGELOG) | Honesty ledger: deferred/blocked/operator-pending with unblock steps. | keep-as-canonical |
| `ASSUMPTIONS.md` (158KB) | Every decision made where the spec is silent, with rationale. | keep-as-canonical |
| `CHANGELOG.md` (128KB) | Keep-a-Changelog; per-subsystem subsections under `[Unreleased]`; landed changes (backward-looking). | keep-as-canonical |
| `FINAL_REPORT.md` (24KB) | Build completion report dated 2026-06-10 against **spec v0.8**; "Thirteen crates, ~41,500 LOC, 64 commits". | archive (stale; superseded — see Drift #1) |

### 1b. docs/ canonical narrative + operations

| Path | Summary | Class |
|---|---|---|
| `docs/architecture.md` (36KB) | Three planes (cognition/harness/safety), crate map ("Sixteen crates as of f31aaa8"), data flow, invariant enforcement. | keep-as-canonical (needs freshen — Drift #1) |
| `docs/system-e2e-overview.md` (24KB) | End-to-end narrative (signal→trade→settlement), §4 each layer in depth, §5 crate map (16 rows), §6 status. | keep-as-canonical (overlaps architecture.md — Overlap A; needs freshen) |
| `docs/quickstart.md` (12KB) | Zero-to-running Sim daemon + ROTA + test battery; every command executed-as-written. | keep-as-canonical |
| `docs/operations.md` (28KB) | Daily-operator manual: CLI as-built (§1), ROTA tour (§2), operator rhythm (§3). | keep-as-canonical |
| `docs/operator.md` (16KB) | Operator action list: secrets/keys, enable flags, approvals, promotion ladder, infra, what-to-view. Verified vs code 2026-06-13. | keep-as-canonical (overlaps operations.md §3 + runbooks — Overlap B) |
| `docs/verification.md` (20KB) | Verification doctrine: independent gates, DST, mutation checks, §4 war stories, §5 how-to-run. | keep-as-canonical |
| `docs/playbook.md` (8KB, NO `#` headers — bare prose) | Retrospective essay "Ralph fleet + one verifier" orchestration pattern; lessons. Reads like a session write-up, not a maintained doc; duplicates docs/design/orchestration.md + verification.md §3. | archive (or fold-into-canon) |
| `docs/close-the-loop-fixes.md` (16KB) | 2026-06-18 P0–P3 fix list from a live soak audit (settlement unwired, etc.). Point-in-time audit artifact. | archive |

### 1c. docs/design/ (decision docs)

| Path | Summary | Class |
|---|---|---|
| `docs/design/orchestration.md` | Three-track implementer coordination charter; loop rules, gates, quality bar. | keep-as-canonical |
| `docs/design/fortuna-cli.md` | Operator CLI spec v1.1 + amendments + binding operator prefs. | keep-as-canonical |
| `docs/design/rota-dashboard.md` | ROTA dashboard design v2.1 + aesthetic tokens + amendments. | keep-as-canonical |
| `docs/design/documentation-plan.md` | Operator-directed 2026-06-12 doc overhaul: defines THE SET & OWNERS (W1–W4) + style contract + docs gate. **The canonical-owner map.** | reference (drives consolidation) |
| `docs/design/signal-contract.md` | "Design thinking only" — spec 5.11 governs. | reference |
| `docs/design/aeolus-fortuna-source-contract.md`, `aeolus-source-contract.md`, `aeolus-kalshi-bucket-matching.md` | Aeolus forecast wire schema + handoff + Kalshi bucket match. | keep-as-canonical / fold-into-canon |
| `docs/design/domain-analysis-personas-design.md` | Versioned domain-expert persona ("skills") system + artifact model. | keep-as-canonical |
| `docs/design/perp-strategies-and-scalar-claims.md` | Perp strategy runtime seam + native-CRPS scalar scoring + swappable ScoringRule. | keep-as-canonical |
| `docs/design/ingestion-observability-contract.md`, `track-d-ingestion-subsystem.md` | Ingestion telemetry/ROTA contract + living subsystem index. | keep-as-canonical |
| `docs/design/track-a-kalshi-paper-clearance.md` | Kalshi adapter paper-clearance record/gate (not yet operator-signed). | keep-as-canonical |
| `docs/design/kalshi-demo-flip.md` | Kalshi demo venue adapter design; Phase1+2 code complete. | fold-into-canon (→ runbooks/demo-flip) |
| `docs/design/implementer-loop.md`, `implementer-loop-track-{a,b,c,d,e}.md` | Per-track mission queues; prior missions complete. | archive |
| `docs/design/track-a-completion-queue.md`, `track-e-persona-brief.md`, `track-m-model-providers-brief.md`, `PROMPT-domain-analysis-skills.md`, `PROMPT-track-e-grader-bridge.md`, `kinetics-perps-module-plan.md` | Historical track queues / session briefs / parked / B-phase plan (complete). | archive |
| `docs/design/rota-observability.md`, `persona-live-wiring-handoff.md`, `synthesis-edge-source-decision.md`, `track-e-changelog.md`, `track-e-aeolus-changelog.md` | Living status / handoff / decision-record / per-track changelogs. | reference |

### 1d. docs/runbooks/ (operational procedures)

All twelve are operator procedures, current → **keep-as-canonical**:
`backup-restore.md`, `demo-bringup.md`, `demo-flip.md`, `fixture-recording.md`,
`halt-and-rearm.md`, `ingestion-ops.md`, `key-rotation-and-secrets.md`,
`kill-switch-drill.md`, `persona-authoring.md`, `rota-local-bringup.md`,
`soak-start.md`, `troubleshooting.md`.

### 1e. docs/reviews/ (60 files — gate verdicts)

| Path / group | Summary | Class |
|---|---|---|
| `docs/reviews/GATE-FINDINGS-LATEST.md` | LIVE findings bus / operator queue; captain-owned; re-read at priority. Referenced as canonical by README, AGENTS, operator.md. | keep-as-canonical |
| `docs/reviews/operator-decisions-2026-06-12.md` | Operator decisions (protected-crate waive, F1, rearm semantics, leverage cap). | archive (historical) |
| `docs/reviews/*-gate-*.md` + `*-INDEPENDENT-*.md` (the other ~58: `phase-{1,2,3}-gate`, `system-0-*`, `track-{b,c,d}-*`, `t41-*`, `t42-*`, `perps-*`, `r5-*`, `soak-go-gate`, `completion-audit`, `f-batch-*`, etc.) | Dated independent gate verdicts per task/track/phase. | archive |

### 1f. docs/audit/2026-06-18/ (18 files — prior deep audit)

| Path | Summary | Class |
|---|---|---|
| `docs/audit/2026-06-18/doc-triage.md` | **PRIOR DOC INVENTORY**: 220 md files classified into authoritative/stale/archive; names 28 canonical docs. Direct predecessor of this area-h task — cross-check. | reference |
| `docs/audit/2026-06-18/AUDIT.md` | Deep-audit atlas/executive summary; "computed but never persisted" root cause; safe-engine verdict. | reference (or archive) |
| `docs/audit/2026-06-18/{AUDITOR-BRIEF,README,PROGRESS,branch-ledger,MVP-CLOSURE-PLAN,PHASE-B-SUMMARY,verification}.md` + `area-1..9-*.md` | Audit dispatch brief, nav, progress ledger, branch classification, closure plan, phase-B summary, independent verification, 9 area findings. | archive |

### 1g. docs/research/ (dated sourced research) — all **reference**

- `docs/research/2026-06-18-baseball-modeling.md`, `2026-06-18-tennis-modeling.md`, `2026-06-18-kalshi-tennis.md`, `2026-06-18-perpetual-futures-modeling.md`, `2026-06-18-scoring-learning-loop-edge-decay-watchdog-brief.md`, `2026-06-19-kairos-ec2-deployment.md`, `2026-06-20-ws2-scoring-grounding.md`, `2026-06-21-ws3-backtest-overfitting-grounding.md` — decision-grade modeling/ops research memos.
- `docs/research/anthropic/models-2026-06.md` — Claude model tiers/IDs/pricing (Opus 4.8 / Sonnet 4 / Haiku 4.5).
- `docs/research/ops/slack-api-2026-06-09/research.md`, `ops/otel-rust-2026-06-10/research.md` — ops API research.
- `docs/research/sources/{aeolus,calendar_bls,nws,nws_climate,rss_fed_press,rss_sec_edgar}/dossier.md`, `TEMPLATE.md`, `kalshi-temperature-stations.md` — source-vetting dossiers + template + station map.
- `docs/research/venue/kalshi-api-2026-06-10/{research.md}` + `raw/pages/*.md` (~92) — Kalshi API research + scraped pages.
- `docs/research/venue/kinetics-perps-2026-06-10/{research.md,SOURCES.md}` + `raw/pages/*.md` (~58) — Kinetics perps research + scraped pages.
- `docs/research/venue/polymarket-us-2026-06-10/{research.md}` + `raw/pages/*.md` (~18) + `raw/web-sources.md` — Polymarket US research + scraped pages.
- `docs/research/venue/{kalshi-fees-2026-06-09,polymarket-fees-2026-06-09}/research.md` — fee-schedule research (the fee source of truth — see Drift note #6).
- `docs/research/2026-06-20-ws2-scoring/PLAN.md`, `research-workspace/PLAN.md` — deep-research plans.

### 1h. Python MVP research tracks — all **reference**

`docs/kairos/{README,SPEC,SOURCES,deploy/README}.md` (perp funding/basis RV
harness), `docs/deuce/README.md` (tennis win-prob MVP), `docs/heater/README.md`
(pitcher-strikeout MVP).

### 1i. docs/superpowers/ (charters + plans + specs)

| Path | Summary | Class |
|---|---|---|
| `docs/superpowers/loop-close-captain-charter.md` | Operational captain-loop charter; re-read every iteration. | keep-as-canonical |
| `docs/superpowers/loop-close-gaps.md`, `loop-close-operator.md` | Current-milestone forward items + captain escalation/decision log. | reference |
| `docs/superpowers/specs/2026-06-19-scoring-and-validation-architecture.md`, `2026-06-19-loop-close-and-provable-demo-design.md`, `specs/2026-06-12-news-aggregation-design.md` | Loop-close north-star + provable-demo design + news-agg design (cite spec authority). | keep-as-canonical |
| `docs/superpowers/specs/2026-06-{18-ground-truth-audit…,18-operator-ui-overhaul…,18-phase-c-close-the-loop…,20-ws2-proof-layer…,21-ws3-generic-backtest…}.md` | Dated design specs (mostly executed). | archive |
| `docs/superpowers/plans/2026-06-*.md` (7) | Dated implementation plans (paper-on-live, ui-overhaul, phase-a/c, ws1/ws2/ws3). | archive |

### 1j. docs/archive/ (already archived) — **archive** (leave)

`docs/archive/README.md`, `docs/archive/gaps-history.md` (435KB, pre-prune full
GAPS), `docs/archive/amendments/AMENDMENT-track-{A-obs2,C-funding-capture,C-funding-poller,C-slice-3b-v2}.md`.

### 1k. Crate-level + fixture READMEs + config personas

| Path | Summary | Class |
|---|---|---|
| `crates/fortuna-core/dst-corpus/README.md` | DST regression corpus: one file/seed, failure story, never-delete rule. | keep-as-canonical |
| `crates/fortuna-invariants/tests/README.md` | Protected invariant tests: additions-only, never weakened. | keep-as-canonical |
| `crates/fortuna-venues/tests/kalshi_doc_samples/README.md` | Doc-derived (NOT operator-recorded) Kalshi samples; Sim-cleared only. | reference |
| `fixtures/kalshi/README.md` | Operator-recorded Kalshi demo fixtures (2026-06-11) + 16 wire findings. | keep-as-canonical |
| `fixtures/kinetics-perps/SESSION-NOTES.md` | Kinetics fixtures (degraded; margin not enabled on demo acct). | reference |
| `fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.meta.md` | Paired BTC perp vs KXBTC live capture (2026-06-13). | reference |
| `fixtures/sources/{calendar,nws,rss}/README.md` | Real public-domain source fixtures + re-record commands. | reference |
| `config/personas/{macro-economist,meteorologist}/persona.md` | Persona system prompts (operator-authored, versioned). | reference (active config) |

### 1l. `.claude/` (tracked tooling docs) + `.github/` + untracked

| Path | Summary | Class |
|---|---|---|
| `.claude/agents/verifier.md` | Adversarial verifier agent definition. | reference |
| `.claude/skills/{fortuna,fortuna-review,deep-codebase-audit}/SKILL.md` + resources/templates | Project skills (crate map, review checklist, audit profile/templates). | reference |
| `.claude/ralph-loop.local.md` | Ralph-loop local state. | reference |
| `.github/workflows/{ci,invariants-dst}.yml` | CI (not md, noted for completeness). | reference |
| `.superpowers/sdd/progress.md` (untracked) | SDD loop progress scratch. | reference (untracked) |

---

## 2. Contradictions / drift

### Drift #1 (HIGH) — Crate count: FOUR different numbers, none matches the 17-crate workspace
The single worst drift. Every crate-map doc is stale; one new crate
(`fortuna-scoring`) is invisible in all narrative docs.

- **As-built ground truth: 17 crates.** `Cargo.toml:3-21` lists 17 members;
  `ls crates/` = 17 dirs. (Sibling evidence `scratch/evidence/a-workspace.md`
  independently states "The 8 spec crates all exist; the extra 9 are …
  (scoring) …" = 17.)
- **spec.md §5.1 says 8.** `docs/spec.md:90-105` crate-layout block lists only
  `fortuna-core, -gates, -exec, -state, -venues, -ledger, -cognition, -ops`
  (8). (As-intended at v0.9 authoring; spec is allowed to be the minimal
  layout, but it is cited by README/AGENTS as the crate authority.)
- **FINAL_REPORT.md says 13.** `FINAL_REPORT.md:18` "Thirteen crates,
  ~41,500 lines of Rust, 64 commits" (built against spec v0.8, dated
  2026-06-10).
- **README.md says 16.** `README.md:165` "Sixteen-crate Rust workspace under
  `crates/`" and lists 16 names — omits `fortuna-scoring`.
- **architecture.md says 16.** `docs/architecture.md:169` "Sixteen crates as
  of `f31aaa8`"; its bold crate-map headers (`architecture.md:174-344`) are 16
  and **never mention `fortuna-scoring`** (grep: 0 hits).
- **system-e2e-overview.md §5 says 16.** `docs/system-e2e-overview.md:306-323`
  crate-map table has 16 rows; `fortuna-scoring` absent (grep: 0 hits).

`fortuna-scoring` is a real crate added AFTER the 16-crate docs were written:
`git log` shows it introduced by commit `3a0c160` ("refactor(scoring): extract
pure fortuna-scoring crate via shim + true CRPS"). Its own
`crates/fortuna-scoring/src/lib.rs:1` declares "fortuna-scoring: the pure,
decoupled scoring library (spec 5.5, 5.15)". So the as-built count moved 16→17
and no narrative doc followed. The architecture.md "as of f31aaa8" hedge is
honest but now stale (the commit exists; the count changed later).

### Drift #2 (HIGH) — system-e2e cites a source file that no longer exists
`docs/system-e2e-overview.md:151` heading "4.5 Beliefs, calibration, scoring
(`fortuna-cognition/beliefs.rs`, `calibration.rs`, `scoring.rs`)" cites
`fortuna-cognition/src/scoring.rs`. That file is **gone** — `ls
crates/fortuna-cognition/src/scoring.rs` → MISSING (only `calibration.rs`
remains). Scoring math was extracted into the standalone `fortuna-scoring`
crate (Drift #1). Dead file reference in a canonical doc.

### Drift #3 (MEDIUM) — `fortuna-scoring` invisible across the entire doc set
Grep for `fortuna-scoring` returns **0 hits** in README.md, architecture.md,
operations.md, operator.md, quickstart.md, system-e2e-overview.md,
verification.md. A whole workspace crate (with spec-5.5/5.15 attribution in its
lib.rs) has no documentation footprint. Consequence of Drift #1/#2; called out
separately because it is the consolidation action item (one canonical crate
map must add it).

### Drift #4 (LOW/MEDIUM) — I4 independence list differs spec vs CLAUDE.md/README
- `docs/spec.md:43` (I4): "must not depend on the cognition runtime, the event
  loop, **or any LLM provider** being healthy." — does NOT list Postgres.
- `CLAUDE.md:18-19` (I4): "Must not depend on the cognition runtime, the event
  loop, **Postgres**, or any LLM provider being healthy." — ADDS Postgres.
- `README.md:31` (I4): "must not depend on the cognition runtime, the event
  loop, **Postgres**, or any LLM provider being healthy." — ADDS Postgres.

Substantively reconcilable (spec §2 Principle 9 + §5.1 already mandate the
kill-switch be Postgres-free, so CLAUDE/README are correct expansions, not
conflicts), but the literal I4 normative text differs between the spec and the
constitution. The spec's I4 sentence should be brought in line so the three
copies of the invariant read identically.

### Drift #5 (LOW) — FINAL_REPORT.md states it is against spec v0.8 while repo spec is v0.9
`FINAL_REPORT.md:3` "Build completed 2026-06-10 against docs/spec.md **v0.8**"
vs `docs/spec.md:5` "Version **0.9**". FINAL_REPORT is a point-in-time artifact
(it even predates the perps/Aeolus/scoring crates), so its "Thirteen crates"
and v0.8 framing are simply stale; README.md:151 still links it as a live "what
was built" doc. Recommend archive + a one-line "superseded; see CHANGELOG"
banner, or stop linking it as current.

### Drift #6 (INFO — not a contradiction) — fee formula is CONSISTENT
Checked because the brief flagged it. Kalshi taker = `0.07·C·p·(1−p)` rounded
up appears identically in `docs/spec.md:123` and
`docs/research/venue/kalshi-fees-2026-06-09/research.md:49,57` (maker 0.0175 on
maker-fee series). Polymarket US quadratic taker 0.05 / maker −0.0125
(`spec.md:123`) matches the fees research. No fee drift found. The spec §5.2's
generic phrase "maker discounts" vs the research's precise "0.0175 only on
maker-fee series" is a level-of-detail gap, not a contradiction.

### Drift #7 (INFO) — gate count consistent at 10
`docs/spec.md:127-139` lists 10 ordered gate checks; `system-e2e-overview.md:309`
and FINAL_REPORT both say "10-check pipeline". No drift. (Recorded so a future
reader doesn't re-flag it.)

### Drift #8 (LOW) — README status date stamp lags repo state
`README.md:36` "Status — as of 2026-06-14" but CHANGELOG `[Unreleased]` carries
Loop-Close WS1 (2026-06-20) and WS2/WS3 work (the current branch is
`feature/ws3-generic-backtest`). README's status section predates the
loop-close workstreams (WS1 live-spine, WS2 proof-layer/`fortuna-scoring`, WS3
backtest) entirely — it never mentions them. As-intended-at-2026-06-14 vs
as-built-now drift; the WS1–WS3 work (settlement wiring, per-producer scoring,
go/no-go calibration gate, the new scoring crate) is undocumented in any
top-level narrative doc.

### Drift #9 (LOW) — `docs/playbook.md` is an orphan with no headers
`docs/playbook.md` (51 lines) has zero `#` headers and reads as a raw session
retrospective ("The pattern: a Ralph fleet + one independent verifier"). It is
not in the README doc map (`README.md:143-161`) and duplicates
`docs/design/orchestration.md` + `docs/verification.md §3`. Either fold its
unique lessons into one of those or archive it; as-is it is an unlinked,
unmaintained doc.

---

## 3. Overlap clusters (multiple owners for one topic — consolidation candidates)

**Overlap A — "what the system is + crate map + data flow" has 3 owners.**
`docs/architecture.md` (three planes + crate map + data flow), `docs/system-e2e-overview.md`
(§4 layer-by-layer + §5 crate map + §6 status), and `docs/spec.md §4/§5.1`
(architecture overview + crate layout) all carry a crate map and an
architecture narrative. All three crate maps disagree with the workspace
(Drift #1). README.md:154-155 even lists architecture.md and
system-e2e-overview.md as distinct docs. **Action:** one canonical crate map
(suggest architecture.md §3 as owner per `documentation-plan.md` W2), and
system-e2e §5 + spec §5.1 + README §Layout should point to it, not restate it.

**Overlap B — operator surface has 3 owners.**
`docs/operations.md §3` (operator rhythm + CLI), `docs/operator.md` (action
list: secrets/flags/approvals/promotion ladder), and `docs/runbooks/*` (the
procedures) + spec §11 / §8 (the promotion ladder + ops). The promotion ladder
appears in spec §11, operator.md §4, and is referenced from README/quickstart.
**Action:** operator.md = the action checklist (keep), operations.md = the
manual (keep), runbooks = procedures (keep), but the ladder thresholds should
have ONE normative home (spec §11) that the others link.

**Overlap C — orchestration / "how we build" has 3+ owners.**
`docs/design/orchestration.md` (charter), `docs/playbook.md` (retrospective),
`docs/verification.md §3` (multi-agent setup), `docs/superpowers/loop-close-captain-charter.md`
(captain-loop charter), and `docs/design/implementer-loop*.md` (per-track queues).
**Action:** orchestration.md + loop-close-captain-charter.md are the live
owners; playbook.md and the implementer-loop track queues archive.

**Overlap D — invariants stated in 4 places.**
`docs/spec.md §3` (normative), `CLAUDE.md §"seven invariants"` (constitution,
authoritative per its own text), `README.md` table, `system-e2e-overview.md §2`,
`AGENTS.md` pointer. This is intentional (constitution restates spec; README/e2e
summarize) and the docs say CLAUDE.md is authoritative — but the I4 wording
already drifted (Drift #4), which is the risk of N copies. **Action:** keep
CLAUDE.md authoritative; reconcile spec §3 I4 to match; summaries should quote,
not paraphrase.

**Overlap E — Kalshi demo-flip has design + 2 runbooks + scattered references.**
`docs/design/kalshi-demo-flip.md` (design), `docs/runbooks/demo-flip.md` (flip
mechanics), `docs/runbooks/demo-bringup.md` (umbrella). README/quickstart/operator
all reference the pair. The design doc is now implemented; **fold** its
still-true content into the runbooks and archive the design doc.

**Overlap F — doc inventory itself has a prior owner.**
`docs/audit/2026-06-18/doc-triage.md` already classified 220 md files and named
28 canonical docs. This area-h pass supersedes it (the count is now 379 and a
crate was added since). Cross-checked: doc-triage's canonical-28 set matches my
keep-as-canonical set for the top-level + narrative docs.

---

## 4. Open questions (uncertain — not asserted)

- Is `FINAL_REPORT.md` intended to stay a frozen historical artifact (then it
  should be banner-marked + de-linked as "current") or to be refreshed to the
  17-crate / spec-v0.9 / loop-close state? README.md:151 currently links it as
  live truth. (Recommend the former.)
- `docs/spec.md §5.1`'s 8-crate layout: is it deliberately the minimal
  "spec-mandated" set (and the other 9 are implementation factoring not owed a
  spec entry), or should the spec enumerate all 17? The spec is the cited crate
  authority in README/AGENTS, so the answer changes whether Drift #1 is a spec
  bug or a README/architecture bug. I did not assume; flagging for operator.
- Whether `docs/playbook.md` carries any lesson NOT already in
  orchestration.md/verification.md before it is archived (I did not diff line
  by line; it reads as a strict subset).
- `.hephaestus/` and `.superpowers/` appeared as untracked dirs in git status;
  only `.superpowers/sdd/progress.md` is an md file there. Not yet part of the
  committed doc set; noted, not classified.

---

## 5. Classification tally

| Class | Count (approx) | Notes |
|---|---|---|
| keep-as-canonical | ~40 | top ledgers, narrative docs, all 12 runbooks, GATE-FINDINGS-LATEST, several design specs, captain charter, test/fixture READMEs |
| fold-into-canon | ~3 | kalshi-demo-flip design, aeolus contracts (partial), playbook (alt) |
| archive | ~95 | 60 reviews (minus GATE-FINDINGS-LATEST) + 18 audit (minus doc-triage) + ~20 design track-queues/plans/specs + FINAL_REPORT + close-the-loop-fixes + playbook |
| reference | ~240 | 167 venue raw pages + ~25 research/dossiers/MVPs + fixtures + personas + .claude tooling + doc-triage + research plans |
| delete | 0 | nothing recommended for deletion (all has provenance value; archive over delete) |

Total ≈ 379 files. (Counts are bucketed estimates; the venue raw-pages bucket
dominates "reference" at 167.)
