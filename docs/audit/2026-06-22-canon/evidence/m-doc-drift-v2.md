# FORTUNA existing-doc inventory & drift map — v2 re-run

- **Review target:** `/Users/xavierbriggs/fortuna-main`, branch `main`, HEAD `1bb6959`. All reads on main.
- **Date:** 2026-06-22. Read-only pass.
- **Workspace reality (ground truth):** 18 crates (`find crates -name Cargo.toml -maxdepth 2 | wc -l` = 18). NEW since baseline: `fortuna-backtest` (src: asof/edge_provider/harness/manifest/records/source/sources/sweep). `fortuna-scoring` already existed (src: corp/deflation/dm/murphy_diagram/pav/pit/rules/samples/scorecard).
- **Total markdown:** 388 files (`find . -name '*.md'` excl target/.git/node_modules). The bulk is `docs/research/venue/**/raw/pages/` (recorded venue API HTML→MD, reference-tier, ~200 files).

---

## A. Prior drift items — status on main

| # | Item | Status | Citations (both sides) |
|---|------|--------|------------------------|
| 1 | Crate count stated 4+ ways; README/arch/e2e = 16; actual now 18 | **STILL-PRESENT (now 2 behind, was the prior state — unchanged)** | Actual 18: `crates/*/Cargo.toml` (18). README says "Sixteen-crate": `README.md:165`. architecture.md "Sixteen crates as of `f31aaa8`": `docs/architecture.md:169`. system-e2e "**16 crates**": `docs/system-e2e-overview.md:16`. None of these three was touched since baseline (README/arch last `9f6ddd1` 2026-06-15; e2e `9f6ddd1`/`e89062f` 2026-06-15) so the additive WS3/WS4 crates (`fortuna-backtest`, and `fortuna-scoring` not in any list) widened the gap from 16→18. |
| 1b | spec.md = 8-crate sketch (stale L0) | **STILL-PRESENT (unchanged)** | `docs/spec.md:90-105` lists exactly 8: core, gates, exec, state, venues, ledger, cognition, ops (`sed -n '95,102p'` → 8 names). Last spec touch `39019c0` 2026-06-14; the v0.2 layout block is explicitly an L0 sketch, never updated. Conservative read: not a contradiction with reality so much as a never-refreshed sketch — but it remains the 5th distinct number. |
| 1c | FINAL_REPORT.md = 13 crates | **STILL-PRESENT (unchanged)** | "Thirteen crates, ~41,500 lines": `FINAL_REPORT.md:18`. Frozen artifact (build completed 2026-06-10). |
| 2 | system-e2e:151 cited deleted file `fortuna-cognition/scoring.rs` | **STILL-PRESENT** | `docs/system-e2e-overview.md:151` header still reads `### 4.5 Beliefs, calibration, scoring (`fortuna-cognition/beliefs.rs`, `calibration.rs`, `scoring.rs`)`. `ls crates/fortuna-cognition/src/scoring.rs` → **No such file**; `find crates/fortuna-cognition -name 'scoring*.rs'` → none. The scoring math now lives in the standalone `fortuna-scoring` crate (corp/dm/pav/pit/rules/scorecard) + `fortuna-cognition/src/{calibration.rs, persona_scoring.rs, scorecard_agg.rs}`. Dead file reference unchanged. |
| 3 | I4 wording drift: spec omits "Postgres" from kill-switch independence; CLAUDE.md + README add it | **STILL-PRESENT (unchanged)** | spec I4 (canonical): "must not depend on the cognition runtime, the event loop, or any LLM provider being healthy" — **no Postgres** in the I4 clause: `docs/spec.md:43`. (spec carries the Postgres-independence elsewhere — Principle 9 `docs/spec.md:31`, and §5.15 `docs/spec.md:308` "Still no Postgres" — but not in the I4 statement itself.) CLAUDE.md I4 adds it: `CLAUDE.md:18-19` ("…the event loop, Postgres, or any LLM provider…"). README I4 adds it: `README.md:31` ("…the cognition runtime, the event loop, Postgres, or any LLM provider…"). The three I4 *statements* are still not word-identical; substantively reconciled by spec Principle 9, so this is wording-drift not a contradiction. |
| 4 | FINAL_REPORT.md written against spec v0.8 while spec is v0.9 | **STILL-PRESENT (unchanged)** | `FINAL_REPORT.md:3` "Build completed 2026-06-10 against docs/spec.md v0.8". spec is v0.9: `docs/spec.md:5`. FINAL_REPORT itself flags it: `:118-119` "Spec v0.9 touch-up is an operator action (GAPS)." Acknowledged-stale, not silently wrong. |
| 5 | docs/playbook.md orphan | **FIXED / superseded by triage** | `docs/playbook.md` IS now referenced: `docs/audit/2026-06-18/doc-triage.md` and `docs/superpowers/plans/2026-06-16-paper-on-live-data.md` link it, and the 2026-06-18 doc-triage explicitly classified it **authoritative** (`doc-triage.md:54` "`docs/playbook.md` … authoritative … Operational playbook"; it is in the 28-doc SoT set at `doc-triage.md:19`). It is no longer an unreferenced orphan. |

