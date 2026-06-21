# loop-close-gaps.md — forward / deferred items for the Loop-Close & Provable Demo milestone

Work-specific gaps tracker for **THIS milestone only** — distinct from the repo-wide `/GAPS.md`
and any worktree copy (named explicitly to avoid collision). The loop records deferred/forward
items here. Truly constitutional / repo-wide gaps also belong in `/GAPS.md`.

## Deferred (this milestone)
- **Synthesis BINARY-belief resolution** — DEFERRED. Synthesis provenance shape is undefined; the
  demo head-to-head is Aeolus-vs-meteorologist. Revisit if a synthesis binary producer ships.
  (Cross-ref `/GAPS.md`.)
- **GO-gate config vs spec §11** — the shipped example config diverges (paper 14 vs 30, fee 0.5 vs
  0.35, synth 100 vs 60-spec). Demo + `go_nogo` use the SPEC values; reconcile the example config in
  a later config pass. (Cross-ref `/GAPS.md`.)

## Follow-on milestones (post loop-close — each its own spec → plan → build)
- **G7 — `k_unc`** estimation-uncertainty sizing shrinkage (Baker–McHale / Chu–Wu–Swartz).
- **G8 — Edge Decay Watchdog** (config-named; needs G1–G6 as inputs first).

## Open during the loop (the loop appends here as it discovers gaps)
_(none yet)_

## Moved to WS2 (captain, 2026-06-19T14:49Z)
- **Unified PredictiveKind->BrierRule trait dispatch (D-4 refactor)** moved from WS1 slice 3 to WS2/G5 — it is a no-op until a 2nd ScoringRule (RPS/Log) exists; G5 is its natural home.

## Open during the loop (WS1.4, 2026-06-20T02:23Z)
- **Unbounded `pending_market_quotes` buffer when `snapshots_pool=None`** (runner.rs:216): the daemon gates the drain on `Some(pool)` (daemon.rs:3085), so in DST/no-persist mode the buffer fills each tick and is never drained — bounded only by short runs. Mirrors the gate-accepted `pending_fills` pattern (also drained only under `Some`); production `main.rs` always wires `Some`. No fix now; revisit if a long no-persist run is ever needed.

## Deferred (WS1.7, 2026-06-20T05:43Z)
- **Per-producer calibration PARAMS persistence** (keying the Platt/calibration fit by producer, not just the quality): runner.rs:757 persists producer=None. Slice 7 delivers per-producer QUALITY selection (the thesis payoff); per-producer PARAMS is a follow-on persist/schema change, deferred (YAGNI for the demo).

## WS1 boundary follow-ups (from slice-8b QA, 2026-06-20T10:49Z)
- **[Minor] daemon de-vig observation gap** — the daemon's `market_p = (bid+ask)/200.0` (daemon.rs:~5443) is asserted in the daemon_smoke test by a PARALLEL re-implementation, not by observing the daemon's threaded `synth_brier`/`market_baseline_brier`. A `/200.0` to `/100.0` mutation in the daemon would survive the suite (go_nogo + the baseline query ARE mutation-tested; only the daemon numeric wiring lacks an observation assertion). Close at the WS1-boundary hard verify: seed a synthesis trade so the synthesis `StrategyRecord` enters `recommendations`, or expose the computed values, and assert the expected de-vigged value.
- **[Minor] ledger.rs:2447-2455 test comment overclaim** — comment says the query test covers snapshot-skip, but that logic is daemon-side. Trim the comment.

## WS1 boundary critical-fix follow-ups (from boundary-Critical grading-station fix, 2026-06-20)
- **[Important] CLV per-event linkage for meteorologist beliefs** — `resolve_and_score_weather_beliefs` (daemon.rs:4846) calls `edges_repo.current_edges_for_event(&b.event_id)` to find market_ids for CLV. The meteorologist's `event_id = "weather:NYC:tmax:DATE#ge87"` does NOT exist in `market_event_edges` (those entries use the Aeolus `aeolus:knyc-...-ge87` namespace, auto-confirmed by `aeolus_bucket_match`). So `edges` is empty → CLV = None for ALL meteorologist beliefs. Fix options: (A) add a second edge row pointing the persona's event_id to the same market as the corresponding Aeolus event; (B) broaden the CLV lookup to walk from `belief.event_id` → `belief.provenance.analysis_id` → signals → market match; (C) drop the `event_id` filter in `snapshots_for_market_before` (repos.rs:905) and key only on `market_id` after finding edges via a cross-namespace lookup. The Brier half (the primary GO metric) resolves correctly; CLV None is fail-closed. Address in WS2 or G5 (requires schema/edge-table changes or a new cross-namespace lookup path). Cross-ref: `crates/fortuna-ledger/src/repos.rs:896` (`snapshots_for_market_before`), `crates/fortuna-live/src/daemon.rs:4846`.
- **[Minor] CLV_MIN_TOUCH_QTY / CLV_MAX_SPREAD_CENTS hardcoded constants** — `daemon.rs:4743-4744` hardcodes `CLV_MIN_TOUCH_QTY = 1` and `CLV_MAX_SPREAD_CENTS = 10`. These control the liquidity filter for CLV capture and should be promoted to `[cognition]` config (alongside the existing CLV-related config keys). No production impact today (defaults are reasonable); address in the config-cleanup pass.

