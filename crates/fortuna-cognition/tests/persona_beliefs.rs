//! Track E E.4: belief consumption (design §9). Tests the μ/σ→p backbone helper
//! and the artifact→binary-belief fan-out (provenance citing the persona +
//! artifact, so a belief replays to it).

use fortuna_cognition::context::{
    assemble_context, content_hash_of, AssemblerConfig, ContextItem, SectionKind,
};
use fortuna_cognition::persona_beliefs::{
    domain_analysis_context_item, map_persona_analysis, normal_cdf, prob_at_least,
    PersonaBeliefError,
};
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;

fn h() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-12T23:00:00.000Z").unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

// ---- the μ/σ→p backbone (deterministic, §9) ----

#[test]
fn normal_cdf_matches_known_standard_normal_values() {
    assert!(close(normal_cdf(0.0), 0.5));
    assert!(close(normal_cdf(1.0), 0.841_3));
    assert!(close(normal_cdf(-1.0), 0.158_7));
    assert!(close(normal_cdf(1.96), 0.975_0));
}

#[test]
fn prob_at_least_reproduces_the_spike_backbone() {
    // μ=64.3, σ=3.1 (the §12 spike envelope). The backbone the runner feeds the
    // persona — the LLM never computes this.
    assert!(close(prob_at_least(60.0, 64.3, 3.1).unwrap(), 0.917));
    assert!(close(prob_at_least(65.0, 64.3, 3.1).unwrap(), 0.411));
    // Monotone non-increasing as the threshold rises.
    let p60 = prob_at_least(60.0, 64.3, 3.1).unwrap();
    let p70 = prob_at_least(70.0, 64.3, 3.1).unwrap();
    assert!(p60 > p70);
    // σ must be > 0.
    assert_eq!(prob_at_least(65.0, 64.3, 0.0), None);
}

// ---- the artifact → binary belief fan-out (§9) ----

fn weather_findings() -> serde_json::Value {
    json!({
        "thresholds": [{"ge": 60, "p": 0.92}, {"ge": 65, "p": 0.41}, {"ge": 70, "p": 0.08}],
        "sigma_trend": "tightening",
        "confidence": "high",
        "regime": "ridge",
        "key_risk": "onshore flow"
    })
}

#[test]
fn weather_thresholds_fan_out_to_one_binary_belief_each() {
    let drafts = map_persona_analysis(
        "meteorologist",
        3,
        "01JANALYSIS",
        "ch-abc",
        "weather:KNYC:tmax:2026-06-12",
        &weather_findings(),
        h(),
    )
    .unwrap();
    assert_eq!(drafts.len(), 3, "three brackets -> three binary beliefs");

    let b65 = drafts
        .iter()
        .find(|d| d.event_id == "weather:KNYC:tmax:2026-06-12#ge65")
        .expect("the >=65 bracket");
    assert!(
        (b65.p - 0.41).abs() < 1e-9,
        "belief p is the persona's stated p"
    );
    assert_eq!(b65.p, b65.p_raw);
    // Evidence cites the persona + the artifact.
    assert_eq!(b65.evidence[0]["source"], "persona:meteorologist@3");
    assert_eq!(b65.evidence[0]["ref"], "01JANALYSIS");
}

#[test]
fn the_provenance_carries_the_replay_anchor() {
    let drafts = map_persona_analysis(
        "meteorologist",
        3,
        "01JANALYSIS",
        "ch-abc",
        "weather:KNYC:tmax:2026-06-12",
        &weather_findings(),
        h(),
    )
    .unwrap();
    let p = &drafts[0].provenance;
    assert_eq!(p["persona_id"], "meteorologist");
    assert_eq!(p["persona_version"], 3);
    assert_eq!(p["analysis_id"], "01JANALYSIS");
    assert_eq!(p["analysis_content_hash"], "ch-abc");
}

#[test]
fn macro_outcomes_fan_out_to_binary_beliefs() {
    let findings = json!({
        "outcomes": [{"label": "MoM >= 0.3%", "p": 0.55}, {"label": "MoM >= 0.4%", "p": 0.20}],
        "regime": "disinflation stalling"
    });
    let drafts = map_persona_analysis(
        "macro-economist",
        1,
        "01JMACRO",
        "ch-m",
        "macro:US-CPI-MoM:2026-06-12",
        &findings,
        h(),
    )
    .unwrap();
    assert_eq!(drafts.len(), 2);
    assert!(drafts
        .iter()
        .all(|d| d.event_id.starts_with("macro:US-CPI-MoM:2026-06-12#")));
    assert_eq!(drafts[0].provenance["persona_id"], "macro-economist");
}

#[test]
fn an_out_of_range_probability_is_rejected() {
    // p must be in (0,1) — a degenerate 1.0 is a BadBelief, never persisted.
    let findings = json!({"thresholds": [{"ge": 60, "p": 1.0}]});
    let err = map_persona_analysis("m", 1, "a", "c", "r", &findings, h()).unwrap_err();
    assert!(matches!(err, PersonaBeliefError::BadBelief { .. }));
}

#[test]
fn empty_findings_are_an_error_not_a_silent_no_op() {
    let err =
        map_persona_analysis("m", 1, "a", "c", "r", &json!({"regime": "x"}), h()).unwrap_err();
    assert_eq!(err, PersonaBeliefError::EmptyFindings);
}

#[test]
fn a_malformed_entry_is_a_counted_error() {
    let findings = json!({"thresholds": [{"ge": 60}]}); // missing p
    let err = map_persona_analysis("m", 1, "a", "c", "r", &findings, h()).unwrap_err();
    assert!(matches!(err, PersonaBeliefError::BadEntry { index: 0, .. }));
}

