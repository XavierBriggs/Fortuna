# Track D — D9 validator-wired ingest core: THE HARD GATE

Date: 2026-06-13. Target: track-d @ 6b52f98 (unmerged), the D6–D9 tranche
(`git diff main...HEAD`). PRIMARY: `crates/fortuna-sources/src/scheduler.rs`
(673 new — D9 core), `tests/ingest_dst.rs` (wired-path DST), `src/validate.rs`
(Layer-1 validator). Secondary: `src/calendar.rs` (D6), `src/corroborate.rs`
(D8). Verifier subagent (executed mutation check) + main-loop confirmation of
the merge-blocking caveats. Rubric fixed before reading.

This gate exists to CLOSE the SSRF-gate MAJOR: "the Layer-1 validator is BUILT +
unit-tested but UNWIRED (zero production call sites)." It could only pass on
reproduction-of-refusal through the WIRED path — not an explanation, not a
direct-validator unit test.

## VERDICT: ACCEPT — D9 HARD GATE SATISFIED (validator wired + mutation-proven)

Mergeable as a COMMIT (6b52f98 is clean), but MERGE IS STRATEGICALLY HELD for
D10 — see "Merge disposition" below. Two non-defect caveats ledgered.

## The headline: reproduction-of-refusal on the WIRED path — CONFIRMED BY EXECUTED MUTATION

- Production call site: `scheduler.rs:232` `reg.validator.assess(now, &candidate)`,
  inside the per-item loop (`:225`), on EVERY fetched item before acceptance —
  only `Verdict::Accept` reaches `out.accepted` (`:233-240`); the three reject
  arms (`:241-264`) record to `out.dropped` + metrics and NEVER emit.
- Wired-path refusal test: `tests/ingest_dst.rs:175`
  `scenario_burst_is_capped_by_the_volume_envelope` drives 100 items through
  `IngestionScheduler::tick()` → 10 accepted / 90 `DropReason::OverVolume`.
  Reinforced by `scheduler.rs:440/463` (`future_dated_item_is_refused_not_ingested`,
  `republished_and_over_volume_are_refused`) — all through `tick()`, no
  direct-`validate()` bypass.
- MUTATION CHECK (EXECUTED, the load-bearing proof): the verifier neutralized
  `assess` → always `Verdict::Accept` in a throwaway worktree →
  `scenario_burst...` FAILED + the two scheduler unit tests FAILED (4 passed /
  1 failed on the DST target). Then restored the file (verified clean) + removed
  the worktree. The wiring IS gated: break it, the wired-path DST reds.

## Rubric (A–F), evidence before verdict