## Live-persona-smoke findings (2026-06-20 — persona path proven end-to-end; hardening follow-ups)
_A gated live one-shot (`crates/fortuna-cognition/tests/persona_live_smoke.rs`, run with `FORTUNA_LIVE_PERSONA_SMOKE=1` + `ANTHROPIC_API_KEY` + `AEOLUS_API_TOKEN`) drove the REAL `run_due_personas` over today's live Aeolus KNYC envelope + live NWS AFD and produced the first genuine meteorologist finding. It exposed two charter bugs (BOTH FIXED in persona.md, kept at v3) and three follow-ups below._
- **[CLOSED 2026-06-20 — Hephaestus Slice B] Persona charter promotion v3→v4.** meteorologist is now **v4** (`config/personas/meteorologist/persona.md`) with a surgical sweep of the shipped-version assertions (persona.rs, persona_runner.rs:526, daemon_smoke.rs:5049, persona_e2e.rs:185; the version-mismatch safety gate rebased v4→v5, bite mutation-proven). NOTE — the original "ON CONFLICT keeps stale hash → HashMismatch" hypothesis was WRONG: the head query is `ORDER BY version DESC LIMIT 1` (`repos.rs:2179`, max-version-wins) and seeding inserts the new version `active`, so on operator deploy+reboot the head becomes v4 and `validate_against` passes with **no seeding/registry code change and no manual registry mutation** (activation = operator deploy, constitution-clean). Verified: full cognition + DB-backed ledger + daemon_smoke suites green.
- **[CLOSED 2026-06-20 — Hephaestus Slice A] `validate_findings` now recurses into nested objects + array items** (`persona_runner.rs`, `validate_against_object_schema`/`validate_sub_schema`) — structural-only (required + additionalProperties), config-driven, path-qualified (`thresholds[1]: ...`). Independently verified: 4 planted mutations killed; both shipped personas' valid findings still pass. _Minor (open, non-blocking): the pre-existing `unwrap_or(true)` absent-`additionalProperties`→allow branch (`persona_runner.rs:169`) has no test — unreachable with the shipped schemas (all use `additionalProperties:false`); add a synthetic-schema test if locking that contract is wanted._
- **[Minor] `SYNTH_MIND_TIMEOUT_SECS = 30` may be too short for the Opus persona tier.** The one-shot timed out at 30s on the first real call (Opus adaptive thinking + 16k max_tokens); 180s succeeded (real calls ran ~15–42s). In the daemon a slow persona call degrades to a counted "mind failed" defect (no crash) but could mean chronic no-findings under load. Consider a separate, longer persona-tier timeout (`daemon.rs:187`, `persona_mind_from_env` `daemon.rs:237`).
- **[Reference] The live persona one-shot harness is a forward-looking demo/diagnostic asset** — the repeatable "does the persona actually produce a loop-valid finding on live data" check. Reuse/extend it for other personas as they ship.

## TOP next-priority — surfaced by the Hephaestus final live gate (2026-06-20)
- **[Important — demo critical path] Persona findings ride in a NULLABLE `journal.body`; the model intermittently returns `journal:null` → no finding.** The final live gate (real Opus on the live NYC forecast) is INTERMITTENT: most runs produce a genuine loop-valid finding, but one returned `journal:null` → `persona_runner.rs:325` "persona produced no findings journal" → zero artifact (cost 10¢, fail-closed: no wrong belief, no capital). Root cause is upstream of Slice A's validator (which runs at :340): the persona path forces the synthesis `{beliefs,proposals,journal}` schema where `journal` is `anyOf:[null,{body}]` (`mind.rs:121,542`), so the charter can only *instruct* (not *guarantee*) journal.body — no charter wording closes the null-escape, and a charter tweak would force a gratuitous v5 bump. **Robust fix (the genuine next slice):** give personas a dedicated structured-output call — `call_priced_structured` (`mind.rs:665`, already used by discovery) with the findings/v2 schema as the *required* output (no journal indirection, no null-escape). This is a Mind-path + `run_persona_analysis` code change. On the milestone's critical path per spec §8 (persona live-smoke layer) / §7 (the worked example expects a reliable persona finding). Guardian-confirmed (2026-06-20) as the correct disposition; do NOT mask via retry.
