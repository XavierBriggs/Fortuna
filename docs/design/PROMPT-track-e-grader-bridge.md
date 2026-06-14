# Track E handoff — wire the weather scoring bridge (close the loop)

You are TRACK E (the Aeolus weather→belief pipeline, F5–F9). This is a focused,
ready-to-build task: **wire the live resolution bridge so Aeolus weather beliefs
are scored against the realized NWS temperature.** Every piece you need is now on
`main`; the only thing missing is the loop that connects them.

## Why this exists

Today the forecast side runs end-to-end (F5–F8 + the F7 `drive()` weather wiring
in `crates/fortuna-live/src/aeolus_venue.rs`): Aeolus forecasts are matched to
Kalshi temperature-bracket markets and emit beliefs. But **no belief is ever
scored against reality** — F9 (`score_reliability`) is only called in tests, with
a hardcoded `let realized = 88.0_f64;`. Until something supplies the *real*
realized temperature, the whole weather edge is unmeasurable (and §5.12 forbids
unscoreable beliefs).

Track D built the missing producer: the **grader**. Your job is the **bridge**
between the grader and F9.

## The seams (all on `main` — do not rebuild)

- **Producer (Track D, `fortuna-sources`):**
  `nws_cli_realized(product_text: &str, station: &str) -> Option<RealizedExtreme>`
  where `RealizedExtreme { station: String, report_date: String, high_f: i64, low_f: i64 }`.
  Pure, deterministic, FAIL-LOUD — returns `None` on any ambiguity (jammed column,
  missing `MM`, absent line, inverted high<low, unparseable date). It NEVER
  fabricates a temperature. `None` ⇒ that day is **unscoreable**.
- **Consumer (F9, `fortuna-cognition`):**
  `aeolus_reliability::score_reliability(fc: &AeolusForecast, realized_f: f64) -> AeolusReliability`
  — per-bracket Brier (`per_bracket[].{event_id, outcome, brier}`) + scalar CRPS.
  Already takes a plain `f64`; you supply it.
- **Persistence (`fortuna-ledger`):**
  `BeliefsRepo::resolve_and_score(belief_id: &str, outcome: bool, brier: f64, clv_bps: Option<f64>)`.
- **The realized signal:** ingestion persists `nws.cli` signals to the signals
  store (payload: `productText`, `report_date` = `YYYY-MM-DD`, `issuingOffice`,
  `issuanceTime`). The `nws_climate` feed is admitted in `source_registry`
  (tier 10 — seeded locally by Track D; the prod seed SQL is in
  `docs/runbooks/ingestion-ops.md`).
- **The forecast:** `AeolusForecast` carries `station()`, `variable()` →
  `Variable::{Tmax, Tmin}`, and `target_date()`.

## The task — a weather resolution loop

Mirror the EXISTING analog `resolve_and_score_funding_beliefs(...)` in
`crates/fortuna-live/src/daemon.rs` — it is the same shape for funding beliefs.
Add a `resolve_and_score_weather_beliefs(...)` (name yours) that, on each
resolution tick, for every open Aeolus weather belief whose `target_date` has
passed (`now >= settles_after`):

1. **Find the realized product.** Look up the persisted `nws.cli` signal whose
   `report_date == target_date` for the belief's **grading station**. NOTE: the
   grading station (the event's `nws_station_id`, e.g. `"NYC"`) is DISTINCT from
   the Aeolus forecast station (e.g. `"KNYC"`). See "Station routing" below.
2. **Grade it:** `let re = nws_cli_realized(&product_text, station)?;`
3. **`None` ⇒ skip.** The day is unscoreable — leave the belief OPEN, emit a
   telemetry/log line, move on. NEVER fabricate or fall back to a derived value.
   (Independence: the realized value is NWS, never Aeolus — the V4 self-grading
   caution. The grader is the forecaster's judge.)
4. **Pick the variable:**
   `let realized_f = match fc.variable() { Variable::Tmax => re.high_f as f64, Variable::Tmin => re.low_f as f64 };`
5. **Score:** `let rel = score_reliability(&fc, realized_f);`
6. **Resolve each bracket belief:** for each `b` in `rel.per_bracket`, map its
   `event_id` (`aeolus:{...}`) back to the persisted `belief_id` and call
   `resolve_and_score(belief_id, b.outcome, b.brier, clv)`.
7. **Persist the Layer-3 reliability** (the per-`(model, scope)` scorecard the
   ROTA V4 board reads) wherever your scorecard store lives.

## Station routing (the one open sub-problem — ledgered)

Matching a grading station to the right CLI office's `nws.cli` signal is the
remaining piece. The CLI product's `issuingOffice` / header identifies the
office; you need a `grading_station → CLI office/station` map, grounded the same
way the F7 `aeolus_venue.rs` station→series map is (cited, not guessed). The
grader assumes a SINGLE-station product (true for all committed fixtures); a rare
multi-station CLI would need a site-section split — defer that, ledger it.

## Tests-first (the gate)

- **Replace the stub.** `crates/fortuna-ledger/tests/aeolus_e2e.rs` currently does
  `let realized = 88.0_f64;`. Replace it with a real grade:
  `let realized = nws_cli_realized(&product_text, "KPDX").map(|r| r.high_f as f64).unwrap();`
  over a committed fixture (`fixtures/sources/nws_climate/cli_product_troutdale.json`
  → high 91), so the e2e grades against a REAL parsed value end-to-end.
- **None-path test:** a jammed/missing product (the existing `cli_product.json`,
  PTKR `MINIMUM 7676`) ⇒ the belief stays UNRESOLVED, never mis-graded.
- Full battery: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace`, `scripts/run-dst.sh`. Ledger in GAPS; tick
  the BUILD_PLAN box. The verifier gates on the merged tree, mutation-proven.

## Ownership / coordination

- The resolver logic is yours (`fortuna-cognition`); the loop trigger lives in
  `fortuna-live` `drive()` — a small, additive touch next to the F7 weather wiring
  + `resolve_and_score_funding_beliefs`. **Coordinate that `drive()` touch with
  Track A** (they own the daemon loop), exactly as the funding resolver did.
- Pointers: GAPS "TRACK D — NWS-CLI realized-extreme GRADER" (the canonical bridge
  recipe + the 3 handoffs), `aeolus_reliability.rs` (F9), `aeolus_venue.rs` (F7
  match + the station map pattern), `aeolus_e2e.rs` (the e2e to extend),
  `resolve_and_score_funding_beliefs` in `daemon.rs` (the analog loop to mirror).

Once this lands, the weather loop is closed end-to-end: forecast → belief → trade
→ **graded outcome → measured reliability**. That measured reliability — not
Aeolus's self-report — is what tells us whether the weather edge is real.
