# FORTUNA canon migration plan

Not a canonical document. This is the transition plan for promoting `scratch/canon-draft/` into the repo and resolving the existing-doc sprawl. Execute only after Gate 2 approval. No existing doc is deleted; superseded ones move to `/archive` with a one-line stamp.

## Promotion: where each draft file lands

| Draft file | Repo destination |
|---|---|
| README.md | `/README.md` (replaces current) |
| NORTH_STAR.md | `/NORTH_STAR.md` (new) |
| CONSTITUTION.md | `/CONSTITUTION.md` (new) |
| ARCHITECTURE.md | `/ARCHITECTURE.md` (new) |
| STANDARDS.md | `/STANDARDS.md` (new) |
| STATE.md | `/STATE.md` (generated; regenerate on promotion) |
| canon.manifest | `/canon.manifest` (new) |
| .claude/CLAUDE.md | `/.claude/CLAUDE.md` (new) |
| decisions/*.md | `/decisions/` (new) |
| tools/{gen-state.sh,state-inputs.tsv,STATE.spec.md,check-canon.sh} | `/tools/` (new) |
| .github/workflows/canon.yml | `/.github/workflows/canon.yml` (new) |

## Existing docs: disposition

Legend: REWRITE (becomes the canon doc), FOLD (content absorbed into a canon doc, original archived), KEEP-REFERENCE (stays under docs/ as non-canonical reference; strip any duplicated canonical fact and point to the owner), KEEP-LEDGER (permitted non-canonical working file at root), ARCHIVE (move to /archive stamped superseded), DEMOTE (stays in place but loses source-of-truth status with a banner).

| Path | Disposition | Target / note |
|---|---|---|
| README.md | REWRITE | -> canon README.md; its crate map is dropped (ARCHITECTURE owns it) |
| CLAUDE.md (root) | FOLD | invariants -> CONSTITUTION, conventions/DoD -> STANDARDS, rules+pointers -> .claude/CLAUDE.md; root file archived |
| AGENTS.md | KEEP-LEDGER | reduce to a pointer to .claude/CLAUDE.md (it is the per-tool mirror) |
| docs/spec.md | DEMOTE | stays as design reference; add a banner: invariants -> CONSTITUTION, crate map -> ARCHITECTURE are now authoritative |
| docs/architecture.md | FOLD | crate map (fix 16 -> 18) -> ARCHITECTURE; original -> /archive |
| docs/system-e2e-overview.md | FOLD | data flow + crate map -> ARCHITECTURE; fixes the dead `fortuna-cognition/scoring.rs` ref; original -> /archive |
| docs/operations.md | KEEP-REFERENCE | operational runbook; strip crate/invariant restatements, point to canon |
| docs/operator.md | KEEP-REFERENCE | operator procedures; add the backtest/validate CLI (currently only in ws4-demo runbook) |
| docs/quickstart.md | FOLD | build/run essentials -> README; keep extended setup as reference |
| docs/playbook.md | KEEP-REFERENCE | build-process doc (now linked/authoritative); not system canon |
| docs/verification.md | KEEP-REFERENCE | verification process; not system canon |
| docs/runbooks/** | KEEP-REFERENCE | operational; includes ws4-demo |
| docs/design/** | KEEP-REFERENCE | the WS3 backtest design spec is folded into ARCHITECTURE; mark it folded |
| docs/research/** | KEEP-REFERENCE | research memos and venue grounding |
| docs/reviews/** (60) | KEEP-REFERENCE | gate verdicts; audit-trail value |
| docs/deuce, docs/heater, docs/kairos | KEEP-REFERENCE | Python research MVPs (out of workspace) |
| docs/diagrams, docs/mockups | KEEP-REFERENCE | visual reference |
| docs/superpowers/** | KEEP-REFERENCE | build-process specs/plans |
| docs/audit/2026-06-18/** (18) | ARCHIVE | prior audit cycle; -> /archive |
| docs/close-the-loop-fixes.md | ARCHIVE | historical fix log; -> /archive |
| docs/kickoff/** | ARCHIVE | historical; -> /archive |
| FINAL_REPORT.md | ARCHIVE | against spec v0.8, superseded; -> /archive |
| GAPS.md | KEEP-LEDGER | authoritative open-issue ledger (retire stale-open G1/G2 per this session) |
| ASSUMPTIONS.md | KEEP-LEDGER | assumptions ledger |
| CHANGELOG.md | KEEP-LEDGER | change log |
| BUILD_PLAN.md | KEEP-LEDGER | task list; archive when the build phase closes |
| PROMPT.md | KEEP-LEDGER | mission/build instructions |
| docs/archive/** | LEAVE | already archived |

## Archive stamp

Each archived file gets a one-line header prepended on move:

```
> ARCHIVED 2026-06-22. Superseded by <canon doc>. Kept for provenance; not source of truth.
```

Move target: `/archive/<original-relative-path>`.

## Drift fixes folded into the migration (no separate task)

- Crate count: ARCHITECTURE.md is now the sole owner and states 18; README/system-e2e/architecture restatements are removed or folded.
- Dead reference: the `fortuna-cognition/scoring.rs` citation is dropped when system-e2e folds into ARCHITECTURE.
- `fortuna-backtest` and `fortuna-scoring`: now present in the ARCHITECTURE crate map.
- I4 wording: CONSTITUTION states the canonical I4 including "Postgres"; spec.md is demoted, so its narrower wording is no longer authoritative.
- GAPS.md G1/G2: retired in this session with fixing-commit citations (operator-approved).

## Coverage debt to schedule (from CONSTITUTION gap list)

These are not doc moves but must be tracked alongside promotion:
1. Write `s2_money_type_integrity` and `s3_clock_only_determinism` invariant tests (flip CONSTITUTION map rows to `present`).
2. Write the parametric `i5_all_append_only_tables_reject_mutation` test.
3. Extend `scripts/check-protected-invariants.sh` to also scan `crates/fortuna-invariants/src/lib.rs`.
4. Decide and record (ADR) the `prompt_hash` defect: implement provenance hashing or amend the spec.
5. Stand up the remote so `invariants-dst.yml` and `canon.yml` actually run; then consider flipping `canon.yml` to blocking.
