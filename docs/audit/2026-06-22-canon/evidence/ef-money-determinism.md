# Evidence: Areas E (Money/Numeric) + F (Determinism)

Audit of FORTUNA @ /Users/xavierbriggs/fortuna-wt-ws3. Read-only. Every claim cites `path:line`.
Spec: `docs/spec.md` (5.2 fee engine, 5.15 perps/PerpPrice). House rule: money = integer cents
(`Cents` i64); `Decimal` only at conversion boundaries; never f64 for money/price in core;
probabilities f64 in cognition only. All time via injected `Clock`.

---

## AREA E — Money and numeric handling

### Type definitions (as-built, matches spec/CLAUDE.md)

- `Cents(i64)` — `crates/fortuna-core/src/money.rs:35`. Checked arithmetic only
  (`checked_add/sub/mul/neg/abs/sum`, lines 48-88), each maps overflow to `MoneyError::Overflow`,
  never panics/wraps. Decimal→cents conversions are direction-explicit: `from_dollars_floor:91`,
  `from_dollars_ceil:97`, `from_dollars_exact:103`, `from_dollars_half_even:116` (only for
  documented banker's-rounding venues). Rounding-against-us is the documented contract (header
  lines 1-8). VERDICT: OK.
- `PerpPrice(i64)` ten-thousandths — `crates/fortuna-core/src/perp.rs:78`. Checked arithmetic
  (lines 91-112); `from_dollars_floor/ceil/exact` (114-139) at venue payload boundary only.
  `PerpValue` (162) carries price×qty in ten-thousandths; `to_cents_floor:192` (gains/PnL) and
  `to_cents_ceil:199` (costs/exposure) implement rounding-against-us via `div_euclid`/`rem_euclid`
  — integer, infallible, no float. `unrealized_pnl` floors (perp.rs:409), `notional_at` ceils
  (416). Type-level separation from `Cents` enforced (distinct newtypes). VERDICT: OK.

### Money / price / fee / size sites touching float

