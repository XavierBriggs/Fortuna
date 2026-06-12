# ROTA v2 — FORTUNA operator dashboard

Design v2.1. 2026-06-11. Authoritative over the v1 draft. Adversarial critique
COMPLETE and folded — the AMENDMENTS section below is BINDING and overrides
the body where they conflict. Operator aesthetic tokens (Section 2) binding.
Implementer runs the Section 10 checklist PLUS the amendment checks as
iteration 0 BEFORE building; independent gate verifies.

## AMENDMENTS (adversarial critique, 2026-06-11 — binding, override the body)

R1. CAPABILITY-OPTION COMPOSITION (was Blocker 1). T4.1/fortuna-live is a
    non-compiling stub tonight; ROTA must NOT be serialized behind it.
    RotaState becomes: `pool: Option<PgPool>`, `deadman_ping_age:
    Option<Arc<AtomicI64>>`; ONLY the snapshot arc is mandatory. Every
    Postgres-derived field is nullable in the contracts; an absent capability
    renders a per-panel "unavailable/degraded" state (HTTP 200, never 500).
    Tests compose ROTA off SimRunner exactly like the existing chain test
    (observability.rs:167-265) — no daemon needed. T4.1 wires ROTA in later
    (~20 lines), not the reverse. T4.3 IS BUILDABLE STANDALONE, TONIGHT.
R2. DATA PLANE (was Blocker 2). fortuna-ops CANNOT depend on fortuna-runner
    (runner dev-depends on ops; reverse edge = cycle) and handlers MUST NOT
    parse Prometheus text. Third declared change: DashboardSnapshot gains a
    structured `views: serde_json::Value` (per-view pre-shaped JSON) populated
    by the composition/test harness from metrics_export() + boards_json().
    Handlers read views + their own ledger queries; nothing else.
R3. SSE IS CUT from v1 (was High 5). The audit tail is a CURSOR-POLLED JSON
    endpoint: GET /api/rota/v1/audit?after=<audit_id>&limit=100 — LOSSLESS
    (drop-oldest-256 broadcast would silently lose most of a burst; lossy is
    off-brand for an I5 audit surface), zero T4.1 hooks, survives restarts,
    backed by existing indexes idx_audit_kind(kind,at)/idx_audit_at
    (migrations 20260609000001:114-115). Shell polls it at 2s. /stream stays
    RESERVED in the namespace. Delete T-4; add a cursor-pagination test.
R4. RATE-BUCKET GAUGE IS CUT (was High 3). Bucket state is pub(crate) inside
    fortuna-gates with zero public read surface; exporting it means widening
    the I1/I3-bearing crate the same night — not for a nice-to-know gauge.
    The gates panel keeps rejections_by_check + halt state (the I3 signal
    already surfaces there). rate_buckets field dropped from the contract.
R5. DEDICATED ROTA PG POOL (was High 4): max_connections(2), short
    acquire_timeout, statement_timeout on every view query; timeout renders
    the panel degraded. NEVER share the daemon's pool — audit-append failure
    is a GLOBAL HALT ("no audit, no trading"); dashboard load must be unable
    to queue against the audit writer. Two pools is the conservative option.
R6. CONTRACT FIXES: health exposes p90/p95/p99 (NO p50 — none is exported;
    do not add one). Money identity defined: total = settled + floating;
    committed is informational (subset of settled); floating comes from the
    mark loop, and the account block is SIM-ONLY until a live venue exposes
    balances — labeled as such in the shell. Per-venue error arrays flattened
    to the single global counter that actually exists. All times rendered
    labeled UTC. Empty DB / no run yet => zeros/nulls, never 500.
R7. COGNITION PANEL: the body's "every source exists" is FALSE here — two
    NEW ledger queries are required and are hereby owned by T4.3: (a)
    BeliefsRepo::recent(limit) listing (ORDER BY belief_id DESC), (b) a
    calibration scope enumeration (DISTINCT scopes + max version). Both with
    tests + sqlx prepare. The operator's belief-reasoning display (evidence +
    provenance JSONB) stays — it rides query (a).
R8. LOCK RULE (one sentence, binding): clone needed data out of the snapshot
    RwLock and RELEASE before any await; no handler holds the snapshot lock
    across a query. (Existing handlers comply; keep it that way.)
R9. BUILD ORDER (minimum viable first slice): rota module + Option-capability
    state + structured views; FIVE surfaces first — Health (dead-man shows
    "unavailable" until T4.1), Money, Gates, Settlement, Streams (filesystem-
    only) — plus the polled audit tail and the gold/black shell with halt
    takeover; tests T-1, T-2, T-6, empty-DB, Pg-down, cursor pagination.
    Then Cognition once R7's queries land. T-3 replay stays feature-gated;
    T-5 perf if time allows.
