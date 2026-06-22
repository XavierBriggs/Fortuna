# Doc Triage — 2026-06-18

Audited branch: `feature/paper-on-live-data` (working tree).
Scope: every `*.md` under repo root and `docs/**` (excluding `target/` and `.git/`).
Total files inventoried: ~220 markdown files (~88,924 lines across all dirs).

---

## Summary

| Class | Count | Notes |
|---|---|---|
| **authoritative** | 28 | Spec, ledgers, runbooks, current audit area reports |
| **stale** | 19 | Superseded design docs, deprecated contracts, old plans |
| **archive** | 173 | All reviews (55), all research raw pages (185), all AMENDMENT-*.md (4), track changelogs, kickoff, fixture notes |

**Total distinct markdown docs (non-.claude/.agents/target):** ~220

**Recommended single-source-of-truth set (the 28 authoritative docs):** `docs/spec.md`, `CLAUDE.md`, `GAPS.md`, `ASSUMPTIONS.md`, `CHANGELOG.md`, `BUILD_PLAN.md`, `PROMPT.md`, `AGENTS.md`, `README.md`, `FINAL_REPORT.md`, `docs/architecture.md`, `docs/system-e2e-overview.md`, `docs/operations.md`, `docs/operator.md`, `docs/verification.md`, `docs/quickstart.md`, `docs/playbook.md`, all 12 `docs/runbooks/*.md`, `docs/audit/2026-06-18/` (11 files), `docs/superpowers/specs/2026-06-18-ground-truth-audit-and-demo-readiness-design.md`, `docs/superpowers/plans/2026-06-18-phase-a-ground-truth-audit.md`, `docs/reviews/GATE-FINDINGS-LATEST.md`, `docs/close-the-loop-fixes.md`.

---

## Triage

### Root-level docs

| Doc | Lines | Class | Reason / superseded-by |
|---|---|---|---|
| `docs/spec.md` | 410 | **authoritative** | Normative design v0.9; CLAUDE.md declares it wins all conflicts |
| `CLAUDE.md` | 78 | **authoritative** | Session and build constitution; references spec |
| `AGENTS.md` | 81 | **authoritative** | Agent-instruction mirror of CLAUDE.md |
| `GAPS.md` | 5858 | **authoritative** | Live honesty ledger; operator items + gate record (though grossly oversized — see Phase-B actions) |
| `ASSUMPTIONS.md` | 2335 | **authoritative** | Every spec-silence decision; enormous but still live |
| `CHANGELOG.md` | 1645 | **authoritative** | Keep-a-Changelog style; newest-first, verifier-reconciled |
| `BUILD_PLAN.md` | 1421 | **authoritative** | Phase task list with commit hashes; still being ticked |
| `PROMPT.md` | 119 | **authoritative** | Master build instructions entrypoint |
| `README.md` | 171 | **authoritative** | Repo entry point |
| `FINAL_REPORT.md` | 360 | **authoritative** | Post-acceptance state narrative (2026-06-10 v0.8 build); accurately describes the 13-crate system and gate record — still truthful for that baseline |
| `AMENDMENT-track-A-obs2.md` | ~100 | **archive** | Track-A build amendment; content superseded by what landed in GATE-FINDINGS-LATEST + CHANGELOG |
| `AMENDMENT-track-C-funding-capture.md` | ~80 | **archive** | Track-C build amendment; merged/superseded |
| `AMENDMENT-track-C-funding-poller.md` | ~80 | **archive** | Track-C build amendment; merged/superseded |
| `AMENDMENT-track-C-slice-3b-v2.md` | ~80 | **archive** | Track-C build amendment; merged/superseded |

### `docs/` top-level

