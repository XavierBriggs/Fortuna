# Handoff: wiring the persona system into the live daemon (Track A)

**Who this is for:** Track A (owner of `crates/fortuna-live`). **What it asks:** add a
small, opt-in persona step to the `drive()` loop. **Why a handoff:** per the operator's
2026-06-13 decision, **Track E exposes the building blocks; Track A wires `drive()`** — so
Track E will not edit your core loop. Everything below is built, tested, and merged-ready on
branch `persona-live-integration` (slices 1, 2a, 2b, 2c).

The one-sentence version: **on a tick, read the signals personas care about, call one
orchestrator, and for each produced artifact persist it and fan it out to beliefs through
the paths you already have.** Default-off, so until an operator sets `[personas].enabled =
true` the loop behaves exactly as today.

Authoritative design: [domain-analysis-personas-design.md](domain-analysis-personas-design.md)
(§4 trust, §7 runner, §10 scoring). Track-E changelog: [track-e-changelog.md](track-e-changelog.md).

---

## 0. The building blocks Track E provides (all tested, all in `crates/fortuna-cognition` / `crates/fortuna-ledger`)

| Block | Signature | Slice |
|---|---|---|
| signal read-back | `SignalsRepo::recent_by_kind(kinds: &[String], received_after: &str, limit: i64) -> Vec<RecentSignalRow>` | 2a |
| the orchestrator | `persona_orchestrator::run_due_personas(now, &[PersonaSchedule], &[SignalEnvelope], &mut PersonaScheduleState, &dyn Mind, &mut DiscoveryBudget) -> Vec<PersonaRunResult>` | 2b |
| belief horizon | `persona_beliefs::belief_horizon(region_key: &str) -> Option<UtcTimestamp>` | 2c |
| fan-out | `persona_beliefs::map_persona_analysis(persona_id, version, analysis_id, content_hash, region_key, findings: &Value, horizon) -> Result<Vec<BeliefDraft>, _>` | E.4a |
| loader + registry check | `PersonaDef::parse(&md, &schema)`, `PersonaDef::validate_against(Option<&RegistryHead>)`, `PersonasRepo::head(persona_id)` | E.2 |
| persistence | `DomainAnalysesRepo::insert(...)` (already in your tree), and your existing `persist_beliefs(...)` | E.1 |

Relevant types (all `fortuna_cognition::…`): `PersonaSchedule { def: PersonaDef, cadences: Vec<Cadence> }`,
`PersonaScheduleState::new(debounce_ms)`, `PersonaRunResult { persona_id, persona_version, region_key, outcome: PersonaOutcome }`,
`SignalEnvelope { signal_id, source, kind, received_at: UtcTimestamp, payload: Value, content_hash }`,
`PersonaOutcome { findings: Option<Value>, content_hash: Option<String>, manifest_hash: Option<String>, signal_manifest, produced_at, cost_cents, defects, .. }` with `produced_artifact() -> bool`.

The firewall (§4), the cost-budget throttle, schema validation, and degrade-to-defects all live
**inside** `run_persona_analysis`, which `run_due_personas` calls — you do not re-implement any of it.

---

## 1. Config: a new `[personas]` section (opt-in, default off)

Add to your `DaemonToml` (`boot.rs`), `#[serde(default)]` so omitting it = disabled:

```toml
[personas]
enabled     = false   # master switch; false => the loop is byte-identical to today
debounce_ms = 0       # PersonaTriggerGate coalescing window
window_hours = 48     # how far back recent_by_kind reads signals
max_signals = 200     # recent_by_kind LIMIT

# one block per registered persona
[[personas.persona]]
id       = "meteorologist"
dir      = "config/personas/meteorologist"   # holds persona.md + schema.json
cadences = []                                # e.g. [{ every_hours = 6 }] or [{ daily_at_hour_utc = 5 }]

[[personas.persona]]
id       = "macro-economist"
dir      = "config/personas/macro-economist"
cadences = [{ daily_at_hour_utc = 12 }]
```

Validate at boot like your other sections (reject an unknown cadence shape — `Cadence::validate()`).

---

## 2. Boot: load + hash-validate each persona (once)

For each configured persona, build a `PersonaSchedule`. The loader is **fail-closed**: a file
whose hash ≠ the active registry row is refused, so a tampered method never runs.

```rust
// pseudocode — adapt to your boot error type
let mut schedules = Vec::new();
for p in &cfg.personas.persona {
    let md     = std::fs::read_to_string(format!("{}/persona.md", p.dir))?;
    let schema = std::fs::read_to_string(format!("{}/schema.json", p.dir))?;
    let def    = PersonaDef::parse(&md, &schema)?;
    let row    = PersonasRepo::new(pool.clone()).head(&p.id).await?;   // Option<PersonaRow>
    let head   = row.as_ref().map(|r| RegistryHead {
        version: r.version, method_hash: r.method_hash.clone(), status: r.status.clone(),
    });
    def.validate_against(head.as_ref())?;                             // refuses inactive/hash/version mismatch
    schedules.push(PersonaSchedule { def, cadences: p.cadences.clone() });
}
let mut persona_state = PersonaScheduleState::new(cfg.personas.debounce_ms);
```

