# Review: T4.2 item 2(i) Kalshi WS dial (slices 1+2) — 2026-06-13

Scope: the TWO T4.2 dial slices ONLY (perps merge 0ea7308/04092f9 EXCLUDED — already
gated, perps-remerge-gate-2026-06-13.md; it touches NO dial file).
- Slice 1: `2683880` "t4.2(2i) slice 1: Kalshi WS dial decision core — redial-with-resubscribe + seq-gap resync"
- Slice 2: `2510556` "t4.2(2i) slice 2: Kalshi WS session pump — subscribe, parse-pump, in-place seq-gap resync"

Base (slice1 parent): 2683880~1   Head: 2510556
Files gated (cumulative dial diff): crates/fortuna-venues/src/kalshi/dial.rs (+411),
crates/fortuna-venues/src/kalshi/mod.rs (+1; `pub mod dial`). GAPS.md ledger entries
reviewed but not code. ws.rs (the parser the dial composes) read for context.

Verdict: ACCEPT
Protected crate touched by the dial slices: NO (perp_i2/perp_i3 invariant tests in the
range belong to the EXCLUDED perps merge; `git show --stat 2683880 2510556` shows neither
dial slice touches crates/fortuna-invariants/).

## Rubric grades (A–F)

- A. NO LIVE NETWORK: PASS. No socket, host, or live-dial construct in dial.rs or any
  fortuna-venues test. Grep for `tungstenite|connect_async|ws://|wss://|TcpStream::connect|
  .demo.|api.elections.kalshi|trading-api.kalshi` over dial.rs + tests/ → zero hits
  (the single match is a doc-comment "demo WS host" at dial.rs:30). The redial/pump
  tests run against an injected scripted `MockWsConn` and recorded frame strings, never
  a live venue. The dial is a pure state machine + a pump over an injected `WsConn`
  trait; the connect that PRODUCES a `WsConn` is explicitly deferred (GAPS, "next slice").
- B. REDIAL-WITH-RESUBSCRIBE: PASS + MUTATION-PROVEN. `dial_survives_the_recorded_reset_
  then_502_evidence` replays the exact ledgered sequence (healthy connect → mid-stream
  ResetWithoutClose → ConnectHttpError{502} on reconnect → recovery) and asserts
  Subscribe / Redial(500ms) / Redial(1000ms) / Subscribe-on-recovery, plus backoff
  RESET on clean connect. `on_connected()` ALWAYS returns `Subscribe` (re-baseline, no
  surviving-subscription assumption). MUTATION: suppressing the resync re-subscribe in
  `pump_session` → `pump_resubscribes_to_resync_on_a_sequence_gap` RED (left 1, right 2).
- C. SEQ-GAP RESYNC: PASS + MUTATION-PROVEN. Parser emits `SeqGap` and does NOT advance
  the baseline across a gap (torn-book guard, ws.rs:132-151); `pump_session` surfaces the
  gap then resubscribes in place + resets the parser WITHOUT dropping the connection
  (`pump_resubscribes_to_resync_on_a_sequence_gap` asserts SeqGap{expected:3,got:4} AND
  sent.len()==2). MUTATION: suppressing gap detection in the parser → the same dial test
  RED ("the gap is surfaced" fails; the torn delta is silently applied as a BookDelta —
  exactly the torn-book hazard).
