# GATE FINDINGS — latest (verifier-owned; every track reads this at priority (a))

State as of 2026-06-13, main @ e85f92c. Main is GREEN (fmt/clippy/
workspace-tests/run-dst all clean per completion-audit-2026-06-13.md).
A BLOCK naming your track preempts your queue. This file is the single
coordination surface; the verifier rewrites it — tracks ACT on it and
ledger their responses in GAPS, never edit this file.

## CAMPAIGN STATE (completion-audit-2026-06-13.md is authoritative)

- Phases 0–3 + T4.1 daemon (SOAK: GO) + T4.4 CLI + T4.3 ROTA (R12 PASSED):
  DONE, gated, on main.
- Docs set landed (3b52bf0); docs gate BLOCK -> ACCEPT (re-gate addendum
  in 2026-06-12-docs-gate.md; pg_dump fix executed clean this session).
- BUILD_PLAN T4.5 entry restored + Phase-5 EXIT written (e85f92c) — both
  had been lost to merge-revert churn.

## TRACK A — completion campaign (queue: docs/design/track-a-completion-queue.md)

M3 DONE (certified ACCEPT, m3-rearm-gate-2026-06-13.md — I2 no-auto-resume
verified, both surfaces, mutation-proven tests). NOW: (2) T4.2 buildable-now — WS dial SLICES 1-2 CERTIFIED (t42-wsdial-gate-2026-06-13.md, ACCEPT 0 findings; resubscribe+gap-detect mutation-proven, no live socket). NEXT dial slice: async transport + signed handshake + keep-alive + the redial loop tying WsDial->pump_session (Clock-injection check re-applies). Then book replay
(redial tests USE the ledgered reset/502 venue evidence in
fixtures/kalshi/README.md; no live socket in tests), book-driven
PaperVenue replay (trade-through is fixture-blocked — ledger it, NEVER
fabricate a trade frame), the 27-item clearance record for operator
signature, kill-switch Kalshi plug (I4 deps absolute), Slack Socket
listener (mock transport; live needs operator token). (3) T4.5 deferred
panels + the re-scoped §5 money model + audit-recents. Accommodate track
D's one flagged drive() seam as a neighbor's commit, do not rewrite it.

## TRACK C — PACKAGE DONE; stopped clean; FULL RE-GATE IN FLIGHT (perps-remerge-gate-2026-06-13.md)

Branch: you Reapplied the reverted merge (d81ab6c) — tip = main + the full
perps tranche as forward history. REBASE RULE: never plain-rebase onto
main while revert 19b3888 is in history (drops your commits as
duplicate-applied) — use `git rebase --reapply-cherry-picks main` or don't
rebase until re-merge. REMAINING: (1) fix kinetics test
`place_maps_gated_order_to_the_recorded_create_request` — derive the
expectation THROUGH the derivation path, not a pinned UUID (read the exec
adjudication c25b368 on main). (2) the 2x leverage cap
(operator-decisions-2026-06-12.md item 4: [perp] max_leverage config +
gate min(config, venue curve) + boundary pin 2.01x-refused/1.99x-passes +
ASSUMPTIONS note that loosening is an I7-review). (3) full re-gate at
10000 -> re-merge request. RE-MERGE (verifier-owned): post-merge
integration check MUST show the previously-failing kinetics test green on
merged main. Standing signatures (waive batch 5 + F1) remain valid.

## T5.B7 / T5.B8 — ORPHANED, post-re-merge (ledgered 2026-06-13 so they don't vanish)

Track C correctly STOPPED rather than grab these (not in its ownership) — the
loop discipline working. They are genuinely BLOCKED on the perps re-merge
landing on main (both extend the merged plane). After the re-merge:
- T5.B7 rung-0 strategies (perp_event_basis Sim, funding_forecast zero-capital,
  funding_carry DATA-ONLY) under the FEE-TRAP RULE (edge floors at assumed
  post-promo fees; promo-$0 never justifies GO; I7 unchanged). Cross-cutting:
  strategy plugins + the merged perp gates/types.
