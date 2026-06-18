# Operator UI Overhaul — Design

**Date:** 2026-06-18 · **Status:** design approved (operator, 2026-06-18); pending spec review
**Authority:** `docs/spec.md` (v0.9) > `CLAUDE.md` > this. Constitution invariants I1–I7 are absolute.
**Relationship to Phase C:** runs in a **parallel git worktree** while the Phase C "close-the-loop"
track (`docs/superpowers/specs/2026-06-18-phase-c-close-the-loop-design.md`) proceeds on the spine.
This document defines the UI and the **contracts** it consumes; Phase C implements the backend data
behind the new contracts (esp. the chain-view, §2.3/§7 of the Phase C design).

---

## 1. Goal

Replace the flat, single-scroll, 24-panel inline-Rust ROTA console with a **navigable, drill-down,
real-time operator dashboard built for scale**, matching the approved mock (`FORTUNA Operator.dc.html`)
and surfacing the two views the operator currently lacks: **The Loop** (per-market chain-view) and
**The Mind** (cognition reasoning/replay). The overhaul is **additive and parallel-safe**: the existing
server-rendered ROTA stays live until the new UI reaches parity, and the new UI develops against typed
contracts + fixtures with zero hard dependency on the Phase C track finishing.

Non-goals: changing the trading core, adding any state-mutating capability to the operator surface, or
building the Phase C backend wiring itself.

## 2. Context — what exists today

- **Current surface (ROTA v2):** `crates/fortuna-ops/src/rota.rs` (~2,228 LOC). One inline HTML/CSS/JS
  const (`ROTA_SHELL`), served by **axum**, **GET-only**, **no auth**, localhost/Tailscale-bound.
  Vanilla-JS short-polling (2s–30s). No SSE. No Node, no frontend build — pure Rust by deliberate
  constitution choice.
- **24 flat panels:** health, money, gates, cognition, settlement, streams, ingest
  (sources/feed/funnel), fills, working orders, strategy P&L, discovery (events/edges), personas
  (+scores/pipeline), analyses, forecasts, forecast feed, perps, db, telemetry, audit tail.
- **Data flow:** pre-shaped JSON from the daemon tick (`fortuna-live/src/views.rs::views_from`) +
  runtime `sqlx` ledger queries in `rota.rs`; in-process `MetricsRegistry` (Prometheus at `/metrics`).
- **The mock:** a Claude Design Components file (`<x-dc>`, `sc-for`/`sc-if`) — a prototype format, not
  deployable. Same visual DNA as ROTA, elevated: identical palette (`#0A0A0B` bg, `#D4AF37` gold,
  `#FFB84D` amber, `#FF3B30` halt, `#30D158` ok), Inter + Marcellus. Hedge-fund placeholder data
  ($128M, equities/fixed-income, Sharpe/VaR) must be re-grounded to FORTUNA semantics (paper PnL in
  cents, prediction-market positions, Brier/CLV, venues).

### 2.1 Gap (mock → codebase)

| # | Gap | Disposition |
|---|-----|-------------|
| 1 | IA: flat 24-panel scroll → navigable sections w/ drill-down | Restructure (§4) |
| 2 | **The Loop** chain-view: signal→belief→edge→proposal→gate→fill→settlement→PnL | **Greenfield**; Phase C P1 data |
| 3 | **The Mind**: live reasoning / replay, evidence, raw→calibrated, decision queue | New surface; partial data exists |
| 4 | Real-time: short-poll → live | SSE (§6) |
| 5 | Tech stack: prototype/inline-const → scale/future stack | Decided (§3) |
| 6 | Placeholder semantics → FORTUNA domain | Re-ground throughout |

Most mock concepts map to data that already exists (beliefs, calibration, sources, strategies, fills,
orders). The genuinely new builds are **The Loop** and **The Mind** — both Phase-C-aligned.

## 3. Decisions (locked)