R10. Known citation drift in the body (line numbers off by a few in
    dashboard.rs/Cargo.toml/recorder/deadman cites) — conclusions verified
    unaffected; trust the amendment section and the checklist, re-verify
    exact lines during fit-validation.
R11. QUALITY FIRST; RESPONSIVE FOR FREE (operator clarification,
    2026-06-11): the acceptance bar is the DESKTOP instrument console —
    its quality is never compromised for any other screen size. Because
    the stack is plain CSS, basic reflow costs nothing: use
    `grid-template-columns: repeat(auto-fit, minmax(320px, 1fr))` on the
    panel grid (one declaration, no JS, desktop rendering unchanged).
    That single free line is the ENTIRE responsive scope for v1; any
    responsive work beyond it — and all dedicated mobile UX — is deferred
    (Section 12) and must never drive a design decision in v1.
R12. BROWSER/VISUAL VALIDATION IS A VERIFICATION-LAYER CONCERN — NOT a repo
    toolchain change. NO Playwright/Node/jest enters this repo (that would
    reverse the Section 1 stack decision). The independent verification
    gate for T4.3 includes a live browser pass driven from the verifier
    session's own tooling (Playwright / Chrome DevTools MCP): boot a seeded
    Sim run serving ROTA; screenshot every panel at desktop 1440px;
    simulate a halt and assert the red takeover renders; assert zero
    browser console errors. Those are the PASS/FAIL quality assertions —
    all desktop. A 390x844 screenshot is also captured as INFORMATIONAL
    ONLY (never pass/fail in v1). Screenshots archive under
    docs/reviews/rota-visual/ and deliver with the gate verdict. The
    implementer's only job for this is R11's one line; the gate does the
    rest.

## 0. Study findings (codebase evidence; every claim cites file:line)

### 0.1 Existing dashboard

`crates/fortuna-ops/src/dashboard.rs:1-109` is a live, GET-only axum dashboard
proven by the T1.5 chain test. It serves:
- GET /           -> inline HTML shell, vanilla JS fetch + setInterval (lines 52, 71-109)
- GET /metrics    -> Prometheus text exposition (lines 34-41)
- GET /api/boards -> JSON boards (lines 43-50)

State: `Arc<RwLock<DashboardSnapshot>>` (line 32). `serve_dashboard(listener,
state)` is the entry point (lines 58-68). The read-only test pattern is at
`tests/dashboard.rs:74-80` (POST 405 for every path). The chain test at
`crates/fortuna-runner/tests/observability.rs:168-265` runs a full sim ->
metrics -> HTTP -> asserts the same numbers end-to-end.

ROTA EXTENDS this; it does not replace it. Existing routes stay; ROTA adds
routes under `/api/rota/v1/` in the same axum Router.

### 0.2 Stack

`Cargo.toml:1-45`: pure Rust workspace (14 crates). Zero Node/npm/Vite anywhere
in the tree. Workspace deps already include axum 0.8 (line 36), tokio "full"
(line 22), serde_json (line 24), sqlx 0.8 postgres (line 33). No frontend build
step exists or is needed.

### 0.3 T4.1 composition

BUILD_PLAN Phase 4 T4.1: the fortuna-live binary owns Postgres repos,
AuditWriter, the real-time tick loop, and the metrics endpoint. ROTA's server
composes with T4.1 in the same axum Router, same process, same PgPool. The
DashboardSnapshot arc is the state bridge: T4.1 populates each tick; ROTA serves.

### 0.4 Live-update mechanics

The existing shell short-polls (`setInterval(refresh, 5000)`, dashboard.rs:103)
— that is the codebase grain. ROTA short-polls all snapshot views (health 2s,
money/gates 5s, cognition/settlement 10s, streams 15s) and uses SSE for the
audit tail ONLY (genuinely a stream). SSE = tokio broadcast Sender + axum
handler; no websocket, no gRPC.

## 1. Stack decision

**Server-rendered Rust (inline HTML const + vanilla JS) + short-poll JSON APIs
+ one SSE endpoint for the audit tail. Zero new toolchain; zero build step.**

Rationale vs SPA: no Node toolchain exists in the repo; the working T1.5
dashboard is already exactly this pattern; overnight buildability requires
staying in Rust; tests stay in cargo; the no-bloat mandate is absolute.
Platform seam: the `/api/rota/v1/` prefix is the only structural concession to
a future SPA/hosted version — when wanted, the API contract is frozen and the
frontend is a static-file swap. "Platform later" does not justify SPA
complexity now.

## 2. Identity and aesthetic (operator tokens — binding)

- Background `#0A0A0B`; surface/card `#141416`; gold primary `#D4AF37`; amber
  accent `#FFB84D`; text `#EDEDEA`; halt/breach `#FF3B30` (reserved
  EXCLUSIVELY — no decorative red); settled/ok `#30D158` (sparingly).