- T5.B8 ops: kill-switch perps flatten (reduce_only IOC + cancel-all — SEPARATE
  killswitch binary, I4 deps absolute), margin/funding telemetry, funding-regime
  ROTA panel.
OWNER PLAN: a RESTARTED track C, scoped to "extend the now-merged perps plane"
(coherent ownership once it's on main) — or a fresh track. Operator spins it up
AFTER the re-merge gate ACCEPTs and the merge lands. Phase-5 EXIT (BUILD_PLAN)
is not met until B7+B8 land.

## TRACK D — BLOCKED: Critical SSRF on the injection surface (track-d-nws-gate-2026-06-13.md)

DO-NOT-MERGE. The gate caught a real vulnerability BEFORE it touched main —
the discipline working on the exact surface flagged highest-risk. Track D
fixes forward at priority (a); the D1-D4 unit does not merge until re-gated.

[CRITICAL — SSRF fail-open, reproduced end-to-end] fetch.rs host-pin uses a
hand-rolled `host_of_https` (fetch.rs:103-122) that parses
`https://evil.example.com\@api.weather.gov/x` as host api.weather.gov (PASSES
the pin) while reqwest's WHATWG url crate resolves it to evil.example.com and
CONNECTS there (fetch.rs:304-316 redirect follow). A malicious Location header
defeats host-pinning — the entire SSRF control. PARSER-DIFFERENTIAL is the root
cause. FIX (root-cause, NOT a backslash blocklist — band-aids on parser
differentials are whack-a-mole): the pin check MUST use the SAME parser as the
HTTP client — `url::Url::parse()` then compare `.host_str()` to the pin, so the
authorization decision and the connection resolve the host IDENTICALLY. Delete
host_of_https. Re-validate EVERY redirect hop through that one canonical parser
(or disable redirect-follow and handle Location explicitly through it).
Regression test: the exact backslash-authority payload + a redirect-to-unpinned
Location, both asserting REFUSAL through the public FetchClient::fetch path.

[MAJOR] Layer-1 per-item structural validation gap + the validator is unwired
(nws.rs:122-150, validate.rs): a shape-drifted NWS item is not refused
per-item. Wire the validator into the ingest path; a non-conforming payload
from the pinned host must refuse-and-quarantine (Layer 1).

Otherwise gate-clean: fmt/clippy/47-of-47 sources tests, no test weakening, no
f64, no wall-time, no unwrap/panic in the source path, protected crate
untouched. The BLOCK rests solely on the SSRF (an explanation cannot waive a
reproduced Critical).

## TRACK D — news-aggregation Phase A (queue: implementer-loop-track-d.md)

fortuna-sources crate, FetchClient, four v1 adapters, registry admission
records — four-layer trust framework Layers 0–2 binding, fixtures-first
under fixtures/sources/, NO model in the ingestion path, one flagged
minimal drive() seam. Gate rubric: spec 5.11 untrusted-data doctrine;
news payloads are the canonical injection surface — expect
doctored-fixture mutation checks at every gate.

## TRACK B — DONE (stopped clean, fully merged). No queue.

## OPERATOR QUEUE (none block the tracks)

1. Soak start — runbooks/soak-start.md (starts the 7-day clock).
2. T4.3 tick decision — accept the money view as shipped (sim-only,
   honest nulls, R6-valid) or hold for the mark-loop source (re-scoped
   into T4.5 either way).
3. Trade-frame recapture — busy market, 180–300s × N (the 600s attempt
   2026-06-13 failed venue-side; evidence ledgered).
4. Slack app token; 5. keys rotation + purge finalization (before any
   push); 6. post-soak/post-fees: Kinetics PROD parity sweep, the I7
   promotion ladder.

---
Historical gate record: docs/reviews/*.md. The verification arc
(17 BLOCK / 14 ACCEPT-WITH-GAPS / 3 ACCEPT pre-campaign) is in
docs/verification.md.
