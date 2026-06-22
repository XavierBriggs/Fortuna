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

## WS4 W5: CLV-for-persona required a market-keyed snapshot read, not just the edge (2026-06-22)
The W5 brief diagnosed the missing link as "only the `market_event_edge` row" — insert the
persona_event_id → existing-Aeolus-market edge, and `current_edges_for_event` resolves so the
producer-agnostic CLV resolver scores the persona's `clv_bps`. That is necessary but NOT sufficient.
The CLV resolver also read its benchmark snapshots filtered by the BELIEF's own `event_id`
(`snapshots_for_market_before(market_id, b.event_id, …)`). But a market's cadence snapshots are
captured under exactly ONE `event_id` — the runner tracks a market under its CONFIRMED edge's event
(`MarketQuoteCapture::event_id` from `market_events`, populated only by confirmed edges), and the
`(market_id, at)` unique index (20260619000001) permits one row per timestamp. The W5 persona edge is
PROPOSED (so it is NOT in the confirmed/tradeable set — `confirmed_edges()` excludes it, no order risk),
so the shared market's snapshots stay tagged with the AEOLUS event_id. A persona belief on a different
event_id therefore found NO snapshots and would still resolve to `clv_bps = None` with the edge alone.
**Resolution (producer-neutral, no `if producer==` branch):** the benchmark mid is a property of the
MARKET's book, not of which event maps to it, so the resolver now reads
`SnapshotsRepo::snapshots_for_market_before_any_event(market_id, cutoff)` (market-keyed). For the
tracking producer (market↔event 1:1) this returns the IDENTICAL rows — zero behavior change, verified by
the unchanged `weather_resolve.rs` Aeolus tests; it ADDITIONALLY lets a co-mapped persona belief read the
same shared-market snapshot. **Honesty:** because the persona points at the SAME market, CLV is computed
from the SAME earliest fill + the SAME shared-market benchmark snapshot → the persona's `clv_bps` is
IDENTICAL to Aeolus's (market-level drift, NOT an independent per-producer confirmation; Brier is the
differentiator). Asserted as EQUALITY in `persona_clv.rs` + the chain-view contract documents it.
**Future tweak:** if a market is ever re-listed for a genuinely different event, a market-only read could
pull the prior event's snapshots; today market tickers are bracket-specific (date+threshold in the
ticker) so the `(station,date,threshold)` market is one physical market — safe. Revisit if market id
reuse across distinct events becomes possible.

## WS4 W6a (safety/correctness hardening, 2026-06-22)

### E4 dead-man — DROPPED as already-satisfied / config-only (bisect result)
The W6 brief flagged F8 ("dead-man ping FAILED: transport failure"). **Bisected — no defect:**
(1) `FORTUNA_DEADMAN_URL` is a REQUIRED env var (`fortuna-live/src/boot.rs:120`,
`required(env, "FORTUNA_DEADMAN_URL")`) — `validate_env` refuses to boot when it is missing, so the URL
is never unset at runtime. (2) The pinger is constructed once (`fortuna-live/src/main.rs:422`) inside a
long-lived `tokio::spawn` loop (`main.rs:429-442`) that NEVER drops it on error — `&mut pinger` persists
across every tick. (3) `daemon::deadman_tick` (`daemon.rs:6184`) on `Err` calls `on_failure` (logs to
stderr) and DOES NOT `record_ping`, so the next tick re-fires — retry-every-interval until the monitor
recovers, no silent backoff (correct-by-design per the module doc + spec Section 8). (4) Kalshi `WsDial`
(`fortuna-venues/src/kalshi/dial.rs:71`) has capped-exponential reconnect backoff (500ms→30s), wired.
So `dead-man ping FAILED` (`main.rs:438`) is the EXPECTED log when the configured monitor URL is
unreachable; the external monitor's own silence-page is the escalation of record (spec Section 8: "missed
pings alert via the monitor's own channel"). **No code change; no vacuous recovery test** (the component
is no-retry-by-design — a "recovers after transport failure" test has no recovery path to exercise). The
only operator action is a reachable `FORTUNA_DEADMAN_URL`.

