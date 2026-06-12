# ASSUMPTIONS.md (agent-maintained)

Every decision made where docs/spec.md is silent: what was assumed, why it is the
conservative option, and the spec section it interprets.

## T4.4 — operator CLI lifecycle, slice 1 (track B)

- **Pidfile format is `<pid>\n<name>\n`** (design A3 names the fields, not
  the encoding): two plain lines, no serialization dep, trivially
  inspectable by the operator with `cat`. Any malformed pidfile reads as
  STOPPED-stale (never trusted, never signaled later).
- **Name validation is substring containment on `ps -o comm=` output:**
  macOS `comm` yields the full executable path (`/bin/sleep`), Linux a
  possibly-truncated basename — `contains(<claimed name>)` is the one test
  that works on both. Conservative direction: a mismatch DEMOTES a live pid
  to stale; it never promotes.
- **`FORTUNA_RUNTIME_DIR` resolves relative to the CWD when overridden, and
  defaults to `data/runtime/`** (A5) relative to the repo root — consistent
  with the A2 contract that lifecycle commands run from the repo root (the
  recorder spawn cwd is pinned there too). Tests pin absolute temp dirs.
- **`logs` execs `tail -n50` for BOTH modes** (A4 specifies `-f`): one code
  path, no whole-file read into memory (daemon logs grow unboundedly under
  append-mode redirection), Ctrl-C lands on tail directly.
- **`status` db section is bounded at 5s** — see the GAPS T4.4 entry for
  the degradable-status posture this pins.
- **`start` spawns per-component, daemon first then recorder, and only the
  missing ones** (design §5 lists daemon-then-recorder; a recorder that
  crashed overnight can be restarted under a still-running daemon without
  touching it). All components already running => idempotent exit 0,
  decided BEFORE the A2 pgrep check (a fully managed system is not a
  collision).
- **Component binaries resolve FORTUNA_BIN_DIR -> current_exe siblings ->
  PATH:** the standard cargo target layout puts fortuna-live and
  fortuna-recorder next to the fortuna binary; FORTUNA_BIN_DIR is the
  operator pin (release vs debug) and the test seam (stub binaries).
- **`--foreground` is exec, not supervise:** it replaces the CLI process
  with `fortuna-live <config-path>` (no pidfile, no recorder, no detach) —
  a debugging mode where the operator owns the terminal; exit codes
  propagate unmodified.
- **`pgrep -f fortuna-recorder` is the A2 collision mechanism** (named by
  the amendment). pgrep FAILING TO RUN refuses the start (fail closed —
  an unverifiable safety check is a failed safety check); pgrep exit 1
  (no match) passes.
- **A7 stopping markers are cleared when `start` claims a pidfile** — a
  marker left by a previous stop must never make a freshly started daemon
  read as "stopping since".
- **`stop`'s decision matrix per component:** stale pidfile (dead pid /
  name mismatch / garbage) => "already stopped", stale file removed,
  NOTHING signaled (A3: a reused pid is never signaled — test-pinned);
  empty mid-claim pidfile => warning + skip (a start is racing; stop never
  steals a claim); running => marker, SIGTERM, bounded wait.
- **stop polls liveness at 200ms cadence against a RealClock deadline**
  (default --timeout-secs 60 per A7); the wait is wall time at the binary
  edge, the sanctioned RealClock source — no SystemTime::now().
- **stop exits 1 whenever ANY component produced a warning** (timeout,
  missing A1 evidence, mid-claim skip) even though the other component
  proceeded — the operator must read the output; exit 0 is reserved for
  fully-confirmed shutdowns and true idempotent no-ops.

## T5.B3 slice 2: I2 composition + invariant additions (track C, 2026-06-12)

- **`equity_with_margin` is the I2 seam, not the wiring:** the composed
  number (event equity + Σ margin-account conservative equity) is what the
  DrawdownMonitor must consume once perp runtime state exists; the daemon
  feed itself is fortuna-live (track A) and lands with B4/B5 integration.
  The invariant tests pin the composition's behavior (perp-only losses
  breach; worse-for-us mark governs; funding paid is drawdown) so the
  wiring cannot later redefine the math.
- **The unmarked flag ALERTS, never blocks:** a position valued on the
  venue's number alone is alert-worthy degradation, but the composed
  equity is already the conservative number available — halting on
  missing marks would make a feed outage into a self-inflicted halt
  (the wide-mark flag in 5.14 has the same posture).

## T5.B3 perp gate arm, slice 1 (track C, 2026-06-12; interprets spec 5.15)

- **"Same sealed GatedOrder through the same I1 pipeline" is read as: same
  `GatePipeline` instance (shared halt flags, shared I3 buckets, same audit
  record stream, same fail-closed loop, same private-constructor seal
  discipline) producing a SECOND sealed type `GatedPerpOrder`.** The literal
  same-type reading is impossible under 5.15's own type-level price
  separation: `GatedOrder.limit_price` is `Cents` and "a PerpPrice must
  never carry an event-contract price nor vice versa". The invariant
  substance — no order reaches a venue without passing the gates,
  constructible only by the pipeline, Serialize-only — is identical.
  FLAGGED for verifier review as the load-bearing design reading.
- **Reduce-only doctrine:** a VALID reduce-only order (position exists,
  opposite direction, qty <= |position|) passes MarginHeadroom,
  LiquidationDistance, LeverageCap, PerpNotionalCap, and EdgeFloor with an
  "exposure-reducing" note. Rationale: such an order strictly reduces
  worst-case exposure and grows liquidation distance; rejecting it would
  force staying at HIGHER risk, and blocking a stop-loss close on a
  margined instrument converts margin-model error into realized loss. It
  still faces halts, price/size sanity, rate limits, idempotency, and
  netting in full. Invalid reduce-only (no position / same direction /
  oversized) rejects outright. FLAGGED for verifier review.
- **Worst-case quantity = max(|pos|, |pos + delta|):** the position passes
  through every value between pos and pos+delta while an order fills, so
  the risk checks price the largest |position| along that path (a
  non-reduce-only flip is priced at the bigger side, test-pinned).
- **Conservative per-contract valuation = max(limit, mark):** the worst of
  what we'd pay and where the market marks; exposure ceils to cents.
- **MM curve semantics:** ascending (max_notional_cents, mm_bps) tiers;
  lookup takes the first tier covering the worst-case notional; beyond the
  last tier the approximation cannot bound the order and it is REFUSED
  (spec 5.15 sentence implemented literally). Safety multiplier is percent
  and validation-enforced >= 100 (below would weaken the venue's own
  requirement); the venue's IM = 1.3 x MM suggests operators set >= 130.
