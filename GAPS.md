# GAPS.md — honesty ledger (open items only)

Open items the implementation defers, lacks, or needs from the operator, each with exact
unblock steps. The full RESOLVED history (5858-line ledger) was archived 2026-06-18 in the
Phase B consolidation → **`docs/archive/gaps-history.md`**.

> **Loop-Close & Provable Demo milestone:** that work tracks its own forward/deferred items in
> **`docs/superpowers/loop-close-gaps.md`** (and operator actions in `docs/superpowers/loop-close-operator.md`).
> This file (`/GAPS.md`) remains the repo-wide constitutional ledger.

## Authoritative open-items source (2026-06-18 ground-truth audit)

The audit is now the canonical "what's open + readiness" source:
- `docs/audit/2026-06-18/AUDIT.md` — risk register (P0–P3) + Demo-Paper-Ready Readiness Scorecard.
- `docs/audit/2026-06-18/MVP-CLOSURE-PLAN.md` — verified gaps + phased close-the-loop plan (Phase C).

## Close-the-loop wiring (Phase C — blocks demo-paper-ready)
- **F0** Calibration fit but never persisted → model arm never sizes. Persist fitted Platt in `stage="paper"`.
- **F1** Settlement in-memory only (`settlement_entries`=0) → no realized PnL. Wire `SettlementsRepo::insert_entry`.
- Fills not persisted (`fills`=0); live bus recording dropped at shutdown → no live replay.
- No `fortuna start paper-demo` CLI; ROTA Health omits `execution_mode`/`order_mutation_enabled`.
- Personas inert: charter never injected (`main.rs:474` uses synthesis charter) + registry empty + `[personas]` OFF.
- World-forward unscoreable trap (`discovery.rs:689` exact-match vs prose `resolution_source`).
- Market-back never dedups events (`daemon.rs:2010` empty `existing_events`) — **CLOSED by C4** (populated `existing_by_market` + `existing_events`; persist-loop guard); no DB unique constraint on events (deferred — append-only table, guard is sufficient).

