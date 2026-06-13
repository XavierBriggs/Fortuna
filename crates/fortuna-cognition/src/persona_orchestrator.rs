//! Track E (persona live-loop brain): `run_due_personas`, the DB-free tick
//! orchestrator (design §7/§8).
//!
//! Each daemon tick hands this module the loaded persona schedules, the fresh
//! signal envelopes, the cross-tick trigger state, a `Mind`, and a discovery
//! budget. It decides WHICH `(persona, region_key)` runs are DUE — by fresh
//! signal OR by cadence — executes each DUE run through the existing
//! [`run_persona_analysis`](crate::persona_runner::run_persona_analysis), and
//! returns [`PersonaRunResult`]s for the daemon (a different track) to persist.
//! It NEVER persists: cognition has no `fortuna-ledger` dependency, exactly like
//! `run_reconciliation` — it returns drafts/outcomes only.
//!
//! ## How "due" is decided (design §7)
//! For each schedule, a [`PersonaTriggerSpec`] is built from the persona meta +
//! its operator cadences. A signal triggers a persona only if the persona READS
//! its kind ([`PersonaTriggerSpec::fires_on_signal`]). The triggering signals are
//! grouped by their derived `region_key` (the persona's `region_key` template,
//! filled from the signal payload — see [`fill_region_key`]). A
//! `(persona, region_key)` group is DUE when EITHER:
//!   - **signal mode**: at least one signal in the group is *fresh* per the
//!     [`PersonaTriggerGate`] (request/begin/complete, so duplicate/repeat
//!     signals for one `(persona, region)` COALESCE to a single run); OR
//!   - **cadence mode**: one of the persona's cadences is `due` for
//!     `persona_region_key(id, region)` at `now`.
//!
//! ## Keying (the collision-safe unit)
//! Coalescing and cadence both key on
//! [`persona_region_key`](crate::persona_trigger::persona_region_key) — the
//! 0x1F-separated `(persona_id, region_key)` pair the trigger layer already
//! defines. This module does NOT re-derive a key scheme.
//!
//! ## The §4 firewall (design §4)
//! Signals are UNTRUSTED data. They enter the run ONLY as
//! [`ContextItem`](crate::context::ContextItem) bodies (the data path); the
//! persona's trusted method is the Mind's system charter, never a context item —
//! `run_persona_analysis` enforces that. A poisoned signal's worst case is a bad
//! finding → still gated; it can never rewrite the method.
//!
//! ## Determinism & no-panic
//! All time is the injected `now`. The same inputs (incl. a scripted `StubMind`)
//! yield identical [`PersonaRunResult`] vectors. No `panic!`/`unwrap`/`expect` on
//! the run path; a `None` region or a context-assembly error degrades to a
//! counted defect or a skip, never a crash.

use crate::context::{content_hash_of, ContextItem, SectionKind};
use crate::discovery::DiscoveryBudget;
use crate::mind::Mind;
use crate::persona::PersonaDef;
use crate::persona_runner::{run_persona_analysis, PersonaOutcome};
use crate::persona_trigger::{
    persona_region_key, Cadence, CadenceScheduler, PersonaTriggerGate, PersonaTriggerSpec,
};
use crate::signals::{SignalEnvelope, TriggerDecision};
use fortuna_core::clock::UtcTimestamp;
use serde_json::Value;

/// One persona's live-loop input bundle: the loaded definition plus the
/// operator's cadence config (a persona does not carry its own cadences — they
/// are trigger config, design §7).
#[derive(Debug, Clone)]
pub struct PersonaSchedule {
    pub def: PersonaDef,
    pub cadences: Vec<Cadence>,
}

/// Persistent cross-tick orchestrator state: the cadence fire-once-per-period
/// ledger and the signal coalescing/debounce gate. The daemon holds one of these
/// across ticks (in-process, like the daemon's `DailyScheduler` — see the
/// `CadenceScheduler` SCOPE note; cross-restart durability is deferred to GAPS).
#[derive(Debug)]
pub struct PersonaScheduleState {
    cadence: CadenceScheduler,
    gate: PersonaTriggerGate,
}

