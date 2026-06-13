//! Track E E.3 (telemetry, design §19): PersonaCounters fold tests. Proves the
//! funnel accounting (runs = analyses + budget_skips + no_signal_skips +
//! failures), the per-reason failure label (incl. the defensive "other"), the
//! daily spend gauge (resets on a UTC-day roll), the metric shapes/labels, and
//! deterministic samples.

use fortuna_cognition::persona_metrics::{PersonaCounters, PersonaMetricSample};
use fortuna_cognition::persona_runner::PersonaOutcome;
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;

fn day1() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T05:00:00.000Z").unwrap()
}

fn day2() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-13T05:00:00.000Z").unwrap()
}

/// Build a PersonaOutcome with the fields the counters read.
fn outcome(
    persona: &str,
    throttled: bool,
    skipped: bool,
    produced: bool,
    cost: i64,
    defects: Vec<&str>,
) -> PersonaOutcome {
    PersonaOutcome {
        persona_id: persona.to_string(),
        persona_version: 1,
        region_key: "r".to_string(),
        produced_at: day1(),
        signal_manifest: vec![],
        findings: produced.then(|| json!({"verdict": "ok"})),
        content_hash: produced.then(|| "hash".to_string()),
        manifest_hash: Some("m".to_string()),
        cost_cents: cost,
        throttled,
        skipped_no_signals: skipped,
        defects: defects.into_iter().map(str::to_string).collect(),
    }
}

fn value(samples: &[PersonaMetricSample], name: &str) -> i64 {
    samples
        .iter()
        .filter(|s| s.name == name)
        .map(|s| s.value)
        .sum()
}

fn value_where(samples: &[PersonaMetricSample], name: &str, key: &str, val: &str) -> i64 {
    samples
        .iter()
        .filter(|s| s.name == name && s.labels.iter().any(|(k, v)| k == key && v == val))
        .map(|s| s.value)
        .sum()
}

#[test]
fn a_valid_run_counts_a_run_an_analysis_and_cost() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome("meteo", false, false, true, 7, vec![]),
        "weather",
        day1(),
    );
    let s = c.samples();
    assert_eq!(value(&s, "fortuna_persona_runs_total"), 1);
    assert_eq!(value(&s, "fortuna_persona_analyses_total"), 1);
    assert_eq!(value(&s, "fortuna_persona_cost_cents_total"), 7);
    assert_eq!(value(&s, "fortuna_persona_budget_skips_total"), 0);
    // runs_total carries {persona, domain}.
    let runs = s
        .iter()
        .find(|x| x.name == "fortuna_persona_runs_total")
        .unwrap();
    assert!(runs
        .labels
        .contains(&("persona".to_string(), "meteo".to_string())));
    assert!(runs
        .labels
        .contains(&("domain".to_string(), "weather".to_string())));
    assert!(runs.counter);
}

#[test]
fn a_throttled_run_counts_a_run_and_a_budget_skip_not_an_analysis() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome("meteo", true, false, false, 0, vec![]),
        "weather",
        day1(),
    );
    let s = c.samples();
    assert_eq!(value(&s, "fortuna_persona_runs_total"), 1);
    assert_eq!(value(&s, "fortuna_persona_budget_skips_total"), 1);
    assert_eq!(value(&s, "fortuna_persona_analyses_total"), 0);
}

#[test]
fn a_no_signal_run_counts_a_skip() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome("meteo", false, true, false, 0, vec![]),
        "weather",
        day1(),
    );
    let s = c.samples();
    assert_eq!(value(&s, "fortuna_persona_no_signal_skips_total"), 1);
    assert_eq!(value(&s, "fortuna_persona_analyses_total"), 0);
}

#[test]
fn a_mind_failure_counts_a_provider_run_failure() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome("meteo", false, false, false, 3, vec!["mind failed: boom"]),
        "weather",
        day1(),
    );
    let s = c.samples();
    assert_eq!(
        value_where(
            &s,
            "fortuna_persona_run_failures_total",
            "reason",
            "provider"
        ),
        1
    );
    assert_eq!(
        value_where(
            &s,
            "fortuna_persona_run_failures_total",
            "reason",
            "schema_invalid"
        ),
        0
    );
}

