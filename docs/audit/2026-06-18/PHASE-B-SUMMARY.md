# Phase B — Consolidation Summary (2026-06-18)

Authored from the Phase-A audit (`AUDIT.md` §12 refactor roadmap + `doc-triage.md` + `branch-ledger.md`).
Executed on branch `feature/paper-on-live-data`; `main` fast-forwarded to the result.

## Done

| # | Action | Result | Commit |
|---|---|---|---|
| 1 | **Integrate in-flight work** | ~1.3k uncommitted lines (runtime ExecutionMode model, OrderMutationPolicy rail, event-exposure gate-9, event_source_evidence migration) committed. Was in NO branch/tag (unrecoverable). Fixed 1 clippy error + migrated `boot_gate.rs`/`kalshi_compose.rs` to the new `[runtime]` contract. **fmt + clippy(-D warnings) + full workspace test suite all green.** | `6a7ed9a` |
| 2 | **Doc archive** | 54 gate reviews → `docs/archive/reviews/` (GATE-FINDINGS-LATEST kept); 254 raw research pages → `docs/archive/research-raw/` (research.md syntheses kept); 4 root AMENDMENTs → `docs/archive/amendments/`. | `c25a377` |
| 3 | **Track artifacts + gitignore** | demo scripts, close-the-loop-fixes.md, Phase-A plan, mockups committed; `.agents/`/`.codex/` gitignored. | `08c407f` |
| 4 | **Branch consolidation** | 15 branches → 5; 5 worktrees → 2. Deleted 10 absorbed/redundant (all archive-tagged). `main` FF'd to trunk (old main at `archive/main`). | (git ops) |
| 5 | **GAPS.md prune** | 5858 → 39 lines (open items only); full history → `docs/archive/gaps-history.md`. Points to the audit as the authoritative open-items source. | this commit |

Working tree is **clean**; all 15 branches preserved as `archive/*` tags (recover via `git checkout archive/<branch>`).

## Kept for operator review (NOT deleted — "no work lost")
- **`track-b`** (worktree `/Users/xavierbriggs/fortuna-wt-b`): **40 uncommitted files** — uncommitted work is in no tag; review + commit/discard.
- `track-d`, `track-e-docs-freshen`: stranded doc-only corrections, superseded by the GAPS prune; kept for review.

## Deferred (with rationale)
- **Database drops** (stale `fortuna_demo_paper_green_*` ×4 + `fortuna_demo_paper_live`): `DROP DATABASE` is irreversible and the daemon was live; left as an operator action (documented in `GAPS.md`).
- **Runtime pointer fix** (`current-demo-db-url`): daemon-owned + the daemon was running; proper fix is a daemon-boot code change (F11), deferred.
- **File splits** (daemon.rs/repos.rs/rota.rs, DriveContext): P2 legibility, no behavior change — a large, risky mechanical refactor best done as its own test-gated effort on the now-clean baseline, not bundled into consolidation.
- **Dual mode model collapse**: the audit found `[runtime]`/`[daemon]` coherent + cross-validated (not competing); collapsing to one axis is a design change deferred.
- **AnthropicVetoMind**: inert stub left in place (P2).

## Next: Phase C
The clean, single-trunk baseline is ready for `MVP-CLOSURE-PLAN.md` — the close-the-loop wiring (F0/F1 + demo CLI + personas) that makes `fortuna start paper-demo` accrue real data.
