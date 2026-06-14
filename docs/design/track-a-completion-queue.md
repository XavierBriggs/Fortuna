# Track A — completion-campaign queue (operator-directed 2026-06-13)

Authoritative for track A's (b)-priority. Re-read every iteration; the
verifier amends as gates land. All standard loop rules apply unchanged.

1. M3 REARM NOTICES (pool-released to track A; small, one iteration):
   - fortuna rearm output gains: "halt cleared in the ledger; the RUNNING
     daemon resumes only on restart — run: fortuna stop && fortuna start"
     (crates/fortuna-cli/src/main.rs rearm arm).
   - A ROTA/health surface for the same fact (simplest honest form: the
     health view exposes halt-state-vs-running-daemon divergence, or a
     last_halt_event summary). Design intent: runbooks/halt-and-rearm.md's
     four-state table must be readable off the console.

2. T4.2 BUILDABLE-NOW (in order):
   (i) Kalshi WS dial: signed handshake (recipe proven in fixtures), keep-
       alive, redial-with-resubscribe + seq-gap resync. The redial tests
       USE THE LEDGERED VENUE EVIDENCE (fixtures/kalshi/README.md, the
       2026-06-13 entry): mid-stream reset-without-close and 502-on-
       reconnect are the recorded behaviors the dial must survive. No live
       socket in any test (mock transport; the dial's first live exercise
       is operator-run).
       >> LIVE HANDSHAKE PROVEN ON DEMO 2026-06-13 (operator-directed, post-STOP):
          the signed WS handshake connects ("OK — 101 upgrade, authenticated") to
          wss://external-api-ws.demo.kalshi.co via examples/kalshi_ws_handshake.rs
          (demo-only, read-only). That first live exercise CAUGHT + FIXED a real
          bug: signed_request omitted the WS upgrade headers
          (InvalidHeader("sec-websocket-key")) — now built via into_client_request().
          Live streamed frames/ping cadence still unobserved (only future-dated demo
          markets were open). See GAPS POST-STOP + CHANGELOG.
   (ii) Venue-generic recorded-stream replay into PaperVenue: the book/
       delta WS fixtures EXIST — build the replay composition for both
       mech strategies. The TRADE-THROUGH portion is fixture-blocked (the
       public trade frame was never captured; retry pending venue
       recovery) — build the book-driven replay now, assert what book
       evidence supports, and LEDGER the trade-frame dependency precisely
       (no fabricated trade fixtures, ever).
       >> DONE e6dd7ec (2026-06-13): recorded_replay.rs (7 tests) — gapless
          fully-typed parse of both recorded fixtures, EXACT assembled book in
          PaperVenue, book-only replay = no fills, both mech strategies composed
          (abstain on the recorded book + liveness controls). Trade-through and
          a multi-market-bracket fixture ledgered in GAPS (never fabricated).
          Full battery green.
   (iii) Adapter paper-clearance VALIDATION: work the 27-item checklist
       (docs/research/venue/kalshi-api-2026-06-10/research.md) against
       fixtures/kalshi/ — each item PASS/FAIL/UNCOVERABLE with the
       fixture file cited; the 7 known gaps stay ledgered. Output: a
       clearance record the operator signs; venue=kalshi stays refused
       until that signature.
       >> CLUSTER 1 DONE f7206a4 (2026-06-13): kalshi_recorded.rs (18 tests) —
          FIRST recorded-fixture adapter tests; PASS items 1,7,8,9,10,13,14,16,
          17,18,20,21. Clearance record docs/design/track-a-kalshi-paper-
          clearance.md (UNSIGNED). Exposed 2 adapter gaps — G1 nested-error
          extraction RESOLVED (b2087fc), G2 exchange-status DTO pending.
       >> CLUSTER 2 CORE DONE 811e383 (2026-06-13): kalshi_recorded_roundtrip.rs
          (4 tests) — place→201→id, place→400→Rejected (G1 e2e), cancel stale-read
          race→Timeout (F16, no false success), fills round-trip. PASS items 6,
          8-routing, 15, 19-roundtrip.
       >> CLUSTER 2 TAIL DONE 1e96d20 (2026-06-13): item 7 recorded 409→AlreadyExists
          round-trip (the real nested order_already_exists body the synthetic
          placeholder awaited). Items 5 + 12 closed by CITED coverage (markets()
          round-trips ×5 in kalshi_adapter.rs; v2-only write path per item 16 +
          DTO-identity) — no vacuous re-tests. Clearance tally now PASSes 5,7,12.
          (iii) fully done bar the live WS handshake (operator-run).
       >> CANCEL-HARDENING F16a DONE 2026-06-13: a stale post-ack single-GET now
          reconciles ONCE against the order LIST (the authoritative terminal
          surface) — canceled→Ok, executed→Rejected, still-stale/absent→Timeout.
          The README finding-16 "recancel-404-as-canceled" heuristic was REJECTED
          (fill-masking — the 404 bodies for canceled/executed/unknown collide
          byte-for-byte); the list status is the safe discriminator. Mutation-
          proven (executed→Rejected). F16b (full multi-attempt bounded-backoff
          poll) deferred — needs an injected Sleeper + a recorded multi-stale
          fixture (never fabricated). See GAPS F16.
       >> CLUSTER 3 auth-401 routing DONE fe86cb5 (2026-06-13): recorded 401 bodies
          → Rejected w/ code surfaced (item 3 PASS; skew-mapping half of item 2).
          WS frame-parse done in 2(ii); remaining: live WS handshake (op-run).
   (iv) Kill-switch KalshiVenue plug: FORTUNA_KILLSWITCH_* credential
       pair, freeze --venue kalshi wiring + tests (mock transport); I4
       dependency rules absolute (no new killswitch deps); live exercise
       operator-run after clearance.
       >> MACHINERY DONE 4e3a484 (2026-06-13): kalshi_freeze.rs proves
          freeze_and_cancel cancels every open order over the REAL KalshiVenue
          (mock transport, no live socket); i4_killswitch_independence stays
          green (zero new dep).
       >> LIVE WIRING DONE 7f69b81 (2026-06-13): main.rs `freeze --venue kalshi`
          — load_kalshi_creds (lib, pure, fail-closed; 3 env vars incl. the new
          _BASE_URL, never defaulted) → KalshiSigner → ReqwestKalshiTransport →
          KalshiVenue → freeze on a SELF-SPUN current-thread tokio runtime
          (RealClock). I4 CONFIRMED — i4_killswitch_independence GREEN (tokio not
          forbidden + already transitive; self-test byte-unchanged). Secret-safe
          (redacting Debug, mutation-tested). 9 fail-closed tests + a subprocess
          refuse-without-creds proof. Full battery green (143/1324/0 + run-dst
          200 0-violations). REMAINING: the live EXERCISE is operator-run (3 env
          vars + demo key; clearance now signed @77bbca5). _BASE_URL operator-dep
          requested in GAPS.
   (v) Slack Socket Mode listener: build to the research contract
       (docs/research/ops/) with mock transport tests — kill REQUESTS
       only, re-arms stay CLI-only; live exercise needs the operator's
       app token (ledger the gate).
       >> A1 DONE ca5082d (2026-06-13): socket.rs decision logic (14 tests) —
          I2 re-arm REFUSED (airtight), allow-list fail-closed, halt-only to an
          injected sink, untrusted-data. Dep-clean (zero new).
       >> A2 DONE f52ee66 (2026-06-13): the ack-first envelope LOOP over a mockable
          SlackSocketTransport/Conn (socket_loop.rs 12 tests + 5 inline units) —
          mirrors the Kalshi WS dial. ack-BEFORE-process, bounded envelope-id dedup
          (SinkError-failed halt stays retryable — code-reviewer should-fix folded),
          SocketDial capped-exponential reconnect surviving loss + disconnect/refresh
          (no escalation on planned refresh), cancel watch. I2 preserved end-to-end.
          Zero new dep. Full battery green (test --workspace 134/1209/0 + run-dst 200
          0-violations). REMAINING: B (operator-gated) the daemon wiring + real WSS
          (tokio-tungstenite, +ping/pong timeout) + [slack.socket_mode] config +
          FORTUNA_SLACK_APP_TOKEN + operator-run live. (GAPS.)

