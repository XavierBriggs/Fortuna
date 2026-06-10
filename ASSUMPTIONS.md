# ASSUMPTIONS.md (agent-maintained)

Every decision made where docs/spec.md is silent: what was assumed, why it is the
conservative option, and the spec section it interprets.

## T1.3 — model veto scaffolding (mech_extremes lands with the volume field)

- **Reduce-only is enforced by TYPE, not policy.** `VetoVerdict` has no
  grow variant; `KeepBps` is constructor-bounded to 1..=9999 (0 = say
  Suppress; 10000 = say Allow; more would be growth) and serde round-trips
  through the checked constructor, so an audit-log replay cannot smuggle an
  out-of-range factor back in. Shrink application floors
  (`floor(qty x keep / 10000)`), proptested never to exceed the input.
- **The veto sits AFTER sizing, BEFORE the gates.** Spec Section 6 says the
  veto can "suppress or shrink" the trade — it must see the sized candidate
  to shrink it; I1 says the gates cannot be consulted by the model — so the
  consult happens strictly upstream of `evaluate_gates` and a suppressed
  candidate never reaches them (proven by the no-gate-rows test).
- **An unanswered veto fails CLOSED, flagged, unscored.** Provider error =>
  suppress (within the veto's reduce-only authority; risks zero capital),
  audit row carries `veto_error: true`, and the suppression is NEVER
  counterfactually scored — an outage is not model judgment and must not
  contaminate the veto value-add measurement. Alerting on veto_error rates
  belongs to T1.5 metrics.
- **Multi-leg proposals from veto-enrolled strategies are suppressed
  whole, loudly.** Partial-group vetoes would manufacture unhedged legs;
  no spec'd strategy needs group vetoes (mech_extremes is single-leg), so
  the semantics stay deliberately undefined rather than invented.
- **Counterfactual scoring assumes a maker fill at the limit price**
  (`fill_assumption: filled_at_limit`, recorded on every score row).
  Whether the resting order would actually have filled is unknowable; the
  assumption is optimistic FOR THE TRADE, i.e. the harshest framing for the
  veto when the vetoed trade would have won. Hypothetical PnL is net of the
  maker fee (maker-only doctrine); the scorer ERRORS if asked to score
  more quantity than was vetoed (no fabricated records). Scoring fires in
  `apply_settlement`, exactly once (drained), at the same 100c/contract
  payout convention the position book uses.
- **Every consultation is audited, Allow included** (`veto_decision` rows
  with qty_before/qty_after and the assessment's `cost_cents` — model
  spend is tracked from day zero; the stub costs 0).
- **Markets settle whether we hold them or not**: the runner's settlement
  path now checks for a tracked position before invoking the strict state
  layer (which still errors on untracked settlement — that discipline is
  unchanged), because a fully vetoed or never-traded market settling is
  normal, not a discrepancy.
- **`VetoMind` mirrors the spec 5.9 `Mind` shape** (`&self`, Send + Sync,
  async, cost in the return) so the Phase 2 Anthropic-backed veto drops in
  behind the same trait; the model id stays a plain `&str` until T2.5
  introduces `ModelId`.

## T1.1 — Kalshi adapter (doc-derived; fixture confirmation pending, see GAPS)

All venue behavior is grounded in docs/research/venue/kalshi-api-2026-06-10
(OpenAPI v3.21.0 + doc pages archived under raw/); doc samples in
crates/fortuna-venues/tests/kalshi_doc_samples/ are NOT recordings.

- **V2 create defaults:** `time_in_force=good_till_canceled`,
  `self_trade_prevention_type=taker_at_cross`, `post_only=false`,
  `subaccount=0`, `exchange_index=0`. GatedOrder carries no TIF/STP — the
  exec policy owns timing (I6); values match the doc example.
- **Side/Action mapping (load-bearing):** V2 quotes the YES leg only —
  (Yes,Buy)→bid@p, (Yes,Sell)→ask@p, (No,Buy)→ask@100−p,
  (No,Sell)→bid@100−p. Inbound reads `outcome_side`/`book_side` only
  (legacy fields are past their deprecation window); Kalshi's model
  collapses buy-yes/sell-no, so inbound canonicalizes to Buy-of-outcome-
  side (venue-truthful under signed net positions); a disagreeing pair
  (e.g. yes/ask) is a hard error.
- **`markets()` is scoped to configured `series_tickers`** (Market has no
  series field; per-series listing is the documented join); empty config
  => empty catalog. `Market.category` = `Series.category`;
  `SettlementMeta{oracle_type:"kalshi_rulebook", resolution_source: joined
  settlement_sources (fallback "kalshi"), expected_lag_hours:
  ceil(settlement_timer_seconds/3600)}`.
- **Status maps:** initialized→Listed, inactive→Halted, active→Trading,
  closed→Expired, determined/disputed/amended→Determined,
  finalized→Settled, UNKNOWN→Halted (conservative: not tradeable).
- **`balance()` uses the integer-cent `balance` field** (documented
  truncating => never overstates cash), not `balance_dollars`.
- **Integer-cent core enforced at the boundary:** scalar `market_type` and
  non-`linear_cent` price structures are filtered out of the catalog;
  fractional `count_fp`/`position_fp` anywhere is a hard error.
- **Fees parse with ceil (against us);** reconciliation `matches` iff
  0 <= modeled−charged <= 1c (documented per-order rounding rebate;
  any overcharge flags). `fee_multiplier` (JSON double) → Decimal via
  shortest-repr string (observed 0/0.5/1 exact); maker scaling m×0.0175
  is the fees-research inference — reconciliation surfaces divergence.
- **`Fill.at` fallback chain** `created_time` → `ts` → injected clock
  (both venue fields optional in the spec).
- **Transport:** cursors/series tickers appended without percent-encoding
  (URL-safe charset assumed); any 2xx on create decodes as success;
  timeouts surface as may-have-executed (resolved by coid via
  AlreadyExists on resubmit); NO retries in the transport (the manager
  owns retry semantics). 429→RateLimited, network→Outage.
- **Cancel reconcile:** the V2 DELETE response body is IGNORED entirely
  (documented wrong-order bug); state confirmed via GET:
  canceled→Ok, executed→Rejected, resting/unknown→Timeout.
- **Fills paging:** terminal page keeps `next_cursor` at the polled cursor
  (at-least-once; dedup on fill_id). A coid-resolution failure fails the
  whole page (safe under re-poll) rather than inventing a client id.

## T1.2 — paper engine

- **The doctrine predicate is yes-space strict inequality.** Spec 11 fixes the
  rule (maker fills only on trade-through, never at touch) but not the math.
  Prints arrive in YES-space (1..=99 integer cents); each resting order maps
  to a yes-space bid or ask via its (side, action); a print fills a bid only
  if `print < limit` and an ask only if `print > limit`. Equality is NEVER a
  fill — `touch_prints_never_fill_resting_orders` enforces this and is the
  one test in the crate that must never be weakened (project skill: "a fill
  at touch must FAIL the suite").
- **Haircut budget is per-print, floor-rounded, shared FIFO.** Spec 11 says
  "configurable quantity haircut" without mechanics. Implemented as
  `budget = floor(print_qty x pct / 100)` shared across ALL our resting
  orders on that market in placement order (time priority). Floor rounds
  against us (a 1-lot print at 50% fills nothing); sharing one budget
  prevents the same print from double-filling stacked orders — both choices
  cap paper optimism.
- **Taker phase crosses DISPLAYED depth only, at displayed prices.** No mid
  fills, no hidden liquidity, no price improvement. Each consumed level
  mutates the local book copy so one order cannot eat the same level twice;
  the next `apply_book` from the feed replaces the book wholesale (the
  canonical feed wins — paper fills claim no market impact beyond the
  snapshot they consumed). Unfilled remainder rests at limit.
- **Resting buys reserve worst-case cost** (notional + max(maker fee, taker
  fee, 0)), recomputed on every partial fill for the remainder; sells
  reserve zero (close-only, no cash at risk). Mirrors the sim venue and
  fortuna-state reservation semantics so paper/live parity holds at the
  Strategy interface.
- **Settlement mirrors the sim venue:** winner-side longs pay
  `payout_per_contract` each, the market's resting orders cancel with
  reservation release, the position entry and book are dropped, the market
  goes `Settled`, double-settlement errors. Public-trade prints with
  out-of-range price/qty are ERRORS, not silent skips (a corrupt feed must
  surface, spec 5.13 discrepancy discipline).
- **Paper/live parity is structural:** `PaperVenue` implements the same
  `Venue` trait the sim and Kalshi adapters implement, so the runner
  composition is byte-identical across stages; only fill semantics differ.
  Verified by the parity test driving the same gated orders through sim and
  paper.

## T0.7 — state crate

- **THE PAIR-VALUE RULE (correction found in hostile review):** YES and NO
  are tracked as SEPARATE lots everywhere (sim venue positions, the state
  book, marks, DST derivation) and never net against each other. A held
  YES+NO pair pays exactly $1 at settlement regardless of outcome; the
  original net-YES model (my T0.3 design, inherited by the first T0.7 cut)
  silently destroyed that value and would have made the sum-arb strategy
  (mech_structural) look like a guaranteed loser in Sim. `net_yes()` survives
  as an EXPOSURE view only (direction risk for gate inputs), never a
  valuation. Real Kalshi auto-nets pairs with an immediate $1/pair credit
  (capital efficiency); holding both lots to settlement is value-equivalent
  and conservative — the difference is recorded in GAPS for T1.1/T1.2.
- **Reductions are close-only per lot**; a sell beyond the held lot is
  `OverClose` (books-vs-venue discrepancy), never a silent flip.
- **Proportional basis on close** = `floor(cost_basis x closed / held)` via
  `div_euclid` (true floor, not Rust's truncation); dust stays in the open
  basis and telescopes out exactly on the final close. Conservation
  (proptested): `yes.basis + no.basis == net cash into the market's
  positions + realized_pnl` at every step.
- **Settlement realizes BOTH lots** (winner at payout, loser at zero) and
  returns the venue-owed payout (winner lot only); voids refund total basis
  across both lots and never touch realized PnL (spec 5.13: the world broke
  the question). Settling/voiding an untracked market errors (discrepancy
  territory, never a silent no-op).
- **Position entries are retained zeroed** after close/settle/void as
  per-market realized-PnL/fee accumulators; lifecycle resets to Open.
- **Marks** price each lot independently against the book (YES at bid, NO at
  100-ask) and sum — a pair marks at ~the 100c pair value minus spread.
  Stale (strictly older than max age) or wide (strictly above max spread,
  both touches present) books still mark at the touch but set `wide_flag`;
  a missing needed touch or missing book marks ZERO with the flag (no
  reliable exit value; a binary lot is never worth less than zero); a
  degenerate ask above 100c clamps to zero + flag.
- **Account views are pure functions of explicit inputs**; `deployable` may
  go negative (over-commitment is reported, never masked); Disputed /
  ResolutionPending positions are excluded from floating while their
  worst-case exposure is reported separately (spec 5.13).
- **Reservations:** one active reservation per intent (duplicates error);
  release is exactly-once (second release returns false, never
  double-frees); rebuild replaces ALL state and ACCEPTS over-envelope totals
  (flagged via `over_envelope`) so a reduced envelope config cannot brick
  boot — new reservations still fail while old ones unwind; unknown-strategy
  entries rebuild (flagged) but new reserves fail closed.
- **Drawdown monitor:** day = 00:00 UTC, auto-rolled inside `check`; breach
  at `loss >= limit` (limit > 0; non-positive limit disables); breach is
  STICKY for the rest of the UTC day even through recovery — defense in
  depth, the gates' halt flag (human re-arm only) is the real lock (I2
  invariant test implemented at this task).

## T0.10 — strategy interface, mech_structural, the composed runner

- **mech_structural v0 = the BRACKET yes-sum scan** (cross-market). The
  spec's "YES/NO sum scans" within one market are degenerate on
  Kalshi-semantics venues: the YES ask ladder IS the NO bid ladder (one book
  of yes-bids and no-bids), so a single-market sum below 100c is a crossed
  book the exchange itself matches. Real structural edge is cross-market
  (bracket families) and cross-venue (T3.4). Bracket families are strategy
  CONFIG in Phase 0; canonical-event edges replace config at Phase 3.
  Bracket monotonicity joins when event mappings exist.
- **Strategies are iterated by the runner over newly recorded bus events**
  (registration order), not wired in as bus handlers: ownership stays
  simple, ordering deterministic, and the bus remains the byte-exact record
  of every input and decision artifact. Spec 5.1's pattern is preserved in
  substance (deterministic dispatch + replayable record).
- **Strategies emit UNSIZED proposals** (legs with limit + honest fair
  value + group policy + urgency). Sizing is the harness's: arb sets from
  envelope headroom (`affordable_sets`, floor division), Kelly for belief
  trades (Phase 2), all re-checked by the gates. Edge distribution across
  arb legs: fair_leg = ask + floor(edge/n) so each leg independently clears
  the gate floor — thin arbs whose per-leg share can't clear the floor are
  deliberately not tradeable.
- **Scan fee estimates use a representative batch (qty 10, ceil per
  contract)**: quantity-1 estimates overstate (ceil eats sub-cent fees) and
  killed real arbs; the gates re-verify at the sized quantity regardless.
- **Group complete-or-unwind in the runner (v0):** TakerComplete cancels
  stale resting legs and lets the strategy re-propose against fresh books
  (gates apply, I1); Unwind FREEZES (cancel unfilled legs, hold filled lots,
  loud audit + bus alert) rather than panic-selling — consistent with the
  flatten planner's philosophy. Spec 5.4's executed unwind lands with paper
  realism (T1.2) where exit fair-values are honest.
- **Audit-failure halt is wired and tested in the runner** (I5: the first
  failed audit write sets a global halt recorded on the still-alive bus).
- **Equity for drawdown in Sim** = venue total cash + conservative marks of
  open lots; positions in limbo states excluded by the marks/lifecycle
  machinery from T0.7.

## T0.9 — ops, kill switch, CLI

- **Kill-switch independence is STRUCTURAL:** it lives in its own crate
  (fortuna-killswitch, deviating from the skill's "binary inside
  fortuna-ops") whose dependency graph cannot contain Postgres/ledger/
  cognition — and the i4 invariant test asserts that graph mechanically
  from cargo metadata, so a future dependency addition fails CI. Its state
  is a flat fsync'd JSONL journal (spec Principle 9 exception).
- **The kill switch never constructs orders.** Emergency "flatten" =
  freeze-and-cancel + journal/report open positions for the operator.
  Placing requires a GatedOrder (I1); the emergency path's job is stopping
  the bleeding (resting risk) and surfacing state, not trading. Operator
  exits happen via venue UI or CLI-confirmed flows.
- **The operator CLI is its own crate (fortuna-cli, binary `fortuna`)**
  because halt/re-arm persistence needs the ledger while ops/killswitch
  stay lighter. halt/rearm write durable halt_events + an audit row with
  the OPERATOR ATTRIBUTED (--operator required); the runner restores flags
  at boot and observes operator events via a halt-poll (T0.10). `fortuna
  kill` execs the standalone binary and never touches the database.
- **Slack (per docs/research/ops/slack-api-2026-06-09):** Socket Mode is
  the chosen interactivity path (no public URL on ITHACA); Phase 0 ships
  send-side only (router + Block Kit approval-message builder); the
  interactivity LISTENER lands with the review flows (Phase 2/3, GAPS).
  The transport surfaces 429/Retry-After as typed errors and never sleeps
  internally (no hidden waits in deterministic paths); the runner owns
  retry/backoff policy. chat.postMessage has no idempotency key (research):
  a send timeout may double-post — the audit row is the dedup record.
  Slack can REQUEST halts; re-arm verbs do not exist over Slack (I2).
- **Secret hygiene:** secret-holding types (Secrets, SlackRouter,
  DeadmanPinger) implement no Debug or redact it; reqwest errors are
  URL-stripped before surfacing; secrets only from env.
- **Dead-man pinger** provides due/record/ping pieces; the LOOP and the
  failure-escalation wiring live in the runner (spec: the system cannot
  report its own death — missed pings alert via the monitor's own channel).

## T0.8 — ledger schema (decisions made at migration design time)

- **Timestamps are TEXT ISO8601** (fixed-ms, the in-process wire form), per
  the spec 5.5 DDL and the conventions line. Lexical order == chronological
  order at fixed precision, so range queries and partition bounds work.
  TIMESTAMPTZ would be more idiomatic Postgres; following the spec's DDL is
  the conservative read.
- **Append-only is enforced in the DATABASE too** (BEFORE UPDATE/DELETE
  triggers raising exceptions), not just at the application layer. CLAUDE.md
  demands INSERT-only repos; the trigger makes I5 hold even against bugs or
  manual psql. `beliefs` is content-immutable via a column-level guard:
  only status/outcome/brier/clv_bps may change (the scoring job's columns,
  spec 5.5).
- **audit and signals are PARTITION BY RANGE with a DEFAULT partition.**
  Spec Section 7 wants monthly partitions; the DEFAULT partition makes the
  system correct from day one, and monthly partitions can be attached by an
  ops job when volume warrants (recorded as future ops work, not a gap in
  correctness).
- **Supersession is pure-INSERT**: new rows carry `supersedes` pointing at
  what they replace (settlements, edges, lessons, beliefs); the old row is
  never touched. Queries derive "current" by anti-joining supersedes.
- **Reservations and halts persist as event streams**
  (reservation_events/halt_events, INSERT-only) and fold to current state at
  boot — reservations because spec 5.14 defines them as derived state, halts
  because I2 must survive restarts (a reboot must NOT clear a drawdown halt;
  the runner restores halt flags from the fold).
- **exec_cursors is mutable** (a checkpoint, not history) — cursor positions
  are derived state like reservations, but a single-row-per-venue checkpoint
  is the honest shape; history lives in audit.

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