| Doc | Lines | Class | Reason / superseded-by |
|---|---|---|---|
| `docs/architecture.md` | 560 | **authoritative** | Current component reference; self-declared accurate as of 2026-06-14 (main). Companion to system-e2e-overview.md |
| `docs/system-e2e-overview.md` | 333 | **authoritative** | End-to-end narrative; pairs with architecture.md |
| `docs/operations.md` | 564 | **authoritative** | Operational procedures |
| `docs/operator.md` | ~180 | **authoritative** | Operator-facing reference |
| `docs/verification.md` | 346 | **authoritative** | Verification procedures |
| `docs/quickstart.md` | ~120 | **authoritative** | Operator/dev quickstart |
| `docs/playbook.md` | ~60 | **authoritative** | Operational playbook |
| `docs/close-the-loop-fixes.md` | ~220 | **authoritative** | 2026-06-18 live audit of running paper-on-live soak; F1–Fn fix list with P0/P1/P2/P3; directly actionable for current branch |

### `docs/audit/2026-06-18/` (all 11 files)

| Doc | Lines | Class | Reason |
|---|---|---|---|
| `AUDITOR-BRIEF.md` | 34 | **authoritative** | Ground-truth audit dispatch brief |
| `PROGRESS.md` | 24 | **authoritative** | Task-level progress ledger for this audit session |
| `area-1-spine.md` | 127 | **authoritative** | Area 1 findings (P0×2, P1×2, P2×1) |
| `area-2-duplication.md` | 130 | **authoritative** | Area 2 findings (P2×4, P3×4) |
| `area-3-safety.md` | 318 | **authoritative** | Area 3 safety/invariants (34/34 pass; P2×2, P3×1) |
| `area-4-legibility.md` | 100 | **authoritative** | Area 4 module/legibility findings |
| `area-5-vendor-coupling.md` | 81 | **authoritative** | Area 5 venue coupling boundary |
| `area-6-tests-replay.md` | 96 | **authoritative** | Area 6 test/replay posture |
| `area-7-ops-cli.md` | 174 | **authoritative** | Area 7 ops/demo CLI (P1×3, P2×5) |
| `area-8-cognition.md` | 134 | **authoritative** | Area 8 cognition/beliefs (P1×3) |
| `area-9-discovery-seeding.md` | 88 | **authoritative** | Area 9 discovery/signal pipeline |

### `docs/superpowers/`

| Doc | Lines | Class | Reason |
|---|---|---|---|
| `specs/2026-06-18-ground-truth-audit-and-demo-readiness-design.md` | 114 | **authoritative** | Current initiative design; approved brainstorm driving this audit |
| `plans/2026-06-18-phase-a-ground-truth-audit.md` | 159 | **authoritative** | Implementation plan for Phase A; currently executing |
| `specs/2026-06-12-news-aggregation-design.md` | 377 | **authoritative** | News/signal aggregation four-layer trust design; referenced by source contracts |
| `plans/2026-06-16-paper-on-live-data.md` | 239 | **stale** | Superseded by the 2026-06-18 ground-truth audit design and the current branch; Phase 1–2 of paper-on-live are implemented (see `feature/paper-on-live-data` commits) |

### `docs/runbooks/` (all 12 — authoritative)

| Doc | Lines | Class |
|---|---|---|
| `backup-restore.md` | 141 | **authoritative** |
| `demo-bringup.md` | 166 | **authoritative** |
| `demo-flip.md` | 133 | **authoritative** |
| `fixture-recording.md` | 154 | **authoritative** |
| `halt-and-rearm.md` | 166 | **authoritative** |
| `ingestion-ops.md` | 191 | **authoritative** |
| `key-rotation-and-secrets.md` | 163 | **authoritative** |
| `kill-switch-drill.md` | 197 | **authoritative** |
| `persona-authoring.md` | 240 | **authoritative** |
| `rota-local-bringup.md` | 60 | **authoritative** |
| `soak-start.md` | 193 | **authoritative** |
| `troubleshooting.md` | 195 | **authoritative** |

### `docs/reviews/` (55 files)