| Site | Type used | Money-path? | Verdict |
|---|---|---|---|
| `fortuna-state/src/sizing.rs:31` `kelly_binary(p:f64, price:Cents, fraction:f64)->f64` | f64 in, **Cents price in** | YES (sizing) | **LEGIT** — output is a *fraction* in [0,1], not money. `price` enters as `Cents`; only `price.raw() as f64` for the Kelly ratio (48-49). Validated finite & bounded (32-47). |
| `fortuna-state/src/sizing.rs:71` `kelly_contracts` `f*1_000_000.0` floor→i128 | f64→i128 ppm | YES (sizing) | **LEGIT** — documented single f64→integer boundary (55-57). Budget computed in widened i128 integer arithmetic (72); money never rides in float. Double-floor conservative. |
| `fortuna-venues/src/fees.rs` whole file | **Decimal** (no f64) | YES (fee engine) | **OK** — coefficients are decimal STRINGS in config (`taker_coeff:String` 32, `parse_decimal:296`), all math `Decimal`, result `Cents::from_dollars_ceil` (183). Rounding default `Up`=against us (50-54, 180-188); `HalfEven` only for documented venues. |
| `fortuna-venues/src/kalshi/adapter.rs:1193` `multiplier_decimal(raw:f64)` | f64→Decimal | YES (fee multiplier) | **LEGIT-BOUNDARY** — venue payload `fee_multiplier` is a JSON double (spec); converted to `Decimal` via shortest-decimal string (1201); non-finite/negative refused (1194). Observed values {0,0.5,1} round-trip exactly. |
| `fortuna-venues/src/kalshi/dto.rs:381` `fee_multiplier:f64` | f64 (DTO field) | boundary | **LEGIT** — raw venue payload field; consumed only via `multiplier_decimal` above. |
| `fortuna-venues/src/kinetics/dto.rs:471-472` `maker/taker_fee_rates: BTreeMap<_,f64>` | f64 (DTO) | YES (perp fee reconcile) | **LEGIT-BOUNDARY** — consumed at `adapter.rs:273` `Decimal::try_from(*rate)` (errors if not exact), then `notional_dollars × rate` in Decimal, `from_dollars_ceil` (287). Modeled-vs-charged reconciliation, ceil against us. |
| `fortuna-venues/src/kinetics/perp_observation.rs:67,180` `Decimal::try_from(funding_rate:f64)` | f64→Decimal | rate, not price | **LEGIT-BOUNDARY** — funding rate is a dimensionless fraction; exact `try_from` (errors on non-representable). Price fields use `parse_perp_price` (string), never f64. |
| `fortuna-venues/src/kinetics/dto.rs:563,442,455` `funding_rate/rate:f64`; `:166,170` `leverage_estimate(s):f64`; `:360 roe`, `:407-409 margin multipliers/thresholds:f64` | f64 (DTO) | rate/leverage boundary | **LEGIT-BOUNDARY** — raw venue JSON numbers; rates→Decimal at use; leverage→integer bps via ceil (see margin_sim below). Not prices. |
| `fortuna-state/src/margin_sim.rs:120` `(10_000.0/leverage).ceil() as i64` (bps) | f64→i64 bps | YES (maintenance margin) | **LEGIT-BOUNDARY** — documented f64-only-at-venue-boundary (119); output integer bps, CEILed (more margin = conservative). Leverage<1 / non-finite fail closed (114-117). |
| `fortuna-runner/src/perp_event_basis_v2.rs:788` `fair_cents_from_q` `(q*100).round() as i64` clamp[1,99] | f64 q→Cents | YES (edge claim) | **LEGIT-CONTROLLED** — the ONE documented f64→Cents mint (779-784). `q` is a kernel probability, screened finite∈[0,1]; result is `fair_value` (edge claim), NOT the order price; gates re-check net edge. Order `limit_price` is the venue best bid (`Cents`) at :1291. |
| `fortuna-runner/src/perp_event_basis.rs:283` `fair = limit.raw()+edge_premium_cents` | **i64** (no float) | YES (edge claim) | **OK** — pure integer; `edge_premium_cents:i64` (cfg). `limit_price=best_bid.price` (Cents, :280,300). |
| `fortuna-cognition/src/basis_v2.rs` (37 f64) + `basis.rs` (26) | f64 forecast | NO | **LEGIT** — explicitly "f64-cognition throughout … never money. There is NO money-type" (basis_v2.rs:1-13). Fair-probability kernel; no `Cents`/`PerpPrice`/IO/Clock. |
| `fortuna-live/src/daemon.rs:5550,5723` `market_p=(bid+ask) as f64/200.0` | f64 probability | NO (scoring) | **LEGIT** — bid/ask read as `best_bid_cents`/`best_ask_cents` (integer, :5544); f64 mid is a *probability* for a Brier-score baseline (:5551). Display/scoring only. |
| `fortuna-live/src/compose.rs:264,267,287,290` `fee_floor_dollars/min_basis_dollars/floor_dollars/cap_dollars:f64` | f64 (TOML config) | threshold/strike config | **SUSPECT (LOW)** — float dollars in config. `fee_floor_dollars`/`min_basis_dollars` are *threshold* dollars passed to the basis kernel (f64 forecast domain, :371-372). `floor/cap_dollars` are bracket STRIKE edges (286-290) → flow into `BracketStrike` for the f64 fair-prob model. These configure the f64 cognition layer (legit), but money-shaped quantities ("dollars") live as f64 in config rather than decimal strings — unlike `fees.rs` which mandates decimal strings (money.rs/fees.rs:31). They do NOT mint order prices (limit_price = venue best bid). See Open Questions. |
| `fortuna-ledger/src/repos.rs:2378,2464` `realized_value:f64` (scalar belief) | f64 | NO (forecast outcome) | **LEGIT** — `realized_value` is the resolved value of a SCALAR belief (e.g. a temperature/forecast quantity), not money; stored alongside `brier`/`clv_bps`/`p` (all f64 scoring/probability). |
| `fortuna-ledger/src/repos.rs` (remaining ~30 f64: `confidence`,`p`,`p_raw`,`brier`,`clv_bps`,`score`) | f64 | NO | **LEGIT** — probabilities, calibration, scoring metrics (cognition-side). |
| `fortuna-scoring/*`, `fortuna-cognition/*` (calibration, beliefs, persona, murphy, pit, pav, dm) | f64 | NO | **LEGIT** — calibration/Brier/CLV/PIT/isotonic scoring math; probabilities, not money. |

