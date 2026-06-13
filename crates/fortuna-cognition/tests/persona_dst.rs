//! Track E E.3c: seeded DST over the persona runner under the cost budget
//! (design §8/§15). Every scenario builds a seeded world — 0..=4 point-in-time
//! signals (0 exercises the skip path), a random (possibly pre-exhausted)
//! DiscoveryBudget, and a CHAOS MIND whose seeded mode mixes a valid finding
//! with every failure mode (provider error, schema-invalid via unknown/missing
//! field, non-JSON prose, empty journal). A separate INTEGRATION coalescing arm
//! threads a trigger gate through the runner with a call-counting mind: K+1
//! concurrent triggers must produce exactly ONE run (one mind call).
//!
//! Invariants checked on EVERY seed:
//!   1. run_persona_analysis NEVER panics and NEVER errors (degrade, not crash).
//!   2. Budget THROTTLE: spent >= cap → no call, no artifact, no spend.
//!   3. No signals → skip: no call, no artifact, no defect.
//!   4. A reached run calls the mind EXACTLY once; artifact ⟺ Valid mode, with
//!      every anchor set; a failure mode yields no artifact + a counted defect.
//!   5. Determinism: the same seed yields a byte-identical content_hash.
//!   6. Coalescing (integration): K+1 triggers through the gate → one runner call.
//!
//! Conventions follow crates/fortuna-core/tests/dst.rs: master seed from
//! DST_MASTER_SEED or wall clock (printed), per-scenario seeds via SplitMix64,
//! failures print the offending seed. Scenario count via PERSONA_DST_SCENARIOS
//! (default 20; the battery runs 2000).

use fortuna_cognition::context::{content_hash_of, AssembledContext, ContextItem, SectionKind};
use fortuna_cognition::discovery::DiscoveryBudget;
use fortuna_cognition::mind::{Mind, MindError, MindOutput};
use fortuna_cognition::persona::PersonaDef;
use fortuna_cognition::persona_runner::{run_persona_analysis, PersonaOutcome};
use fortuna_cognition::persona_trigger::PersonaTriggerGate;
use fortuna_cognition::signals::TriggerDecision;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::SplitMix64;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

const PERSONA_MD: &str = "+++\n\
id = \"dst\"\n\
version = 1\n\
domain = \"weather\"\n\
domain_tags = [\"t\"]\n\
reads_signal_kinds = [\"k\"]\n\
tier = \"cheap\"\n\
region_key = \"r\"\n\
output_schema_version = \"v1\"\n\
+++\n\
the trusted method body.\n";

const SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["verdict"],
        "properties":{"verdict":{},"note":{}}}"#;

fn trigger_at() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T05:00:00.000Z").unwrap()
}

fn signal_at() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T04:00:00.000Z").unwrap()
}

fn a_signal(id: &str, body: &str) -> ContextItem {
    ContextItem {
        item_id: id.to_string(),
        section: SectionKind::FreshSignals,
        body: body.to_string(),
        content_hash: content_hash_of(body),
        at: signal_at(),
    }
}

/// The seeded chaos mode for one scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChaosMode {
    Valid,
    Provider,
    UnknownField,
    MissingRequired,
    NonJsonProse,
    EmptyJournal,
}

impl ChaosMode {
    fn pick(rng: &mut SplitMix64) -> ChaosMode {
        match rng.next_u64() % 6 {
            0 => ChaosMode::Valid,
            1 => ChaosMode::Provider,
            2 => ChaosMode::UnknownField,
            3 => ChaosMode::MissingRequired,
            4 => ChaosMode::NonJsonProse,
            _ => ChaosMode::EmptyJournal,
        }
    }
    /// Does this mode yield an artifact when REACHED (budget allowed, signals
    /// present)? Only a clean Valid finding does.
    fn yields_artifact(self) -> bool {
        self == ChaosMode::Valid
    }
}

/// A seeded mind that counts its calls (to prove the runner calls it once).
struct ChaosMind {
    mode: ChaosMode,
    cost_cents: i64,
    calls: AtomicUsize,
}