- **Funding drag inputs:** the candidate DECLARES intended holding in 8h
  windows (holding_windows >= 1 enforced — every position can cross a
  funding tick); drag = order notional x funding_drag_bps_per_window x
  windows, ceiled. The drag also debits available equity in the headroom
  check (plan §2's "margin consumed at liquidation + funding drag").
- **Checks 2/3/9 (Capital, PositionCaps, EventExposure) do not run on the
  perp arm** — dispatch on kind per 5.15: the margin account is the
  dedicated capital envelope (MarginHeadroom is the capital check), the
  leverage/notional caps are the position-caps analog, and perps have no
  canonical events. Per-STRATEGY exposure caps for perps bind at sizing
  (T5.B7); ledgered as a deliberate deferral.
- **Rate-limit logic is deliberately duplicated** in the perp arm
  (consume_perp_rate), kept in lockstep with pipeline::check_rate_limits
  over the SAME buckets, rather than refactoring the I3-pinned original
  mid-flight. Both arms set the same venue halt on breach (cross-domain
  halt is test-pinned).

## T5.B2 perps core types (track C, 2026-06-12; interprets spec 5.15)

- **`PerpPosition.avg_entry` is a whole-tick `PerpPrice`:** the venue quotes
  fixed-point dollar strings at the $0.0001 tick, so whole ten-thousandths
  cover every observed payload. If a venue-reported average entry ever
  carries finer precision, the B4 DTO layer converts it with an explicit
  rounding direction chosen against us — precision policy stays at the
  payload boundary, not inside the core type.
- **`FundingAccrual.rate` is `Decimal` inside a core type, deliberately:**
  the record is an append-only venue-payload boundary artifact whose job is
  to reproduce the venue's reported rate EXACTLY for reconciliation against
  funding_history. The venue applies its own ±2% cap and 0.01% zero
  threshold BEFORE reporting; the constructor records reported rates and
  never re-derives them (re-deriving would manufacture false discrepancies).
  No money math rides on the Decimal beyond the single floored conversion
  to `Cents` at construction.
- **Funding `amount` sign convention:** positive = received, negative =
  paid — matches the venue's funding_history `funding_amount` convention so
  reconciliation is a direct compare. Flooring the signed flow toward -inf
  is against-us in both directions (receipts never overstated, payments
  never understated).
- **Position-update math (fills, flips, realized PnL) is NOT in B2:** spec
  5.15 defines the types; fill application belongs to the state/paper
  layers and lands with T5.B5 (SimClock funding accrual + liquidation sim).
  `PerpPosition` exposes only sign helpers, conservative uPnL, and ceiled
  notional — the read-side surface B3's gates need.
- **`MarginAccountView.pending_funding` is caller-supplied:** computing the
  in-progress window estimate (TWAP accrual on SimClock) is T5.B5 work; the
  view's contract pins where it lands in equity (balance + unrealized +
  pending_funding = the I2 drawdown input) without faking an estimator.
- **`FundingAccrual` carries no id:** ids are assigned by the ledger layer
  at INSERT (append-only convention); the core type is the record content.
- **`InstrumentKind` lives in `fortuna_core::perp` but is NOT yet threaded
  through the shared `Market` structs:** the fortuna-venues market types are
  outside track-C ownership (orchestration.md); threading lands with B3/B4
  inside owned files, and any shared-struct edit it forces will be ledgered
  as a cross-track item instead of made unilaterally.
- **No new DST scenarios from B2:** pure value types with no event-loop or
  order-path interaction; the dedicated perp DST arms are T5.B6 (queued).

## Latency levers: concurrent legs + stream ingestion (operator-directed)

- **Concurrent leg submission preserves the determinism doctrine:** the
  phase split (journal-persist all legs -> place all CONCURRENTLY ->
  process outcomes in LEG ORDER) keeps every decision artifact and
  journal write deterministic; only the network IO overlaps. join_all
  returns in input order regardless of completion order. All-or-nothing
  UPGRADED: any pre-submission gate rejection aborts the whole group
  (the old interleaved path could strand a submitted leg for the unwind
  machinery). Proven by a yielding mock venue (max in-flight == legs on
  a single-threaded executor).
- **Stream layer is canonical YES space, cents-only:** StreamEvent /
  MarketStream / BookAssembler are venue-generic; the assembler refuses
  TORN states (delta before snapshot, overdrawn level) — a book we
  cannot prove whole is never trusted. RecordedStream replays captured
  sequences; fortuna-paper::feed_stream_event drives the SAME honest
  fill rules from a stream (the recorded-replay seam — operator
  recordings plug in directly).
- **Assembled books VALIDATE before rendering:** a crossed or
  structurally invalid assembled book errors at the assembler (torn
  state => resync), not at a downstream sink. If live data shows
  legitimately crossed transient books, that is new venue knowledge the
  fixture session must capture — the assembler will not be loosened on
  speculation. Negative resting quantities in snapshots fail loud (zero
  quantities are simply absent levels).
