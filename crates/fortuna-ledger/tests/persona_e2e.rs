//! Track E E.6: the end-to-end meteorologist proof (design §9/§10/§11, proven on
//! the real ledger). One scenario wires the WHOLE persona pipeline:
//!   registry insert -> hash-bound loader -> runner (scripted StubMind) ->
//!   persist domain_analyses -> fan-out to binary beliefs -> persist events +
//!   beliefs -> resolve + score -> persona promote/retire proposal.
//! It asserts the belief REPLAYS to the persisted artifact (provenance.analysis_id
//! -> a domain_analyses row), the §11 evaluation gate is zero-capital at low n,
//! and the trusted method never leaves the file. No daemon, no live endpoint —
//! a scripted StubMind stands in for the model (the §12 spike de-risked the live shape).

use fortuna_cognition::context::{content_hash_of, ContextItem, SectionKind};
use fortuna_cognition::discovery::DiscoveryBudget;
use fortuna_cognition::mind::{MindOutput, StubMind};
use fortuna_cognition::persona::{PersonaDef, RegistryHead};
use fortuna_cognition::persona_beliefs::map_persona_analysis;
use fortuna_cognition::persona_runner::run_persona_analysis;
use fortuna_cognition::persona_scoring::{
    propose_promotion, score_persona, Baseline, PersonaScope, PersonaScopeRecord, PromotionVerdict,
};
use fortuna_core::clock::UtcTimestamp;
use fortuna_ledger::{BeliefsRepo, DomainAnalysesRepo, EventsRepo, PersonasRepo};
use serde_json::json;
use sqlx::PgPool;

