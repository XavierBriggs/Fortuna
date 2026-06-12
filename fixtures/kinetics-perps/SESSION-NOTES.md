# Kinetics perps fixtures — OPERATOR-RECORDED (demo environment)

Recorded 2026-06-12 ~02:23–02:31 UTC by
`crates/fortuna-venues/examples/record_kinetics_fixtures.rs` (operator-authorized
session, demo credentials only, mock funds). Targets the §12 operator fixture
requests in `docs/research/venue/kinetics-perps-2026-06-10/research.md`.
Naming follows `fixtures/kalshi/`: `<area>__<case>.json` = VERBATIM response
body; sibling `.meta.json` = method/path/status/sanitized request body/note;
`ws__*.jsonl` = verbatim WS text frames (NOTE: server text frames carry a
trailing newline, so the .jsonl files contain blank separator lines — skip
empty lines when replaying). `session__manifest.meta.json` = full session
record. No request headers or key material are recorded.

## THE SESSION RAN DEGRADED — read this first

The demo account is **NOT margin-enabled**: `GET /margin/enabled` (signed,
accepted) returns `{"enabled": false}` and `GET /margin/balance` returns 403
`user is not enabled for margin trading` (`auth__margin_enabled_ok`,
`auth__margin_balance`). Order writes fail 400 `user_not_found:_<user-uuid>`
from `service: "exchange"` — the margin exchange has no user record at all
until enablement (`orders__create_gtc_blocked`, `groups__create`,
`subaccounts__create`).

This **contradicts the research doc §9 reading** that "demo perps is open to
everyone today" means recordable-without-further-action: the SURFACE is open
(public endpoints + WS handshake all work) but trading requires per-account
margin enablement even on demo.

**OPERATOR ACTION REQUIRED:** enable margin/perps trading for the demo
account (demo web app — likely the same application/education flow described
in research §2, or a support toggle), then re-run
`cargo run -p fortuna-venues --example record_kinetics_fixtures`. The
recorder auto-detects enablement and will run the FULL flow (order lifecycle,
funding position, groups, subaccount transfers) on the next run.

## Item coverage (research §12)

| Item | Status | Notes |
|---|---|---|
| 1 auth round-trip + 401s + skew | **COVERED** | Signing recipe `ts + METHOD + /trade-api/v2/margin/...` accepted (200 on /margin/enabled). Bad-signature 401 + skew grid captured (see findings 3-4). |
| 2 margin WS handshake | **COVERED** | Signed-path question SETTLED: signing `/trade-api/ws/v2/margin` → 101 on the dedicated margin host. Fallback path never needed. `subscribed` acks, snapshot+delta with `seq`, ticker with `funding_rate` + all three mark prices, heartbeat ping captured. |
| 3 order lifecycle | **BLOCKED** (enablement) | One blocked-evidence probe recorded (`orders__create_gtc_blocked`); no thrashing. |
| 4 client_order_id duplicate / freed-after-cancel | **BLOCKED** (enablement) | — |
| 5 reduce_only + GTC rejection | **BLOCKED** (enablement) | — |
| 6 insufficient margin 400 | **BLOCKED** (enablement) | — |
| 7 price band + off-tick bodies | **BLOCKED** (enablement) | — |
| 8 orderbook ordering + aggregation | **COVERED** | §11.1 conflict SETTLED — see finding 1. |
| 9 positions/balance/risk with open position | **PARTIAL** | `risk__parameters` (public) captured: IM multiplier 1.3 (1.1 HYPE/SHIB), `liquidation_margin_ratio_threshold: 1`, `queue_entry: 0.8`. Position-bearing reads blocked (balance 403; positions returns 200 but empty). |
| 10 funding position + funding_history | **BLOCKED / PARTIAL** | **NO POSITION WAS OPENED — the deliberate funding position does NOT exist** (order create blocked). Funding-rate public surfaces captured; `funding_history` read works (200, empty) and revealed undocumented required params (finding 6). |
| 11 fee fields post-June-11 (PROD) | SKIPPED (per directive) | Demo `fee_tiers` captured anyway: maker 0.0005 / taker 0.0012 flat across all demo perps — real values now, not the schema's 5/12/8 bps "examples". |
| 12 order groups | **BLOCKED** (enablement) | Create probe → 400 `user_not_found:_<uuid>`. |
| 13 subaccount transfer idempotency | **BLOCKED** (enablement) | Create with empty JSON body → 404 `user_not_found`. Body-less POST → 400 `invalid_content_type` (finding 7). |
| 14 GET /margin/orders status filter | **PARTIAL** | resting/canceled/executed/open/garbage ALL return 200 `{"cursor":"","orders":[]}` on this empty account — garbage values are NOT rejected, so the vocabulary is unconfirmable until orders exist (and the filter may be ignored entirely). Same silent-garbage footgun as the event API's cursor. |
| 15 maintenance-window status (PROD) | SKIPPED (per directive) | — |
| 16 intra-exchange transfer 4xx body | **PARTIAL** | Returns 403 `user is not enabled for margin trading` — the enablement gate fires BEFORE the documented "currently not available" gate, so that body is still uncaptured. |
| 17 PROD parity sweep | SKIPPED (per directive) | — |
| 18 operator outreach questions | SKIPPED (human action) | — |