- **Kalshi WS message layer is doc-derived and dial-gated:** parser +
  subscribe builder built ONLY against the verbatim official examples
  and archived AsyncAPI spec (research 2026-06-10). We subscribe with
  use_yes_price=true and parse BOTH sides on the yes scale (the no-leg
  default is the documented trap; flag semantics are fixture item #20).
  Sub-cent prices/fractional counts refuse. Per-sid seq tracking: a gap
  returns SeqGap and the baseline does NOT advance — every subsequent
  delta keeps screaming until the composition resubscribes. The live
  socket DIAL (signed-handshake auth, ping/pong, redial policy) is
  fixture-gated like the REST adapter; ws-via-fill stays out of scope
  (REST fills remain the fee source of truth).

## F1-F3 + telemetry hardening (operator-prompted, post-independent-gate)

- **Degrade is never silent (F1):** strategies buffer typed
  DegradeRecords (budget_exhausted w/ scope+spent+cap, provider,
  schema_invalid, refused, context, model_proposals_discarded); the
  runner drains them each tick into 'cognition' audit rows + bus events;
  budget breaches count once AT THE DRAIN (no double count with strategy
  metrics). The ops alert rule (fortuna-ops alerts module) works over
  scrape DELTAS: every budget breach alerts (one message per scrape with
  the count — page storms help nobody); failure bursts alert at a config
  threshold; quiet scrapes are silent. The live composition diffs
  counters per scrape and routes through SlackRouter.
- **Latency percentiles are CONSERVATIVE bucket estimates:** fixed
  bounds (1ms..60s, 14 buckets + overflow) keep the histogram
  deterministic and Copy; quantile_ms(q) reports the UPPER edge of the
  first bucket reaching q x count (overflow reports observed max) — the
  estimate never understates latency. Exported both as Prometheus
  cumulative buckets (le labels, +Inf == count) for PromQL and as direct
  p90/p95/p99 gauges for the boards. Changing bucket bounds is a
  config-style decision.
- **Section 8 surface completed where cheaply derivable from runner
  state:** gate rejections BY CHECK (labeled), venue API error counter
  (polling-loop outages), settlement voids/reversals counters,
  envelope utilization bps per strategy (active/envelope from the
  reservation ledger), cognition cost cents (completed decisions,
  merged from strategy metrics; PER-DECISION cost rides in belief
  PROVENANCE — the f-batch gate caught this entry claiming it rode in
  the cognition audit rows, which carry degrade events, not costs) and
  fortuna_mind_spend_today_cents, the budget-true day total including
  failed-call burn (gate finding: the completed-decision counter alone
  undercounts spend; the mind's own budget is the authoritative source).
  Capital-in-limbo and overdue gauges already existed (T1.5). NOT
  runtime metrics by design: triage recall/precision, rolling Brier,
  calibration curves (weekly-review artifacts from the ledger);
  context token usage (cost cents is the operative control; token
  counters can ride the same path when wanted).

## Order/fill latency metrics (spec Section 8; operator-prompted)

- **Measurement points:** ack latency = clock delta across the awaited
  `place()` (truthfully ~0 under SimClock — the await is instantaneous;
  becomes real wall time in paper/live); fill latency = venue execution
  timestamp (fill.at) minus submit time, by client order id, observed on
  APPLIED fills only (duplicates never double-count). Submit times prune
  at terminal intent states. Aggregate is count/sum/max (mean = sum/count
  downstream, Prometheus convention) — no histogram buckets until live
  data says where the edges belong.
- **Unknown-outcome submissions are measured too** (the order may be
  live; if its fills arrive, their latency counts).
- **Dropped-then-redelivered fills measure EXECUTION latency, not
  delivery lag** (fill.at is stamped at execution) — delivery lag is
  visible separately as cursor-poll behavior. Recorded so nobody reads
  the fill metric as a delivery-health signal.

## E5 — verification-gate minor sweep

- **Remaining f64 touchpoints, recorded:** cycle.rs fair-value floor
  ((p * 100).floor() — conservative direction: an edge is never rounded
  into existence) and review.rs fee/PnL ratio (recommendation-only
  output; never money movement). Both are probability/ratio space, not
  money; the money paths are integer (Cents, i128-widened Kelly budget).
- **run-dst.sh now fails CLOSED:** a DST harness that fails to BUILD
  fails the battery (the T0.4-era "passing vacuously" escape existed for
  the pre-harness epoch and is gone).
- **Watchdog partition:** venue-DEPENDENT checks (posted->confirmed,
  books-vs-venue mismatch) skip during an outage and resume next poll
  (streaks neither advance nor clear on darkness); venue-INDEPENDENT
  checks (overdue, dispute-on-last-known-meta, orphan scan) run through
  outages. A dispute flag the venue never delivered is not actionable —
  that is data absence, not starvation.
- **Discards are visible:** model-emitted ProposalDrafts the cycle
  discards are counted (per-strategy metric -> RunCounters -> metrics
  export) and every proposal audits a "proposal" row carrying the
  decision cycle's context-manifest hash (mechanical scans: None) —
  decision replayability (spec 5.7) reaches the audit log in Sim.
- **Regression corpus remains empty** — no randomized run has produced
  a red seed to minimize (the settlement-DST shakeout caught a HARNESS
  arm bug pre-commit, fixed in place; engine seeds only go to the
  corpus). The vacuity stays disclosed until the first red seed lands.

## E1/E5a — calibration + Kelly wiring (verification-gate fixes)

- **The calibration layer now BINDS in the decision cycle.** Each mind
  belief's raw p is calibrated before the comparator: fitted method at
  the scope's resolved_n >= 50, shrinkage toward the market prior below,
  and an UNWIRED scope (no CalibrationContext) shrinks FULLY to market —
  it structurally prices no edge. The market prior is the Direct-edge
  quote mid only (a negation/composite prior could poison the shrinkage
  target); no Direct quote => prior 0.5. Calibrated p REPLACES belief.p;
  p_raw is preserved for scoring.
- **Kelly sizing keys off legs[0]** (synthesis proposals are single-leg
  by construction today; a multi-leg synthesis proposal would need
  per-leg probabilities — revisit with any multi-leg synthesis design).
- **Synthesis sizing = min(haircut-Kelly, affordability).** EdgeCandidate
  and ProposedLeg carry the calibrated win-probability of the candidate
  side (NO-side candidates carry 1 - p). The runner computes fraction =
  kelly_fraction (RunnerConfig, fed from ops [sizing] config; clamped,
  non-finite => 0) x calibration quality (composition feed per strategy;
  UNWIRED => 0 => zero size). Every kelly sizing emits an audit row with
  p/quality/fraction/kelly/affordable. Mechanical legs (calibrated_p =
  None) keep pure affordability — an arb's edge is structural.
- **kelly_contracts is integer money (E5a):** the f64 fraction converts
  once to floored parts-per-million; the budget multiplies in i128 and
  floors — money never rides in a float; double flooring is conservative.
- **i7_promotion_gates.rs received a one-line compile fix** (new required
  RunnerConfig field `kelly_fraction: 0.25` in its config fixture) — no
  assertion logic touched; added to the protected-crate waive queue in
  GAPS.md as batch 4.

## T3.6 — closing watchdogs + decision records (moved from GAPS)

- **Divergence detector (spec 5.13) wiring:** the runner takes canonical
  event resolutions as a composition feed (set_canonical_resolution per
  market with the edge id). On settlement OR correction application, a
  disagreeing canonical truth records a settlement_divergence discrepancy
  carrying both truths and the edge; PnL follows venue truth unchanged.
  The edge-confidence hit is applied by the COMPOSITION as a superseding
  edge insert (the record names the required action) — the runner does
  not write the ledger.
- **Orphan watchdog (spec 5.13) wiring:** the composition feeds the
  coverage view (markets with a fresh open belief or a mechanical owner)
  via set_position_coverage; unwired coverage DISABLES the watchdog
  (silence must mean "not wired", never "everything covered"). A held
  position outside coverage alerts ONCE (audit watchdog row, kind
  orphaned_position) with forced-exit evaluation as the disposition.
- **Sub-cent price structures excluded** (decision record, T0.3/T1.1):
  core money is integer cents; the Kalshi adapter filters
  deci_cent/tapered_deci_cent structures and scalar markets from the
  catalog. The same rule binds the future Polymarket adapter (0.0001
  ticks). Revisit only if such markets matter commercially (would need a
  price-tick type).
- **Kalshi pair auto-netting not modeled** (decision record, T0.7/T1.2):
  sim and paper hold YES+NO lots to settlement — value-identical to
  netting, conservative on capital (no early $1/pair credit). Verify
  exact venue netting against recorded fixtures; if confirmed, the early
  credit is a paper capital-realism follow-up.
- **Runner halt-poll documentation** (T0.10 note): in the Sim composition
  gates are in-process (halts act immediately). The LIVE composition
  polls persisted halt state; the go-live runbook (FINAL_REPORT.md) pins
  the interval at <= 500ms with an alert on poll failure, matching the
  runtime_state poller convention.

## T3.5 — synth_events in Paper

- **synth_events IS a SynthesisStrategy instance** (the T2.6 cycle
  behind the Strategy trait), not a new mechanism: confirmed-edges-only
  comparator (a wrong equivalence converts conviction into an unhedged
  position), 5c edge floor, shadow quota 3/day. Market selection
  (low-attention, weak-consensus) is the T3.2 discovery loop's job —
  the strategy trades whatever confirmed edges the composition hands it.
- **"Paper-only initially" = a declared stage CAP** (Stage::Paper in
  the factory). SynthesisConfig gained a `stage` field set by the
  composition from promotion::effective_stage(cap, operator_records):
  with no records synth_events runs at Sim TODAY; one operator record
  raises it to Paper and never higher than the cap.
- **The paper-boundary test quotes maker INSIDE the proposal's limit**
  (limit 62 from the comparator, quoted at 59): sizing, timing, and
  price-within-limit belong to the harness (I6) — the strategy's limit
  is the max acceptable price, not an instruction to cross. The honest
  fill rules then bind: a print AT 59 never fills; a print THROUGH
  fills with the 50% haircut. Recorded-stream replay stays
  operator-blocked on Kalshi fixtures (GAPS).

## T3.4 — polymarket US slot

- **The stub refuses rather than minimally simulates.** A stub that
  returned empty catalogs or zero fees would flow silently through the
  composition (a fabricated zero fee inflates every edge through gate
  check 6). `VenueError::FixtureGated` is a dedicated variant so the
  composition and ops can distinguish "venue gated by policy" from any
  runtime failure. The fee model refuses computation for the same
  reason. No research loop was run: the stub makes ZERO claims about
  Polymarket behavior, and GAPS sequences research BEFORE fixtures
  before any implementation.

## T3.3 — shadow-mode model comparison

- **The pairing key is the context manifest hash.** "Identical
  AssembledContext to the incumbent" (Section 11) is provable: the
  challenger receives the incumbent's exact AssembledContext and every
  shadow belief carries context_manifest_hash + shadow:true + the
  challenger's model_id in harness-stamped provenance. The comparison
  joins on it; duplicate (category, hash) pairs deduplicate rather than
  inflate the count.
- **Budget is checked BEFORE sampling:** an unaffordable cycle must not
  consume the day's sample quota. Challenger failures are counted on
  the harness and return None — the live loop never sees a challenger
  problem.
- **"Brier/CLV >= incumbent" encoded as:** challenger mean Brier <=
  incumbent mean Brier AND challenger mean CLV >= incumbent where BOTH
  sides measured; CLV unmeasurable on both sides => Brier decides. Every
  ACTIVE category must qualify with >= 30 paired resolutions. The
  verdict is {PromoteRecommended, Hold} — recommendation-only (I7);
  applying a swap is an operator config change recorded in audit.
- **I7 stubs closed (zero ignored invariant tests):** the model-swap
  clause pins evaluate_model_swap (no record / short record / worse
  challenger => Hold). The stage-promotion clause pins
  runner::promotion::effective_stage — everything starts at Sim;
  promotion advances ONE contiguous step per HUMAN-actor record
  ("system"/blank cannot promote); demotion steps apply for any actor
  (automatic on breach); the declared stage is a cap. The composition
  sources records from audit rows written by the operator CLI; records
  are supplied time-ordered.

## T3.2 — discovery loops

- **The normalization and watchlist contracts ride in the journal body
  as strict JSON** (same vehicle as the weekly review): MindOutput's
  I6-pinned surface is never grown; free prose degrades to an empty
  outcome with a defect. Hallucinated event matches (ids not in the
  existing set) and normalizations for non-survivor markets are dropped
  loudly, never created.
- **Match-before-create is enforced structurally:** a claimed match
  must name a real existing event; a new-event draft must carry
  statement + criteria. The composition supplies existing events per
  category (the matching context) and creates rows only from validated
  drafts.
- **Every proposed edge gets a confirmation card** carrying BOTH the
  model's confidence and the deterministic check score; high_stakes =
  non-direct mapping OR deterministic score < 1.0 (resolution-source or
  horizon mismatch — the UMA-mode failure). Cards are review-queue
  artifacts; confirmation itself stays an operator superseding-insert
  via EdgesRepo. Horizon tolerance for the deterministic check is 24h
  (market close and event horizon rarely align to the minute).
- **Tradability = min(volume/volume_norm, 1) x category calibration
  quality, 0 if no checkable resolution source** (spec names the score,
  not the formula). Persisted append-only (tradability_scores), one row
  per scoring run; latest() is the read.
- **Unscoreable rule:** the declared resolution source must be present
  AND enabled in the source registry at creation; otherwise the event is
  unscoreable, excluded from watchlist_count, and beliefs on it are
  REFUSED with a defect (likewise beliefs on undeclared events).
  resolved_samples/resolved_stats now exclude unscoreable events — the
  exclusion lives in the QUERY so every calibration consumer inherits it.
- **DiscoveryBudget throttles BEFORE spending** (00:00 UTC roll, like
  CostBudget); a throttled loop is not a defect. "World-forward is the
  first thing throttled" is composition ordering: it shares the budget
  and runs after market-back.

## T3.1 — weekly/monthly review jobs

- **The weekly review's deterministic core never depends on the mind.**
  Calibration audit + GO/NO-GO recommendations compute first; commentary
  and lesson candidates layer on top. Mind failure, a missing journal,
  or a contract-violating body degrade to a report with a recorded
  defect — never a lost report, never repaired prose.
- **Lesson candidates ride inside the journal body as strict JSON**
  (WeeklyCommentary: {commentary, lesson_candidates[]}). MindOutput's
  I6-pinned surface (beliefs/proposals/journal/cost) is NOT grown for
  lessons; the journal field is the spec'd text vehicle. Free prose
  fails the parse and yields zero candidates (never guessed from text).
- **Lesson promotion is an operator action** (spec 8 approve/reject):
  the weekly job DRAFTS candidates; LessonsRepo.insert is called by the
  operator approval path. Confirmation extends review_at and demotion
  (monthly decay) both happen by SUPERSEDING INSERT — the lessons table
  is append-only; the live row is the unsuperseded chain head; acting
  on a non-head row is refused.
- **GO/NO-GO encodes Section 11 as deterministic checks** with reasons:
  invariant violations are an unconditional NO-GO; mechanical needs >=
  30 paper days; synthesis needs >= 60 resolved beliefs AND measurable
  positive CLV; both need positive expectancy net of fees and fee/PnL
  < 0.35. Brier-vs-market-baseline for synthesis GO needs per-belief
  market-implied baselines (richer query) — recorded as pending; CLV
  carries the market-relative criterion meanwhile. Verdicts are
  RECOMMENDATIONS (I7): no stage, no mutation surface.
- **Refit versioning:** weekly refits advance version = prior + 1 per
  scope (caller supplies priors from CalibrationParamsRepo.latest);
  below n=50 no fit; degenerate records refuse with the reason in
  fit_defect. extremization_k stays 1.0 until the audit supports more.
- **Monthly allocation rule (spec silent on the algorithm):** net
  (pnl - fees - cognition cost) < 0 => recommend halving; otherwise
  hold. Recommendations never sum above the current total (no invented
  capital); increases are explicitly the operator's. Kill-switch test
  and backup restore drill emit as checklist REMINDERS only.
- **Per-strategy aggregates are review INPUTS** (the composition
  computes them from its own state/intents); fills carry no strategy
  column and the audit log is not an analytics store.

## Phase 2 EXIT — composed decision loop, invariants, aeolus_eval

- **The synthesis adapter triggers one cycle per book event for a
  mapped market.** TriggerEngine debounce/coalescing (T2.2) wires in at
  the live composition (Phase 3); the Sim tick cadence is already one
  book event per market per tick, so the adapter stays honest without
  it. Candidates map to single-leg Passive proposals at the candidate's
  max price (cap at displayed ask); urgency escalation is a Phase 3
  policy decision.
- **Cognition failure semantics:** ANY CycleError (provider, schema-
  invalid, refusal, budget exhaustion, context assembly) degrades to
  zero proposals + a counted `cognition_failures` metric. The tick
  NEVER errors and mechanical strategies are unaffected (spec 5.9
  "degrade to mechanical-only"). Counters merge strategy metrics at
  read time (the runner cannot reach inside Box<dyn Strategy> state).
- **I6 is enforced three ways:** deny_unknown_fields on the whole mind
  output surface (smuggled sizing/order fields reject the WHOLE
  output, never silently dropped — added to BeliefDraft, ProposalDraft,
  JournalDraft, MindOutput); a field-set pin on ProposalDraft/MindOutput
  (growing them breaks the invariant test); and a dependency-direction
  assertion (fortuna-cognition cannot name fortuna-venues/exec/state/
  runner types).
- **I7 is implemented for what exists:** the Sim runner refuses
  higher-staged strategies at construction; the Stage ladder is a total
  order. The operator-action-record and shadow-comparison clauses are
  staged stubs owned by T3.1/T3.3 (their rails don't exist yet) — the
  same pattern I5 used for its T0.10 extension. See GAPS.
- **Synthesis DST master seed follows the core DST convention** (wall
  clock unless DST_MASTER_SEED is set, every failure prints its seed);
  scripts/run-dst.sh runs it as stage 2 with the same seed count. Each
  scenario replays itself and demands a byte-identical recording.
- **aeolus_eval EXIT evidence uses the FORTUNA-defined contract fixture**
  (fixtures/aeolus/sample_envelope.json). Brier is computed by the
  scoring composition as (p - outcome)^2; the operator-recorded real
  export stays open in GAPS (it validates Aeolus's exporter, not our
  parser).

## T2.8 — calibration layer

- **Shrinkage weight is linear, w = min(n/50, 1):** spec 5.10 names the
  N >= 50 threshold but not the ramp shape; linear-in-n is the simplest
  deterministic ramp with no extra parameters to version. At n=0 the
  output IS the market prior; with NO market prior available the claim
  shrinks toward 0.5 (max uncertainty) — conservative, never a crash.
- **Below the threshold the fitted method AND extremization are
  ignored** (not blended): a fit on under-50 samples proves nothing,
  and extremizing an unproven claim amplifies it — both anti-
  conservative. Above threshold: method first, then extremization.
- **Platt fit refuses degenerate records** (empty, all-one-outcome,
  singular Hessian from no-spread or separation-saturated data) rather
  than silently returning identity — an unfittable record must surface
  to the weekly audit, not pass as "calibrated". Newton from fixed
  init (a=1, b=0), fixed iteration bound: bit-deterministic, so the
  same forward record always yields the same versioned parameters.
- **Isotonic apply is a step function** (value of the largest fitted
  threshold at or below the input), not interpolated: interpolation
  invents probabilities between observed points; steps only ever
  output pooled observed frequencies.
- **calibration_quality = min(n/50, 1) x max(0, 1 - 2*gap)** where gap
  is the n-weighted mean |claimed - observed| over reliability-curve
  buckets. The 2x slope zeroes quality at a 50-point average gap
  (claiming certainty on coin flips); the n-ramp keeps small samples
  from buying size through luck. Feeds the T2.6 haircut directly.
- **Calibrated outputs clamp strictly inside (0,1)** (eps 1e-9): a
  calibrated certainty would lie and break log-loss scoring.
- **Repo storage:** one row per (model, strategy, category, kind,
  version); kind = the method tag ('platt'/'isotonic'; the schema also
  admits 'shrinkage'/'extremization' rows for future split storage).
  The whole CalibrationParams JSON goes in `params`; `latest()` =
  highest version for the scope. Updates are new versions (UNIQUE +
  T0.8 append-only trigger refuse anything else).

## T2.7 — daily reconciliation + aeolus_eval

- **"No orders are placed from this loop" is STRUCTURAL:** the
  ReconciliationOutcome has no field that can carry a trade; proposals
  the mind emits anyway are COUNTED (discarded_proposals, audited) and
  dropped. A reconciliation that produces no journal is an ERROR — the
  journal is its one job; tomorrow's plan rides inside it.
- **The aeolus envelope contract is FORTUNA's interface definition**
  (strict serde, deny_unknown_fields): Aeolus's exporter is written TO
  it; the operator fixture validates conformance (GAPS has the exact
  unblock). Zero capital is structural: map_aeolus_envelope returns
  BeliefDrafts only — no proposal type exists in the path. Event ids
  namespace as `aeolus:{event_hint}`; p_raw preserves the raw forecast;
  provenance marks model_id="aeolus". Empty brackets = broken export =
  error, never a silent no-op.
- **One journal per UTC day** (DB unique index; second insert refuses).

## T2.6 — decision cycle, comparator, haircut, triage

- **The comparator handles Direct and Negation edges ONLY.**
  Bracket-component and conditional-on mappings carry composite
  semantics a v1 comparator must not guess at — skipped, never
  mispriced. Fair values floor to integer cents (an edge is never
  rounded into existence); candidates are two-sided (low p buys NO via
  no_ask = 100 - yes_bid) and capped at the displayed ask.
- **Stale beliefs never reach the comparator** (the T2.3 freshness
  verdict is an input); edge-tier policy is enforced per candidate
  (Confirmed demanded where configured).
- **The haircut fails closed:** quality clamps to [0,1]; NaN/non-finite
  => fraction 0 (an unmeasured calibration earns no size). The base
  fraction (0.25 default) and the quality value are inputs; T2.8
  computes quality from the calibration record. (E1 correction,
  2026-06-10: until E1 this lib had ZERO call sites — the claim that the
  runner used it was false. It is now wired: runner sizing for
  calibrated legs = min(kelly_contracts, affordable_sets) with fraction
  = config kelly_fraction x composition-fed quality; unwired quality
  sizes zero.)
- **Shadow sampling is FIRST-K per UTC day** (deterministic and
  replayable; a random sample needs a seed and buys nothing at these
  volumes). Shadow runs produce beliefs that are scored normally and
  NEVER produce trade candidates; a quota-exhausted decline makes no
  mind call and costs nothing.
- **Triage v1 is a rule stub** (AlwaysAccept/AlwaysDecline) behind the
  verdict shape the cheap-model tier will use; the Mind-backed triage
  wires in the live composition (same scoring contract). Per-event
  serialization + debounce stay in the T2.2 TriggerEngine; the cycle
  owns what happens after Fire.
- **The Sim composition of the full loop (StubMind under DST incl.
  cognition-failure scenarios) is Phase 2 EXIT work** after T2.8 puts
  calibration in the path — the cycle machinery is complete and tested
  here.

## T2.5 — Mind trait, StubMind, AnthropicMind

- **AnthropicMind speaks raw HTTP behind a `MindTransport` trait.** There
  is no official Rust SDK (per the claude-api reference consulted at this
  task); the wire format follows the documented /v1/messages contract
  (x-api-key from env, anthropic-version 2023-06-01, adaptive thinking,
  NO sampling params — removed on current models). The transport does
  NOT retry; the decision cycle (T2.6) owns retry policy.
- **The "feature flag" for live exercise is the env key**: the reqwest
  transport constructs ONLY from a non-empty ANTHROPIC_API_KEY and fails
  loudly otherwise — degradation to mechanical-only is explicit, never
  accidental. Live exercise is operator-blocked (GAPS).
- **Model tiering per spec 5.9 as CONFIG defaults:** synthesis =
  claude-fable-5, triage = claude-haiku-4-5. Token PRICES are config too
  (cents per MTok; they change) — documented defaults from the reference:
  Fable 5 $10/$50, Haiku $1/$5 per MTok. Cost = ceil(tokens x price /
  1M), recorded against the budget WHETHER OR NOT the output parses
  (tokens were spent either way).
- **Structured output via output_config.format json_schema**; numeric
  range constraints are unsupported at the schema layer, so probability
  (0,1) and price [1,99] domains re-validate in code post-parse. ANY
  validation failure rejects the WHOLE output — never repaired (5.9).
  A refusal stop_reason surfaces as MindError::Refused, not retried.
- **Provenance is HARNESS-stamped, never model-emitted:** the model
  cannot know its own prompt hash, so the schema excludes provenance and
  AnthropicMind stamps {model_id, context_manifest_hash, cost_cents}
  post-validation. BeliefDraft.provenance is serde-default for exactly
  this flow.
- **Budgets check BEFORE the call** (a breach never spends another cent
  finding out); per-day rolls at 00:00 UTC; per-cycle <= 0 refuses
  outright. The mechanical-only degradation + alert lives in the decision
  cycle (T2.6).
- **StubMind:** scripted outputs in order; an exhausted script yields the
  EMPTY decision (deterministic null), never an error.

## T2.4 — context assembler

- **The budget unit is CHARACTERS, not tokens.** Tokenizers are
  model-specific and non-deterministic across versions; a char budget is
  exact and replayable. The composition root sets it conservatively
  (~4 chars/token rule of thumb belongs in config comments, not code).
- **Packing is greedy in-order:** sections in spec priority order, input
  order within a section; an item that does not fit is SKIPPED and
  counted, later smaller items may still fit. Whole items only — a
  truncated stored item would lie about its hash.
- **"Before the trigger" is STRICT:** an item timestamped exactly at the
  trigger is excluded (and counted). Conservative reading of
  "only data timestamped before the cycle trigger".
- **Hash verification covers every OFFERED item, not just included ones**
  (a corrupted reference poisons replayability whether or not it fits)
  and is fail-closed. Manifest serialization failure is an ERROR, never
  an empty-string hash.
- **Anonymization pseudonymizes the rendered item ids stably within one
  build; the MANIFEST keeps real ids** (replayability is not anonymized;
  the rendered text is what a retrospective evaluator sees). Body-content
  entity stripping beyond ids (tickers inside prose) is the evaluation
  harness's concern at T3.3 — noted, not silently claimed.
- **Injection hygiene at the formatting layer:** bodies render inside
  delimited `<context-item>` blocks tagged with id+section; the Mind's
  prompt (T2.5) instructs that block content is data. I6 + gates bound
  the blast radius regardless.

## T2.3 — belief ledger ops, freshness, scoring

- **Probabilities are STRICTLY inside (0,1).** p = 0 or 1 is a claim of
  certainty — schema-invalid model output, rejected, never clamped or
  repaired (spec 5.9 discipline). NaN/inf likewise.
- **Supersession is transactional:** the new row INSERTs and the prior
  open row flips to 'superseded' in one transaction; the DB content guard
  (T0.8) refuses any change to content fields, proven by a repo test
  that tries.
- **Score-once is repo-enforced over the guard:** resolve_and_score
  updates only `WHERE outcome IS NULL AND status IN (open, superseded)` —
  a second scoring, or scoring an abandoned belief, errors. (Superseded
  beliefs still score: the model said it, the record grades it.)
- **Abandonment excludes from calibration entirely** (event died — the
  world broke the question): `resolved_samples` reads status='resolved'
  only.
- **Freshness:** the pre-benchmark tightened age OVERRIDES the category
  age inside the window (staleness costs most near the benchmark); a
  relevant signal newer than the belief's creation forces refresh
  regardless of age; the comparator-side exclusion of Stale beliefs is
  pinned here and enforced where the comparator lands (T2.6).
- **Calibration curves omit empty buckets** (no fake calibration points)
  and report (mean_p, observed frequency, n) per bucket; grouping
  dimensions (model/category/strategy) are the caller's query via
  `resolved_samples`-style joins.

## T2.2 — signal ingestion + trigger engine

- **Dedup is per-(source, content_hash), receipt-time excluded.** The
  same content re-fetched later IS the duplicate (RSS re-serves are not
  news); the same payload from a DIFFERENT source is a distinct signal
  (corroboration is information). The hash is SHA-256 over
  source/kind/canonical-JSON payload — serde_json's default (sorted) map
  makes key order unable to defeat dedup; the index rebuilds at boot from
  the append-only store (`dedup_pairs`).
- **The registry refuses fail-closed** — unregistered AND disabled
  sources produce `Refused*` outcomes, never partial ingestion. Trust
  tier is 0..=10 by construction (the schema CHECK range; the spec names
  no vocabulary). Registry rows are updatable ON THE RECORD (updated_at);
  demotion evidence lives in belief attribution + audit (T2.3+).
- **Poll-or-push unifies as drain-on-poll:** push adapters buffer
  internally and drain on `fetch`. `received_at` is assigned by the
  ADAPTER at receipt from its injected clock (point-in-time authority).
- **Trigger semantics:** request_cycle is Fire only when no cycle is in
  flight for the event AND the post-completion debounce window has
  passed; in-flight requests count as pending and `complete_cycle`
  RETURNS that count (the caller audits and decides about a follow-up —
  coalesced bursts are never silent). `begin_cycle` is idempotent.
- **Keyword matching is data-only:** case-insensitive substring over the
  payload's STRING values; nothing in any payload is ever interpreted as
  an instruction (5.11 discipline).
- **Triage (cheap-model gate between trigger and frontier) is T2.6** per
  BUILD_PLAN, as is the divergence-rule's belief input (no beliefs yet).

## T2.1 — events, edges, snapshots, CLV

- **Event lifecycle legality lives in cognition; the repo persists.** The
  events table's status is mutable row state (per the T0.8 schema; the
  5.13 lifecycle is enforced by `CanonicalEvent::transition` legal-or-
  error steps before any repo write). Dead reasons are the closed
  vocabulary (voided|source_lost|mutated); ResolvedFinal and Dead are
  terminal.
- **Edge confirmation is a SUPERSEDING INSERT**, never an update: tier =
  Confirmed iff `confirmed_by` is set; `EdgeTier::satisfies` is the
  structural gate (multi-leg/cross-venue strategies demand Confirmed —
  a wrong equivalence edge converts an arb into an unhedged position).
- **Deterministic edge checks:** resolution-source mismatch scores a HARD
  0.0 (different oracles can disagree forever — the UMA-style failure
  mode); horizon mismatch beyond tolerance scores 0.5 (reviewable);
  missing data counts as mismatch, never a pass. Spec 5.12 names the two
  checks; the 0/0.5/1.0 scale is the conservative interpretation.
- **Snapshot scheduler is pure dueness logic:** a scheduled kind
  (T-24h/T-1h/T-5m) fires once its window opens and never at/after
  benchmark_at (post-event oracle-drift exclusion); dedup by
  (event, market, kind). On-trade snapshots are unscheduled — that hook
  lands in the decision cycle (T2.6).
- **CLV math is integer-exact:** own-side mid kept as bid+ask (x2 space),
  bps = (mid - entry)/entry x 10_000 with integer division; NO entries
  mirror to 200 - yes_mid_x2. The liquidity filter requires BOTH touches,
  min size on both, spread cap, and crossing sanity — anything else
  yields None ("no CLV rather than fake CLV"). The job that walks
  resolved BELIEFS and writes their clv_bps lands with the belief ledger
  ops (T2.3); the machinery (clv_bps + latest_liquid_before) is complete.
- **Discovery loops (market-back, world-forward) are T3.2** per
  BUILD_PLAN; T2.1 ships the tables' ops they will write through.

## T1.5 — metrics, dashboard, digest, accounting export

- **"OpenTelemetry" is implemented as the discipline, not the SDK** (this
  is the one place spec Section 8 names a technology whose Rust exporters
  are not yet stable). Research (docs/research/ops/otel-rust-2026-06-10,
  OTel Rust 0.32.0 of 2026-05-09): Metrics API/SDK Stable but the
  Prometheus exporter is Beta and OTLP RC — and stale "discontinued"
  claims about the prometheus crate still circulate, a trap the research
  doc pins. Chosen: a deterministic in-process `MetricsRegistry`
  (BTreeMap-ordered, clock-free, integer-valued — cents/counts/flags
  only, no f64) rendered as Prometheus TEXT EXPOSITION 0.0.4 (stable
  since 2014) at GET /metrics. Spec metric NAMES are kept so adopting
  opentelemetry-otlp later is a transport swap at the exporter edge only.
- **The runner exports plain `MetricSample` data; ops maps it into the
  registry.** The deterministic core carries no telemetry dependency.
  Strategy attribution of PnL/fees: a market's realized numbers attribute
  to the strategy that traded it via the intent set (exact under the
  one-working-order discipline); a market touched by two strategies
  labels `shared` rather than guessing.
- **Phase 2 metric names (CLV, Brier, calibration, model cost, triage)
  are NOT emitted yet** — emitting a constant for an unmeasured quantity
  would be a lie; each lands with its producing subsystem.
- **Dashboard:** read-only by construction (only GET routes exist);
  binds the caller-supplied listener — the composition root binds
  loopback/tailnet only (spec: Tailscale-only is operator network
  config). The Instrument shell is a single dependency-free HTML page
  polling /api/boards.
- **Digest is a pure function** of explicit `DigestInputs`; the honesty
  numbers (halts, open discrepancies, overdue settlements, capital in
  limbo, veto suppressions) always render. Sending goes through the
  audited Slack router (T0.9); scheduling (06:00 UTC morning digest)
  belongs to the live composition binary.
- **Accounting export files are write-once** (`create_new`), named by UTC
  date, checked-both-before-writing-either; corrections are NEW files,
  never overwrites (spec: "immutable ledger file"). venue_class column
  carries the tax class (event_contract | crypto | equity).

## T1.4 — settlement lifecycle, watchdogs, discrepancies

- **The notice stream is THE settlement input.** `Venue::settlements_since
  (cursor)` (new trait method) delivers authoritative venue settlement
  records at-least-once; the runner's processor dedups on notice_id and
  reconciles — it never assumes settlement from its own actions (spec
  5.13). Corrections arrive as NEW notices for the same market.
- **Entry chains are superseding inserts.** pending -> posted ->
  confirmed | reversed; every transition is a NEW entry (the Pg
  settlement_entries table refuses UPDATE; the in-memory
  `SettlementLedger` mirrors that shape). Illegal transitions error.
  A duplicate pending over an unfinished chain is refused; only a
  Reversed head accepts a fresh pending (the corrected re-settlement).
- **Confirmation = venue positions show no residual** for the market
  (its truth incorporated ours). Balance-delta attribution was rejected
  as racy (trading flows interleave); balance drift belongs to a separate
  global watchdog (T1.5 metrics).
- **Reversal restores exact pre-settlement lots.** The runner snapshots
  lots+realized BEFORE applying any settlement; a venue correction
  reverses the books to the cent (clawback = payout the reversed
  settlement credited), re-scores VETO counterfactuals against the
  corrected outcome (correction-flagged rows; the originals stand,
  append-only), then re-settles through the same fresh path. A reversed
  position re-enters ResolutionPending, never tradable Open.
- **Voids abandon veto counterfactuals** (scored neither right nor wrong
  — the world broke the question, mirroring the spec's belief
  disposition); position refund = exact cost basis, realized PnL
  untouched, fees stay sunk.
- **Kalshi settlements mapping:** market_result yes/no map; `scalar`
  settlements are SKIPPED (the catalog filter means we can never hold
  one); any OTHER value — including whatever voids turn out to look like,
  which is UNDOCUMENTED — is a hard error so a real void cannot pass
  silently (fixture-confirmation item in GAPS). notice_id =
  ticker+settled_time (no venue id exists; stable across re-polls).
- **Watchdog constants pinned in code this phase** (config at T1.5 with
  alert routing): settlement-overdue grace = 1h past close_at +
  expected_lag_hours, alerts ONCE per market; books-vs-venue position
  mismatch must persist 3 consecutive ticks (in-flight fills explain
  transient drift) and then writes a discrepancy AND a GLOBAL halt
  (containment: per-strategy attribution is impossible from venue
  positions alone; spec 5.4's freeze-the-strategy needs attribution we
  get in the live composition).
- **Dispute freeze:** the venue catalog is refreshed every tick (statuses
  are watchdog inputs); a Disputed market's held position moves to
  lifecycle Disputed once (out of bankroll, IN exposure at worst case per
  spec 5.13). `MarketStatus::Disputed` is a new variant; the Kalshi map
  now sends `disputed` there (was Determined at T1.1 — refined, the T1.1
  ASSUMPTION line is superseded by this one).
- **Settlement-payout reconciliation:** when a notice carries the venue's
  paid amount, it is compared against our computed payout; mismatch
  writes a `settlement_payout_mismatch` discrepancy, never absorbed.
- **`runner.apply_settlement` remains as a sim-test convenience** routed
  through the same veto-scoring helper; the processor path is the
  production path. Tests that pre-date the processor still use it; both
  paths share scoring exactly-once semantics.
- **DST now exercises void and reversal arms** (1% settle action split:
  re-settle correction on settled markets, 25% void on live ones);
  I-money extends through refunds and claw/repay; I-position excludes
  voided markets like settled ones.

## T1.3 — mech_extremes

- **"Sub-$100k volume" is enforced via a provable upper bound, not an
  undocumented field.** Kalshi's documented-required `volume_fp` (lifetime
  contracts) maps to `Market.volume_contracts` (ceil-parsed: over-stating
  volume keeps the filter conservative). Every traded contract pair
  escrows exactly $1, so dollar volume <= contracts x $1 ALWAYS; capping
  at 100_000 contracts therefore admits only markets that are sub-$100k
  under ANY definition of dollar volume. The `dollar_volume` field seen in
  one raw doc sample is NOT in the documented schema and is not relied on.
  Unknown volume = SKIP (a whale market with a missing field must not
  slip in). This under-selects some genuinely small markets — conservative;
  revisit when fixtures pin `dollar_volume` semantics.
- **"Price extreme" = the favorite side's own-space best bid at/above
  `extreme_min_cents`** (validated 51..=99; composition default 90). In a
  binary market fading the overpriced longshot IS buying the underpriced
  favorite, so the proposal is always a BUY of the favorite.
- **Maker-only is structural:** the limit JOINS the own-side best bid
  (book validity bid < ask guarantees no cross; a defensive re-check
  skips rather than crosses), urgency is Passive, and the never-crosses
  sweep test pins it. No taker escalation path exists in this strategy.
- **`fair_value = limit + bias_premium_cents`, clamped to 99c** (a binary
  is never worth 100c before settlement); if the clamp eats the whole
  premium there is no honest edge claim and nothing is proposed. The
  premium is an operator-tuned config (longshot-bias literature), not a
  fitted parameter — the gates recompute net edge from it (spec 5.3).
- **Catalog guards fail closed:** non-Trading status, unknown close time,
  close nearer than `min_ms_to_close`, and books missing either touch all
  skip. A book snapshot for a market absent from the catalog metadata is
  skipped entirely.
- **One shot per (market, side, limit):** the same book state never
  re-proposes; a moved bid is a new key (the gates' one-working-order
  rule and position caps bound the stack independently).
- **Runner-level doctrine scenarios live in the composed-loop tests**
  (sim_loop / veto_loop / mech_extremes), as established at T0.10; the
  core DST world (gates->manager->venue) gained no new failure modes from
  this task — no strategy code runs inside it.

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
  envelope headroom (`affordable_sets`, floor division); belief trades
  size by haircut-Kelly bounded by affordability (WIRED at E1, 2026-06-10
  — earlier revisions of this entry claimed the Kelly wiring landed in
  Phase 2, which was false until the E1 fix); all re-checked by the gates. Edge distribution across
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
- **Capture tools use wall-clock time at the IO edge** (gate finding F6,
  ledgered 2026-06-11): `fortuna-recorder` (B0) and the
  `record_kalshi_fixtures` example call `SystemTime::now()`/`Instant::now()`
  directly. The injected-Clock rule exists for the deterministic core
  (bus/audit/replay); a live-capture tool's timestamps ARE the data being
  recorded, and neither tool feeds the core event loop. Both crates keep
  their logic halves pure and unit-tested; only their capture loops touch
  the wall clock.
- **ROTA audit-tail query is runtime sqlx, not compile-time `query!`**
  (rota-slices gate F3, ledgered 2026-06-11): `fortuna_ops::rota::
  audit_tail_page` uses `sqlx::query_as::<_, AuditRowTuple>(...)` (runtime)
  rather than the compile-time-checked `query_as!`. Deliberate: this is the
  ONE read-only dashboard query, its columns are pinned by the audit-table
  migration (`audit_id, at, kind, actor, ref_id`), and it is now covered by a
  live `#[sqlx::test]` (cursorless-latest + forward-pagination + empty). Going
  compile-time would couple the whole crate's build to the sqlx-offline cache
  (`.sqlx`) for a single path; the test + schema pin give the same safety
  without that coupling. Revisit if ROTA grows more ledger queries (R7
  cognition panel) — at that point an `ops -> ledger` dep with the ledger's
  prepared queries (design §6) is the cleaner home.
- **degrade_alerts / CalibrationParamsRepo.latest wiring status** (ledger-gate
  fix 1, 2026-06-11 — this entry was CLAIMED to exist by a GAPS line since the
  f-batch closure and never actually written until now; the GAPS line is
  corrected in place): `fortuna_ops::alerts::degrade_alerts` is built and
  tested but its scrape-delta consumer does not exist until the T4.1 daemon
  loop lands; `CalibrationParamsRepo.latest` is built and tested but has no
  live call site until the daemon's calibration binding lands. Anything citing
  them as operational before T4.1 completes overstates.
- **fortuna-recorder assumes a SINGLE WRITER** (ledger-gate fix 4a): JSONL
  append from two recorder processes can interleave partial lines and corrupt
  the day files. Nothing enforces this today beyond operator discipline; the
  T4.4 CLI `start` refusal on an unmanaged recorder process (design A2) is the
  planned enforcement. Until then: never start a second instance.
- **The dead-man heartbeat reads wall time via `RealClock`, not the runner's
  SimClock** (gate finding 2026-06-11 minor 4 — RECONCILED 2026-06-11 after
  the RealClock fix; the earlier "the task reads SystemTime::now()" wording
  was STALE and is corrected here): main's spawned heartbeat stamps each ping
  with `RealClock.now()` (clock.rs — the ONE legal wall-time source, and a
  `Clock` impl) so the liveness signal to the EXTERNAL monitor stays real-time
  even while the trading composition runs on a SimClock during a sim soak.
  This is NOT an exception to the injected-Clock rule — `RealClock` IS that
  rule's sanctioned wall source — so NO ledger exception is needed (consistent
  with the GAPS note). The `deadman_tick` LOGIC takes `now` as a parameter
  (clock-injected, mock-tested); only main supplies it, from the wall via
  RealClock. Zero raw `SystemTime::now()` remain in fortuna-live/src.
