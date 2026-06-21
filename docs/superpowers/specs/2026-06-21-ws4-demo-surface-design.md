# WS4 — Demo Surface (Design Spec)

**Status:** design (decision-grade v1) · **Date:** 2026-06-21 · **Authority:** `docs/spec.md` (v0.9) > `CLAUDE.md` > the milestone spec (`2026-06-19-loop-close-and-provable-demo-design.md`, §WS4 + the worked example) > this. Invariants I1–I7 are absolute.

**Goal:** Make the closed, provable loop **showable** — a demo-readiness *backend* (CLI + endpoints + a serialized chain-view contract) that turns the WS1 live spine + WS2 proof layer + WS3 backtested track record into a runnable, inspectable demo. The rich render is a **separate UI session's** job against our committed contract.

**Architecture:** FORTUNA owns the **data, endpoints, and serialization**; the UI session renders. **Contract-first** — the E3 chain-view contract commits before its endpoint so the UI session unblocks and builds in parallel (the same pattern WS2 used for the scorecard contract). No web/render code in WS4.

**Tech Stack:** Rust 2021; `serde` (the contract); `axum` (the ROTA route); `sqlx` (ledger reads, compile-checked + `SQLX_OFFLINE`); the `fortuna-cli` hand-rolled dispatch; reuse the WS2 `Scorecard` + WS3 `ValidationRun` contract types.

## Global Constraints
- Rust 2021; integer `Cents`; no `panic!`/`unwrap`/`expect` in non-test code; `thiserror` per crate.
- All time via the injected `Clock`. `sqlx` compile-checked; DB tests/clippy under `SQLX_OFFLINE=true DATABASE_URL=postgres:///fortuna?host=/tmp`.
- Read-only views (I5); the model authors nothing new (I6); `crates/fortuna-invariants/` additions-only.
- Paper-safe: the demo runs `execution_mode="paper_ledger"`; the `i_paper_live_no_real_order` wall holds (no real-venue order, ever).
- Secrets env-only, never printed (presence/length checks only).

## 1. Context

- **Where WS4 sits (milestone §3):** WS1 (live spine) + WS2 (proof layer) are merged to main; WS3 (the generic backtest that *seeds the real track record*) is being built in parallel. WS4 is the milestone's finish line — the demo surface.
- **The demo's job (scoring-arch spec §0):** accrue **trustworthy validation data** (per-producer scored beliefs + CLV + realized PnL) so the edge is *measurable* before capital scales. The demo is the **forward-collection entry point**, not a throwaway.
- **The split:** FORTUNA = data + endpoints + contract. The **UI session (separate) = render**, building against the committed contract and waiting for the WS4 contract commit.

## 2. Slices (contract-first ordering)

### W1 — E3 chain-view CONTRACT (commits first; the UI session's input)
Pure `Serialize` types in `fortuna-ops` (the ROTA crate), composing the WS2 `Scorecard` and WS3 `ValidationRun` contract types (reuse, never redefine). Golden-JSON tests; **no endpoint yet**. This is the commit the UI session waits on.

```
ChainView {
  event:   { event_linkage, category, scope, target_date, market_ticker }
  safety:  { execution_mode, order_mutation_enabled, book_freshness_secs }       // safety pills
  signals: [ { source, kind, at, summary } ]                                     // what triggered it
  producers: [                                                                   // the head-to-head, per producer
    { producer_id, producer_type, mind_id?, mind_version?,
      p_raw, p_cal, rationale?,                                                  // rationale = reasoning drill-in (append-only, never executed)
      belief_at,
      score?: { status, outcome?, brier?, clv_bps? } }                          // post-resolution; CLV non-null for BOTH after W5
  ]
  proposal?:   { market, side, max_price_cents, size, thesis, belief_ref, urgency }
  gate?:       { decision, checks: [ { name, passed, detail } ] }               // the I1 universal-gate trace (render-only, never a bypass)
  fill?:       { price_cents, qty, orders, at }                                 // orders == 0 (paper)
  settlement?: { outcome, realized_pnl_cents, settled_at, resolution_source }
  scorecard?:  <WS2 Scorecard>                                                   // GO whole-truth: Brier-vs-baseline, CORP, DM, reliability
  validation?: <WS3 ValidationRun>                                              // deflated view: PBO, SPA p_c, family_n_trials, verdict — Option-absent until WS3 merges
}
```
- Every stage is `Option` — the chain renders at any maturity; a freshly-tagged event has signals+beliefs but no fill/settle/score yet.
- The head-to-head (`producers[]`) is the showpiece: Aeolus + meteorologist on the same bracket, each with `p_cal`, `brier`, `clv_bps`.

### W2 — E3 endpoint
`GET /api/rota/v1/chain?event=<event_linkage>` assembles `ChainView` from the ledger (signals → beliefs-by-producer → proposal → gate → fill → settlement → scores) + the scorecard + validation. Read-only, GET-only (405 on mutating methods), degrades to HTTP 200 + `{"status":"unavailable"}` per the ROTA R1 doctrine. PATHS count bump + the route-table test (`every_path_is_get_only_and_200`).

