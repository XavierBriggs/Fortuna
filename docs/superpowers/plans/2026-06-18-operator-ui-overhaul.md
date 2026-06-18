# Operator UI Overhaul — Implementation Plan (v2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the flat 24-panel inline-Rust ROTA console with a navigable, drill-down, real-time React SPA (the approved mock), built foundations-first and parallel-safe to the Phase C track.

**Architecture:** A new top-level `ui/` Vite+React+TS SPA, **outside the Cargo workspace**, consuming the existing `/api/rota/v1/*` JSON via a typed contract layer with a fixtures mode (zero daemon/Phase-C dependency to develop). Polling-primary real-time (SSE client + fallback; SSE *server* deferred). Served by axum from `fortuna-ops` via `rust-embed` behind a `spa` cargo feature — one binary, Node dev/CI-only. The existing ROTA stays mounted until parity.

**Tech Stack:** Vite, React 18, TypeScript, Tailwind CSS, owned shadcn-style components, TanStack Query, TanStack Table, react-router; Vitest + Testing-Library, Playwright; Rust: **axum 0.8**, rust-embed.

**Spec:** `docs/superpowers/specs/2026-06-18-operator-ui-overhaul-design.md`

### v2 changelog (post adversarial-verify, 3 reviewers — verifier + Explore + senior critique)
- **Dropped ts-rs as the anti-drift mechanism** — there are NO typed Rust view structs (`crates/fortuna-live/src/views.rs:73-76` returns `serde_json::Value`; `rota.rs` has 77 `json!` sites). Contracts are **hand-authored TS, source of truth**, guarded by a **runtime contract-fidelity test** (fixtures validated against live `/api/rota/v1/*` JSON when a daemon is reachable). (was Task 18 → now Task 18 "Contract fidelity")
- **Fixtures captured from the live paper-demo daemon** (primary), not hand-shaped — the view builders have nullable/sentinel/degraded semantics a hand-author will get wrong. (Task 5)
- **Contract layer has 4 shape families**, not one `Board`: `GenericBoard` (20 endpoints), `SnapshotView` (health/money/telemetry), `CompositeView` (gates/settlement/cognition/perps), `SpecialView` (build/audit). (Task 5)
- **Chain-view is fixtures-only, no parity claim** — Phase C E3 currently renders it as an HTML panel (`rota.rs:2107`); the JSON contract `/api/rota/v1/chain` must be agreed with the Phase C track before it's wired live. (Task 10)
- **SPA fallback is GET-only**; a NEW additive test asserts 405 on POST to extensionless SPA paths — the existing `every_path_is_get_only_and_200` is protected-invariant lineage and must never be weakened. (Task 7)
- **Deterministic offline scaffold** — hand-author the scaffold files; never run `npm create vite .` with `.` resolving into a non-empty dir. (Task 1)
- **rust-embed**: commit `ui/dist/.gitkeep` + a stub `index.html` so `cargo build --features spa` compiles on a clean checkout; default `cargo build`/`test` never enables `spa`. (Task 7)
- **SSE polling-primary**; SSE *server* endpoint deferred (no owner today; crosses Phase-C `daemon.rs`). (Task 6)
- **Command Center composed client-side** from existing views (health+money+strategies+gates) — no invented aggregate endpoint. (Task 9)
- **Fonts via `@fontsource/inter` + `@fontsource/marcellus`** (OFL) copied to `ui/public/fonts/`. (Task 2)
- **Worktree uses an isolated `CARGO_TARGET_DIR`** so UI Rust builds never stomp C's `target/`. (Global Constraints)
- Tasks 16/17 split into smaller views; nav test scoped to the `<nav>` landmark.

## Global Constraints