**A. Validator wiring / refusal — CONFIRMED** (above).
**B. Untrusted-data doctrine (5.11) — CONFIRMED.** No model/LLM in the path
(deps: reqwest/feed-rs/sha2/chrono/serde; `config.rs:215-219` REJECTS
`extraction="model"` for enabled sources — "Phase A: no model in the ingestion
path"). Scheduler never branches on payload content — SHA-256-hashes the opaque
payload (`scheduler.rs:323`), carries it as inert `AcceptedSignal` data; an
embedded instruction is data, never control. Doctored/malformed calendar input
fails closed (`calendar.rs:235-236,261,239`). SSRF host-pin NOT regressed:
calendar/rss fetch only through the shared `FetchClient`/`HostPin`;
`canonical_https_host` (fetch.rs) untouched, still the unified WHATWG `url`
parser; the rss.rs +13 is a pure `parse_feed_kind` refactor (no fetch/pin code).
**C. Clock injection — CONFIRMED.** Zero wall-time in scheduler/calendar/
corroborate (grep: no `SystemTime::now`/`Instant::now`/`Utc::now`/`Local::now`).
`tick(now: UtcTimestamp)` takes injected time, never sleeps (`:185`);
time-of-day derived from `epoch_millis` (`:357-362`). All 5 DST scenarios drive
`ts(ms)` explicitly under fault; registered in `scripts/run-dst.sh` (+5,
scoped to `-p fortuna-sources --test ingest_dst`).
**D. House style — CONFIRMED.** No `f64` for money/price (the only f64 is the
Jaccard similarity coefficient in `corroborate.rs:55/135/144` — spec-permitted
like a probability). No `unwrap`/`expect`/`panic!` before any `#[cfg(test)]` in
scheduler/calendar/corroborate/rss; `content_hash` is infallible
(`unwrap_or_default` + write-to-String). `deny_unknown_fields` guards the config
control-plane (`config.rs:94/100/114`); ingested payloads are deliberately
opaque `Value` (dumb-adapter design; strict downstream parsing is ledgered).
**E. D6 calendar + D8 corroborate — CONFIRMED.** Calendar fixtures-first
(`fixtures/sources/calendar/{bls_schedule.ics,bls_latest.rss}`), fail-closed,
with `calendar_claimed_time → None` for `release_scheduled` so intentional
future release times don't trip Layer-1 (`calendar.rs:285-298`). Corroborate is
fully deterministic (BTreeSet ordering, index union-find, stable cluster ids by
first appearance, output order = input order; empty-set guard); annotation is
algorithm-computed, never model-self-reported. GAPS/ASSUMPTIONS additions honest
(AFD-firehose live finding + the volume-envelope mitigation; D7-GDELT/D6-FRED
deferrals with fixture-blocked reasons).
**F. Protected crate + exposure — CONFIRMED / STATED.** `git diff main...HEAD --
crates/fortuna-invariants` empty. No test weakening (pure-addition tranche).
EXPOSURE: zero `fortuna-live` changes; no crate outside `fortuna-sources`
references `IngestionScheduler` → the scheduler is UNREACHABLE from the daemon.
The validator wiring is real and gated, but LIVE-INGEST EXPOSURE REMAINS BEHIND
THE PENDING D10 `drive()` SEAM (`BUILD_PLAN.md:772`, still `[ ]`). "Hard gate
satisfied" = the validator is provably on the scheduler's ingest path and
refusal reproduces through it — NOT yet a live signal flowing in the daemon.

## Commands (verbatim, clean detached worktree @ 6b52f98, CARGO_TARGET_DIR=/tmp/fortuna-gate-target)
- `cargo fmt -p fortuna-sources --check` → exit 0
- `cargo clippy -p fortuna-sources --all-targets -- -D warnings` → exit 0
- `cargo test -p fortuna-sources` → 84 passed (lib) + 5 passed (ingest_dst), 0 failed
- mutation (`assess`→Accept) → `--test ingest_dst` 4 passed / 1 failed (scenario_burst RED), then restored
- `scripts/run-dst.sh` SKIPPED (compiles the whole workspace; disk 99%/16Gi). Its D9 line is the scoped ingest_dst run already executed green.

## Merge disposition — HELD for D10 (not a defect)
6b52f98 is a clean, gate-passed COMMIT and main is clean (021d0e5). Merge is
HELD on purpose: (1) the scheduler is unreachable from the daemon, so merging
D6–D9 in isolation delivers ZERO live benefit until D10 wires the `drive()`
seam; (2) track-d's WORKING TREE carries an uncommitted, NON-COMPILING D10
overlay (`factory.rs` untracked; `config.rs`/`lib.rs`/`scheduler.rs` modified —
`IngestionScheduler` lacks `#[derive(Debug)]` while `factory.rs` calls
`.unwrap_err()` on it; also fails `fmt --check`). Merging additive-unreachable
code under an implementer's active uncommitted work is a coordination hazard for
no upside. PLAN: track-D finishes + commits D10 (the implementer's own battery
gate forces the Debug-derive fix before they can commit), then gate D10, then
merge D6–D9–D10 as one coherent, reachable, gated unit. Divergence risk is low
(fortuna-sources is track-D-owned + additive; no other track edits it).
MERGE THE COMMIT, NEVER THE DIRTY TREE.

## Minor (ledgered, non-blocking)
- `BUILD_PLAN.md:770` completion note says "89 crate tests"; committed reality
  is 84 lib + 5 DST. The extra count appears to include the uncommitted
  `factory.rs` tests. Cosmetic count drift in a completion note.