**Net:** of 7 prior sub-items, 1 FIXED (playbook), 6 STILL-PRESENT (none WORSE in kind, but item 1 is materially worse in *magnitude* — gap grew 16→18 because the canonical trio was never touched while two crates were added).

---

## B. New docs since baseline (selected; classification)

Baseline note: "since baseline only ASSUMPTIONS/CHANGELOG/GAPS + two WS4 docs changed" — but `git diff --stat HEAD~40` shows 78 md files changed/added (15,282 insertions). The additive WS3/WS4 and modeling-track docs are the bulk. Key NEW canonical-adjacent docs:

| Doc | One-line | Classification |
|-----|----------|----------------|
| `docs/superpowers/specs/2026-06-21-ws3-generic-backtest-design.md` | 143-line design spec: 3 locked decisions, Source/Record contracts, the four integrity gates (G-PIT/G-DEAD/G-PARITY/G-TRUTH), replay harness, sweep+deflation, AeolusArchiveSource, CLI, DST, invariant safety | **keep-canonical → fold-candidate into ARCHITECTURE** (this is the only structured description of the `fortuna-backtest`/`fortuna-scoring` subsystem; see §C) |
| `docs/superpowers/plans/2026-06-21-ws3-generic-backtest.md` | 207-line S1–S7 implementation plan + integrity-gate boundary | reference (plan, executed) |
| `docs/research/2026-06-21-ws3-backtest-overfitting-grounding.md` | 112-line research grounding: PBO/CSCV, purge/embargo, SPA, MinTRL, DSR | reference (research provenance) |
| `docs/superpowers/specs/2026-06-21-ws4-demo-surface-design.md` | 108-line WS4 demo-surface design (E1–E6 + G1 CLV-for-persona) | keep-canonical (track-own) |
| `docs/superpowers/plans/2026-06-21-ws4-demo-surface.md` | 225-line WS4 plan W2–W7 | reference |
| `docs/runbooks/ws4-demo.md` | 159-line operator runbook: `doctor → backtest aeolus-archive → validate → start paper-demo → GET /chain`; Brier-primary GO honesty clause | **keep-canonical (runbook)** — currently the ONLY doc that documents the `fortuna backtest` / `fortuna validate` CLI surface |
| `docs/superpowers/specs/2026-06-20-ws2-proof-layer-design.md` | 353-line WS2 proof-layer design | keep-canonical (track-own) |
| `docs/superpowers/specs/2026-06-19-scoring-and-validation-architecture.md` | 234-line scoring/validation architecture | keep-canonical (track-own); overlaps fortuna-scoring (see §D) |
| `docs/research/2026-06-20-ws2-scoring-grounding.md` (+ `/PLAN.md`) | WS2 scoring research grounding | reference |
| `docs/audit/2026-06-18/*` (11 files), `docs/reviews/2026-06-18*` & `2026-06-19-task-ws1-3.md` | Phase-A ensemble audit + per-task gates | reference/archive (dated) |
| `docs/deuce/`, `docs/heater/`, `docs/kairos/` READMEs+SPEC+SOURCES; `docs/research/2026-06-18-{baseball,tennis,perpetual-futures}*`, `2026-06-19-kairos-ec2-deployment.md` | Out-of-tree modeling/research tracks (DEUCE tennis, HEATER baseball, KAIROS perps) | reference (research; not part of the Rust workspace) |
| `docs/superpowers/{loop-close-captain-charter,loop-close-gaps,loop-close-operator}.md` | Captain-loop orchestration charter + operator/gaps surfaces | reference (process) |
| `research-workspace/PLAN.md` | scratch plan | archive/ignore |

---

## C. NEW drift

