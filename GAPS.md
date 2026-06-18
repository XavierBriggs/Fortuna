# GAPS.md — honesty ledger (open items only)

Open items the implementation defers, lacks, or needs from the operator, each with exact
unblock steps. The full RESOLVED history (5858-line ledger) was archived 2026-06-18 in the
Phase B consolidation → **`docs/archive/gaps-history.md`**.

## Authoritative open-items source (2026-06-18 ground-truth audit)

The audit is now the canonical "what's open + readiness" source:
- `docs/audit/2026-06-18/AUDIT.md` — risk register (P0–P3) + Demo-Paper-Ready Readiness Scorecard.
- `docs/audit/2026-06-18/MVP-CLOSURE-PLAN.md` — verified gaps + phased close-the-loop plan (Phase C).

## Close-the-loop wiring (Phase C — blocks demo-paper-ready)
- **F0** Calibration fit but never persisted → model arm never sizes. Persist fitted Platt in `stage="paper"`.
- **F1** Settlement in-memory only (`settlement_entries`=0) → no realized PnL. Wire `SettlementsRepo::insert_entry`.
- Fills not persisted (`fills`=0); live bus recording dropped at shutdown → no live replay.
- No `fortuna start paper-demo` CLI; ROTA Health omits `execution_mode`/`order_mutation_enabled`.
- Personas inert: charter never injected (`main.rs:474` uses synthesis charter) + registry empty + `[personas]` OFF.
- World-forward unscoreable trap (`discovery.rs:689` exact-match vs prose `resolution_source`).
- Market-back never dedups events (`daemon.rs:2010` empty `existing_events`); no DB unique constraint on events.

## Operator actions (runtime; not code — daemon was live during Phase B)
- Stale demo DBs `fortuna_demo_paper_green_2026061704*` (×4) + `fortuna_demo_paper_live` are abandoned snapshots —
  drop manually when convenient (`DROP DATABASE` is irreversible; left for the operator). The LIVE DB is `fortuna_demo`.
- `data/runtime/current-demo-db-url` is STALE (points at `green_044732`). Proper fix: daemon writes the live
  `DATABASE_URL` on boot. Until then ignore the pointer; the live DB is `fortuna_demo`.
- `GRANT INSERT ON funding_rates_historical` must be applied on a fresh demo DB (add to demo-launch runbook).

## Branch follow-ups (Phase B; all archive-tagged — recover via `git checkout archive/<branch>`)
- **`track-b`** (worktree `/Users/xavierbriggs/fortuna-wt-b`): **40 UNCOMMITTED files** — review + commit/discard.
  NOT touched in Phase B (uncommitted work is not in any tag).
- `track-d`, `track-e-docs-freshen`: stranded doc-only corrections (GAPS/BUILD_PLAN freshening), superseded by this
  prune; kept for review — delete when satisfied.

## Deferred refactors (Phase B roadmap; P2 legibility — no behavior change, test-gated when done)
- File splits: `daemon.rs` (4854L), `repos.rs` (2479L), `rota.rs` (2227L); a `DriveContext` for `drive()`'s 20-param
  signature. (AUDIT.md §12 / area-4)
- Dual mode model (`[runtime]` vs `[daemon]`): coherent + cross-validated today; collapse-to-one-axis deferred. (area-2)
- `AnthropicVetoMind`: `StubVetoMind::allow_all` inert stub. (area-2)