### 3.1 Tech stack — **Vite + React + TypeScript SPA** (not Next.js)
Researched (2026): for an internal, localhost/Tailscale-bound, read-only, real-time dashboard with no
SEO and no public users, a Vite SPA beats Next.js — Next.js's SSR/RSC/file-routing value is zero behind
the network boundary, adds cognitive overhead, and forces a **second runtime (Node)** beside the
Rust/axum backend. A static SPA is the cheapest, most portable target; axum serves it via embedded
assets. React = largest talent pool ("inclusive") + richest data/charts ecosystem.

- **UI framework:** React + TypeScript, Vite build.
- **Design system:** shadcn/ui + Tailwind — **owned** components (copy-in, not a dependency).
- **Data:** TanStack Query (snapshots/caching) + TanStack Table (dense grids).
- **Charts:** hand-rolled SVG where the mock already does (sparkline, donut, quantile fan, reliability
  diagram); a small lib (visx) only if a view needs it.
- **Real-time:** SSE (`axum::response::Sse`) — one-directional server→client fits a read-only console;
  simpler than WebSocket.
- **Serve:** embedded into the `fortuna-ops` binary (see §8).

Research sources: designrevision.com/blog/vite-vs-nextjs; techsy.io/en/blog/nextjs-vs-react-vite;
dev.to "Next.js vs Vite 2026"; openwebsolutions.in real-time trading dashboard; dev.to alexeagleson
"Axum + React + Vite + shared types"; tokio-rs/axum discussion #867 (SPA + APIs).

### 3.2 Interaction posture — **read-only + safe affordances**
The UI **never mutates trading state** (stays GET + SSE). It surfaces everything, including the
decision/promotion queue, halt state, and I7 gate status. For operator actions it offers **safe
affordances only**: one-click copy of the exact CLI command (e.g. a pre-filled
`fortuna rearm <scope> --reason "…" --operator <name>`), and deep-links to the relevant decision doc /
runbook. These affordances are **purely client-side** (clipboard + anchor links built from data already
on screen); they introduce **no new endpoint** and nothing the operator clicks reaches the daemon. This
honors I2 (re-arm human + out-of-band), I4 (kill independent of the runtime/UI), I6 (propose-only), I7
(promotion out-of-band), and "build the rails; never simulate the human."

### 3.3 Build sequencing — **foundations-first vertical slice**
Durable skeleton first, then one complete end-to-end slice (Command Center + The Loop), then iterate
remaining sections as independent PRs (§9). The existing ROTA stays live throughout — no big-bang.

## 4. Information architecture

Nine navigable sections, each with a snapshot view + drill-down detail. Subsumes all 24 current panels;
promotes The Loop and The Mind to first-class.

| # | Section | Subsumes / adds |
|---|---------|-----------------|
| 1 | **Command Center** | system/halt status, paper-PnL summary, strategies-at-a-glance, activity, risk limits, alerts |
| 2 | **The Loop** *(hero)* | per-market chain-view: signal→belief→edge→proposal→gate→fill→settlement→PnL + safety state (`execution_mode`, order-mutation, freshness). Phase C P1. |
| 3 | **The Mind** | live reasoning/replay, evidence cited, raw→calibrated, I6 boundary, cost budget, model tiers, decision queue |
| 4 | **Beliefs** | feed, lifecycle, calibration scopes, per-producer scorecards (Brier/CLV), forecasts + forecast-feed, Aeolus-vs-meteorologist head-to-head |
| 5 | **Strategies** | grid → detail w/ I7 forward-validation gate, per-strategy PnL, edges, fills |
| 6 | **Gates & Risk** | I1 rejections-by-check, rate buckets, halt/drawdown — the safety surface |
| 7 | **Execution** | fills, working orders, positions, settlement/PnL, perps basis |
| 8 | **Sources & Discovery** | sources health, ingestion funnel/feed, discovery events/edges, personas/analyses |
| 9 | **Audit & System** | append-only audit tail (I5), telemetry, db counts, recorder streams, build/gate verdict |

## 5. Repo layout

