# Review: T4.2 redial-loop slice (806c199) — 2026-06-13
Verdict: ACCEPT-WITH-GAPS (1 Minor). Cheap-tier, executed-evidence, by the
verification session — the originally-dispatched independent agent died on a
server-side rate-limit wave mid-battery; its SALVAGED evidence (final message)
is combined with direct re-verification below. No verdict was issued on the
agent's incomplete word; every PASS here carries a command I ran this session.

## Rubric (the prior dial gate t42-wsdial-gate deferred the Clock check to here)

- A (Clock-injection — the deferred check): PASS. No wall-clock reads in the
  dial (grep: zero SystemTime::now / Instant::now / Utc::now in dial.rs). The
  only timing primitive is `tokio::time::sleep(backoff)` inside a
  `tokio::select!{ cancel.changed() => return, sleep => {} }` (dial.rs:233-236)
  — the correct cancellable-backoff IO-edge pattern (CLAUDE.md "tokio for IO at
  the edges"). Backoff is pure capped-exponential `Duration` math
  (with_backoff(base, cap), cap 30s default); tests pin it to `Duration::ZERO`
  for determinism. The tokio runtime clock at the edge does not leak into the
  deterministic core (the live dial isn't run by the DST harness; its first
  live use is operator-run).
- B (cancellability / no runaway / no leak): PASS, 1 MINOR. The killed agent
  MUTATION-PROVED the cancel arm load-bearing (neutralize the mid-pump
  `cancel.changed()` => the recovery test HANGS). No `tokio::spawn` in the
  slice (grep) — `run_dial` is inline async, so cancel-return fully tears it
  down, no detached task leaks. MINOR: only the mid-pump cancel path has a
  dedicated test; the top-of-loop (`*cancel.borrow()`, :215) and backoff-select
  (:234) cancel arms share the same watch mechanism but lack independent tests
  — coverage gap, not a defect (mechanism is mutation-confirmed shared).
- C (reset+502 recovery, non-vacuous): PASS. Ran
  `run_dial_survives_a_reset_then_a_502_and_recovers` green (kalshi_ws: 7
  passed, 0 failed). Read its assertions (dial.rs:330-372): drives MockWsTransport
  through 3 connections (healthy→ResetWithoutClose, 502, recovered); asserts
  connect_count()==3 (survived BOTH recorded venue failures), snapshots==2 (the
  recovery snapshot on conn #3 arrives ONLY because the reconnect resubscribes —
  this assertion IS the resubscribe load-bearing check), deltas==1 (no torn book
  across the gap). Non-vacuous by construction.
- D (no live network — absolute): PASS. grep tungstenite/connect_async/ws://
  /wss:///TcpStream over src + tests, excluding mock/fixture/comment => ZERO
  hits. Transport is a trait; tests inject MockWsTransport. No test opens a
  socket; no .demo/prod host dialed.
- E (backoff bounded): PASS. Capped-exponential with an explicit cap (30s
  default; tests pin ZERO). No infinite-fast spin, no unbounded growth.
- F (fixture-gating unchanged): not re-verified here (no dial change touches the
  boot refusal); the venue=kalshi clearance refusal is unrelated to this slice.

## Commands run (verbatim, this session)
- grep wall-clock/socket/spawn over dial.rs + tests => clock-clean, socket-clean, no spawn
- sed dial.rs:230-240 => tokio::select! cancellable sleep confirmed
- cargo test -p fortuna-venues --test kalshi_ws => "test result: ok. 7 passed; 0 failed"
- read dial.rs:330-372 => recovery test assertions non-vacuous (connect_count 3, snapshots 2, deltas 1)
- (two resubscribe mutations attempted; both failed Rust type-inference at the
  test tooling, inconclusive — superseded by the assertion-structure proof above)

## Finding
- [Minor] cancel-path test coverage: backoff-select and top-of-loop cancel arms
  lack dedicated tests (mid-pump path is mutation-proven; mechanism shared).
  Track A: add two small cancel tests or ledger the gap. Non-blocking.

Merge note: the slice is on main as track-A's own commit; this CERTIFIES it
(ACCEPT-WITH-GAPS). No fix-forward required; the Minor is a coverage add.

## ADDENDUM 1 — Sleeper-injection follow-up (7fb57c9) + a self-correction

The Sleeper follow-up (7fb57c9): CERTIFIED. Track A went beyond this gate's
requirement — my rubric A accepted the tokio edge sleep as PASS, but track A
injected it through a `Sleeper` trait (TokioSleeper prod / RecordingSleeper
tests), so run_dial now embeds NO wall time and tests are deterministic without
real delays. The cancellable select is preserved (cancel.changed() arm still
present alongside sleeper.sleep). async-trait is a workspace dep. clippy
-D warnings clean.

SELF-CORRECTION (claim-vs-reality discipline applies to the verifier too): the
original verdict above cited `cargo test --test kalshi_ws => 7 passed` as
evidence the recovery test passes — that was the WRONG TARGET (the WS-PARSER
integration tests, coincidentally also 7). The dial recovery/cancel tests are
LIB unit tests in dial.rs. Corrected: ran
`cargo test -p fortuna-venues --lib dial` =>
`run_dial_survives_a_reset_then_a_502_and_recovers ... ok` (7 passed, 0 failed)
at 7fb57c9. The original verdict's CONCLUSION stands (the killed agent's
mutation evidence and the non-vacuous assertion-reading carried it); only my
cited command was mistargeted. Now confirmed by the correct target.
