# Operator UI Overhaul — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat 24-panel inline-Rust ROTA console with a navigable, drill-down, real-time React SPA (the approved mock), built foundations-first and parallel-safe to the Phase C track.

**Architecture:** A new top-level `ui/` Vite+React+TS SPA, **outside the Cargo workspace**, consuming the existing `/api/rota/v1/*` JSON via a typed contract layer with a fixtures mode (zero daemon/Phase-C dependency to develop). Real-time via SSE with polling fallback. Served by axum from `fortuna-ops` via `rust-embed` behind a `spa` cargo feature — one binary, Node is dev/CI-only. The existing ROTA stays mounted until parity.

**Tech Stack:** Vite, React 18, TypeScript, Tailwind CSS, owned shadcn-style components, TanStack Query, TanStack Table, react-router, Vitest + Testing-Library, Playwright; Rust: axum, rust-embed, ts-rs (hardening).

**Spec:** `docs/superpowers/specs/2026-06-18-operator-ui-overhaul-design.md`

## Global Constraints

- **Read-only + safe affordances** — the UI NEVER mutates trading state. GET + SSE only. No state-mutating endpoint exists. Operator affordances are client-side only (clipboard CLI strings, deep-links). Honors I2/I4/I6/I7. (spec §3.2)
- **Untrusted data (spec 5.11)** — market titles, news, evidence, model reasoning rendered as **data only**. NEVER `dangerouslySetInnerHTML`. Enforced by ESLint rule + an XSS test.
- **No external origins** — self-host fonts (Inter, Marcellus) under `ui/public/fonts/`; strict CSP; no CDN. (spec §9)
- **Default `cargo build`/test stays Node-free** — the SPA embed is behind `--features spa`; default build serves legacy ROTA. (spec §8)
- **Money in cents, never f64 for money** — any value the UI labels as money traces to an integer-cents contract field; format at the display boundary only.
- **Design tokens are canonical** — `--bg #0A0A0B`, `--surface #141416`, `--surface2 #1b1b1e`, `--gold #D4AF37`, `--amber #FFB84D`, `--text #EDEEEA`, `--halt #FF3B30`, `--ok #30D158`; Marcellus (display), Inter (UI), tabular-nums on all numerics. (spec §10)
- **Every number on screen traces to a real contract field** — no hedge-fund placeholder semantics (no equities/Sharpe/VaR-style filler) survive from the mock.
- **Conventional commits**, frequent. Co-author trailer per repo convention.

---

## File Structure

```
ui/
  package.json, vite.config.ts, tsconfig.json, tsconfig.node.json
  index.html
  .eslintrc.cjs                      # incl. react/no-danger: error
  tailwind.config.ts, postcss.config.cjs
  playwright.config.ts
  public/fonts/                      # self-hosted Inter + Marcellus woff2
  src/
    main.tsx                         # React root, QueryClientProvider, router
    app/
      App.tsx                        # shell: sidebar + header + tick tape + <Outlet/>
      routes.tsx                     # route table → views
      nav.ts                         # nav item config (9 sections)
    design/
      tokens.css                     # CSS variables (canonical palette)
      globals.css                    # base + font-face
      cn.ts                          # className merge helper
      components/                    # owned primitives: Card, Panel, Badge, StatPill,
                                     #   DataTable, NavButton, SectionHeader, Sparkline,
                                     #   Donut, QuantileFan, Bar, HaltOverlay
    api/
      client.ts                      # fetch wrapper, base URL, mode switch
      query.ts                       # QueryClient + typed useBoard/useView hooks
      sse.ts                         # SSE transport hook + polling fallback
      contracts/                     # TS types for every /api/rota/v1 shape
        board.ts                     # generic Board {title,generated_at,columns,rows,summary}
        commandCenter.ts, chain.ts, mind.ts, beliefs.ts, strategies.ts, ...
      fixtures/                      # canned JSON per contract (fixtures mode)
    views/
      CommandCenter.tsx
      Loop.tsx                       # chain-view (hero)
      Mind.tsx, Beliefs.tsx, Strategies.tsx, GatesRisk.tsx,
      Execution.tsx, SourcesDiscovery.tsx, AuditSystem.tsx
    lib/
      format.ts                      # cents→$, pct, bps, age, tabular helpers
      affordances.ts                 # build CLI command strings + doc deep-links (client-side)
    test/
      setup.ts
crates/fortuna-ops/
  Cargo.toml                         # + [features] spa = ["dep:rust-embed"]; optional rust-embed
  src/spa.rs                         # NEW: embedded-asset handler, index.html fallback (feature spa)
  src/rota.rs                        # MODIFY: mount spa routes when feature enabled (additive)
```