| Doc | Lines | Class | Reason |
|---|---|---|---|
| `GATE-FINDINGS-LATEST.md` | 1373 | **authoritative** | The single live coordination surface; verifier-owned; state as of 2026-06-15 PM (main@67e88cc). The one review to keep hot. |
| All other 54 reviews (`system-0-1-2-gate-*.md`, `phase-1-gate-*.md`, `t41-*.md`, `track-*-gate-*.md`, etc.) | 3–27k lines each | **archive** | Point-in-time gate verdicts; closed history; GATE-FINDINGS-LATEST supersedes for current state. Retain for audit trail but remove from active reading set. |

### `docs/design/` (31 files)

| Doc | Lines | Class | Reason / superseded-by |
|---|---|---|---|
| `aeolus-fortuna-source-contract.md` | 594 | **authoritative** | Rev 3 (2026-06-13) operator-approved wire schema; active contract for Aeolus→FORTUNA `aeolus.forecast/v2` |
| `aeolus-source-contract.md` | 282 | **stale** | Earlier v2 draft; superseded by `aeolus-fortuna-source-contract.md` rev 3 (which explicitly supersedes it) |
| `aeolus-kalshi-bucket-matching.md` | 117 | **authoritative** | Kalshi temperature-bracket bucket matching; referenced by aeolus source contract |
| `domain-analysis-personas-design.md` | 592 | **authoritative** | Six-slice persona design; §18 is the plan; Track E's authoritative design |
| `perp-strategies-and-scalar-claims.md` | 629 | **authoritative** | Perp strategies and scalar claims design; referenced by track-C |
| `rota-dashboard.md` | 742 | **authoritative** | ROTA dashboard spec; large but current (2026-06-15 timestamp) |
| `rota-observability.md` | 111 | **authoritative** | ROTA observability design; pairs with ingestion-observability-contract |
| `ingestion-observability-contract.md` | 220 | **authoritative** | Ingestion observability contract ("one writer, many readers") |
| `signal-contract.md` | 145 | **authoritative** | Signal contract for untrusted signal ingest |
| `fortuna-cli.md` | 405 | **authoritative** | FORTUNA CLI design (2026-06-14); current |
| `kinetics-perps-module-plan.md` | 211 | **authoritative** | Kinetics perps module plan; perp-v2 partially built |
| `orchestration.md` | 136 | **authoritative** | Orchestration design |
| `synthesis-edge-source-decision.md` | 49 | **authoritative** | Adjudicated design decision (2026-06-12); short; informative |
| `kalshi-demo-flip.md` | 167 | **stale** | Track-C design doc; Phase 1+2 are CODE DONE and merged; the doc is a design artifact that preceded and is now captured in GATE-FINDINGS-LATEST + CHANGELOG |
| `track-e-changelog.md` | 445 | **archive** | Track-E persona changelog; historical build record now all merged. Kept as trace but not active. |
| `track-e-aeolus-changelog.md` | 310 | **archive** | Track-E Aeolus pipeline changelog; historical build record, all merged. |
| `track-a-completion-queue.md` | 179 | **archive** | Track-A completion queue; tracks are IDLE/merged per GATE-FINDINGS-LATEST |
| `track-a-kalshi-paper-clearance.md` | 113 | **archive** | Track-A Kalshi paper clearance; merged/completed |
| `track-d-ingestion-subsystem.md` | 190 | **archive** | Track-D ingestion design; D9+D10 merged; GATE-FINDINGS-LATEST + CHANGELOG supersede |
| `implementer-loop.md` | 88 | **stale** | Generic overnight loop instructions for multi-track phase; tracks are now IDLE; `docs/superpowers/plans/2026-06-18-phase-a-ground-truth-audit.md` replaces as the active north star |
| `implementer-loop-track-a.md` | 67 | **archive** | Track-A-specific loop instructions; track IDLE |
| `implementer-loop-track-b.md` | 124 | **archive** | Track-B-specific loop instructions; track IDLE |
| `implementer-loop-track-c.md` | 41 | **archive** | Track-C-specific loop instructions; track IDLE |
| `implementer-loop-track-d.md` | 104 | **archive** | Track-D-specific loop instructions; track IDLE |
| `implementer-loop-track-e.md` | 115 | **archive** | Track-E-specific loop instructions; track IDLE |
| `persona-live-wiring-handoff.md` | 271 | **stale** | Track-E handoff doc (2026-06-14); superseded by merged state; GATE-FINDINGS-LATEST records the merged outcome |
| `PROMPT-domain-analysis-skills.md` | 169 | **archive** | One-off prompt doc for domain-analysis skills subagent batch |
| `PROMPT-track-e-grader-bridge.md` | 104 | **archive** | One-off prompt for grader-bridge; track IDLE |
| `track-e-persona-brief.md` | 112 | **archive** | Track-E persona brief; track IDLE |
| `track-m-model-providers-brief.md` | 57 | **archive** | Track-M model providers brief; track not active |
| `documentation-plan.md` | 38 | **stale** | 2026-06-12 doc overhaul plan; the overhaul completed (architecture.md, runbooks, etc. freshened 2026-06-15); plan itself is spent |