Capture count: **35 REST fixture pairs + 2 WS capture pairs + 1 manifest = 75
files.**

## Open funding position (item 10)

**NONE.** The deliberate 1-contract position could not be opened (order create
blocked by margin enablement). After the operator enables margin, the next
recorder run opens it (1 contract, long, `KXBTCPERP1`, IOC) and leaves it open;
the manifest will carry the details under `open_funding_position`.

## Load-bearing findings (wire vs research doc)

1. **§11.1 orderbook ordering SETTLED — spec text is WRONG, live observation
   confirmed.** Demo REST book (`orderbook__depth0`): bids ascending
   4.8368→6.3329, asks descending 7.6635→6.3416 — **worst→best on both sides,
   best at array END**. `depth=5` returns the 5 BEST levels but keeps the
   worst→best ordering (`orderbook__depth5`). The WS `orderbook_snapshot`
   uses the same ordering. The adapter MUST sort defensively, never assume.
2. **Margin WS signing path SETTLED**: sign `GET /trade-api/ws/v2/margin`
   (the URL path itself) — accepted 101 on
   `wss://external-api-margin-ws.demo.kalshi.co/trade-api/ws/v2/margin`.
   (Whether the event-API string would ALSO pass was not probed — primary
   worked, fallback never fired.)
3. **REST timestamp-skew window is ASYMMETRIC** (new — the event session
   never probed +30s): −5s, +5s and **+30s all PASS auth** (they reach the
   403 enablement gate); −30s, −5min, +5min are 401. So past-skew tolerance
   ∈ (5s, 30s] rejected at 30s, but future-skew tolerance ∈ [30s, 5min).
   Don't assume a symmetric window.
4. **Two distinct 401 bodies**, both nested-error shape: stale timestamp →
   `{"error":{"code":"header_timestamp_expired",...}}` (no details); wrong
   signature → `{"error":{"code":"authentication_error",
   "details":"INCORRECT_API_KEY_SIGNATURE"}}`.
5. **All three event-API error-body shapes recur on the margin surface**
   (consistent with fixtures/kalshi README finding 1): nested `{"error":{...}}`
   (auth, enablement, user_not_found), flat `{"code","message"}`
   (`subaccounts__create_nobody` → `invalid_content_type`), bare `{"msg":...}`
   (`funding__history_no_params`). Parse all three.
6. **`GET /margin/funding_history` has UNDOCUMENTED required query params**:
   `start_date` AND `end_date` (bare-msg 400s name them one at a time);
   ISO `YYYY-MM-DD` accepted → 200 `{"funding_history":[]}`. Research §8a
   lists no required params; OpenAPI doesn't either.
7. **`POST /portfolio/margin/subaccounts` requires a JSON content type even
   though OpenAPI declares no request body**: body-less POST → flat 400
   `invalid_content_type`; empty `{}` body → proceeds (to the 404
   `user_not_found` enablement wall). Always send `{}`.
8. **Error `code` strings can be DYNAMIC**: order/group create return
   `"code": "user_not_found:_f50604b8-…"` (user uuid embedded in the code);
   subaccount create returns static `"code": "user_not_found"` with **404**,
   while orders/groups use **400** for the same condition. Adapters must
   prefix-match codes and not key on status for this family.
9. **Enablement gating is INCONSISTENT across the private surface**: gated —
   `/margin/balance` (403), order/group/subaccount writes (400/404),
   `/portfolio/intra_exchange_instance_transfer` (403). NOT gated —
   `/margin/positions`, `/margin/orders` (+filters), `/margin/fee_tiers`,
   `/margin/notional_risk_limit`, `/account/limits/perps`,
   `/margin/funding_history` (all 200, empty), and the private WS channels
   (`user_orders`/`fill`/`order_group_updates` all subscribe OK,
   `ws__private_lifecycle.jsonl`).