Hold `schedules` + `persona_state` across ticks (like your `DailyScheduler`). Cross-restart
durability of the cadence/gate is in-process only today (noted in GAPS — same scope as your
existing schedulers).

---

## 3. The `drive()` step (the ~15 lines)

Run this on whatever cadence you prefer (each tick, or a persona sub-cadence). It is a no-op
when `!enabled`.

```rust
// 1. read the signals these personas care about
let kinds: Vec<String> = schedules.iter()
    .flat_map(|s| s.def.meta.reads_signal_kinds.iter().cloned())
    .collect();
let after = /* now - window_hours, as ISO8601 */;
let rows  = SignalsRepo::new(pool.clone())
    .recent_by_kind(&kinds, &after, cfg.personas.max_signals).await?;

// 2. ledger rows -> the orchestrator's cognition-native input
let signals: Vec<SignalEnvelope> = rows.into_iter().filter_map(|r| {
    Some(SignalEnvelope {
        received_at: UtcTimestamp::parse_iso8601(&r.received_at).ok()?,  // skip an unparseable row
        signal_id: r.signal_id, source: r.source, kind: r.kind,
        payload: r.payload, content_hash: r.content_hash,
    })
}).collect();

// 3. one call decides what's due and runs it (firewall/budget/determinism inside)
let results = run_due_personas(now, &schedules, &signals, &mut persona_state,
                               mind.as_ref(), &mut persona_budget).await;

// 4. persist each produced artifact, then fan out to beliefs through your existing paths
for r in results {
    for d in &r.outcome.defects {
        // route to #fortuna-ops / your alert sink (these are audit-worthy, not crashes)
        runner.apply_external_alert("personas", &format!("{}: {d}", r.persona_id));
    }
    if !r.outcome.produced_artifact() { continue; }      // throttled / skipped / degraded

    let findings   = r.outcome.findings.as_ref().expect("produced_artifact => Some");
    let content_h  = r.outcome.content_hash.as_deref().expect("produced_artifact => Some");
    let analysis_id = /* your ULID mint (clock-monotonic, like belief ids) */;

    DomainAnalysesRepo::new(pool.clone()).insert(
        &analysis_id, &r.persona_id, r.persona_version, &domain_for(&r.persona_id),
        &r.region_key, &iso(r.outcome.produced_at),
        &serde_json::to_value(&r.outcome.signal_manifest)?,   // signal_manifest -> JSON
        findings, content_h,
        r.outcome.manifest_hash.as_deref().unwrap_or(""),
        r.outcome.cost_cents, None /* supersedes */, &now_iso,
    ).await?;

    // beliefs need a resolution horizon; no parseable date => persist the artifact, skip beliefs
    let Some(horizon) = belief_horizon(&r.region_key) else { continue };
    let drafts = map_persona_analysis(
        &r.persona_id, r.persona_version, &analysis_id, content_h,
        &r.region_key, findings, horizon,
    )?;
    // attribute to a strategy for the gate pipeline + scoring (see note below)
    let strategy = persona_strategy_id(&r.persona_id);
    let pairs: Vec<_> = drafts.into_iter().map(|d| (strategy.clone(), d)).collect();
    persist_beliefs(&pool, &pairs, &now_iso, belief_id_base).await?;   // your existing path
}
```

(The two `.expect(...)` are guarded by `produced_artifact()`; if your money-path lints forbid
`expect` even when guarded, bind with `let Some(..) = .. else { continue }` instead.)

---

## 4. Two integration decisions that are yours

1. **`StrategyId` for persona beliefs** (the `persona_strategy_id` above). The gate pipeline and
   the §10 calibration scope key on `strategy`. A single dedicated strategy (e.g. `"domain-analysis"`)
   or one per persona both work; the per-`(persona, version)` scoring in `persona_scoring.rs` is
   orthogonal (it keys on `PersonaScope`, not `StrategyId`). Pick one and keep it stable — it is a
   promotion-gate boundary (I7).
2. **`analysis_id` minting** — use your existing clock-monotonic ULID scheme (the one
   `persist_beliefs` already uses for belief ids), so a tick's ids are deterministic under `SimClock`.

---

## 5. Invariants you inherit for free (don't re-litigate)

- **I6 propose-only:** `PersonaOutcome` and the `domain_analyses` table carry no order/size/price
  field — pinned in `fortuna-invariants/tests/i6_persona_propose_only.rs`. A persona emits DATA.