### `docs/research/` (185 files)

| Doc | Lines | Class | Reason |
|---|---|---|---|
| `venue/kalshi-api-2026-06-10/research.md` | 950 | **authoritative** | Kalshi API research synthesis (2026-06-10) |
| `venue/polymarket-us-2026-06-10/research.md` | 836 | **authoritative** | Polymarket API research synthesis |
| `venue/kinetics-perps-2026-06-10/research.md` | 844 | **authoritative** | Kinetics Perps API research synthesis |
| `venue/kalshi-fees-2026-06-09/research.md` | 351 | **authoritative** | Kalshi fee research; fee facts verified against PDF (per BUILD_PLAN T0.3) |
| `venue/polymarket-fees-2026-06-09/research.md` | 355 | **authoritative** | Polymarket fee research |
| `ops/slack-api-2026-06-09/research.md` | 429 | **authoritative** | Slack API research |
| All `raw/pages/*.md` (179 files across Kalshi, Polymarket, Kinetics) | 3–2811 lines | **archive** | Raw scraped API documentation pages; source material for the research.md syntheses; not read directly in active sessions |

### `docs/kickoff/`

| Doc | Lines | Class | Reason |
|---|---|---|---|
| `T4.1-kickoff.md` | ~40 | **archive** | Phase T4.1 kickoff note; historical |

### `docs/mockups/` (HTML/PNG files — not markdown, but noted for completeness)

No markdown files in `docs/mockups/`. The 3 HTML mockups + PNGs are design artifacts; classified separately as **archive** (design exploration, pre-implementation).

### `docs/diagrams/`

No markdown files. `.dot`/`.png`/`.svg` architecture diagrams — **authoritative** as visual companions to `docs/architecture.md`.

### `config/personas/` (2 files)

| Doc | Lines | Class |
|---|---|---|
| `config/personas/macro-economist/persona.md` | ~30 | **authoritative** — active persona config |
| `config/personas/meteorologist/persona.md` | ~30 | **authoritative** — active persona config |

### `crates/` markdown (5 files — all authoritative)

| Doc | Class | Reason |
|---|---|---|
| `crates/fortuna-core/dst-corpus/README.md` | **authoritative** | DST corpus instructions |
| `crates/fortuna-invariants/tests/README.md` | **authoritative** | Protected invariant test instructions |
| `crates/fortuna-venues/tests/kalshi_doc_samples/README.md` | **authoritative** | Fixture sample instructions |
| `fixtures/kalshi/README.md` | **authoritative** | Kalshi fixture recording notes |
| `fixtures/sources/{calendar,nws,rss}/README.md` | **authoritative** | Source fixture notes |

### `fixtures/kinetics-perps/SESSION-NOTES.md`

| Doc | Class | Reason |
|---|---|---|
| `fixtures/kinetics-perps/SESSION-NOTES.md` | **archive** | Session notes for Kinetics fixture recording session; informational only |

