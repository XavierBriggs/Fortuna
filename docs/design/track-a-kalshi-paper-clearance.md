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
- **Cluster 2 (PENDING):** transport round-trips via `MockKalshiTransport`
  (place / cancel-reconcile / fills flows; HTTP-status → `VenueError` routing;
  unauth GET; legacy order family).
- **Cluster 3 (PENDING):** auth-skew (signed-request 401 bodies) and WS handshake
  (the recorded `ws__*.jsonl` frame parse/assemble already landed in slice 2(ii)
  `recorded_replay.rs`; the live 101 handshake is operator-run).

## Adapter gaps the recording EXPOSED (ledgered in GAPS.md; resolve before promotion)

- **G1 — nested error body not structure-extracted.** `KalshiErrorBody.error` is
  `Option<String>` (dto.rs:562) but 17/19 recorded 4xx bodies send
  `{"error":{"code","message","service"}}` (an object). The struct-parse fails and
  `error_reason` surfaces the venue code via the **raw-JSON fallback**, not
  structured extraction. Functionally safe (HTTP-status routing is independent and
  correct), but diagnostics carry raw JSON. Fix: `error: Option<serde_json::Value>`
  + extract nested `code`/`message`/`details`. **Diagnostic quality, not blocking.**
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
| 2 | Timestamp skew tolerance window | PENDING-C3 | `auth__skew_*` | README finding 2: >5s and <30s. Signed-request replay deferred. |
| 3 | Auth error bodies (bad sig / unknown key / missing header) | PENDING-C3 | `auth__bad_signature.json` etc. | Cluster 3. |
| 4 | Signature path `/trade-api/v2` both hosts + WS | PENDING-C3 | `auth__balance_alt_host.json` | README finding 15 (path-only signing). |
| 5 | Unauthenticated GET /markets works | PENDING-C2 | `markets__unauth_list.json` | Transport-level. |
| 6 | V2 create 201 body; IOC remaining; avg_fill_price | PENDING-C2 | `orders__create_v2_*.json` | Transport round-trip. |
| 7 | Duplicate client_order_id → 409 code `order_already_exists` | PASS* | `orders__duplicate_client_order_id.json` | Wire code pinned + surfaced (`recorded_duplicate_client_order_id_code...`, `..._nested_4xx_...`). *409→AlreadyExists routing is C2. |
| 8 | Insufficient balance → exact code | PASS* | `orders__insufficient_balance.json` | `code:"insufficient_balance"` pinned + surfaced. *routing C2. |
| 9 | Invalid price structure → exact code | PASS* | `orders__invalid_price_structure.json` | `code:"invalid_price"` pinned + surfaced. *routing C2. |
| 10 | post_only cross behavior | PASS | `orders__post_only_cross.json` | `recorded_post_only_cross_is_rejected_at_create...` — 400 `invalid_order`/"post only cross" (demo diverges from docs' 201-then-cancel). |
| 11 | STP both modes | PARTIAL | `orders__stp_self_cross.json` | `taker_at_cross` fixture exists (replay C2); **`maker` mode UNCOVERABLE** (README known gap — unobserved). |
| 12 | Legacy POST /portfolio/orders | PENDING-C2 | `orders__legacy_*.json` | Transport-level. |
| 13 | V2 rejects numeric count/price | PASS | `orders__numeric_field_types.json` | `recorded_flat_error_body_is_structured_extracted` — flat `{"code","message","details"}` extracted. |
| 14 | Cancel canceled/executed/unknown → 404 | PASS | `orders__cancel_already_canceled.json` / `_executed` / `_unknown_id` | `recorded_cancel_terminal_states_all_return_not_found` — all nested `not_found`. |
| 15 | Cancel-ack vs read-surface reconcile race | PENDING-C2 | `orders__cancel_v2.json` + `orders__get_after_cancel.json` | README finding 16; transport round-trip with the 360ms stale-read race. |
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

**Cluster 1 tally:** PASS 1,7,8,9,10,13,14,16,17,18,20,21 · PARTIAL 11,19,22,23,24,25,27 · PENDING-C2 5,6,12,15 · PENDING-C3 2,3,4 · UNCOVERABLE 26 (+ sub-items of 11,17,19,22,27 as noted).

## Operator sign-off

Venue `kalshi` may be promoted from Sim toward PAPER only after:

- [ ] Cluster 2 (transport round-trips: place/cancel/fills routing) landed + green.
- [ ] Cluster 3 (auth-skew 401 bodies; WS live handshake notes) landed + green.
- [ ] Adapter gaps G1 (nested error extraction) and G2 (exchange-status DTO)
      resolved, or explicitly accepted with written rationale.
- [ ] UNCOVERABLE items (26; STP maker mode; cursor stability/expired; settlement
      units; populated series fee fields; maintenance-window status) reviewed and
      either re-captured or accepted as live-first risks.

```
Operator: ______________________________   Date: ________________
Decision: [ ] cleared for PAPER   [ ] hold — items: __________________
```
