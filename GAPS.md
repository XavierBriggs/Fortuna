# GAPS.md - honesty ledger (agent-maintained)

Open items the implementation defers, lacks, or needs from the operator. Acceptance
requires this file to contain ONLY operator-blocked items, each with exact unblock steps.

Status (post-E-batch, 2026-06-10): the T3.6 completion claim was FALSIFIED
by the full-build gate (docs/reviews/system-0-3-final-2026-06-10.md, BLOCK:
four unledgered Majors). The fix batch (commits 1d1c033..1e3e5e7) closed
E1-E5 and the RE-RUN GATE is an ACCEPT
(docs/reviews/system-0-4-egate-2026-06-10.md): every E-item graded CLOSED
with executed evidence, regression battery clean (630 tests, three-stage
10,000-seed DST). An INDEPENDENT re-gate (docs/reviews/
system-0-4-egate-INDEPENDENT-2026-06-10.md, blind to the first e-gate
verdict, fresh seeds) corroborated all five E-closures on their ledgered
close criteria — and found ONE new Major (F1 below) plus two Minors that
the first e-gate missed. F1-F3 were closed and re-gated (f-batch-gate, ACCEPT-WITH-GAPS; its three
Minors closed at head). Everything below is an OPERATOR action. One Minor stays disclosed: the
regression-seed corpus is empty (no randomized run has produced a red
seed; discipline in place).

## TRACK A — T4.2 item 2(i) WS dial: decision core + session pump + redial LOOP done (dial.rs); only the concrete live transport remains

Queue item 2(i) (Kalshi WS dial). Built the SURVIVAL DECISION core as a pure,
deterministic state machine — crates/fortuna-venues/src/kalshi/dial.rs (new;
`pub mod dial`) — paired with the existing ws.rs message layer (which already
detects seq gaps via KalshiWsEvent::SeqGap):
- DisconnectCause {ResetWithoutClose, ConnectHttpError{status}, KeepAliveTimeout,
  Transport}; DialAction {Subscribe, Redial{backoff}, Resync}; WsDial with
  capped-exponential backoff (base 500ms, cap 30s; reset on a clean connect;
  retries INDEFINITELY — a persistent outage surfaces via the venue error
  counter + health view, not a give-up).
- Behaviors: a reconnect ALWAYS re-subscribes (never assumes a surviving
  subscription); any loss/refusal redials after backoff; a seq gap resyncs
  WITHOUT perturbing the connection-level backoff.
- TDD (RED-first via a stubbed backoff, then the real schedule): the recorded
  venue evidence is the headline test — dial_survives_the_recorded_reset_then_
  502_evidence replays fixtures/kalshi/README.md's 2026-06-13 sequence (healthy
  connect -> mid-stream reset-without-close -> 502 on reconnect -> recovery) and
  asserts subscribe/redial(500ms)/redial(1000ms)/resubscribe. Plus
  a_seq_gap_resyncs_without_touching_the_redial_backoff and the_backoff_is_capped.
  NO live socket in any test (pure state machine).
Full battery green (fmt/clippy --workspace --all-targets/cargo test --workspace =
110 ok-result lines/run-dst.sh 10000 — all exit 0, zero invariant violations).

SLICE 2 (this commit) — the per-connection SESSION PUMP. Added the WsConn seam
(async-trait send/recv) + `pump_session(conn, tickers, next_sub_id, on_event) ->
DisconnectCause`: subscribe ONCE, pump frames through a fresh KalshiWsParser into
the sink, RESYNC in place on a seq gap (resubscribe + reset the parser; monotone
sub-id threaded across reconnects), and return the end-cause (recv error / clean
close / unparseable frame -> reconnect to re-baseline; a failed subscribe ->
its cause). TDD RED-first via a stubbed pump, then a scripted MockWsConn replaying
real doc-shaped frames (subscribe/snapshot/delta + a seq-4-after-2 gap). Three
#[tokio::test]s; NO live socket. (tokio-tungstenite is already a fortuna-venues
dep — the real WS lib for the live transport.)

SLICE 3 (this commit) — the REDIAL LOOP + the WsTransport seam. Added the
WsTransport::connect trait (-> Result<Box<dyn WsConn>, DisconnectCause>) and
run_dial(transport, tickers, dial, cancel, on_event): connect -> on_connected
(reset backoff) -> pump_session UNDER a cancel-select -> on end, on_connection_lost
-> a tokio::select! CANCELLABLE backoff sleep -> repeat, indefinitely until the
watch cancel flips. Both the backoff AND an in-flight pump are cancellable (a stop
never waits out a backoff or a healthy stream). TDD RED-first via a stubbed
run_dial, then a scripted MockWsTransport replaying the CONNECT-LEVEL evidence
(connect-ok->reset, connect-502, connect-ok->recovery): run_dial_survives_a_reset_
then_a_502_and_recovers asserts 3 connects + both snapshots + the pre-reset delta,
cancelling from the sink on recovery. NO live socket. Battery: 122 ok-result lines,
DST 4 corpus + 10000 seeds zero violations.

ARCHITECTURE DECISION (flag for the verifier): tokio moved from a dev- to a NORMAL
dependency of fortuna-venues. run_dial uses tokio::select!/time::sleep/sync::watch
in LIB code, which the dev-only tokio could not satisfy in the strict
`--workspace --all-targets` build (the scoped -p build hid it via dev-dep feature
unification — exactly the cross-crate red the loop doc warns scoped batteries
miss). House-style-compliant ("tokio for IO at the edges"; a venue adapter driving
a live socket IS that edge), and the concrete live transport (below) needs tokio +
tokio-tungstenite as normal deps regardless. If the verifier prefers the loop at
the daemon edge, run_dial relocates to fortuna-live (cheap — it is generic over
WsTransport).

STILL DEFERRED to the NEXT 2(i) slice: the CONCRETE live KalshiWsTransport — a
tokio-tungstenite WsTransport impl doing (b) the signed-handshake connect
(auth.rs recipe; classify a 502 / a reset-without-close into the matching
DisconnectCause) and (c) a ping/pong keep-alive timer (the KeepAliveTimeout cause
is modelled and already redials in run_dial). Its first LIVE exercise is
operator-run. REFINEMENT still open: backoff resets on TCP-connect; a
flap-resistant version resets only after the first healthy frame / Subscribed ack.

## TRACK A RE-ACTIVATED (operator 2026-06-13): completion-campaign queue — M3 DONE (queue item 1)

The operator re-activated the Ralph loop after the RALPH STOP below; the loop doc
now points (b)-priority at docs/design/track-a-completion-queue.md (verifier-
amended), which re-pointed the queue: M3 -> T4.2 buildable-now -> T4.5 -> backlog.
The RALPH STOP below is SUPERSEDED.

M3 REARM NOTICES — DONE (queue item 1; "small, one iteration"). The queue
explicitly released M3 to track A including the fortuna-cli touch. Built across
the three authorized surfaces, TDD where testable:
- fortuna-cli (rearm arm): extracted pure rearm_success_message(); a re-arm now
  also prints "halt cleared in the ledger; the RUNNING daemon resumes only on
  restart — run: fortuna stop && fortuna start". RED-first unit test
  rearm_message_tells_the_operator_to_restart.
- fortuna-live/views.rs (health view): new "rearm_requires_restart" field (= the
  running halt state; honest — I2 is restart-gated, so a running halt clears only
  on restart). RED-first test a_halted_health_view_flags_that_rearm_requires_a_
  restart (clear => not flagged; halted => flagged true).
- fortuna-ops/rota.rs (ROTA surface, queue-authorized): the health panel renders
  the restart guidance when the field is set, and the full-screen #halt overlay
  now carries the restart instruction. Presentation layer (the JS template is not
  content-tested in this repo; the /rota 200 route test guards serving; the DATA
  is the tested views.rs field). Satisfies runbooks/halt-and-rearm.md's design
  intent — the four-state divergence is readable off the console.
DESIGN NOTE: views_from is PURE (no DB), so it cannot compute the TRUE ledger-vs-
running divergence (that needs the R5 read pool in fortuna-ops). The queue's
"simplest honest form" is the always-true I2 fact (a running halt clears only on
restart), surfaced whenever halted — no false divergence claim. A richer ledger-
vs-running comparison (read halt_events via rota_pool, flag only when ledger=clear
but running=halted) is a possible later enhancement, ledgered here.

CROSS-TRACK FMT FIX (committed separately): c139386's T4.2 WS work left a COMMITTED
fmt red on main (crates/fortuna-venues/examples/record_kalshi_fixtures.rs) that
blocked the shared workspace battery. Cleared mechanically (cargo fmt, whitespace
only); T4.2/Kalshi is now track A's queue item 2, so this file is in-purview.

Battery (commit-gate, all real exit codes): fmt --check clean; clippy --workspace
--all-targets -D warnings exit 0; cargo test --workspace exit 0 (110 ok-result
lines, incl. the two new M3 tests); run-dst.sh 10000 exit 0 (zero invariant
violations).

NEXT: queue item 2 = T4.2 buildable-now (Kalshi WS dial first, using the ledgered
venue evidence in fixtures/kalshi/README.md). Also re-check the new (untracked)
docs/reviews/completion-audit-2026-06-13.md as priority (a) next iteration.

## RALPH STOP 2026-06-13T01:16:26Z (Track A — queue exhausted; loop ends clean)

Per implementer-loop.md rule 6 (every priority item blocked/exhausted; idle-and-
stopped beats bloat; do NOT invent work), this Track-A loop stops. The daemon is
SOAK-READY and gate-ACCEPTED (SOAK: GO — 7923255 / soak-go-gate-2026-06-12.md).