---

## Milestone 1 — Foundations

### Task 1: Scaffold the `ui/` SPA

**Files:**
- Create: `ui/package.json`, `ui/vite.config.ts`, `ui/tsconfig.json`, `ui/tsconfig.node.json`, `ui/index.html`, `ui/src/main.tsx`, `ui/src/app/App.tsx`
- Create: `ui/.gitignore` (node_modules, dist)

**Interfaces:**
- Produces: a buildable Vite+React+TS app; `npm run build` emits `ui/dist/`.

- [ ] **Step 1:** Scaffold with the non-interactive Vite template, then pin deps.
```bash
cd ui 2>/dev/null || (mkdir -p /Users/xavierbriggs/fortuna-ui-PLACEHOLDER) # run from worktree root
npm create vite@latest . -- --template react-ts
npm install
npm install -D tailwindcss postcss autoprefixer @tanstack/react-query @tanstack/react-table react-router-dom
npm install -D vitest @testing-library/react @testing-library/jest-dom jsdom @playwright/test eslint-plugin-react
```
- [ ] **Step 2:** Set `vite.config.ts` `base: '/'`, `build.outDir: 'dist'`, `server.proxy['/api'] → http://127.0.0.1:9187`, `server.proxy['/metrics']`. Add Vitest config (`environment: 'jsdom'`, `setupFiles: src/test/setup.ts`).
- [ ] **Step 3:** Verify the baseline builds.
```bash
npm run build
```
Expected: build succeeds, `dist/index.html` exists.
- [ ] **Step 4:** Smoke test renders.
```ts
// src/app/App.test.tsx
import { render, screen } from '@testing-library/react';
import App from './App';
test('renders shell wordmark', () => { render(<App/>); expect(screen.getByText(/ROTA/i)).toBeInTheDocument(); });
```
- [ ] **Step 5:** Run + commit.
```bash
npx vitest run && git add ui && git commit -m "feat(ui): scaffold Vite+React+TS SPA"
```

### Task 2: Design tokens + Tailwind + self-hosted fonts

**Files:**
- Create: `ui/src/design/tokens.css`, `ui/src/design/globals.css`, `ui/tailwind.config.ts`, `ui/postcss.config.cjs`, `ui/src/design/cn.ts`
- Create: `ui/public/fonts/` (Inter, Marcellus woff2 — download once, commit)

**Interfaces:**
- Produces: Tailwind theme exposing `bg/surface/surface2/gold/amber/text/halt/ok/line/soft/muted/faint`; `cn()` helper.

- [ ] **Step 1: Write the failing test** — token presence.
```ts
// src/design/tokens.test.ts
import './tokens.css';
test('gold token defined', () => {
  const v = getComputedStyle(document.documentElement).getPropertyValue('--gold').trim();
  // jsdom won't compute CSS vars from imported css; instead assert tailwind config maps it:
});
```
(Prefer a tailwind.config unit assertion instead — import the config and assert `theme.extend.colors.gold === '#D4AF37'`.)
- [ ] **Step 2:** Author `tokens.css` with the canonical palette CSS variables (verbatim from Global Constraints) and `@font-face` for Inter/Marcellus pointing at `/fonts/*.woff2`.
- [ ] **Step 3:** `tailwind.config.ts` maps the palette to `colors`, sets `fontFamily.display: ['Marcellus']`, `fontFamily.sans: ['Inter',...]`, enables `tabular-nums` utility default on numerics.
- [ ] **Step 4:** Run config assertion test → PASS. Build still green (`npm run build`).
- [ ] **Step 5: Commit** `feat(ui): design tokens, Tailwind theme, self-hosted fonts`.

### Task 3: Owned component primitives (shadcn-style)

**Files:**
- Create: `ui/src/design/components/{Card,Panel,Badge,StatPill,DataTable,NavButton,SectionHeader,Sparkline,Donut,QuantileFan,Bar,HaltOverlay}.tsx`
- Test: co-located `*.test.tsx`