### Fee math determinism / against-us (spec 5.2) — CONFIRMED

- One engine `ScheduleFeeModel` (`fees.rs:93`); all arithmetic `Decimal`; coefficients are decimal
  strings (`fees.rs:31-33`, parsed `parse_decimal:296`) — **config never carries float fee coeffs**.
- Rounding `Up` by default = against us both directions (fees ↑, rebate magnitudes ↓), `ceil`
  (`fees.rs:180-184`). `HalfEven` only for venues that document banker's rounding (44-45, 185-187).
- Quadratic / FlatBps / Tiered all funnel to `Cents::from_dollars_ceil`/`half_even` (147-188).
- Per-fill reconciliation modeled-vs-charged: Kalshi `adapter.rs` + perps `kinetics/adapter.rs:260`
  `reconcile_fee` (notional×rate, `from_dollars_ceil:287`, mismatch ⇒ `FeeDiscrepancy`).
- Fill fees parsed against us: `kinetics/adapter.rs:241` fee `from_dollars_ceil`, realized_pnl
  `from_dollars_floor:246`. VERDICT: OK.

### Venue price parsing — CONFIRMED string→Decimal→newtype (never f64)

- Kalshi: `dto.rs:41 parse_dollars_to_cents_exact`, `:54` ceil variant → `Cents`.
- Kinetics perp: `dto.rs:52 parse_perp_price` (string→`PerpPrice::from_dollars_exact`), `:87
  parse_dollars`→Decimal. Prices NEVER ride in f64.

### E summary
- Float-on-money SUSPECTS: **0 hard defects.** 1 LOW-severity stylistic suspect:
  basis config money-shaped fields as f64 (`compose.rs:264,267,287,290`) — they feed the f64
  cognition kernel and never mint an order price, but are "dollars" living as float in config.
- All other f64 hits are LEGIT: probabilities/calibration/scoring (cognition), venue-payload
  boundaries with exact `Decimal::try_from` or string parse + ceil/floor against us, or metric
  display casts.

---

## AREA F — Determinism

### Clock trait (as-built, matches CLAUDE.md/spec)

- `pub trait Clock: Send + Sync { fn now(&self)->UtcTimestamp }` — `fortuna-core/src/clock.rs:163`.
- `RealClock` (168) is "the ONLY permitted wall-time read" — `Utc::now()` at `clock.rs:172`.
- `SimClock` (178) deterministic, monotone non-decreasing, rejects backwards time (`set:203-213`,
  `BackwardsTime` error). `UtcTimestamp` truncated to fixed ms precision for byte-identical replay
  (clock.rs:32-45). VERDICT: OK — single sanctioned wall-clock read.
- Decision loops carry the clock: `fortuna-cognition/src/cycle.rs:345,353 Arc<dyn Clock>`;
  `fortuna-exec/src/manager.rs:221,234 Arc<dyn Clock>`; `fortuna-live/src/daemon.rs:207,241,282,…
  Arc<dyn Clock>` (venue/read/paper/journal clocks all cloned from the one injected clock,
  daemon.rs:723,760,767,1050). VERDICT: OK.

### Wall-clock / RNG sites