- Numbers: JetBrains Mono / ui-monospace (tabular lining figures). Text:
  system-ui. Flat gold-line-on-black instrument aesthetic; no gradients.
- Logo: SVG wheel (8-spoked) + cornucopia form, gold on transparent, at
  `assets/rota/logo.svg` (geometry spec in Section 9); FORTUNA wordmark,
  "ROTA" sub-label; favicon derived from the wheel. No external CDN resources
  — localhost/Tailscale only.

## 3. Architecture

```
fortuna-live (T4.1)
  +- Arc<PgPool>                       (fortuna-ledger::connect(); shared)
  +- Arc<RwLock<DashboardSnapshot>>    (metrics text + boards; existing T1.5)
  +- Arc<SseHub>                       (audit-tail broadcast; new)
  +- Arc<PathBuf>                      (perishable_dir = data/perishable)
  |
  axum Router (single, composed at startup)
    GET /                        -> ROTA shell (const ROTA_SHELL)
    GET /metrics                 -> existing
    GET /api/boards              -> existing
    GET /api/rota/v1/health      |
    GET /api/rota/v1/money       |
    GET /api/rota/v1/gates       |  six snapshot views
    GET /api/rota/v1/cognition   |
    GET /api/rota/v1/settlement  |
    GET /api/rota/v1/streams     |
    GET /api/rota/v1/stream      -> SSE audit tail
    GET /assets/rota/*           -> static assets
```

All routes GET-only by construction; the route-table test (pattern
tests/dashboard.rs:74-80) asserts 405 on every mutating method for every path.

```rust
pub struct RotaState {
    pub pool: Arc<PgPool>,
    pub snapshot: Arc<RwLock<DashboardSnapshot>>,
    pub sse_hub: Arc<SseHub>,
    pub perishable_dir: Arc<PathBuf>,
    pub daily_budget_cents: i64,      // FortunaConfig::cognition (config.rs:93)
    pub per_cycle_budget_cents: i64,
}
```

SseHub: `tokio::sync::broadcast::Sender<AuditTailEvent>`, capacity 256. T4.1
calls `hub.publish(row)` after every successful `AuditWriter::append`. Slow
clients get Lagged (drop-oldest); the tick loop is never blocked.

## 4. Panel / data inventory (v1 = seven panels; every source exists today)

| Panel | Source | File:line | Transport |
|---|---|---|---|
| Health/Wheel | halts, ticks, fill/ack latency quantiles, HaltsRepo::active | runner.rs:2209-2215, :228-229, :1995-2016; repos.rs:147-173 | poll /health 2s |
| Money | boards_json positions; metrics pnl/fees/reserved/utilization; NEW account fields from inspect_totals | runner.rs:2220-2250, :2139-2180, :1132 | poll /money 5s |
| Gates | rejections_by_check; recent("gate_decision"); NEW fortuna_rate_bucket_fill gauge | runner.rs:2182-2190; audit.rs:74-93; runner.rs:1911 | poll /gates 5s |
| Cognition | spend/breach/failure/shadow counters; CalibrationParamsRepo::latest; recent beliefs; CognitionConfig budgets | runner.rs:1939-2080; repos.rs:1220-1251, :947-967; config.rs:89-99 | poll /cognition 10s |
| Settlement/Watchdogs | capital_in_limbo, overdue; DiscrepanciesRepo::open_count; voids/reversals; recent("watchdog") | runner.rs:2196-2215, :252-256; repos.rs:403-415; audit.rs:74 | poll /settlement 10s |
| Venue/Streams | venue_api_errors; book ages (NEW boards field); perishable JSONL mtime+counts | runner.rs:2084-2090; recorder/lib.rs:83-106; recorder/main.rs:34-43 | poll /streams 15s |
| Audit tail | AuditWriter append -> SseHub broadcast | audit.rs:47-72 | SSE /stream |

DEFERRED (source incomplete today): triage recall/precision (shadow scoring
cross-join unwritten); market-discovery view (Tradability/Edges join
unwritten); perps/funding-regime panel (T5.B8 owns it); DST/gate-verdict badge
(no review-file parser). WS gap/resync counters render stub 0 until T4.2 ships
the dial.

Telemetry additions (the ONLY runner changes, both read-path): the
`fortuna_rate_bucket_fill{venue,market}` gauge in metrics_export(), and
account-view + book-age fields in boards_json(). Zero money-path changes.

## 5. JSON view contracts (snapshot-tested)

