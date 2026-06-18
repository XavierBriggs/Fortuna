# Review: D10 (2/2) — wire ingestion live into the daemon — 2026-06-13
Base: main  Head: 57ea19c (track-d, UNMERGED pre-merge gate)  Verdict: ACCEPT
Protected crate touched: no (git diff main...HEAD -- crates/fortuna-invariants is EMPTY)

This is the live-exposure hard gate: D10/2 wires the D9 validator-guarded scheduler
into the running daemon (fortuna-live), making the Layer-1 StructuralValidator
LIVE-REACHABLE for the first time. It is the one flagged cross-track touch into Track A.

## Criteria (fixed before reading the diff)
- A DEFAULT-OFF / FAIL-CLOSED (task contract; spec 5.11): PASS — `ingestion: Option<IngestionSection>`
  (boot.rs +DaemonToml field); `enabled: bool` is REQUIRED (no #[serde(default)] => empty
  `[ingestion]` table fails to parse, forcing explicit enabled); `#[serde(deny_unknown_fields)]`
  at boot.rs:176. Spawn is triple-gated in main.rs: `ingest_pool` set only `is_some_and(|s| s.enabled)`,
  then match arm `(Some(sec), Some(ipool)) if sec.enabled`. Example config has NO `[ingestion]`
  (verified absent) => None => no clone, no loop. daemon_smoke 15/15 green: `drive()` signature is
  STRUCTURALLY untouched (ingestion is a separate loop spawned in main.rs, not a drive() param),
  so the non-disruption guarantee is by construction. DEFAULT-OFF PROOF: enabled is mandatory +
  deny_unknown_fields + the spawn guard; no path spawns ingestion without explicit enabled=true.
- B VALIDATOR LIVE END-TO-END + MUTATION (task contract; spec 5.11 data-not-instructions): PASS —
  e2e test ingestion.rs:358 `validator_is_live_a_future_item_never_becomes_an_envelope` drives
  IngestionCore.tick -> scheduler.tick (validator) -> normalize_and_dedup, asserts the future item
  is refused (dropped==1) and NEVER becomes an envelope (envelopes==1). MUTATION CHECK PERFORMED:
  neutralized the validator at scheduler.rs:232 (`match reg.validator.assess(...)` -> `match Verdict::Accept`,
  always-accept) IN-PLACE in the warm worktree => the e2e test went RED (panic at ingestion.rs:366
  "only the fresh item is normalized": envelopes became 2). RESTORED (md5 back to 2c9c62a9...,
  git clean), `cargo clean -p fortuna-live` + `-p fortuna-sources` (evicted 6.9 GiB of possibly-
  contaminated binaries), rebuilt from clean => GREEN. The daemon-side test is non-vacuous and
  genuinely coupled to the live validator. HARD GATE SATISFIED.
- C OFF-MONEY-PATH + I4 (task contract; spec I4): PASS — ingestion.rs imports ONLY signals normalizer,
  clock, SignalsRepo (signals store, not intents/orders), SlackRouter, IngestionScheduler; references
  fortuna_gates/fortuna_exec/fortuna_state/GatedOrder/place(/positions ZERO times (only doc-comment
  "hard gate" mentions). Persist failure is non-fatal: tick_and_persist matches `Err(_) => persist_failures += 1`
  (ingestion.rs:159); run_ingestion_loop returns `IngestStats` (not Result) so it cannot propagate a
  crash/halt; Slack send is `let _ = slack.send(...)` (swallowed). i4_killswitch_independence PASSES
  (16.27s real subprocess) — it walks fortuna-killswitch's transitive normal-dep graph and forbids
  sqlx/postgres/fortuna-ledger/fortuna-cognition; ingestion's deps (ledger/cognition/sources) are NOT
  reachable from the killswitch, so I4 gained no ingestion dependency.
- D CLOCK INJECTION (CLAUDE.md; spec): PASS — run_ingestion_loop reads time via `clock.now()`
  (ingestion.rs:267); NO SystemTime::now/Instant::now/Utc::now in the loop logic (swept). Real-time
  sleep only at the IO edge via `tokio::time::sleep(tick_interval)` inside a `tokio::select!` that
  also honors the stop oneshot (ingestion.rs:268-271); a pre-tick `stop.try_recv()` gives clean
  shutdown. IngestionCore is deterministically testable (scripted Source + explicit `ts(ms)` clock).
- E NO MODEL + minimal cross-track seam (spec 5.11; I6): PASS — no Mind/LLM/provider/model/Belief in
  ingestion.rs (dumb adapter). boot.rs (+41) is purely the additive `[ingestion]` config section;
  main.rs (+50) is additive flag-gated spawn/stop; lib.rs +1 module line; Cargo +1 dep
  (fortuna-sources). drive() unchanged (daemon_smoke 15/15 is the non-disruption proof).
- F HOUSE STYLE + PROTECTED CRATE + BUILD (CLAUDE.md): PASS — no f64/f32, no unwrap/expect/panic
  outside #[cfg(test)] in ingestion/boot/main added regions (swept); thiserror enum
  IngestionBuildError; user_agent is config-sourced (section.user_agent), no secret literals.
  crates/fortuna-invariants UNTOUCHED (empty diff). Build WORKS with SQLX_OFFLINE=true (all of fmt/
  clippy/test ran under SQLX_OFFLINE=true GREEN — the offline query cache covers the new SignalsRepo/
  SourceRegistryRepo queries; CI offline mode will not break). fmt clean, clippy --all-targets
  -D warnings exit 0, cargo test -p fortuna-live all green (incl. ingestion 3, daemon_smoke 15).

## Findings
- [Minor] No dedicated boot-level test exercises `[ingestion]` config parsing (e.g. example yields
  ingestion:None, deny_unknown_fields rejects a typo, empty `[ingestion]` table fails because enabled
  is required). The default-off guarantee is provable from code (required enabled + deny_unknown_fields
  + main.rs spawn guard + daemon_smoke proving drive() untouched) but is not asserted at the
  config-parse layer. Non-money-path coverage gap; ledger in GAPS.md. Not a blocker.

## Commands run (verbatim results)
SQLX_OFFLINE=true cargo fmt -p fortuna-live --check            -> FMT_EXIT=0
SQLX_OFFLINE=true cargo clippy -p fortuna-live --all-targets -- -D warnings  -> CLIPPY_EXIT=0 (Finished, no warnings)
SQLX_OFFLINE=true cargo test -p fortuna-live                   -> all binaries ok; daemon_smoke 15/15; lib 5/5 (incl ingestion 3); 0 failed
SQLX_OFFLINE=true cargo test -p fortuna-invariants i4          -> i4_killswitch_independence ... ok (1 passed, 16.27s)
MUTATION (scheduler.rs:232 always-accept) -> validator_is_live_... FAILED (panic ingestion.rs:366) [RED, as required]
RESTORE + cargo clean -p fortuna-live -p fortuna-sources + rebuild -> validator_is_live_... ok [GREEN]
git diff main...HEAD -- crates/fortuna-invariants -> EMPTY (protected crate untouched)
post-review: git status clean, scheduler.rs md5 2c9c62a9... (restored), disk 10Gi avail (freed 6.9GiB)
