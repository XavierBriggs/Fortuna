# Track D — D10 (1/2) config-driven source factory: gate + a stale-artifact post-mortem

Date: 2026-06-13. Target: track-d @ 30ae38f (unmerged), commit "D10 (1/2):
config-driven source factory + the [sources] feed variant". Files: factory.rs
(+296 new), config.rs (+9), lib.rs (+2), scheduler.rs (+5). Gated by the main
loop directly (focused slice; the live-exposure hard gate re-applies at 2/2).

## VERDICT: ACCEPT-SLICE — factory clean, no validator bypass, dirty-tree caveat resolved

D10(1/2) is the config→source construction layer the daemon's `drive()` seam
(2/2) will call. It does NOT make the scheduler reachable from the daemon yet —
the live-exposure hard gate still lands at 2/2.

- **No validator bypass (the load-bearing property).** `build_scheduler`
  (factory.rs:42) routes every config row through `scheduler.register(id, source,
  schedule, claimed, validator_cfg)` (factory.rs:64) with a `StructuralConfig`;
  the factory emits NO signals directly (grep: no `AcceptedSignal`/`emit`/`push`).
  So the D9 per-item validator (`scheduler.rs:232`) still governs everything the
  factory builds — "validator-guarded sources" per the module doc.
- **No-model Phase-A enforcement intact** (config.rs rejects enabled model
  extraction / scrape / mcp); the new `feed: Option<String>` field is the adapter
  variant the factory needs (nws→alerts|afd, calendar→schedule|latest), validated
  in the factory.
- **House style clean**: no `unwrap`/`expect`/`panic!`/`f64` outside `#[cfg(test)]`
  in factory.rs; `deny_unknown_fields` on the new config surfaces (config.rs:98,
  104, 119); negative tests reject missing/invalid trust tier (factory.rs:253,272).
- **Dirty-tree caveat from the D9 gate is RESOLVED**: the overlay is committed,
  track-d tree clean at 30ae38f; the `#[derive(Debug)]` fix landed (the
  implementer's own battery forced it).
- **Fresh battery (CARGO_TARGET_DIR isolated of the contamination below)**:
  `fmt -p fortuna-sources --check` exit 0; `clippy -p fortuna-sources
  --all-targets -D warnings` exit 0; `cargo test -p fortuna-sources` → 88 lib +
  5 DST passed, 0 failed, incl. `scenario_burst_is_capped_by_the_volume_envelope`.

## Post-mortem: a STALE SHARED-TARGET ARTIFACT produced a FALSE failure (caught, not shipped)

First battery run reported `scenario_burst_is_capped_by_the_volume_envelope`
FAILED (`left: 100, right: 10` — nothing refused), deterministic across re-runs.
Investigation BEFORE reporting (root-cause-or-it-isn't-real):
- The D10(1/2) diff has NO mechanism to break the cap: `scheduler.rs` only added
  an additive `source_ids()` getter; `config.rs` only added the `feed` field;
  the cap type + logic (`StructuralConfig.volume_envelope`, enforcement at
  `validate.rs:147`) live in `validate.rs`, which D10(1/2) does NOT touch. The
  test sets `volume_envelope: 10` directly and is itself unchanged.
- The failure signature (100 accepted / nothing refused) is EXACTLY what the D9
  gate's `assess`→always-`Accept` MUTATION produces. The D9 verifier ran that
  mutation against the SHARED `CARGO_TARGET_DIR=/tmp/fortuna-gate-target`; the
  separate `ingest_dst` integration-test BINARY persisted stale-linked against the
  mutated lib, while the lib's own unit tests recompiled fresh (so 88 lib passed
  but the stale integration binary failed — the tell).
- PROOF: `cargo clean -p fortuna-sources` (evicted 3297 files / 416 MiB incl. the
  stale test binary) → fresh rebuild → `scenario_burst ... ok`, 5/5 DST green.
  The contamination is remediated.
- D9's ACCEPT is UNAFFECTED: its verdict rested on the clean 84-lib+5-DST run
  BEFORE the mutation; the mutation was its last step and it observed RED then
  restored. Only the NEXT gate (this one) inherited the stale binary.

## PROTOCOL AMENDMENT (so this cannot recur)
**A mutation-check experiment (deliberately breaking code to confirm a test reds)
MUST run in an ISOLATED `CARGO_TARGET_DIR` (e.g. `/tmp/fortuna-mut-<n>`), OR be
followed by `cargo clean -p <pkg>` of the mutated package, before any later gate
reuses the shared target.** Otherwise the mutated artifact contaminates
`/tmp/fortuna-gate-target` and yields false PASS/FAIL in subsequent gates. The
tell is a split result: a package's lib unit tests pass while its integration
test binary (or vice-versa) fails with a logic-mutation signature. Verifier
subagent briefs that request a mutation check now carry this isolation rule.

## Remaining on track D
D10 (2/2) — the `drive()` seam wiring the scheduler into `fortuna-live` — is the
commit that makes the validator-guarded ingest path REACHABLE from the daemon.
That is the live-exposure hard gate; it re-applies the D9 refuse-and-quarantine
discipline ON the daemon path. After 2/2: gate D9+D10 as a unit, then merge.