fn ts(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(s).unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn meteorologist_end_to_end_registry_to_scored_beliefs(pool: PgPool) {
    // ---- 1. Load the SHIPPED persona + register it, method_hash-bound. ----
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/personas/meteorologist"
    );
    let md = std::fs::read_to_string(format!("{dir}/persona.md")).unwrap();
    let schema = std::fs::read_to_string(format!("{dir}/schema.json")).unwrap();
    let def = PersonaDef::parse(&md, &schema).unwrap();
    let method_hash = content_hash_of(&md);

    let personas = PersonasRepo::new(pool.clone());
    personas
        .insert(
            "p-1",
            &def.meta.id,
            def.meta.version,
            &def.meta.domain,
            &json!(def.meta.domain_tags),
            &json!(def.meta.reads_signal_kinds),
            &def.meta.tier,
            &method_hash,
            &def.meta.output_schema_version,
            "active",
            None,
            "2026-06-13T00:00:00.000Z",
            "2026-06-13T00:00:00.000Z",
        )
        .await
        .unwrap();

    // The loader validates against the registry HEAD — the hash binds (§6).
    let head = personas.head(&def.meta.id).await.unwrap().unwrap();
    def.validate_against(Some(&RegistryHead {
        version: head.version,
        method_hash: head.method_hash.clone(),
        status: head.status.clone(),
    }))
    .expect("the shipped file's hash matches the registered row");

    // ---- 2. Run the persona (scripted StubMind = the §12 spike findings). ----
    let region = "weather:KNYC:tmax:2026-06-12";
    let signal_body = "Aeolus envelope KNYC tmax 2026-06-12: mu=64.3 sigma=3.1";
    let signals = vec![ContextItem {
        item_id: "aeolus-knyc".to_string(),
        section: SectionKind::FreshSignals,
        body: signal_body.to_string(),
        content_hash: content_hash_of(signal_body),
        at: ts("2026-06-12T04:00:00.000Z"),
    }];
    let findings = json!({
        "thresholds": [{"ge": 60, "p": 0.92}, {"ge": 65, "p": 0.41}, {"ge": 70, "p": 0.08}],
        "sigma_trend": "tightening", "confidence": "high",
        "regime": "stagnant upper ridge", "key_risk": "onshore flow backdoor front near 21Z"
    });
    let scripted: MindOutput = serde_json::from_value(json!({
        "beliefs": [], "proposals": [],
        "journal": {"body": findings.to_string()}, "cost_cents": 1
    }))
    .unwrap();
    let mind = StubMind::scripted(vec![scripted]);
    let mut budget = DiscoveryBudget::new(100);
    let outcome = run_persona_analysis(
        &def,
        region,
        &signals,
        &mind,
        &mut budget,
        ts("2026-06-12T05:00:00.000Z"),
    )
    .await
    .unwrap();
    assert!(outcome.produced_artifact(), "the run produced an artifact");

    // ---- 3. Persist the artifact (the composition's persist step). ----
    let analyses = DomainAnalysesRepo::new(pool.clone());
    let analysis_id = "01JMETEOEANALYSIS";
    let content_hash = outcome.content_hash.clone().unwrap();
    analyses
        .insert(
            analysis_id,
            &outcome.persona_id,
            outcome.persona_version,
            &def.meta.domain,
            &outcome.region_key,
            &outcome.produced_at.to_iso8601(),
            &serde_json::to_value(&outcome.signal_manifest).unwrap(),
            outcome.findings.as_ref().unwrap(),
            &content_hash,
            outcome.manifest_hash.as_deref().unwrap(),
            outcome.cost_cents,
            None,
            "2026-06-12T05:00:00.000Z",
        )
        .await
        .unwrap();
    // The content_hash is the replay anchor — the persisted row round-trips it.
    assert_eq!(
        analyses.get(analysis_id).await.unwrap().content_hash,
        content_hash
    );

    // ---- 4. Fan out to BINARY beliefs citing the persisted artifact. ----
    let drafts = map_persona_analysis(
        &outcome.persona_id,
        outcome.persona_version,
        analysis_id,
        &content_hash,
        region,
        outcome.findings.as_ref().unwrap(),
        ts("2026-06-12T23:00:00.000Z"),
    )
    .unwrap();
    assert_eq!(drafts.len(), 3, "three brackets -> three binary beliefs");

    // ---- 5. Persist events + beliefs (provenance = the replay anchor). ----
    let events = EventsRepo::new(pool.clone());
    let beliefs = BeliefsRepo::new(pool.clone());
    for (i, d) in drafts.iter().enumerate() {
        events
            .create(
                &d.event_id,
                &format!("NYC high bracket {i}"),
                "NWS daily climate report",
                "nws",
                Some("2026-06-12T23:00:00.000Z"),
                "2026-06-11T10:00:00.000Z",
                "weather",
                "2026-06-11T10:00:00.000Z",
            )
            .await
            .unwrap();
        beliefs
            .insert(
                &format!("b-{i}"),
                "2026-06-11T10:00:00.000Z",
                &d.event_id,
                d.p,
                d.p_raw,
                "2026-06-12T23:00:00.000Z",
                &d.evidence,
                &d.provenance,
                None,
            )
            .await
            .unwrap();
    }

    // EVERY persona belief REPLAYS to the persisted artifact: its provenance carries
    // the persona, the analysis_id, AND the content_hash (the I5/5.7 replay anchor),
    // and that domain_analyses row exists + round-trips the same content_hash.
    let recent = beliefs.recent(10).await.unwrap();
    assert_eq!(recent.len(), 3, "all three persona beliefs persisted");
    for b in &recent {
        assert_eq!(b.provenance["persona_id"], "meteorologist");
        assert_eq!(b.provenance["persona_version"], 2); // v2 after D3 persona.md bump
        let cited = b.provenance["analysis_id"].as_str().unwrap();
        assert_eq!(cited, analysis_id);
        assert_eq!(
            b.provenance["analysis_content_hash"].as_str().unwrap(),
            content_hash,
            "the content-hash anchor survives fan-out -> persist -> read-back"
        );
        assert_eq!(
            analyses.get(cited).await.unwrap().content_hash,
            content_hash
        );
    }

    // ---- 6. Resolve + score (the high verified >=60 only). ----
    let outcomes = [true, false, false];
    for (i, o) in outcomes.iter().enumerate() {
        let target = f64::from(u8::from(*o));
        let brier = (drafts[i].p - target) * (drafts[i].p - target);
        beliefs
            .resolve_and_score(&format!("b-{i}"), *o, brier, Some(50.0))
            .await
            .unwrap();
    }
    let stats = beliefs.resolved_stats("weather").await.unwrap();
    assert_eq!(stats.len(), 3);

    // ---- 7. Score the (persona, version) scope + propose. ----
    let record = PersonaScopeRecord {
        scope: PersonaScope {
            persona_id: outcome.persona_id.clone(),
            persona_version: outcome.persona_version,
        },
        samples: stats.iter().map(|s| (s.p, s.outcome)).collect(),
        clv_bps: stats.iter().filter_map(|s| s.clv_bps).collect(),
    };
    let card = score_persona(&record);
    assert_eq!(card.n, 3);
    let proposal = propose_promotion(
        &card,
        None,
        Baseline { brier_mean: 0.5 },
        Baseline { brier_mean: 0.5 },
        60,
    );
    // §11: zero-capital below the evaluation floor — no premature promotion.
    assert_eq!(
        proposal.verdict,
        PromotionVerdict::Evaluating {
            resolved: 3,
            needed: 60
        }
    );

    // Sanity (persist-path only): the persisted artifact (findings + signal_manifest)
    // carries NONE of the method-body text (distinctive persona.md phrases) — the
    // persist step stores the model output verbatim, it never injects the method.
    // NOTE: the §4 firewall PROPER — the method rides in the Mind SYSTEM MESSAGE,
    // never the context-data path, and a planted injection is ignored — is proven in
    // fortuna-cognition/tests/persona_runner.rs's SpyMind tests; a StubMind here
    // cannot exercise it, so this is only the persistence-boundary check.
    let persisted = analyses.get(analysis_id).await.unwrap();
    let artifact_text = format!("{}{}", persisted.findings, persisted.signal_manifest);
    assert!(!artifact_text.contains("operational meteorologist"));
    assert!(!artifact_text.contains("Trust and safety"));
}