- **§4 firewall:** the method rides only in the Mind system charter; signals are untrusted
  `<context-item>` data. `run_due_personas`/`run_persona_analysis` enforce it; your wiring only
  moves signals (data) and persists outputs.
- **Zero capital until proven:** persona beliefs pass the SAME gate pipeline; no orders are placed
  on them until the §11 promotion gate passes for a subset (an operator action, never the daemon).

---

## 6. The one known limitation (documented in GAPS)

A **cadence with no in-window signal for any region is a no-op** — regions are derived from signal
payloads (`fill_region_key`), so a "naked" cadence has nothing to key. This is fine for the shipped
personas (the macro release-window run still has a calendar signal present). If you later want
cadence-only runs over an operator-supplied region catalog, that is a small additive extension to
`PersonaSchedule` — ask Track E.

---

## 7. How to verify your wiring

Track E's library tests already cover the orchestrator/firewall/fan-out/horizon. For the daemon
seam, add a `daemon`-side test that: enables `[personas]`, inserts a fixture signal, ticks once, and
asserts one `domain_analyses` row + N beliefs with provenance `{persona_id, persona_version,
analysis_id, analysis_content_hash}` resolving to that artifact. Mirror the end-to-end shape in
`crates/fortuna-ledger/tests/persona_e2e.rs` (the pipeline, sans the live loop). Gate as usual:
`fmt` + `clippy --workspace --all-targets -D warnings` + `cargo test --workspace` + `run-dst.sh`.

---

## 8. Slice 3 — surface persona promote/retire verdicts in the weekly review (do this WITH your review wiring)

Per design §10/§11/§21, persona scoring is an **additive parallel layer** — you do **not** extend
`review::ScopeKey` (that struct literal is yours at `daemon.rs:1024`; adding fields there is exactly
what §21 deliberately avoided so the boundary holds). Instead, in your weekly-review step, score each
`(persona, version)` with the already-built `fortuna_cognition::persona_scoring` and route the
verdicts to `#fortuna-review` alongside the GO/NO-GO recs. **Recommendation-only (I7):** the daemon
never promotes/retires; the operator acts out-of-band (a superseding `personas` registry insert, or
`status='retired'`). The §20.1 ROTA personas-view reads the SAME scoring.

The API (all built + tested in `persona_scoring.rs`):

```rust
use fortuna_cognition::persona_scoring::{
    Baseline, PersonaScope, PersonaScopeRecord, propose_promotion, score_persona,
};
// per (persona, version):
let record = PersonaScopeRecord {
    scope:   PersonaScope { persona_id, persona_version },
    samples: /* Vec<(claimed_p, outcome_bool)> — the scope's RESOLVED beliefs */,
    clv_bps: /* Vec<f64> — their CLV measurements (skip the None ones) */,
};
let card     = score_persona(&record);
let proposal = propose_promotion(
    &card,
    prior_version_card.as_ref(),                  // Option<&PersonaScorecard> (beats_prior_version)
    Baseline { brier_mean: no_persona_brier },    // raw-source-direct beliefs, SAME events
    Baseline { brier_mean: market_brier },        // market-implied p, SAME events
    cfg.review.min_resolved_beliefs_synthesis,    // the §11 floor (≈60)
);
// proposal.verdict ∈ Evaluating { resolved, needed } | Promotable | RetireCandidate
route_to_review(format!(
    "{}@{} — {:?}: {}",
    record.scope.persona_id, record.scope.persona_version, proposal.verdict, proposal.rationale,
));
```

**The one data dependency (a shared building block).** Each `PersonaScopeRecord` needs the scope's
RESOLVED beliefs (claimed `p`, `outcome`, `clv_bps`) grouped by the provenance
`{persona_id, persona_version}` that the fan-out stamps (`map_persona_analysis`). Two ways:
- **Quick:** filter `BeliefsRepo::recent(limit)` (it already returns `brier`/`clv_bps`/`provenance`)
  by `provenance.persona_id` / `persona_version` — fine for a first cut, bounded by `limit`; or
- **Clean:** a dedicated `BeliefsRepo::resolved_persona_stats(persona_id, version) -> PersonaScopeRecord`
  query — the §20.1 ROTA personas-view needs the SAME data. **Track E will add this query on request**
  (a small `recent_by_kind`-style ledger slice); ask and it lands.

**The baselines** (`no_persona_brier`, `market_brier`) are the §11 comparison, scored over the SAME
resolved events: the raw-source-direct belief Brier and the market-implied (`p` = price) Brier. You
already assemble market-implied inputs in the weekly review. If either baseline is unavailable for a
scope, leave it `Evaluating` — never promote without both (§11).

This step is **not a blocker** for personas running or being scored: `persona_scoring` works
standalone today; Slice 3 just surfaces its verdicts inside the daemon's weekly digest.