- **Read-only + safe affordances** — the UI NEVER mutates trading state. GET + SSE only. No state-mutating endpoint exists. Operator affordances are client-side only (clipboard CLI strings, deep-links). Honors I2/I4/I6/I7. (spec §3.2)
- **Untrusted data (spec 5.11)** — market titles, news, evidence, model reasoning, signal summaries rendered as **data only**. NEVER `dangerouslySetInnerHTML`. Affordance `href`s must reject `javascript:`/non-`https` schemes. Enforced by ESLint `react/no-danger` + per-view inertness tests.
- **No external origins** — fonts self-hosted under `ui/public/fonts/`; strict CSP; no CDN.
- **Default `cargo build`/test stays Node-free** — the SPA embed is behind `--features spa`; default build serves legacy ROTA.
- **Isolated build dir** — the UI worktree sets `CARGO_TARGET_DIR=<worktree>/target-ui` (or builds are serialized) so it never corrupts C's `target/`. Never `pkill rustc`.
- **axum 0.8 idioms** — `{param}` path syntax, `Router::fallback_service`/method-restricted fallback, `axum::response::sse`. Copy the merge pattern at `crates/fortuna-ops/src/dashboard.rs:81`.
- **Money in cents, never f64 for money** — every money value traces to an integer `*_cents` contract field; `f64` only for cognition probabilities (`p`, `p_raw`). Format only at the display boundary.
- **Design tokens canonical** — `--bg #0A0A0B`, `--surface #141416`, `--surface2 #1b1b1e`, `--gold #D4AF37`, `--amber #FFB84D`, `--text #EDEEEA`, `--halt #FF3B30`, `--ok #30D158`; Marcellus (display), Inter (UI), tabular-nums on numerics. (spec §10)
- **Every on-screen number traces to a real contract field** — no hedge-fund placeholder semantics (equities/Sharpe/VaR) survive from the mock.
- **Daemon port** — proxy `/api` + `/metrics` → `http://127.0.0.1:9187` (verified `config/fortuna.example.toml:154`).
- **Conventional commits**, frequent. Co-author trailer per repo convention.

---

## File Structure

```
ui/                                  # NEW; outside Cargo workspace
  package.json, vite.config.ts, tsconfig.json, tsconfig.node.json, index.html
  .gitignore (node_modules; NOT dist), .eslintrc.cjs (react/no-danger:error)
  tailwind.config.ts, postcss.config.cjs, playwright.config.ts
  public/fonts/                      # Inter + Marcellus woff2 (from @fontsource)
  dist/.gitkeep                      # committed stub so --features spa compiles clean
  src/
    main.tsx
    app/{App.tsx,routes.tsx,nav.ts}
    design/{tokens.css,globals.css,cn.ts}
    design/components/{Card,Panel,Badge,StatPill,DataTable,NavButton,SectionHeader,Sparkline,Donut,QuantileFan,Bar,HaltOverlay,ChainStage}.tsx
    api/{client.ts,query.ts,sse.ts}
    api/contracts/{board.ts,snapshot.ts,composite.ts,special.ts,chain.ts,mind.ts,index.ts}
    api/fixtures/*.json              # captured from live daemon
    lib/{format.ts,affordances.ts}
    views/{CommandCenter,Loop,Mind,Beliefs,Strategies,GatesRisk,Execution,Sources,Discovery,Personas,Audit,SystemTelemetry}.tsx
    test/setup.ts
crates/fortuna-ops/
  Cargo.toml                         # + [features] spa = ["dep:rust-embed"]; optional rust-embed
  src/spa.rs                         # NEW (feature spa): embedded assets, GET-only index fallback
  src/rota.rs                        # MODIFY: mount spa under #[cfg(feature="spa")] (additive, GET-only)
  tests/spa.rs                       # NEW (feature spa): 405 on POST to SPA paths; index served
```

---

## Milestone 1 — Foundations

### Task 1: Scaffold the `ui/` SPA (deterministic, offline-safe)

**Files:** Create `ui/{package.json,vite.config.ts,tsconfig.json,tsconfig.node.json,index.html,.gitignore}`, `ui/src/{main.tsx}`, `ui/src/app/App.tsx`, `ui/dist/.gitkeep`, `ui/dist/index.html` (stub).

**Interfaces:** Produces a buildable Vite+React+TS app; `npm run build` emits `ui/dist/`.