### /api/rota/v1/health
```json
{ "generated_at": "...", "stage": "sim", "halt_active": false,
  "halt_reason": null, "ticks_total": 42, "last_tick_age_ms": 1230,
  "fill_latency_p50_ms": 5, "fill_latency_p90_ms": 14, "fill_latency_p99_ms": 48,
  "dead_man_last_ping_age_secs": 45,
  "venues": [ { "id": "sim", "healthy": true, "api_error_count": 0 } ] }
```
last_tick_age_ms = now - DashboardSnapshot.generated_at (loop-deadness proxy);
halt_reason from HaltsRepo::active (null when none).

DEAD-MAN AGE — CONTRACT RECONCILED 2026-06-11 (remediation2 gate note 6):
the daemon's dead-man pinger landed as an INDEPENDENT spawned task with a
closure-owned `DeadmanPinger` (daemon::deadman_tick) — there is no
`last_ping_at: Arc<AtomicI64>` seam, and adding one now would reach into
the running task. ROTA v1 therefore reports `dead_man_last_ping_age_secs:
null` (capability absent — the health panel shows "dead-man: external"
and defers to the external monitor's own page). If a live age is wanted
later, the pinger gains a shared `Arc<AtomicI64>` last-ping stamp the
task writes and RotaState reads — a small follow-up, not a v1 blocker.

### /api/rota/v1/money
```json
{ "generated_at": "...", "settled_cents": 1000000, "committed_cents": 45000,
  "floating_cents": 12000, "total_cents": 1012000,
  "strategies": [ { "strategy": "mech_structural", "realized_pnl_cents": 4200,
    "fees_paid_cents": 380, "reserved_exposure_cents": 45000,
    "envelope_utilization_bps": 1500 } ],
  "positions": [ { "market": "...", "yes_qty": 10, "no_qty": 0,
    "realized_pnl_cents": 400, "fees_cents": 35, "lifecycle": "Open" } ] }
```
Account fields = NEW boards_json "account" key populated each tick from
SimVenue::inspect_totals() (already called runner.rs:1132).

### /api/rota/v1/gates
```json
{ "generated_at": "...", "total_rejections": 7,
  "rejections_by_check": [ { "check": "EdgeFloor", "number": 6, "count": 5 } ],
  "rate_buckets": [ { "venue": "kalshi", "market": null, "fill_pct": 72 } ],
  "recent_rejections": [ { "audit_id": "01HX...", "at": "...",
    "check": "EdgeFloor", "reason": "net edge 42bps < floor 100bps",
    "intent_ref": "01HX..." } ] }
```

### /api/rota/v1/cognition
```json
{ "generated_at": "...", "mind_spend_today_cents": 1240,
  "daily_budget_cents": 50000, "per_cycle_budget_cents": 500, "spend_pct": 2,
  "cognition_failures_total": 3, "budget_breaches_total": 0,
  "shadow_cycles_total": 12, "beliefs_drafted_total": 8,
  "calibration_scopes": [ { "model_id": "...", "strategy": "synth_events",
    "category": "weather", "kind": "platt", "version": 3,
    "effective_at": "..." } ],
  "recent_beliefs": [ { "belief_id": "01HX...", "event_id": "01HX...",
    "p": 0.67, "p_raw": 0.71, "status": "open", "brier": null,
    "clv_bps": null,
    "evidence": { "...": "model's structured reasoning, beliefs.evidence JSONB" },
    "provenance": { "model_id": "...", "manifest_hash": "...",
      "cost_cents": 12 } } ] }
```

Operator amendment (2026-06-11): recent_beliefs includes each belief's
persisted `evidence` and `provenance` JSONB (migrations
20260609000001_initial.sql:66-67) — the model's stated reasoning surfaces in
the cognition panel as click-to-expand rows. Server-side truncate evidence at
4KB per row for the panel payload. NO new storage: raw LLM responses /
extended thinking are deliberately not persisted by the cognition layer;
displaying them would require a cognition-layer change and is OUT OF SCOPE
for ROTA v1 (deferred note in Section 12).

### /api/rota/v1/settlement
```json
{ "generated_at": "...", "capital_in_limbo_cents": 5000,
  "settlements_overdue": 0, "discrepancies_open": 0,
  "settlement_voids_total": 1, "settlement_reversals_total": 0,
  "recent_watchdog_events": [ { "audit_id": "01HX...", "at": "...",
    "kind": "settlement_overdue", "market_ref": "..." } ] }
```

### /api/rota/v1/streams
```json
{ "generated_at": "...", "venue_api_errors_total": 2,
  "venues": [ { "id": "sim", "book_age_ms": 850, "ws_gap_count": 0,
    "resync_count": 0 } ],
  "recorder": [ { "stream": "perp_orderbook", "key_count": 11,
    "last_capture_age_secs": 22, "rows_today": 140, "healthy": true } ] }
```
recorder section = filesystem stat of data/perishable/<today>/<stream>.jsonl;
healthy = last_capture_age_secs < 120 (two missed 30s cycles).

### /api/rota/v1/stream (SSE)
`event: audit` with the AuditRow JSON per append; `event: heartbeat` every 5s;
`?kind=` filter; EventSource auto-reconnect; drop-oldest at capacity 256.

## 6. Crate composition

New module in fortuna-ops: `src/rota/{mod,views,sse,recorder,routes}.rs`.
Modified: dashboard.rs (serve_dashboard gains RotaState, merges rota_router);
runner.rs (rate-bucket gauge + boards account/book-age fields);
fortuna-ops/Cargo.toml (+ fortuna-ledger, after the cycle check V-5);
assets/rota/ (logo.svg, shell.html — inlined as const like INSTRUMENT_SHELL).
Same Arc<PgPool> as T4.1; no second Postgres connection.

## 7. Cut from the v1 draft, and why

| Item | Disposition | Reason |
|---|---|---|
| Vite+React+TS SPA, build-dash.sh | CUT | no Node toolchain in repo; Rust-only overnight; API is the seam anyway |
| SSE for all metric deltas | SCOPED to audit tail | short-poll is the codebase grain, adequate at 2-15s, cargo-testable |
| Gate-verdict badge from docs/reviews/ | CUT v1 | no review-file parser exists |
| Separate rejection-reasons panel | MERGED into Gates | same audit source |
| Triage recall, market-discovery, perps panels | DEFERRED | queries/prereqs don't exist yet (T4.3 follow-up / T5.B8) |
| Auth, RBAC, remote hosting, historical analytics, mobile | NOT v1 | operator-listed |
| Write/control plane | NEVER | I2/I4 absolute |

## 8. Testing plan

In crates/fortuna-ops/tests/rota.rs (new) + observability.rs (extended):
- T-1 route-table: every path GET 200 / POST,PUT,DELETE 405 (extends
  tests/dashboard.rs:74-80).
- T-2 seeded-run chain (extends observability.rs:168-265): same seeded run ->
  assert /money pnl == runner.report().realized_pnl.raw(); /gates
  total_rejections == counters; /health halt_active flips with set_halt;
  /settlement discrepancies_open == 0 on the clean run.
- T-3 demo-fixture replay (feature-gated until T4.2): replay
  fixtures/kalshi/ws frames, doctor one seq gap, assert ws_gap_count > 0;
  recorder healthy=false with no file / true with fresh stub.
- T-4 SSE drop-oldest: publish 512 into capacity 256; publisher never blocks;
  slow rx gets Lagged; hub still publishes.
- T-5 perf bound: seed 10k audit rows / 500 beliefs / 50 discrepancies; each
  view < 100ms (10x headroom over the chain test; catches N+1).
- T-6 JSON contract snapshots per view (keys + types pinned).

## 9. SVG mark geometry

viewBox 0 0 48 48, transparent. Wheel: circle r=20 @ (24,24) stroke #D4AF37
sw=2; hub r=4 filled; 8 spokes sw=1 at 45-degree steps. Cornucopia: curved horn
stroke #D4AF37 sw=1.5 fill none, tip ~(14,14) curving to mouth ~(34,34), mouth
ellipse rx=5 ry=3 rotated 45 degrees. Implementer may adjust coordinates,
keeping: eight-spoked wheel, cornucopia in the lower-right quadrant, all gold.

## 10. Implementer validation checklist (run BEFORE any code; record below)

- V-1 `cargo check -p fortuna-ops` clean; serve_dashboard/DashboardSnapshot +
  three routes present at dashboard.rs:52-68; POST 405 test at
  tests/dashboard.rs:74-80.
- V-2 `grep fortuna_rate_bucket_fill crates/fortuna-runner/src/runner.rs` = no
  hits (the gauge is genuinely new).
- V-3 boards_json (runner.rs:2220-2250) has only "positions"+"ops" keys — the
  "account" extension is genuinely new.
- V-4 fortuna-ops/Cargo.toml has axum+tokio; fortuna-ledger ABSENT (to add).
- V-5 cycle check before adding fortuna-ledger to fortuna-ops: inspect
  fortuna-ledger/Cargo.toml deps; if it depends on fortuna-ops that's a cycle
  -> pass the pool via trait object instead. Verify with cargo tree.
- V-6 AuditWriter::recent(kind,&str, limit:i64)->Result<Vec<AuditRow>> at
  audit.rs:74; AuditRow fields audit_id/at/kind/actor/ref_id/payload
  (audit.rs:19-28).
- V-7 `grep -r discrepancy_resolutions crates/fortuna-ledger/migrations/` >= 1
  CREATE TABLE (open_count query depends on it; missing = runtime failure).
- V-8 CalibrationParamsRepo + CalibrationParamsRow exported from ledger lib.rs.
- V-9 data/perishable/<date>/ exists with JSONL; capture_row schema (v,
  cycle_id, captured_at_ms, stream, key, status, body, derived) at
  recorder/lib.rs:87-106 matches the scanner.
- V-10 fixtures/kalshi/ws__orderbook_trade_yes.jsonl non-empty; first line has
  a seq field (gap-inject test feasible).
- V-11 tokio "full" includes sync/broadcast (Cargo.toml:22). No new feature.
- V-12 clippy -D warnings clean at HEAD before writing any ROTA code.

### Fit-validation notes

Recorded 2026-06-11 (implementer loop iteration 2; validation only, no code).
Verdict: **BUILDABLE AS AMENDED** — every V-check passes; the amendments
resolve every misfit found in the body. Build to the amendments.

SLICE 1 BUILT (2026-06-11): crates/fortuna-ops/src/rota.rs — RotaState
(R1 Option-capability: only the snapshot arc is mandatory; pool +
perishable_dir optional), rota_router (all v1 GET routes), five surfaces
reading the new DashboardSnapshot.views field (R2; no Prometheus-text
parsing) with explicit "unavailable" when a view is unpopulated, the
cursor-polled audit tail (R3; raw sqlx ascending audit_id, degraded
empty-page when pool absent — NOT SSE), and the gold-on-black shell
(R11 single auto-fit grid line; R12 no JS toolchain). fortuna-ops gained
sqlx (no fortuna-ledger dep yet — the audit query is raw sqlx, sidestepping
the cycle entirely for now). 4 tests: route-table T-1 (GET 200 / mutating
405 on every path), degraded surfaces (200 never 500), populated-view
verbatim, shell tokens. NEXT SLICES: the composition populates
snapshot.views from metrics_export()+boards_json() (the daemon between-
segments hook) + the T-2 seeded-run chain; the streams surface reading
data/perishable; R5 dedicated 2-conn pool when the daemon wires the pool;
cursor-pagination test; R7 cognition panel (BeliefsRepo::recent +
calibration scopes — needs the two new ledger queries); R12 browser pass
(verifier-layer).

SLICE 2 BUILT (2026-06-11): crates/fortuna-live/src/views.rs — `views_from`
shapes the per-view JSON the slice-1 handlers serve (R2: the daemon shapes,
fortuna-ops never depends on fortuna-runner). POPULATED: `health` (halt
state via the NEW pure `SimRunner::active_halt()` accessor, fill-latency
p90/p95/p99 per R6 — NO p50, dead_man null per gate note 6, venue error
count) and `settlement` (limbo, overdue, voids, reversals) fully, plus the
primary scalars of `gates` (total_rejections) and `streams`
(venue_api_errors_total). Each view carries the §5 `generated_at`, passed
in by the between-segments closure (which holds the clock) so `views_from`
stays pure/clock-free and the lib never reads a clock. Wired into main's
closure (builds views BEFORE the try_write, R8). 4 unit tests (shape, halt
surfacing, settlement, scalars-present/arrays-deferred). The daemon→ops
contract is covered end-to-end without a new dev-dep: the producer shape
(views.rs tests), the consumer passthrough (slice-1 `populated_view_is_
served_verbatim`, since read_view is a literal `views.get(name).cloned()`),
and the live wiring (daemon_smoke runs the closure). DELIBERATELY DEFERRED
(each needs a capability this slice lacks; faked values read as "all clear"
to an operator, so the keys are ABSENT not zeroed): `money` (needs the new
boards "account" field), `cognition` (R7 two ledger queries),
gates.rejections_by_check (needs a runner read-path accessor — escapes only
via Prometheus text today, which R2 forbids parsing), recent_rejections /
recent_watchdog_events (R5 audit pool), streams.recorder + per-venue
book_age_ms (filesystem scan + new boards field), health.last_tick_age_ms
(no last-tick wall stamp tracked). REMAINING after slice 2: those deferred
views/fields; R5 pool; cursor-pagination test; the streams fs-scan; the
Phase-3 shell/assets; R12 browser pass.

SLICE 3 BUILT (2026-06-11): serve_dashboard now MOUNTS the ROTA console
(design §6: "serve_dashboard gains RotaState, merges rota_router"). Before
this, rota_router existed but was wired into NOTHING — the running daemon
served only the legacy Instrument boards, so slices 1+2 were unreachable
live (the dashboard was up only in fortuna-ops tests). serve_dashboard's
signature changed Shared -> RotaState; it derives the legacy routes' state
from rota.snapshot and `.merge`s rota_router(rota) (both Router<()> after
with_state — no route overlap: legacy /,/metrics,/api/boards vs ROTA
/rota,/api/rota/v1/*). The daemon main builds RotaState::standalone(snapshot)
(pool/perishable_dir None this slice — the operator's primary panels need
only the snapshot; the audit tail + recorder scan are later slices). The 3
serve_dashboard callers (dashboard + observability tests, daemon main) all
updated. New test serve_dashboard_mounts_the_rota_console_alongside_the_
instrument: red-first (/rota 404 before the merge), proves the populated
slice-2 health view is SERVED through the merged tree and that ROTA's
read-only doctrine survives the merge (POST/rota -> 405). NOW: an operator
running the daemon can open /rota and watch live health/settlement/gates/
streams. REMAINING unchanged from slice 2 minus the mounting.

SLICE 4 BUILT (2026-06-11): the streams recorder filesystem-scan (§5
recorder section). `fortuna_ops::rota::scan_recorder(perishable_dir,
generated_at)` stats data/perishable/<today>/<stream>.jsonl and the
/streams handler MERGES the result into the daemon-shaped venue view when
ROTA holds the perishable_dir capability (standalone omits it — degraded,
never faked). PERF-CRITICAL DESIGN CALL: the scan reads only file METADATA
(mtime -> last_capture_age_secs, len -> size_bytes, healthy = age < 120s) —
NEVER content. The §5 rows_today/key_count fields are DEFERRED because
counting them means reading the whole file, and bracket_quotes.jsonl is
~1.3GB today; a line-count on the 15s poll would be a self-inflicted DoS
(exactly what the T-5 perf budget guards). size_bytes is the cheap
growth proxy in their place. The scan is clock-free: "now" + today both
come from the snapshot's generated_at (the daemon's last clock read), so
fortuna-ops adds no wall read; deterministic under test (probe-file mtime
drives the fixture dates). Daemon main now wires perishable_dir =
"data/perishable" (matching fortuna-recorder's default --out-dir) so the
scan is LIVE, not dead code. 3 tests: scan unit (fresh/stale/missing-dir),
handler-merge (recorder present with capability), handler-omit (absent
without). REMAINING: money + cognition views, audit-tail recents (R5 pool),
gates.rejections_by_check, cursor-pagination test, rows_today/key_count
(content-read optimisation), Phase-3 shell/assets, R12 browser pass.

SLICE 5 BUILT (2026-06-11): R5 dedicated audit pool. `fortuna_ledger::
connect_readonly_pool` = an ISOLATED 2-conn read pool (short acquire_timeout +
3s statement_timeout, no migrations) wired into the daemon's RotaState.pool
(was None) so the audit TAIL is LIVE — never the writer's pool (audit-append
failure is a global halt; dashboard load must not queue against the audit
writer). Connect failure => audit panel degrades empty, daemon never crashes.
The /audit available:true path is now HTTP-tested end-to-end (F1
cursorless-latest at the handler layer). This pool also unblocks the cognition
view's two ledger queries (next slice). MONEY VIEW is now DESIGN-BLOCKED (not a
deferral): §5's account model (settled/committed/floating, total=settled+
floating) has no faithful source — inspect_totals returns (cash, reserved,
counts), and positions are not strategy-attributed; building it would fabricate
a financial surface. Ledgered in GAPS for an operator/design call. REMAINING:
cognition view (R7, now pool-unblocked), money view (design-blocked),
gates.rejections_by_check, Phase-3 shell/assets, R12 browser pass.

SLICE 6 BUILT (2026-06-11): gates.rejections_by_check. New pure read accessor
`SimRunner::rejections_by_check()` (check name -> count, sorted) — the breakdown
otherwise escapes only via Prometheus text, which R2 forbids parsing. views_from
shapes it into the gates view as sorted {check, count} entries; §5's per-check
"number" is OMITTED (the runner keys by check NAME only — a gate number would be
a guess). Test asserts the consistency invariant: the by-check counts SUM to
total_rejections (holds for any run, including zero). The gates surface is now
complete bar recent_rejections (audit-pool query). REMAINING: cognition view
(R7), money view (design-blocked), recent_rejections/recent_watchdog (audit
queries), Phase-3 shell/assets, R12 browser pass.

SLICE 7 BUILT (2026-06-11): money view — SIM-ONLY subset (R6; the r5-pool gate's
verifier-endorsed unblock). boards_json gained an "account" block {cash_cents,
reserved_cents} from SimVenue::inspect_totals; views_from money: basis="sim-only",
settled_cents=cash, committed_cents=reserved (both real), positions reshaped to
§5 yes_qty/no_qty. floating_cents + total_cents are NULL — §5's
total=settled+floating needs the mark loop (not exposed; "the mark loop is the
missing source", verifier), so they are honestly null and the "sim-only" basis
label prevents misreading as the complete picture. This completes the FIVE
primary surfaces with real data. REMAINING: cognition view (R7's two ledger
queries), the full §5 money model (mark-loop floating + per-strategy attribution
— operator/design call), recent_rejections/recent_watchdog (audit queries),
Phase-3 shell/assets, R12 browser pass.

- V-1 PASS: serve_dashboard + the three routes present (dashboard.rs ~52-68;
  `route("/")`, `/metrics`, `/api/boards`); POST-405 loop at
  tests/dashboard.rs:74-80 exactly as cited.
- V-2 PASS: `fortuna_rate_bucket_fill` = 0 hits in runner.rs. NOTE: R4 CUT
  the gauge — V-2's meaning inverts from "genuinely new, to add" to "stays
  absent"; the gates panel ships without rate_buckets.
- V-3 PASS: boards_json top level is exactly "positions" (runner.rs:2237) +
  "ops" (:2238); the "account" extension is genuinely new (R6: sim-only,
  labeled).
- V-4 PASS: fortuna-ops deps = core, gates, axum, tokio. fortuna-ledger
  ABSENT (to add); fortuna-runner ABSENT (R2's no-cycle rule already holds).
- V-5 PASS — NO CYCLE: fortuna-ledger RUNTIME deps = core, venues, exec,
  gates (cognition is DEV-ONLY — corrected 2026-06-11, ledger-gate fix 4c;
  the original note mislabeled it a runtime dep); fortuna-ops is not among
  them, so ops -> ledger is a safe new edge. (Re-verify with cargo tree
  when the dep lands.)
- V-6 PASS: `pub async fn recent(&self, kind: &str, limit: i64) ->
  Result<Vec<AuditRow>, LedgerError>` at audit.rs:75 (1-line drift from the
  cite, R10 anticipated); AuditRow has the seven fields.
- V-7 PASS: discrepancy_resolutions CREATE TABLE present in exactly 1
  migration file.
- V-8 PASS: CalibrationParamsRepo + CalibrationParamsRow exported
  (ledger lib.rs:33-34).
- V-9 PASS: data/perishable/2026-06-11/ live (recorder running, pid 79813 —
  do not restart it); capture_row schema fields match the cited range.
- V-10 PASS with nuance: ws__orderbook_trade_yes.jsonl non-empty; the FIRST
  lines are `subscribed` acks (no seq); the first DATA frame
  (orderbook_snapshot, line 3) carries seq:1 — gap-inject remains feasible;
  the T-3 scanner must skip non-seq frames.
- V-11 PASS: workspace tokio = features ["full"] (includes sync/broadcast —
  moot for v1 since R3 cut SSE, but true).
- V-12 PASS: clippy --workspace --all-targets -D warnings clean at HEAD
  (run this iteration, 2026-06-11, before these notes).
- R7 precondition CONFIRMED: `BeliefsRepo::recent` does not exist (0 hits in
  repos.rs) — the two new ledger queries (recent beliefs; calibration scope
  enumeration) are genuinely T4.3-owned work, with tests + sqlx prepare.

Body-vs-amendment conflicts the builder must NOT implement from the body:
§0.4/§3/§5 SSE machinery (SseHub, /stream handler, T-4) — R3 cut it, audit
tail is cursor-polled; §3 RotaState mandatory `pool: Arc<PgPool>` and §6
"same Arc<PgPool>; no second Postgres connection" — R1 (Option capability)
and R5 (DEDICATED 2-conn pool) override BOTH; §5 gates `rate_buckets` field
— R4 dropped; §5 health `fill_latency_p50_ms` — R6: no p50 exists or gets
added (runner exports p90/p95/p99 only — verified in metrics_export).
Bloat watch: none beyond the body items the amendments already cut.

## 11. Implementation sequence

Phase 1 skeleton (tests first): ledger dep after V-5; rota module (sse,
recorder scanner, routes, view stubs); serve_dashboard merge; T-1 green.
Phase 2 data: rate-bucket gauge (+ metric-names test extension at
sim_loop.rs:347), boards account fields, six real handlers, SSE handler; T-2,
T-4, T-6 green; clippy clean.
Phase 3 shell + assets: logo.svg per Section 9; shell.html (tokens, 7-panel
grid, per-panel fetch + EventSource, halt red takeover, tabular numbers);
inline as const; favicon; manual smoke vs a Sim run incl. simulated halt.
Phase 4 gate: T-5 perf, T-3 feature-gated replay, fmt/clippy/workspace/DST
green, tick T4.3 with note + hash. Independent gate verifies.

## 12. Explicitly NOT building in v1

Auth, RBAC, remote hosting, write/control-plane endpoints (NEVER — I2/I4),
historical analytics, mobile layout, multi-operator, triage-recall panel,
perps/funding panel (T5.B8), live WS gap counters (stub until T4.2), raw LLM
response / extended-thinking display (not persisted by the cognition layer by
design; if ever wanted, it is a cognition-layer persistence task with its own
retention/secrets review — not a dashboard task).