### `fixtures/perp-basis/paired_cycle_btc_perp_vs_kxbtc.meta.md`

| Doc | Class | Reason |
|---|---|---|
| `fixtures/perp-basis/*.meta.md` | **authoritative** | Fixture provenance record; required to replay perp basis test |

---

## Recommended Source-of-Truth Map

| Topic | Single authoritative doc |
|---|---|
| System design / invariants | `docs/spec.md` (v0.9) |
| Session/build constitution | `CLAUDE.md` |
| Architecture (component reference) | `docs/architecture.md` |
| End-to-end narrative | `docs/system-e2e-overview.md` |
| Current gate state / track status | `docs/reviews/GATE-FINDINGS-LATEST.md` |
| Current audit findings | `docs/audit/2026-06-18/area-*.md` + `PROGRESS.md` |
| Demo-paper-ready design | `docs/superpowers/specs/2026-06-18-ground-truth-audit-and-demo-readiness-design.md` |
| Close-the-loop fix list | `docs/close-the-loop-fixes.md` |
| Open items / operator blocks | `GAPS.md` |
| Spec-silence decisions | `ASSUMPTIONS.md` |
| Changelog (code) | `CHANGELOG.md` |
| Task backlog | `BUILD_PLAN.md` |
| Kalshi venue API | `docs/research/venue/kalshi-api-2026-06-10/research.md` |
| Aeolus wire contract | `docs/design/aeolus-fortuna-source-contract.md` (rev 3) |
| Persona design | `docs/design/domain-analysis-personas-design.md` |
| ROTA dashboard | `docs/design/rota-dashboard.md` |
| Operator runbooks | `docs/runbooks/` (12 files; each is its own topic) |
| News/signal trust | `docs/superpowers/specs/2026-06-12-news-aggregation-design.md` |

---

## Phase-B Doc Actions

### Action 1 — Archive `docs/reviews/` (minus GATE-FINDINGS-LATEST)

Move all 54 point-in-time gate verdict files from `docs/reviews/` into `docs/archive/reviews/` (or tag them with a `ARCHIVED:` header). These are ~400KB of historical verdicts referenced nowhere in active flows. `GATE-FINDINGS-LATEST.md` is the only review file that should remain in `docs/reviews/`. **Impact:** reduces active doc surface by 54 files, ~360KB.

### Action 2 — Archive all `docs/design/implementer-loop-track-*.md` + `AMENDMENT-*.md` (root) + `docs/design/PROMPT-*.md` + track changelogs

11 files totaling ~1,100 lines. All tracks are IDLE (per GATE-FINDINGS-LATEST 2026-06-15 PM). These were per-track build instructions and one-off prompts; content is captured in GAPS/CHANGELOG/GATE-FINDINGS-LATEST. Move to `docs/archive/tracks/`. **Impact:** declutters `docs/design/` to ~20 genuinely active design documents.

### Action 3 — Archive `docs/research/venue/*/raw/pages/` (179 files, ~46k lines)

The raw scraped API pages (Kalshi, Polymarket, Kinetics) are source material that was synthesized into the 6 `research.md` files. They are never read directly in active work and make `find`/grep noisy. Move to `docs/archive/research-raw/` or a `.gitattributes` linguist exclusion. Keep the 6 synthesis `research.md` files in place. **Impact:** removes ~179 files, ~46k lines from active doc tree; `docs/research/` shrinks to 6 readable synthesis files.

### Bonus — Split or summarize GAPS.md (5858 lines)

GAPS.md has grown into a historical log of resolved items (95%+ of content is `RESOLVED` entries). Per its own preamble, *"Acceptance requires this file to contain ONLY operator-blocked items."* Create `docs/archive/gaps-history.md` (resolved dump), truncate `GAPS.md` to only the open/operator-pending items. Estimated reduction: 5800 → ~100 lines. This is the single highest-value cleanup for active session legibility.