### edge_id_base collision (DEMO-CRITICAL) — FIXED via disjoint persona prefix
`PersonasWiring.edge_id_base` (`main.rs:599`) and `DiscoveryWiring.edge_id_base` (`main.rs:678`) both
seed from the same `start_ms`, and both minted `01EDG{seq:021}` — so when discovery + personas co-run in
one `drive()` (the demo end-state) the first edge ids collided on the `market_event_edges` PK
(`insert_edge`, `repos.rs`, has no `ON CONFLICT`). Non-fatal (the persona insert error is caught as an Ops
alert and the loop continues — no money/I6 impact) BUT the persona's CLV was silently dropped from the
head-to-head. **Fix:** persona edges now mint `01EDP{seq:021}` (new `PERSONA_EDGE_ID_PREFIX` const +
`mint_edge_id` helper in `daemon.rs`), structurally disjoint from discovery's `01EDG` regardless of the
shared base. No production code filters edges by the `01EDG` prefix (verified by grep — all `01ED*`
literals elsewhere are test fixtures), so the prefix change is safe. Pinned by
`persona_and_discovery_edge_ids_are_disjoint_for_the_same_seq` (mutation-proven: reverting the persona
prefix to `01EDG` reds it). The boundary live gate (`ws4-live-smoke.sh`, W6b) still owes the assertion
that the head-to-head persona gets non-null CLV with discovery co-active.

### parse_linkage_fields byte-slice panic — FIXED (no-panic rule)
`parse_linkage_fields` (`fortuna-ops/src/rota.rs`) did `seg.len() >= 10 && seg[..10]…`, which PANICS when
a multibyte UTF-8 char straddles byte index 10 (linkage data is untrusted; `event_linkage` may carry any
bytes). Not reachable today (ASCII-only linkages) but it violated the no-panic-in-non-test rule. **Fix:**
`seg.get(..10).filter(is_iso_date_prefix)` — returns `None` on a non-char-boundary slice instead of
panicking, and `is_iso_date_prefix` STRENGTHENS the check from "10 chars + two hyphens" to a real
`dddd-dd-dd` shape (digits in every non-hyphen position). Pinned by
`multibyte_segment_at_byte_10_does_not_panic` + `multibyte_inside_date_shaped_segment_does_not_panic`
(both red on the pre-fix code) + an ASCII happy-path test.

### recalibrate provenance — RELABELED (gate math untouched; the preferred fix)
`LedgerEdgeProvider::recalibrate` (`fortuna-backtest/src/edge_provider.rs:338`) keys on the FLAT
`config_index` and applies a per-config TEMPERATURE scaling (τ = 0.7 + 0.3·index), but the GO surface
reported the decoded trial-grid coordinate (`window=…`, `method=…`) as `selected_config` — so
`method=None` read as "no recalibration applied" when a temperature transform actually was. The gate math
is VERIFIED-honest (G-PIT join, G-PARITY shared scorer, conjunctive Brier-primary decide) and was NOT
changed. **Preferred fix (relabel only):** `format_go_surface` (`fortuna-cli/src/backtest_cmd.rs`) now
frames the value as `selected_config: trial-grid[window=… method=… threshold=…]` and adds a
`recal_applied:` disclosure line stating the applied recal is a per-config temperature scaling and the
named knobs are ILLUSTRATIVE of the trial grid, not the applied transform. Real per-method dispatch
(actual Platt/Isotonic) was intentionally NOT implemented (it would change the family's OOS columns and
require re-deriving the verdict); the temperature family is a genuine one-parameter recalibration that
makes the configs differ. `validate_real_edges` / `validate_yields_honest_verdict` re-run GREEN (the
verdict is unchanged). Pinned by `go_surface_discloses_recal_is_a_temperature_index_not_the_named_knobs`.