- [ ] **Step 1:** Confirm `ui/` is empty, then **hand-author** the scaffold files (do NOT run `npm create vite .` in a non-empty dir). `package.json` with `type:module`, scripts (`dev`,`build`,`preview`,`test`,`test:e2e`,`lint`), deps: react, react-dom, react-router-dom, @tanstack/react-query, @tanstack/react-table; devDeps: vite, @vitejs/plugin-react, typescript, tailwindcss, postcss, autoprefixer, vitest, jsdom, @testing-library/react, @testing-library/jest-dom, @playwright/test, eslint, eslint-plugin-react, @fontsource/inter, @fontsource/marcellus.
- [ ] **Step 2:** `npm install` (network) — if it fails offline, report and stop; do not fabricate.
- [ ] **Step 3:** `vite.config.ts`: `base:'/'`, `build.outDir:'dist'`, `server.proxy['/api']` + `['/metrics']` → `http://127.0.0.1:9187`, Vitest (`environment:'jsdom'`, `setupFiles:'src/test/setup.ts'`). Commit `ui/dist/.gitkeep` + a minimal `dist/index.html` stub.
- [ ] **Step 4:** Smoke test:
```tsx
// src/app/App.test.tsx
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import App from './App';
test('renders shell wordmark', () => { render(<MemoryRouter><App/></MemoryRouter>); expect(screen.getByText(/ROTA/i)).toBeInTheDocument(); });
```
- [ ] **Step 5:** `npm run build && npx vitest run` → PASS. Commit `feat(ui): scaffold Vite+React+TS SPA (offline-safe)`.

### Task 2: Design tokens + Tailwind + self-hosted fonts

**Files:** Create `ui/src/design/{tokens.css,globals.css,cn.ts}`, `ui/tailwind.config.ts`, `ui/postcss.config.cjs`; copy `@fontsource/{inter,marcellus}` woff2 into `ui/public/fonts/`.

**Interfaces:** Tailwind theme exposes `bg/surface/surface2/gold/amber/text/halt/ok/line/soft/muted/faint`; `cn()` helper; fonts load from `/fonts/*.woff2`.

- [ ] **Step 1:** Failing test — import `tailwind.config.ts`, assert `theme.extend.colors.gold === '#D4AF37'` and `ok === '#30D158'` and `halt === '#FF3B30'`.
- [ ] **Step 2:** Author `tokens.css` (canonical CSS vars verbatim), `globals.css` (`@font-face` Inter/Marcellus → `/fonts/*.woff2`, base resets, scrollbar styling from the mock), `tailwind.config.ts` (palette → colors, `fontFamily.display:['Marcellus']`, `fontFamily.sans:['Inter',...]`), copy fonts from `node_modules/@fontsource/*/files/*.woff2` to `public/fonts/`.
- [ ] **Step 3:** Run config test → PASS; `npm run build` green.
- [ ] **Step 4:** Commit `feat(ui): design tokens, Tailwind theme, self-hosted fonts`.

### Task 3: Owned component primitives (shadcn-style)

**Files:** Create `ui/src/design/components/{Card,Panel,Badge,StatPill,DataTable,NavButton,SectionHeader,Sparkline,Donut,QuantileFan,Bar,HaltOverlay}.tsx` + co-located `*.test.tsx`.

**Interfaces:** `<Panel title>…</Panel>`; `<Badge tone="ok|amber|halt|gold|muted">`; `<StatPill label value tone?>`; `<DataTable columns rows>` (TanStack Table); `<Sparkline points/>`, `<Donut segments/>`, `<QuantileFan quantiles/>` (SVG); `<HaltOverlay reason/>`.