| Site | Call | Inside Clock / sanctioned? | Verdict |
|---|---|---|---|
| `fortuna-core/src/clock.rs:172` | `Utc::now()` | YES — inside `RealClock::now` | **OK** (the one permitted read) |
| `fortuna-recorder/src/main.rs:39` | `SystemTime::now()` (`now_ms`) | NO — but B0 perishable-data recorder binary | **OK (non-decision)** — standalone fixture/data-capture tool; pure logic lives in lib.rs, the capture loop (network+wall clock) is isolated in main.rs (lib.rs:1-3). Not in any gate/exec/state/decision path; only stamps a `cycle_id` and capture timestamps for later replay. |
| `fortuna-recorder/src/main.rs:177` | `Instant::now()` | NO — recorder loop pacing | **OK (non-decision)** — measures cycle duration for the capture pacer; no decision/money effect. |
| `fortuna-ops/examples/rota_local.rs:79` | `SystemTime::now()` | NO — example binary | **OK** — `examples/` demo, not shipped decision path. |
| `fortuna-venues/examples/record_kinetics_fixtures.rs:66,356-358,790` | `SystemTime/Instant::now()` | NO — fixture-recorder example | **OK** — offline fixture capture tool. |
| `fortuna-venues/examples/record_kalshi_fixtures.rs:59,1160-1162` | `SystemTime/Instant::now()` | NO — fixture-recorder example | **OK** — offline fixture capture tool. |
| (decision crates: cognition/gates/exec/state/runner/live daemon) | — | n/a | **NONE** — no direct `*::now()` in any decision/gate/exec/state path; all time via injected `Clock`. |
| `fortuna-venues/src/kalshi/auth.rs:106` | `rand::rngs::OsRng` (`sign_with_rng` Pss) | n/a (crypto) | **OK** — RSA-PSS request-signing salt; CSPRNG nonce required by the signature scheme; not a decision input (101-106). |
| `fortuna-venues/src/kalshi/ws_transport.rs:314` | `OsRng` `RsaPrivateKey::new` | n/a (test) | **OK** — inside `fn test_transport()` test helper (keygen); not shipped code. |
| `fortuna-venues/examples/record_kinetics_fixtures.rs:83` / `record_kalshi_fixtures.rs:67` | `rand::random()` ([u8;16]) | n/a (example) | **OK** — random id in offline fixture-recorder examples. |

### RNG findings
- No RNG in any decision/gate/exec/state/cognition path. The only `rand` dep is `fortuna-venues`
  (Cargo.toml:19), used solely for (a) RSA-PSS crypto signing nonce (`auth.rs`, CSPRNG, required),
  (b) a test helper, (c) offline fixture-recorder example ids. No unseeded RNG affects any decision,
  so backtest/shadow reproducibility is not at risk from RNG.

### F summary
- Non-Clock time reads in DECISION paths: **0.** The only `SystemTime/Instant::now()` outside
  `RealClock` are in the standalone recorder binary and `examples/` fixture tools (non-decision,
  data-capture). All decision loops carry `Arc<dyn Clock>`.
- RNG: 0 in decision paths; all uses are crypto-signing / test / fixture-example.

---

## Open questions
1. `compose.rs:264,267,287,290` — `fee_floor_dollars`/`min_basis_dollars`/`floor_dollars`/
   `cap_dollars` are money-shaped TOML fields typed `f64`. They configure the f64 basis-cognition
   kernel (forecast domain) and do not mint order prices, so not a money-path defect. But the
   house rule says money lives in integer cents / decimal strings in config (cf. `fees.rs:31`
   "never floats in config"). Is float-dollar config for strike edges an intentional cognition-
   side exception, or should strike/threshold dollars be decimal strings → `Cents`/`PerpPrice`
   to avoid a sub-cent strike rounding surprise feeding the fair-prob model? (Stylistic; LOW.)
2. Recorder `main.rs:39,177` and `examples/*` use raw `SystemTime/Instant::now()`. These are
   out of the deterministic core by design (data capture, not replay), so no `Clock` is wired.
   Confirm the recorder is permanently excluded from the deterministic-replay boundary (it
   appears to be: it produces the fixtures that the deterministic core later replays).
