# Phase-1 Review Area g — Observability (FORTUNA)

Scope: audit-log implementation, decision reconstructability (I5), append-only
enforcement, replay machinery, Slack/metrics. Authoritative spec: docs/spec.md
§5.5, §5.7, §8, invariant I5.

All claims cite `path:line`. As-built vs as-intended noted where they diverge.
READ-ONLY review; no code/docs were modified.

---

## 1. What is actually logged

### 1.1 The audit table + writer

- Table: `audit (audit_id, at, kind, actor, ref_id, payload JSONB)`, PK
  `(audit_id, at)`, monthly RANGE partition with a DEFAULT partition.
  Migration: `crates/fortuna-ledger/migrations/20260609000001_initial.sql:104-118`.
- Append-only trigger on the table: `audit_append_only BEFORE UPDATE OR DELETE`
  → `fortuna_refuse_mutation()` (initial.sql:117-118, fn at :14-18).
- Writer (INSERT-only): `crates/fortuna-ledger/src/audit.rs:54-79`
  (`AuditWriter::append(kind, actor, ref_id, payload)`). Read paths:
  `latest_at()` (:87-95, A8 crash-tell), `recent(kind, limit)` (:98-125).
- There is **no typed enum** of audit kinds. `kind` is a free `&str` at every
  call site. The "record types" below are the set of literal strings actually
  passed — this is itself a finding (no compile-time guarantee the vocabulary is
  closed; a typo silently creates a new kind).

### 1.2 Two audit sinks, one table

- **Ledger `AuditWriter`** (async, 6-arg incl. `actor`): used by the CLI and the
  daemon's bridge.
- **Runner `AuditSink`** (sync, 3-arg: `kind, ref_id, payload`):
  `crates/fortuna-runner/src/runner.rs:872-885`. The runner never sets `actor`.
- Bridge from sync→async: `PgAuditSink` in
  `crates/fortuna-live/src/audit_bridge.rs:119-145`. It hardcodes `actor = None`
  on the forwarded call (`audit_bridge.rs:103`). **Consequence: every audit row
  written through the daemon/runner has `actor = NULL`.** Only CLI-issued rows
  (`lifecycle`, `halt`) carry an actor (operator / `$USER`).
- Fail-closed I5 contract: a failed audit write → `RunnerError::AuditFailed`
  (`audit_bridge.rs:134-143`) → runner sets `HaltScope::Global`
  (`runner.rs:876-884`, `audit_dead` latch). Asserted in
  `crates/fortuna-invariants/tests/i5_audit_append_only.rs:168-188`.

### 1.3 Audit record types present in code (kind strings)

| kind | where written (path:line) | actor set? | ref_id | notes |
|---|---|---|---|---|
| `lifecycle` | cli/src/main.rs:628, :825 | yes (`$USER`) | None | start/stop; best-effort (skipped if no DATABASE_URL) |
| `halt` | cli/src/main.rs:1064, :1078; runner.rs:707, :1718, :1755 | CLI: operator; runner: None | None | set / rearm / drawdown / runaway |
| `alert` | runner.rs:817 (`apply_external_alert`) | No | None | spec-8 "every alert is also an audit row"; daemon routes all Slack here |
| `daemon_shutdown` | runner.rs:853 | No | None | graceful-shutdown marker |
| `cognition` | runner.rs:1040 | No | event_id | degrade records (budget_exhausted, model_proposals_discarded, etc.) |
| `proposal` | runner.rs:1099 | No | **None** | carries `manifest_hash`, `strategy`, `thesis` — the decision-provenance row |
| `sizing` | runner.rs:1160 | No | None | kelly/haircut inputs |
| `gate_decision` | runner.rs:1176 (unsized), :1229 (per-check) | No | intent_id (per-check); None (unsized) | serializes `GateCheckRecord` |
| `order` | runner.rs:1076 (ttl_cancel), :1336/:1352 (reject/unknown), :1780/:1792/:1829/:1854 | No | intent_id | see §2: successful Acked submit does NOT write an `order` row |
| `veto_decision` | runner.rs:1403, :1453, :1482 | No | varies | model veto (reduce-only) |
| `veto_counterfactual` | runner.rs:1938 | No | market | |
| `veto_abandoned` | runner.rs:1959 | No | varies | |
| `fill` | runner.rs:1626 | No | fill_id | serializes the full `Fill` |
| `settlement`, `settlement_duplicate`, `settlement_reversal` | runner.rs:2079/2109/2177/2204/2226/2256/2289/3131 | No | varies | |
| `discrepancy` | runner.rs:2342 | No | varies | spec 5.13 |
| `watchdog` | runner.rs:2425, :2444, :2542 | No | market | dispute-freeze / overdue / terminal |