## WS3 backtest — guardian boundary findings (2026-06-21, operator queue)
### G1. `fortuna validate` ships a placeholder edge-provider; purge/embargo unreachable in production (Important)
The S7 `run_validate` `EdgeProvider` (`crates/fortuna-cli/src/backtest_cmd.rs:173-178`) returns empty
OOS edge series for every config, so `fortuna validate` can only emit `GoDecision::Insufficient` on a
fresh ledger — it never computes a real GO/NO-GO from replayed history. `run_sweep`
(`crates/fortuna-backtest/src/sweep.rs:332-336`) hard-codes `no_windows` + `Duration::zero()` into both
`pbo()` calls, so the purged+embargoed CSCV (the research's "#1 lie-prevention", mandatory for Aeolus's
overlapping same-station-day labels) has **NO reachable production code path** — it is implemented and
unit-proven (`purged_cscv_bites_on_known_overlap`) but never wired. The replay path (scored output) and
the sweep path (per-slice OOS edge series) are disconnected — no seam feeds `ReplayHarness::replay`'s
scorecards back into the sweep matrix.
**Why not blocking (guardian PASS):** the deflation MATH is honest and bites (guardian mutation-verified);
the `backtest` command (seed the real track record) IS fully wired to the real read-only Aeolus archive +
ledger; and the empty-edge `validate` path is FAIL-SAFE (`decide` guards `effective_n<30 || n_logits==0
→ Insufficient`, `scorecard.rs:362`) so it can NEVER emit a false GO. Plan S7 only required validate to
run-sweep→write→print.
**Unblock (WS4 / follow-on):** wire a per-slice `EdgeProvider` that reads replayed scorecards from the
ledger and supplies real `LabelWindow`s (so purge/embargo actually runs); until then `fortuna validate`
on real data returns `Insufficient` by construction — NOT a tested-on-real-data verdict. (Corrects the
plan's "No placeholders" self-review line for the production validate path.)

### G2. Decoupling + scoring-purity not enforced by an executable test (Minor)
The fortuna-backtest source-literal grep and the fortuna-scoring no-new-dep assertion are enforced only
by the boundary gate (`.hephaestus/ws3.gates` lines 12/14/16) and per-slice shell greps — not by a
permanent `#[test]` in the corpus. `i_decoupling_spine.rs` scans fortuna-gates/exec/state but NOT
fortuna-backtest/scoring. Currently satisfied (verified at the boundary). **Unblock:** add a `#[test]`
that greps `fortuna-backtest/src` for source literals + asserts `fortuna-scoring`'s Cargo.toml dep set,
and include it in the boundary battery.

## Disputed invariant tests
### C5 BookAge gate vs i1_universal_gate hardcoded check-count (2026-06-18, Phase C)
Task C5 (book-freshness gate) adds an 11th gate check (BookAge) to `GateCheck::ALL`. The i1_universal_gate invariant test hardcodes the count `assert_eq!(out.records.len(), 10)` (2 sites: i1_universal_gate + i1_prop_all_orders_carry_gate_verdicts). Adding ANY gate check makes that `10` wrong. The C5 subagent changed `10` → `GateCheck::ALL.len()` (a self-adjusting STRENGTHENING) — but that MODIFIES a protected invariant assertion, which the constitution forbids without operator review. **RESOLVED 2026-06-18 (operator-approved, see chat):** chose the SEPARATE BookAge check (cleaner: single-responsibility + distinct `gate_rejections{check="book_age"}` telemetry + explicit ordering before price-sanity + spec-faithful). The i1 count update `10 → GateCheck::ALL.len()` is a genuine STRENGTHENING (verifies EVERY check produces a verdict regardless of count, not a fixed N) and is operator-blessed. The change was re-applied (cherry-pick of a9140c0). **Protected-invariant baseline re-based past this commit:** future `check-protected-invariants.sh` runs in this session compare against the post-C5 commit so this approved change is grandfathered while any NEW invariant modification is still caught. The hardcoded-`10` brittleness is the root cause; using `GateCheck::ALL.len()` makes i1 self-adjusting for future checks.