A new top-level `ui/` (sibling to `crates/`), kept **out of the Cargo workspace** so its toolchain never
touches money-path builds.

```
fortuna/
  crates/…                      # Rust, unchanged
  ui/                           # NEW — the SPA
    src/
      app/                      # shell, routing, nav, layout
      design/                   # tokens, shadcn components (owned), charts
      views/                    # CommandCenter, Loop, Mind, Beliefs, Strategies, GatesRisk, Execution, Sources, Audit
      api/                      # typed clients, query hooks, SSE transport
      api/contracts/            # TS types generated from Rust (ts-rs) — committed
      fixtures/                 # canned JSON per contract — fixtures mode
    index.html
    vite.config.ts
    package.json
  assets/fonts/                 # self-hosted Inter + Marcellus (no CDN)
```

## 6. Contract strategy — the parallel-safety mechanism

The UI builds against a **typed, versioned contract layer**, never against the daemon directly. This is
what lets the UI worktree and the Phase C worktree move independently.

- **`ts-rs` generates the TS contract types from the Rust view structs** (the `/api/rota/v1/*` response
  shapes in `views.rs` and the new chain-view/mind structs). A Rust struct change regenerates the TS and
  breaks the UI build → contracts cannot silently drift. Generated types are committed under
  `ui/src/api/contracts/`; a CI check fails if regeneration produces a diff.
- **Fixtures mode** (`VITE_MODE=fixtures`): canned JSON for every contract. The SPA develops, renders,
  and runs its full test suite with **no running daemon and no dependency on Phase C completion**.
- **New surfaces define their contract shape now** as a stub + fixture (chain-view, mind reasoning). The
  Phase C track implements the backend endpoint to match the agreed shape. **The contract is the only
  coordination point** between the two worktrees — there is no code-level coupling.
- **Versioning:** existing endpoints stay `/api/rota/v1/*`. New endpoints are added under v1 where
  additive; a `v2` namespace is introduced only if an existing shape must change incompatibly.

### 6.1 New contracts this overhaul introduces (shapes to be finalized with the Phase C track)
- `GET /api/rota/v1/chain?market=<id>` — the per-market chain: ordered stages (signal, belief(s) by
  producer, edge, proposal, gate decision w/ per-check verdicts, fill(s), settlement, realized PnL) +
  safety state. Backed by Phase C's chain-view work.
- `GET /api/rota/v1/mind/sessions` and `…/mind/sessions/{id}` — cognition sessions: model tier, canonical
  event, reasoning text (untrusted — render as data), evidence cited, raw→calibrated probability,
  decision-queue items. Backed by the D3/E3 reasoning-replay work.
- `GET /api/rota/v1/stream` — SSE channel (see §7).

## 7. Real-time & data flow