**Interfaces:**
- Produces: `<Panel title>…</Panel>`, `<Badge tone="ok|amber|halt|gold|muted">`, `<StatPill label value tone?>`, `<DataTable columns rows>` (TanStack Table), `<Sparkline points/>` (SVG), `<Donut segments/>`, `<QuantileFan quantiles/>`, `<HaltOverlay reason/>`.

- [ ] **Step 1: Write failing tests** per primitive (render label, apply tone class, DataTable renders N rows, HaltOverlay shows reason). Example:
```tsx
test('Badge applies halt tone', () => {
  render(<Badge tone="halt">HALTED</Badge>);
  expect(screen.getByText('HALTED').className).toMatch(/halt/);
});
```
- [ ] **Step 2: Run → fail** (components not defined).
- [ ] **Step 3:** Implement each primitive with Tailwind tokens, matching the mock's borders/radii/spacing (`border-line`, `rounded-[14px]`, surfaces). SVG primitives mirror the mock's hand-rolled paths. No Radix needed.
- [ ] **Step 4: Run tests → PASS.**
- [ ] **Step 5: Commit** `feat(ui): owned component primitives`.

### Task 4: App shell + routing + nav

**Files:**
- Create: `ui/src/app/{App.tsx,routes.tsx,nav.ts}`; Modify `ui/src/main.tsx`
- Test: `ui/src/app/App.test.tsx` (extend)

**Interfaces:**
- Consumes: primitives (Task 3).
- Produces: sidebar (9 sections from `nav.ts`), header (system/risk/alerts/mode pills + clock), tick tape, `<Outlet/>` content host; routes map each section to its view component.

- [ ] **Step 1: Write failing test** — nav renders 9 sections; clicking "The Loop" routes to `/loop`.
```tsx
test('nav has 9 sections and routes', async () => {
  render(<MemoryRouter><App/></MemoryRouter>);
  expect(screen.getAllByRole('link')).toHaveLength(9);
});
```
- [ ] **Step 2: Run → fail.**
- [ ] **Step 3:** Implement shell per mock layout (grid `248px 1fr`, sticky sidebar, header pills, tick tape). `nav.ts` lists the 9 sections (label, icon, path). Views start as stub panels.
- [ ] **Step 4: Run → PASS;** `npm run build` green.
- [ ] **Step 5: Commit** `feat(ui): app shell, routing, 9-section nav`.

### Task 5: Contract layer + fixtures + TanStack Query

**Files:**
- Create: `ui/src/api/{client.ts,query.ts}`, `ui/src/api/contracts/{board.ts,index.ts}`, `ui/src/api/fixtures/*.json`, `ui/src/lib/format.ts`
- Test: `ui/src/api/{client.test.ts,query.test.tsx}`

**Interfaces:**
- Produces:
  - `type Board = { title: string; generated_at: string; columns: {key:string;label?:string}[]; rows: Record<string,unknown>[]; summary?: Record<string,unknown> }`
  - `client.get<T>(path): Promise<T>` — honors `import.meta.env.VITE_MODE==='fixtures'` → returns fixture JSON; else fetches `/api/rota/v1/...`.
  - `useView<T>(key, path)` — TanStack Query hook with per-view `staleTime`.
  - `format.cents(n)`, `format.pct(n)`, `format.bps(n)`, `format.age(iso)`.

- [ ] **Step 1: Write failing tests** — fixtures-mode `client.get` returns the canned board; `useView` exposes `data`; `format.cents(12842)` → `"$128.42"`.
- [ ] **Step 2: Run → fail.**
- [ ] **Step 3:** Implement client (mode switch via a fixtures registry keyed by path), `query.ts` (QueryClient with sensible defaults), `format.ts`. Author fixture JSON for the existing boards (health, money, gates, fills, working_orders, strategies, ingest_sources, beliefs/cognition, telemetry, audit) shaped from the live `/api/rota/v1/*` responses (capture via the running daemon if available; otherwise hand-shape from `crates/fortuna-ops/src/rota.rs` view builders).
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** `feat(ui): typed contract layer, fixtures mode, query hooks`.

### Task 6: SSE transport + polling fallback

**Files:**
- Create: `ui/src/api/sse.ts`; Test: `ui/src/api/sse.test.ts`