### C1. The `fortuna-backtest` subsystem is invisible to every canonical doc (HIGH)
`grep -c 'fortuna-backtest\|fortuna-scoring'` = **0** in `docs/spec.md`, `docs/architecture.md`, `docs/system-e2e-overview.md`. README's two "backtest/scoring" hits are unrelated (`README.md:32` = belief scoring columns, `:79` = Brier/CRPS settlement scoring) — neither crate is named.
- architecture.md crate map (`docs/architecture.md:174-344`) enumerates exactly 16 bold `**[fortuna-*]**` entries — **no backtest, no scoring**.
- system-e2e crate-map table (`docs/system-e2e-overview.md:306-323`) lists 16 crates — **no backtest, no scoring** (only `fortuna-paper` at `:313`).
- The only mention of "backtest" in spec.md is the *philosophical* "not a backtesting playground" (`docs/spec.md:15`) and "Backtests validate deterministic components only" (`docs/spec.md:26`, Principle 4) — which is the *rationale* for WS3's deterministic-component-only scope, but does not document the crate.

### C2. The `fortuna backtest` / `fortuna validate` CLI is documented only in the WS4 runbook, not in operator docs (MEDIUM)
`grep -rln 'fortuna backtest|fortuna validate' docs/*.md docs/runbooks/*.md` (excl research/superpowers/reviews/audit) → **only** `docs/runbooks/ws4-demo.md` (`:49,:55,:71,:74` etc.). The canonical operator surfaces — `docs/operator.md` (operator action list) and `docs/operations.md` (daily-operator manual) — do not mention the backtest/validate verbs at all. New operator-facing CLI capability exists with no entry in the two docs an operator would read first.

### C3. WS3 design doc is canonical-quality but unfolded (MEDIUM — the fold-candidate)
`docs/superpowers/specs/2026-06-21-ws3-generic-backtest-design.md` (143 L) is the *only* structured spec of the new subsystem: it defines the Source/Record contracts (§4), the four integrity gates G-PIT/G-DEAD/G-PARITY/G-TRUTH (§5), the replay harness (§6), the sweep + deflation machinery (§7), `AeolusArchiveSource` (§9), and invariant safety (§12). architecture.md is the home for the crate map and gate pipeline but has no backtest section. Recommend folding a condensed "fortuna-backtest / fortuna-scoring" entry + a short "backtest integrity gates" subsection into `docs/architecture.md` (and a crate-map row into `docs/system-e2e-overview.md:306-323`).

### C4. CHANGELOG / GAPS / ASSUMPTIONS are internally consistent with the code (GOOD — no drift)
- **Guardian findings present in GAPS.** "edge-provider placeholder" + "purge-not-wired" are recorded: `GAPS.md:224-243` (G1: "`fortuna validate` ships a placeholder edge-provider; purge/embargo unreachable in production"; cites `crates/fortuna-cli/src/backtest_cmd.rs:173-178` and `crates/fortuna-backtest/src/sweep.rs:332-336`). "decoupling/scoring-purity not enforced by a test" recorded as G2: `GAPS.md:245-251`. These match the WS3 boundary commit `c26abc6`.
- Note: G2's *Unblock* ("add a `#[test]` that greps fortuna-backtest/src for source literals + asserts fortuna-scoring dep set") was subsequently **implemented** per `CHANGELOG.md:15` (`crates/fortuna-backtest/tests/decoupling.rs`, three `#[test]`s, mutation-proven). So GAPS G2 is now *stale-open* — the gap it describes was closed in W6b but the GAPS entry was not retired. (Minor — a GAPS/CHANGELOG reconciliation item.)
- **CHANGELOG documents the new crates' work** (`CHANGELOG.md:15,17` reference `fortuna-backtest`/`fortuna-scoring` by path; WS1 entry `:26` notes backtest rows excluded from promotion count).
- **ASSUMPTIONS documents the decoupling discipline** (`ASSUMPTIONS.md:2337-2357` WS3 S2 replay-harness scoping; `:2357` notes `crates/fortuna-backtest/src/` is grep-gated against source literals).

### C5. `fortuna-scoring` crate is undocumented anywhere canonical (HIGH — companion to C1)
The crate physically exists with 9 modules (`corp, deflation, dm, murphy_diagram, pav, pit, rules, samples, scorecard`) and is a real `path` dependency of BOTH `fortuna-cognition` (`Cargo.toml:8`) and `fortuna-backtest` (`Cargo.toml:13`) — i.e. it is the extracted pure-math scoring library. It predates the baseline yet has never appeared in any crate map. Whereas system-e2e §4.5 (`:151`) still points scoring at the deleted `fortuna-cognition/src/scoring.rs`.