#[test]
fn schema_and_empty_journal_failures_count_as_schema_invalid() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome(
            "meteo",
            false,
            false,
            false,
            3,
            vec!["findings schema violation: unknown field 'x'"],
        ),
        "weather",
        day1(),
    );
    // The "no findings journal" defect also classifies as schema_invalid.
    c.observe(
        &outcome(
            "meteo",
            false,
            false,
            false,
            3,
            vec!["persona produced no findings journal"],
        ),
        "weather",
        day1(),
    );
    let s = c.samples();
    assert_eq!(
        value_where(
            &s,
            "fortuna_persona_run_failures_total",
            "reason",
            "schema_invalid"
        ),
        2
    );
}

#[test]
fn an_unrecognized_failure_defect_classifies_as_other() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome(
            "meteo",
            false,
            false,
            false,
            1,
            vec!["something completely unexpected"],
        ),
        "weather",
        day1(),
    );
    let s = c.samples();
    assert_eq!(
        value_where(&s, "fortuna_persona_run_failures_total", "reason", "other"),
        1
    );
}

#[test]
fn coalesced_triggers_are_counted_separately_from_runs() {
    let mut c = PersonaCounters::new();
    c.observe_coalesced("meteo", "weather", 3);
    let s = c.samples();
    assert_eq!(value(&s, "fortuna_persona_triggers_coalesced_total"), 3);
    assert_eq!(value(&s, "fortuna_persona_runs_total"), 0);
}

#[test]
fn the_spend_today_gauge_accrues_and_resets_at_the_utc_day_roll() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome("meteo", false, false, true, 5, vec![]),
        "weather",
        day1(),
    );
    c.observe(
        &outcome("meteo", false, false, true, 4, vec![]),
        "weather",
        day1(),
    );
    let s = c.samples();
    let gauge = s
        .iter()
        .find(|x| x.name == "fortuna_persona_spend_today_cents")
        .unwrap();
    assert!(!gauge.counter, "spend_today is a gauge, not a counter");
    assert_eq!(gauge.value, 9, "accrues within the day");
    // cumulative counter keeps climbing; the gauge resets next day.
    c.observe(
        &outcome("meteo", false, false, true, 2, vec![]),
        "weather",
        day2(),
    );
    let s = c.samples();
    assert_eq!(
        value(&s, "fortuna_persona_spend_today_cents"),
        2,
        "gauge reset on day roll"
    );
    assert_eq!(
        value(&s, "fortuna_persona_cost_cents_total"),
        11,
        "counter unaffected by the roll"
    );
}

#[test]
fn the_funnel_accounting_identity_holds() {
    // runs == analyses + budget_skips + no_signal_skips + failures.
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome("m", false, false, true, 1, vec![]),
        "weather",
        day1(),
    );
    c.observe(
        &outcome("m", false, false, true, 1, vec![]),
        "weather",
        day1(),
    );
    c.observe(
        &outcome("m", true, false, false, 0, vec![]),
        "weather",
        day1(),
    );
    c.observe(
        &outcome("m", false, true, false, 0, vec![]),
        "weather",
        day1(),
    );
    c.observe(
        &outcome("m", false, false, false, 2, vec!["mind failed"]),
        "weather",
        day1(),
    );
    c.observe(
        &outcome(
            "m",
            false,
            false,
            false,
            2,
            vec!["findings schema violation"],
        ),
        "weather",
        day1(),
    );
    let s = c.samples();
    let runs = value(&s, "fortuna_persona_runs_total");
    let parts = value(&s, "fortuna_persona_analyses_total")
        + value(&s, "fortuna_persona_budget_skips_total")
        + value(&s, "fortuna_persona_no_signal_skips_total")
        + value(&s, "fortuna_persona_run_failures_total");
    assert_eq!(runs, 6);
    assert_eq!(runs, parts, "the funnel must account for every run");
}

#[test]
fn two_personas_emit_independent_label_sets_deterministically() {
    let mut c = PersonaCounters::new();
    c.observe(
        &outcome("meteo", false, false, true, 1, vec![]),
        "weather",
        day1(),
    );
    c.observe(
        &outcome("macro", false, false, true, 2, vec![]),
        "macro",
        day1(),
    );
    let s1 = c.samples();
    let s2 = c.samples();
    assert_eq!(s1, s2, "samples are deterministic");
    assert_eq!(
        value_where(&s1, "fortuna_persona_cost_cents_total", "persona", "meteo"),
        1
    );
    assert_eq!(
        value_where(&s1, "fortuna_persona_cost_cents_total", "persona", "macro"),
        2
    );
}
