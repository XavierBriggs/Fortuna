# Paper-on-Live-Data Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One honest mode where the full strategy set runs against **live Kalshi production market prices** but executes **paper fills locally** — never sending a real order — so edges can be validated with real prices and zero capital at risk.

**Architecture:** A composite `PaperLiveVenue` implements the existing `Venue` trait by **splitting two concerns the trait conflates**: *market-data reads* delegate to a read-only live-Kalshi client; *execution + portfolio* delegate to an embedded `PaperVenue` fed live books + public trades each tick. The real-venue connection has **no `place()` path wired** — a compile-shaped + invariant-tested safety wall guarantees no order can reach the exchange.

**Tech Stack:** Rust 2021 workspace; `fortuna-venues` (`Venue` trait, Kalshi adapter), `fortuna-paper` (`PaperVenue`), `fortuna-live` (daemon composition), `fortuna-invariants` (protected executable safety tests), `sqlx`/Postgres, the DST corpus.

---

## 0. Why this exists (read first)

The daemon today boots exactly two ways: `venue="sim"` (synthetic) or `venue="kalshi"` (hardwired to the **demo** endpoint, which has the real catalog but **no liquidity** → no prices → nothing to validate an edge against). The components for "paper against live prices" exist but were **never composed**:

- The Kalshi adapter supports production (`KALSHI_PROD_BASE_URL`, `client.rs:28`) but only `build_kalshi_demo_transport` is wired (`daemon.rs:560`).
- `PaperVenue` (`fortuna-paper`) does realistic through-not-touch fills but is only used in unit/DST tests, never in the live daemon.

This plan composes them into the missing mode. It is deliberately **not** a hack: it introduces the conceptual split (data-source vs execution-target) the system will want long-term, while reusing the existing `Venue` trait so it lands incrementally.

## 1. The architectural insight: decouple *data source* from *execution target*

The `Venue` trait (`fortuna-venues/src/lib.rs:91–122`) bundles three concerns:

| Concern | Methods |
|---|---|
| **Market data** | `markets`, `book` (+ needed: recent trades) |
| **Execution** | `place(GatedOrder)`, `cancel` |
| **Portfolio/account** | `positions`, `open_orders`, `balance`, `account`, `fills_since`, `settlements_since` |

The four real run-modes are points in a `(data-source × execution-target)` matrix:

