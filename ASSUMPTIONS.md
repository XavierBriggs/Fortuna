# ASSUMPTIONS.md (agent-maintained)

Every decision made where docs/spec.md is silent: what was assumed, why it is the
conservative option, and the spec section it interprets.

## T0.6 — order manager

- **`Venue::open_orders()` added to the trait** (5.2 sketch omits it; 5.4 boot
  reconciliation cannot exist without it).
- **Boot does not auto-resubmit.** Intents with no venue evidence after a
  crash (Created-only, or Submitted with nothing at the venue and no fills)
  are closed (`BootClosed`); strategies re-propose through the gates against
  CURRENT state. Auto-resubmitting a stale intent would honor a gate verdict
  issued against a dead market state. The idempotent-coid machinery still
  protects the case where the order DID reach the venue (AlreadyExists ->
  adopt).
- **Orphan venue orders at boot are cancelled and reported** (spec's "adopt
  orphans" sentence defines adoption as cancel + alert; we follow it
  literally).
- **Late fills are applied to Cancelled AND BootClosed intents** (status
  unchanged, flagged): venue truth arrives late and reality always wins;
  rejecting it would desync positions. Fills against Rejected intents remain
  illegal (a clean venue reject cannot have fills; if one appears it is a
  venue discrepancy and must error).