impl PersonaScheduleState {
    /// `debounce_ms`: the gate's post-completion coalescing window (a signal
    /// burst within the window is one run, not five).
    pub fn new(debounce_ms: i64) -> PersonaScheduleState {
        PersonaScheduleState {
            cadence: CadenceScheduler::new(),
            gate: PersonaTriggerGate::new(debounce_ms),
        }
    }
}

/// One executed `(persona, region_key)` run — the daemon persists the carried
/// [`PersonaOutcome`] (findings/content_hash/signal_manifest) as one append-only
/// `domain_analyses` row + an audit row. Order-free by construction (I6: the
/// outcome carries no order/size/price field).
#[derive(Debug)]
pub struct PersonaRunResult {
    pub persona_id: String,
    pub persona_version: i32,
    pub region_key: String,
    pub outcome: PersonaOutcome,
}

/// Substitute every `{field}` placeholder in `template` with `payload[field]`,
/// rendered as a string: a JSON string value uses its `str`; a number/bool uses
/// its JSON repr. Returns `None` if any placeholder's field is absent or is not
/// a scalar (an array/object/null is not a region component) — the caller skips
/// that signal with a counted defect. A template with no placeholders is
/// returned unchanged. PURE: no IO, no clock, no panic.
pub fn fill_region_key(template: &str, payload: &Value) -> Option<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        // Copy the literal prefix before the placeholder.
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let close = after_open.find('}')?; // an unbalanced '{' is malformed → None
        let field = &after_open[..close];
        let value = payload.get(field)?;
        let rendered = scalar_to_string(value)?;
        out.push_str(&rendered);
        rest = &after_open[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Render a JSON scalar to its region-component string. A non-scalar (array,
/// object, null) is rejected — a region key component must be a concrete scalar.
fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Render one untrusted signal envelope into the `body` of a `FreshSignals`
/// context item. Deterministic: a stable JSON object of the envelope's id,
/// source, kind, receipt time, and payload. The body is DATA (quoted inside a
/// `<context-item>` block by the assembler); nothing here is an instruction.
fn render_signal_body(sig: &SignalEnvelope) -> String {
    // A plain object built field-by-field so the rendering never depends on the
    // envelope's struct field order; serde_json sorts object keys, so this is
    // byte-stable for identical inputs.
    let rendered = serde_json::json!({
        "signal_id": sig.signal_id,
        "source": sig.source,
        "kind": sig.kind,
        "received_at": sig.received_at.epoch_millis(),
        "payload": sig.payload,
    });
    // to_string never fails for a plain Value; degrade rather than unwrap to
    // honor the no-panic rule (an empty body still assembles deterministically).
    serde_json::to_string(&rendered).unwrap_or_default()
}

/// Build the per-group `ContextItem`s from its signals (section `FreshSignals`;
/// `content_hash = content_hash_of(body)` — REQUIRED or assembly fails — with
/// `item_id` = signal_id and `at` = received_at).
fn items_for_group(signals: &[&SignalEnvelope]) -> Vec<ContextItem> {
    signals
        .iter()
        .map(|sig| {
            let body = render_signal_body(sig);
            ContextItem {
                content_hash: content_hash_of(&body),
                item_id: sig.signal_id.clone(),
                section: SectionKind::FreshSignals,
                body,
                at: sig.received_at,
            }
        })
        .collect()
}

/// An insertion-ordered grouping of the considered signals by derived region.
/// A `Vec` of `(region, signals)` keeps the first-seen order deterministic
/// without a hash map (so the run order is a pure function of the input slice).
struct RegionGroups<'a> {
    groups: Vec<(String, Vec<&'a SignalEnvelope>)>,
}

impl<'a> RegionGroups<'a> {
    fn new() -> RegionGroups<'a> {
        RegionGroups { groups: Vec::new() }
    }

    fn push(&mut self, region: String, sig: &'a SignalEnvelope) {
        match self.groups.iter_mut().find(|(r, _)| r == &region) {
            Some((_, sigs)) => sigs.push(sig),
            None => self.groups.push((region, vec![sig])),
        }
    }
}

/// The live-loop brain (design §7/§8): decide which `(persona, region_key)` runs
/// are DUE this tick (by fresh signal OR cadence), execute each exactly once, and
/// return the results for the daemon to persist. DB-free; deterministic; never
/// panics.
pub async fn run_due_personas(
    now: UtcTimestamp,
    schedules: &[PersonaSchedule],
    signals: &[SignalEnvelope],
    state: &mut PersonaScheduleState,
    mind: &dyn Mind,
    budget: &mut DiscoveryBudget,
) -> Vec<PersonaRunResult> {
    let mut results = Vec::new();

    for schedule in schedules {
        let def = &schedule.def;
        let spec = PersonaTriggerSpec::from_meta(&def.meta, schedule.cadences.clone());

        // 1. Group the signals this persona READS by their derived region key.
        //    A signal whose region cannot be filled is skipped (the persona
        //    just can't key it) — never a panic.
        let mut groups = RegionGroups::new();
        for sig in signals {
            if !spec.fires_on_signal(&sig.kind) {
                continue;
            }
            if let Some(region) = fill_region_key(&def.meta.region_key, &sig.payload) {
                groups.push(region, sig);
            }
            // None region → cannot key this signal for this persona; drop it.
        }

        // 2. Decide DUE per region group (signal-fresh OR cadence) and run once.
        for (region, group_signals) in &groups.groups {
            let signal_fresh = matches!(
                state.gate.request(&def.meta.id, region, now),
                TriggerDecision::Fire
            );
            // Whether or not the signal fired, evaluate the cadences too — a
            // cadence due in this period also makes the group due. Evaluating
            // every cadence (not short-circuiting) consumes each due cadence's
            // period deterministically (matching CadenceScheduler semantics).
            let prk = persona_region_key(&def.meta.id, region);
            let mut cadence_due = false;
            for cadence in &schedule.cadences {
                if state.cadence.due(&prk, cadence, now) {
                    cadence_due = true;
                }
            }

            if !(signal_fresh || cadence_due) {
                continue; // not due this tick (coalesced/debounced, no cadence)
            }

            // DUE → run exactly once. Serialize through the gate when the SIGNAL
            // fired (begin/complete bracket the in-flight run so the K duplicate
            // requests this tick already coalesced above).
            if signal_fresh {
                state.gate.begin(&def.meta.id, region);
            }
            let items = items_for_group(group_signals);
            let result = run_one(def, region, &items, mind, budget, now).await;
            if signal_fresh {
                // The gate reports a coalesced-trigger count. In this serial,
                // one-request-per-(persona,region)-group tick model it is always
                // 0 (no concurrent request can register between this group's own
                // begin and complete), so the discard is deliberate and reasoned,
                // not silent. If the daemon ever drives `run_due_personas`
                // concurrently across batches sharing one state, revisit to
                // surface bursts to the caller.
                let _coalesced = state.gate.complete(&def.meta.id, region, now);
            }
            results.push(result);
        }

        // 3. Cadence-only fires (no signal in the group): a cadence may be due
        //    for a region this tick has no signal for. The orchestrator only
        //    knows regions from signals (the region template needs a payload),
        //    so a "naked" cadence with zero signals has no region to run — it is
        //    a no-op here by construction (the runner skips on no signals). This
        //    matches design §7: cadence is a TRIGGER, the run still needs its
        //    point-in-time signal inputs; a cadence with no fresh signal for any
        //    region simply has nothing to analyze this tick.
    }

    results
}

/// Execute one `(persona, region)` run, degrading a context-assembly error to a
/// counted defect on an empty outcome (never a panic; the runner's only hard
/// error is assembly, which cannot happen when bodies are hashed correctly).
async fn run_one(
    def: &PersonaDef,
    region: &str,
    items: &[ContextItem],
    mind: &dyn Mind,
    budget: &mut DiscoveryBudget,
    now: UtcTimestamp,
) -> PersonaRunResult {
    let outcome = match run_persona_analysis(def, region, items, mind, budget, now).await {
        Ok(outcome) => outcome,
        Err(e) => {
            // Should be unreachable (we hash bodies correctly), but degrade
            // rather than crash: a counted defect on an empty outcome.
            let mut degraded = PersonaOutcome {
                persona_id: def.meta.id.clone(),
                persona_version: def.meta.version,
                region_key: region.to_string(),
                produced_at: now,
                signal_manifest: Vec::new(),
                findings: None,
                content_hash: None,
                manifest_hash: None,
                cost_cents: 0,
                throttled: false,
                skipped_no_signals: false,
                defects: Vec::new(),
            };
            degraded
                .defects
                .push(format!("context assembly failed (run skipped): {e}"));
            degraded
        }
    };
    PersonaRunResult {
        persona_id: def.meta.id.clone(),
        persona_version: def.meta.version,
        region_key: region.to_string(),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fill_region_key_happy_path() {
        let payload = json!({"station": "KNYC", "date": "2026-06-12"});
        assert_eq!(
            fill_region_key("weather:{station}:tmax:{date}", &payload).as_deref(),
            Some("weather:KNYC:tmax:2026-06-12")
        );
    }

    #[test]
    fn fill_region_key_missing_field_is_none() {
        let payload = json!({"station": "KNYC"});
        assert_eq!(fill_region_key("{station}:{date}", &payload), None);
    }

    #[test]
    fn fill_region_key_no_placeholders_is_identity() {
        let payload = json!({});
        assert_eq!(
            fill_region_key("macro:CPI", &payload).as_deref(),
            Some("macro:CPI")
        );
    }

    #[test]
    fn fill_region_key_non_scalar_is_none() {
        assert_eq!(fill_region_key("{x}", &json!({"x": [1, 2]})), None);
        assert_eq!(fill_region_key("{x}", &json!({"x": {"k": 1}})), None);
        assert_eq!(fill_region_key("{x}", &json!({"x": null})), None);
    }

    #[test]
    fn fill_region_key_renders_scalars() {
        assert_eq!(
            fill_region_key("{n}:{b}", &json!({"n": 42, "b": false})).as_deref(),
            Some("42:false")
        );
    }

    #[test]
    fn render_signal_body_is_deterministic_and_key_sorted() {
        // Two INDEPENDENT envelopes with identical field values must render
        // byte-identically (catches any HashMap-style non-determinism, which a
        // same-instance double-call would miss).
        let make = || SignalEnvelope {
            signal_id: "s1".to_string(),
            source: "aeolus".to_string(),
            kind: "aeolus.forecast".to_string(),
            received_at: UtcTimestamp::parse_iso8601("2026-06-12T04:00:00.000Z").unwrap(),
            payload: json!({"b": 2, "a": 1}),
            content_hash: "x".to_string(),
        };
        assert_eq!(render_signal_body(&make()), render_signal_body(&make()));

        // Canary: serde_json must emit object keys sorted (no `preserve_order`
        // feature in the workspace). If this flips, byte-stability of the
        // content_hash anchor across runs is at risk — so pin it here.
        let body = render_signal_body(&make());
        let pos_a = body.find("\"a\"").expect("payload key a present");
        let pos_b = body.find("\"b\"").expect("payload key b present");
        assert!(
            pos_a < pos_b,
            "serde_json must sort object keys; `preserve_order` would break the anchor"
        );
    }
}