3. T4.5 ROTA deferred panels (after T4.2: the WS gap/resync counters flip
   live as part of it; the rest per the BUILD_PLAN T4.5 entry and its
   TEST RULE — populated-path seeds, no vacuous tests).
   >> VALIDATED 2026-06-13 (fit-notes in rota-dashboard.md §10 "T4.5 validation";
      code-explorer map). Build order: (e) /gates.recent_rejections → (e)
      /settlement.recent_watchdog → (a) discovery joins → (b) verdict badge (low value).
      >> SLICE 1 DONE 59fa594 (2026-06-13): /gates.recent_rejections — view_gates merges
         recent_rejections from the audit gate_decision trail (verdict=Reject), §5 shape,
         newest-first; runtime-sqlx text-extract (audit_tail precedent); 3 populated-path
         tests; battery green (1384/0 + run-dst 200 0-viol).
      >> SLICE 2 DONE 9558d56 (2026-06-13): /settlement.recent_watchdog_events — view_settlement
         merges the audit `watchdog` rows (sub-kinds settlement_overdue/dispute_freeze/
         orphaned_position; ref_id=market), §5 {audit_id,at,kind,market_ref}, newest-first;
         runtime-sqlx text-extract; 3 populated-path tests; battery green (1387/0 + run-dst
         200 0-viol).
      >> SLICE 3 DONE 7ed3138 (2026-06-13): the gate-verdict BADGE — new /api/rota/v1/build
         parses the latest docs/reviews/*.md verdict (RotaState.reviews_dir capability;
         parse_verdict_token anywhere-in-line + ACCEPT*/BLOCK-validated; newest-by-mtime,
         no-panic). Parser units + populated-path scanner + endpoint + degraded; battery
         green (1391/0 + run-dst 200 0-viol).
      >> CORRECTION: (a) discovery joins are NOT track-A-buildable — design §4 DEFERS them
         (queries/prereqs unwritten), §12 puts triage-recall NOT-in-v1, and GATE-FINDINGS
         scopes "discovery" observability to TRACK B. (The build-order line above mis-listed
         (a) as a track-A step; superseded.)
      NET: track-A's T4.5 buildable-WITHOUT-OPERATOR surface is COMPLETE (e gates + e
      settlement + b badge). Remaining T4.5 is operator/verifier-BLOCKED:
      BLOCKED (operator/verifier, see GAPS): (c) WS counters need the operator-run live
      dial wired into drive(); (d) full money model needs an operator/design call to
      surface the mark-loop AccountView via a SimRunner accessor.

