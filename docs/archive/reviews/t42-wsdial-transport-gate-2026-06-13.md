# T4.2(2i) WS-dial — concrete-transport + keep-alive + error-classification gate

Date: 2026-06-13. Target (on main): `de481f6` (slice 4, tungstenite-error
classification), `ce72161` (slice 5, keep-alive silent-half-open detection),
`29c45c8` (concrete `KalshiWsTransport` over tokio-tungstenite). Files:
`crates/fortuna-venues/src/kalshi/{ws_transport.rs,dial.rs}`, Cargo.toml, GAPS.md.
Continues t42-wsdial-gate-2026-06-13.md (slices 1-2). Verifier subagent +
main-loop corroboration. Rubric fixed before reading.

## VERDICT: ACCEPT-SLICE — dial logic promotion-ready, gate-clean

The concrete transport is a thin adapter over tokio-tungstenite; every dial
DECISION (redial-with-resubscribe, seq-gap resync, keep-alive half-open detection,
error classification) is tested against an in-memory mock. The only untested seam
is the live socket round-trip — honestly ledgered, reserved for the operator's
first-live exercise (venue=kalshi stays boot-refused until then).

## Findings (evidence before verdict; file:line)

**A. Live-socket discipline — CONFIRMED (the headline).** Dial logic is generic:
`run_dial<T: WsTransport>` / `pump_session<C: WsConn>` (`dial.rs:202,281`). The
mock seam is concrete: `MockWsTransport::connect` returns `MockWsConn` with zero
`connect_async` (`dial.rs:382-410`); `MockWsConn` replays scripted recv/send
(`dial.rs:340-369`). The load-bearing recovery test
`run_dial_survives_a_reset_then_a_502_and_recovers` (`dial.rs:426-495`) drives
`run_dial(&MockWsTransport,…)` and asserts `connect_count()==3` + two recovery
snapshots + the 500ms→1000ms backoff — full redial/resubscribe through the mock,
no socket. All five `connect_async` refs (`ws_transport.rs:4,17,34,96,152`) sit
BEFORE the `#[cfg(test)]` boundary at line 228; none in any test. MUTATION CHECK
satisfied: the dial logic is exercisable without `connect_async`. (`kinetics_ws.rs:185`
`wss://…demo.kalshi.co` is a string-constant assertion of the signed URL, not a
live connect — pre-cleared.)

**B. Clock/Sleeper injection — CONFIRMED.** Slice 5's `KeepAlive` is a pure state
machine taking `now_ms` (`dial.rs:155-176`), fed from the injected clock at the IO
edge (`ws_transport.rs:195,213`); the keep-alive tick sleeps through the injected
`Sleeper` (`ws_transport.rs:212`). Three scripted-clock tests drive the half-open
timeout deterministically. The only wall-time is `tokio::time::sleep` inside the
production `TokioSleeper` impl (`dial.rs:270`) — the single designed IO-edge seam.
Zero `Instant::now`/`SystemTime::now`/`Utc::now`.

**C. House style on the path — CONFIRMED.** No `f64`/`f32` in either file. Every
`unwrap`/`expect`/`panic!` is inside `#[cfg(test)]` (past the 228 / 331 boundaries)
or a doc comment. `classify_ws_error` (`ws_transport.rs:47-63`) maps every
tungstenite error class to a typed `DisconnectCause` with a catch-all `_ =>
Transport` — no panic on any class. `redial_backoff` (`dial.rs:324`) deliberately
avoids `unreachable!()` to honor the venues no-panic rule (degrades impossible arms
to `Duration::ZERO`).

**D. No regression — CONFIRMED.** `cargo test -p fortuna-venues --lib` →
`21 passed; 0 failed; 0 ignored`. By name: the recovery test, the recorded
reset/502 evidence test, `pump_resubscribes_to_resync_on_a_sequence_gap`,
`a_seq_gap_resyncs_without_touching_the_redial_backoff`, + 5 ws_transport tests.
+99 lines to dial.rs (the KeepAlive add), no `-` deletions in `src/` — slices-1-2
logic untouched.

**E. Protected crate + honest GAPS — CONFIRMED.** `git diff 465f2d4..29c45c8 --
crates/fortuna-invariants` empty. GAPS updates to "WS DIAL COMPLETE" and ledgers
the untested live-socket round-trip + the operator-run-first dependency, no
fabricated coverage. Test-weakening sweep: no added `#[ignore]`, no removed
asserts, no proptest cuts; only `-` lines are the legit dev-dep→normal-dep
promotion of `futures`/`tokio-tungstenite`.

## Commands run (all under CARGO_TARGET_DIR=/tmp/fortuna-gate-target)
- `cargo fmt -p fortuna-venues --check` → exit 0
- `cargo clippy -p fortuna-venues --all-targets -- -D warnings` → exit 0, 0 warnings
- `cargo test -p fortuna-venues --lib` → `21 passed; 0 failed`

## Residual NOT closed by this gate (preserved honestly)
Scope was the venue crate (disk at 15Gi/99% — workspace battery + DST out of
scope). The GAPS line "DST 4 corpus + 10000 seeds zero violations" is the
IMPLEMENTER's claim, NOT verifier-confirmed here. It does not bear on the
dial-logic gate (covered by the 21/21 targeted lib tests) but must be
workspace-confirmed at the next full battery before any Phase-4 EXIT roll-up
counts it.