- TanStack Query pulls REST snapshots (cadences match today's, tunable per view).
- A single **SSE channel** (`/api/rota/v1/stream`) pushes typed delta events (new fill, belief
  committed, gate reject, halt state change, settlement) that invalidate or patch the relevant queries.
- One service layer owns the SSE connection; **graceful fallback to polling** if SSE is unavailable.
- SSE payloads carry only identifiers + minimal deltas; full records are fetched via the typed REST
  clients, keeping one source of truth per record.

## 8. Build / serve & constitution reconciliation

- The SPA is embedded into the `fortuna-ops` binary via **`rust-embed` behind a `spa` cargo feature**.
- **Default `cargo build` / `cargo test` need no Node** and serve the legacy ROTA — the money-path CI
  stays Node-free and deterministic. The constitution's no-build/determinism rules govern the core
  (single-threaded deterministic loop, `Clock`, DST); the SPA toolchain touches only the observability
  surface and is gated out of the default build.
- A dedicated **UI CI job** builds `ui/dist` (Node) then the binary with `--features spa`.
- The **deployable stays one Rust binary**, served by axum on the existing metrics listener; Node is a
  **dev/CI-only** dependency, never a runtime one.
- Dev convenience: a `ServeDir` path or the Vite dev server (proxying `/api` to the daemon) for hot
  reload; embedding is the release path.
- Legacy ROTA stays mounted (e.g. at `/rota`) until the SPA reaches parity; cutover/retirement is an
  explicit operator decision.

## 9. Security & untrusted-data handling

- **GET + SSE only**, localhost/Tailscale-bound, no auth — identical posture to today. No state-mutating
  endpoint exists; the route-table test that asserts 405 on POST/PUT/DELETE is extended to the SPA mount.
- **Self-host fonts** (Inter, Marcellus) under `assets/fonts/` — the box may be air-gapped/Tailscale-only
  and the repo bans external CDNs. Strict CSP; no external origins permitted.
- **Untrusted data (spec 5.11):** market titles, news, evidence payloads, and model reasoning are
  rendered as **data only** — React default escaping, never `dangerouslySetInnerHTML`. Enforced by an
  ESLint rule and a dedicated XSS test (a malicious market title renders inert).

## 10. Design system

Tokens lifted directly from the mock's CSS variables (`--bg #0A0A0B`, `--surface #141416`,
`--surface2 #1b1b1e`, `--gold #D4AF37`, `--amber #FFB84D`, `--text #EDEEEA`, `--halt #FF3B30`,
`--ok #30D158`, line/soft/muted/faint alphas) → Tailwind theme + shadcn theming. Typography: Marcellus
(display/wordmark), Inter (UI), tabular-nums for all numerics. The mock is the elevated ROTA aesthetic,
so this is codification, not invention. Halt state renders as a full red takeover, matching today.

## 11. Testing

- **Vitest + Testing-Library** — component/unit, all against fixtures.
- **Playwright** — E2E flows (navigate sections, open a chain, expand a belief) against fixtures mode.
- **Contract check** — `ts-rs` regeneration produces no diff; CI fails otherwise.
- **Untrusted-data XSS test** — malicious strings in titles/evidence/reasoning render inert.
- **Rust side** — the existing dashboard route tests are extended to cover the SPA mount and the new
  endpoints' GET-only / honest-degradation behavior.

## 12. Milestones (foundations-first)

1. **Foundations** — `ui/` scaffold; design system + tokens; app shell + routing/nav; API/contract layer
   (ts-rs + fixtures mode); SSE transport; axum embed + `spa` feature flag; UI CI job. Self-hosted fonts.
2. **Vertical slice** — Command Center + **The Loop** (chain-view) end-to-end, against real
   `/api/rota/v1/*` + stubbed Phase-C chain/mind contracts. Proves the whole stack.
3. **Iterate sections** — The Mind, Beliefs, Strategies, Gates & Risk, Execution, Sources & Discovery,
   Audit & System — each an independent PR with its own fixtures + tests.
4. **Parity + cutover** — when sections reach parity with the 24 panels, retire legacy ROTA (operator
   decision).

## 13. Out of scope

- Action-capable UI (constitution; see §3.2).
- Public / multi-tenant / authenticated access.
- Mobile-native.
- The Phase-C backend wiring itself — this design defines the contracts; the Phase C track implements the
  data behind the chain-view and mind-reasoning endpoints.

## 14. Risks / open questions

- **Chain-view contract depends on Phase C.** Mitigation: define the shape + fixtures now; the UI renders
  from fixtures until Phase C lands the endpoint. The contract doc is the agreement; revisit if Phase C's
  chain model differs from the assumed stage list.
- **Node in a pure-Rust repo.** Mitigation: feature-gated embed (§8) keeps default builds Node-free;
  Node is dev/CI-only. Document the UI build in the runbook.
- **Contract drift.** Mitigation: ts-rs generation + CI diff check (§6).
- **Mock placeholder semantics leaking in.** Mitigation: every number on screen must trace to a real
  FORTUNA contract field; no equities/Sharpe/VaR-style hedge-fund filler survives.
- **Parity scope creep.** Mitigation: the 24-panel inventory is the parity checklist; "better views"
  means re-organized + drill-down, not new data the backend can't supply.