### W3 — E1 `fortuna doctor`
A readiness command printing a green/red checklist, exiting non-zero on any red: DB reachable · migrations applied · env/creds present (presence only, never printed) · **mode-safe** (`execution_mode`/`orders_enabled` paper-safe) · GRANTs (the app role can read/write the tables it needs) · source reachable (read-only Aeolus/Kalshi ping). Reuses the ROTA Health probes where they exist.

### W4 — E2 `fortuna start paper-demo`
Extends `start` with a `paper-demo` mode: fresh migrated DB; `execution_mode="paper_ledger"` (paper fills, **no real-venue order** — the `i_paper_live_no_real_order` wall holds); the **F11 pointer-write** — the daemon writes the live `DATABASE_URL` to `data/runtime/current-demo-db-url` on boot (the GAPS-noted stale-pointer fix). The forward-collection entry point.

### W5 — G1 CLV-for-persona fix
At persona belief-formation, match the meteorologist bracket to the corresponding Aeolus bracket's market (by station/date/threshold) and `insert_edge(persona_event_id → that market_id)` (repos.rs `insert_edge`). Then `current_edges_for_event(persona event_id)` resolves → CLV computes for the meteorologist. The head-to-head finally shows CLV for both producers (today it is always `None` for the persona — loop-close-gaps "[Important] CLV per-event linkage", option A). The Brier half (primary GO) already resolves; this completes the CLV half.

### W6 — hardening + docs + config
- **E6:** `rearm` exists (`fortuna-cli` + `gates.rearm()` halt.rs:87); add the **I4 refusal** — refuse to rearm if the kill-switch sentinel is present (the out-of-band kill switch outranks a halt clear).
- **E4 dead-man:** the daemon writes a liveness heartbeat; a dead-man checker alerts + reconnects when it goes stale.
- **E5:** demo runbook (doctor → WS3 backtest-seed → start paper-demo → chain-view) + Aeolus stable-source note + CHANGELOG.
- **Config-cleanup:** GO-gate example config → spec §11 values (paper 30, fee 0.35, synth 60 — loop-close-gaps "GO-gate config vs spec §11"); CLV constants (`CLV_MIN_TOUCH_QTY`/`CLV_MAX_SPREAD_CENTS`, daemon.rs:4743-4744) → `[cognition]` config.

## 3. Invariant safety

- **I1:** the chain-view's `gate` is the universal-gate TRACE for display only — never a path that bypasses or re-runs the gate. (The gate pipeline is unchanged; WS4 only reads its recorded result.)
- **I2/I4:** `rearm` clears a human-cleared halt (I2); W6 hardens it to REFUSE when the kill-switch sentinel is present (I4 — the out-of-band kill switch does not depend on, and is not cleared by, rearm).
- **I5:** all WS4 surfaces are read-only views; no row is mutated. `fortuna doctor` and the chain endpoint never write.
- **I6:** the chain renders RECORDED beliefs/scores; the model authors nothing new. `rationale` is append-only display text, never executed (untrusted-data discipline).
- **I7:** the demo surfaces the GO/NO-GO; promotion to live capital remains an operator action.
- **The paper-demo wall:** `execution_mode="paper_ledger"` keeps `i_paper_live_no_real_order` satisfied — no real order is ever placed by the demo.
- `crates/fortuna-invariants/` is additions-only.

## 4. Testing
- **W1:** golden-JSON round-trip for `ChainView` (incl. `validation: None` and a fully-populated chain).
- **W2:** the route-table test (GET-only, 200-degrade, PATHS bump); an assembly test (a seeded event → the expected chain stages).
- **W3:** `doctor` exits non-zero when a check is red (e.g. migrations missing) — mutation-proof.
- **W4:** the paper-demo mode keeps `i_paper_live_no_real_order` (an executable test + a reds-it mutation); the pointer-write lands the live URL.
- **W5:** a meteorologist belief gets a **non-null `clv_bps`** at resolution (today it's `None`); the head-to-head carries CLV for both.
- **W6:** `rearm` REFUSES when the kill-switch sentinel is present (I4) — mutation-proof; the dead-man checker fires on a stale heartbeat.

## 5. Sequencing & coordination
- **W1 (contract) commits NOW** → the UI session unblocks and builds the render against it. (This is the commit the operator told the UI session to wait for.)
- **W2's endpoint can serve live (WS2) data immediately;** the `validation` field stays `Option`-absent until WS3 merges.
- **Full WS4 implementation sequences after WS3 merges** so the chain renders the real backtested record — but the contract + endpoint do not block on it.
- WS4 builds in its own worktree (own target dir) to avoid build-lock contention with the parallel WS3 builder.

## 6. Deferrals (not WS4)
- Synthesis binary-belief resolution (provenance shape undefined; demo is Aeolus-vs-meteorologist) — YAGNI.
- Per-producer calibration *params* persistence — YAGNI (quality selection already ships).
- G2 unified resolver dispatch (the daemon `_weather_`/`_funding_` fork merge) — a non-demo-blocking refactor; defer.
- The rich web render — the separate UI session's job against the W1 contract.