---

## D. Overlap clusters (topics with multiple owners) — updated

| Topic | Owners | Note |
|-------|--------|------|
| **Crate count / crate map** | spec.md:90-105 (8, L0 sketch) · README.md:165 (16) · architecture.md:169 (16) · system-e2e:16,306-323 (16) · FINAL_REPORT.md:18 (13) — **actual 18** | 5 distinct numbers; the canonical trio (README/arch/e2e) is internally agreed at 16 but 2 behind reality. |
| **Scoring math** | `fortuna-scoring` crate (corp/dm/pav/pit/rules/scorecard) · `fortuna-cognition` (calibration.rs, persona_scoring.rs, scorecard_agg.rs) · system-e2e §4.5 (`:151`, points at DELETED scoring.rs) · `docs/superpowers/specs/2026-06-19-scoring-and-validation-architecture.md` · CHANGELOG `fortuna-cognition::scoring` (`:510`) | Genuine multi-owner: pure rules live in fortuna-scoring; calibration/aggregation in cognition; e2e doc's pointer is dead. |
| **I4 kill-switch independence wording** | spec.md:43 (no Postgres in the I4 clause) · spec Principle 9 :31 · spec §5.15 :308 · CLAUDE.md:18-19 (adds Postgres) · README.md:31 (adds Postgres) | Substantively reconciled; statement wording not unified. |
| **Backtest / validate CLI** | `docs/runbooks/ws4-demo.md` (only) · WS3 design spec · WS3 plan — NOT in operator.md / operations.md / architecture.md | New surface; canonical operator docs silent. |
| **GO/NO-GO validation gate thresholds** | spec §11 · `GAPS.md:80` ("GO-gate config diverges from spec §11 thresholds") · CHANGELOG W6b config-hardening (`:13`) · ws4-demo runbook `:96` · WS3 G-TRUTH design §5 | Config↔spec divergence is itself a tracked GAPS item; WS4 W6b partially closed it (14→30, 0.5→0.35, 100→60). |
| **Demo bring-up flow** | `docs/runbooks/demo-bringup.md` (umbrella) · `docs/runbooks/demo-flip.md` · `docs/runbooks/ws4-demo.md` (new) | Three demo runbooks now; ws4-demo adds the backtest/validate path the others lack. |

---

## E. Recommendations (for the operator queue — not applied here)

1. **Crate-count one-liner fix (×3):** `README.md:165` ("Sixteen-crate"→"Eighteen-crate", add backtest+scoring to the inline list), `docs/architecture.md:169` ("Sixteen crates"→"Eighteen"), `docs/system-e2e-overview.md:16` ("16 crates"→"18") + add two crate-map rows (`architecture.md` after the recorder entry; `system-e2e:323` table).
2. **Dead-file fix:** `docs/system-e2e-overview.md:151` — drop `scoring.rs` from the §4.5 header or repoint to `fortuna-scoring` + `scorecard_agg.rs`.
3. **Fold WS3 design into ARCHITECTURE:** add a "fortuna-backtest / fortuna-scoring (offline replay & integrity gates)" section sourced from `2026-06-21-ws3-generic-backtest-design.md` §3–§7,§12.
4. **Operator-doc coverage:** add the `fortuna backtest` / `fortuna validate` verbs to `docs/operator.md` and/or `docs/operations.md` (cross-link ws4-demo runbook).
5. **GAPS reconcile:** retire/annotate `GAPS.md:245` (G2) — its Unblock test was shipped (`decoupling.rs`, `CHANGELOG.md:15`).
6. **I4 wording:** optional — unify the I4 *statement* across spec.md:43 / CLAUDE.md:18 / README.md:31 (or footnote that Postgres-independence is carried by Principle 9).

---

## Appendix — evidence commands run (all read-only on main)

- `find crates -name Cargo.toml -maxdepth 2 | wc -l` → 18
- `find . -name '*.md' -not -path './target/*' … | wc -l` → 388
- `grep -c 'fortuna-backtest\|fortuna-scoring'` → spec 0, architecture 0, system-e2e 0
- `ls crates/fortuna-cognition/src/scoring.rs` → No such file
- `grep -n 'fortuna-scoring' crates/fortuna-{cognition,backtest}/Cargo.toml` → both depend on it
- `git log -1 --format=… -- <doc>` → README/arch `9f6ddd1` 2026-06-15; e2e `e89062f`/`9f6ddd1` 2026-06-15; spec `39019c0` 2026-06-14 (canonical trio untouched post-baseline)