| Mode | Data source | Execution target | Capital |
|---|---|---|---|
| Sim | synthetic | synthetic fills | none |
| Demo (today's "paper") | Kalshi demo | Kalshi demo | mock |
| **Paper-on-Live (this plan)** | **Kalshi prod (read-only)** | **local PaperVenue** | **none** |
| Live (gated, I7) | Kalshi prod | Kalshi prod | real |

**`PaperLiveVenue` realizes the new row by composition, not a trait rewrite:** market-data methods read the live client; execution + portfolio methods are the embedded `PaperVenue`. The long-term direction (a future, separate refactor) is to split `Venue` into `MarketDataSource` + `ExecutionVenue`; this composite is the correct, low-risk stepping stone and does not foreclose that split.

## 2. Component design

```
                         ┌─────────────────────────── PaperLiveVenue (NEW) ──────────────────────────┐
 strategies → gate →     │  Venue::place(GatedOrder)  ─────────────────►  PaperVenue.place()  (LOCAL) │
 GatedOrder              │  Venue::cancel             ─────────────────►  PaperVenue.cancel()         │
                         │  Venue::positions/balance/account/fills_since/settlements_since ──► Paper  │
 each tick, drive() →    │  Venue::markets/book       ─────────────────►  KalshiReadClient (PROD, RO) │
   refresh_market_data() │  refresh_market_data():                                                     │
                         │     books  = read.book(m)        → PaperVenue.apply_book(m, bids, asks)     │
                         │     trades = read.recent_trades(m, since_ts)                                │
                         │             for t in trades: PaperVenue.apply_public_trade(m, t.px, t.qty)  │
                         └─────────────────────────────────────────────────────────────────────────────┘
                                    KalshiReadClient: reads only — NO place()/cancel() in its type
```

**New/changed pieces:**
- `KalshiReadClient` — a **read-only** view over the Kalshi adapter (prod transport): `markets`, `book`, `recent_trades`, `settlements_since`. It deliberately does **not** expose `place`/`cancel`. (Implementation reuses `KalshiVenue`'s read methods but behind a read-only surface so execution is unreachable by type.)
- `recent_trades(market, since_ts) -> Vec<PublicTrade>` — **net-new** adapter method over the public, unauthed `GET /markets/trades` (`ticker`, `limit`, `min_ts`, `cursor`). This is the live print stream `PaperVenue.apply_public_trade` consumes.
- `build_kalshi_prod_transport` — mirror of `build_kalshi_demo_transport` pointed at `KALSHI_PROD_BASE_URL`, reading prod creds (`KALSHI_API_KEY_ID` / `KALSHI_PRIVATE_KEY_PATH`).
- `PaperLiveVenue` — the composite `impl Venue`.
- `refresh_market_data()` — called once per `tick()` (or per segment) to push live books + trades into the embedded PaperVenue before strategies read quotes.

**Reuse, unchanged:** `PaperVenue::new/add_market/apply_book/apply_public_trade/settle_market` and its through-not-touch + haircut fill model (`fortuna-paper/src/lib.rs:153,183`); the `Venue` trait; `GatedOrder` seal; `SimRunner::new_with_venue` (`runner.rs:470`).

## 3. The safety wall (most important section)

**Requirement:** in Paper-on-Live mode it must be *impossible* for a real order to reach the exchange, even with a bug.

Three independent layers:
1. **Type-level:** `KalshiReadClient` has no `place`/`cancel` methods at all. `PaperLiveVenue::place` calls only `self.paper.place(...)`. There is no code path from a `GatedOrder` to the prod transport's order endpoint.
2. **Construction-level:** the prod transport built for this mode is handed only to the read client. The boot gate (`boot.rs`) for `execution="paper"` refuses to construct any path that wires prod creds into an executing `KalshiVenue`.
3. **Invariant-level (protected, executable):** a new test in `fortuna-invariants` (additions-only) drives a `PaperLiveVenue` whose read client is a fault-injecting mock that **panics if its order endpoint is ever called**, places N gated orders, and asserts the order endpoint was never touched and fills came only from `apply_public_trade`. Mirrors the existing kill-switch independence test style.

**Stage semantics (I7):** Paper-on-Live is a *paper* stage — it validates edges *before* a promotion, never executes real capital. It does **not** widen the promotion ladder. The audit trail records `execution=paper, data_source=prod` on every decision so a replay is unambiguous.

## 4. How it interacts with the invariants

- **I1 (universal gate):** unchanged. Proposals still pass the 10-check pipeline → `GatedOrder` seal → `Venue::place`. `PaperLiveVenue` accepts the sealed type like any venue.
- **I6 (propose-only model):** unchanged. The model proposes; the harness sizes/times/executes via the composite.
- **I7 (promotion):** new mode is paper; boot refuses real execution; no promotion-ladder change.
- **I5 (append-only audit):** every fill, book refresh, and the `data_source=prod/execution=paper` tag are audit rows; no schema change beyond an added provenance field on decisions (additive).
- **Discovery (T4.2):** *orthogonal* to Phase 1 — see §6.

## 5. Phasing (each phase ships working, testable software)

- **Phase 1 — Paper-on-Live core (this plan, detailed below).** Trades the **declared** markets (`[kalshi].series` / `bracket_sets` / perp ladder) against live prod prices with paper fills. No discovery dependency. This is the edge-validation mode.
- **Phase 2 — Discovery contract fix + T4.2 (separate sub-plan).** Fix the shared journal-as-strict-JSON contract so the real AnthropicMind can drive `world_forward_discovery` AND `market_back_discovery`; then wire the live catalog (`market_meta → Vec<MarketView>`) so markets **auto-discover** instead of being hand-declared.
- **Phase 3 — Consolidation + soak (separate sub-plan).** One config composing all strategies; budget/cost rails; the multi-day validation soak + the scoring/calibration loops.

## 6. The discovery contract — the cross-cutting blocker (scopes Phase 2)

Both `world_forward_discovery` (`discovery.rs:522`) and `market_back_discovery` (`discovery.rs:312`) parse `output.journal.body` as **strict JSON** (`WatchlistBatch` / `NormalizationBatch`, `#[serde(deny_unknown_fields)]`). But the AnthropicMind's `JournalDraft.body` is an **unconstrained `String`** (`mind.rs:108`, schema `mind.rs:504`) — real Opus returns prose → `serde_json::from_str` fails ("expected value at line 1 column 1"). Both paths were only tested with `StubMind` scripted JSON.

**Proper fix (Phase 2, not a heuristic prose-scrape):** give the Mind a *structured-output channel for discovery cycles*. Options, decided in the Phase 2 brainstorm:
1. A dedicated `Mind::discover(ctx, schema) -> StructuredOutput` that uses Anthropic structured-outputs / tool-use to force a typed payload (cleanest; the discovery payload stops riding in the free-text journal).
2. Or a per-cycle `output_config` schema applied to `journal.body` when the cycle kind is discovery.
Then an **integration test against the real AnthropicMind** (not StubMind) gates both discovery paths. Until this lands, `market_back` auto-discovery stays inert — which is why Phase 1 deliberately does not depend on it.

**LANDED (Phase 2 — market-back AND world-forward, 2026-06-17):**
- **P2.1 (contract fix, option 1):** added `Mind::decide_structured(ctx, schema) -> StructuredDecision` — `AnthropicMind` forces a typed payload via the provider `json_schema` output channel; `StubMind` falls back to its scripted journal JSON via the trait default. `market_back_discovery` now calls `decide_structured(ctx, normalization_schema())` instead of parsing free-text journal prose, so the real Opus path no longer fails.
- **P2.2 (live catalog wiring):** the daemon sources the market-back catalog from `runner.market_views()` (the venue catalog the per-tick poll refreshes into `market_meta`) **each segment** — never carried as `DiscoveryWiring` state (the `catalog` field was removed). With real Kalshi markets the prefilter (category allowlist / volume floor / calibration quality) runs over the live listings, so markets auto-discover instead of being hand-declared. Also fixed a prod-relevant waste found here: market-back now early-returns (no mind call, no budget spend) when the prefilter leaves **zero survivors** (`market_back_is_inert_with_no_survivors`).
- **P2.3 (world-forward structured output):** `world_forward_discovery` now also rides `decide_structured`, against a **combined `watchlist_schema()`** that carries BOTH the candidate events AND their zero-capital beliefs in one typed payload (the model's beliefs no longer ride `output.beliefs`, which was the one reason world-forward still needed `decide()`). The unscoreable rule is unchanged (the code is authority; the schema only guides), and the harness still stamps belief provenance `{model_id, context_manifest_hash, cost_cents}` — now in the discovery layer, since the structured channel returns a raw `Value`. **This is what lets the real Opus mind turn the live signal stream into watchlist events + beliefs** instead of returning prose that fails the strict-JSON parse (observed: `watch:` events = 0 in the live soak before this).

---

## Phase 1 — Tasks

> Conventions: money is `Cents` (i64 newtype), no `unwrap/expect/panic` on money paths, `thiserror` per crate, all time via the injected `Clock`. TDD: failing test → minimal impl → green → commit. Run `cargo fmt --all --check`, `SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings`, the crate tests, and (where touched) `scripts/run-dst.sh` before each commit. Protected crate `fortuna-invariants` is additions-only.

### Task 1: `recent_trades` public-trades reader on the Kalshi adapter

**Files:**
- Modify: `crates/fortuna-venues/src/kalshi/adapter.rs` (add method near `book()` ~`:405`)
- Modify: `crates/fortuna-venues/src/kalshi/dto.rs` (DTOs for the trades response)
- Test: `crates/fortuna-venues/src/kalshi/adapter.rs` (`#[cfg(test)]`) + fixture `fixtures/kalshi/trades__public_markets.json`
- Reference: `docs/research/venue/kalshi-api-2026-06-10/` for the `GET /markets/trades` schema

- [ ] **Step 1: Record/provenance the fixture.** Capture a real `GET /markets/trades?ticker=...&limit=100` response into `fixtures/kalshi/trades__public_markets.json` via `fortuna-recorder` (secret-scanned; no creds in the file — it is a public endpoint). If recording is blocked, transcribe the schema from the OpenAPI doc and mark the fixture `provenance: openapi-schema` in a sibling `.provenance` note. **Never fabricate prices.**
- [ ] **Step 2: Write the failing parse test.** Assert that parsing the fixture yields `Vec<PublicTrade>` with `{ market, yes_price: Cents, qty: i64, ts: UtcTimestamp }`, prices converted to integer cents (rounding documented), NO→YES mirrored consistently with `book()`.
- [ ] **Step 3: Run it — expect FAIL** (`PublicTrade`/`recent_trades` undefined).
- [ ] **Step 4: Add the `PublicTrade` type + `recent_trades` DTO + parse** (mirror `book()`'s price conversion; reject malformed with `VenueError`, never panic).
- [ ] **Step 5: Add `async fn recent_trades(&self, market: &MarketId, since_ts: Option<UtcTimestamp>) -> Result<Vec<PublicTrade>, VenueError>`** issuing `GET /markets/trades?ticker=&limit=100&min_ts=` (unauthed path; reuse the transport's GET). Map 429→`RateLimited`.
- [ ] **Step 6: Run tests — expect PASS.** Mutation-proof: flip the through-conversion (cents vs dollars) and confirm the test REDs.
- [ ] **Step 7: Commit** `feat(kalshi): public recent_trades reader (GET /markets/trades)`.

> Note: `recent_trades` is added as an inherent method on `KalshiVenue`, NOT to the `Venue` trait (keeps the trait stable; the composite calls it directly on the read client).

### Task 2: Read-only prod transport builder

**Files:**
- Modify: `crates/fortuna-live/src/daemon.rs` (near `build_kalshi_demo_transport` `:560`) + `crates/fortuna-live/src/main.rs` (cred resolution `:532`)
- Test: `crates/fortuna-live/src/daemon.rs` `#[cfg(test)]`

- [ ] **Step 1: Failing test** that `build_kalshi_prod_transport(key_id, key_pem, clock)` produces a transport whose base URL is `KALSHI_PROD_BASE_URL` (assert via a test seam exposing the configured base, mirroring any existing demo-transport test).
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement `build_kalshi_prod_transport`** as the exact mirror of the demo builder but `KALSHI_PROD_BASE_URL`. Keep credential IO at the binary edge (`main.rs`): add `resolve_kalshi_prod_creds(env)` reading `KALSHI_API_KEY_ID` + `KALSHI_PRIVATE_KEY_PATH`, PEM wrapped in `Secret`, errors name the var/path never the value.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(live): read-only Kalshi prod transport builder + cred resolution`.

### Task 3: `KalshiReadClient` (read-only surface, no execution)

**Files:**
- Create: `crates/fortuna-venues/src/kalshi/read_client.rs`
- Modify: `crates/fortuna-venues/src/kalshi/mod.rs` (export)
- Test: same file `#[cfg(test)]`

- [ ] **Step 1: Failing test** constructing `KalshiReadClient::new(transport, clock, series)` and calling `markets()`, `book()`, `recent_trades()`, `settlements_since()` against a mock transport; assert the type has **no** `place`/`cancel` (compile-enforced — the test simply doesn't reference them; document the intent in a doc-comment the invariant test in Task 6 backs).
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement** `KalshiReadClient` wrapping the same read internals `KalshiVenue` uses (extract shared read logic if needed; do NOT duplicate signing). Expose only reads + `recent_trades`.
- [ ] **Step 4: Run — PASS.**
- [ ] **Step 5: Commit** `feat(kalshi): read-only client surface (no execution path)`.

### Task 4: `PaperLiveVenue` composite

**Files:**
- Create: `crates/fortuna-paper/src/paper_live.rs` (or a new `fortuna-paper-live` if dep direction requires — paper depends on venues for the read client; verify no cycle)
- Modify: `crates/fortuna-paper/src/lib.rs` (export)
- Test: same file `#[cfg(test)]`

- [ ] **Step 1: Failing test — reads delegate, execution is paper.** Construct `PaperLiveVenue::new(read_client_mock, paper_config, starting_cash, clock, fees)`. The mock read client returns a book (bid 30 / ask 60) and one public trade. Call `refresh_market_data()`, then `place(gated buy YES @ 40)`; assert: no fill yet (40 < 60 ask, rests as maker); then a public trade printing *through* 40 (e.g. 39) yields a fill via `apply_public_trade`; `markets()`/`book()` return the mock's data; `balance()` reflects the paper fill.
- [ ] **Step 2: Run — FAIL.**
- [ ] **Step 3: Implement `impl Venue for PaperLiveVenue`:** `markets`/`book` → read client; `place`/`cancel`/`positions`/`balance`/`account`/`fills_since`/`settlements_since` → embedded `PaperVenue`; `recent_trades`+`book` pushed into the PaperVenue inside `refresh_market_data()`. `place()` MUST call only `self.paper.place`.
- [ ] **Step 4: Run — PASS.** Mutation-proof: make `place()` (wrongly) call a read-client method and confirm it fails to compile / the test REDs.
- [ ] **Step 5: Commit** `feat(paper): PaperLiveVenue — live reads + local paper execution`.

### Task 5: Wire `refresh_market_data()` into the drive loop

**Files:**
- Modify: `crates/fortuna-runner/src/runner.rs` `tick()` (`:774`) OR `crates/fortuna-live/src/daemon.rs` segment loop — call `venue.refresh_market_data()` before strategies read quotes. Prefer a `Venue`-optional hook so Sim/Kalshi are byte-unchanged.
- Test: `crates/fortuna-runner/src/runner.rs` `#[cfg(test)]`

- [ ] **Step 1: Failing test** that a tick on a `PaperLiveVenue` pushes the live book before `BookSnapshot` is published, so a strategy sees the live quote.
- [ ] **Step 2–4:** add the hook (a default-noop trait method `async fn refresh_market_data(&self) -> Result<(), VenueError> { Ok(()) }` on `Venue`, overridden by `PaperLiveVenue`) so Sim/Kalshi paths are unchanged; run red→green. Confirm `daemon_smoke` DST still byte-identical for sim.
- [ ] **Step 5: Commit** `feat(runner): per-tick venue market-data refresh hook (default no-op)`.

### Task 6: PROTECTED invariant — no real order in paper-on-live

**Files:**
- Create: `crates/fortuna-invariants/tests/i_paper_live_no_real_order.rs` (additions-only)

- [ ] **Step 1: Write the invariant test.** A `PaperLiveVenue` whose read client is a mock that **panics on any order/cancel endpoint call**. Drive: feed books/trades, place 50 seeded gated orders, run cancels, settle. Assert: (a) zero panics (no order endpoint touched), (b) all fills originated from `apply_public_trade` (through-not-touch — a fill *at touch* must not appear), (c) `fills_since` returns only paper fills.
- [ ] **Step 2: Run — PASS** (the wall holds).
- [ ] **Step 3: Mutation-proof** — temporarily make `PaperLiveVenue::place` call the read client's transport order path; confirm the invariant test PANICS/REDs; revert.
- [ ] **Step 4: Commit** `test(invariants): paper-on-live cannot place a real order (I-wall)`.

### Task 7: Config + boot gate for the mode

**Files:**
- Modify: `crates/fortuna-live/src/boot.rs` (`:538` stage gate) + the `[daemon]` config model
- Modify: `config/fortuna.example.toml` (document the mode)
- Test: `crates/fortuna-live/src/boot.rs` `#[cfg(test)]`

- [ ] **Step 1: Failing boot tests** for a new `[daemon] data_source = "kalshi_prod"`, `execution = "paper"` pair: (a) it composes `PaperLiveVenue`; (b) `execution = "live"` is REFUSED (I7) with a clear error; (c) `data_source = "kalshi_prod"` with `execution = "kalshi"` (real) is REFUSED unless an explicit operator-promotion record exists.
- [ ] **Step 2–4:** add the config fields (`#[serde(deny_unknown_fields)]`), the validation, and the compose branch that builds `KalshiReadClient(prod) → PaperLiveVenue`; red→green.
- [ ] **Step 5: Commit** `feat(live): boot gate + config for paper-on-live (refuses real execution)`.

### Task 8: Paper-realism DST scenario

**Files:**
- Create/extend: `crates/fortuna-runner/tests/paper_live_dst.rs` + a seed in `crates/fortuna-core/dst-corpus/`

- [ ] **Step 1:** seeded scenario: replayed live-ish books + public trades (from the Task 1 fixture) drive a strategy; assert deterministic fills, **a fill at touch FAILS the suite**, PnL/settlement reconcile, byte-identical replay on re-run.
- [ ] **Step 2–4:** implement; pin the seed file with a comment naming the realism property. Run `scripts/run-dst.sh`.
- [ ] **Step 5: Commit** `test(dst): paper-on-live realism scenario (through-not-touch, replayable)`.

### Task 9: Phase-1 battery + a short supervised dry-run

- [ ] `cargo fmt --all --check` · `SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace` · `scripts/run-dst.sh` · `scripts/check-protected-invariants.sh` — all green.
- [ ] Operator-supervised dry-run (kill switch ready): boot paper-on-live on a *few* declared markets, confirm on ROTA: live books arriving, strategies pricing against real quotes, paper fills only on trade-through, PnL accruing, **zero** orders on the real exchange (verify the prod account shows no orders). Stop.
- [ ] Commit a `CHANGELOG.md` entry + a `docs/runbooks/paper-on-live-bringup.md` stub.

---

## Verification strategy (the verify loop)

- **TDD every task** (red→green), **mutation-proof every safety-relevant assertion** (flip the property, confirm RED) — green alone is not verification.
- **Gate the integrated tree**, not the branch tip: after merging the phase, re-run the full battery + invariants on the merged workspace.
- **The wall is an invariant** (Task 6), protected + additions-only; a regression auto-blocks.
- **Fixtures are real + provenanced + secret-scanned** (Task 1); a fill-at-touch failing the suite is the load-bearing realism check.
- **DST**: new seed pinned forever; replay must be byte-identical.
- **Adversarial review** before any merge to `main` (a separate verifier pass per the playbook).

## Risks / open questions / decisions needed

1. **Prod creds are an operator action.** Reading prod market data uses real-account creds (read-only). Operator must approve pointing at production and confirm the creds in `.env`. (Decision pending — the build can proceed against the demo endpoint for the read client until then; the composite is endpoint-agnostic.)
2. **Read rate limits.** Unauthed `markets`/`trades` limits are undocumented; authed `book` costs tokens. Poll cadence must respect them; add a read-side budget if needed. Surfacing dropped polls (no silent caps).
3. **`book()` requires auth** (only `markets`/`trades` are public). So the read client still needs prod creds for live books — reinforces that the safety wall (no execution from that signed client) is essential.
4. **Calibration cold-start still applies to synthesis.** Even on live prices, the model arm won't trade until calibration accrues; the **mechanical arms trade immediately**. "Validate edges" early = mechanical fills + belief scoring; model-driven fills accrue. Set expectations.
5. **Phase 2 discovery-contract fix** is a real prerequisite for auto-discovery and is its own brainstorm (structured-output channel vs per-cycle schema). Do not let Phase 1 depend on it.
6. **WS vs REST trades.** Phase 1 uses REST `recent_trades` polling (simpler). WS `trade` channel (lower latency, but auth + more surface) is a later optimization.

## Self-review

- **Spec coverage:** live reads (T2,3), public trades (T1), paper execution (T4), per-tick refresh (T5), safety wall (T3 type + T6 invariant + T7 boot), config (T7), realism/replay (T8), battery (T9). Discovery/T4.2 explicitly deferred to Phase 2 with rationale. ✓
- **Placeholders:** none — each task names files, the test intent, and the commit; code-heavy bodies are written at execution time per task (this is a design+plan doc; Phase-1 tasks are bite-sized and ordered).
- **Type consistency:** `PublicTrade {market, yes_price: Cents, qty: i64, ts}` used in T1/T4/T8; `refresh_market_data()` default-noop on `Venue` used in T5/T4; `PaperLiveVenue::new(read_client, paper_config, starting_cash, clock, fees)` consistent T4/T6/T7.