## WS2 proof-layer (2026-06-21) — follow-ons
- **Macro-economist persona not yet live-smoke-validated.** Only the meteorologist has a gated live one-shot (`crates/fortuna-cognition/tests/persona_live_smoke.rs`). The macro-economist shares the structured `decide_structured` path (the S7 sanitizer + harness range-enforcement cover it; its charter is NOT stale — already "emit ONLY the structured finding"). Unblock: add a gated macro-economist live one-shot, or run one against live data before relying on its findings. A validation-coverage gap, not a code gap.
- **Meteorologist charter v5 activation is an operator action (I7).** S9 promoted the charter v4→v5 (completes S1's structured-output contract + a non-empty-threshold-ladder instruction; the live smoke went 0/3→3/3). The daemon registry seeds ON CONFLICT DO NOTHING and the deliberate-promotion gate refuses an unmatched edit, so to ACTIVATE v5 the operator inserts + activates the v5 row; until then a running daemon keeps the prior active head.

## Operator actions (runtime; not code — daemon was live during Phase B)
- Stale demo DBs `fortuna_demo_paper_green_2026061704*` (×4) + `fortuna_demo_paper_live` are abandoned snapshots —
  drop manually when convenient (`DROP DATABASE` is irreversible; left for the operator). The LIVE DB is `fortuna_demo`.
- `data/runtime/current-demo-db-url` is STALE (points at `green_044732`). Proper fix: daemon writes the live
  `DATABASE_URL` on boot. Until then ignore the pointer; the live DB is `fortuna_demo`.
- `GRANT INSERT ON funding_rates_historical` must be applied on a fresh demo DB (add to demo-launch runbook).

## Branch follow-ups (Phase B; all archive-tagged — recover via `git checkout archive/<branch>`)
- **`track-b`** (worktree `/Users/xavierbriggs/fortuna-wt-b`): **40 UNCOMMITTED files** — review + commit/discard.
  NOT touched in Phase B (uncommitted work is not in any tag).
- `track-d`, `track-e-docs-freshen`: stranded doc-only corrections (GAPS/BUILD_PLAN freshening), superseded by this
  prune; kept for review — delete when satisfied.

## fortuna-ledger domain-coupled query methods (A7 known gap) — RESOLVED WS1.2
**Resolved 2026-06-19 by task WS1.2 (feature/paper-on-live-data).**
`open_aeolus_weather_due` renamed to `open_weather_bracket_due`; the
`provenance->>'model_id' = 'aeolus'` WHERE literal replaced with
grading-keys-present (`nws_station_id IS NOT NULL AND variable IS NOT NULL AND
target_date IS NOT NULL`). The `i_decoupling_spine.rs` known-gap comment still
references the old name (comments only, never assertions — not modified to comply
with the protected-invariants rule).

## Deferred refactors (Phase B roadmap; P2 legibility — no behavior change, test-gated when done)
- File splits: `daemon.rs` (4854L), `repos.rs` (2479L), `rota.rs` (2227L); a `DriveContext` for `drive()`'s 20-param
  signature. (AUDIT.md §12 / area-4)
- Dual mode model (`[runtime]` vs `[daemon]`): coherent + cross-validated today; collapse-to-one-axis deferred. (area-2)
- `AnthropicVetoMind`: `StubVetoMind::allow_all` inert stub. (area-2)

## C2 follow-on: source-registry domain_tag specificity (2026-06-18, Phase C)
`SourceRegistry::resolve` (signals.rs) fuzzy-maps prose `resolution_source` → a registry id by token-subset over the source_id and its `domain_tags`. A **single-token** `domain_tag` that is a common English word (e.g. `["weather"]`, `["press"]`) will match ANY prose containing that word — over-eager (false scoreability against the wrong source). Operator-gated: the registry is curated, so use multi-word phrases ("federal reserve") or acronyms that also appear in the source_id ("fomc" on `rss_fomc_*`) as domain_tags when specificity matters. A code-level mitigation (≥2-token domain_tags, or single-token-must-appear-in-source_id) was deliberately NOT applied — it trades this false-positive for false-negatives on legit single-token acronyms, and the real registry contents aren't known. Revisit if discovered events resolve to wrong sources in the soak.

## C3 follow-on: empty category_allowlist asymmetry (2026-06-18, Phase C)
The market-back prefilter (discovery.rs:122) treats an empty `[discovery] category_allowlist` as reject-ALL (`!contains` → fail-closed); the world-forward gate (C3, discovery.rs:727) treats empty as bypass (fail-open, "no vocab configured = no filter"). Same config field, opposite default — only bites an UNCONFIGURED deployment. Mitigation: the demo config MUST set `category_allowlist` (E2) so both paths use the real vocab. Reconcile the two defaults (pick one semantic for empty) in a future discovery-config pass; not changed now to avoid touching market-back's existing tests/behavior.

## C5 invariants: i1 test count update requires protected-ref re-baseline (2026-06-18)
Task C5 (book-freshness gate) added `GateCheck::BookAge` as check 11 to `GateCheck::ALL`.
This required updating `crates/fortuna-invariants/tests/i1_universal_gate.rs` to replace the
literal `10` with `GateCheck::ALL.len()` (two occurrences) and import `GateCheck`. This is a
STRENGTHENING change (the assertion now asserts 11 checks, not 10) but `scripts/check-protected-invariants.sh c2c68ec` flags it because it detects removed lines. **Operator action required**: re-baseline the protected ref by running `scripts/check-protected-invariants.sh <new-commit>` after merging C5. The failing-script is a count-update, NOT a logic-weakening.

## C4 part-2: mech_structural bracket ladder still uses config (deferred — 2026-06-18, Phase C)
The `mech_structural` strategy (and the demo config's `[kalshi].bracket_sets`) still sources
its bracket-market ladders from static config, which may hold expired or stale dated tickers
(e.g. `JUN16` brackets after 2026-06-16). The idempotent market-back event-matching fix (C4)
addresses event deduplication but does NOT refresh the bracket ladder live. Unblock steps:
resolve the live day-set from the live catalog (the Kalshi venue `market_views()`) using the
rolling series prefix (e.g. `KXHIGHNY`) instead of dated ticker literals in config; OR ensure
the demo config always uses rolling/date-agnostic bracket identifiers that remain valid across
demo runs. Handle in E2/E5 (demo config hardening + live catalog wiring for the mech arm).

## GO-gate config diverges from spec §11 thresholds (2026-06-19, found by scoring-doc V&V)
The shipped `config/fortuna.example.toml` sets two GO/NO-GO thresholds that diverge from spec
Section 11 (spec.md:384), surfaced while validating the Scoring & Validation Architecture doc:
- **`min_paper_days_mechanical = 14`** (fortuna.example.toml:92) vs spec §11 **`≥ 30 trading days for mechanical`**. Config is HALF the spec bar — a mechanical strategy could clear the paper gate in 2 weeks instead of the spec-mandated ~6. `crates/fortuna-cognition/tests/review.rs:123` uses 30 (the spec value), so the code authors knew the bar; the example config diverges. Consumed at review.rs:147/214.
- **`max_fee_pnl_ratio = 0.5`** (fortuna.example.toml:94) vs spec §11 **`fee/PnL ratio < 0.35`**. Config permits fees up to 50% of PnL before NO-GO; spec caps at 35%. Consumed at review.rs:280.
Both diverge in the *less strict* direction (weaker gate than spec). NOT changed yet — the demo config (E2) and a spec/config reconciliation pass should decide whether to (a) tighten the example config to the spec values (30 / 0.35), or (b) ratify the looser demo values with an explicit rationale. Until reconciled, the §8 demo scorecard cites the SPEC values (30 / 0.35) as authoritative, not the shipped config. NB: `min_resolved_beliefs_synthesis = 100` (config) is *stricter* than spec's `≥ 60`, so it is conservative and needs no action.

## Persona/synthesis binary beliefs are never resolved or scored (2026-06-19, found by scoring-doc V&V)
There are exactly TWO live belief resolvers: `resolve_and_score_weather_beliefs` (daemon.rs:4637,
previously Aeolus-only — queue renamed to `open_weather_bracket_due` by WS1.2, now
producer-agnostic; an `aeolus:` event-id prefix guard still gates scoring in daemon.rs:4723)
and `resolve_and_score_funding_beliefs`
(daemon.rs:4378, funding scalar). Meteorologist/persona binary beliefs (event_id
`{region_key}#{suffix}`, provenance `persona_id`; persona_beliefs.rs) and synthesis binary beliefs
match NEITHER filter, so they are never resolved or scored in production. `resolved_persona_stats`
(repos.rs:1305) selects `WHERE outcome IS NOT NULL`, but nothing live sets that outcome (only
persona_e2e.rs does, by hand). **Consequence:** the Aeolus-vs-meteorologist head-to-head (the D3
thesis payoff, spec/demo §0) accrues ZERO scored persona data today. Unblock = D-4/G2 in the
scoring architecture doc: build resolution for persona + synthesis binary beliefs (not just unify
the two existing forks). This is a hard prerequisite for the demo's head-to-head, scheduled P1.

## WS1 slice 5: LiquidityPolicy constants not in config (2026-06-19)
`resolve_and_score_weather_beliefs` uses hardcoded `CLV_MIN_TOUCH_QTY = 1` and
`CLV_MAX_SPREAD_CENTS = 10` to construct the `LiquidityPolicy` for CLV computation.
These are sensible defaults (1-contract touch minimum; ≤10c spread is tight)
but should be promoted to operator config (e.g., `[clv]` table in `fortuna.toml`)
once field data from the live paper soak validates the right thresholds.
No existing config key covers this; the constants are documented at the site.
Unblock: add `[clv]` TOML section, update `FortunaConfig`, thread to resolver.

## Disputed invariant tests
### C5 BookAge gate vs i1_universal_gate hardcoded check-count (2026-06-18, Phase C)
Task C5 (book-freshness gate) adds an 11th gate check (BookAge) to `GateCheck::ALL`. The i1_universal_gate invariant test hardcodes the count `assert_eq!(out.records.len(), 10)` (2 sites: i1_universal_gate + i1_prop_all_orders_carry_gate_verdicts). Adding ANY gate check makes that `10` wrong. The C5 subagent changed `10` → `GateCheck::ALL.len()` (a self-adjusting STRENGTHENING) — but that MODIFIES a protected invariant assertion, which the constitution forbids without operator review. **RESOLVED 2026-06-18 (operator-approved, see chat):** chose the SEPARATE BookAge check (cleaner: single-responsibility + distinct `gate_rejections{check="book_age"}` telemetry + explicit ordering before price-sanity + spec-faithful). The i1 count update `10 → GateCheck::ALL.len()` is a genuine STRENGTHENING (verifies EVERY check produces a verdict regardless of count, not a fixed N) and is operator-blessed. The change was re-applied (cherry-pick of a9140c0). **Protected-invariant baseline re-based past this commit:** future `check-protected-invariants.sh` runs in this session compare against the post-C5 commit so this approved change is grandfathered while any NEW invariant modification is still caught. The hardcoded-`10` brittleness is the root cause; using `GateCheck::ALL.len()` makes i1 self-adjusting for future checks.