- D. DETERMINISM / CLOCK: PASS (for this slice's scope). Grep over dial.rs for
  `SystemTime|Instant::now|Utc::now|tokio::time|sleep|f32|f64` → zero (the only `panic!`
  is inside `#[cfg(test)]` at line 402). Backoff is computed as pure capped-exponential
  `Duration` values (deterministic, no jitter); the dial performs NO wall-clock read and
  NO sleep. The Clock-driven keep-alive timer + the loop that consumes these Durations
  are deferred to the next slice and ledgered (the `KeepAliveTimeout` cause is modelled,
  its firing timer is not yet present). No f64 anywhere; prices/qty are handled by the
  composed parser as integer Cents/Contracts.
- E. FIXTURE-GATING: PASS. The dial introduces no boot/connect path and never references
  clearance; it is plumbing inside fortuna-venues. The venue=kalshi boot refusal lives in
  crates/fortuna-live/src/boot.rs:247 with test boot.rs:168
  `venue_kalshi_refuses_until_fixture_clearance` — `git diff 2683880~1 2510556 --
  crates/fortuna-live/` is EMPTY, so the refusal is untouched and unbypassed.
- F. BATTERY: PASS. fmt clean; clippy -D warnings RC=0; `cargo test -p fortuna-venues`
  all green (12/19/33/10/31/10/11/13/8/2/4/27/14/0, 0 failed, 0 ignored); the 6 dial
  tests green; workspace `cargo check --all-targets` clean (additive API breaks no
  consumer); test-weakening sweep over the dial diff = ALL additions, no removed assert /
  no `#[ignore]` / no proptest reduction / no loosened tolerance; protected crate
  untouched by the dial slices; worktree restored clean after both mutations (shas match).

## Findings

- No Critical. No Major. No Minor blocking findings.
- [Note, not a defect] The Clock injection, signed-handshake connect, ping/pong
  keep-alive timer, and the redial LOOP tying WsDial→pump_session across reconnects are
  DEFERRED to the next 2(i) slice. This is honestly ledgered in GAPS.md by both slices
  (not silently dropped) and is consistent with the dial being a pure decision core. The
  Clock-discipline check (rubric D) therefore applies fully only when that loop lands;
  the operator should re-gate the next slice specifically for: (1) injected Clock for
  backoff sleeps and keep-alive deadlines (no SystemTime/Instant/tokio::time::sleep on
  wall-clock), (2) a mock ASYNC transport replaying the reset/502 evidence end-to-end,
  (3) the signed-handshake connect against the auth.rs recipe. None of this weakens the
  current slices; it is the explicitly-scoped remainder of item 2(i).

## Commands run (verbatim results, trimmed to verdict lines)

- Scope: `git log --oneline caefc14..2510556` → slices 2683880 + 2510556 identified by
  subject; perps merge 0ea7308/04092f9 between them touches no dial file
  (`git log --oneline 2683880..2510556 -- .../kalshi/dial.rs .../kalshi/mod.rs` → only 2510556).
- `git diff --stat 2683880~1 2510556 -- .../kalshi/dial.rs .../kalshi/mod.rs` → dial.rs +411, mod.rs +1.
- A grep (dial.rs + tests/): live-dial constructs → none (only doc-comment "demo WS host").
- D grep (dial.rs): SystemTime/Instant/Utc/tokio::time/sleep/unwrap/expect/panic/f32/f64
  → only `panic!` at line 402 inside `#[cfg(test)]`.
- `cargo fmt --check -p fortuna-venues` → FMT_EXIT=0.
- `cargo clippy -p fortuna-venues --all-targets -- -D warnings` → CLIPPY_RC=0, zero error/warning lines.
- `cargo test -p fortuna-venues --lib kalshi::dial` → 6 passed; 0 failed; 0 ignored.
- `cargo test -p fortuna-venues` → all groups OK, 0 failed, 0 ignored.
- `cargo check --workspace --all-targets` → Finished, RC=0, no error/warning.
- MUTATION B (suppress resync resubscribe in pump_session) →
  `pump_resubscribes_to_resync_on_a_sequence_gap` FAILED (assertion left:1 right:2). Restored (sha 3a0fe6fd...).
- MUTATION C (suppress seq-gap detection in ws.rs parser) → same dial test FAILED
  ("the gap is surfaced" — torn delta applied as BookDelta). Restored (sha 8931133a...).
- Final `git status --short` → empty (worktree clean); HEAD 2510556.

## Merge note

These two slices are already on main as track-A T4.2 item 2(i) commits. This gate
CERTIFIES them (ACCEPT). No fix-forward required. The deferred async/Clock-driven loop
remains queued and must be gated as its own slice when it lands (see the Note above).
