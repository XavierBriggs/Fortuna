//! T3.3: the shadow-mode model comparison harness (spec Section 11).
//!
//! Doctrine under test:
//! - The challenger runs full decision cycles in SHADOW on the IDENTICAL
//!   AssembledContext as the incumbent — the pairing key is the manifest
//!   hash, making "identical context" provable, not asserted.
//! - Shadow runs are STRUCTURALLY zero-orders: the run type carries
//!   beliefs only. Beliefs are stamped with the challenger's model_id
//!   and scored like any others (both belief ledgers make the comparison
//!   direct and fair).
//! - Shadow operates under its OWN budget (never the live budget) and
//!   may SAMPLE cycles (deterministic first-K per UTC day); a throttled
//!   or unsampled cycle shadows nothing and costs nothing.
//! - evaluate_model_swap is the I7 gate: a Promote RECOMMENDATION exists
//!   only with >= 30 resolved PAIRED beliefs per active category AND
//!   challenger Brier <= incumbent AND CLV >= incumbent where measured.
//!   No record, no promotion. The operator applies any swap.
//!
//! Written BEFORE src/shadow.rs per the repository TDD doctrine.

use fortuna_cognition::context::{assemble_context, AssemblerConfig};
use fortuna_cognition::mind::{MindError, MindOutput, StubMind};
use fortuna_cognition::shadow::{
    evaluate_model_swap, PairedScore, ShadowHarness, ShadowHarnessConfig, SwapThresholds,
    SwapVerdict,
};
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;

