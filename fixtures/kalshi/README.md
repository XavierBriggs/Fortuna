# Kalshi fixtures — OPERATOR-RECORDED (demo environment)

Recorded 2026-06-11 ~06:28–06:33 UTC by `crates/fortuna-venues/examples/
record_kalshi_fixtures.rs` (operator-authorized session, demo credentials,
mock funds). Naming: `<area>__<case>.json` = VERBATIM response body;
sibling `.meta.json` = method/path/status/sanitized request body/checklist
note; `session__manifest.meta.json` = the full 60-row session record.
`ws__*.jsonl` = verbatim WS text frames, one per line, replayable straight
into `KalshiWsParser`. No request headers or key material are recorded.

Checklist coverage: docs/research/venue/kalshi-api-2026-06-10/research.md
§Uncertainties (27 items). Traded market for the session:
KXWTACHALLENGERMATCH-26JUN11JIMLEP-LEP (most-liquid two-sided open market
at session time; also the settlement seed).

## Load-bearing wire findings (where the wire diverges from the docs)

| # | Finding | Fixture |
|---|---|---|
| 1 | **Error envelope is NESTED**: `{"error":{"code","message","details"?,"service"?}}` — the OpenAPI's FLAT `ErrorResponse` does not occur on the wire (auth layer AND API layer). Adapter must parse the nested shape. | every 4xx capture |
| 2 | Timestamp skew tolerance: ±5s accepted, −30s and ±5min rejected → window is >5s and <30s | auth__skew_* |
| 3 | Duplicate `client_order_id` → 409 code string **`order_already_exists`** | orders__duplicate_client_order_id |
| 4 | A CANCELED order's client_order_id does NOT free up (409 on reuse) — client ids are permanent per account | orders__reuse_canceled_client_id |
| 5 | Cancel of already-canceled / executed / unknown order → **404** (not 200-with-zero) | orders__cancel_already_canceled / _executed / _unknown_id |
| 6 | `post_only` that would cross is **rejected at create**: 400 `invalid_order` / details "post only cross" (docs describe 201-then-cancel w/ PostOnlyCrossCancel; demo runs the newer build) | orders__post_only_cross |
| 7 | V2 create REJECTS numeric (non-string) `count`/`price` | orders__numeric_field_types |
| 8 | `limit=1001` → hard 400 (no clamp); GARBAGE cursor → silent 200 (no error — footgun) | markets__limit_over_max / __garbage_cursor |
| 9 | Taker fee from a real fill: price 0.52, fee `0.017500` = ceil-against-us of 0.07×P×(1−P) — quadratic ×0.07 confirmed | orders__create_v2_taker_ioc + fills__after_taker |
| 10 | Units locked: Balance int cents (TRUNCATED) + `balance_dollars` 4dp string; Fill `fee_cost` 6dp dollars string; `count_fp` strings | auth__balance_ok, fills__after_taker |
| 11 | Pagination last-page signal: `cursor: ""` (empty string, present) | fills__after_taker, markets__single_filter_lastpage |
| 12 | STP `taker_at_cross` self-cross: taker created 201 with fill_count 0.00 AND remaining 0.00 (canceled, no self-fill); resting order untouched | orders__stp_self_cross + __stp_resting_after |
| 13 | WS: signed handshake on `/trade-api/ws/v2` accepted (101) on both flag states; server pings ~10s cadence (8 in 90s) | ws__orderbook_trade_yes/.meta, _noleg |
| 14 | Insufficient balance → 400 (body recorded) | orders__insufficient_balance |
| 15 | Both demo hosts accept the same signature (path-only signing confirmed) | auth__balance_alt_host |

## Known gaps left open by this session (tracked in GAPS.md)

- Settlement record: seed position placed (orders__settlement_seed); re-poll
  `GET /portfolio/settlements` after the market closes and add the capture.
- VOIDED market settlement: cannot be forced; capture when one occurs.
- Series fee fields (`series__base`): the demo market object carried no
  `series_ticker` — fetch via event lookup in a follow-up; fee MATH is
  already confirmed from the real fill (finding 9).
- Prod-parity re-record (checklist #26) and a real maintenance-window
  `GET /exchange/status` (#27): before first live use.
- WS capture is from a quiet market (5–7 frames: subscribed + snapshot +
  deltas). Contract shapes confirmed; a busy-market capture lands with the
  perps session.

`crates/fortuna-venues/tests/kalshi_doc_samples/` remains the DOC-DERIVED
set used to build the adapter pre-fixtures; retire entries only as adapter
tests are re-pointed at the recordings here (T4.2).
