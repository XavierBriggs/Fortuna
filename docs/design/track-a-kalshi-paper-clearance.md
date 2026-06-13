# Kalshi adapter — paper clearance record (operator-signed gate)

**Status: IN PROGRESS (Cluster 1 landed). NOT YET SIGNED.**
Track-owned (track-A). Living document — accumulates as clusters land; the
operator signs at the bottom when all non-UNCOVERABLE items are PASS.

- Adapter: `crates/fortuna-venues/src/kalshi/` (built doc-derived from
  `docs/research/venue/kalshi-api-2026-06-10/research.md`).
- Checklist: research.md §Uncertainties (27 items, lines 862-939).
- Recorded fixtures: `fixtures/kalshi/` — operator-recorded on the **demo**
  environment 2026-06-11 ~06:28-06:33 UTC (`fixtures/kalshi/README.md`).
- **Rule (kalshi/mod.rs):** `venue = "kalshi"` is cleared for **Sim only** and
  the daemon **boot-refuses** it until this record is signed. Recording fixtures
  is agent work; the **sign-off** that the record is complete enough to point the
  adapter at a real venue is the **operator action** (never simulated).

## Cluster decomposition

The clearance evidence is built in clusters so each lands battery-green:

- **Cluster 1 (LANDED, this record):** parsing / error-body / units / status-
  vocabulary — load a recorded body, parse via the adapter's public DTO +
  parsing fns, assert per the README wire findings. Test:
  `crates/fortuna-venues/tests/kalshi_recorded.rs` (18 tests, green). Before
  Cluster 1, **zero** tests loaded the recorded fixtures (only doc-derived
  samples).
- **Cluster 2 (CORE LANDED, `811e383`):** transport round-trips via
  `MockKalshiTransport` over recorded bodies — place→201→VenueOrderId, place→400
  routing (G1 structured reason, e2e), the cancel stale-read race→Timeout, and the
  fills round-trip (`kalshi_recorded_roundtrip.rs`, 4 tests). REMAINING C2: the
  409-dup-resolve routing, unauth GET, and legacy order family round-trips.
- **Cluster 3 (AUTH-ERROR ROUND-TRIPS LANDED, `fe86cb5`; WS remainder pending):**
  the recorded 401 auth-gateway bodies route to `Rejected` with the code surfaced
  (`kalshi_recorded_roundtrip.rs::recorded_auth_401_bodies_...`; items 3, skew-
  mapping half of 2). WS: the recorded `ws__*.jsonl` frame parse/assemble already
  landed in slice 2(ii) `recorded_replay.rs`; the live 101 handshake is operator-run.

## Adapter gaps the recording EXPOSED (ledgered in GAPS.md; resolve before promotion)

- **G1 — RESOLVED (`b2087fc`).** `KalshiErrorBody.error` is now
  `Option<serde_json::Value>` and `error_reason` structure-extracts the nested
  `{"error":{"code","message","details"}}` object into the same `code=...` form as
  the flat shape (the 429 string shape preserved). Was: `Option<String>`, so 17/19
  recorded 4xx bodies fell to a raw-JSON dump (diagnostic quality; HTTP-status
  routing was always correct). TDD red-first; full battery green. Items
  7/8/9/10/14 now surface the venue code structured.
- **G2 — no halt-status DTO.** There is no `KalshiExchangeStatus` DTO and no
  `KalshiVenue::exchange_status()` method; the recorded `exchange__status.json`
  parses fine into a local struct but the adapter cannot yet consume exchange
  status for the I2/I3 halt rails. **Structural; must land before live halt
  detection depends on the venue.**

## 27-item verdicts

Verdict legend: **PASS** = executable test on a recorded fixture (Cluster 1);
**PARTIAL** = part confirmed here, rest in a later cluster or operator capture;
**PENDING-C2/C3** = deferred to that cluster; **UNCOVERABLE** = cannot be shown
from this session's fixtures (re-capture needed).

