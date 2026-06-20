# loop-close-gaps.md — forward / deferred items for the Loop-Close & Provable Demo milestone

Work-specific gaps tracker for **THIS milestone only** — distinct from the repo-wide `/GAPS.md`
and any worktree copy (named explicitly to avoid collision). The loop records deferred/forward
items here. Truly constitutional / repo-wide gaps also belong in `/GAPS.md`.

## Deferred (this milestone)
- **Synthesis BINARY-belief resolution** — DEFERRED. Synthesis provenance shape is undefined; the
  demo head-to-head is Aeolus-vs-meteorologist. Revisit if a synthesis binary producer ships.
  (Cross-ref `/GAPS.md`.)
- **GO-gate config vs spec §11** — the shipped example config diverges (paper 14 vs 30, fee 0.5 vs
  0.35, synth 100 vs 60-spec). Demo + `go_nogo` use the SPEC values; reconcile the example config in
  a later config pass. (Cross-ref `/GAPS.md`.)

## Follow-on milestones (post loop-close — each its own spec → plan → build)
- **G7 — `k_unc`** estimation-uncertainty sizing shrinkage (Baker–McHale / Chu–Wu–Swartz).
- **G8 — Edge Decay Watchdog** (config-named; needs G1–G6 as inputs first).

## Open during the loop (the loop appends here as it discovers gaps)
_(none yet)_

## Moved to WS2 (captain, 2026-06-19T14:49Z)
- **Unified PredictiveKind->BrierRule trait dispatch (D-4 refactor)** moved from WS1 slice 3 to WS2/G5 — it is a no-op until a 2nd ScoringRule (RPS/Log) exists; G5 is its natural home.

## Open during the loop (WS1.4, 2026-06-20T02:23Z)
- **Unbounded `pending_market_quotes` buffer when `snapshots_pool=None`** (runner.rs:216): the daemon gates the drain on `Some(pool)` (daemon.rs:3085), so in DST/no-persist mode the buffer fills each tick and is never drained — bounded only by short runs. Mirrors the gate-accepted `pending_fills` pattern (also drained only under `Some`); production `main.rs` always wires `Some`. No fix now; revisit if a long no-persist run is ever needed.

## Deferred (WS1.7, 2026-06-20T05:43Z)
- **Per-producer calibration PARAMS persistence** (keying the Platt/calibration fit by producer, not just the quality): runner.rs:757 persists producer=None. Slice 7 delivers per-producer QUALITY selection (the thesis payoff); per-producer PARAMS is a follow-on persist/schema change, deferred (YAGNI for the demo).