4. Deferred backlog: the main.rs composition-root guard (rides with the
   next main.rs change per the ledgered rationale).

COORDINATION: track D will land ONE flagged minimal drive() seam commit in
fortuna-live (your files) — accommodate it on rebase, review it as a
neighbor's seam, do not rewrite it; conflicts beyond the seam = ledger +
bus, never a silent merge resolution.

## ADDED 2026-06-13: T5.B7/B8 reassigned to track A (ownership correction)

> **SUPERSEDED 2026-06-13 — DO NOT BUILD B7/B8 from track A.** This section was
> written `ea63559` (06-12 22:58) when track C had stopped. The operator's LATER
> track reorg (`7fa4115`, 06-13 04:43, recorded in the bus GATE-FINDINGS-LATEST.md
> "TRACK STRUCTURE") assigned the **cognition belief-pipeline + perps (incl. the
> scalar plane, F5–F9) to track C** and directed C/D/E to continue
> (`82d32c8`). Track-c is an ACTIVE worktree building in fortuna-cognition/
> fortuna-runner; track A starting B7/B8 there now would COLLIDE in exactly the
> shared crates the reorg exists to separate. The bus is the authoritative
> coordination surface and it preempts this queue. Left in place for history; the
> live ownership is track C. (The perp foundation C delivered — scalar type,
> perp-strategy seam, funding kernel, basis kernel — is already merged to main.)

Track C correctly STOPPED — B7/B8 live in TRACK A's crates, not track C's:
T5.B7 perp strategy plugins need the Strategy trait/Proposal/CoreHandle in
crates/fortuna-runner (track A); T5.B8 kill-switch flatten is in
crates/fortuna-killswitch (track A); B8 telemetry + the funding-regime panel
are in crates/fortuna-ops. Track C delivered the in-territory INPUT: the
deterministic funding-forecast kernel (track-c @ 507b1ad: FundingWindow +
finalize_funding_rate, in fortuna-state). After T4.2 + T4.5, track A:
- T5.B7: wire perp_event_basis (Sim) + funding_forecast (consuming track C's
  507b1ad kernel — cherry-pick or merge it) + funding_carry (DATA-ONLY) as
  Strategy plugins; FEE-TRAP RULE (edge floors at assumed post-promo
  5-12bps; promo-$0 never justifies GO); I7 unchanged.
- T5.B8: kill-switch perps flatten (reduce_only IOC + cancel-all; extend the
  SEPARATE killswitch binary, I4 deps absolute); margin/funding telemetry;
  the funding-regime ROTA panel. Closes the Phase-5 EXIT.