- [ ] **Step 1:** Failing tests per primitive (render label; tone→class; DataTable renders N rows; HaltOverlay shows reason).
- [ ] **Step 2:** Run → fail.
- [ ] **Step 3:** Implement with Tailwind tokens matching the mock (`border-line`, `rounded-[14px]`, surfaces, SVG paths mirror the mock). No Radix.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(ui): owned component primitives`.

### Task 4: App shell + routing + nav

**Files:** Create `ui/src/app/{App.tsx,routes.tsx,nav.ts}`; modify `ui/src/main.tsx`; extend `App.test.tsx`.

**Interfaces:** Consumes primitives. Produces sidebar (sections from `nav.ts`), header (system/risk/alerts/mode pills + clock), tick tape, `<Outlet/>`. Routes map each section to its view.

- [ ] **Step 1:** Failing test — scoped to the nav landmark:
```tsx
import { within } from '@testing-library/react';
test('nav lists every section', () => {
  render(<MemoryRouter><App/></MemoryRouter>);
  const nav = screen.getByRole('navigation');
  expect(within(nav).getAllByRole('link').length).toBeGreaterThanOrEqual(9);
});
```
- [ ] **Step 2:** Run → fail.
- [ ] **Step 3:** Implement shell per mock layout (grid `248px 1fr`, sticky sidebar, header pills, tick tape). `nav.ts` lists sections (label, icon, path). Views start as stub panels. Use react-router `<Link>` (role=link) inside a `<nav>`.
- [ ] **Step 4:** Run → PASS; `npm run build` green.
- [ ] **Step 5:** Commit `feat(ui): app shell, routing, section nav`.

### Task 5: Contract layer + fixtures (captured) + TanStack Query

**Files:** Create `ui/src/api/{client.ts,query.ts}`, `ui/src/api/contracts/{board.ts,snapshot.ts,composite.ts,special.ts,index.ts}`, `ui/src/api/fixtures/*.json`, `ui/src/lib/format.ts`; `ui/src/api/{client.test.ts,query.test.tsx}`.

**Interfaces:**
- `type GenericBoard = { title:string; generated_at:string; columns:{key:string;label?:string}[]; rows:Record<string,unknown>[]; summary?:Record<string,unknown> }` — for the 20 board endpoints (fills, strategies, working_orders, discovery, discovery_edges, personas, persona_scores, persona_pipeline, analyses, forecasts, forecast_feed, db, ingest_sources, ingest_feed, ingest_funnel, …).
- `snapshot.ts` — `health`, `money`, `telemetry` (daemon-shaped; model each from captured JSON).
- `composite.ts` — `gates`, `settlement`, `cognition`, `perps` (base + sub-surfaces).
- `special.ts` — `build` (`{generated_at,latest_gate_verdict}`), `audit` (`{available,rows,next_after,summary}` cursor).
- `client.get<T>(path)` — fixtures mode (`import.meta.env.VITE_MODE==='fixtures'`) → fixture JSON keyed by path; else fetch `/api/rota/v1/...`.
- `useView<T>(key,path)` TanStack Query hook; `format.cents/pct/bps/age`.

- [ ] **Step 1:** Failing tests — fixtures-mode `client.get('fills')` returns the captured board; `format.cents(12842)==='$128.42'`.
- [ ] **Step 2:** Run → fail.
- [ ] **Step 3:** **Capture fixtures from the live paper-demo daemon (primary):**
```bash
for v in health money gates cognition settlement streams build ingest_sources ingest_feed ingest_funnel fills strategies working_orders discovery discovery_edges personas persona_scores persona_pipeline analyses forecasts forecast_feed perps db telemetry; do
  curl -s "http://127.0.0.1:9187/api/rota/v1/$v" > "ui/src/api/fixtures/$v.json"; done
curl -s "http://127.0.0.1:9187/api/rota/v1/audit?limit=20" > ui/src/api/fixtures/audit.json
```
If no daemon is reachable, start the paper-demo (runbook) or fall back to hand-shaping from `crates/fortuna-ops/src/rota.rs` view builders for ONLY the endpoints needed by the current task — and note in the commit which fixtures are hand-shaped (to re-capture later).
- [ ] **Step 4:** Implement `client.ts`, `query.ts`, `format.ts`, the four contract families from the captured shapes. Run tests → PASS.
- [ ] **Step 5:** Commit `feat(ui): typed contract layer (4 families), captured fixtures, query hooks`.

### Task 6: SSE client + polling fallback (polling is the v1 transport)

**Files:** Create `ui/src/api/sse.ts`; `ui/src/api/sse.test.ts`.

**Interfaces:** `useLiveStream(onEvent)` — opens `EventSource('/api/rota/v1/stream')` IF present; on event `{kind:'fill'|'belief'|'gate_reject'|'halt'|'settlement', id:string}` invalidates matching query keys; **falls back to interval polling** on error/fixtures/absent-server. **The SSE server endpoint is deferred** — polling is the delivered v1 transport; do NOT claim real-time parity.

- [ ] **Step 1:** Failing test — mocked EventSource emits `{kind:'fill',id:'x'}` → handler invalidates `fills`; on error → switches to polling timer.
- [ ] **Step 2:** Run → fail.
- [ ] **Step 3:** Implement (single connection owner; kind→queryKeys; fallback timer; no-op gracefully if `/stream` 404s).
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(ui): SSE client with polling fallback (polling = v1 transport)`.

### Task 7: axum embed behind `spa` feature (GET-only fallback)

**Files:** Modify `crates/fortuna-ops/Cargo.toml` (`[features] spa = ["dep:rust-embed"]`, optional `rust-embed`); create `crates/fortuna-ops/src/spa.rs`; modify `crates/fortuna-ops/src/rota.rs` (mount under `#[cfg(feature="spa")]`); create `crates/fortuna-ops/tests/spa.rs`.

**Interfaces:** Under `--features spa`: `GET /` and extensionless GETs serve `index.html`; assets by path; `/api/*`, `/metrics`, `/rota` unchanged. **Fallback is GET-only → non-GET on any SPA path returns 405.**

- [ ] **Step 1:** Failing test (`tests/spa.rs`, feature-gated): `GET /` → 200 `text/html`; `GET /loop` → SPA index; **`POST /loop` and `POST /` → 405**; assert this ADDS coverage and does not touch `tests/rota.rs::every_path_is_get_only_and_200`.
- [ ] **Step 2:** Build SPA first (`cd ui && npm run build`), then `cargo test -p fortuna-ops --features spa` → fail (not implemented).
- [ ] **Step 3:** Implement `spa.rs` with `rust-embed` (`#[folder="$CARGO_MANIFEST_DIR/../../ui/dist"]`), mime-by-extension, **GET-only** index fallback (a `get(handler)` route / method-restricted `fallback_service` so non-GET → 405). Wire into `rota.rs` router additively under `#[cfg(feature="spa")]`. Keep `ui/dist/.gitkeep` + stub so a clean checkout compiles.
- [ ] **Step 4:** `cargo test -p fortuna-ops --features spa` PASS; **`cargo build -p fortuna-ops` (no feature) PASS with no Node/dist needed**; `cargo clippy -p fortuna-ops --features spa -- -D warnings` clean.
- [ ] **Step 5:** Commit `feat(ops): serve embedded SPA behind 'spa' feature, GET-only (default off)`.

### Task 8: Test harness + lint + CI job

**Files:** Create `ui/playwright.config.ts`, `ui/src/test/setup.ts`, `ui/.eslintrc.cjs`, CI workflow; modify `ui/package.json` scripts.

- [ ] **Step 1:** XSS guard test — fixture `title:"<img src=x onerror=alert(1)>"` renders literal text, no `<img>` injected.
- [ ] **Step 2:** `.eslintrc.cjs` with `plugin:react/recommended` + `'react/no-danger':'error'`; `npm run lint` PASS.
- [ ] **Step 3:** Playwright `webServer` runs `vite preview` w/ `VITE_MODE=fixtures`; one E2E: load `/`, navigate `/loop`.
- [ ] **Step 4:** CI workflow: `npm ci && npm run build` (emits `ui/dist`) **before** `cargo build -p fortuna-ops --features spa`; default `cargo test --workspace` does NOT enable `spa`. Run `npm run test && npm run test:e2e` → PASS.
- [ ] **Step 5:** Commit `chore(ui): test harness, lint (no-danger), CI job`.

---

## Milestone 2 — Vertical slice

### Task 9: Command Center view (composed client-side, no new endpoint)

**Files:** Create `ui/src/views/CommandCenter.tsx`, `ui/src/api/contracts/commandCenter.ts` (a *view-model* assembled from existing contracts, not a wire type); `ui/src/views/CommandCenter.test.tsx`.

**Interfaces:** Consumes `useView` for `health`, `money`, `strategies`, `gates` and composes a client-side view-model `{ system:{state,mode,halt_reason?}; paper_pnl_cents:{realized,floating,day_change_bps}; strategies:[{id,name,state,paper_pnl_cents}]; activity:[{kind,title,body,at}]; risk:[{label,used,limit,tone}]; alerts:number }`. No `/command_center` endpoint.

- [ ] **Step 1:** Failing test — renders paper PnL from cents; `system.state==='halt'` → `<HaltOverlay/>`; lists strategies + risk bars from fixtures; assert NO `$128`/`equities`/`Sharpe`/`VaR` strings.
- [ ] **Step 2:** Run → fail.
- [ ] **Step 3:** Implement, composing from the four existing views; mock's layout, real FORTUNA fields.
- [ ] **Step 4:** Run → PASS; Playwright `/` shows Command Center.
- [ ] **Step 5:** Commit `feat(ui): Command Center (composed from existing views)`.

### Task 10: The Loop — chain-view (fixtures-only; contract pending Phase C)

**Files:** Create `ui/src/api/contracts/chain.ts`, `ui/src/api/fixtures/chain.json`, `ui/src/views/Loop.tsx`, `ui/src/design/components/ChainStage.tsx`; `ui/src/views/Loop.test.tsx`.

**Interfaces:** Consumes `useView<Chain>('chain','/api/rota/v1/chain?market=<id>')` — **fixtures-only.** The JSON contract below is a PROPOSAL to the Phase C track (E3 currently renders the chain as HTML). Do NOT wire live or claim parity until C agrees the shape.
```ts
type ChainStage =
  | {kind:'signal'; source:string; at:string; summary:string}            // untrusted → data only
  | {kind:'belief'; producer:string; p:number; p_raw:number; horizon:string; evidence:string[]}
  | {kind:'edge'; side:string; edge_bps:number}
  | {kind:'proposal'; strategy:string; thesis:string; side_hint:string}  // I6: unsized
  | {kind:'gate'; verdict:'accept'|'reject'; checks:{name:string;pass:boolean;detail?:string}[]}
  | {kind:'fill'; venue:string; price_cents:number; qty:number; fee_cents:number}
  | {kind:'settlement'; resolved:string; realized_value_cents:number}
  | {kind:'pnl'; realized_pnl_cents:number};
type Chain = { market_id:string; market_title:string; safety:{execution_mode:string;order_mutation_enabled:boolean;book_fresh:boolean}; stages:ChainStage[] };
```

- [ ] **Step 1:** Failing test — renders ordered stage rail signal→…→pnl; gate stage shows per-check accept/reject; safety banner shows `execution_mode`; untrusted `summary`/`evidence` render inert (XSS).
- [ ] **Step 2:** Run → fail.
- [ ] **Step 3:** Implement `ChainStage` + `Loop.tsx` (market picker → chain rail + safety state).
- [ ] **Step 4:** Run → PASS; Playwright `/loop` shows the chain from fixtures.
- [ ] **Step 5:** Commit `feat(ui): The Loop chain-view (fixtures; contract pending Phase C)`.

---

## Milestone 3 — Iterate sections (independent; same pattern)

Each task: define/confirm contract (from the captured fixtures) → build the view from primitives → acceptance test (render fixture, key fields present, untrusted data inert, no placeholder semantics) → wire live query + SSE invalidation → commit. Re-ground every value to a real field. Split for right-sizing:

- **Task 11: The Mind** — `mind.ts`: sessions list + detail (`model_tier`, `canonical_event`, `reasoning` [untrusted], `evidence[]`, `p_raw`, `p`, `decision_queue[]`, `cost_budget{spent_cents,cap_cents}`, `model_tiers[]`, static `i6_boundary`). Fixtures-only (D3/E3-backed later). View: reasoning stream (plain text, no typing anim v1), evidence chips, raw→calibrated, I6 card, budget bar, decision queue. Per-field XSS inertness on `reasoning`/`evidence`.
- **Task 12: Beliefs** — feed (title, cat, p_raw→p, status), lifecycle counts, calibration scopes (n, brier, method), per-producer scorecards (Brier/CLV), Aeolus-vs-meteorologist head-to-head. Titles untrusted.
- **Task 13: Strategies** — grid (id,name,state,thesis,ladder,metrics) → detail (key metrics, I7 forward-validation gate: window n/30, CLV vs market, Brier vs base, verdict="awaiting operator"), recent beliefs, fills & orders.
- **Task 14: Gates & Risk** — total rejections, rejections-by-check table, rate-bucket utilization, halt/drawdown state, risk-limit bars (the safety surface).
- **Task 15: Execution** — fills table, working orders w/ status pills, positions, settlement (capital-in-limbo, overdue), per-strategy PnL, perps basis.
- **Task 16a: Sources** — sources health table, ingestion funnel, ingest feed.
- **Task 16b: Discovery & Personas** — discovery events/edges, personas registry + scorecard + pipeline, domain analyses browser (analyses text untrusted).
- **Task 17a: Audit** — append-only audit tail, cursor-polled (I5), via the `special` audit contract.
- **Task 17b: Telemetry & System** — telemetry board (TanStack Table), db table counts, recorder stream health, build/gate verdict badge.

---

## Milestone 4 — Hardening & parity

### Task 18: Contract-fidelity test (replaces ts-rs)
**Files:** `ui/src/api/contracts/fidelity.test.ts` (+ optional `crates/fortuna-ops/tests/contract_fidelity.rs`).
Validate that each captured fixture conforms to its hand-authored TS contract (shape assertion), AND, when `VITE_CONTRACT_LIVE=1` and a daemon is reachable, that the LIVE `/api/rota/v1/*` JSON still conforms (catches backend drift without needing typed Rust structs). CI runs the fixture-shape assertion always; the live check opportunistically. No ts-rs. Commit.

### Task 19: Affordances (client-side CLI + deep-links)
**Files:** `ui/src/lib/affordances.ts` + integration in Strategies (promotion), Command Center (halt/re-arm), Gates&Risk. Build `fortuna rearm/kill/promote` strings + doc/runbook deep-links from on-screen data; copy-to-clipboard; **reject non-`https` hrefs**. **No endpoint.** Test: generated string matches expected; copy never POSTs; `javascript:` href rejected. Commit.

### Task 20: Parity checklist + cutover (operator-gated)
Verify each of the 24 legacy panels has a home in the new IA (checklist vs spec §2.1/§4). When parity holds, propose retiring legacy `/rota` — **operator decision, not automatic.** Document in CHANGELOG + runbook.

---

## Self-Review

**1. Spec coverage:** §3→T1–6,9; §3.2→Constraints+T19; §4 IA→T4,9–17; §6 contracts/fixtures→T5,18; §6.1 new contracts→T9(CC composed),10(chain),11(mind); §7 real-time→T6 (polling-primary, honest); §8 embed→T7; §9 security/untrusted→Constraints+T7,8,10,19; §10→T2,3; §11→T1,8+per-view; §12→milestones; §13 out-of-scope respected. Covered.

**2. Placeholder scan:** Contracts, fixtures (captured), test code, and commands are concrete; visual JSX specified by contract + acceptance test (correct altitude). No TBD/TODO. ts-rs vapor removed.

**3. Type consistency:** `GenericBoard`/`SnapshotView`/`CompositeView`/`SpecialView`/`Chain`/`ChainStage`/`Mind` names stable; `client.get`/`useView`/`useLiveStream` signatures stable; integer cents throughout, formatted only via `format.cents`.

**Execution order:** T1→T8 sequential. T9, T10 = vertical slice (critical path). T11–T17b mutually independent (parallelizable across subagents). T18–T20 after contract shapes stabilize. The SPA SSE server and the live chain/mind/`command_center` endpoints are explicitly deferred/coordinated, NOT delivered by this plan — polling + fixtures cover them honestly.
```