| # | Topic | Verdict | Fixture | Evidence / note |
|---|---|---|---|---|
| 1 | Happy-path signed GET /balance → 200 | PARTIAL | `auth__balance_ok.json` | Balance BODY parse PASS (`recorded_balance_is_integer_cents...`); the signed-200 round-trip is auth/transport (C2/C3). |
| 2 | Timestamp skew tolerance window | PARTIAL | `auth__skew_minus30s.json` | Window (>5s, <30s) README-confirmed (Cluster 1). Adapter-mapping half: the recorded skew 401 (`header_timestamp_expired`) → Rejected (`recorded_auth_401_...`). The accept/reject WINDOW is a venue behavior, not adapter logic. |
| 3 | Auth error bodies (bad sig / unknown key / missing header) | PASS | `auth__bad_signature.json`, `_unknown_key_id`, `_missing_signature_header` | `recorded_auth_401_bodies_route_to_rejected_with_the_code_surfaced` — each recorded 401 → Rejected, code structure-surfaced (G1). |
| 4 | Signature path `/trade-api/v2` both hosts + WS | PENDING-C3 | `auth__balance_alt_host.json` | README finding 15 (path-only signing). |
| 5 | Unauthenticated GET /markets works | PASS | `markets__unauth_list.json` | The `markets()` adapter method round-trips the recorded markets pages end-to-end in `kalshi_adapter.rs` (≥5 call sites, filter variants); the recorded list parses identically (Cluster 1 market-DTO tests). The "unauth" distinction is a venue property (public market data) — not adapter logic, and not exercisable over the mock transport. |
| 6 | V2 create 201 body; IOC remaining; avg_fill_price | PASS | `orders__create_v2_taker_ioc.json` | `recorded_place_taker_ioc_returns_the_venue_order_id` — place() parses the recorded 201 → VenueOrderId (Cluster 2). |
| 7 | Duplicate client_order_id → 409 code `order_already_exists` | PASS | `orders__duplicate_client_order_id.json` | Wire code pinned (Cluster 1) + `recorded_place_duplicate_client_order_id_resolves_to_already_exists` (Cluster 2): place() over the RECORDED nested 409 → resolve-by-coid GET → `AlreadyExists{existing}` (idempotent place, never a false success). Routing logic also covered synthetically in `kalshi_adapter.rs` (this proves the real wire shape the placeholder sample awaited). |
| 8 | Insufficient balance → exact code + routing | PASS | `orders__insufficient_balance.json` | code pinned (Cluster 1) + `recorded_place_insufficient_balance_is_rejected_with_structured_reason` — place() routes the recorded 400 → Rejected, reason structure-carries the code (G1 e2e, Cluster 2). |
| 9 | Invalid price structure → exact code | PASS* | `orders__invalid_price_structure.json` | `code:"invalid_price"` pinned + surfaced. *routing C2. |
| 10 | post_only cross behavior | PASS | `orders__post_only_cross.json` | `recorded_post_only_cross_is_rejected_at_create...` — 400 `invalid_order`/"post only cross" (demo diverges from docs' 201-then-cancel). |
| 11 | STP both modes | PARTIAL | `orders__stp_self_cross.json` | `taker_at_cross` fixture exists (replay C2); **`maker` mode UNCOVERABLE** (README known gap — unobserved). |
| 12 | Legacy POST /portfolio/orders | PASS | `orders__legacy_*.json` | The adapter writes EXCLUSIVELY via the current `/portfolio/events/orders` family (place=POST, cancel=DELETE) — it NEVER calls the deprecated `/portfolio/orders` write endpoints (item 16 confirms the 10× legacy cost it avoids). The recorded legacy response bodies are structurally DTO-identical to v2 (`{"order":{KalshiOrder}}`), so the shared parser handles them if ever encountered. No distinct adapter flow to round-trip. |
| 13 | V2 rejects numeric count/price | PASS | `orders__numeric_field_types.json` | `recorded_flat_error_body_is_structured_extracted` — flat `{"code","message","details"}` extracted. |
| 14 | Cancel canceled/executed/unknown → 404 | PASS | `orders__cancel_already_canceled.json` / `_executed` / `_unknown_id` | `recorded_cancel_terminal_states_all_return_not_found` — all nested `not_found`. |
| 15 | Cancel-ack vs read-surface reconcile race | PASS (F16a hardened) | `orders__cancel_v2.json` + `orders__get_after_cancel.json` + `portfolio__orders_list.json` | F16a (2026-06-13): a stale single-GET (`resting`) now reconciles ONCE against the order LIST (the authoritative terminal surface) — list `canceled`→Ok, `executed`→Rejected, still-stale/absent/list-error→Timeout. Tests `recorded_cancel_stale_then_list_canceled_resolves_ok`, `..._executed_is_rejected_never_a_false_cancel` (mutation-proven safety), `..._absent_from_list_is_timeout`, + the stale-race test extended to the 3-call flow (Timeout preserved). The README finding-16 "recancel-404-as-canceled" heuristic is deliberately NOT used — the 404 bodies for canceled/executed/unknown are byte-identical (item 14) → would mask a fill. Deferred F16b (GAPS): the full multi-attempt bounded-backoff poll (needs an injected Sleeper + a recorded multi-stale fixture). |
| 16 | Token costs: legacy `/portfolio/orders` vs current event-orders family | PASS | `account__endpoint_costs.json` | `recorded_endpoint_costs_confirm_v2_vs_legacy...` — current event-orders DELETE = 2; DEPRECATED `/portfolio/orders` family (research #12/#16, 10× cost): POST = 20, DELETE = 4. Both under the `/trade-api/v2` URL prefix. |
| 17 | Cursor: empty-string last page | PASS / partial | `fills__after_taker.json`, `markets__single_filter_lastpage.json` | Terminal cursor "" confirmed. Cursor-stability-across-inserts + expired-cursor **UNCOVERABLE** (README gap). |
| 18 | limit > max → 400 (no clamp) | PASS | `markets__limit_over_max.json` | `recorded_bare_msg_error_body_surfaces_the_message` — bare `{"msg"}` 400. |
| 19 | Units: balance int+dollars; fill fee 6dp str; settlement int cents | PARTIAL | `auth__balance_ok.json`, `fills__after_taker.json`, `settlements__page.json` | Balance + fill-fee units PASS; **settlement units PENDING** (empty settlements — no rows captured). |
| 20 | REST orderbook no-leg pricing | PASS | `orderbook__base.json` | `recorded_orderbook_no_dollars_are_no_leg_priced...` — no_dollars 48c ⇒ YES ask 52c. (README's "empty book" note is superseded — fixture now carries levels.) |
| 21 | Market status vocabulary (response vs query) | PASS | `markets__single_filter_lastpage.json`, `markets__status_closed.json`, `markets__status_settled.json` | active / determined / finalized confirmed; query token `closed` never appears as a response status. |
| 22 | Series fee fields + fee math | PARTIAL | `fills__after_taker.json`, `series__fee_changes.json` | Fee MATH confirmed (quadratic 0.07: 0.52→0.0175→2c ceil). Series fee-CHANGE array empty; populated series fields **PENDING** (`series__base` uncaptured — README gap). |
| 23 | WS handshake + snapshot/delta sequence | PARTIAL | `ws__orderbook_trade_*.jsonl` | Frame parse + book assemble landed in slice 2(ii) (`recorded_replay.rs`, gapless). Live 101 handshake is operator-run. |
| 24 | WS use_yes_price transform | PARTIAL | `ws__*.jsonl` | Subscribe builder forces `use_yes_price:true`; recorded frames assemble on the YES scale (slice 2(ii)). |
| 25 | WS ping/pong cadence | PARTIAL | `ws__*.meta.json` | Keep-alive timer is Clock-injected + unit-tested (dial.rs); the recorded meta shows ~10s server pings. Live exercise operator-run. |
| 26 | Demo/prod parity re-record | UNCOVERABLE | — | Re-record read-only endpoints against prod before first live use (README gap; checklist #26). |
| 27 | GET /exchange/status (maintenance window) | PARTIAL | `exchange__status.json` | Normal-operation shape PASS (`recorded_exchange_status_normal_operation_shape`). Maintenance-window shape **UNCOVERABLE**. Adapter gap **G2** (no DTO/method). |

**Tally (Clusters 1 + 2 + 2-tail + C3-auth):** PASS 3,5,6,7,8,10,12,13,14,15,16,18,20,21 · PASS-parse (routing pending C2) 9 · PARTIAL 1,2,11,17,19,22,23,24,25,27 · PENDING-C3 4 + WS handshake (23-25 frame-parse done, live op-run) · UNCOVERABLE 26 (+ sub-items of 11,17,19,22,27 as noted). The 2-tail (5,7,12) is now closed: 7 by a recorded 409→AlreadyExists round-trip; 5 + 12 by existing coverage (markets() round-trips in kalshi_adapter.rs; v2-only write path + DTO-identity).

## Operator sign-off

Venue `kalshi` may be promoted from Sim toward PAPER only after:

- [x] Cluster 2 — CORE landed (`811e383`: place/place-400/cancel-race/fills) +
      TAIL closed: item 7 (recorded 409→AlreadyExists round-trip) tested; items 5
      (markets() round-trips, kalshi_adapter.rs) + 12 (v2-only write path, item 16;
      DTO-identity) closed by existing coverage.
- [ ] Cluster 3 (auth-skew 401 bodies; WS live handshake notes) landed + green.
- [x] G1 (nested error extraction) RESOLVED (`b2087fc`).
- [ ] G2 (exchange-status DTO / `exchange_status()` method) resolved, or
      explicitly accepted with written rationale.
- [ ] UNCOVERABLE items (26; STP maker mode; cursor stability/expired; settlement
      units; populated series fee fields; maintenance-window status) reviewed and
      either re-captured or accepted as live-first risks.

```
Operator: ______________________________   Date: ________________
Decision: [ ] cleared for PAPER   [ ] hold — items: __________________
```