#[test]
fn a_deep_tail_backbone_stays_a_valid_probability() {
    // P(high >= 40F) in a μ=64.3/σ=3.1 July world is ~0 — but must remain a VALID
    // belief probability strictly inside (0,1), not exactly 0.
    let p = prob_at_least(40.0, 64.3, 3.1).unwrap();
    assert!(
        p > 0.0 && p < 1.0,
        "deep-tail p must be a valid probability, got {p}"
    );
    let p_hi = prob_at_least(95.0, 64.3, 3.1).unwrap();
    assert!(p_hi > 0.0 && p_hi < 1.0);
}

#[test]
fn a_non_integer_threshold_renders_stably() {
    let findings = json!({"thresholds": [{"ge": 64.5, "p": 0.5}]});
    let drafts = map_persona_analysis("m", 1, "a", "c", "weather:r", &findings, h()).unwrap();
    assert_eq!(drafts[0].event_id, "weather:r#ge64.5");
}

#[test]
fn thresholds_and_outcomes_together_never_collide_on_an_event_id() {
    // A threshold ge65 and an outcome literally labelled "ge65" must NOT share an
    // event_id (the `ge`/`out:` prefixes keep them apart).
    let findings = json!({
        "thresholds": [{"ge": 65, "p": 0.4}],
        "outcomes": [{"label": "ge65", "p": 0.6}]
    });
    let drafts = map_persona_analysis("m", 1, "a", "c", "r", &findings, h()).unwrap();
    assert_eq!(drafts.len(), 2);
    let ids: std::collections::BTreeSet<_> = drafts.iter().map(|d| d.event_id.clone()).collect();
    assert_eq!(ids.len(), 2, "the two event_ids must be distinct");
    assert!(ids.contains("r#ge65"));
    assert!(ids.contains("r#out:ge65"));
}

#[test]
fn duplicate_entries_are_rejected_not_silently_emitted() {
    let findings = json!({"thresholds": [{"ge": 65, "p": 0.4}, {"ge": 65, "p": 0.6}]});
    let err = map_persona_analysis("m", 1, "a", "c", "r", &findings, h()).unwrap_err();
    assert!(matches!(err, PersonaBeliefError::DuplicateEvent { .. }));
}

// ---- E.4b: the artifact as a high-priority context item (design §9) ----

#[test]
fn the_domain_analysis_section_is_high_priority_just_under_open_beliefs() {
    // Packed just under OpenBeliefs, ahead of every lower-priority section.
    assert!(SectionKind::OpenBeliefs < SectionKind::DomainAnalysis);
    assert!(SectionKind::DomainAnalysis < SectionKind::MarketSnapshot);
    assert!(SectionKind::DomainAnalysis < SectionKind::FreshSignals);
    assert!(SectionKind::DomainAnalysis < SectionKind::Lessons);
    assert!(SectionKind::DomainAnalysis < SectionKind::Episodic);
    // The pre-existing high-priority order is preserved by the insertion.
    assert!(SectionKind::Charter < SectionKind::AccountState);
    assert!(SectionKind::AccountState < SectionKind::OpenBeliefs);
}

#[test]
fn a_domain_analysis_context_item_carries_the_findings_and_the_replay_anchor() {
    let findings = json!({"thresholds": [{"ge": 65, "p": 0.41}], "regime": "ridge"});
    let item = domain_analysis_context_item(
        "meteorologist",
        3,
        "01JANALYSIS",
        "weather:KNYC:tmax:2026-06-12",
        &findings,
        "ch-anchor",
        h(),
    );
    assert_eq!(item.section, SectionKind::DomainAnalysis);
    assert_eq!(item.item_id, "01JANALYSIS");
    // The item hash follows the assembler convention (hash of the body); the
    // artifact's replay anchor rides IN the body.
    assert_eq!(item.content_hash, content_hash_of(&item.body));
    assert!(
        item.body.contains("artifact ch-anchor"),
        "the replay anchor is in the body"
    );
    assert!(item.body.contains("persona:meteorologist@3"));
    assert!(item.body.contains("ridge"));
}

#[test]
fn a_domain_analysis_item_renders_as_data_and_packs_before_signals() {
    let trigger = h();
    let earlier = UtcTimestamp::parse_iso8601("2026-06-12T22:00:00.000Z").unwrap();
    let analysis = {
        let mut it = domain_analysis_context_item(
            "meteorologist",
            3,
            "01JA",
            "r",
            &json!({"k": "v"}),
            "ch",
            earlier,
        );
        it.at = earlier;
        it
    };
    let signal = ContextItem {
        item_id: "sig".to_string(),
        section: SectionKind::FreshSignals,
        body: "raw signal body".to_string(),
        content_hash: content_hash_of("raw signal body"),
        at: earlier,
    };
    let assembler = AssemblerConfig {
        budget_chars: 100_000,
        anonymize: false,
    };
    let ctx =
        assemble_context(&[signal, analysis], trigger, "persona-consume", &assembler).unwrap();
    // Rendered as a delimited DATA block under its section, BEFORE the raw signals.
    let da = ctx
        .rendered
        .find("domain_analysis")
        .expect("domain_analysis section");
    let fs = ctx
        .rendered
        .find("fresh_signals")
        .expect("fresh_signals section");
    assert!(
        da < fs,
        "the domain analysis packs ahead of the raw signals"
    );
    // The domain-analysis artifact specifically is wrapped as a delimited DATA
    // block (injection hygiene, 5.11) — not raw prose.
    assert!(ctx
        .rendered
        .contains("<context-item id=\"01JA\" section=\"domain_analysis\">"));
}
