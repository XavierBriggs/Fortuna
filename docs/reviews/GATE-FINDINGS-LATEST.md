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

(1) M3 rearm notices — CLI "resumes only on restart" text + a ROTA health
surface for halt-vs-running divergence. (2) T4.2 buildable-now — WS dial
(redial tests USE the ledgered reset/502 venue evidence in
fixtures/kalshi/README.md; no live socket in tests), book-driven
PaperVenue replay (trade-through is fixture-blocked — ledger it, NEVER
fabricate a trade frame), the 27-item clearance record for operator
signature, kill-switch Kalshi plug (I4 deps absolute), Slack Socket
listener (mock transport; live needs operator token). (3) T4.5 deferred
panels + the re-scoped §5 money model + audit-recents. Accommodate track
D's one flagged drive() seam as a neighbor's commit, do not rewrite it.

## TRACK C — perps re-merge package (HIGHEST-VALUE; code exists, gate-clean)

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