- **Resubmitting an intent the manager already knows is Acked short-circuits**
  to the known venue order id without a venue call (manager-level
  idempotency, mirroring the venue's coid idempotency).
- **Cancel of a Submitted-unknown intent (no venue id) is refused**; the
  caller reconciles first. Cancel Timeout leaves status unchanged
  (CancelOutcome::Unknown) for the next sweep to retry.
- **TTL is measured from intent creation** (created_at), per-strategy
  configurable with a default; sweep skips Submitted-unknown intents (no
  venue id to cancel).
- **Group unhedged-notional (v1)** = spread between the most- and
  least-filled legs' filled notional; completion economics use a declared
  `value_per_set` walked against current books at taker, all-in with fees;
  insufficient depth or a missing book always unwinds (cannot price
  completion honestly = do not chase it).
- **Flatten planner** marks at the touch and walks visible depth net of
  taker fees; any unfillable remainder forces FreezeAndCancel regardless of
  the auto bound.

## T0.5 — gate pipeline

- **A rate-limit breach halts the VENUE for both bucket kinds** (venue bucket
  and per-market bucket). Spec 5.3's halt taxonomy (check 1) knows global/
  strategy/venue scopes only; a market-scoped runaway is still a runaway on
  that venue, so the venue halt is the conservative mapping. The reason
  string names the breaching bucket.
- **Orders rejected before check 7 consume no rate tokens.** The limits
  protect venue API submissions; an order that never got that far is not a
  submission. (Pinned by test.)
- **Sells contribute zero worst-case exposure** in checks 2/3/9: venue
  semantics are close-only (T0.3), so a sell can only reduce exposure.
  Re-verified against fixtures in T1.1.
- **Edge floor mechanics (check 6):** worst-case fee = max(maker, taker, 0)
  at the limit price; pass iff net >= 0 AND floor(net x 10000 / notional) >=
  min_net_edge_bps (spec's parenthetical defines reject when < threshold);
  bps floor-division rounds against us; `fair_value` is in the candidate's
  own side space, like its limit price.
- **Check 9 with no event mapping** is config-driven (`require_event_mapping`,
  default false until discovery exists in Phase 3): when off, the order
  passes with an audit note that the cap could not bind; when on, reject.
  Fail-closed-by-default would block ALL trading before Phase 3, which is
  why the operator chooses.
- **Price sanity reference** = book mid (or the single-sided touch), else
  last trade; no reference at all -> reject. A book whose market differs from
  the order's market -> reject (fail-closed against caller bugs).
- **Hot reload preserves halts unconditionally** (a config push must never
  re-arm anything) and reinitializes rate buckets at full burst (operator-
  initiated; acceptable).
- **I1/I3 invariant stubs implemented** per the protected README's sanctioned
  path (the owning task removes #[ignore] and writes the real assertions;
  nothing weakened, names preserved, compile-fail half added as doc-tests).

## T0.4 — DST harness

- **Master entropy comes from `RealClock` unless `DST_MASTER_SEED` is set**,
  and is always printed. Randomized novelty per run is the point (Antithesis/
  VOPR pattern); reproducibility comes from the printed per-scenario seed,
  not from a frozen corpus. RealClock is the one legal wall-time source.
- **Quiesce-phase venue calls retry through INJECTED TRANSIENT errors**
  (bounded). A transient API error is retryable by definition; failing a
  scenario because the last poll randomly faulted would be harness noise,
  not a system defect. Explicit outage windows are ended by advancing the
  sim clock first.
- **Phase-0 invariant set** asserted per scenario: I-money (venue cash equals
  a replay of the dedup'd fill stream + payouts), I-reserve (no leaked
  reservations after cancel-all; cash never negative; reserved never exceeds
  cash), I-position (fill-derived == venue), I-delivery (no fill lost to
  cursor mechanics), I-determinism (every scenario runs twice; traces must be
  byte-identical). Order-journal/crash-recovery scenarios extend this at
  T0.6 per BUILD_PLAN; gate scenarios at T0.5.

## T0.3 — venues, fees, sim venue

- **`Venue::balance()` added to the trait.** The spec's 5.2 trait sketch omits
  it, but 5.4/5.14 make venue balances authoritative for reconciliation, so
  the trait must surface them. It returns AVAILABLE (unreserved) cash, which
  is what venues report and what affordability means.
- **`fills_since` returns a `FillPage` (fills + next_cursor)** instead of the
  sketch's bare `Vec<Fill>`: a cursor protocol needs the venue to hand back
  the next cursor, and choosing it venue-side lets delivery be honestly
  at-least-once (late and duplicate delivery arise naturally; consumers dedup
  on `fill_id`).
- **Duplicate client order ids are refused with `AlreadyExists{existing}`**,
  mirroring Kalshi's ORDER_ALREADY_EXISTS (researched 2026-06-09, official
  API docs). Exec treats it as success-equivalent on resubmission. The sim
  venue models venue-faithful behavior rather than silently returning Ok.
- **Sim venue sells are close-only** (rejected beyond held position net of
  already-working sells), modeling Kalshi semantics; to be re-verified
  against fixtures in T1.1.
- **Sim venue buys reserve worst-case cost** (limit x qty + max(maker,taker)
  fee at limit) at accept; exact reserved amounts are stored and released
  verbatim (never recomputed), so fee-schedule changes can't drift the ledger.
- **Fee rounding model: ceil per fill by default.** Kalshi's PDF rounds up
  per trade total and its engine uses a per-order accumulator at $0.0001
  precision; ceil-per-fill is the conservative model per the research doc.
  `half_even` mode exists only for venues that DOCUMENT banker's rounding
  (Polymarket US). Maker coefficients may be negative (per-fill rebates are
  real: Polymarket US theta = -0.0125); taker negatives are config errors.
  Under "up" rounding, rebate magnitudes round DOWN (ceil of a negative):
  against us in both directions.
- **Per-category coefficient tables** (Polymarket Intl 0.03-0.07 by category)
  are expressed by instantiating one schedule per category at the adapter
  level (T3.4), not by a third config mechanism; `category_multipliers`
  covers the Kalshi-style scaling case.

## T0.2 — deterministic bus + replay

- **Recorded time is authoritative during replay.** The replayer drives a fresh
  SimClock from each recorded event's stamp before dispatching it, instead of
  trying to reproduce the original harness's clock-advance pattern. Spec 5.1
  requires byte-identical replay but is silent on clock reconstruction; this is
  the conservative reading (replay can never falsely diverge because of clock
  bookkeeping, and a corrupt recording with backwards stamps fails loudly).
- **Fail-closed handler-error semantics, pinned by test:** a handler error stops
  dispatch immediately, the erroring handler's pending publishes are discarded,
  the failing event remains in the recording (audit truth: it WAS dispatched),
  and the bus error is fatal to the run (the runner halts; no resume API).
  Spec 5.1/Section 9 imply fail-closed but don't specify outbox disposition;
  discarding is conservative (no half-processed derived state).
- **`EventPayload` starts with only a `Raw{kind,data}` variant.** Typed variants
  are added by the tasks that own them (venue events in T0.3, gate verdicts in
  T0.5, ...). Conservative: inventing the full event taxonomy now would
  pre-commit downstream contracts the spec assigns to later sections.
- **Handler ids are unique per bus** (subscribe rejects duplicates): event
  origin attribution and replay identity depend on stable, unambiguous ids.

## T0.1 — fortuna-core foundations

- **Timestamp precision is fixed at milliseconds** (`YYYY-MM-DDTHH:MM:SS.mmmZ`),
  truncated at construction. Spec/conventions say "UTC ISO8601" but are silent on
  precision. Fixed precision makes serialization byte-identical (replay/audit
  determinism is load-bearing, spec 5.1/I5), and ULIDs are millisecond-granular, so
  nothing in the system can act on finer time anyway. Truncating at construction
  (rather than at serialization) guarantees the in-memory value always equals its
  wire form.
- **SimClock is monotone non-decreasing**; `set()` backwards is an error and
  `advance`/`set` leave time unchanged on error. Spec is silent on sim-clock
  semantics. Conservative because replay determinism assumes a forward-only sim
  time; a test that needs backwards time must model it explicitly (e.g. venue
  timestamp skew as data, not as the injected clock).
- **Id generation uses an in-house SplitMix64 PRNG** (pinned by published test
  vectors) instead of the `rand` crate. Spec is silent on the PRNG. `rand`'s small
  RNGs make no cross-version/cross-platform byte-stability promise, and id
  determinism feeds the bus/audit/replay chain; owning 10 lines of pinned PRNG is
  the conservative option.
- **IdGen monotonicity policy** (ULID spec interpretation): within one millisecond
  the 80-bit random part increments; if the injected clock reads backwards, the
  generator clamps to its high-water-mark millisecond (ids never duplicate or
  reorder); pre-1970 or >= 2^48 ms timestamps are errors; random-part exhaustion
  within one millisecond is an error, never a silent wrap. Erroring over wrapping
  is conservative: a wrap would silently break id ordering, which downstream
  consumers (audit, journal) are allowed to rely on.