DELIVERED this session (each gate-clean — the FULL workspace battery
fmt/clippy --workspace --all-targets/cargo test --workspace/run-dst.sh 10000 ran
green, real exit codes, on every commit):
- 1e5ff71 — ROTA gates view carries the real 1-based spec gate number (R5-slice
  #3); removed the false "a gate number would be a guess" rationale.
- 5120db8 — operator runbook for the Phase-4 EXIT soak (FINAL_REPORT §5) — SOAK-GO
  re-pointed queue item 1; grounded in code (env contract, fortuna CLI, SIGTERM,
  rearm-requires-restart).
- ed17a81 — annotated the two stale M2-disclosure sites RESOLVED-visibly — queue
  item 3.

SOAK-GO RE-POINTED QUEUE (1-4) FINAL STATE:
- item 1 (runbook) DONE (5120db8); item 3 (M2 annotations) DONE (ed17a81).
- item 2 (M3 rearm notices) — BOUNDARY-BLOCKED for Track A. Both surfaces are
  track-B files: the CLI "pending restart" line is fortuna-cli (T4.4) and the
  ROTA health render is fortuna-ops/rota.rs:513-514 (FIELD-SPECIFIC, so a
  views.rs-only field is invisible = bloat). The loop-doc boundary holds even
  over the GATE-FINDINGS re-pointing → needs a track-B owner or an operator
  boundary-waiver. Behavior is already I2-compliant (gate C4 PASS); M3 is the
  operator-VISIBILITY layer only.
- item 4 (T4.2 post-fixture tranche) — operator-blocked (Kalshi fixture session).

BACKLOG #2 (distinct reader/writer-pool boot assertion) — DEFERRED WITH RATIONALE
(supersedes a prior over-optimistic "fully buildable" claim). The wiring is
CURRENTLY CORRECT (main.rs:66 writer max=8; :125 connect_readonly_pool reader
max=2 + 3s timeouts; :138 into RotaState) and the R5 isolation BEHAVIOR is
handler-tested; only main()'s wiring CHOICE is untested. A distinctness assertion
is technically available (sqlx 0.8.6 PgPool::options().get_max_connections()), but
closing it means extracting main()'s pool-pairing into a tested seam — refactoring
the composition root of a SOAK-READY, ACCEPTED daemon to guard a currently-correct
wiring against a hypothetical (poor risk/reward). DISPOSITION: ride with the next
substantive main.rs change (the T4.2 venue-wiring tranche reworks this region),
not a standalone refactor of accepted code.

WHY NO OTHER WORK: perps re-merge + 2x leverage cap + T5.B = track C; T4.3 ROTA
render + T4.4 CLI = track B; the SOAK-GO [Info] items are not non-vacuously
testable now (lessons ORDER BY is counts-only consumption) or are "eventually"
doc notes. Filling iterations with these would violate rule 6.

COORDINATION NOTES FOR THE OPERATOR:
- A concurrent DOCS session is producing a full doc suite (README, AGENTS.md,
  docs/{quickstart,architecture,operations,verification}.md, docs/runbooks/*.md
  ×8 incl. soak-start.md which OVERLAPS FINAL_REPORT §5) — currently BLOCK on its
  OWN gate (docs/reviews/2026-06-12-docs-gate.md). DEDUP needed: FINAL_REPORT §5
  vs docs/runbooks/soak-start.md (likely §5 → a pointer once the canonical
  runbook lands).
- Concurrent design commits (cc099b4, db0ce00) scope a news-aggregation subsystem
  (spec-only; the docs-gate confirms zero source drift).
- HAZARD: a concurrent git operation WIPED an uncommitted GAPS edit of mine mid-
  iteration. Multiple sessions committing/resetting the SAME working tree is
  unsafe for uncommitted work — a further reason this loop stops rather than keep
  churning docs against an actively-mutated tree.

RE-ENGAGE TRIGGER: if the perps re-merge re-gate surfaces a NEW Track-A exec
finding (as the client-id one did → c25b368), it lands in GATE-FINDINGS-LATEST.md
— restart this loop, or route it to an active Track-A session, to address it.

## TRACK A — SOAK: GO received (7923255); verifier re-pointed the queue — item 1 (operator runbook) CLOSED

The verifier's first UNCONDITIONAL ACCEPT (docs/reviews/soak-go-gate-2026-06-12.md;
top of GATE-FINDINGS-LATEST.md) declares the daemon at 8ea8a4d FIT TO START the
7-day Phase-4 EXIT soak (Sim, mock funds; the START itself is the operator's). It
RE-POINTS Track A, in order (this supersedes my prior "#2 distinct-pools next"
self-sequencing — the verifier's queue governs; #2 stays a valid backlog item after):
1. **Operator runbook — CLOSED (this commit).** FINAL_REPORT.md gained "## 5.
   Phase-4 EXIT soak runbook — start/stop/observe (Sim)", closing the gate's
   Minor 2 (the start contract existed only by reconstruction; `grep -rn
   "target/release/fortuna-live" --include="*.md"` found 0 hits → now present).
   GROUNDED IN CODE, not invented: required env from boot.rs::validate_env
   (DATABASE_URL, FORTUNA_SLACK_BOT_TOKEN, the five FORTUNA_SLACK_CHANNEL_*,
   FORTUNA_DEADMAN_URL; ANTHROPIC_API_KEY optional iff [cognition] allow_stub_mind);
   start/stop via the `fortuna` CLI (`start [--foreground]`, `stop [--timeout-secs
   N]`) and the raw `./target/release/fortuna-live config/fortuna.toml`;
   SIGTERM/SIGINT → graceful shutdown; the rearm-requires-restart fact (I2 is
   RESTART-GATED — run_loop.rs:127-136; test
   a_running_daemon_never_auto_clears_a_halt_on_rearm_only_a_restart_does). Ten
   soak-watch metrics cross-referenced to the verdict. Sections renumbered 5→6
   (go-live), 6→7 (watch-first); no cross-refs to those numbers exist. Doc-only,
   zero code delta; full battery still run green (fmt/clippy --workspace
   --all-targets/cargo test --workspace/run-dst.sh 10000 — all exit 0).
   COORDINATION FLAG (operator/orchestrator dedup): a CONCURRENT session created
   an uncommitted standalone ops-doc suite in this shared checkout during this
   iteration — docs/runbooks/{soak-start,halt-and-rearm,kill-switch-drill,
   fixture-recording,key-rotation-and-secrets,troubleshooting}.md + docs/
   {architecture,operations,quickstart,verification}.md + AGENTS.md + README
   edits. docs/runbooks/soak-start.md OVERLAPS this FINAL_REPORT §5. I committed
   FINAL_REPORT §5 because it is the verifier-directed, kickoff-specified
   (FINAL_REPORT.md:126) Track-A item-1 location and their files are not on this
   branch; the two should be reconciled (likely §5 → a pointer to the canonical
   docs/runbooks/soak-start.md once that lands). I did NOT touch the concurrent
   files (loop-doc boundary + never git add -A).
2. **M3 rearm notices (CLI "pending restart" + ROTA health surface) — BOUNDARY-
   BLOCKED for Track A; needs a track-B owner OR an operator boundary-waiver.**
   The directive RE-POINTED M3 to Track A, but its two surfaces are BOTH track-B
   files and the loop-doc boundary "holds even over GATE-FINDINGS pool offers":
   the CLI "pending restart" line is fortuna-cli (T4.4), and the ROTA health
   render is fortuna-ops/src/rota.rs:513-514 — which is FIELD-SPECIFIC (renders
   `j.halt_active`/`j.halt_reason` explicitly), so a views.rs-only data field
   (the only in-boundary slice I own) would be INVISIBLE to the operator =
   dead bloat, not a deliverable. ADJUDICATION: I do NOT cross into track-B to
   satisfy a re-pointing the loop-doc boundary explicitly overrides; M3 awaits
   either track-B picking it up or an operator waiver of the T4.3/T4.4 boundary.
   Compounding signal: a CONCURRENT session is actively editing this exact space
   (uncommitted docs/design/fortuna-cli.md + docs/runbooks/halt-and-rearm.md) —
   building M3 now would also risk a cross-session collision. Must land before
   the operator's first soak halt drill (still true; just not by THIS track
   unilaterally). The behavior itself is already I2-compliant (gate C4 PASS);
   M3 is the operator-VISIBILITY layer only.
3. **Annotate the two stale M2-disclosure sites — CLOSED (this commit).** GAPS
   site (the "[Major M2] ... unbuilt" bullet below) and BUILD_PLAN T4.1
   "HONESTLY DEFERRED" both annotated RESOLVED-visibly (struck/bracketed, never
   erased), citing the M2 slices + the SOAK: GO gate (B1-B4) + the "M2 IS FULLY
   RESOLVED" anchor. Doc-only; full battery still run green.
4. **T4.2 post-fixture tranche — OPEN, still operator-blocked** on the Kalshi
   fixture-recording session.

NEXT-ITERATION POSTURE: items 1 + 3 done; item 2 (M3) boundary-blocked (above);
item 4 operator-blocked. No in-boundary Track-A build item remains on the
re-pointed queue. The backlog #2 (distinct reader/writer-pool boot assertion,
fortuna-live — fully in-boundary) is the remaining buildable Track-A item if the
loop continues past the re-pointed queue; M3 stays blocked pending a track-B
owner or operator waiver.

## TRACK A — T4.1/M2 COMPLETE; "EXHAUSTED" was PREMATURE — two missed ROTA-slice follow-ups taken back (#3 CLOSED, #2 next)

CORRECTION (2026-06-12, this commit): the "buildable valuable work EXHAUSTED"
claim below was a FALSE ledger claim. A fresh priority-(b) sweep found two
genuinely-open, non-blocked, Track-A follow-ups I had skated past — the
"STILL OPEN — TRACK A follow-ups" bullet under the r5test-slice6 gate section
(views.rs + main.rs, owned by Track A, NOT track B/C, NOT operator-blocked):
- **#3 (gate spec-number in the ROTA gates view): CLOSED this commit.** The old
  views.rs rationale "a fabricated gate number would be a guess" was FALSE:
  the runner keys `rejections_by_check` on the GateCheck Debug name, and
  `GateCheck::ALL`/`index()` (both already pub in fortuna-gates) recover the
  exact 1-based spec position from that name — no guess. views_from now emits
  `"number"` per entry (EdgeFloor→6); an unrecognised key (never produced by
  the runner) degrades to null, never a fabricated number. RED-first:
  gates_rejection_view_carries_the_spec_gate_number failed on the absent field
  ({"check":"EdgeFloor","count":1}), passes now. fortuna-gates moved dev-dep→
  dep in fortuna-live (already transitive via fortuna-runner; no cycle). FULL
  battery green (fmt/clippy --workspace --all-targets/cargo test --workspace/
  run-dst.sh 10000 — all exit 0, zero invariant violations).
- **#2 (distinct reader/writer pool boot assertion): STILL OPEN, sequenced next.**
  No test pins that main.rs wires `connect_readonly_pool` (reader) ≠ `connect`
  (writer) — a wiring merge would fail no test (the R5 handler test self-
  constructs its pools). Deferred-not-blocked: the clean fix needs the binary's
  pool-wiring extracted into a testable seam (PgPool exposes no identity/options
  to compare directly), so it is a larger slice than #3 — taking it as its own
  iteration rather than rushing it onto #3's battery.

Overnight 2026-06-12→13: Track A's buildable, valuable T4.1 surface is DONE and
gate-clean (the FULL workspace battery — fmt/clippy --workspace --all-targets/
cargo test --workspace/run-dst.sh 10000 — ran green on every commit). Delivered:
- ALL T4.1 completion-gate findings adjudicated: M1 BLOCK fix (de0426c,
  verifier-cleared in 4daa103), m1 categories-allowlist re-ledger (93844eb),
  m2 refresh-failure integration test (c9edd00), m3 stale-comment fix (de0426c);
  + the R12 halt-rearm option-(a) regression pin (7cc510f).
- Perps-revert client-id finding ADJUDICATED + an executable upgrade-safety pin
  (c25b368): the core idempotency is sound (crash-recovery reads the
  journal-persisted id, never re-derives via the IdGen); the actual fix is
  TRACK-C's kinetics test (verifier option 2). No Track-A code change needed.
- M2 FULLY RESOLVED: daily reconciliation (dbcd941+3d5c18b) + weekly review
  (2fd0d77, 6018ca6, 542e5f3) + monthly review (4cb3391, 8ea8a4d) — all built,
  wired into drive(), tested + mutation-proven. The daemon is feature-complete
  + soak-ready (boot, run-loop, halt poll, graceful shutdown, synthesis arm w/
  mind+calibration+edges+belief-persist, mech_extremes+veto, rich digest, all
  review cadences + schedulers).
REMAINING Track-A items are ALL blocked or other-track: T4.2 (operator Kalshi
fixture-recording session — boot.rs refuses venue=kalshi until clearance), M3
(track-B rearm CLI/ROTA notices — the loop-doc boundary "do not touch track B's
files" holds over the pool offer), the perps re-merge + the operator-confirmed
2x leverage cap (track-C). SUPERSEDED by the CORRECTION above: ONE buildable
Track-A item remains (#2 distinct-pools boot assertion); the rest of this list
holds. The original "No buildable VALUABLE Track-A work remains" was wrong —
the lesson is that "exhausted" must be re-derived from a fresh GAPS sweep each
iteration, not inherited.
STANCE: NOT hard-cancelled. Next buildable item is #2 (above). Each re-fire I
re-check priority (a) (GATE-FINDINGS + GAPS) first — a BLOCK or perps re-merge
finding preempts — then take #2 when its testable-seam slice is ready; absent
either I yield WITHOUT manufacturing low-value work. The operator may cancel the
loop in the morning if no further Track-A work materializes.

## TRACK A — perps-revert client-id finding: ADJUDICATED (core idempotency is UPGRADE-SAFE)

The signed perps merge a586b4a was reverted 19b3888 because kinetics_adapter.rs:
168's deterministic client_order_id shifted in the merged tree (f6384bf5 vs
c445aeac). GATE-FINDINGS routes "exec id-derivation" to Track A and raises the
LOAD-BEARING question: is crash-resubmission idempotency (AlreadyExists dedup)
stable across daemon UPGRADES (code that shifts the id stream)? ADJUDICATED:
YES — the core is upgrade-safe; NO Track-A code change.

ROOT CAUSE: IntentId = self.ids.next(clock) (runner.rs:910) draws from the
shared SEEDED IdGen, so it is SEQUENCE-dependent — merging perps (which consumes
the same stream before an order) shifts NEW-intent IntentIds, hence
ClientOrderId::from_intent = "fortuna-{intent}" (market.rs:168, pure), hence
track-C's downstream UUID. EXPECTED for NEW intents: the IdGen gives
byte-identical WITHIN-build replay; a new build is a new program, and cross-
build NEW-id stability is not a replay requirement.

WHY IDEMPOTENCY IS SAFE (the scary part does NOT apply): crash-recovery NEVER
re-derives a crashed order's id via the IdGen. boot_reconcile matches the
venue's open-order client_order_id against by_coid — rebuilt from the JOURNAL
FOLD (the PERSISTED ids) — and ADOPTS (manager.rs:778-798) or CLOSES (577 path).
The persisted "fortuna-{intent}" is reused across builds because it is READ, not
recomputed. NEW executable pin isolating this (the crash_resubmission test's
candidate(seed) re-derivation is IdGen-pure and CONFOUNDS the proof):
crash_recovery_adopts_a_resting_order_via_its_persisted_client_order_id
(manager.rs) — a resting order is adopted on boot via its persisted id, no
re-gating; mutation-proven (forcing the by_coid match to None orphans it, RED).

DISPOSITION: the verifier's option 1 (make the derivation context-free) is
UNNECESSARY and would HARM — it changes the IdGen-based new-intent id generation,
breaking byte-identical replay determinism, for ZERO idempotency benefit. The
verifier's option 2 is the RIGHT fix and is TRACK-C domain: kinetics_adapter's
test pins a tree-state-dependent UUID — derive the expectation THROUGH the same
path (or make the recorded-create assertion id-agnostic), not pin a UUID. The
signed merge re-runs after that test fix + re-gate; main is green post-revert.

## T4.1 completion gate (t41-completion-gate-2026-06-12.md): BLOCK driver M1 FIXED + m3 closed

The independent T4.1 completion gate returned BLOCK (base 8467d0f, head 17245de).
The composition itself graded mechanically sound (all 3 gate mutations held; DST
10000 all stages green; every targeted suite green). The BLOCK had ONE
mechanical driver, now fixed:
- [Major M1, BLOCK driver] FIXED: 304f746 added synthesis_cents +
  [gates.per_strategy.synthesis] to config/fortuna.example.toml but did NOT
  update the example-config pin (fortuna-ops tests/config.rs:84), so
  `cargo test --workspace` exited 101. Fix: synced the pinned envelopes BTreeMap
  to include ("synthesis", 200_000) — a pin TRACKING the deliberate config
  addition, NOT a weakening (verifier-endorsed framing). Proven by the FULL
  `cargo test --workspace` (WS_EXIT=0), not a -p subset.
- [Minor m3] FIXED: stale operator guidance in daemon.rs + main.rs ("synthesis
  trades nothing until the operator config adds a synthesis_cents envelope…")
  was false since 304f746 closed that gap; corrected to state the gap is closed
  and synthesis trades when a real mind is keyed + the [synthesis] arm composed.
LESSON (root cause, verifier D3): per-crate scoped batteries (304f746 ran
`-p fortuna-live` only) MISS cross-crate pins — the example-config pin lives in
fortuna-ops. RULE: any change to config/fortuna.example.toml (or shared config
types) MUST run the FULL `cargo test --workspace` as the commit gate, never a
-p subset. The DoD/loop-doc already require --workspace; M1 is why.
RE-GATE BATTERY (this commit, full + real exit codes): fmt --check 0; clippy
--workspace --all-targets -D warnings 0; cargo test --workspace 0; run-dst.sh
10000 0 (corpus replay + 10000 seeds; synthesis/settlement/daemon_smoke green).
REMAINING gate findings (NOT this commit; queued):
- [Major M2 — RESOLVED 2026-06-12] ~~daily reconciliation + weekly/monthly
  reviews unbuilt~~ (ticked box, named contract items deferred). NOW BUILT:
  daily reconciliation (slices 1-2), weekly review (A/B1/B2), monthly review
  (C1/C2) — all wired into drive(), tested + mutation-proven; the SOAK: GO gate
  (docs/reviews/soak-go-gate-2026-06-12.md, criteria B1-B4) graded each on
  executed evidence. See "===> M2 IS FULLY RESOLVED" below. The original
  "Operator waive-or-subtask decision" is moot — resolved by building, not
  waiving; the verifier's sub-checkbox recommendation is superseded by the
  executed-evidence gate. (Annotated per the SOAK: GO re-pointed queue item 3.)
- [Major M3] rearm docs 1/3 — ASSUMPTIONS done; CLI + ROTA notices are track-B
  (GAPS:148-163); behavior itself I2-compliant (gate C4 PASS). Should land
  before the soak's first halt drill. NOTE: 7cc510f (post-gate, unseen by this
  verdict) added the explicit option-(a) regression pin.
- [Minor m1] RESOLVED via re-ledger (deliberate deferral, not implemented).
  Decision-doc req 4 lists three [synthesis] filters: categories allowlist,
  venue, max-edge count. Venue + max_edges (deterministic edge-id truncation)
  ARE built + tested (compose::synthesis_edges); the CATEGORIES ALLOWLIST is
  NOT. `SynthesisSection.category` is the CALIBRATION-scope selector (keys
  calibration_for_scope), a DIFFERENT concern — never an edge-category filter.
  The old "(category allowlist deferred to S3b)" pointer (corrected inline at
  the S3a entry below) was STALE: S3b closed without it. DISPOSITION:
  deferred-by-choice as an OPTIONAL narrows-only filter — the verifier confirms
  its absence is NOT fail-open, and it is redundant with the existing narrowing
  (venue filter + max_edges + the confirmed-only gate + the operator deciding
  which edges get confirmed). NOT soak-critical. IF later wanted
  (events-category-join rationale): add `categories: Option<Vec<String>>` to
  SynthesisSection and, in synthesis_edges, retain only edges whose event
  category (edge.event_id -> events.category, a non-overlapping EdgesRepo join
  like confirmed_edges) is in the allowlist; deterministic + tested. Deviation
  recorded in docs/design/synthesis-edge-source-decision.md req 4.
- [Minor m2] CLOSED: the R5/R2 refresh-failure INTEGRATION test is now committed
  — daemon_smoke.rs::refresh_failure_keeps_last_known_edges_alerts_and_survives:
  a failing per-segment edge refresh KEEPS last-known + ALERTS (audit row) + the
  loop survives to clean shutdown. Non-vacuous by the superseding-UNCONFIRMED
  construction (confirmed_edges() asserted empty pre-drive, so a successful
  refresh would read 0); mutation-proven (swapping the live pool for the broken
  one drops the count 1->0 + no alert, RED). Full workspace battery green.

## OPERATOR-INFRA — disk hit ENOSPC mid-session (HARD-BLOCKS the build battery)

2026-06-12 (overnight): the machine disk (/System/Volumes/Data, 926Gi; ~10Gi
free at session start) reached 100% (ENOSPC) during the post-commit
investigation after 7cc510f. ATTRIBUTION CORRECTED per the T4.1 gate verdict's
[Info] note: the dominant cause was the VERIFIER session creating a SECOND
scratch CARGO_TARGET_DIR (/tmp/fortuna-gate-target) against the disk-hygiene-v2
single-target rule (it freed ~30GB mid-session); my clippy --all-targets +
run-dst.sh builds on the shared crates target were a contributing, not sole,
factor. At ENOSPC the harness cannot open a
command's output file, so NO bash command (build/test/commit/even `rm`) can
launch: the loop's DoD battery is hard-blocked, and no code commit can be
gate-clean until the disk has stable headroom for a cold workspace build.

REMEDIATION APPLIED (this session): `cargo clean` (frees crates target,
~21-33G; freed disk from 0 -> 12Gi+ and climbing as it runs). SHARED-TARGET
RISK noted: per the battery-ops memory, target/ is shared with the verifier
session + rust-analyzer; cargo clean wipes their artifacts too. No other
cargo/rustc build was active at clean time (verified via ps), so no in-flight
build was corrupted — only a cold-rebuild cost imposed on the next battery in
any session.

OPERATOR UNBLOCK (exact):
1. Free durable space — the volume is 99% full of NON-target data; a
   `cargo clean` only buys a temporary window the next cold build re-consumes
   (a single --workspace --all-targets build is ~35G, right at the edge).
2. Address data/perishable/ growth (the recorder runs continuously; the
   purge/retention finalization is already a parked operator item). Do NOT
   kill the recorder — the operator rotates/purges out-of-band.
3. Consider a dedicated/larger volume for target/ (it is shared across the
   implementer + verifier + rust-analyzer, so it churns under every battery).

## TRACK A — NEXT ITEM (scoped, ready to execute when the disk is stable): daily reconciliation wiring

T4.1 (BUILD_PLAN 282-283) requires "daily reconciliation 00:00 UTC;
weekly/monthly reviews"; both were honestly DEFERRED post-tick (BUILD_PLAN
375-377) and are the EXIT "boot reconciliation" surface (line 511). The
cognition logic EXISTS and is Track-A-CONSUMABLE (not a fortuna-cognition
edit): fortuna_cognition::reconciliation::run_reconciliation(mind,
context_items, now) -> ReconciliationOutcome { journal, beliefs,
discarded_proposals, manifest_hash, cost_cents }. It is STRUCTURALLY order-free
(the outcome type carries no order; mind proposals are counted + discarded) —
an I6-aligned property worth an explicit test. Wire it into drive's daily block
(where rich_daily_digest already fires via DailyScheduler.due()):
  1. Assemble ContextItems from the day: fills + open positions (runner/
     manager) + originating beliefs (BeliefsRepo, read-only), point-in-time.
  2. run_reconciliation with the daemon's existing Arc<dyn Mind>. Default
     unscripted StubMind -> MindOutput::empty() -> journal None ->
     ReconError::NoJournal: handle as a GRACEFUL SKIP + one audit/alert, never
     a crash. Journal-producing mind (real Anthropic, or a scripted stub in
     tests) -> persist JournalDraft + beliefs (existing persist_beliefs) +
     audit discarded_proposals + cost.
  3. NO orders placed (structural; assert it).
  TESTS (daemon_smoke, scripted minds — the S5/S6 pattern): journal-producing
  mind -> journal persisted + zero orders + discards audited (mutation-proven);
  empty StubMind -> graceful skip + alert, no crash, zero orders.
  VERIFY FIRST: does fortuna-ledger have a journal repo/table for JournalDraft?
  If absent, scope persistence as a non-overlapping ledger addition (like
  EdgesRepo was) OR ledger as cross-track. This determines whether the slice is
  fully Track-A or needs a ledger touch.

DESIGN-VALIDATED 2026-06-12 (Explore map; ready to build). VERIFY-FIRST
RESOLVED: JournalRepo EXISTS (repos.rs:1170) => the slice is FULLY Track-A, no
ledger migration. Validated API:
- run_reconciliation(mind: &dyn Mind, items: &[ContextItem], now) ->
  ReconciliationOutcome{journal: Option<JournalDraft{body:String}>, beliefs,
  discarded_proposals, manifest_hash, cost_cents}; Err(NoJournal) when journal
  is None (a stub mind's MindOutput::empty()).
- JournalRepo::insert(journal_id, day, body:&Value, created_at). Table `journal`
  has a UNIQUE index on `day` (ONE journal/UTC-day) + append-only trigger;
  JournalRepo::get_day(day) -> Option<JournalRow> gives idempotency.
- Context: ContextItem{item_id, section: SectionKind, body, content_hash, at};
  build from runner.manager().intents() (fills, cum_filled>0) +
  runner.positions().positions() (open positions) as AccountState items.
- Audit path: runner.apply_external_alert(&mut self, kind, message) writes a
  kind='alert' audit row {source:'daemon', kind, message} — reuse for the cycle
  audit (journal-written/discards/cost) AND the graceful-skip/failure alerts.
- Beliefs: reuse persist_beliefs(pool, drafts, now_iso, id_base).
SLICING (each a complete, gate-clean slice; the full workspace battery is the
commit gate — never a -p subset, loop rule 4):
 - SLICE 1 DONE (this commit): `run_daily_reconciliation(runner:&mut SimRunner<
   PgIntentJournal>, pool, mind:&dyn Mind, now, id_base) -> Result<bool,
   DaemonError>` helper in daemon.rs, NOT yet wired into drive() (no signature
   ripple). Context from counters()+positions-count (one AccountState item;
   beliefs-context deferred); run_reconciliation -> JournalRepo::insert
   (idempotent via get_day, one journal/UTC-day); apply_external_alert audits the
   cycle (journal-written + discarded_proposals + beliefs-count + cost); NoJournal
   / any error => graceful skip + audit + Ok (the daily boundary survives, mirrors
   the refresh-failure arm); NO orders (structural). Tests (daemon_smoke):
   daily_reconciliation_writes_a_journal_and_places_no_orders (mutation-proven:
   skipping the insert turns it RED) + ..._gracefully_skips_when_the_mind_writes_
   no_journal. Full workspace battery green (fmt/clippy --workspace --all-targets/
   cargo test --workspace/run-dst.sh 10000).
 - SLICE 2 DONE (this commit): run_daily_reconciliation wired into drive()'s
   daily block, INSIDE the same `if daily.due(now)` as the digest (one due()
   fires both); new `reconciliation: Option<(PgPool, Arc<dyn Mind>)>` drive()
   param threaded from main (reuses the synthesis mind via .clone(), built before
   `pool` moves into the halt poller); a reconciliation DB failure alerts to #ops
   but never crashes the boundary; journal id_base = now.epoch_millis() (unique
   per day — no PK collision across a multi-day run). The 5 existing drive() call
   sites pass reconciliation=None; new e2e test
   drive_runs_daily_reconciliation_at_the_utc_day_boundary (mutation-proven:
   neutering the wiring drops the journal, RED). Full workspace battery green.
   ===> DAILY RECONCILIATION is now FULLY WIRED (slice 1 helper + slice 2 loop).
   REMAINING M2 sub-item: the weekly/monthly REVIEWS (fortuna_cognition::review).

## TRACK A — M2 weekly/monthly REVIEWS: DESIGN-VALIDATED 2026-06-12 (Explore map)

HONEST FRAMING FIRST: the daemon North Star (built, gated, soak-ready + daily
reconciliation) is MET. The reviews are M2's SECOND disclosed-but-unbuilt item;
M2 is an OPERATOR waive-or-build decision. The weekly review fires ~ONCE in a
soak week (EXIT-relevant); the MONTHLY won't fire in a week (low soak value).
Both are ADVISORY ONLY (recommendations; promotion is the human act, I7).

weekly_review(mind, context_items, records:&[ScopeRecord], prior_versions:
&BTreeMap<ScopeKey,u32>, strategies:&[StrategyRecord], thresholds:
&GoNoGoThresholds, now) -> WeeklyReview. DETERMINISTIC CORE (calibration_report
+ go_nogo) computes FIRST and survives any mind outcome; commentary + lesson
candidates layer on top (so it produces output even with a StubMind — unlike
reconciliation's NoJournal-skip). monthly_review(strategies:&[AllocationInput],
active_lessons:&[LessonStatusView], now) -> MonthlyReview (NO mind; pure).

INPUT SOURCES (validated):
- ScopeRecord{key:ScopeKey{model_id,strategy,category}, samples:Vec<(f64,bool)>,
  clv_bps:Vec<f64>} <- BeliefsRepo::resolved_stats(category) (repos.rs:1118,
  returns ResolvedStat{p,outcome,brier,clv_bps}). MEDIUM (~80 LOC assembly loop
  per scope).
- prior_versions <- CalibrationParamsRepo::latest(model,strategy,category,kind)
  .version, once per scope. EASY-MEDIUM.
- StrategyRecord{strategy,kind,paper_days,resolved_beliefs,realized_pnl_cents,
  fees_cents,clv_mean_bps,invariant_violations} <- digest_snapshot()'s
  DigestStrategyRow covers strategy/pnl/fees EXACTLY; the rest by DAEMON-LEVEL
  APPROXIMATION (no exact per-strategy source — documented honestly):
  paper_days = daemon uptime in days; resolved_beliefs = resolved_stats(synth
  category).len() for the synthesis arm / 0 for mechanical; clv_mean_bps from
  resolved_stats; invariant_violations = 0 (healthy daemon; aggregate is 0 — a
  per-strategy histogram is a ledgered refinement). HONEST-MEDIUM.
- GoNoGoThresholds{min_paper_days_mechanical,min_resolved_beliefs_synthesis,
  max_fee_pnl_ratio}: NO config source exists -> NEW [review] config section
  (FortunaConfig + example.toml + validate). MEDIUM, multi-file.

PERSISTENCE: WeeklyReview -> JournalRepo::insert (JSON body) + MessageKind::
Digest to #fortuna-digest. lesson_candidates -> MessageKind::Review (PROPOSE
ONLY, I7 — the daemon NEVER calls LessonsRepo::insert; the operator promotes).
CADENCE: NO WeeklyScheduler/MonthlyScheduler exist -> NEW, copy DailyScheduler's
fire-once-per-period pattern (weekly = epoch_days.div_euclid(7); monthly =
year-month key).

SLICE PLAN (full workspace battery is the commit gate, every slice):
 - Slice A DONE (this commit): [review] config (compose::ReviewSection ->
   GoNoGoThresholds via to_thresholds; DaemonToml.review opt-in + example.toml +
   the parse test) + WeeklyScheduler (Monday-aligned 7-day window,
   (epoch_day+3).div_euclid(7)) + MonthlyScheduler (calendar-month "YYYY-MM"
   key) in daemon.rs, copying DailyScheduler. Tests:
   review_section_parses_from_the_committed_example_and_is_optional (boot) +
   weekly/monthly scheduler fire-once-per-period (run_loop, both transitions
   asserted; week keys computed + verified). Full workspace battery green ON THE
   POST-REVERT TREE (the track-C perps merge a586b4a was reverted 19b3888
   mid-session for client-id instability; slice A is orthogonal to perps).
 - Slice B1 DONE (this commit): daemon::run_weekly_review helper — assembles
   ScopeRecord (resolved_stats over [synthesis].category) + prior_versions
   (CalibrationParamsRepo::latest) + StrategyRecord (digest_snapshot + the
   approximations: paper_days=uptime, resolved_beliefs=scope count, invariant_
   violations=0), calls weekly_review (deterministic core: calibration + GO/NO-GO
   recs, I7 recs-only), audits the cycle, returns the WeeklyReview. NOT wired into
   drive() yet. PERSISTENCE CHOICE: audit-only (NO journal write — the journal is
   the daily reconciliation's one-row-per-UTC-day surface and the weekly fires on
   the same day boundary => unique-`day` collision; the audit row is durable, the
   Slack #digest summary is slice B2). Test (daemon_smoke): seeds 50 resolved
   beliefs + params, runs with a StubMind, asserts the deterministic core read
   all 50 (calibration[0].n==50) + produced GO/NO-GO recs + no commentary +
   audited; mutation-proven (dropping the samples => n=0, RED). Full workspace
   battery green (daemon_smoke 12/12).
 - Slice B2 DONE (this commit): run_weekly_review wired into drive() via a
   ReviewWiring struct (bundles pool+mind+ReviewSection+synth_category+start+
   WeeklyScheduler into ONE Option drive() param, threaded from main reusing the
   synthesis mind via .clone()). The WEEK boundary (separate scheduler from
   `daily`; both fire on a Monday) runs the review + routes the WeeklyReview to
   Slack — #digest (calibration + GO/NO-GO summary), #review (lesson candidates,
   PROPOSE-ONLY, I7); a failure alerts #ops but never crashes the boundary. The 6
   existing drive() call sites pass None; e2e test
   drive_runs_the_weekly_review_at_the_week_boundary (mutation-proven: neutering
   the wiring drops the audit, RED). Full workspace battery green (daemon_smoke
   13/13). ===> WEEKLY REVIEW is now FULLY WIRED (slice A foundation + B1 helper
   + B2 loop).
 - Slice C1 DONE (this commit; spec-completeness — won't fire in a WEEK soak,
   serves longer runs): daemon::run_monthly_review helper — assembles
   AllocationInput per strategy (digest pnl/fees + config envelopes + cognition
   cost from counters, synth-attributed) + LessonStatusView (a direct
   `lessons WHERE status='active'` query_as, no new repo method needed), calls
   the PURE monthly_review (allocation recs + cost audit + lessons_due_demotion +
   operator checklist), audits durably. NOT wired into drive() yet. Test
   (daemon_smoke): trades once + seeds an active overdue lesson, asserts
   allocations + the lesson due-for-demotion + checklist + audited; mutation-
   proven (a non-matching status filter => 0 lessons, RED). Full workspace
   battery green (daemon_smoke 14/14).
 - Slice C2 DONE (this commit): run_monthly_review wired into drive()'s review
   block via a MonthlyScheduler + envelopes field on ReviewWiring (no new drive()
   param — reuses the bundled review param). The month boundary (its own
   scheduler) routes the allocation/cost summary to #digest + the operator drills
   (kill-switch test, backup restore) to #ops (I7 — operator action). 6 drive()
   call sites pass None; e2e drive_runs_the_monthly_review_at_the_month_boundary
   (mutation-proven). Full workspace battery green (daemon_smoke 15/15).
   ===> M2 IS FULLY RESOLVED: the verifier's two disclosed-but-unbuilt items
   (daily reconciliation re-run + weekly/monthly reviews) are ALL BUILT + WIRED +
   gate-clean. The operator's M2 waive-or-build call is now moot (everything is
   built). [Honest caveat: the monthly review won't FIRE in a continuous-WEEK
   soak; it serves longer runs. The weekly + daily — the EXIT-relevant cadences —
   fire during the soak.]
 - Slice C (LOW soak value — won't fire in a week): monthly_review wiring
   (AllocationInput from envelopes+digest; LessonStatusView from LessonsRepo::
   active).
RECOMMENDATION: building Slice A+B completes M2's weekly review (EXIT-relevant);
monthly (Slice C) is deferrable. The daemon is soak-ready WITHOUT any of this, so
this is gate-clean COMPLETENESS work, not soak-blocking — the operator's M2
waive-or-build call governs whether it ships.
DEFERRED (follow-on, ledgered): beliefs-CONTEXT enrichment (originating beliefs
into the reconciliation context — needs a BeliefsRepo recent-read; slice 1 uses
fills+positions context, faithful + sufficient for the scripted-mind tests). The
weekly/monthly REVIEWS (fortuna_cognition::review) are a SEPARATE M2 sub-item
AFTER reconciliation. M2 bookkeeping (waive-with-sub-checkboxes vs un-tick) stays
the operator's call; building these items resolves the underlying gap regardless.

## Engineering items F1-F3: CLOSED (gate-verified)

Found OPEN by the independent e-gate; closed by b4c839f (F1: degrade
kind preserved + drained to 'cognition' audit rows + bus events, budget
breaches counted once at the drain and exported; ops alerts module —
every breach alerts with the scrape count, failure bursts threshold-
gated; F2: BUILD_PLAN T2.8 visible correction; F3: wholly-discarded
output writes a model_proposals_discarded trace) and graded CLOSED by
the targeted re-gate (docs/reviews/f-batch-gate-2026-06-10.md,
ACCEPT-WITH-GAPS). That gate's three Minors closed at the head commit:
the settlement_dst aggregate coverage asserts now gate on a 100-scenario
floor (a 20-scenario draw can legitimately miss the halting arms — a
coverage assert that intermittently reds healthy code erodes trust in
real reds; repro master 1781139292562 now passes), the cost-metric
undercount gained the budget-true surface (fortuna_mind_spend_today_cents
includes failed-call burn, test-asserted on a schema-invalid call), and
the false "cost rides in cognition audit rows" doc line was corrected
visibly in ASSUMPTIONS (per-decision cost rides in belief provenance).

Verification record: five verdicts in docs/reviews/ (phase-1, phase-2,
system-0-1-2, phase-3, system-0-3-final, all 2026-06-10). The phase-3 gate
and the system gate disagreed on acceptance-item grading; the disagreement
was adjudicated with executed evidence (zero Kelly call sites, no
`impl Mind for AnthropicMind`, vacuous per-cycle budget, zero discrepancy
hits in seeded DST — all confirmed at 7bbc3ef). The system gate's stricter
reading governs.

## Engineering items E1-E5: CLOSED (gate-verified)

Ledgered open at 7bbc3ef; closed by 1d1c033 (E1+E5a: calibration layer
binds in the cycle, haircut-Kelly sizing = min(kelly, affordable) with
composition-fed quality failing closed to zero, integer-money Kelly
budget), E2 commit (AnthropicMind behind `dyn Mind` with owned budget +
env-gated factory), 5954999 (E3: per-cycle cap binds via begin_cycle
tracking; config surface added), ca38028 (E4: 10-arm settlement/watchdog
seeded DST as run-dst.sh stage 4, fail-closed script), 1e3e5e7 (E5:
watchdog outage partition, counted discards + audited proposal manifest
hashes, hygiene). Each close criterion was graded CLOSED with executed
evidence by the independent gate: docs/reviews/system-0-4-egate-2026-06-10.md
(verdict ACCEPT). False documentation was corrected with the correction
visible (ASSUMPTIONS.md, BUILD_PLAN.md T2.6 note), never erased.

## Minor engineering residue: status (from the three INDEPENDENT batch gates)

CLOSED at head (this commit):
- mind-spend gauge exported with the wrong type flag -> moved to the
  gauge block (counter: false).
- Unchecked i64 add in the assembler delta -> checked_add, fail closed.
- Lenient envelope parsing -> STRICT: frames without a type tag refuse;
  orderbook frames without sid/seq refuse (lenient zeros could alias
  real sequence state); pinned by tests.
- Crossed-assembled-book refusal + non-array-side refusal pinned as
  committed tests (were gate-scratch-verified only).
- Out-of-order leg completion pinned: a staggered mock completes legs in
  REVERSE input order; outcomes and journals still land in leg order.
- Multi-leg DST arm added (settlement_dst Arm::MultiLegGroup): two-leg
  groups drive submit_group_concurrent under seeded ack-delay/api-error/
  reject faults; 400-scenario shakeout green, all 11 arms hit.
- settlement_voids / settlement_reversals counters post-state asserted.
- Kelly legs[0] design constraint corrected in ASSUMPTIONS. [CORRECTED
  2026-06-11, ledger-accuracy gate, SECOND-GATE MAJOR: the rest of this
  line previously claimed the degrade_alerts/CalibrationParamsRepo
  ASSUMPTIONS entry and a Polymarket "95" erratum already existed — both
  were false from the f-batch closure until 2026-06-11, when the real
  corrections finally landed: the wiring-status entry is now in
  ASSUMPTIONS.md, and the research doc carries an ERRATUM stating that
  neither "96" nor "95" matched a canonical count (ground truth: 38
  source-table rows; 93 archived raw files). A closure claim that
  survived TWO gates unverified is exactly the defect class this ledger
  exists to prevent.]

T4.1 DAEMON STATUS (2026-06-11, post-composition-main): fortuna-live now
BOOTS AND RUNS — boot-validated config (incl. the committed example) ->
Postgres connect+migrate -> composed SimRunner (mech_structural over the
[sim] world, Pg journal + Pg audit) -> run loop (HaltsRepo poll <=500ms,
ticks on the injected clock; wall time enters ONLY at the binary edge +
cadence driver) -> SIGTERM/SIGINT -> graceful shutdown (cancel + final
audit row; smoke-asserted via the same stop channel, req-10 smoke in
run-dst.sh stage 5) -> GET-only metrics endpoint from config -> degrade
alerts routed to Slack (env-built router) + audited every segment ->
dead-man heartbeat (independent task, WIRED) -> daily-boundary scheduler
-> #fortuna-digest. [UPDATED 2026-06-11 per remediation2 gate: this
block formerly listed the dead-man pinger and scheduled loops as open
AFTER they were wired — a claim-vs-reality stale-ledger defect; corrected
here. The dfb849f commit MESSAGE claimed a GAPS dead-man flip that the
commit did not contain; the flip landed shortly after — recorded for the
trail since commit messages cannot be edited.]
HONESTLY STILL OPEN before the T4.1 tick (the box stays unticked):
- SYNTHESIS COMPOSITION: DONE (S1-S3b, 2026-06-12). compose_runner composes a
  SynthesisStrategy from the confirmed-tier edge load, OPT-IN on [synthesis],
  alongside mech_structural. BUT the arm is INERT: its mind is a StubMind
  PLACEHOLDER and calibration is None, so it structurally prices no edge + makes
  no trade until S5. (The earlier "not fed into a daemon-booted
  SynthesisStrategy" / "composes only mech_structural" claims are now stale —
  corrected here + in daemon.rs.)
- S4: per-segment edge REFRESH in drive() (keep last-known on failure +
  count/alert, never crash) — unwired (edges load once at composition today).
- S5: the mind_from_env / CostBudget binding (StubMind -> AnthropicMind). The
  allow_stub_mind gate exists and the StubMind placeholder now HAS a consumer
  (the composed synthesis arm), but the REAL AnthropicMind is not yet composed.
- S6: belief drain+persist into the booted strategy. The PATH exists + is tested
  (Strategy::drain_beliefs -> runner.drain_pending_beliefs ->
  daemon::persist_beliefs, FK-correct, idempotent); but the StubMind produces NO
  beliefs, so nothing drains until S5's real mind. Then the RICH daily digest +
  daily reconciliation re-run + weekly/monthly cognition reviews.
- mech_extremes-WITH-VETO strategy binding (reduce-only model veto). DONE (this
  commit): compose_runner composes the OPT-IN [mech_extremes] arm (spec Section
  6 item 2) ALONGSIDE mech_structural/synthesis, ENROLLED in veto_strategies
  with veto_mind = StubVetoMind::allow_all() (REQUIRED — a veto-enrolled
  strategy with no mind FAILS to boot, runner.rs:347). The strategy + the veto
  application machinery (consult_veto, counterfactual scoring) ALREADY EXIST +
  are tested (mech_extremes.rs, veto_loop.rs) — this is COMPOSITION wiring only,
  touching ONLY fortuna-live (compose.rs section, boot.rs field+parse, daemon.rs
  arm). [mech_extremes] is a presence-toggle: empty table => conservative
  defaults (extreme_min_cents 90, bias_premium 2, max_volume_contracts 100_000,
  min_ms_to_close 1h), fields optionally override; an out-of-range value is a
  LOUD compose error. INERT in pure-sim: sim markets carry no volume/close
  metadata so market_eligible() skips them — mech_extremes activates only with
  real markets (T4.2); the wiring + veto enrollment is the deliverable. The
  veto mind is a STUB (allow_all, inert) until S5 binds the Anthropic-backed
  veto mind (alongside the synthesis StubMind->AnthropicMind). Test
  (daemon_smoke sqlx::test, TDD red observed): WITH [mech_extremes] the runner
  BOOTS (proving the veto mind wired — else boot fails) + strategy_ids contains
  "mech_extremes"; WITHOUT it, neither (fail closed).
DO NOT tick T4.1 / start the soak until S5 (the real mind) — else the StubMind
degrades every cycle and pollutes the soak metrics.

## T4.1 — R12 halt-rearm finding: ADJUDICATED (option a, restart-gated)
The R12 drill flagged that the running daemon's halt poll APPLIES halts but
never CLEARS on a re-arm (a halt_events kind='rearm' left the daemon halted
until restart; the boot fold DID read set->rearm correctly). ADJUDICATED (a):
this is DELIBERATE + correct per I2 ("no automatic resumption") — the running
daemon never auto-clears; a re-arm requires a human DB rearm PLUS a deliberate
RESTART (boot fold reads set->rearm). A restart is the unambiguous human
resumption act; a poll-driven clear edges toward the daemon resuming on its
own (CLAUDE.md: when the spec is silent, choose the conservative option).
Documented: ASSUMPTIONS.md (the posture) + the run_loop.rs Ok(None) comment
(was misleading — "the halt cleared out-of-band" — now clarifies the GATE stays
halted; the latch reset only re-audits a fresh same-reason halt). Option (b)
(poller clears on rearm) REJECTED: it reverses the deliberate I2 design.
CROSS-TRACK follow-on (track B files, NOT track A's — released to the
orphaned-minor pool): surface "re-arm pending daemon restart" in the `fortuna`
re-arm output (fortuna-cli) + the ROTA health panel (fortuna-ops) so the
operator knows to restart.
REGRESSION PIN (2026-06-12): tests/run_loop.rs::
a_running_daemon_never_auto_clears_a_halt_on_rearm_only_a_restart_does now
asserts option (a) EXPLICITLY — a halt applied then ten Ok(None) "re-arm"
polls leaves the daemon halted (tick.halted), with exactly one halt audit and
ZERO clear/rearm/unhalt audit rows. Mutation-proven non-vacuous: wiring a
gates.rearm into run_loop's Ok(None) arm flips it RED at the tick.halted
assertion (probe added + reverted, never committed). This upgrades the prior
INCIDENTAL coverage (polled_halt_applies_to_the_gates_and_audits, whose
Ok(None) tail happened to cover it) into a named guard against a future
"helpful auto-clear on Ok(None)" refactor — option (b), which I2 forbids.

## TRACK A — SYNTHESIS-IN-MAIN build plan (validated 2026-06-12, no code)

Ownership (orchestration.md): Track A owns fortuna-live, fortuna-runner,
fortuna-venues/src/kalshi*, fortuna-paper. fortuna-ledger is NOT track A's, but
EdgesRepo is DISJOINT from track B's R7 additions (BeliefsRepo::recent +
calibration scopes), so an EdgesRepo method is a non-overlapping addition to
repos.rs (clean merge). fortuna-cognition (Mind/StubMind, EdgeView,
DecisionCycle) is consumed, not edited.

Survey (against synthesis-edge-source-decision.md requirements 1-5):
- SynthesisStrategy (fortuna-runner/src/synthesis.rs, MINE) = SynthesisConfig
  {id, edges: Vec<EdgeView>, comparator, triage, shadow_quota, calibration:
  Option<CalibrationContext>, stage} + new(config, mind: Arc<dyn Mind>). Empty
  edges => quotes() empty => zero proposals (requirement 3 fail-closed already
  holds at the strategy layer; PIN it with a test).
- Edge source: market_event_edges table (edge_id, market_id, venue, event_id,
  mapping_type, confidence, proposed_by, confirmed_by NULLABLE, supersedes,
  created_at). CONFIRMED = confirmed_by IS NOT NULL; CURRENT = NOT EXISTS a row
  superseding it. EdgesRepo has current_edges_for_event/_market but NO
  confirmed-load. NEEDS: EdgesRepo::confirmed_edges() (+ filters). NOTE:
  fortuna-ledger uses compile-time sqlx::query! -> a new query! needs `cargo
  sqlx prepare` to refresh the .sqlx offline cache (else clippy/offline build
  misses); verify sqlx-cli before that sub-slice.
- EdgeView (fortuna-cognition/src/cycle.rs) is the strategy's edge type; map
  EdgeRow -> EdgeView at the composition (fortuna-live).
- Mind: compose_runner composes only mech_structural today (vec![MechStructural]).
  SynthesisStrategy needs a Mind; allow_stub_mind gate exists (boot.rs). StubMind
  first; AnthropicMind via mind_from_env is the mind-binding sub-slice.
- Calibration: compose::calibration_for_scope EXISTS + tested (fortuna-live, MINE).
- Config: [synthesis] filters (categories allowlist, venue, max_edges with
  deterministic truncation by edge id) belong in DaemonToml (fortuna-live/boot.rs,
  MINE), NOT fortuna-ops/config.rs.
- Stage: composition derives via promotion::effective_stage(declared_cap,
  operator_records) — never self-promote (I7).

Build sub-slices (each its own iteration, TDD, battery-gated):
  S1. EdgesRepo::confirmed_edges() (+ sqlx prepare). DONE (this commit):
      confirmed_edges() loads confirmed_by IS NOT NULL AND non-superseded edges
      (ORDER BY created_at, edge_id); test confirmed_edges_returns_confirmed_
      current_heads_only seeds 6 edges and asserts the load == exactly the 2
      confirmed-current heads [cf-head, cf-new], with the unconfirmed (unconf),
      the superseded confirmed (cf-old), and the req-5 conservative case
      (cf-base confirmed but superseded by an UNCONFIRMED reproposal -> neither)
      all excluded. TDD red was OBSERVED (stub -> Ok(vec![]) -> assertion
      left:[] right:[cf-head,cf-new]); .sqlx offline cache refreshed. The
      EdgesRepo addition is DISJOINT from track B's R7 BeliefsRepo/calibration
      additions in repos.rs (decision-authorized by synthesis-edge-source-
      decision.md req 1; clean merge anticipated).
  S2. SynthesisStrategy empty-edge fail-closed PIN (fortuna-runner test).
      DONE (this commit): empty_edge_set_fails_closed_but_a_present_edge_trades
      in synthesis_loop.rs — requirement 3 pinned NON-VACUOUSLY by the
      empty-vs-present contrast (the SAME mind+book that trades with the KX-A
      confirmed edge present produces zero proposals + no position when the edge
      set is empty, proving the edge set load-bearing). The EdgeRow->EdgeView map
      moves to S3 (it belongs at the composition where EdgeRow is loaded).
  S3-prep DONE (this commit): SimRunner::strategy_ids() accessor — the seam
      the S3 composition test asserts on (WHICH strategies booted). Was MISSING;
      without it the composition is untestable (compose_runner builds its own
      mind, so behaviour-testing needs no injection seam either). TDD red
      observed (stub Vec::new() -> [] != ["synth_sim"]).
  S3a DONE (this commit): compose::synthesis_edges(pool, &SynthesisSection)
      -> Result<Vec<EdgeView>, ComposeError> — loads EdgesRepo::confirmed_edges,
      filters by venue, truncates by edge id (max_edges), maps EdgeRow->EdgeView
      (match mapping_type snake_case; tier=Confirmed), Err on a corrupt
      mapping_type (ComposeError::BadEdge) so S4's refresh keeps last-known.
      SynthesisSection = {venue, max_edges} (category allowlist deferred to S3b
      — needs an events-category join). [CORRECTION 2026-06-12: "deferred to S3b"
      went STALE — S3b closed WITHOUT the allowlist. Now a deliberate optional-
      filter deferral; see the [Minor m1] disposition near the top.] TDD red
      OBSERVED (stub Ok(vec![]) ->
      len 0 != 1); sqlx::test seeds confirmed kalshi + polymarket + an
      unconfirmed edge and asserts venue/max_edges filtering + the mapped fields
      (NON-VACUOUS). DISK NOTE: a warm fortuna-live battery is ~1-2GB (measured),
      NOT the ~15GB I'd feared — S3 is NOT disk-blocked.
  S3b-1 DONE (this commit): the [synthesis] OPT-IN config —
      DaemonToml.synthesis: Option<SynthesisSection> (+ RawToml + parse). Its
      PRESENCE composes synthesis (S3b-2 wires that); ABSENT => mechanically-only
      (fail closed). Parse test: the committed example has no [synthesis] => None;
      appending one parses venue/max_edges (NON-VACUOUS).
  S3b-2 DONE (this commit): compose_runner composes the synthesis arm GATED on
      dcfg.synthesis.is_some(). SynthesisConfig {id "synthesis", edges via
      synthesis_edges (ComposeError mapped to DaemonError::Compose), comparator
      {5, Confirmed}, triage AlwaysAccept, shadow_quota 0, calibration: None
      PLACEHOLDER, stage Stage::Sim} + Arc::new(StubMind::scripted(vec![]))
      PLACEHOLDER, pushed to `strategies`. Composition test (daemon_smoke.rs,
      sqlx::test): with [synthesis]+a seeded confirmed sim edge, runner.
      strategy_ids() contains BOTH "synthesis" and "mech_structural"; without
      [synthesis], only "mech_structural" (fail closed). Battery 47/0. daemon.rs
      doc corrected (the stale "synthesis not yet fed into a daemon-booted
      SynthesisStrategy" claim is now FALSE — replaced with the S3b reality +
      the S5/S6 remainder). ===> S3 (the daemon synthesis COMPOSITION) is
      COMPLETE: S1 edge source + S2 fail-closed PIN + S3a load/filter/map +
      S3b-1 opt-in config + S3b-2 wiring. The arm is INERT (StubMind, calibration
      None) — do NOT tick T4.1 / start the soak until S5 binds the real mind.
      Remaining T4.1 tail (loop-doc order: synthesis-in-main -> mech_extremes
      +veto -> mind binding): S4 per-segment refresh DONE; mech_extremes+veto
      DONE; S5a mind binding DONE (synthesis mind PARAM + "synth_events"-scoped
      calibration; arm trades a seeded edge); S5b mind_from_env DONE (synthesis
      side: AnthropicMind when keyed, else StubMind; main wires it from env);
      S6a belief drain+persist WIRED into drive() DONE; S6b RICH digest (terse ->
      DigestInputs composition) DONE. T4.1 BOX TICKED 2026-06-12 (tail commits
      64d45db..304f746; the daemon is built + battery-gated; the verifier gates
      the batch AT the tick per GATE-FINDINGS; the Sim soak is OPERATOR-started —
      outward-facing secrets + the key + a release build — NOT autonomously
      started). NEXT (post-tick Track A, while T4.2 stays operator-blocked on the
      fixture session): the daily reconciliation re-run + weekly/monthly cognition
      reviews (scheduled-loop sub-slices). DEMO-CONFIG PREP (this commit): (1) the S5a config gaps are
      CLOSED in config/fortuna.example.toml — synthesis_cents=200_000 envelope +
      [gates.per_strategy.synthesis] (so the operator's copy supports the
      synthesis arm); (2) FIXED an S5b bug — CognitionSection's field was `model`
      but the example uses `synthesis_model`, so the operator's model choice was
      SILENTLY dropped to the default; renamed the field to `synthesis_model` to
      match. REMAINING PREREQ before LIVE synthesis (boundary, NOT blocking the
      sim soak which runs mechanically-only / with the stub): AnthropicVetoMind
      must be BUILT by the fortuna-cognition owner (the veto side of mind binding
      — out of Track A bounds). Deferred + inert for the sim soak:
      synth_events_config/effective_stage canonicalization; [synthesis] CATEGORY
      events-join filter; AnthropicMindConfig prices->config; the daemon's
      triage_model/shadow_budget_cents (example fields the synth arm does not use
      yet — AlwaysAccept triage, shadow_quota 0).
  S4. drive() per-segment edge refresh (requirement 2): keep last-known on
      failure + count/alert, never crash. DONE (this commit). ORDER REVERSAL
      (honest, vs 1770c1f which leaned "S5a precedes S4"): the GOVERNING
      implementer-loop.md orders the tail "synthesis-in-main -> mech_extremes
      +veto -> mind binding"; synthesis-in-main = decision-doc req 1-5, and req
      2 IS the per-segment refresh, so S4 COMPLETES synthesis-in-main and
      precedes mind binding. It is also the conservative order: the fail-closed
      refresh is a SAFETY net (never trade a guessed set, never crash) better in
      place BEFORE the mind can trade live. Built CLEANER than the validated
      as_any/downcast (trait polymorphism, no Any): (1) Strategy::edge_count()
      -> Option<usize> + refresh_edges(&[EdgeView]) -> Option<usize>, both
      DEFAULT None (mechanical strategies untouched); SynthesisStrategy overrides
      (wholesale replace; empty set = VALID, req 3). (2) SimRunner::refresh_
      synthesis_edges(&[EdgeView]) -> Option<usize> + synthesis_edge_count() ->
      Option<usize> (iterate strategies; the daemon composes exactly one synth
      arm, refresh handles many defensively). (3) drive() gains synthesis_refresh:
      Option<(PgPool, SynthesisSection)> (ONE Option param; the 3 callers — main
      + daemon_smoke x2 — main passes Some when [synthesis] is set, the smokes
      None). Per segment it re-loads compose::synthesis_edges + refresh_synthesis_
      edges; on Err it KEEPS last-known (simply does not refresh), counts
      edge_refresh_failures, alerts ONCE on the failing transition (edge_refresh_
      transition — the same dedup shape as poll_failing) + surfaces the run total
      on shutdown. 3 NON-VACUOUS tests: (a) runner swap (synthesis_loop.rs)
      1->2->0 edges via refresh_synthesis_edges/synthesis_edge_count, TDD red
      observed (None != Some(1) before the overrides); (b) integration (daemon_
      smoke sqlx::test) boots with 0 confirmed edges (count 0), confirms an edge
      MID-RUN, drives one segment, count 0->1 (the ledger re-read is LOAD-BEARING,
      not boot-cached); (c) the dedup latch (daemon.rs unit) alerts once/outage,
      counts every failure, re-alerts after recovery. FAILURE-PATH NOTE:
      ComposeError::BadEdge is UNREACHABLE from a real DB (mapping_type is
      CHECK-constrained to the 4 values synthesis_edges all handle), so the
      keep-last-known-on-Err path is proven via the pure edge_refresh_transition
      helper, not a corrupt-row injection. BATTERY: fmt clean; clippy --workspace
      --all-targets -D warnings GREEN (all 15 crates); tests GREEN on the COMPLETE
      reverse-dep set of the change (fortuna-runner + fortuna-live + fortuna-
      invariants — the only crates depending on the changed code; the rest are
      behaviorally unaffected by construction) incl. the 3 new S4 tests; run-dst.sh
      GREEN (3 corpus + 2000 seeds + synthesis_dst + settlement_dst). A single
      `cargo test --workspace` invocation was repeatedly OOM-killed / target-thrashed
      by the concurrent multi-track load (load ~29, disk near the 10GB floor) — the
      VERIFIER's independent full battery is the backstop for the unaffected crates.
      The arm is STILL INERT (StubMind, calibration None) — do NOT tick T4.1 until S5.
  S5a. make synthesis TRADEABLE (mind-injection + calibration). SCOPE
      RE-RESOLVED 2026-06-12 — design-validate found the prior note WRONG on two
      load-bearing points (corrected visibly):
      * CORRECTION 1 (correctness): the calibration scope strategy is NOT
        "synthesis". The CANONICAL synthesis strategy is synth_events (fortuna-
        runner/src/synth_events.rs, spec S6 item 4: id "synth_events",
        SYNTH_EVENTS_STAGE_CAP=Paper, MIN_EDGE 5, shadow_quota 3), and the
        calibration convention keys on "synth_events" (the existing compose.rs
        test seeds model="claude-fable-5" / strategy="synth_events" / category /
        "platt"). Querying "synthesis" would fetch NOTHING => silent no-trade
        (the OPPOSITE of tradeable). Scope = ("claude-fable-5","synth_events",
        cfg.category,"platt").
      * CORRECTION 2 (stale): "5 call sites" is now EIGHT (main + daemon_smoke
        x7 after S4 + mech_extremes added tests).
      * ALSO DISCOVERED: the S3b daemon arm is an AD-HOC SynthesisConfig (id
        "synthesis", HARDCODED Stage::Sim, shadow 0, AlwaysAccept) that diverges
        from canonical synth_events AND from GAPS line 198's own stated rule
        ("composition derives via effective_stage ... never self-promote, I7")
        — the hardcoded Sim is a latent I7 drift.
      RESOLUTION (build to this next iteration): canonicalize the daemon
      synthesis arm to synth_events_config(edges, triage, calibration) — id
      "synth_events", stage = promotion::effective_stage(SYNTH_EVENTS_STAGE_CAP,
      operator_promotion_records) [= Sim with no promotions; I7-correct],
      shadow 3; bind calibration_for_scope("claude-fable-5","synth_events",
      cfg.category,"platt") -> SynthesisConfig.calibration + set_calibration_
      quality("synth_events", quality). compose_runner gains mind: Arc<dyn Mind>
      (8 call sites: StubMind for existing, a scripted believing mind for the
      live test). [synthesis].category added = the calibration scope. TEST
      CHURN: the S3b/S4/mech_extremes assertions on strategy_ids "synthesis"
      become "synth_events". MODEL_CONST -> [cognition].model in S5b. Live test
      (daemon_smoke sqlx::test): event(category weather) + a confirmed sim edge
      + a calibration_params row + RESOLVED beliefs (quality>0 => non-zero size,
      per compose.rs's calibration seeding) + a scripted believing mind + a book
      -> tick -> the arm TRADES (proposal + position; non-vacuous populated
      path). main stays StubMind (inert) => production-live is S5b. VETO-MIND
      GAP (separate, OUT of Track A): AnthropicVetoMind does NOT exist (fortuna-
      cognition, which Track A consumes-not-edits; veto.rs said "arrives in
      Phase 2 T2.5" but it never landed) — the mech_extremes veto stays
      StubVetoMind::allow_all until its fortuna-cognition owner builds it.
      DONE (this commit) — MINIMAL variant built (fortuna-live only): compose_
      runner gains mind: Arc<dyn Mind> (8 call sites — main + daemon_smoke x7
      pass StubMind, the live test a believing mind); [synthesis].category added;
      when set, calibration_for_scope(SYNTH_CALIBRATION_MODEL "claude-fable-5",
      "synth_events", category, "platt") -> SynthesisConfig.calibration +
      set_calibration_quality("synthesis", quality). DEVIATION from a814f56's
      "canonicalize to synth_events_config" plan (honest): kept the arm id
      "synthesis" + the calibration SCOPE "synth_events" DECOUPLED (the scope
      keys the fitting pipeline; the id keys set_calibration_quality) — this is
      correct + smaller + needs NO strategy_ids test churn. The synth_events_
      config / effective_stage / id-rename canonicalization is DEFERRED as a
      follow-up: it is INERT until operator promotions exist (effective_stage(
      Paper,[])==Sim, which the hardcoded Sim already equals) and needs an audit
      -> PromotionRecord loader that does not yet exist. Live test (daemon_smoke
      sqlx::test) proves the NON-VACUOUS populated path: seeded calibration_params
      + 50 RESOLVED beliefs (==FULL_AUTONOMY_N so the shrink weight w==1 — at
      n<50 the belief shrinks toward the quote mid and prices NO edge) + a
      confirmed sim edge + a believing mind + a book -> tick -> the synth arm
      PROPOSES + sizes + SUBMITS an order. NEW CONFIG GAPS surfaced (BINDING for
      S5b's live arm; the live test injects both): the synth arm needs (1) a
      `synthesis_cents` ENVELOPE and (2) a `[gates.per_strategy.synthesis]` block
      — the example config has NEITHER (mech_structural + mech_extremes only), so
      without them the arm prices but sizes ZERO / is gate-rejected fail-closed.
      The example + operator config MUST add both before S5b binds the real mind.
      main stays StubMind (inert) — production synthesis is dark until S5b.
  S5b. mind_from_env helper. DONE (this commit) — SYNTHESIS side only
      (fortuna-live): daemon::mind_from_env<T: MindTransport>(cognition,
      transport: Option<T>, clock) -> Arc<dyn Mind> — Some(transport) =>
      AnthropicMind {model from [cognition].model, max_tokens/prices/charter =
      code consts, CostBudget from [cognition] budgets}; None => StubMind. main
      builds the transport: ANTHROPIC_API_KEY present (validated) =>
      Some(ReqwestMindTransport::from_env(timeout)) else None, then mind_from_env
      + passes to compose_runner. The KEY reaches only the transport (env), never
      config/logs. Clock = RealClock (the real-time daemon's SimClock tracks wall
      time, so the budget day-reset aligns; a fully shared clock is a ledgered
      refinement). [cognition].model added (default "claude-fable-5", spec 5.9
      synthesis tier). Unit test (daemon.rs): mind_from_env(Some scripted
      transport).id()=="claude-fable-5" (AnthropicMind, id IS the model — proves
      config.model flows) vs (None).id()=="stub-mind" — NON-VACUOUS distinct
      branches; the scripted transport NEVER carries a real key (kickoff money
      pitfall). FOLLOW-UPS ledgered: (a) AnthropicMindConfig prices/max_tokens
      are code consts — promote to [cognition] (the comment says prices "are
      config"); (b) the VETO side is BLOCKED — AnthropicVetoMind does NOT exist
      (fortuna-cognition; Track A consumes-not-edits) — its owner must build it
      before the veto goes live; the veto stays StubVetoMind::allow_all. PROD-
      LIVE PREREQ (S5a gap): synthesis trades only once the operator config adds
      `synthesis_cents` envelope + [gates.per_strategy.synthesis].
  S6a. belief drain+persist WIRED INTO drive(). DONE (this commit): per segment,
      within the synthesis_refresh-Some path (only the synth arm drafts beliefs),
      drive() calls runner.drain_pending_beliefs() -> persist_beliefs(pool,
      drafts, now_iso, belief_id_base). belief_id_base seeds from the drive-start
      epoch (unique across runs) + increments per persist (unique within a run);
      a full ULID is the ledgered req-6 refinement (persist_beliefs already notes
      it). A persist FAILURE alerts (Ops) + counts but never crashes — beliefs
      are the calibration substrate, NOT the money path (I5 governs the audit
      log). The drained set is LOST on persist failure (re-buffering = a ledgered
      refinement). Test (daemon_smoke sqlx::test): [synthesis] + a believing mind
      + a confirmed sim edge + a book -> drive one segment -> a belief for evt-1
      lands in the ledger (before 0 -> after >=1); MUTATION-PROVEN non-vacuous
      (disabling the persist drops after to 0). Note: belief DRAFTING is
      calibration-independent (the cycle calls the mind regardless), so this
      persists even when calibration is None.
  S6b. RICH daily digest. SCOPE VALIDATED 2026-06-12 (no code) — the data is
      MOSTLY available; ONE confirmed gap. Map DigestInputs (fortuna-ops/
      digest.rs::compose_daily_digest, a PURE fn) <- runner:
      - date_utc/stage: trivial (now + "sim").
      - strategies[].{realized_pnl,fees,exposure}: AVAILABLE — the runner already
        attributes per strategy at metrics_export (a market_strategy map ->
        pnl_by/fees_by summed over positions; reserved_total(strategy) =
        exposure).
      - strategies[].fills: THE GAP — Position carries NO fills count (only
        realized_pnl + fees_paid, confirmed). Source options: (a) track a
        fills_by_strategy counter in the runner, incremented at fill-apply time
        via the same market->strategy attribution as pnl_by — RECOMMENDED (cheap,
        consistent); (b) count from the IntentJournal.
      - halts_active/discrepancies_open/settlements_overdue/capital_in_limbo:
        AVAILABLE — boards_json's ops block already reads them (gates.halts().
        global_halted(), counters.discrepancies, overdue_alerted.len(),
        settlements.capital_in_limbo()); expose via a runner accessor (NOT JSON
        parsing).
      - veto_decisions/veto_suppressed: counters().
      BUILD PLAN (next iteration): (1) fortuna-runner (owned): add
      fills_by_strategy tracking (option a) + a digest_snapshot() accessor
      returning the raw primitives (per-strategy rows + the ops fields). (2)
      fortuna-live daemon.rs: rich_daily_digest(runner, now) composes DigestInputs
      + compose_daily_digest; replace terse_daily_digest in drive's daily block.
      (3) test (daemon_smoke): seed a position + a veto + a discrepancy -> the
      rich digest text surfaces per-strategy PnL + the honesty numbers + veto
      (non-vacuous). DEFERRED to later sub-slices: the daily reconciliation re-run
      + weekly/monthly cognition reviews (separate review machinery, post-tick).
      DONE (this commit), built to the plan: (1) fortuna-runner: DigestSnapshot +
      DigestStrategyRow structs + a digest_snapshot() accessor (per-strategy
      pnl/fees over positions + FILLED-order count over intents, both via the
      market_strategy attribution metrics_export uses; reserved_total = exposure;
      halts/discrepancies/overdue/limbo from the boards_json internals; veto from
      counters). (2) fortuna-live: rich_daily_digest maps DigestSnapshot ->
      fortuna_ops DigestInputs -> compose_daily_digest; drive's daily block now
      emits it (terse_daily_digest kept — still unit-tested). Tests: fortuna-
      runner digest_snapshot test (a filled synth trade attributes a synth_sim
      row with fills>=1 + fees>0; non-vacuous); the daemon_smoke gate-#3c digest
      assertion updated to the rich "FORTUNA daily digest%" prefix (assertion
      unchanged — exactly one digest; NOT a weakening). FILLS NOTE: per-strategy
      "fills" = FILLED-order count (Position has no fill-event count), distinct
      from the aggregate fills_applied (fill events) — documented.
The populated-path test rule (the verifier's vacuous-test lesson) applies to
EVERY sub-slice: assert REAL non-empty edge sets / non-zero proposals, never a
shape that passes under a fabricated/empty fixture.

## TRACK A — DST determinism anchors (verifier orchestration §6) — DONE
The DST regression corpus has been empty since T0.4, so the corpus-replay arm
was a no-op. Committed 3 high-activity PASSING anchor seeds to
crates/fortuna-core/dst-corpus/ (31337: crash+boot quiesce + outage, 23
orders/13 fills; 8675309: fault-dense, 13 faults; 777: fill-dense, 13/13) —
they pin replay determinism across refactors (a refactor that breaks
determinism now reds the corpus). Validated: `cargo test -p fortuna-core
--test dst -- --seeds 0` => "3 corpus + 0 random seeds, zero invariant
violations". Chosen as a disk-light item while the volume sat at ~20Gi/98%.
CORRECTION (measured next iteration): a WARM fortuna-live battery costs only
~1-2GB (rlibs are cached from earlier sessions; only the changed crate
recompiles) — NOT the ~15GB I had feared by over-anchoring on the S1
fortuna-ledger drop (which included `sqlx prepare` + cold-ish deps). S3 was
NEVER disk-blocked; S3a shipped immediately after on a warm fortuna-live
battery (16Gi free after). The ~15GB hazard is a COLD full build (the shared
gate target), not a warm scoped one.
Slack send-failure count is now SURFACED (drive sums total_send_failures
and audits a final Ops alert if >0) — the earlier "_send_failures
discarded" is fixed.

REMEDIATION-2 FOLLOW-UP (2026-06-11, honesty correction): commit f78ba4e
was framed as "hoist halt-dedup (Major) + 5 minors", but remediation2-gate
finding #4 (raw SystemTime::now() in fortuna-live main.rs at the binary
edge — CLAUDE.md calls a wall read outside a Clock impl a defect even at
the edge) was NOT actually addressed in that commit; the pre-existing F6
ASSUMPTIONS entry covers only the capture tools (recorder + fixture
example), not this daemon binary. Self-caught on the next iteration's
priority-(a) re-verification. CLOSED HERE: main.rs now reads wall time
through `fortuna_core::clock::RealClock` (the one documented legal
wall-time source, and a Clock impl — so no longer "outside the Clock
impls") for both the start timestamp and the dead-man heartbeat tick; zero
raw SystemTime::now() remain in fortuna-live/src. The dead-man deliberately
reads the WALL via RealClock (not the runner's SimClock) so its heartbeat
stays real-time during a sim soak. No ASSUMPTIONS exception is needed — the
clean fix (route through the Clock impl) was taken over the ledger-it
alternative. [RECONCILED 2026-06-11, audit-tail-fix gate minor 4: the
ASSUMPTIONS dead-man entry is now a DESIGN NOTE (dead-man reads wall via
RealClock, not SimClock — NOT an exception), no longer the stale
"reads SystemTime::now()" claim; the two ledgers agree.]

T41-DAEMON-GATE FIXES (2026-06-11, the daemon gate's BLOCK):
- F1 clippy-red (drive too_many_args at 77588c5/817d2e7): FIXED at
  871c339 (#[allow] — the args are distinct composition inputs);
  CLIPPY_EXIT=0 verified at head with a real exit code. PROCESS FIX: the
  battery is captured with `; echo EXIT=$?`, never `| tail -1` (the pipe
  masked clippy's exit and let two red commits through — the gate caught
  exactly that).
- F2 halt re-audit flood (Major): FIXED — run_loop dedups on halt
  identity (apply+audit once per reason; re-audit on change; clears on
  out-of-band re-arm). Test: a standing halt over 20 polls = 1 audit row.
- F3 SIGTERM-with-working-orders (Major): FIXED — daemon_smoke gains
  signal_with_working_orders_cancels_them_and_audits (thin books leave
  resting orders; the stop channel = main's SIGTERM channel; asserts
  cancelled>=1 + journaled cancels + one final audit row). REAL OS-signal
  delivery is not asserted in-repo (the handler routes the same channel;
  ledgered as the untestable seam).
- F4 poll-failure alert: IMPLEMENTED — drive routes an Ops alert when a
  segment had halt-poll failures (halt rail blind).
- F5/F6 comments + reservation assertion: shutdown.rs comment corrected
  (handler exists); audit_death_staging now asserts reserved_total==0 on
  every abort fail-point. DST-ARM DECISION: the audit-death-mid-staging
  failure mode is covered by the audit_death_staging SWEEP (~40
  deterministic sub-cases/run) rather than a randomized settlement_dst
  arm — the vector is the staging boundary, not a venue-fault timeline,
  so an exhaustive sweep pins it better than seeded sampling.
- F7 belief tidy: error-string dup detection REPLACED with a checked
  EXISTS query; belief ids are caller-monotonic sortable TEXT (NOT full
  ULIDs — daemon does not thread the runner IdGen; uniqueness+sort is all
  the PK needs); the append-only beliefs table IS the persistence record
  (no separate audit row by design); not-called-from-main stays ledgered
  (synthesis-in-main is edge-source-design-blocked).

REMAINING (composition-wiring; T4.1 in progress — status 2026-06-11):
- fortuna_ops::alerts::degrade_alerts scrape-delta consumer: FULLY
  WIRED. The daemon drive loop scrapes per segment and routes via
  daemon::route_alerts (Slack send + audit row always, spec 8; send
  failures counted, never silent); MAIN now builds the SlackRouter from
  the validated env via daemon::build_slack_router over the reqwest
  transport (token present => Some; a config-named channel id absent in
  env is a LOUD boot error, never silent-None). 6 pinned tests (mock
  transport: route+audit, no-router, failed-send-counted, build none/
  some/loud-missing-id). No remaining sliver on this residue line.
- CalibrationParamsRepo.latest call site: compose::calibration_for_scope
  now fetches latest + resolved_stats and builds CalibrationContext +
  calibration_quality (fail-closed None / zero; corrupt params row
  errors LOUDLY — all test-pinned). STILL OPEN: the daemon main must
  feed these into SynthesisStrategy::new + set_calibration_quality —
  lands with the composition main / req-10 smoke.

## T4.3 ROTA — slice progress (box unticked; in progress 2026-06-11)

- Slice 1 (c3550f9): read-only rota router + Option-capability RotaState +
  cursor-polled audit tail + gold/black shell (R1/R2/R3/R11/R12).
- Slice 2 (this commit): daemon-side `fortuna_live::views::views_from`
  populates DashboardSnapshot.views so the slice-1 handlers serve REAL data
  instead of "unavailable". POPULATED: health (halt via the new pure
  `SimRunner::active_halt()`; p90/p95/p99 — no p50 per R6; dead_man null per
  gate note 6; venue errors) + settlement (limbo/overdue/voids/reversals)
  fully; gates.total_rejections + streams.venue_api_errors_total scalars.
  §5 per-view generated_at is passed in by the between-segments closure
  (which holds the clock) so views_from stays pure/clock-free (lib invariant
  preserved). Runner change is ONE pure read accessor (active_halt); zero
  money-path change. The daemon→ops contract is covered by views.rs unit
  tests (producer shape) + slice-1 populated_view_is_served_verbatim
  (consumer; read_view is a literal views.get(name) passthrough) +
  daemon_smoke (wiring) — no new dev-dep.
- Slice 3 (this commit): serve_dashboard now MOUNTS rota_router (§6) — before
  this, rota_router was wired into nothing and the running daemon served only
  the legacy Instrument boards, so slices 1+2 were unreachable live. Signature
  Shared -> RotaState; legacy routes derive state from rota.snapshot, ROTA
  merges in at /rota + /api/rota/v1/* (no route overlap). Daemon main builds
  RotaState::standalone (pool/perishable_dir None this slice). 3 callers
  updated; new red-first test proves /rota serves the populated health view and
  that read-only survives the merge (POST -> 405). An operator running the
  daemon can now open /rota.
- Slice 4 (this commit): the streams recorder filesystem-scan. scan_recorder
  stats data/perishable/<today>/<stream>.jsonl (mtime->age, len->size_bytes,
  healthy=age<120s); the /streams handler merges it when perishable_dir is
  present. PERF CALL: metadata-only, NEVER a content read — bracket_quotes.jsonl
  is ~1.3GB and a line-count on the 15s poll would be a self-inflicted DoS, so
  §5's rows_today/key_count are DEFERRED (content-read optimisation; size_bytes
  is the cheap proxy). Clock-free: now+today come from snapshot.generated_at.
  Daemon main wires perishable_dir="data/perishable" (matches recorder default)
  so the scan is live. 3 tests (scan fresh/stale/missing, handler merge/omit).
  NOTE: perishable_dir is hardcoded to "data/perishable" in the daemon (matches
  fortuna-recorder's default out_dir and the daemon's repo-root cwd assumption);
  making it a DaemonToml field is a future nicety, not a blocker. Midnight-
  rollover edge: the scan picks today's dir from generated_at's date; for ~the
  first seconds after UTC midnight the new day's dir may be briefly empty while
  the recorder finishes the prior file — acceptable (the panel shows the new
  day), documented here.

ROTA-SLICES GATE REMEDIATION (rota-slices-gate-2026-06-11.md, BLOCK narrow;
6 findings — tracked here as they close):
- F1 [MAJOR] audit-tail cursorless returned the OLDEST page: CLOSED (this
  commit). audit_tail_page(pool, after, limit) extracted + tested: cursorless
  => LATEST page (newest `limit` rows, ORDER BY audit_id DESC then re-sorted
  ASC); present cursor => forward (`> cursor ASC`). Doc aligned. The owed
  cursor-pagination test now exists and INCLUDES the absent-cursor case
  (#[sqlx::test]) + empty-table. The shell already polls cursor-less, so it
  now shows the live tail with no shell change.
- F3 [Minor] runtime sqlx audit query: LEDGERED (ASSUMPTIONS) as a deliberate
  choice for the single read-only dashboard query (schema-pinned by migration,
  now #[sqlx::test]-covered; avoids sqlx-offline build coupling). Same edit as
  F1 -> closed together.
- F2 [Minor] /favicon.ico 404 (the only live-browser console error, an R12
  criterion): CLOSED (this commit). rota_router serves /favicon.ico => 204 No
  Content (stub; the real Section 9 cornucopia/wheel mark lands in the Phase-3
  asset slice). Tested standalone (favicon_is_a_204_not_a_404, + POST 405) AND
  through the live merged serve_dashboard tree (the dashboard mount test).

AUDIT-TAIL-FIX GATE (audit-tail-fix-gate-2026-06-11.md, ACCEPT-WITH-GAPS — the
first non-BLOCK after four BLOCKs; F1-cursorless + slice-4-scan + F3-ledger all
VERIFIED). New/carried Minors:
- #1 [NEW] scan_recorder faked healthy:true on a malformed generated_at
  (parse_iso8601 unwrap_or(0) -> age clamped to 0): CLOSED (this commit). now_ms
  is now Option; age is computed only when BOTH the file mtime AND a parseable
  "now" are known, else None => unhealthy + null age (degraded-never-faked).
  Test: scan_recorder_rejects_a_malformed_generated_at_never_faking_healthy
  (valid date prefix + unparseable instant — the gate's exact vector).
- #2 favicon: CLOSED (276e67a). #1 scan_recorder malformed-clock: CLOSED (7e35f51).
- #3 DailyScheduler/digest (3 sub-parts): CLOSED (this commit).
  (a) fire-on-boot: LEDGERED as INTENDED — the digest fires on the first due()
  (boot) and at each UTC-day rollover. A boot digest confirms the digest path is
  live on startup and gives the boot day at least a partial line (no-fire-on-boot
  would skip the boot day entirely). Honest now that the label says so (see b);
  DailyScheduler.due() unchanged (its once-per-day test still holds).
  (b) labeling: FIXED — terse_daily_digest now reads "FORTUNA digest <date>
  (sim, cumulative since boot)" because RunCounters accrue for the runner
  LIFETIME, not per UTC day; labeling them "the day's" overstated. True per-day
  deltas (snapshot-at-boundary) are part of the RICH DigestInputs surface
  (synthesis-in-main-blocked). Test: terse_daily_digest_labels_its_counters_
  honestly_as_since_boot.
  (c) drive()-level assertion: ADDED — daemon_smoke now asserts drive() emits AND
  audits exactly one digest (kind 'alert', message LIKE 'FORTUNA digest%';
  route_alerts audits even with no Slack router, spec 8).
- #4 [Minor] ASSUMPTIONS/GAPS dead-man contradiction: CLOSED (this commit,
  docs-only). The ASSUMPTIONS entry was stale ("the task reads
  SystemTime::now()") and mis-framed as a "justified exception"; corrected to a
  DESIGN NOTE — the dead-man reads wall via RealClock (a Clock impl, the legal
  source), NOT the SimClock, so NO exception is needed, matching GAPS:142. The
  GAPS line gained a reconcile clause. Code verified: main.rs:144 reads
  RealClock.now(); zero raw SystemTime::now() in fortuna-live/src.
  => the audit-tail-fix gate's Minor list is now fully remediated (1-4 closed).
- INFORMATIONAL (not a ROTA code fix): raw-JSON panels (Phase-3 presentation);
  LIVE recorder risk_parameters stale-on-boot (recorder/B0 capture-loop
  investigation — do NOT touch the running recorder).
- DEFERRED (capability-gated; keys ABSENT not faked-zero so a panel never
  reads falsely "all clear"): money view (needs the new boards "account"
  field, R6); cognition view (R7 — BeliefsRepo::recent + calibration-scope
  enumeration, two new ledger queries); recent_rejections /
  recent_watchdog_events (R5 dedicated audit pool + a query); streams.recorder
  + per-venue book_age_ms (recorder filesystem scan + new boards field);
  health.last_tick_age_ms (no last-tick wall stamp tracked). Also remaining:
  Phase-3 shell/assets, R12 browser pass.
  [DONE since: cursor-pagination test (audit_tail_page tests); R5 audit pool;
  gates.rejections_by_check — now POPULATED via the new SimRunner::
  rejections_by_check() accessor (sorted {check,count}, sums to
  total_rejections; §5 per-check "number" omitted — runner keys by name only).]

## T4.3 ROTA — slice 5 (R5 pool) + the money-view design finding (2026-06-11)

- R5 DEDICATED AUDIT POOL: BUILT (this commit). `fortuna_ledger::
  connect_readonly_pool` makes an ISOLATED 2-connection read pool (short
  acquire_timeout + a 3s statement_timeout via after_connect; NO migrations) —
  NEVER the daemon's writer pool, so dashboard load cannot queue against the
  audit writer (audit-append failure is a global halt). Daemon main wires it
  into RotaState.pool (was None); a connect failure degrades the audit panel to
  empty, never crashes the daemon. The /audit handler's available:true path is
  now HTTP-tested end-to-end (audit_handler_serves_the_live_tail_when_a_pool_is_
  present) — F1 cursorless-latest at the handler layer. The audit TAIL is now
  LIVE on the running daemon; this pool also unblocks the cognition view's two
  ledger queries (next).
- R5-POOL GATE finding #1 (the only Minor): CLOSED (this commit). The R5
  saturation/ISOLATION property is now PINNED by a committed handler-level test
  (exhausted_rota_pool_degrades_to_200_while_the_writer_is_unimpeded,
  #[sqlx::test] with PoolOptions/ConnectOptions injection): a bounded 2-conn
  reader saturated (both conns held) => GET /audit degrades to HTTP 200,
  available:false, bounded by acquire_timeout (never hung/500), WHILE a
  concurrent INSERT on a SEPARATE writer pool proceeds <1s and commits. A future
  refactor that merged the pools back would now fail this test. Also addressed
  the paired informational note: the audit_tail Err arm no longer returns
  available:true with raw sqlx text — it degrades to available:false + a neutral
  detail (no error-text leak; the cause is logged server-side).
- MONEY VIEW — SIM-ONLY SUBSET BUILT (the r5-pool gate's verifier-endorsed
  unblock: "ship the view with the SIM-ONLY subset per R6"). boards_json gained
  an "account" block {cash_cents, reserved_cents} from SimVenue::inspect_totals;
  views_from shapes the money view: basis="sim-only", settled_cents=cash,
  committed_cents=reserved (both REAL), positions reshaped to §5 yes_qty/no_qty.
  floating_cents + total_cents are NULL — the §5 identity total=settled+floating
  needs the MARK LOOP, which is not exposed (verifier-confirmed: "the mark loop
  is the missing source"); they are honestly null, never faked-zero, and the
  "sim-only" basis label means an operator never reads this as the complete
  picture. STILL OPEN (full §5 model, an operator/design call): floating from a
  mark-loop accessor + per-strategy PnL attribution for strategies[] + a live
  venue's settled/floating semantics (the account block is sim-only until then).

## T4.3 ROTA — r5test-slice6 + money-view gates (2026-06-12, both ACCEPT-WITH-GAPS)

Two consecutive non-BLOCK verdicts (3rd + 4th). VERIFIED: the R5 isolation test
has teeth (verifier scratch-merged the pools -> RED); slice 6 is read-path-only;
slice 7 money is honest (literal nulls, sim-only labeled). The recurring signal
is the VACUOUS-TEST class (a shape/invariant assertion that passes under a
fabricated/zeroed panel). Fix list:
- #1 [Minor] gates "sum == total" test was VACUOUS (the arb run rejects nothing,
  so total=0 and an empty accessor passes): CLOSED (this commit). New test
  gates_rejections_by_check_is_non_vacuous_on_a_rejecting_run forces real
  rejections (unreachable net-edge floor min_net_edge_bps=100000) and asserts a
  NON-EMPTY by_check summing to a NON-ZERO total — a stubbed/empty accessor now
  FAILS. Teeth confirmed.
- COGNITION COUNTER slice: REVERTED before commit (not shipped). Its counters
  (mind_spend/failures/breaches/shadow/beliefs_drafted) are STRUCTURALLY ZERO
  under mech_structural (no cognition strategy), so any counter test is
  vacuous-by-nature (the verifier's escalating defect class) — a non-vacuous
  test needs cognition-active (non-zero) data, which only synthesis-in-main
  produces (edge-source design-blocked). Cognition is DEFERRED until synthesis,
  with the recent_beliefs/calibration_scopes ledger queries owned there.
- #4 [Minor, vacuous-test 2nd occ] money test vacuous on the populated path
  (a zeroed panel passed): CLOSED (this commit). money_view_is_the_sim_only_
  account_subset now pins the REAL 11/3 arb seed — settled_cents == 995_639
  (= 1_000_000 starting cash − 4_361 spent: 50×(25+28+30) notional + 66+71+74
  taker fees), three legs each yes_qty == 50 with per-leg fees 66/71/74 and
  realized_pnl == 0; committed == 0 because every leg FILLED (nothing rests). A
  NEW test money_view_committed_is_non_zero_when_capital_is_reserved injects
  ack_delay_pm=1000 so the legs reserve but never fill: committed == 4_361
  (> 0), settled == 1_000_000, zero positions. MUTATION-PROVEN: zeroing the
  source money block (settled/committed → 0, positions → []) turns BOTH tests
  RED. Teeth confirmed.
- STILL OPEN — TRACK A follow-ups (these are fortuna-live files — views.rs +
  main.rs — owned by Track A per orchestration.md, NOT track B; the "T4.3 ROTA
  slice" label names the FEATURE, but the view-shaping + boot path live in
  Track A's crate): #2 add a daemon boot-path assertion that the reader
  (connect_readonly_pool, main.rs:93) and writer (connect) pools are DISTINCT
  objects (the R5 test self-constructs its pools — a wiring merge would fail no
  test); #3 the gates rationale "number would be a guess" is FALSE —
  GateCheck::index() (fortuna-gates/src/pipeline.rs) gives the exact spec number
  (EdgeFloor=6); include the number per §5 OR correct the rationale. Operator
  also slotted BUILD_PLAN T4.5 (ROTA v1.1 deferred panels), after T4.2; its
  TEST RULE bakes in the populated-path-seed lesson.
  - STATUS (2026-06-12, this commit): **#3 CLOSED** — views_from now emits the
    real 1-based spec number per rejection entry (reverse-mapped via
    GateCheck::ALL/index(), no guess); the false rationale is removed; RED-first
    test gates_rejection_view_carries_the_spec_gate_number. Full picture in the
    "EXHAUSTED was PREMATURE" correction at the top of this file. **#2 STILL
    OPEN**, next Track-A iteration (needs a testable pool-wiring seam in the
    fortuna-live binary).

## POST-STOP CONTINUATION (operator-directed, 2026-06-12 ~10:40Z): orphaned minors F-1 + F-2 taken back

The Ralph loop ended clean below; the operator then said "continue" and
the bus (10:30Z update) had released track-B's two orphaned Minors to the
pool. Both originate from this track's own commits, so this session took
them back:

- **F-1 (A8 audit-age line): CLOSED** — see the updated entry in the
  T4.4 slice-1 section. Ownership step-out DECLARED: ledger/src/audit.rs
  gained ONE method (`latest_at`) + ONE struct (`LatestAudit`) + one
  lib.rs export word — the exact "one-line AuditWriter addition" the
  original deferral named, sanctioned by the pool release + operator
  continue. Nothing else in audit.rs touched.
- **F-2 (A2 spawn-cwd pinning): CLOSED** — lifecycle paths now anchor to
  the REPO ROOT derived from the config path (`config/`'s parent; a
  config outside a config/ dir anchors to its own directory): the
  recorder out-dir, the runtime dir default (env override still wins),
  and the children's spawn cwd (`Command::current_dir(root)`) are all
  root-anchored, so `fortuna start` from a wrong cwd can no longer
  re-anchor data/ paths or fork the B0 dataset. Root derivation and
  out-dir anchoring unit-tested; all four lifecycle commands resolve the
  SAME root so status/logs/stop look where start wrote.
- The third bus item (one manual §13 runbook execution) remains the
  OPERATOR's — it requires stopping the manual recorder.

## RALPH STOP 2026-06-12T08:20:05Z (track B — queue exhausted, loop ends clean)

Track B's assigned queue (docs/design/implementer-loop-track-b.md priority
(b), per docs/design/orchestration.md) is COMPLETE:

- T4.4 operator CLI — BOX TICKED. Three gate-pending slices on track-b:
  slice 1 (config check / logs / status process-health, A9 exit-0 pinned),
  slice 2 (start: A2 pgrep refusal, A3 O_EXCL claim, A4 detach), slice 3
  (stop: A1 log-confirmed shutdown w/ append-offset semantics, A7 timeout
  posture, zombie-aware liveness). 38 tests in the crate; §13 manual smoke
  runbook recorded in the design doc.
- T4.3 track-B items — ALL FOUR DONE: the R7 ledger queries
  (BeliefsRepo::recent + CalibrationParamsRepo::scopes, populated-path
  tests, sqlx prepare), the cognition view (counters honest-absent until
  synthesis; ledger arrays live over the R5 pool; 4KB evidence
  truncation), the instrument presentation layer (per-panel renderers,
  UTC labels, click-to-expand evidence, raw expanders, §0.4 cadences),
  and assets/rota/logo.svg (§9 geometry, favicon + asset routes).

WHY STOP (loop rule 6): the bus names no track-B findings (the three open
Minors live in fortuna-live/fortuna-runner — track A's files); the
remaining T4.3 surface is explicitly not track B's (full §5 money model =
operator/design call; R12 browser pass = verifier; audit-recents were not
in the queue enumeration); T4.5/T4.2/T5.B belong to tracks A/C. Inventing
work violates "idle-and-stopped beats bloat".

STATE FOR THE VERIFIER: five track-B commits awaiting gate on branch
track-b (T4.4 slices 1-3, T4.3 slices 8-9), every one committed only
after a green full battery (fmt / clippy -D warnings / cargo test
--workspace / DST corpus) run under env -u DATABASE_URL (the operator-URL
canary is ledgered above). Cross-track notes for the gate are in the
T4.3/T4.4 sections: the venues-example fmt violation at HEAD (track A's),
the 39 stale .sqlx entries (owners'), the lib.rs one-line export and the
two favicon-test evolutions (both declared). DISK: the ENOSPC treadmill
entry above remains an operator action.

This entry's commit is DOCS-ONLY on the batteried HEAD (zero code delta
since the slice-9 battery: workspace 773/0, DST exit 0).

## T4.3 cognition slice (track B; 2026-06-12) — ownership notes for the gate

- **fortuna-ledger/src/lib.rs gained ONE additive pub-use line** (the two
  new row types). Track-B ledger ownership reads "the two R7 query
  additions in repos.rs"; the queries are unreachable from fortuna-ops
  without the export, so the line is read as PART of the query addition.
  Zero existing exports moved or changed; flagged here for the gate.
- **R7 query tests live in a NEW file crates/fortuna-ledger/tests/
  rota_queries.rs** (R7 mandates "both with tests"): purely additive —
  no existing ledger test file was touched.
- **.sqlx cache: only track B's two query JSONs are committed.** A full
  `cargo sqlx prepare --workspace` regenerated 41 missing entries — 39 of
  them are PRE-EXISTING staleness from other tracks' queries (cache not
  refreshed for several commits); committing those would put track B's
  name on surfaces it doesn't own. They remain untracked; owners/verifier
  should run prepare at the next gate.
- **Design §3 deviation — RotaState gains NO budget fields:** fortuna-live
  main.rs (track A) constructs RotaState as a STRUCT LITERAL; adding
  fields breaks a file track B may not edit. Budgets (daily/per-cycle)
  ride the daemon-shaped cognition view when track A's synthesis-in-main
  populates it — same channel as the counters. If the literal ever
  becomes a builder, the §3 shape can be revisited.
- **Cognition counters render as explicit absence, not zeros:** under
  mech_structural the counters are structurally zero (the r5test-slice6
  gate's vacuous-data class); the panel shows counters_status:
  "unavailable" until synthesis-in-main composes a cognition strategy.
  The LEDGER arrays are live and populated-path-tested with real seeded
  values (p=0.67/0.71, evidence reasoning text, provenance cost, max
  version 2) — a fabricated panel cannot pass them.
- **Slice 9 test evolution (declared, not a weakening):** the favicon
  test `favicon_is_a_204_not_a_404` asserted the INTERIM 204 stub and its
  own comment said the §9 mark "replaces this in the Phase-3 asset
  slice". That slice is here: the test became
  `favicon_serves_the_wheel_mark_never_a_404` with STRONGER asserts
  (200 + image/svg+xml + wheel markup + the unchanged POST-405). The F2
  intent (no 404 / no console error) is preserved and tightened. The
  serve_dashboard merge test pinned the same interim 204 at its own
  assertion site — evolved identically (200 + content-type), found red
  by the battery before commit.

## T4.4 CLI — slice 1 (track B; box unticked; 2026-06-12)

- **SIGTERM mechanism (design checklist item 8, decided at fit-validation):**
  `nix` is not in the workspace tree, so the `stop` slice will shell out to
  `kill -15 <pid>` (never `Child::kill` — that is SIGKILL). Recorded per the
  design's else-branch; the code lands with the stop slice.
- **Design §6 deviation — `toml` added to fortuna-cli:** the A6 status line
  ("config on disk: venue=…") needs `[daemon].venue`, which FortunaConfig
  deliberately drops (the daemon owns that section — live/src/boot.rs).
  Implemented as a raw `toml::Value` read; `toml` was already a workspace
  dep (ops uses it), zero new external code. Flagged for the gate.
- **A8 audit-age status line — CLOSED (orphaned minor F-1, post-pool-
  release):** originally DEFERRED here because AuditWriter (ledger/src/
  audit.rs) was outside track-B ownership and a kind-filtered
  approximation through `recent()` would be a FALSE crash-tell (a healthy
  daemon writing only cognition/veto rows would read stale). The bus
  released track-B ownership to the pool at session stop and the operator
  continued the session — the unblock this entry named. Closed with:
  `AuditWriter::latest_at()` (kind-agnostic, ULID-ordered newest row, at
  + kind; tests in ledger/tests/audit_latest.rs incl. the kind-agnostic
  assertion), one additive lib.rs export (LatestAudit), sqlx prepare (one
  new cache JSON), and the status line ("most recent audit row: 42s ago
  (kind …)" / "none yet"; formatting unit-tested incl. unparseable-at
  degradation).
- **"Degradable" status interpretation (test-pinned):** A9 pins only the
  no-DATABASE_URL case (exit 0). This slice extends the same posture to
  DATABASE_URL-set-but-unreachable: status prints `db: unavailable — …` and
  still exits 0, bounded at 5s (sqlx's own pool timeout is 30s — a status
  command must not hang the operator's view during a Pg outage). Pinned by
  `status_db_unreachable_still_exits_zero`.
- **CROSS-TRACK finding (not track B's to fix; for the bus):**
  `crates/fortuna-venues/examples/record_kinetics_fixtures.rs:801` is
  unformatted AT HEAD — `cargo fmt --check` is red workspace-wide before any
  track-B change (verified on a clean tree 2026-06-12; track B's own diff is
  fmt-clean and the sweep `cargo fmt` produced was deliberately REVERTED to
  stay inside ownership). Owner: track A (fortuna-venues). One `cargo fmt`
  there clears it.
- **Battery environment note (track B session):** the interactive shell
  exports the OPERATOR's DATABASE_URL, which outranks the `.cargo/config.toml`
  dev default (`force = false`) and reproduces the documented 42501
  `i5_audit_append_only` canary. Track-B batteries therefore run under
  `env -u DATABASE_URL` so sqlx tests route to the dev server. No operator-DB
  writes occurred (the failure mode is a DENIED `CREATE DATABASE`).

Slice 2 (`start`) additions:

- **`[recorder]` config table is read but not yet in the committed example:**
  `start` builds the recorder invocation from an optional `[recorder]` table
  (interval_secs / bracket_series / out_dir) with defaults pinned to the A2
  live invocation verbatim (30s, KXBTC15M,KXBTC,KXBTCD, data/perishable made
  ABSOLUTE against cwd). Adding the section to config/fortuna.example.toml is
  OUTSIDE track-B ownership (config/ is unassigned) — needs track A or an
  operator edit; until then the defaults govern and are test-visible in
  recorder_invocation().
- **A2 refusal scope (conservative interpretation):** an unmanaged
  fortuna-recorder process refuses the WHOLE start — even a daemon-only
  spawn — until the operator migrates. Rationale: a managed spawn alongside
  an unmanaged recorder normalizes the exact double-appender state A2
  exists to prevent; spec-silent => conservative.
- **The success spawn path is NOT integration-tested on this box, by
  design and by necessity:** design §9 makes start->status->stop a manual
  runbook check (forking is timing-flaky in CI), and this machine
  intentionally hosts the operator's UNMANAGED recorder, so a clean
  `fortuna start` here correctly REFUSES (the A2 path — which IS
  integration-tested, with a planted decoy so it stays deterministic on
  clean machines too). Claim atomicity (8-thread race), append-mode
  redirect, claim-release-on-spawn-failure, and pidfile-write+marker-clear
  are unit-tested at the primitive level.
- **The `lifecycle` audit row path is not Pg-integration-tested:** the CLI
  reads DATABASE_URL from env; the sqlx::test harness does not hand a URL
  to a spawned binary. The append mirrors the existing tested halt/rearm
  pattern (checklist item 10 signature) and is best-effort by A10. Verifier
  scratch-test or the manual runbook covers it; flagged honestly here.

Slice 3 (`stop` — T4.4 commands complete) additions:

- **Zombies read as not-running** (found red-first: the stop tests' stubs
  are children of the test process, so their TERM-exits left ps-visible
  zombies and the liveness poll never saw an exit): `comm_of` now reads
  `ps -o stat=` alongside comm and treats stat Z* as not-running. This is
  production-correct, not a test accommodation — a zombie pidfile target
  is an EXITED process whose parent has not reaped it; it is not
  signalable work, and `stop` must count its exit as an exit.
- **fortuna-recorder has NO SIGTERM handler** (crate outside track-B
  ownership): default TERM termination can land mid-append and tear a
  JSONL line — the same defect class A2 guards against, with a microscopic
  window (one write per 30s interval). `stop` cannot fix this from the
  CLI side; a trap/flush handler belongs to the recorder's owner. Flagged
  for track A / operator queue.
- **A1 evidence choice:** the daemon's stderr line `fortuna-live: clean
  shutdown` (main.rs, redirected into the managed log by `start`) is the
  log evidence `stop` requires; the Pg final audit row remains the I5
  record (A10 framing). `stop` accepts the marker only at/after the log
  byte-offset captured BEFORE the signal — append-mode logs carry previous
  runs' markers, and a stale marker must never vouch for a fresh crash
  (offset semantics test-pinned, including the pre-seeded-marker case).
- **A daemon that was never started via `start` has no managed log**, so
  A1 cannot be confirmed: stop still SIGTERMs and waits, then exits 1
  with an honest "no shutdown line" warning. The managed lifecycle is the
  contract; outside it, stop degrades loudly rather than lying.
- **DISK INCIDENT 2026-06-12 (environmental; operator notified live):**
  the Data volume hit 100% (161MiB free) during the slice-3 battery —
  link steps failed with ENOSPC across crates. Track B removed its OWN
  worktree target/ (7.2G, regenerable build cache; nothing else touched —
  not other tracks' targets, not data/, not Pg) restoring ~13Gi free, and
  re-ran the battery from a clean build. The volume remains ~99% used
  overall. Survey: MAIN checkout target/ = 35G (shared by the verifier's
  gate batteries + rust-analyzer — NOT track-B's to clean), track-C
  target/ = 9.7G (track C's), track-B = 7.2G (cleaned). A
  `cargo clean` in the main checkout between gate firings is the big
  FORTUNA-side lever — verifier/operator call. Risk while pressure
  lasts: ENOSPC could hit the B0 recorder's JSONL appends and any
  track's battery mid-link.
  RECURRED same night (next iteration): 0 bytes free again — briefly
  blocked even session tooling temp-files; track-B target/ deleted a
  second time (~14Gi back). The pattern is a TREADMILL: each track
  battery rebuild is ~8GB across three tracks; headroom lasts roughly
  one battery. Operator re-notified live. Track B continues but every
  battery now starts from a cold build (slower iterations) and may
  ENOSPC mid-link if another track builds concurrently.

## SECURITY INCIDENT 2026-06-11 (gate finding F1, Critical) — keys were committed

WHAT HAPPENED: both Kalshi PEM private keys (`.keys/fortuna-demo-v1.txt`
and `.keys/fortuna-key.txt` — the latter mapped by .env to BOTH
KALSHI_PRIVATE_KEY_PATH and FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH)
were tracked in git from the B0 commit until the same-day remediation.
ROOT CAUSE: an agent `echo "data/" >> .gitignore` onto a file whose last
line `.keys/**` had no trailing newline corrupted the pattern to
`.keys/**data/`, un-ignoring `.keys/`; the next `git add -A` swept the
keys in. EXPOSURE BOUND: this repository has NEVER been pushed — the key
material never left this machine; it existed only in local git objects.
REMEDIATION (engineering, done same day — CORRECTED per the remediation
re-gate's F1d finding, which caught this entry describing the PLANNED
purge as a completed one): .gitignore repaired (`.keys/` restored,
trailing newline, `.playwright-mcp/` added), keys + runtime data
untracked at HEAD, and the BRANCH history rewritten via filter-branch
(old->new hash mapping in docs/reviews/history-rewrite-2026-06-11.md —
hashes cited in documents dated before 2026-06-11T08:30Z are
pre-rewrite). FINALIZATION HAS NOT RUN: the refs/original backup ref and
reflogs still REACH THE OLD OBJECTS (the key blobs remain recoverable
inside .git — `git show <old-hash>:.keys/...` works by design until
finalization); the permission classifier correctly refused the
irreversible step (reflog expire + gc) without explicit operator
authorization. Full-unreachability verification happens only AFTER the
operator-approved finalization. The earlier text here claimed "reflog
expire + gc ... VERIFIED" — that was the plan, written ahead of the
denial and not reconciled; corrected, not erased. Pre-batch
.playwright-mcp blobs (zero-secret browser logs) also remain in older
history; their purge is optional and folds into the same finalization
decision. PROCESS FIX: never append to .gitignore with `>>`; edit with
anchored tools and verify `git status --ignored` after.
F5 DISPOSITION (ledger-gate fix 2): the B0/B1-gate's F5 (runtime data +
playwright litter committed) closed as follows — data/ purged from branch
history in the F1 rewrite and gitignored; .playwright-mcp/ untracked at
HEAD and gitignored; PRE-batch playwright blobs remain main-reachable
(zero-secret browser logs) and their removal would need a second rewrite
— folded into the operator finalization decision as an optional extra.
OPERATOR ACTIONS REQUIRED (two distinct decisions):
1. ROTATE both Kalshi keys (treat as compromised per policy even though
   exposure was machine-local — the live key is also the I4 kill-switch
   credential): demo + prod key pages at (demo.)kalshi.co Account &
   security -> API Keys; place new PEMs at the paths .env names; the
   fixture set is unaffected (recorded with the demo key, which you may
   rotate independently).
2. FINALIZE THE PURGE (irreversible; classifier-gated to you): run
   `git for-each-ref --format='%(refname)' refs/original/ | xargs -n1 git
   update-ref -d && git reflog expire --expire=now --all && git gc
   --prune=now` from the repo root (or tell the agent "finalize the
   purge" to run it with your authorization). Until this runs, the old
   key blobs remain reachable inside .git via the backup ref. Do this
   BEFORE any first push of this repository, whatever else happens.

## Operator adjudication queue — RESOLVED (signed off 2026-06-10)

OPERATOR SIGN-OFF RECORDED: 2026-06-10, in-session, verbatim "I sign off",
given in direct response to this queue (the four waive batches below).
This converts every rule-based BLOCK from the protected-crate touches.
The audit record stays below for the trail.

- **Protected-crate waives (3 batches).** crates/fortuna-invariants/ was
  touched in Phases 1-3; each touch triggered the automatic BLOCK rule.
  All three were audited line-by-line across the five reviews: every
  deletion in history was a baseline `#[ignore]`+`todo!()` placeholder
  replaced by implemented tests; ZERO existing assertions modified,
  deleted, or loosened. Batches: (1) Phase 1 — three one-line
  `volume_contracts: None` fixture compile-fixes (i1/i3/i4); (2) Phase 2 —
  I6/I7 stub closures + 2 new T3-staged stubs; (3) Phase 3 (6276274) —
  closure of those two I7 stubs. Unblock: operator reviews
  `git log -p -- crates/fortuna-invariants/` (or the diffs quoted in the
  verdict files) and records the waive decision in this file under
  "Disputed invariant tests" or in the ops log; that converts the
  rule-based BLOCKs. Batch (4), added by the E-batch: one fixture line in
  i7_promotion_gates.rs (`kelly_fraction: 0.25,` in its runner_config —
  a compile fix for the new required RunnerConfig field), confirmed
  assertion-clean via full patch by the E-gate verdict.

## Path to production (ordered; after this list only operator actions remain)

1. DONE (1d1c033..1e3e5e7): E1-E4 + E5 sweep landed, E5a before E1.
2. DONE (docs/reviews/system-0-4-egate-2026-06-10.md): full gate re-run
   at 1e3e5e7 — ACCEPT; all remaining gaps are in this file's operator
   sections.
3. DONE 2026-06-10: operator signed off on the protected-crate waives
   (queue above, "I sign off" recorded in-session).
4. OPERATOR: provision credentials (.env per README) — ANTHROPIC_API_KEY
   first, then the one-haiku-smoke-call under a tight CostBudget; Slack
   app token + allow-listed user ids (Socket Mode listener exercise);
   Kalshi credentials last (they unlock nothing alone by design — I7).
5. OPERATOR: Kalshi demo-env fixture recording session (single session
   covers the 27-item checklist + websocket streams + voided market + fee
   fields — details in the Kalshi section below). STATUS 2026-06-10:
   delegated to the agent; recorder tool BUILT and session attempted —
   blocked on a demo key-id/PEM pairing mismatch (one operator step to
   unblock; see the Kalshi section). Then ENGINEERING:
   venue-generic runner composition replaying recordings into PaperVenue
   (first post-fixture task), kill-switch KalshiVenue plug with its OWN
   FORTUNA_KILLSWITCH_* credentials.
6. OPERATOR: Aeolus recorded-export one-liner (section below); Polymarket
   research authorization if/when wanted (parallel, not on the critical
   path); spec v0.9 fee touch-up before any Polymarket go-live.
7. PROMOTION (I7, operator-only): mech strategies Sim -> Paper forward
   window -> operator promotion to live capital; model swaps only via the
   shadow comparison harness recommendation; drawdown-halt re-arms stay
   CLI-only out-of-band. Live trading remains OFF until every step above
   is recorded.

## Operator-blocked: Kalshi fixtures (one recording session unblocks all)

**SESSION COMPLETE 2026-06-11:** after the operator installed the matching
demo key (the original mismatch: the configured id was a fresh key, the
available PEM was a February-dated one), the full session ran end to end —
60 captures under fixtures/kalshi/ covering the 27-item checklist EXCEPT
the ledgered exceptions (see README Known gaps incl. STP `maker` mode
unobserved, #20 vacuous empty-book capture, #17 cursor-stability sub-items
— gate finding F4), both WS flag states, and cleanup. Load-bearing wire
findings (full table in fixtures/kalshi/README.md): THREE error-body
shapes on the wire — nested {"error":{...}} (17/19 4xx), the flat OpenAPI
shape (json-decode 400s), and bare {"msg"} (parameter-validation 400s);
the adapter must parse all three (CORRECTED 2026-06-11, gate F2: the
original "nested everywhere / flat never occurs" claim was falsified by
this set's own captures); CANCEL-ACK/READ STALE RACE captured live (gate
F3 — checklist #15's highest-stakes item): DELETE 200 then GET ~360ms
later still "resting"/full remaining while re-cancel 404s — adapter must
poll-until-terminal after cancel and treat recancel-404 as canceled; 409
dup code string `order_already_exists`; canceled client_order_ids never
free up; non-resting cancels are 404; skew window (>5s, <30s);
post_only-cross rejected AT CREATE on demo (docs say 201-then-cancel —
demo/prod divergence to re-check); quadratic taker fee x0.07 confirmed
against two independent fills; cursor last-page = empty string.
REMAINING for clearance (T4.2): adapter re-pointed at recordings + nested-
envelope fix; settlement capture after the seeded market closes; voided
market when one occurs; series fee fields via event lookup; prod-parity
read-only re-record before live. The PERPS fixture session (18 items,
research §12) is now also credential-unblocked — recorder extension queued.

Historical record of the blocked first attempt (resolved above):
the recorder TOOL is built and committed —
`crates/fortuna-venues/examples/record_kalshi_fixtures.rs`, demo-hosts-only,
covers the 27-item checklist + both-flag-state WS captures + cleanup — and
the session ran to the auth wall, where it is BLOCKED ON A CREDENTIAL
PAIRING: the demo key id in .env (after repairing a stray leading character
in the pasted value) IS recognized by the demo environment, but the only
available private key (`~/keys/kalshi-demo.pem`, moved from
`~/Downloads/kalshi-demo-key.txt`) does not pair with it — every signed
request returns 401 `INCORRECT_API_KEY_SIGNATURE` under TWO independent
signing implementations (the adapter's rsa crate and an openssl-CLI probe)
across four message-format variants and both demo hosts; local clock skew
was +0.8s. Conclusion: the PEM belongs to a different key (a second demo
key, or the live key's download). UNBLOCK (operator, one step): either
locate the PEM that pairs with the configured demo key id, or create a
fresh demo API key at demo.kalshi.co (Account & security -> API Keys),
save the download to `~/keys/kalshi-demo.pem` (chmod 600) and put its key
id in `KALSHI_API_DEMO_KEY_ID`, then rerun:
`set -a && source .env && set +a && cargo run -p fortuna-venues --example
record_kalshi_fixtures`.
Incidental findings already banked from the probes: (a) the wire 401 error
envelope is NESTED — `{"error":{"code","message","details"}}` — not the
flat `ErrorResponse` the OpenAPI spec documents (at minimum for the auth
gateway; fixture the API-layer shape too); (b) unauthenticated GET /markets
returns 200 on demo (checklist #5, demo half); (c) auth `details` strings
observed so far: `INVALID_PARAMETER` (malformed key id) and
`INCORRECT_API_KEY_SIGNATURE` (sig mismatch).

- **Kalshi fixture recording + adapter clearance (T1.1).** The adapter is
  BUILT and tested against doc-derived samples (124 venues tests), but it
  is cleared for Sim development ONLY. Paper/live clearance requires
  operator-recorded fixtures under fixtures/kalshi/ confirming the 27-item
  checklist in docs/research/venue/kalshi-api-2026-06-10/research.md
  (highest-stakes items: 409-duplicate body shape, error code catalog,
  cancel-reconcile race, fills cursor terminal semantics, timestamp skew
  tolerance, fee_multiplier maker scaling).
  Unblock: operator records demo-env fixtures per
  crates/fortuna-venues/tests/kalshi_doc_samples/README.md.
  The SAME capture must also include:
  - websocket `orderbook_snapshot`/`orderbook_delta` + public `trade`
    messages. (Status 2026-06-10: the WS MESSAGE layer is BUILT
    doc-derived — parser, seq-gap detection, yes-scale subscribe builder,
    BookAssembler, and the stream->PaperVenue replay seam are tested
    against the verbatim official examples. The capture CONFIRMS the
    contract — esp. use_yes_price semantics — confirm against the fixture checklist's no-leg-pricing item (research checklist item #20; the recording session should exercise BOTH flag states) — and unblocks
    the live socket DIAL (signed-handshake auth, keep-alive, redial) plus
    the venue-generic runner composition replaying the recordings.),
  - a VOIDED market's settlement record (`market_result` documents only
    yes/no/scalar; the adapter hard-errors on anything else so a live void
    surfaces loudly instead of passing silently),
  - fee fields on fills (verifies the inferred maker-fee x multiplier
    scaling and the unused-in-the-wild `flat` fee_type mapping).
- **Kill-switch live venue plug (after fixture clearance).** The binary +
  freeze logic + self-test are complete and I4-proven against the sim
  venue; `freeze --venue kalshi` stays unwired until the adapter passes
  fixture confirmation — the kill switch must not take its first real
  cancel path through unverified venue code. Unblock: fixtures above, then
  wire KalshiVenue into the killswitch with its OWN credential set
  (FORTUNA_KILLSWITCH_* env).

## Operator-blocked: credentials

- **Venue + Anthropic + Slack credentials (env vars).** STATUS 2026-06-10:
  PROVISIONED AND VERIFIED LIVE by the operator — DATABASE_URL (fortuna db
  migrated, 23 relations owned by fortuna_app, connection verified as the
  app role), ANTHROPIC_API_KEY (the recommended haiku smoke call returned
  "FORTUNA smoke OK", 16in/8out tokens — the env-key cognition gate is now
  ARMED), Slack bot `fortuna` (auth.test ok; test post landed in ALL FIVE
  channels), FORTUNA_DEADMAN_URL set (deliberately not pinged yet: one
  ping arms the monitor and the runtime is not running — expect a false
  "down" page if armed before go-live). Remaining in this entry:
  - Kalshi: a key id is set — CONFIRM it is the DEMO-environment key (the
    fixture session needs demo; prod trading + the separate
    FORTUNA_KILLSWITCH_* pair come later) and that KALSHI_PRIVATE_KEY_PATH
    points at the downloaded .key PEM (chmod 600, outside the repo).
  - (Historical note kept: AnthropicMind was built and mock-tested at
    T2.5; the env-key gate IS the feature flag.)
  - Slack app token(s): send-side routing (config-driven channel router,
    Block Kit approval builder) is built and tested with a mock transport;
    the Socket Mode interactivity listener (button presses, slash-command
    kill REQUESTS — never re-arms, which stay CLI-only) is built-to-contract
    work that needs a real app + allow-listed user ids to exercise.
    Research contract ready: docs/research/ops/ (apps.connections.open,
    envelope ack, user-id allow-listing).
  - Kalshi API credentials: live trading also requires promotions (I7) and
    fixtures above; credentials alone unlock nothing by design.

## Operator-blocked: Aeolus

- **Operator-recorded Aeolus envelope export (T2.7).** The ingestion
  contract is FORTUNA-defined (AeolusEnvelope, strict deny-unknown-fields)
  and the full fixture->drafts->persisted->scored path is proven against
  fixtures/aeolus/sample_envelope.json (the contract sample). The
  OPERATOR-RECORDED real export remains open — it validates Aeolus's
  exporter, not FORTUNA's parser. A read-only export from the Aeolus box
  was attempted and DENIED by the permission classifier (prod read without
  explicit approval — correct call). Unblock: operator runs ONE read-only
  command and commits the output as fixtures/aeolus/recorded_envelope.json:
  `ssh Aeolus 'sqlite3 -json /home/ec2-user/aeolus/artifacts/live/aeolus.db
  "SELECT ... one run ..."'` shaped to the contract (or adds an export
  endpoint to aeolus-runner). Any mismatch is a contract negotiation,
  never a silent adapt.

## Operator-blocked: Polymarket US

- **Polymarket US adapter is a fixtures-gated STUB (T3.4); RESEARCH NOW
  DONE (2026-06-10, operator-authorized this session):**
  docs/research/venue/polymarket-us-2026-06-10/research.md (829 lines, 95
  archived sources (the doc header said 96; the independent gate counted 95 — erratum noted in the doc)). Material findings that reshape the build decision:
  (a) retail API has NO CLIENT ORDER ID — FORTUNA's crash-resubmission
  idempotency model does not transfer (institutional stack has clordId);
  (b) SUB-CENT TICKS ARE LIVE (0.5c, 0.25c preprod) + decimal quantities
  + decimal settlements — three explicit conflicts with the integer-cents
  core; (c) NO RETAIL SANDBOX — fixtures would be minimum-size recordings
  on PROD, or institutional preprod via firm onboarding; (d) fee reality
  CONFIRMED vs the 2026-06-09 research (taker 0.05 / maker -0.0125
  quadratic, banker's rounding); (e) sports-only listings today.
  OPERATOR DECISION RECORDED 2026-06-10: "polymarket should be after the
  perptuals [sic]" — Polymarket US is SEQUENCED AFTER the Kinetics perps
  module (shelved for now; the stub keeps refusing everything). Revisit
  after perps lands; the cents-core conflict still requires a spec-level
  price-tick decision before any build.

## Kinetics perps module (operator-directed 2026-06-10)

- **TRACK C FINDING — RESOLVED upstream (2026-06-12): the pre-existing
  `cargo fmt --check` violation** at
  crates/fortuna-venues/examples/record_kinetics_fixtures.rs:801 was
  fixed on main and reached track C via rebase onto f4b4a54;
  `cargo fmt --check` is now FULLY clean workspace-wide (verified at the
  T5.B5 battery). Original finding kept for the trail: the file was
  outside track C ownership (examples/, not src/kinetics*), so track C
  ledgered + skipped per loop rule 7 while its own diffs stayed
  fmt-clean.
- **T5.B4 kinetics adapter: ALL 4 SLICES LANDED, BOX TICKED** [heading
  updated per final-gate Info; the slice trail below is preserved]
  (track C, 2026-06-12, commits dd82ca1 + c4f6248): slice 1 = the DTO
  layer, fixtures-gated with FULL coverage (all 76 bodies classified +
  parsed vs recorded statuses, both WS streams zero-unknown); slice 2 =
  KineticsClient over the shared signed transport (own credential PAIR
  at composition; same RSA-PSS recipe — fixture item 1), request-shaping
  fixtures-gated against every .meta.json (method/path/query/body
  equality; the meta-equality test caught a real divergence: group
  trigger/reset send {}). SLICE 3 LANDED (commit e3d0dde): the adapter
  proper — place accepts ONLY GatedPerpOrder (I1 structural in the test
  suite: orders are gated through the real pipeline), reduce_only+GTC
  refused BEFORE the wire, 409 duplicate resolves to
  AlreadyExists{existing} via first-page client-id scan (PAGINATION GAP:
  a duplicate beyond the first 100 listed orders stays Rejected —
  acceptable for crash resubmission which retries promptly, ledgered),
  system fills classify as the distinct Liquidation arm (never silently
  absorbed), per-fill fee reconciliation vs posted tiers (the RECORDED
  promo-\$0 fill correctly yields a discrepancy vs the 0.0012 taker
  tier — fee-trap surfacing as designed). The RiskCurve converter
  (bus fix 4) landed with the gate-fix batch (3b21b7e).
  SLICE 4 LANDED (commit bbadfc0): WS session layer — recorder-accepted
  subscribe commands byte-pinned, handshake constants (finding 2), seq/
  torn discipline with no-advance-until-fresh-snapshot, both recorded
  streams replay gapless and fully typed. **T5.B4 BOX TICKED.**
  OPEN venue-state/composition items (not code gaps): funding_history
  ENTRY shape uncaptured (item 10 partial — demo rate was 0; raw JSON
  until a populated capture or the PROD parity sweep); notional risk
  limit per-market values uncaptured (empty map on demo);
  duplicate-resolution list scan is first-page-only (pagination);
  the WS `fill` channel's frame SHAPE is UNCAPTURED in the corpus (the
  committed private stream carries zero fill frames — see the
  SESSION-NOTES committed-capture annotation): real-time fill/
  liquidation notification rests on REST fills polling until a
  fill-frame capture or the PROD parity sweep lands;
  the LIVE dial (TLS + signed WS handshake + redial policy, reqwest
  transport wiring with the kinetics credential pair from env) is
  composition work that lands with the daemon integration — no live
  traffic from track C, ever (demo flips are operator actions).
- **TRACK C GATE FIX LIST: ALL 4 CLOSED (commit 3b21b7e)** — verdict
  was ACCEPT-WITH-GAPS, zero Critical
  (track-c-perp-gates-gate-2026-06-12.md). (1) CLOSED: at-boundary
  equality pinned for the liquidation floor AND the leverage cap
  (pass-at-exact + reject-one-past pairs; mutations M1/M3 now die).
  (2) CLOSED as ENFORCE: MarginSim refuses |rate| > 2% — the venue
  clamps before reporting, so an over-cap rate is corrupt input; we
  error rather than silently clamp data entering an append-only record.
  (3) CLOSED: leverage cap renders as '{n}.{d}x'. (4) CLOSED:
  RiskCurve::from_leverage_estimates (numeric tier sort, ceil'd
  conservative bps, fail-closed on leverage<1/non-finite/bad keys),
  shape-tested vs fixtures/kinetics-perps/markets__single.json; T5.B5
  tick wording corrected VISIBLY in BUILD_PLAN. Operator decisions
  parked on the bus (waive batch 5, F1 disposition, leverage-cap
  number) remain operator actions, not track-C work.
- **T5.B6 perp DST: DONE, box ticked** (track C, 2026-06-12, commit
  335e5e6): run-dst.sh gains the perp_dst stage (fortuna-state, same
  seed count as the other stages) — 6 accounted arms with a coverage
  floor, 7 per-seed invariants, all green at 2000 scenarios. NOTE FOR
  BATTERY OPERATORS (2nd occurrence today): a `cargo test --workspace`
  FIRST run under parallel-battery load was killed/contended (495/4,
  binaries vanishing mid-run); the immediate clean isolated re-run was
  853/0. If a track's battery reds with vanishing binaries, isolate and
  re-run before judging — and prefer one battery at a time per box.
  Operator config guidance discovered by invariant 3 is in ASSUMPTIONS
  ("T5.B6 perp DST"): set min_liquidation_distance_bps >= price_band_bps.
- **T5.B5 paper margin: DONE, box ticked** (track C, 2026-06-12, commit
  e8fe069): MarginSim in fortuna-state (track-C margin ownership) —
  mark-based PnL with VWAP-against-us entries, 04/12/20-UTC funding
  schedule (funding_times_between) + append-only accrual log,
  whole-account liquidation sim from recorded risk curves at worse-mark
  [ANNOTATION 2026-06-12 per final-gate Minor 4, never erased: at e8fe069
  "recorded risk curves" overstated the data source (config curves only);
  the recorded-curve path landed later via
  RiskCurve::from_leverage_estimates in the gate-fix batch 3b21b7e —
  same correction BUILD_PLAN carries visibly]
  + penalty, negative balances modeled. DEFERRED with owner: driving
  MarginSim from recorded streams inside fortuna-paper belongs to that
  crate's owner (track A); the engine + tests are the track-C
  deliverable. NOTE for all tracks: a disk-full (3 parallel batteries +
  36G main target) interrupted one DST stage mid-battery; track C freed
  15.4GiB by cleaning ITS OWN worktree target and re-ran the battery in
  full. Main's 36G target may want a quiet-moment `cargo clean` by its
  owner.
- **T5.B3 gate extensions: DONE, box ticked** (track C, 2026-06-12,
  commits 7782f5c slice 1 + b4561ca slice 2): the perp gate arm in
  fortuna-gates (MarginHeadroom/LiquidationDistance/LeverageCap/
  PerpNotionalCap, spec numbering 11-14, + perp arms of PriceSanity/
  SizeSanity/EdgeFloor with funding drag + fee-trap assumed fees, shared
  rate/idempotency/netting/halts, GatedPerpOrder sealed type, PerpConfig
  validation, 36 spec-first tests) + fortuna-state equity_with_margin
  (I2 composition: balance + worse-for-us uPnL + pending funding, 8
  tests) + fortuna-invariants ADDITIONS (perp I1 seal compile-fails,
  I2-extension breach lifecycle, I3 cross-domain halt; 3 new files).
  Two design readings FLAGGED for the verifier in ASSUMPTIONS ("T5.B3
  perp gate arm, slice 1"): GatedPerpOrder as a second sealed type, and
  the reduce-only risk-gate/edge-floor skip.
  DEFERRED with owner: wiring equity_with_margin into the daemon's live
  drawdown feed is fortuna-live (track A) and only becomes meaningful
  when perp runtime state exists (B4/B5); until then the composition +
  invariant pins are the deliverable.
- **OPERATOR WAIVE QUEUED (protected-crate touch, expected per
  orchestration.md): commit b4561ca touches crates/fortuna-invariants/ —
  PURE ADDITIONS: 3 brand-new test files (perp_i1_sealed_order,
  perp_i2_drawdown_extension, perp_i3_cross_domain_halt) + append-only
  doc-test additions to src/lib.rs (verified 25 insertions / 0
  deletions). No existing assertion, tolerance, or test name was
  touched. Waive request: confirm the additions stand.**
- **T5.B2 core perps types: DONE** (track C, 2026-06-12):
  fortuna-core/src/perp.rs — InstrumentKind, PerpPrice (i64
  ten-thousandths, checked ops, Decimal only at payload boundaries),
  PerpValue with explicit floor/ceil conversion to Cents, FundingAccrual
  (append-only record; positive rate = longs pay; amount floored against
  us), signed PerpPosition (floored uPnL, ceiled notional),
  MarginAccountView (worse-of-venue-vs-conservative mark governs, per the
  5.15 halt-math rule). 38 tests incl. 7 property suites written from
  spec text BEFORE implementation. Deferred-with-rationale items in
  ASSUMPTIONS ("T5.B2 perps core types"); InstrumentKind threading
  through shared Market structs deliberately deferred to B3/B4 (ownership
  boundary).
- **Phase A research: DONE** (2026-06-10/11):
  docs/research/venue/kinetics-perps-2026-06-10/research.md — 844 lines,
  ~50 sources, 110 raw archives including perps_openapi.yaml /
  perps_asyncapi.yaml / the SCM spec verbatim plus live prod+demo API
  captures. Headline build facts: DEMO CARRIES PERPS (open to all, mock
  funds); auth = same RSA-PSS recipe under /margin/*; tick $0.0001 (breaks
  Cents as the price carrier — venue-scoped PerpPrice type proposed);
  client_order_id REQUIRED (idempotency transfers); portfolio margin via
  API + UNPUBLISHED maintenance-margin formula (conservative gate stance
  required); Klear liquidates via order_source=system fills (legitimate
  venue-originated fills need a lifecycle state); fees $0 promo with real
  rates via /margin/fee_tiers from the June 11 release (re-check then).
  Known conflicts the doc flags: orderbook ordering vs spec text,
  help-center contract-size mislabel, NFA-id discrepancy in Kinetics' own
  filing.
- **Phase B: CONFIRMED by the operator 2026-06-11** ("your B1–B8 order
  supersedes the truncated directive") with amendments A (B0 recorder
  first/standalone — BUILT and RUNNING), B (funding_carry data-only
  >=60d), C (fee-trap rule); the operator's recovered original list is
  folded in (docs/design/kinetics-perps-module-plan.md §6 verbatim).
  BUILD_PLAN T5.B0-B8 enumerates the confirmed order. (This entry
  previously said "awaiting confirmation" after confirmation had landed —
  one ledger held two states; corrected per gate finding F7.)
- **OPERATOR (rides the SAME demo-key unblock as the Kalshi session):**
  perps fixture recording session — 18-item request list in research §12,
  output under fixtures/kinetics-perps/ (margin-WS signing path, order
  lifecycle, 409 code, funding/risk/fee_tiers captures). The same session
  must also capture, on the EVENT API: a public WS `trade` frame (never
  observed in the 60-capture session — ledger-gate fix 3; it gates the
  paper-engine trade-through replay), the STP `maker` mode, a two-sided
  REST orderbook (#20 re-capture), and the settlement re-poll. One credential
  fix, two recording sessions, ideally back-to-back.

## Spec maintenance: RESOLVED by v0.9 (2026-06-11)

- **Spec 5.2 fee claims** (stale "Intl mostly zero" / "US flat 10bp")
  were corrected — not erased — in the operator-directed v0.9 amendment
  (B1, confirmed in-session 2026-06-11: "Proceed with B1 (spec v0.9
  amendment)"), which also added the 5.15 perpetual-futures domain. The
  perps fee model (notional-fraction maker/taker via fee_tiers) entered
  5.2 alongside the corrections; the Kalshi event quadratic x0.07 is now
  marked fixture-confirmed (real demo fill, 2026-06-11).

## RALPH STOP 2026-06-13T02:34:09Z (track C — queue exhausted/out-of-ownership)

Stopping per loop rule 6 ("every priority item is blocked/exhausted — do
NOT invent unrequested work to stay busy; idle-and-stopped beats bloat").
Verified against the 2026-06-13 bus (main @ caefc14) this iteration:

PRIORITY (a) — bus track-C work = the perps RE-MERGE PACKAGE. COMPLETE
from my side at commit e027250 (this branch, tip = reapply d81ab6c + my
fixes):
  1. kinetics test re-recording-proof rework (all 4 suites derive
     through the path; the revert root-cause UUID pin is gone) — DONE.
  2. operator 2x leverage ceiling (min(config, asset cap), boundary-
     pinned) — DONE.
  3. full re-gate at 10000 -> re-merge — the BATTERY is green at 10000
     (fmt/clippy/workspace 966/0/DST exit 0, corpus 4 seeds); the
     re-gate + re-merge itself is VERIFIER-OWNED. I am blocked on the
     verifier for the perps plane to land on main. No track-c re-gate
     verdict exists past e027250 yet (latest reviews are 2026-06-12,
     pre-dating this commit).

PRIORITY (b) — loop queue after the red-seed fix = T5.B7 -> T5.B8. BOTH
are OUTSIDE track-C ownership (rule 7: I own perp modules in
fortuna-core, fortuna-gates perp extensions, fortuna-state margin pieces,
fortuna-venues/src/kinetics*, perp DST arms — and nothing else):
  - T5.B7 perp strategies (perp_event_basis / funding_forecast /
    funding_carry) would live in crates/fortuna-runner/ alongside
    mech_structural/mech_extremes/synthesis — NOT an owned crate.
  - T5.B8 kill-switch perps flatten lives in crates/fortuna-killswitch/
    (a SEPARATE I4 binary the loop forbids coupling to my crates);
    margin/funding telemetry + the funding-regime ROTA panel live in
    fortuna-ops / fortuna-runner / assets — none owned.
  Per rule 7 these are LEDGER + SKIP, not buildable by track C. They
  need either a track that owns those crates, or an explicit ownership
  expansion + the perps plane RE-MERGED first (B7 strategies consume the
  perp gate arm + types + sim; B8 flatten consumes the kinetics adapter
  — both should build on a MERGED base, not stack more on the unmerged
  re-merge branch and compound re-gate risk).

NET: the entire in-ownership perps tranche (B2, B3, B4, B5, B6 + the
gate-fix batch + the re-merge package) is DONE and gate-clean; the
north-star "B2-B6 as gate-clean work allows" is fully met. Nothing
in-ownership remains. The branch awaits the verifier's re-gate + the
operator's standing signatures (waive batch 5 + F1, already approved) to
re-merge. REBASE HAZARD (bus): do NOT plain-rebase this branch onto main
while revert 19b3888 is in history — it drops these commits as
duplicate-applied; the verifier re-merges, or use
`git rebase --reapply-cherry-picks`.

Battery at stop (this iteration, current HEAD e027250): fmt 0, clippy 0,
workspace 966/0, run-dst.sh 10000 exit 0 (4 corpus seeds incl. the
committed red seed; all 7 perp DST arms fire). Branch clean, nothing
pushed.

## PERPS RE-MERGE PACKAGE: COMPLETE (track C, 2026-06-12, re-armed loop)

Per the bus ("FOR THE POOL / restarted TRACK C") + the amended loop
queue, all three parts landed on the reinstated plane (revert-of-revert
d81ab6c):

1. CLIENT-ID FINDING (the revert root cause, exec-adjudicated option 2):
   the kinetics suites were value-pinning capture uuids; the corpus was
   RE-RECORDED for the re-gate and every pinned value broke. ALL FOUR
   suites now derive expectations through the path (params parsed out of
   meta.json; responses vs the recorded body's own fields; structure
   over capture state) — re-recording-proof, doctrine in ASSUMPTIONS.
   Bonus from the new capture: the WS fill frame shape is NOW CAPTURED
   and typed (closes the ledgered fill-frame gap); deep funding history
   REFUTES the universal 8h grid (wire finding, ASSUMPTIONS; research
   §4's "empirically confirmed" claim holds for the current era only —
   erratum candidate for the verification session's research doc).
2. OPERATOR 2x LEVERAGE CEILING (decision item 4): [perp.venues]
   max_leverage_x10 (Option; absent = interim per-asset ceiling stands),
   enforced as min(ceiling, asset cap), boundary-pinned (1.99x/2.00x
   pass, 2.01x refused). Production config entry = operator/composition
   step.
3. RED-SEED CORPUS COMMITMENT (amended queue): seed 11819682492387934495
   now lives in crates/fortuna-core/dst-corpus/ (perp-curve-exceeded-*.
   seed, failure-mode comment, never-delete) and the perp harness loads
   perp-dst-tagged corpus seeds with expect-arm + determinism assertions
   (replacing the in-file regression list; the designed-refusal arm +
   spec-5.15 halt landed pre-revert at e0d4ae2 and is reinstated).

## RALPH STOP 2026-06-12T10:22:01Z (track C — queue exhausted, protocol stop)

[POST-STOP ADDENDUM ~10:50Z, operator-resumed: the final gate
(track-c-final-gate-2026-06-12.md) BLOCKED on one red B6 seed — the
harness miscounted MarginSim's DESIGNED over-tier refusal as a failure
(nothing failed open). Fixed at e0d4ae2: designed curve_exceeded arm +
spec-5.15 halt, the production fail-closed path now ASSERTED rather
than dodged, seed 11819682492387934495 in the in-harness regression set
(deterministic every battery run), the gate's exact master green at
10000, full run-dst.sh 10000 exit 0. All three gate Minors + both
actionable Infos closed in the same commit (SESSION-NOTES
committed-capture annotation, fill-frame gap named here, B5 wording
annotated, B4 heading unstale'd, error-string whitespace). The merge
conditions in the verdict are met; re-gate when convenient.]

Track C's directive queue is COMPLETE; per the loop's rule 6, stopping
beats inventing unrequested work. Final state:

- T5.B2 core perps types: DONE (0fb2fa6-era; gate-verified).
- T5.B3 gate extensions + I2 composition + invariant ADDITIONS: DONE
  (gate-verified; operator waive batch 5 pending on the bus).
- T5.B5 paper margin (MarginSim): DONE (tick wording corrected per gate
  fix; recorded-curve converter landed).
- T5.B6 perp DST battery stage: DONE (6 arms, 7 invariants, in
  run-dst.sh permanently).
- T5.B4 kinetics adapter: DONE this session (DTOs -> client -> adapter
  -> WS session, every layer fixtures-gated against the operator
  recordings; box ticked at 4fd16de).
- Cumulative gate verdict: ACCEPT-WITH-GAPS, zero Critical; all 4
  fix-list Minors CLOSED (3b21b7e).

NOT track-C work (stays with owners): T5.B7 strategies + T5.B8 ops
panel (unassigned in orchestration.md; B8 mounts in ROTA per the bus);
operator queue (waive batch 5, F1 disposition, leverage-cap number,
key rotation, purge, disk); live/demo flips and credentials (operator
only); fortuna-paper MarginSim wiring + daemon equity feed (track A);
the live kinetics dial composition (daemon integration).

Final battery at stop: fmt 0 diffs, clippy 0 errors, workspace 939/0,
DST exit 0 (2000/stage; corpus 3 seeds). Branch track-c at 4fd16de,
rebased on main f4b4a54-era; all work committed, nothing pushed.

## Track D — news-aggregation Phase A

- **GATE RESPONSE — CRITICAL SSRF fail-open: FIXED (2026-06-13).** The gate
  (track-d-nws-gate-2026-06-13.md + the STOP escalation) reproduced a
  parser-differential host-pin bypass: the hand-rolled `host_of_https` in
  fetch.rs read `https://evil.example.com\@api.weather.gov/x` as host
  `api.weather.gov` (ADMITTED) while reqwest's WHATWG parser connects to
  `evil.example.com`. ROOT-CAUSE FIX applied exactly as directed: `host_of_https`
  DELETED; the pin decision now uses `reqwest::Url::parse(url).host_str()` — the
  SAME parser reqwest connects through — in one helper `canonical_https_host`,
  called for both the initial URL and every redirect hop (redirect-follow stays
  disabled in the transport; Location is re-validated through the same helper).
  Regression tests, all through the public `FetchClient::fetch` path: the exact
  backslash payload REFUSED as the initial URL AND as a redirect Location; a
  redirect-to-unpinned Location REFUSED; plus `#@`-fragment and path-only
  shapes. Verified empirically that the parser resolves the vuln payload to
  evil.example.com (refused) and the mirror `api.weather.gov\@evil…` to the
  pinned host (correctly admitted — `@evil…` is path; the pin tracks the TRUE
  connection host). Two stale pre-fix pin tests corrected to true WHATWG
  semantics (no assertion weakened — they encoded a wrong hand-rolled-parser
  model; `https:///nopath` resolves to host `nopath`, not an error). Empty-host
  guard added. NOT a backslash blocklist — parser unification. SWEEP: grepped
  the crate for any other hand-rolled URL/host parsing — the only remaining
  `starts_with("https://")` is config.rs:165, a COSMETIC startup pre-check
  (fail-fast on obviously-non-https config); it is NOT a security boundary —
  every fetched URL (initial + each redirect hop) is gated by
  `canonical_https_host` via `HostPin`, the single parser. Awaiting re-gate of
  the whole D1–D5 unit.

- **GATE RESPONSE — MAJOR (Layer-1 validator unwired): is D9, by design.** The
  gate noted the `StructuralValidator` (validate.rs) is not wired into a
  per-item ingest path. That is correct and intended for this stage: the
  validator runs in the INGESTION SCHEDULER (D9), between adapter.fetch() and
  the cognition normalizer — adapters stay dumb (spec 5.11). There is no ingest
  path to wire it into until D9 exists. The adapters already expose the inputs
  the scheduler needs (`nws_claimed_time`, `rss_claimed_time`). Tracked as D9
  scope; not part of this SSRF-only fix iteration (per the bus: "your NEXT
  iteration is THE SSRF FIX, nothing else"). If the gate wants the validator
  wired sooner, D9 can be pulled forward after the SSRF re-gate.

- **D4 NWS AFD full-text is a deferred second hop.** NwsSource emits the AFD
  product SUMMARY (id, office, issuanceTime, code) from the `/products?type=AFD`
  list. Attaching the full `productText` requires a second hop
  `GET /products/{id}` (shape captured in fixtures/sources/nws/afd_product.json).
  The summary already dedups and drives a "new AFD issued" trigger; the text
  hop is enrichment, not a blocker. Follow-up for a later iteration (would add
  a two-hop mode to NwsSource or a dedicated AFD-text source).

- **D4 NWS scheduler-side wiring pending D9.** The adapter exposes
  `nws_claimed_time` for the Layer-1 future-dated check, but the scheduler
  (D9) is what calls StructuralValidator with the extracted claimed_time and
  builds Candidates from RawSignals. D4 ships the adapter + the extractor;
  D9 connects them. Registry row + `[sources.nws_*]` config entries are also
  created at scheduler-wiring time (dossier admitted the source at tier 9).

- **Layer-4 consumption floors are only half-enforceable from this track.**
  The trigger floor can be enforced at the drive() seam (filter which signals
  are offered to TriggerEngine); the resolution-source floor is consumed
  inside world-forward discovery (fortuna-cognition — not Track D ownership).
  Phase A ships the config + registry data; the cognition-side check is
  ledgered here for the owner of fortuna-cognition to wire (design §4.4
  Layer 4).

- **CROSS-TRACK fmt red on main — RESOLVED on main (2026-06-13).** During
  D1 (pre-rebase) `cargo fmt --check` failed on
  `crates/fortuna-venues/examples/record_kalshi_fixtures.rs:43` (Track A/C
  commit c139386). After rebasing track-d onto main @ e85f92c the red is
  gone (gate bus certified main GREEN); `cargo fmt --check` is clean on the
  rebased tree. No Track D action was needed or taken. Left here as the
  record of why D1's commit message references it.

- **D3 Layer-1 stale-republication flag is a BOUNDED in-memory check, not
  authoritative dedup.** `StructuralValidator` flags a content hash seen
  within its recent-hash buffer (default 4096 hashes, FIFO eviction). A
  republication older than the buffer window is NOT flagged here — it is
  still caught downstream by the ledger's `UNIQUE(source, content_hash)`
  dedup (the source of truth, fortuna-cognition normalizer). The Layer-1
  flag exists for fast-path observability (a feed re-emitting old items is a
  health signal), not correctness. Sizing the buffer vs. a source's real
  re-emission window is a per-source tuning concern for D9/operations.

- **D3 re-decomposition: per-source Layer-0 dossiers ride with their
  adapters (D4–D7), not D3.** D3 shipped the dossier TEMPLATE/rubric
  (docs/research/sources/TEMPLATE.md) + the Layer-1 validator. Each source's
  filled dossier lands with that source's adapter and fixtures, facts
  grounded in research at record time. Phase A still exits with Layers 0–2
  complete; this is a sequencing change, not a scope cut.

## Disputed invariant tests
(none)