**Interfaces:**
- Produces: `useLiveStream(onEvent)` — opens `EventSource('/api/rota/v1/stream')` in live mode; on event, invalidates the matching query keys; **falls back to interval polling** if `EventSource` errors or in fixtures mode. Event shape: `{ kind: 'fill'|'belief'|'gate_reject'|'halt'|'settlement', id: string }`.

- [ ] **Step 1: Write failing test** — given a mocked EventSource emitting `{kind:'fill',id:'x'}`, the registered handler invalidates the `fills` query key; on error it switches to polling.
- [ ] **Step 2: Run → fail.**
- [ ] **Step 3:** Implement `sse.ts` (single connection owner; map kind→queryKeys; fallback timer).
- [ ] **Step 4: Run → PASS.**
- [ ] **Step 5: Commit** `feat(ui): SSE live transport with polling fallback`.

### Task 7: axum embed behind `spa` cargo feature

**Files:**
- Modify: `crates/fortuna-ops/Cargo.toml` (add `[features] spa = ["dep:rust-embed"]`, optional `rust-embed`)
- Create: `crates/fortuna-ops/src/spa.rs` (feature-gated embedded-asset handler with `index.html` SPA fallback)
- Modify: `crates/fortuna-ops/src/rota.rs` (mount spa routes only under `#[cfg(feature="spa")]`; additive, GET-only)
- Test: `crates/fortuna-ops/tests/spa.rs` (feature-gated)

**Interfaces:**
- Consumes: `ui/dist` at compile time (`#[folder = "$CARGO_MANIFEST_DIR/../../ui/dist"]`).
- Produces: under `--features spa`, `GET /` serves the SPA, unknown non-API paths fall back to `index.html`; `/api/rota/v1/*`, `/metrics`, legacy `/rota` unchanged.

- [ ] **Step 1: Write failing test** (feature-gated): with the embed, `GET /` returns 200 `text/html`; `GET /loop` (no extension) returns the SPA index; a POST to any SPA path returns 405.
- [ ] **Step 2: Run → fail** (`cargo test -p fortuna-ops --features spa`). Note: requires `ui/dist` to exist — build it first (`cd ui && npm run build`).
- [ ] **Step 3:** Implement `spa.rs` (rust-embed handler, mime from extension, index fallback for extensionless GET), wire into `rota.rs` router additively under `#[cfg(feature="spa")]`. Default build path unchanged.
- [ ] **Step 4: Run tests** — `cargo test -p fortuna-ops --features spa` PASS; `cargo build -p fortuna-ops` (no feature) PASS and **needs no Node/dist**.
- [ ] **Step 5: Commit** `feat(ops): serve embedded SPA behind 'spa' feature (default off)`.

### Task 8: Test harness + lint + CI job

**Files:**
- Create: `ui/playwright.config.ts`, `ui/src/test/setup.ts`, `ui/.eslintrc.cjs`
- Create: `.github/workflows/ui.yml` (or repo CI equivalent) — build `ui/dist`, run vitest + playwright, then `cargo build -p fortuna-ops --features spa`
- Modify: `ui/package.json` scripts (`test`, `test:e2e`, `lint`, `build`)

**Interfaces:**
- Produces: `npm run lint` enforces `react/no-danger: error`; Playwright runs against `VITE_MODE=fixtures` preview.

- [ ] **Step 1:** Add the XSS guard test (Playwright or vitest): a fixture with `title: "<img src=x onerror=alert(1)>"` renders the literal text, no element injected.
- [ ] **Step 2:** Add `.eslintrc.cjs` with `plugin:react/recommended` + `'react/no-danger':'error'`; `npm run lint` → PASS.
- [ ] **Step 3:** Playwright config: `webServer` runs `vite preview` with `VITE_MODE=fixtures`; one E2E that loads `/` and navigates to `/loop`.
- [ ] **Step 4:** Run `npm run test && npm run test:e2e` → PASS.
- [ ] **Step 5: Commit** `chore(ui): test harness, lint (no-danger), CI job`.

---

## Milestone 2 — Vertical slice (proves the whole stack)

### Task 9: Command Center view

**Files:**
- Create: `ui/src/api/contracts/commandCenter.ts`, `ui/src/api/fixtures/command_center.json`, `ui/src/views/CommandCenter.tsx`
- Test: `ui/src/views/CommandCenter.test.tsx`

