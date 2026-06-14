# Kalshi demo-flip — design (track C)

Status: **Phase 1 DONE** (commits: de4d2d8 `Venue::account()` + the
`SimRunner<V: Venue = SimVenue, J>` generalization — landed gate-clean; the DST
corpus is byte-identical, so A3 holds). **Phase 2** (`compose_kalshi_runner` +
boot gate + `ActiveRunner`) is buildable now; its *live run* is operator-blocked
(demo credentials + the T4.2 fixture checklist). Explore-traced + architect-validated
2026-06-14. Authority chain: docs/spec.md > CLAUDE.md > this doc.

## Goal

Let `fortuna-live` run a **Kalshi DEMO** (mock funds) at `Stage::Paper`,
pre-promotion, while prod/live stays **REFUSED at the boot gate**. The operator
flips the demo by config (`[daemon] venue = "kalshi", stage = "paper"`) + demo
env credentials; the daemon never auto-promotes (I7).

## Hard invariants (non-negotiable)

- **A3 / sim path byte-unchanged.** `SimRunner<SimVenue, J>` (the post-refactor
  default) behaves IDENTICALLY to today's `SimRunner<J>` — same determinism, same
  DST recordings, same `tick()` byte-output. The full DST corpus stays green.
- **Venue-independent core.** No Kalshi specifics leak into the generic runner.
- **`crates/fortuna-invariants/` is ADD-ONLY** — never weaken/modify an existing test.
- Demo at `Stage::Paper`; LiveMin/Scaled REFUSED at boot. Promotion is a human
  action through the forward-validation gate (I7).
- House style: no panic/unwrap/expect in money paths; all time via the injected
  `Clock`; secrets only via env (never config/logs); clippy `-D warnings` clean.
- **Tests NEVER hit the real Kalshi API** — only `MockKalshiTransport` / fixtures.

## Key finding: the Kalshi adapter already exists

`crates/fortuna-venues/src/kalshi/adapter.rs` (`KalshiVenue`) is **fully
`Venue`-trait-complete** (all 11 methods), with a demo base URL
(`KALSHI_DEMO_BASE_URL`), RSA-PSS request signing (`KalshiSigner`), and fee
reconciliation. It is "cleared for Sim dev only until operator fixture clearance"
— i.e. CODE-complete, evidence-blocked. So the demo-flip work is the **runner
generalization + composition + boot gate**, NOT a new adapter.

## The four hard decisions (resolved)

1. **`inspect_totals` → add `account()` to the `Venue` trait.** `inspect_totals`
   (SimVenue-only; `(cash, reserved, fills, pending)`) is called 3× in the runner
   (`runner.rs` ~1390/1532/2497). Add to the trait:
   `async fn account(&self) -> Result<(Cents, Cents), VenueError>` (cash, reserved).
   - SimVenue: delegates to `inspect_totals().0/.1` (numbers byte-identical).
   - KalshiVenue: cash from `balance()`; reserved `= Cents::ZERO` at Paper (the
     real venue has no reservation-ledger API — honest, not fabricated; the
     open-orders-sum derivation is a GAPS follow-on).
   - PaperVenue: cash from `balance()`, reserved ZERO. Polymarket/Kinetics stubs:
     `Err(FixtureGated)`.
   - The 3 runner callsites become `self.venue.account().await?`. `report()`
     becomes `async` (blast radius — update the ~6 test callsites).
   - The DST harness (`fortuna-core/tests/dst.rs`) calls `inspect_totals` directly
     on a `SimVenue` (not via the runner) — UNAFFECTED, stays byte-identical.

2. **Keep `clock: Arc<SimClock>` (concrete) — do NOT generalize the clock.** The
   `RealCadence` already advances a `SimClock` by wall-elapsed ms → real-time
   tracking for a paper/live runner; DST uses a scripted cadence → deterministic.
   `run_loop.rs` only *reads* `runner.clock.now()` (a `Clock` trait method).
   Generalizing the clock buys nothing and widens the blast radius. A3 holds.

3. **Real HTTP in `tick()` is not a blocker at Paper.** `tick()` is already
   `async` and always awaited `venue.book()/fills_since()/...`. For KalshiVenue
   those do real HTTP; just set a longer `tick_interval_ms` (≥5000) for Kalshi.
   Outages already return `VenueError` and are handled (continue-not-crash).

4. **Venue-injecting constructor.** Generalize to
   `SimRunner<V: Venue = SimVenue, J: IntentJournal + Send = MemoryJournal>`,
   field `venue: V`. New constructor:
   `new_with_venue(config, strategies, audit, start, journal, venue: V, clock: Arc<SimClock>, allowed_stages: &[Stage])`.
   `new_with_journal` stays (builds SimVenue internally, passes `&[Stage::Sim]`).
   `RunnerConfig.faults` becomes `Option<FaultConfig>` (SimVenue-specific; `None`
   on the injected-venue path). Extract the stage-allowlist check into a helper.

## Routing type-mismatch → an `ActiveRunner` enum

`compose_runner` returns `SimRunner<SimVenue, PgIntentJournal>`;
`compose_kalshi_runner` returns `SimRunner<KalshiVenue, PgIntentJournal>` — two
different types. `drive()` + the segment helpers need one concrete type. Solution
(minimal blast radius, no `Box<dyn>`/object-safety issues): an enum in `daemon.rs`