fn t(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

fn ctx(now: UtcTimestamp) -> fortuna_cognition::context::AssembledContext {
    assemble_context(
        &[],
        now,
        "decision",
        &AssemblerConfig {
            budget_chars: 10_000,
            anonymize: false,
        },
    )
    .unwrap()
}

fn belief_output(p: f64) -> MindOutput {
    serde_json::from_value(json!({
        "beliefs": [{
            "event_id": "evt-1",
            "p": p,
            "p_raw": p,
            "horizon": "2026-06-20T18:00:00.000Z",
            "evidence": [{"source": "shadow", "ref": "sig-1"}]
        }],
        "proposals": [],
        "journal": null
    }))
    .unwrap()
}

fn harness(daily_quota: u32, day_cap_cents: i64) -> ShadowHarness {
    ShadowHarness::new(ShadowHarnessConfig {
        challenger_model_id: "claude-opus-9".to_string(),
        daily_sample_quota: daily_quota,
        per_cycle_cap_cents: 100,
        per_day_cap_cents: day_cap_cents,
    })
}

// ----------------------------------------------------------- shadow runs

#[tokio::test]
async fn shadow_runs_pair_by_manifest_and_stamp_the_challenger() {
    let challenger = StubMind::scripted(vec![belief_output(0.7)]);
    let mut shadow = harness(5, 1_000);
    let now = t("2026-06-11T12:00:00.000Z");
    let context = ctx(now);

    let run = shadow
        .maybe_shadow(&challenger, &context, now)
        .await
        .unwrap()
        .expect("first cycle of the day samples");

    // The pairing key IS the incumbent's context manifest hash.
    assert_eq!(run.manifest_hash, context.manifest_hash);
    assert_eq!(run.beliefs.len(), 1);
    // Harness stamps the challenger identity (fair attribution).
    assert_eq!(run.beliefs[0].provenance["model_id"], "claude-opus-9");
    assert_eq!(
        run.beliefs[0].provenance["context_manifest_hash"],
        serde_json::Value::String(context.manifest_hash.clone())
    );
    assert_eq!(
        run.beliefs[0].provenance["shadow"],
        serde_json::Value::Bool(true)
    );
    // ZERO ORDERS is structural: ShadowRun has beliefs, hash, cost —
    // no field can carry a proposal or an order.
}

#[tokio::test]
async fn shadow_samples_first_k_per_day_and_respects_its_own_budget() {
    let challenger = StubMind::scripted(vec![
        belief_output(0.7),
        belief_output(0.6),
        belief_output(0.5),
    ]);
    let mut shadow = harness(2, 1_000);

    let d1 = t("2026-06-11T08:00:00.000Z");
    assert!(shadow
        .maybe_shadow(&challenger, &ctx(d1), d1)
        .await
        .unwrap()
        .is_some());
    let d1b = t("2026-06-11T09:00:00.000Z");
    assert!(shadow
        .maybe_shadow(&challenger, &ctx(d1b), d1b)
        .await
        .unwrap()
        .is_some());
    let d1c = t("2026-06-11T10:00:00.000Z");
    assert!(
        shadow
            .maybe_shadow(&challenger, &ctx(d1c), d1c)
            .await
            .unwrap()
            .is_none(),
        "daily sample quota of 2 exhausted"
    );
    // New UTC day: quota resets.
    let d2 = t("2026-06-12T00:00:01.000Z");
    assert!(shadow
        .maybe_shadow(&challenger, &ctx(d2), d2)
        .await
        .unwrap()
        .is_some());

    // Budget exhausted: throttled BEFORE the call, not an error.
    let challenger = StubMind::scripted(vec![belief_output(0.7)]);
    let mut broke = harness(5, 0);
    let now = t("2026-06-11T12:00:00.000Z");
    assert!(broke
        .maybe_shadow(&challenger, &ctx(now), now)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn shadow_failure_is_counted_never_fatal() {
    struct FailingMind;
    #[async_trait::async_trait]
    impl fortuna_cognition::mind::Mind for FailingMind {
        fn id(&self) -> &str {
            "failing"
        }
        async fn decide(
            &self,
            _ctx: &fortuna_cognition::context::AssembledContext,
        ) -> Result<MindOutput, MindError> {
            Err(MindError::Provider {
                reason: "529".to_string(),
            })
        }
    }
    let mut shadow = harness(5, 1_000);
    let now = t("2026-06-11T12:00:00.000Z");
    let run = shadow.maybe_shadow(&FailingMind, &ctx(now), now).await;
    assert!(run.is_ok(), "challenger failure never breaks the live loop");
    assert!(run.unwrap().is_none());
    assert_eq!(shadow.failures(), 1);
}

// ------------------------------------------------------------- swap gate

fn paired(category: &str, n: usize, inc_brier: f64, ch_brier: f64) -> Vec<PairedScore> {
    (0..n)
        .map(|i| PairedScore {
            category: category.to_string(),
            manifest_hash: format!("hash-{category}-{i}"),
            incumbent_brier: inc_brier,
            challenger_brier: ch_brier,
            incumbent_clv_bps: Some(50.0),
            challenger_clv_bps: Some(60.0),
        })
        .collect()
}

fn thresholds() -> SwapThresholds {
    SwapThresholds {
        min_resolved_per_category: 30,
    }
}

#[test]
fn no_record_no_promotion() {
    let eval = evaluate_model_swap(&[], &["weather".to_string()], &thresholds());
    assert_eq!(eval.verdict, SwapVerdict::Hold);
    assert!(eval.reasons.iter().any(|r| r.contains("0")));
}

#[test]
fn promotion_needs_every_active_category_qualified() {
    let mut records = paired("weather", 40, 0.20, 0.15);
    // Politics has too few paired resolutions.
    records.extend(paired("politics", 10, 0.25, 0.20));
    let active = vec!["weather".to_string(), "politics".to_string()];

    let eval = evaluate_model_swap(&records, &active, &thresholds());
    assert_eq!(eval.verdict, SwapVerdict::Hold);
    assert!(eval
        .reasons
        .iter()
        .any(|r| r.contains("politics") && r.contains("10")));

    // With enough politics pairs, the challenger qualifies everywhere.
    let mut records = paired("weather", 40, 0.20, 0.15);
    records.extend(paired("politics", 35, 0.25, 0.20));
    let eval = evaluate_model_swap(&records, &active, &thresholds());
    assert_eq!(eval.verdict, SwapVerdict::PromoteRecommended);
    // The evaluation is evidence, not action: per-category means ride
    // along for the operator.
    assert_eq!(eval.categories.len(), 2);
    assert!(eval.categories.iter().all(|c| c.qualified));
}

#[test]
fn worse_brier_or_clv_holds() {
    // Challenger Brier worse: hold.
    let records = paired("weather", 40, 0.15, 0.20);
    let eval = evaluate_model_swap(&records, &["weather".to_string()], &thresholds());
    assert_eq!(eval.verdict, SwapVerdict::Hold);
    assert!(eval.reasons.iter().any(|r| r.contains("Brier")));

    // Challenger CLV worse: hold, even with better Brier.
    let records: Vec<PairedScore> = (0..40)
        .map(|i| PairedScore {
            category: "weather".to_string(),
            manifest_hash: format!("hash-{i}"),
            incumbent_brier: 0.20,
            challenger_brier: 0.15,
            incumbent_clv_bps: Some(80.0),
            challenger_clv_bps: Some(20.0),
        })
        .collect();
    let eval = evaluate_model_swap(&records, &["weather".to_string()], &thresholds());
    assert_eq!(eval.verdict, SwapVerdict::Hold);
    assert!(eval.reasons.iter().any(|r| r.contains("CLV")));

    // CLV unmeasurable on both sides: Brier alone decides.
    let records: Vec<PairedScore> = (0..40)
        .map(|i| PairedScore {
            category: "weather".to_string(),
            manifest_hash: format!("hash-{i}"),
            incumbent_brier: 0.20,
            challenger_brier: 0.15,
            incumbent_clv_bps: None,
            challenger_clv_bps: None,
        })
        .collect();
    let eval = evaluate_model_swap(&records, &["weather".to_string()], &thresholds());
    assert_eq!(eval.verdict, SwapVerdict::PromoteRecommended);
}

#[test]
fn only_paired_contexts_count() {
    // Records whose manifest hash appears once per (category, hash) are
    // the contract; a duplicate hash means the same context was scored
    // twice — the gate deduplicates rather than double-counting.
    let mut records = paired("weather", 30, 0.20, 0.15);
    let dup = records[0].clone();
    records.push(dup);
    let eval = evaluate_model_swap(&records, &["weather".to_string()], &thresholds());
    assert_eq!(
        eval.categories[0].n, 30,
        "duplicate manifest hashes never inflate the count"
    );
    assert_eq!(eval.verdict, SwapVerdict::PromoteRecommended);
}