10. **Ticker frame confirms the research §8b shape**: `funding_rate
    {rate, next_funding_time_ms, ts_ms}` + `reference_price` +
    `settlement_mark_price` + `liquidation_mark_price` + notional fields, all
    `_ms` epoch timestamps. **Funding rates are JSON NUMBERS (floats), not
    strings** — in the ticker frame, `/margin/funding_rates/estimate`
    (`funding_rate: 0`) and `/historical` (e.g. `-0.0009593552948286`).
    Dollars stay strings; rates don't.
11. **Funding grid confirmed**: historical entries at 04:00/12:00/20:00 UTC;
    `next_funding_time` 2026-06-12T04:00:00Z (REST estimate) =
    `next_funding_time_ms` 1781236800000 (WS).
12. **Demo BTC `tick_size: ""`** reproduced (§11.7); other markets show
    values. Parser must tolerate empty/absent.
13. **Demo catalog**: 17 markets, all `1`-suffixed + the two TEST-EQUITY
    perps (matches research §3 demo listing).
14. **Heartbeat**: ping payload `heartbeat` observed (private session, 1 ping).
    The BUSY public session saw **0 pings in 75s / 1712 frames** — pings are
    not guaranteed every 10s under flow; don't use ping cadence alone as a
    liveness signal on active connections.
15. **Rate-limit shape** (`account__limits_perps`): basic tier read
    bucket 400/400, write 100/100, `grants: []` — perps Read 400 at Basic
    matches research P12.
16. **`notional_risk_limit` default is `"0.0000"`** with no per-market
    overrides on this (unprovisioned) account — possibly an artifact of
    non-enablement; re-check after enablement before treating 0 as the real
    default.
17. **Aggregated orderbook buckets label by FLOOR on both sides**
    (`orderbook__agg_010`: best-ask bucket "6.3000" sits below the actual
    best ask 6.3416) — aggregated prices are display-only, never executable.

## Re-run checklist (after operator enables margin on the demo account)

1. `set -a; source .env; set +a` (demo pair only) and re-run the example from
   the repo root — it auto-detects enablement and runs the full §12 flow.
2. Verify the manifest's `open_funding_position` and leave it OPEN.
3. After the next 04:00/12:00/20:00 UTC funding time, capture
   `GET /margin/funding_history?start_date=...&end_date=...` (item 10 close-out).
4. Items 11/15/17 remain PROD/read-only follow-ups per the research doc.

## Re-run 2026-06-12 ~02:50Z (post margin-enablement, verifier session)

- Operator enabled perps on the demo account; rail finding: the intra-exchange
  transfer endpoint is LIVE on demo (research expected 4xx) — used to fund the
  margin subaccount ($50 via KINETICS_FUND_CENTICENTS=500000, run 1), then the
  full order-lifecycle family captured in run 2: create 201, duplicate
  client_order_id 409, amends (decrease + price), cancels, IOC fill.
- OPEN FUNDING POSITION (item 10): KXBTCPERP1 long 1.00 @ 6.3587, fee 0 —
  deliberately left open to cross the 04:00 UTC funding tick; capture
  funding_history after the tick. DO NOT CLOSE before then.
- WS private lifecycle: fill=true, order_group_updates=true, but
  user_orders=false across 21 frames — the user_orders channel never emitted
  during a live lifecycle; adapter must not assume it (fills arrived on the
  fill channel).
- Oddity: auth__margin_balance captured 0.0000 at run-2 start even though
  run-1's $50 transfer (200 + transfer_id) had completed and run-2 orders
  succeeded — transfer settlement appears async relative to the balance read;
  reconcile timestamps in the meta files during the adapter build.

## Item 10 disposition (2026-06-12 ~04:15Z)

Funding history is EMPTY after the 04:00Z tick because demo's funding_rate
is currently 0 (funding__rates_estimate.json: rate=0, next=12:00Z) — a zero
payment posts no entry. Two recorder fixes landed en route: the history
window is now dynamic (was hardcoded end_date=2026-06-11, stale after UTC
rollover), and the funding amount math is documented. DISPOSITION: position
stays open (zero carry cost at rate 0); future verification-loop firings
opportunistically re-capture when funding_rate != 0; the entry SHAPE can
alternatively come from the PROD read-only parity sweep (item 17, operator).
Item 10 = PARTIAL (blocked by venue state, not by us).

## Funding observation 2 (2026-06-12 12:05Z tick)

rate=0 again at the second observed tick; history still empty. Working
hypothesis: demo's funding engine is pegged at zero. Disposition: checks
downgrade to ~daily opportunistic; the funding_history ENTRY SHAPE comes
from the PROD read-only parity sweep (operator item 17, post-fee
activation). Position stays open (zero carry at rate 0).