**Interfaces:**
- Consumes: `useView<CommandCenter>('command_center','/api/rota/v1/command_center')` (new aggregate endpoint; until backend exists, fixtures serve it).
- Produces (contract): `type CommandCenter = { system: {state:'ok'|'degraded'|'halt'; mode:string; halt_reason?:string}; paper_pnl_cents:{realized:number;floating:number;day_change_bps:number}; strategies: {id:string;name:string;state:string;paper_pnl_cents:number}[]; activity: {kind:string;title:string;body:string;at:string}[]; risk:{label:string;used:number;limit:number;tone:string}[]; alerts:number }`.

- [ ] **Step 1: Write failing test** — renders paper PnL formatted from cents, halt state drives `<HaltOverlay/>` when `system.state==='halt'`, lists strategies + activity from fixture; asserts **no** hardcoded "$128M / equities" strings exist.
- [ ] **Step 2: Run → fail.**
- [ ] **Step 3:** Implement the view from primitives, re-grounded to FORTUNA semantics (paper PnL cents, strategies-at-a-glance, activity feed, risk-limit bars, alerts). Mock's layout, real fields.
- [ ] **Step 4: Run → PASS;** Playwright: `/` shows Command Center.
- [ ] **Step 5: Commit** `feat(ui): Command Center view`.

### Task 10: The Loop — chain-view (hero)

**Files:**
- Create: `ui/src/api/contracts/chain.ts`, `ui/src/api/fixtures/chain.json`, `ui/src/views/Loop.tsx`, `ui/src/design/components/ChainStage.tsx`
- Test: `ui/src/views/Loop.test.tsx`

**Interfaces:**
- Consumes: `useView<Chain>('chain','/api/rota/v1/chain?market=<id>')` — **backed by the Phase C chain-view work (spec §6.1); fixtures serve it until then.**
- Produces (contract — coordinate with Phase C track):
```ts
type ChainStage =
  | {kind:'signal'; source:string; at:string; summary:string}            // untrusted summary → data only
  | {kind:'belief'; producer:string; p:number; p_raw:number; horizon:string; evidence:string[]}
  | {kind:'edge'; side:string; edge_bps:number}
  | {kind:'proposal'; strategy:string; thesis:string; side_hint:string}  // I6: unsized
  | {kind:'gate'; verdict:'accept'|'reject'; checks:{name:string;pass:boolean;detail?:string}[]}
  | {kind:'fill'; venue:string; price_cents:number; qty:number; fee_cents:number}
  | {kind:'settlement'; resolved:string; realized_value_cents:number}
  | {kind:'pnl'; realized_pnl_cents:number};
type Chain = { market_id:string; market_title:string; safety:{execution_mode:string;order_mutation_enabled:boolean;book_fresh:boolean}; stages: ChainStage[] };
```

- [ ] **Step 1: Write failing test** — renders the ordered stage rail signal→…→pnl; gate stage shows per-check accept/reject; safety banner shows `execution_mode`; untrusted `summary`/`evidence` render as inert text (XSS guard).
- [ ] **Step 2: Run → fail.**
- [ ] **Step 3:** Implement `ChainStage` + `Loop.tsx` (a market picker → the chain rail + safety state). Each stage a card; gate expands per-check; PnL terminal node.
- [ ] **Step 4: Run → PASS;** Playwright: navigate `/loop`, open a market, see the full chain.
- [ ] **Step 5: Commit** `feat(ui): The Loop chain-view (hero)`.

---

## Milestone 3 — Iterate sections (each independent; same pattern)

Each task below: define contract + fixture (shaped from `crates/fortuna-ops/src/rota.rs` view builders), build the view from primitives, write the acceptance test (render fixture, key fields present, untrusted data inert, no placeholder semantics), wire live query + SSE invalidation, commit. Re-ground every value to a real contract field.

### Task 11: The Mind
Contract `mind.ts`: sessions list + detail (`model_tier`, `canonical_event`, `reasoning` [untrusted], `evidence[]`, `p_raw`, `p`, `decision_queue[]`, `cost_budget{spent_cents,cap_cents}`, `model_tiers[]`, `i6_boundary` static). Backed by D3/E3 reasoning-replay (spec §6.1); fixtures until then. View: reasoning stream (no typing animation needed for v1; render text), evidence chips, raw→calibrated, I6 boundary card, cost budget bar, decision queue. Acceptance test + commit.