Notes on hypothesized kinds:
- **`config_change` — ABSENT.** No `config_change` audit kind exists anywhere.
  Config is loaded at boot (TOML); there is no runtime config-change audit row.
  Spec I5 lists "config change" as one of the things the audit log must capture.
  This is an as-intended-but-not-as-built gap (config is effectively static and
  changes require restart, but the row type the spec names does not exist).
- **`killswitch_test` — ABSENT from the Postgres audit by design.** The kill
  switch (`fortuna-killswitch`, I4) must not depend on Postgres, so it writes a
  **JSONL journal**, not the `audit` table: `flatten_*`/`freeze` events in
  `crates/fortuna-killswitch/src/lib.rs:450-751`, self-test entrypoint in
  `crates/fortuna-killswitch/src/main.rs:76`. The I4 revocation sentinel is a
  file (`run_loop.rs` `KILLSWITCH_REVOKED`). So kill-switch activity IS logged,
  but in a separate out-of-band store, not the unified audit log.

---

## 2. Decision reconstruction chain (end-to-end)

Spec §5.5/§5.7: a belief carries provenance `{model_id, prompt_hash,
context_manifest_hash, cost_cents}`; the context assembler emits a manifest into
the trail; gate decisions log verdict+reason; orders/fills mirror execution.
Walk each link:

| # | Link | Mechanism | path:line | Present? |
|---|---|---|---|---|
| A | Context manifest computed | `assemble_context` builds `ContextManifest` (items + content hashes + exclusion counts) and SHA-256 `manifest_hash` | cognition/src/context.rs:136-238 | YES |
| B | Manifest hash carried out of the cycle | `CycleOutcome.manifest_hash` (decision) / `RunOutcome.manifest_hash` (persona) | cognition/src/cycle.rs:728; persona_runner.rs:354-355 | YES |
| C | Belief provenance stamped by harness | LLM `Mind` stamps `{model_id, context_manifest_hash, cost_cents}` | cognition/src/mind.rs:691-696 | PARTIAL — **`prompt_hash` is NOT stamped** (spec.md:181 requires it) |
| D | Provenance persisted to `beliefs.provenance` JSONB | `BeliefsRepo::insert` writes `&draft.provenance` | live/src/daemon.rs:4320-4331; ledger/src/repos.rs:1219-1235 | YES |
| E | Proposal → audit with manifest_hash | `proposal` audit row carries `manifest_hash`, `strategy`, `thesis` | runner.rs:1098-1108 | YES (but `ref_id = None` — see break #1) |
| F | Gate decision logged (verdict+reason) | `GateCheckRecord {check, verdict, reason, at, intent_id, client_order_id}` serialized per check | gates/src/pipeline.rs:127-135; runner.rs:1228-1232 | YES (ref_id = intent_id) |
| G | Order mirrored | rejects/unknowns/ttl audited; full lifecycle in `intent_events` durable journal | runner.rs:1335-1356; ledger/src/intent_journal.rs:41-? | PARTIAL — **successful Acked submit writes NO `order` audit row** (runner.rs:1321-1332); state lives only in `intent_events` |
| H | Fill mirrored | `fill` audit row (full `Fill`) + persisted to `fills` table | runner.rs:1625-1629; daemon.rs:2883-2913; repos.rs:38-67 | YES (fills-table persist is opt-in + alert-on-fail; audit row is on the halt-path) |

### Chain breaks / weaknesses

1. **No belief_id ↔ proposal/gate join key in the audit log.** The `proposal`
   row carries `manifest_hash` + `thesis` but `ref_id = None` (runner.rs:1100);
   gate/order/fill rows are keyed by `intent_id`/`fill_id`. The synthesis
   `thesis` string embeds the belief id as free text
   (`synthesis.rs:293-299` → `"synthesis: belief {id} ..."`), and the belief
   carries `context_manifest_hash`. So a human CAN stitch
   belief → manifest_hash → proposal row, and proposal → intent → gate/order/fill
   by reading payload fields, but **there is no structured foreign key** linking a
   persisted belief row to its proposal/gate/order audit rows. Reconstruction is
   manifest-hash + thesis-string correlation, not a typed join.

2. **`prompt_hash` is never recorded** (link C). Spec.md:181 mandates
   provenance `{model_id, prompt_hash, context_manifest_hash, cost_cents}`. The
   as-built stamp (mind.rs:692-696) omits `prompt_hash`. The rendered prompt is
   not hashed or stored anywhere (`grep prompt_hash` → only a test fixture
   `cognition/tests/beliefs.rs:36` and the spec line). The `context_manifest_hash`
   covers context-item identity+content, but the system/instruction prompt and
   the exact rendered string are NOT hash-pinned. A model swap or prompt-template
   edit is not detectable from the trail. **This is the most material chain gap
   for exact replay.**

3. **Successful order submissions are not individually audited** (link G). Only
   rejects/unknowns/ttl-cancels produce an `order` audit row; an `Acked` submit
   only bumps counters (runner.rs:1321-1332). The authoritative record of a
   successful order is the `intent_events` durable journal
   (intent_journal.rs, INSERT-only, trigger-protected) — a SEPARATE log from
   `audit`. So "orders mirror execution" is true, but the mirror is the intent
   journal, not the audit table; an audit-log-only reader misses successful
   submits.

### Verdict on replayability — see §4 and Summary.

---

## 3. Append-only enforcement

### 3.1 DB triggers (the authority)

- Generic refuse fn: `fortuna_refuse_mutation()` —
  `migrations/20260609000001_initial.sql:14-18` (raises on any UPDATE/DELETE).
  Attached to: `market_event_edges` (:55), `audit` (:117), `signals` (:133),
  `intent_events` (:155), `fills` (:181), `market_snapshots` (:200),
  `price_snapshots` (:218), `settlement_entries` (:234), `discrepancies` (:243),
  `discrepancy_resolutions` (:255), `journal` (:266), `lessons` (:278),
  `calibration_params` (:294), `reservation_events` (:307), `halt_events` (:319).
- Belief content guard (C1 scoped exception): `fortuna_beliefs_guard()` —
  initial.sql:79-99. DELETE always refused; UPDATE refused unless ONLY the
  scoring columns (`status, outcome, brier, clv_bps`) change — every content
  column (`belief_id, created_at, event_id, p, p_raw, horizon, evidence,
  provenance, supersedes`) is `IS DISTINCT FROM`-guarded. Trigger
  `beliefs_guard BEFORE UPDATE OR DELETE` at :98-99. Matches I5 / CLAUDE.md C1.
- Later migrations add the same posture to `domain_analyses`,
  `scalar_beliefs` (set-once `realized_value`), `belief_scores`, `trade_scores`,
  `scorecards` (per repo doc-comments at repos.rs:2122, :2582, :2722, :2887).

### 3.2 App-layer INSERT-only discipline + every UPDATE audited

`grep UPDATE crates/fortuna-ledger/src/repos.rs` — each occurrence classified:

- **Allowed-by-design (C1 scoring-only, set-once):**
  - `resolve_and_score` `UPDATE beliefs SET status,outcome,brier,clv_bps ...
    WHERE outcome IS NULL` — repos.rs:1316-1327. Compliant (the 4 scoring cols,
    once).
  - `abandon_open_for_event` `UPDATE beliefs SET status='abandoned'` —
    repos.rs:1342-1349. Status-only; permitted by the guard.
  - mark-superseded `UPDATE beliefs SET status='superseded'` — repos.rs:1242.
  - `ScalarBeliefsRepo::resolve` `UPDATE scalar_beliefs SET realized_value,
    resolved_at WHERE realized_value IS NULL` — repos.rs:2467-2476. Set-once.
  - `domain_analyses SET status='superseded'` — repos.rs:2285.
- **Mutable-by-design tables (NO append-only trigger):**
  - `events` status/dead/unscoreable — repos.rs:546/557/568. The `events` table
    (initial.sql:21-36) intentionally has no append-only trigger; status is a
    lifecycle field. (Spec models event status as mutable.)
  - `source_registry` upsert `ON CONFLICT DO UPDATE` — repos.rs:1068. Registry
    table, no trigger, has `updated_at`. By design.
- **TEST-ONLY (never production):**
  - `try_mutate_content_for_test` `UPDATE beliefs SET p=$2` — repos.rs:1644-1656.
    Explicitly a test hook proving the DB guard rejects content mutation.
- `audit.rs` and `intent_journal.rs`: **no UPDATE/DELETE at all** — pure INSERT
  + SELECT. Confirmed.

**No app-layer UPDATE/DELETE violates I5.** The one `UPDATE beliefs SET p`
(repos.rs:1650) is a test hook that asserts the trigger fires. The beliefs
status UPDATEs ride the C1 exception and the guard enforces it.

### 3.3 Invariant test

`crates/fortuna-invariants/tests/i5_audit_append_only.rs:128-189` provisions a
migrated DB (`#[sqlx::test]`), asserts `UPDATE audit` and `DELETE FROM audit`
both error with "append-only" (lines 142-153), asserts byte-stable re-reads
(replay determinism, :155-166), and asserts the no-audit-no-trading halt
(:168-188). Protected crate — solid.

---

## 4. Replay machinery

- **`scripts/replay.sh`** has two modes:
  - `<recording.jsonl>` → `cargo run -p fortuna-core --bin replay-verify`
    (replay.sh:31).
  - `--seed <N>` → re-runs a DST seed (`cargo test -p fortuna-core --test dst
    --replay-seed N`, replay.sh:24-26).
- **`replay-verify`** (`crates/fortuna-core/src/bin/replay-verify.rs`) is a
  STRUCTURAL verifier of a recorded **bus event stream** (JSONL): seq density
  from 0, non-decreasing timestamps, byte-stable round-trip (:33-104). It does
  NOT regenerate derived events and does NOT touch the audit log.
- **Source of the recording**: `fortuna-recorder` is a B0 *perishable-data*
  (orderbook top-of-book) capture tool (`crates/fortuna-recorder/src/lib.rs:1-8`),
  prices in ten-thousandths — it is NOT the audit/decision recorder. The bus
  `Recording` (fortuna-core::bus) is what `replay-verify` consumes.
- **Decision replay from the audit log is NOT implemented.** Both the script
  comment (replay.sh:9-10: "live-decision replay from audit manifests arrives
  with the ledger") and the binary doc-comment (replay-verify.rs:5-7: "for live
  decisions, audit manifests (T0.8+)") explicitly defer it. As-built, you cannot
  feed the `audit` table to a tool and re-derive the decision; you can only
  (a) structurally verify a recorded bus stream, (b) re-run a DST seed
  deterministically, or (c) manually reconstruct from audit JSON payloads.

---

## 5. Metrics / Slack

### 5.1 "Every Slack message is also an audit row"

- The claim is a **caller contract**, not enforced inside the Slack client:
  `crates/fortuna-ops/src/slack.rs:123-130` states "the caller of
  `SlackRouter::send` MUST persist an audit row"; the ops crate has no DB dep.
- The contract IS satisfied in the daemon: `route_alerts`
  (`crates/fortuna-live/src/daemon.rs:5882-5917`) is the ONLY caller of
  `r.send(...)` (:5896). For every message it calls
  `runner.apply_external_alert(...)` → writes an `alert` audit row
  (runner.rs:815-820) on success (:5898), on send-failure (:5908, audited as
  `[SLACK SEND FAILED]`), AND when no router is configured (:5894). Every Slack
  routing decision the daemon makes (alerts, digests, calibration reports — all
  go through `route_alerts`, daemon.rs:3752-4173) produces an audit row.
  **Claim holds for the daemon path.**
- Caveat: enforcement is by convention (a future `send` caller that forgets to
  audit would not be caught at compile time). And per §1.2 these `alert` rows
  carry `actor = NULL`. Inbound Slack interactivity (button presses → "operator
  actions logged with actor", spec §8) is **not built** — ops/src/lib.rs:9-12
  states the crate "only ever SENDS to Slack — no inbound interactivity surface
  yet." So the spec's "interactive responses logged with actor" is a gap.

### 5.2 Metrics / OpenTelemetry

- `fortuna-ops` has a hand-rolled metrics registry rendering **Prometheus text
  exposition 0.0.4** (`crates/fortuna-ops/src/metrics.rs:4-6, :109-181`).
  Counters/gauges declared (e.g. `fortuna_gate_rejections_total`,
  `fortuna_exec_working_orders`, metrics.rs:235-237).
- **No OpenTelemetry / OTLP exporter.** metrics.rs:4-6 documents the deliberate
  choice: "the OTel Rust prometheus/OTLP exporters are Beta/RC, so the wire
  [format chosen is Prometheus text]" (research-grounded). ROTA console reads a
  pre-shaped view, never parses Prometheus text (rota.rs:342-346). So
  observability metrics exist (Prometheus text + dashboard view) but there is no
  OTel/tracing-export integration.

---

## Verdict (replayability)

**A decision is PARTIALLY replayable from the audit log; full byte-exact replay
from the audit table alone is NOT achievable as-built.**

What IS reconstructable from the trail: the context that fed a belief (manifest
items + content hashes via `context_manifest_hash` in `beliefs.provenance`), the
model id and cost, the proposal's thesis + manifest_hash, every gate verdict +
reason keyed by intent, vetoes, fills, settlements, watchdog actions, halts, and
lifecycle. Append-only is DB-enforced and tested; no app-layer mutation
violates I5.

What BREAKS exact replay:
1. **No `prompt_hash`** (mind.rs:692-696 vs spec.md:181) — the exact prompt is
   never hashed/stored; a prompt-template or model-string change is invisible.
2. **No structured belief↔proposal↔order key** — links are reconstructed by
   manifest-hash + free-text `thesis` correlation, not typed foreign keys
   (`proposal` rows have `ref_id = None`, runner.rs:1100).
3. **Decision-replay tooling does not exist** — `replay.sh`/`replay-verify`
   replay the bus event stream and DST seeds, NOT the audit log; audit-manifest
   replay is explicitly deferred (replay.sh:9-10, replay-verify.rs:5-7).
4. **Successful order submits live only in `intent_events`**, not the audit
   table (runner.rs:1321-1332) — a single-log audit reader is incomplete.
5. **`config_change` audit kind is absent**; **kill-switch activity is logged
   out-of-band (JSONL), not in `audit`** (by I4 design).
6. **All daemon/runner audit rows have `actor = NULL`** (audit_bridge.rs:103);
   only CLI rows carry an actor.

Append-only enforcement status: **STRONG.** DB triggers reject UPDATE/DELETE on
all I5 tables; the beliefs C1 exception is correctly scoped to the 4 scoring
columns and set-once; the only content UPDATE in repos is a test hook proving
the guard; the invariant test pins all of it.

---

## Open questions

- Is `prompt_hash` omission intentional (manifest_hash deemed sufficient) or an
  oversight vs spec.md:181? The `beliefs.rs` test fixture includes `prompt_hash`,
  suggesting it was once intended. Needs operator/spec adjudication.
- The "audit-manifest decision replay" (T0.8+) is referenced as a future task in
  two places — is it on the BUILD_PLAN as still-open, or descoped? Not verified
  here (out of area-g scope).
- Persona / discovery / aeolus / funding belief paths stamp their own provenance
  (persona_beliefs.rs:121, discovery.rs:796, aeolus_beliefs.rs:95,
  funding_forecast.rs:267 uses `Value::default()` = null provenance). The
  funding-forecast scalar path stamping null provenance may be a per-producer
  reconstructability gap; not deeply traced (binary-belief LLM path was the
  primary scope).