impl ChaosMind {
    fn new(mode: ChaosMode, cost_cents: i64) -> ChaosMind {
        ChaosMind {
            mode,
            cost_cents,
            calls: AtomicUsize::new(0),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl Mind for ChaosMind {
    fn id(&self) -> &str {
        "chaos"
    }
    async fn decide(&self, _ctx: &AssembledContext) -> Result<MindOutput, MindError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.mode == ChaosMode::Provider {
            return Err(MindError::Provider {
                reason: "seeded transport failure".to_string(),
            });
        }
        let journal = match self.mode {
            ChaosMode::Valid => Some(json!({"verdict": "ridge"}).to_string()),
            ChaosMode::UnknownField => Some(json!({"verdict": "x", "sneaky": 1}).to_string()),
            ChaosMode::MissingRequired => Some(json!({"note": "no verdict"}).to_string()),
            ChaosMode::NonJsonProse => Some("it will be warm".to_string()),
            ChaosMode::EmptyJournal => None,
            ChaosMode::Provider => unreachable!(),
        };
        let body = json!({
            "beliefs": [],
            "proposals": [],
            "journal": journal.map(|b| json!({"body": b})),
            "cost_cents": self.cost_cents,
        });
        serde_json::from_value(body).map_err(|e| MindError::SchemaInvalid {
            reason: e.to_string(),
        })
    }
}

struct ScenarioOutcome {
    content_hash: Option<String>,
    produced: bool,
    throttled: bool,
    skipped: bool,
}

fn run_scenario(seed: u64) -> Result<ScenarioOutcome, String> {
    let mut rng = SplitMix64::new(seed);
    let persona =
        PersonaDef::parse(PERSONA_MD, SCHEMA).map_err(|e| format!("persona parse: {e}"))?;

    // Seeded budget: cap and a pre-spend that may exhaust it.
    let cap = (rng.next_u64() % 60) as i64; // 0..=59
    let pre_spend = (rng.next_u64() % 70) as i64; // 0..=69
    let mut budget = DiscoveryBudget::new(cap);
    budget.record_spend(pre_spend, trigger_at());
    let allowed = budget.spent_today_cents() < cap;

    // 0..=4 signals (0 exercises the skip path), strictly before the trigger.
    let n_signals = (rng.next_u64() % 5) as usize;
    let signals: Vec<ContextItem> = (0..n_signals)
        .map(|i| {
            a_signal(
                &format!("sig-{i}"),
                &format!("signal-{i}-{}", rng.next_u64()),
            )
        })
        .collect();

    let mode = ChaosMode::pick(&mut rng);
    let cost = (rng.next_u64() % 50) as i64;
    let mind = ChaosMind::new(mode, cost);

    let outcome: PersonaOutcome = futures::executor::block_on(run_persona_analysis(
        &persona,
        "weather:dst:r",
        &signals,
        &mind,
        &mut budget,
        trigger_at(),
    ))
    .map_err(|e| format!("seed {seed}: runner errored (must degrade): {e}"))?;

    // Exactly one of {throttled, skipped, ran}, in the runner's order: budget
    // first, then no-signals skip, then the mind call.
    if !allowed {
        if !outcome.throttled
            || outcome.produced_artifact()
            || outcome.cost_cents != 0
            || outcome.manifest_hash.is_some()
            || mind.calls() != 0
        {
            return Err(format!(
                "seed {seed}: budget-exhausted run not cleanly throttled (cap={cap}, pre={pre_spend})"
            ));
        }
    } else if signals.is_empty() {
        if !outcome.skipped_no_signals
            || outcome.produced_artifact()
            || outcome.cost_cents != 0
            || outcome.manifest_hash.is_some()
            || mind.calls() != 0
            || !outcome.defects.is_empty()
        {
            return Err(format!("seed {seed}: no-signal run not cleanly skipped"));
        }
    } else {
        if outcome.throttled || outcome.skipped_no_signals {
            return Err(format!(
                "seed {seed}: a run was mis-flagged throttled/skipped"
            ));
        }
        if outcome.manifest_hash.is_none() {
            return Err(format!(
                "seed {seed}: reached the mind but no manifest_hash"
            ));
        }
        if mind.calls() != 1 {
            return Err(format!(
                "seed {seed}: mind called {} times (must be exactly 1)",
                mind.calls()
            ));
        }
        let want_artifact = mode.yields_artifact();
        if outcome.produced_artifact() != want_artifact {
            return Err(format!(
                "seed {seed}: produced_artifact={} but mode={mode:?}",
                outcome.produced_artifact()
            ));
        }
        if want_artifact {
            if outcome.content_hash.is_none()
                || outcome.findings.is_none()
                || !outcome.defects.is_empty()
            {
                return Err(format!(
                    "seed {seed}: artifact missing an anchor or has defects"
                ));
            }
        } else if outcome.defects.is_empty() {
            return Err(format!("seed {seed}: a failure mode produced no defect"));
        }
    }

    // Invariant 6 (integration coalescing): thread a gate through the runner with
    // a call-counting mind. K+1 concurrent triggers → exactly ONE run (one call).
    let k = (rng.next_u64() % 4 + 2) as usize; // 2..=5
    let coalesce_mind = ChaosMind::new(ChaosMode::Valid, 1);
    let coalesce_signals = vec![a_signal("csig", "coalesce body")];
    let mut fresh_budget = DiscoveryBudget::new(1_000); // always allows
    let mut gate = PersonaTriggerGate::new(0);
    if gate.request("dst", "r", trigger_at()) != TriggerDecision::Fire {
        return Err(format!("seed {seed}: first coalesce request did not Fire"));
    }
    gate.begin("dst", "r");
    futures::executor::block_on(run_persona_analysis(
        &persona,
        "weather:dst:r",
        &coalesce_signals,
        &coalesce_mind,
        &mut fresh_budget,
        trigger_at(),
    ))
    .map_err(|e| format!("seed {seed}: coalesce run errored: {e}"))?;
    let mut coalesced = 0;
    for _ in 0..k {
        if gate.request("dst", "r", trigger_at()) == TriggerDecision::CoalescedInFlight {
            coalesced += 1;
        }
    }
    gate.complete("dst", "r", trigger_at());
    if coalesce_mind.calls() != 1 {
        return Err(format!(
            "seed {seed}: {} mind calls for K+1 triggers (must coalesce to 1)",
            coalesce_mind.calls()
        ));
    }
    if coalesced != k {
        return Err(format!(
            "seed {seed}: {coalesced}/{k} requests coalesced in flight"
        ));
    }

    let produced = outcome.produced_artifact();
    Ok(ScenarioOutcome {
        produced,
        throttled: outcome.throttled,
        skipped: outcome.skipped_no_signals,
        content_hash: outcome.content_hash,
    })
}

#[test]
fn persona_runner_survives_budget_and_chaos_on_every_seed() {
    let scenarios: u64 = std::env::var("PERSONA_DST_SCENARIOS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let master: u64 = std::env::var("DST_MASTER_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            use fortuna_core::clock::Clock;
            fortuna_core::clock::RealClock.now().epoch_millis() as u64
        });
    println!("[persona-dst] master seed {master} -> {scenarios} scenario(s)");

    let mut master_rng = SplitMix64::new(master);
    let mut failures: Vec<(u64, String)> = Vec::new();
    let (mut produced, mut throttled, mut skipped) = (0u64, 0u64, 0u64);
    for _ in 0..scenarios {
        let seed = master_rng.next_u64();
        match run_scenario(seed) {
            Ok(first) => match run_scenario(seed) {
                // Invariant 5: byte-identical content_hash on replay.
                Ok(second) if second.content_hash == first.content_hash => {
                    produced += u64::from(first.produced);
                    throttled += u64::from(first.throttled);
                    skipped += u64::from(first.skipped);
                }
                Ok(_) => failures.push((seed, "content_hash differs on replay".to_string())),
                Err(e) => failures.push((seed, format!("replay errored: {e}"))),
            },
            Err(e) => failures.push((seed, e)),
        }
    }
    println!("[persona-dst] {produced} artifacts, {throttled} throttled, {skipped} skipped");
    assert!(failures.is_empty(), "persona DST violations: {failures:?}");
}