### Task 12: Beliefs
Contract from existing `beliefs`/`forecasts`/`forecast_feed`/calibration: feed (title, cat, p_raw→p, status), lifecycle counts, calibration scopes (n, brier, method), per-producer scorecards (Brier/CLV), the Aeolus-vs-meteorologist head-to-head (two producers on the same event). Acceptance test + commit.

### Task 13: Strategies
Grid (id, name, state, thesis, promotion ladder, metrics) → detail (key metrics, I7 forward-validation gate: calibration window n/30, CLV vs market, Brier vs base, promotion verdict = "awaiting operator"), recent beliefs, fills & orders. Acceptance test + commit.

### Task 14: Gates & Risk
From `gates`: total rejections, rejections-by-check table, rate-bucket utilization, halt/drawdown state, risk-limit bars. The safety surface. Acceptance test + commit.

### Task 15: Execution
From `fills`/`working_orders`/`strategies`/settlement/`perps`: fills table, working orders with status pills, positions, settlement (capital-in-limbo, overdue), per-strategy PnL, perps basis. Acceptance test + commit.

### Task 16: Sources & Discovery
From `ingest_sources`/`ingest_funnel`/`ingest_feed`/`discovery`/`personas`/`analyses`: sources health table, ingestion funnel, discovery events/edges, personas registry + scorecard + pipeline, domain analyses browser. Acceptance test + commit.

### Task 17: Audit & System
From `audit`/`telemetry`/`db`/`streams`/`build`: append-only audit tail (cursor-polled, I5), telemetry board (TanStack Table), db table counts, recorder stream health, build/gate verdict badge. Acceptance test + commit.

---

## Milestone 4 — Hardening & parity

### Task 18: ts-rs contract generation + drift check
**Files:** Modify the Rust view structs (`crates/fortuna-live/src/views.rs` + new chain/mind structs) to `#[derive(ts_rs::TS)]`; add a `cargo test` that generates TS into `ui/src/api/contracts/generated/`; CI fails on diff. Replace hand-authored contract types with the generated ones where they correspond. Acceptance: regeneration produces no diff; UI build green against generated types. Commit.

### Task 19: Affordances (client-side CLI + deep-links)
**Files:** `ui/src/lib/affordances.ts` + integration in Strategies (promotion), Command Center (halt/re-arm), Gates&Risk. Build `fortuna rearm/kill/promote` command strings + doc/runbook deep-links from on-screen data; copy-to-clipboard. **No endpoint.** Test: generated string matches expected; clicking copies, never POSTs. Commit.

### Task 20: Parity checklist + cutover decision (operator-gated)
Verify each of the 24 legacy panels has a home in the new IA (checklist in spec §2.1 + §4). When parity holds, propose retiring legacy `/rota` — **operator decision, not automatic.** Document in CHANGELOG + runbook.

---

## Self-Review

**1. Spec coverage:** §3 stack→Tasks 1–6; §3.2 posture→Global Constraints + Task 19; §4 IA→Tasks 4,9–17; §6 contracts/fixtures→Task 5; §6.1 new contracts→Tasks 9,10,11; §7 real-time→Task 6; §8 embed→Task 7; §9 security/untrusted→Global Constraints + Tasks 8,10; §10 design system→Tasks 2,3; §11 testing→Tasks 1,8 + every view; §12 milestones→the four milestones. Covered.

**2. Placeholder scan:** Visual JSX is specified by contract + acceptance test rather than transcribed line-for-line (correct altitude for a design-system-driven multi-view build); all contracts, fixtures, test code, and commands are concrete. No TBD/TODO.

**3. Type consistency:** `Board`, `CommandCenter`, `Chain`/`ChainStage`, `Mind` names are used consistently; `useView`/`client.get`/`useLiveStream` signatures stable across tasks; cents fields integer throughout, formatted only via `format.cents`.

**Note on execution order:** Tasks 1→8 are sequential (each builds on the prior). Tasks 11–17 are mutually independent (parallelizable across subagents) once Tasks 1–10 land. Task 18 should follow once contract shapes stabilize.
