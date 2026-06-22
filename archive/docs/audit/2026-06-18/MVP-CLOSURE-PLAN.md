# MVP Closure Plan — `fortuna start paper-demo`

**Authored from the 2026-06-18 audit** (`docs/audit/2026-06-18/AUDIT.md` + `verification.md`). This is **Phase C**: it runs *after* Phase B consolidation (refactor roadmap in AUDIT.md §12). Every gap below is a verified finding; nothing here is a guess.

## North Star
A safe, replayable, venue-connected system that closes the loop from live market data → risk-gated paper execution → auditable PnL → scored validation, toward a $50k/month validated PnL run-rate.

## Definition Of Done
`fortuna start paper-demo` boots a lean, single-trunk system on **live Kalshi data** with **no constructible order-mutation path**; the loop **settles and scores**; all strategies accrue paper data; one view shows the full chain (mode, freshness, belief, proposal, gate, paper fill, settlement, score); calibration accrues so the model arm trades. `cargo fmt`, `clippy -D warnings`, the invariant suite, and DST all green.

## Required Loop
`signal → event → belief → market mapping → proposal → gate → paper-fill → settlement → realized PnL → belief/trade score → (calibration) → ramp signal`, every transition persisted and operator-visible.

## Current Gaps (verified)

| Gap | Sev | Evidence | Fix | Test |
|---|---|---|---|---|
| Settlement in-memory only; `settlement_entries`=0 | P0 | `runner.rs:1918`; `SettlementsRepo::insert_entry` 0 prod callers | call `SettlementsRepo::insert_entry` when a market resolves | resolve a market → row in `settlement_entries`; realized PnL computed |
| Calibration fit never persisted; `calibration_params`=0 | P0 | `daemon.rs:4222`; `CalibrationParamsRepo::insert` 0 prod callers | in `stage="paper"`, persist the weekly review's `fitted` Platt | ≥50 resolved beliefs → a `calibration_params` row → synthesis sizes non-zero |
| Fills not persisted; `fills`=0 | P1 | `FillsRepo::insert` 0 prod callers; `runner.rs:1442` | call `FillsRepo::insert` on fill-applied | paper fill → row in `fills` |
| Bus recording dropped; no live replay | P1 | `runner.rs:90` ShutdownReport lacks `recording_jsonl` | thread `recording_jsonl` into the durable shutdown path | recorded segment replays byte-identically |
| No single demo command | P1 | `main.rs:8-17` | add `fortuna start paper-demo` (wraps demo-launch steps + readiness check) | command boots demo; prints mode/order-mutation |
| ROTA hides safety state | P1 | `daemon.rs:1421-1438`; `rota.rs:2107` | emit + render `execution_mode`, `order_mutation_enabled`, book freshness | dashboard shows mode + "orders: disabled" |
| Persona charter never injected | P1 | `main.rs:474` `synthesis_mind.clone()`; `persona_runner.rs:213` | build per-persona `AnthropicMind` with `system_charter = def.method` | persona run sends persona charter, not synthesis charter |
| Personas OFF + registry empty | P1 | no `[personas]`; `personas`=0 | seed `personas` rows + add `[personas]` enable + budget | boot with personas enabled succeeds; `domain_analyses` accrue |
| World-forward unscoreable trap | P1 | `discovery.rs:689` exact-match vs prose `resolution_source` | normalize/fuzzy-match resolution_source to registry IDs | world-forward event gets a belief + becomes scoreable |
| funding GRANT missing on fresh DB | P1 | `daemon.log:32` | add `GRANT INSERT ON funding_rates_historical` to demo-launch | fresh-DB boot → funding inserts succeed |
| stale-book not gated | P2 | no `BookAge` check in gate pipeline | add a freshness gate (reject orders on stale book) | stale book → proposal rejected |
| clippy `collapsible_if` | Minor | `daemon.rs:1681` | collapse the `if` | `clippy -D warnings` clean |

## Phase 1: Make The System Safe (mostly done — make it *visible*)
Safety is already code-enforced (I1–I7, 34 tests pass; no real-order path in paper). Remaining: **ROTA must show `execution_mode` + `order_mutation_enabled`** so the operator can *see* orders are suppressed; fix the clippy error (DoD); add the freshness gate (stale-book reject); add a `doctor` readiness check.

## Phase 2: Close The Demo Loop (the core — root cause A)
Wire the four `insert` calls so outcomes persist: **settlement → realized PnL**, **fills → `fills`**, **calibration (paper) → `calibration_params`** (which wakes the synthesis/model arm), and confirm weather belief grading feeds calibration. After this, the loop closes and **data accrues**: realized paper PnL per strategy + scored beliefs + a fitted calibration.

## Phase 3: Make It Replayable
Persist the live bus recording (`recording_jsonl`) to a durable store and add a replay test that reproduces a live segment's decisions + PnL from events. Add the missing WS `trade`-frame fixture so paper-fill realism has a regression seed.

## Phase 4: Make It Operable
`fortuna start paper-demo` single command + the one chain-view; resolve the dual mode model to one authoritative axis; add the funding GRANT + operator `rearm` verb; clean stale DBs + the pointer file; Phase-B branch/doc consolidation.

## Phase 5: Prepare For Live Mode (out of scope for the demo)
Add a compile-time barrier for `ProductionOrders` (runtime gate already refuses it); the promotion ladder + ramp engine; live credentials handling. **Not part of demo-paper-ready.**

## Non-Negotiable Safety Checks (must stay true throughout)
1. Demo/paper cannot construct a real-execution path (invariant `i_paper_live_no_real_order` — keep green).
2. Live mode requires explicit arming (runtime refuses `ProductionOrders`).
3. The gate is mandatory; every order is a sealed `GatedOrder` (I1).
4. Every decision is audited (append-only, DB-enforced).
5. PnL is reconstructable from persisted events (the Phase-2 + Phase-3 goal).
6. Replay reproduces decisions (Phase-3 goal).
7. Kill switch independent of Postgres/cognition/event-loop (I4 — keep green).

## Strategy & persona scope for the demo
**All 5 strategies plug into the one closed spine.** mech_extremes / mech_structural trade from day 1; synthesis activates once calibration accrues (Phase 2 + ~2–3 weeks of resolved beliefs); perp_event_basis_v2 + funding_forecast accrue data. **Personas (2–3, evidence-picked):** weather (`meteorologist`, proven) first; econ/finance (`macro-economist` config exists) after the Area-9 seeding fix; a third (politics or crypto) chosen by a short market-quality research pass. Persona activation needs the charter fix + registry seed + enable (Phase 1/2).
