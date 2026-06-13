//! Track E E.3 (telemetry, design §19): persona-runner counters.
//!
//! A `PersonaCounters` fold (keyed by persona) accumulates `PersonaOutcome`s into
//! the operator-facing funnel — runs → analyses, with the degrade counters
//! (budget skips, no-signal skips, run failures by reason, coalesced) explaining
//! every drop. `samples()` returns `PersonaMetricSample`s whose shape mirrors the
//! runner's `MetricSample` (name/help/counter/labels/value), so the composition
//! drains them into `fortuna-ops`'s integer-only `MetricsRegistry` through the
//! SAME loop it uses for `metrics_export()` — no new telemetry infrastructure.
//!
//! Integer-only (design §19): counts and cents go here (Prometheus); the float
//! scorecard (Brier/quality) stays in the ROTA JSON, never these counters. Labels
//! are persona-agnostic, so a new persona emits the same metric names.
//!
//! Accounting identity (test-pinned): for each persona,
//! `runs == analyses + budget_skips + no_signal_skips + sum(failures)`.

use crate::persona_runner::PersonaOutcome;
use fortuna_core::clock::UtcTimestamp;
use std::collections::BTreeMap;

const MILLIS_PER_DAY: i64 = 86_400_000;

/// One metric sample — shape-compatible with the runner's `MetricSample` so the
/// composition can map it field-for-field into the ops registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaMetricSample {
    pub name: &'static str,
    pub help: &'static str,
    pub counter: bool,
    pub labels: Vec<(String, String)>,
    pub value: i64,
}

#[derive(Debug, Clone, Default)]
struct Acc {
    domain: String,
    runs: i64,
    analyses: i64,
    budget_skips: i64,
    no_signal_skips: i64,
    coalesced: i64,
    cost_cents: i64,
    /// Budget-true spend since the last UTC midnight (the gauge); resets on a
    /// day roll, mirroring `fortuna_mind_spend_today_cents`.
    spend_today: i64,
    spend_day: i64,
    /// reason -> count; reason in {"provider","schema_invalid","other"}.
    failures: BTreeMap<&'static str, i64>,
}

/// Persona-runner telemetry fold (design §19). Deterministic ordering via
/// `BTreeMap` (a byte-identical render on identical input).
#[derive(Debug, Clone, Default)]
pub struct PersonaCounters {
    per: BTreeMap<String, Acc>,
}

/// Classify a failed run's reason from its defects (the §19 `reason` label).
fn classify_failure(defects: &[String]) -> &'static str {
    if defects.iter().any(|d| d.contains("mind failed")) {
        "provider"
    } else if defects.iter().any(|d| {
        d.contains("schema violation")
            || d.contains("violated the contract")
            || d.contains("no findings journal")
    }) {
        "schema_invalid"
    } else {
        "other"
    }
}

impl PersonaCounters {
    pub fn new() -> PersonaCounters {
        PersonaCounters::default()
    }

    /// Fold one persona run into the counters. `domain` is the persona's domain
    /// (the metric label), taken from the persona definition by the caller; `now`
    /// is the injected Clock time (rolls the daily spend gauge).
    pub fn observe(&mut self, outcome: &PersonaOutcome, domain: &str, now: UtcTimestamp) {
        let acc = self.per.entry(outcome.persona_id.clone()).or_default();
        if acc.domain.is_empty() {
            acc.domain = domain.to_string();
        }
        acc.runs += 1;
        acc.cost_cents = acc.cost_cents.saturating_add(outcome.cost_cents);
        // Daily spend gauge: reset on a UTC-day roll, then accrue.
        let day = now.epoch_millis().div_euclid(MILLIS_PER_DAY);
        if acc.spend_day != day {
            acc.spend_day = day;
            acc.spend_today = 0;
        }
        acc.spend_today = acc.spend_today.saturating_add(outcome.cost_cents);
        if outcome.throttled {
            acc.budget_skips += 1;
        } else if outcome.skipped_no_signals {
            acc.no_signal_skips += 1;
        } else if outcome.produced_artifact() {
            acc.analyses += 1;
        } else {
            let reason = classify_failure(&outcome.defects);
            *acc.failures.entry(reason).or_insert(0) += 1;
        }
    }

    /// Record triggers that coalesced into an in-flight run (from the trigger
    /// gate's reported count). Does NOT count as a run.
    pub fn observe_coalesced(&mut self, persona_id: &str, domain: &str, count: u64) {
        let acc = self.per.entry(persona_id.to_string()).or_default();
        if acc.domain.is_empty() {
            acc.domain = domain.to_string();
        }
        acc.coalesced = acc.coalesced.saturating_add(count as i64);
    }

    /// The metric samples (design §19 names). Deterministic order.
    pub fn samples(&self) -> Vec<PersonaMetricSample> {
        let mut out = Vec::new();
        for (persona, acc) in &self.per {
            let pd = vec![
                ("persona".to_string(), persona.clone()),
                ("domain".to_string(), acc.domain.clone()),
            ];
            let p = vec![("persona".to_string(), persona.clone())];
            out.push(counter(
                "fortuna_persona_runs_total",
                "persona runs (a trigger fired a run)",
                pd.clone(),
                acc.runs,
            ));
            out.push(counter(
                "fortuna_persona_analyses_total",
                "persona analyses persisted",
                pd,
                acc.analyses,
            ));
            out.push(counter(
                "fortuna_persona_budget_skips_total",
                "persona runs throttled by the cost budget",
                p.clone(),
                acc.budget_skips,
            ));
            out.push(counter(
                "fortuna_persona_no_signal_skips_total",
                "persona runs skipped (no in-window signals)",
                p.clone(),
                acc.no_signal_skips,
            ));
            out.push(counter(
                "fortuna_persona_triggers_coalesced_total",
                "persona triggers coalesced into an in-flight run",
                p.clone(),
                acc.coalesced,
            ));
            out.push(counter(
                "fortuna_persona_cost_cents_total",
                "persona-attributed cognition spend",
                p.clone(),
                acc.cost_cents,
            ));
            out.push(PersonaMetricSample {
                name: "fortuna_persona_spend_today_cents",
                help: "persona-attributed spend today (resets 00:00 UTC)",
                counter: false,
                labels: p,
                value: acc.spend_today,
            });
            for (reason, n) in &acc.failures {
                out.push(counter(
                    "fortuna_persona_run_failures_total",
                    "persona runs that reached the mind but failed",
                    vec![
                        ("persona".to_string(), persona.clone()),
                        ("reason".to_string(), (*reason).to_string()),
                    ],
                    *n,
                ));
            }
        }
        out
    }
}

fn counter(
    name: &'static str,
    help: &'static str,
    labels: Vec<(String, String)>,
    value: i64,
) -> PersonaMetricSample {
    PersonaMetricSample {
        name,
        help,
        counter: true,
        labels,
        value,
    }
}