```
pub enum ActiveRunner { Sim(SimRunner<SimVenue, PgIntentJournal>),
                        Kalshi(SimRunner<KalshiVenue, PgIntentJournal>) }
```

with a small `impl ActiveRunner` delegating the methods `drive()` needs (tick,
shutdown, drain_pending_*, refresh_edges, counters, boards_json, report, …).
`drive()` takes `&mut ActiveRunner`; `main.rs` builds the right variant by
`dcfg.daemon.venue`.

## Phase 1 — `SimRunner<V, J>` generalization (the safe foundation)

Self-contained; lands gate-clean; sim path byte-identical. Files + steps:

- `fortuna-venues/src/lib.rs`: add `account()` to the `Venue` trait.
- `fortuna-venues/src/sim.rs`, `kalshi/adapter.rs`, `polymarket/`, `kinetics/`,
  `fortuna-paper/src/lib.rs`: add `account()` impls (per decision 1).
- `fortuna-runner/src/runner.rs`: `RunnerConfig.faults → Option`; struct gains
  `V: Venue = SimVenue`; split impls (`SimRunner<SimVenue, J>` for
  `new_with_journal`; `SimRunner<V, J>` for `new_with_venue` + shared methods);
  `venue() -> &V`; 3 `inspect_totals` → `account().await?`; `report()` async;
  `check_stage_allowlist` helper.
- `fortuna-live/src/run_loop.rs`: `run_loop<V: Venue, J>` (body unchanged).
- `fortuna-live/src/daemon.rs`: cosmetic return-type/annotation updates;
  J-generic helpers (`registry_from`, digests, `send_alerts`) gain `V: Venue`.
- Tests: update `SimRunner<PgIntentJournal>` annotations → `SimRunner<SimVenue,
  PgIntentJournal>`; `RunnerConfig { faults: Some(..) }`; `report().await`.
- NEW tests: `account()` ≡ `inspect_totals` (fortuna-venues); a type-level A3
  check (`SimRunner::<SimVenue, MemoryJournal>::new` still resolves);
  ADD-ONLY invariant `i7_sim_runner_new_still_refuses_paper_staged_strategies`.
- Gate: fmt, clippy -D warnings, test --workspace, run-dst (2000) — all green.
- Risk: `report()`-async + `RunnerConfig.faults→Option` are the wide-blast-radius
  items (compiler-checked).

## Phase 2 — `compose_kalshi_runner` + boot gate

- `boot.rs`: `DaemonSection` gains `stage: String` (default `"sim"`); a
  `KalshiSection { series, bracket_sets }` + `DaemonToml.kalshi: Option<..>`.
  `validate_bootable` kalshi arm: `stage="paper"` allowed; `sim`/`live_min`/
  `scaled` refused; require `[kalshi]` non-empty. `venue="sim"` requires
  `stage="sim"`.
- `daemon.rs`: `compose_kalshi_runner(...)` — reads `KALSHI_API_DEMO_KEY_ID` +
  `KALSHI_DEMO_PRIVATE_KEY_PATH` from env (the established recorder convention:
  the path is routing data, the file CONTENT is the secret, read + Secret-wrapped
  here, never logged); builds
  `KalshiSigner` + `ReqwestKalshiTransport(KALSHI_DEMO_BASE_URL)` + `KalshiVenue`;
  `RealClock`-driven `SimClock`; `Stage::Paper`; real Kalshi markets from
  `[kalshi]`; `new_with_venue(..., &[Stage::Sim, Stage::Paper])`. Plus the
  `ActiveRunner` enum + delegation.
- `main.rs`: route `compose_runner` vs `compose_kalshi_runner` by `daemon.venue`;
  wrap in `ActiveRunner`.
- `config/fortuna.example.toml`: `[daemon] stage = "sim"` + a commented `[kalshi]`.
- NEW tests (no live network): `boot_gate.rs` (kalshi@paper boots; live_min/
  scaled refused; sim+paper refused; missing `[kalshi]` refused);
  `kalshi_compose.rs` (`compose_kalshi_runner` against `MockKalshiTransport`,
  asserts venue id "kalshi" + Stage::Paper); ADD-ONLY invariants
  (`new_with_venue` accepts Paper, refuses LiveMin/Scaled).

## Invariant preservation

I1 (gate still constructed from `config.gate_config`; every order through the
pipeline), I2 (drawdown via `account()` → halt), I5 (audit sink injected,
venue-agnostic), I6 (no Venue method takes model output), I7 (`allowed_stages`
checked at construction; the existing `SimRunner::new` Paper-refusal test is
UNTOUCHED — `new()` still routes through `&[Stage::Sim]`). DST harness touches
`SimVenue` directly → byte-identical.

## Operator-blocked (code builds; live run waits)

1. Demo credentials: `KALSHI_API_DEMO_KEY_ID` + `KALSHI_DEMO_PRIVATE_KEY_PATH`
   (operator generates on the Kalshi demo portal, saves the PEM to a gitignored
   file → `.env` points the path var at it). Same two vars the recorders read.
2. Fixture clearance (GAPS Kalshi section, T4.2): the 27-item checklist must close
   before the daemon runs against the real demo API.
3. `[kalshi].series` tickers (from a demo-account inspection).
4. KalshiVenue `account()` reserved=0 → derive from `open_orders()` once the
   per-order fee model is fixture-confirmed (GAPS follow-on).
