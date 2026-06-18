//! T2.6: the decision cycle (spec 5.8), comparator, Kelly haircut
//! (spec 5.14), and triage tier with declined-trigger shadow sampling.
//!
//! Doctrine under test:
//! - The COMPARATOR derives two-sided, UNSIZED candidates from fresh
//!   calibrated beliefs against live prices through the edges. Stale
//!   beliefs never reach it; edges below the strategy's tier are
//!   skipped; direct and negation mappings price correctly; bracket /
//!   conditional mappings are skipped (v1).
//! - The calibration HAIRCUT scales the Kelly fraction by quality in
//!   [0,1]: quality 0 = no trade, never a negative or amplified bet.
//! - TRIAGE decisions are all logged; a deterministic fixed daily sample
//!   of DECLINED triggers is marked for shadow execution (recall is
//!   measured, not assumed). Day = 00:00 UTC.
//! - The DECISION CYCLE composes trigger -> triage -> assemble -> mind
//!   -> beliefs + candidates, accumulating cost; a declined trigger
//!   produces a record and no mind call.
//!
//! Written BEFORE src/cycle.rs per the repository TDD doctrine.

use fortuna_cognition::beliefs::Freshness;
use fortuna_cognition::cycle::{
    compare_beliefs_to_markets, haircut_kelly_fraction, BeliefView, ComparatorConfig, CycleError,
    DecisionCycle, EdgeView, MarketQuote, ShadowSampler, StubTriageMind, TriageDecision,
    TriageVerdict,
};
use fortuna_cognition::events::{EdgeTier, MappingType};
use fortuna_cognition::mind::{MindOutput, StubMind};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::Side;

fn t(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

fn belief(event: &str, p: f64, fresh: bool) -> BeliefView {
    BeliefView {
        belief_id: format!("b-{event}"),
        event_id: event.to_string(),
        p,
        freshness: if fresh {
            Freshness::Fresh
        } else {
            Freshness::Stale {
                reason: "old".to_string(),
            }
        },
    }
}

fn edge(market: &str, event: &str, mapping: MappingType, confirmed: bool) -> EdgeView {
    EdgeView {
        market: market.to_string(),
        event_id: event.to_string(),
        mapping,
        tier: if confirmed {
            EdgeTier::Confirmed
        } else {
            EdgeTier::Proposed
        },
    }
}

fn quote(market: &str, bid: i64, ask: i64) -> MarketQuote {
    MarketQuote {
        market: market.to_string(),
        yes_bid_cents: bid,
        yes_ask_cents: ask,
    }
}

fn config() -> ComparatorConfig {
    ComparatorConfig {
        min_edge_cents: 5,
        required_tier: EdgeTier::Proposed,
    }
}

// -------------------------------------------------------------- comparator

#[test]
fn direct_edge_buys_the_cheap_side_both_directions() {
    // Belief p=0.70 => fair YES 70c. Ask 60 => buy YES (edge 10).
    let candidates = compare_beliefs_to_markets(
        &[belief("evt-1", 0.70, true)],
        &[edge("KXA", "evt-1", MappingType::Direct, true)],
        &[quote("KXA", 58, 60)],
        &config(),
    );
    assert_eq!(candidates.len(), 1);
    let c = &candidates[0];
    assert_eq!(c.market, "KXA");
    assert_eq!(c.side, Side::Yes);
    assert_eq!(c.fair_cents, 70);
    assert_eq!(c.max_price_cents, 60, "cap at the displayed ask");
    assert_eq!(c.edge_cents, 10);
    assert_eq!(c.belief_id, "b-evt-1");

    // Belief p=0.20 => fair YES 20c, fair NO 80c. YES bid 30 means NO ask
    // is 70 => buy NO (edge 10).
    let candidates = compare_beliefs_to_markets(
        &[belief("evt-2", 0.20, true)],
        &[edge("KXB", "evt-2", MappingType::Direct, true)],
        &[quote("KXB", 30, 33)],
        &config(),
    );
    assert_eq!(candidates.len(), 1);
    let c = &candidates[0];
    assert_eq!(c.side, Side::No);
    assert_eq!(c.fair_cents, 80, "NO fair = 100 - 20");
    assert_eq!(c.max_price_cents, 70, "NO ask = 100 - yes_bid");
    assert_eq!(c.edge_cents, 10);
}

#[test]
fn negation_edge_mirrors_the_probability() {
    // Negation: market YES means the event does NOT happen.
    // p(event)=0.70 => market fair YES = 30c. Ask 22 => buy YES edge 8.
    let candidates = compare_beliefs_to_markets(
        &[belief("evt-1", 0.70, true)],
        &[edge("KXNEG", "evt-1", MappingType::Negation, true)],
        &[quote("KXNEG", 20, 22)],
        &config(),
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].fair_cents, 30);
    assert_eq!(candidates[0].side, Side::Yes);
    assert_eq!(candidates[0].edge_cents, 8);
}

#[test]
fn stale_beliefs_low_tiers_small_edges_and_complex_mappings_are_excluded() {
    let beliefs = [
        belief("evt-stale", 0.9, false),  // stale: excluded (5.5)
        belief("evt-thin", 0.62, true),   // edge below floor
        belief("evt-unconf", 0.9, true),  // edge tier below requirement
        belief("evt-bracket", 0.9, true), // bracket mapping: v1 skip
    ];
    let edges = [
        edge("KXS", "evt-stale", MappingType::Direct, true),
        edge("KXT", "evt-thin", MappingType::Direct, true),
        edge("KXU", "evt-unconf", MappingType::Direct, false),
        edge("KXBR", "evt-bracket", MappingType::BracketComponent, true),
    ];
    let quotes = [
        quote("KXS", 50, 52),
        quote("KXT", 58, 60), // fair 62, ask 60: edge 2 < 5 floor
        quote("KXU", 50, 52),
        quote("KXBR", 50, 52),
    ];
    let mut cfg = config();
    cfg.required_tier = EdgeTier::Confirmed;
    let candidates = compare_beliefs_to_markets(&beliefs, &edges, &quotes, &cfg);
    assert!(
        candidates.is_empty(),
        "stale/thin/unconfirmed/complex all excluded: {candidates:?}"
    );
}

// ----------------------------------------------------------- kelly haircut

#[test]
fn haircut_scales_the_fraction_and_zero_quality_means_no_trade() {
    assert!((haircut_kelly_fraction(0.25, 1.0) - 0.25).abs() < 1e-12);
    assert!((haircut_kelly_fraction(0.25, 0.5) - 0.125).abs() < 1e-12);
    assert_eq!(haircut_kelly_fraction(0.25, 0.0), 0.0);
    // Quality is clamped to [0,1]: never amplifies, never negative.
    assert!((haircut_kelly_fraction(0.25, 1.7) - 0.25).abs() < 1e-12);
    assert_eq!(haircut_kelly_fraction(0.25, -0.3), 0.0);
    // NaN quality fails closed to zero.
    assert_eq!(haircut_kelly_fraction(0.25, f64::NAN), 0.0);
}

// ------------------------------------------------------------ shadow sample

#[test]
fn declined_triggers_shadow_sample_first_k_per_utc_day() {
    let mut sampler = ShadowSampler::new(2);
    let day1 = t("2026-06-11T08:00:00.000Z");
    assert!(sampler.should_shadow(day1));
    assert!(sampler.should_shadow(t("2026-06-11T09:00:00.000Z")));
    assert!(
        !sampler.should_shadow(t("2026-06-11T10:00:00.000Z")),
        "daily quota of 2 exhausted"
    );
    // New UTC day: quota resets.
    assert!(sampler.should_shadow(t("2026-06-12T00:00:01.000Z")));
}

// ------------------------------------------------------------ decision cycle

fn stub_output() -> MindOutput {
    serde_json::from_value(serde_json::json!({
        "beliefs": [{
            "event_id": "evt-1",
            "p": 0.70,
            "p_raw": 0.72,
            "horizon": "2026-06-20T18:00:00.000Z",
            "evidence": [{"source": "aeolus", "ref": "sig-1"}]
        }],
        "proposals": [],
        "journal": null
    }))
    .unwrap()
}

fn near_identity_calibration() -> fortuna_cognition::cycle::CalibrationContext {
    use fortuna_cognition::calibration::{fit_platt, CalibrationMethod, CalibrationParams};
    // Well-calibrated record: the fit is near-identity, so calibrated p
    // tracks the claim (E1: a cycle now REQUIRES a calibration context
    // for beliefs to keep autonomous weight; this is the neutral one).
    let mut samples = Vec::new();
    for i in 0..100 {
        samples.push((0.7, i % 10 < 7));
        samples.push((0.3, i % 10 < 3));
        samples.push((0.5, i % 2 == 0));
    }
    fortuna_cognition::cycle::CalibrationContext {
        params: CalibrationParams {
            version: 1,
            method: CalibrationMethod::Platt(fit_platt(&samples).unwrap()),
            extremization_k: 1.0,
            fitted_on_n: 300,
        },
        resolved_n: 300,
    }
}

#[tokio::test]
async fn accepted_trigger_runs_the_mind_and_derives_candidates() {
    let mind = StubMind::scripted(vec![stub_output()]);
    let triage = TriageDecision::AlwaysAccept;
    let mut cycle = DecisionCycle::new(triage, ShadowSampler::new(1), config())
        .with_calibration(near_identity_calibration());

    let outcome = cycle
        .run(
            "evt-1",
            &mind,
            &[], // context items (empty is fine; the assembler is exercised at T2.4)
            &[edge("KXA", "evt-1", MappingType::Direct, true)],
            &[quote("KXA", 58, 60)],
            t("2026-06-11T12:00:00.000Z"),
        )
        .await
        .unwrap();

    assert_eq!(outcome.triage, TriageVerdict::Accepted);
    assert_eq!(outcome.beliefs.len(), 1);
    assert_eq!(
        outcome.candidates.len(),
        1,
        "fair 70 vs ask 60 => candidate"
    );
    assert_eq!(outcome.candidates[0].side, Side::Yes);
    assert!(!outcome.shadow, "an accepted run is not a shadow run");
    // The manifest hash rode through (provenance replayability).
    assert!(!outcome.manifest_hash.is_empty());
}

#[tokio::test]
async fn declined_trigger_skips_the_mind_unless_shadow_sampled() {
    let mind = StubMind::scripted(vec![stub_output(), stub_output()]);
    let triage = TriageDecision::AlwaysDecline;
    // Quota 1: the first decline shadow-runs, the second does not.
    let mut cycle = DecisionCycle::new(triage, ShadowSampler::new(1), config());

    let first = cycle
        .run(
            "evt-1",
            &mind,
            &[],
            &[edge("KXA", "evt-1", MappingType::Direct, true)],
            &[quote("KXA", 58, 60)],
            t("2026-06-11T12:00:00.000Z"),
        )
        .await
        .unwrap();
    assert_eq!(first.triage, TriageVerdict::Declined);
    assert!(
        first.shadow,
        "first declined trigger of the day shadow-runs"
    );
    assert_eq!(
        first.beliefs.len(),
        1,
        "shadow beliefs are produced (and scored normally)"
    );
    assert!(
        first.candidates.is_empty(),
        "shadow runs NEVER produce trade candidates"
    );

    let second = cycle
        .run("evt-2", &mind, &[], &[], &[], t("2026-06-11T13:00:00.000Z"))
        .await
        .unwrap();
    assert_eq!(second.triage, TriageVerdict::Declined);
    assert!(!second.shadow, "quota exhausted: plain decline");
    assert!(second.beliefs.is_empty(), "no mind call at all");
}

#[tokio::test]
async fn mind_triage_accept_runs_the_frontier_mind() {
    // Spec 5.9 cheap-tier seam: a Mind-backed triage that ACCEPTS escalates to the
    // frontier mind exactly like AlwaysAccept — beliefs produced + candidates
    // derived. Proves the Mind variant routes Accept through the full cycle.
    let mind = StubMind::scripted(vec![stub_output()]);
    let triage = TriageDecision::Mind(std::sync::Arc::new(StubTriageMind::scripted(vec![(
        "evt-1".to_string(),
        TriageVerdict::Accepted,
    )])));
    let mut cycle = DecisionCycle::new(triage, ShadowSampler::new(1), config())
        .with_calibration(near_identity_calibration());

    let outcome = cycle
        .run(
            "evt-1",
            &mind,
            &[],
            &[edge("KXA", "evt-1", MappingType::Direct, true)],
            &[quote("KXA", 58, 60)],
            t("2026-06-11T12:00:00.000Z"),
        )
        .await
        .unwrap();
    assert_eq!(outcome.triage, TriageVerdict::Accepted);
    assert_eq!(outcome.beliefs.len(), 1, "accept -> the frontier mind ran");
    assert_eq!(
        outcome.candidates.len(),
        1,
        "fair 70 vs ask 60 => candidate"
    );
}

#[tokio::test]
async fn mind_triage_decline_skips_the_frontier_mind() {
    // A Mind-backed triage that DECLINES (shadow quota 0, so no shadow run) gates
    // the frontier mind OUT — no beliefs, no candidates. The cheap tier saved the
    // expensive call. NON-VACUOUS: the frontier mind has an output ready; an empty
    // belief set can only mean it was never called.
    let mind = StubMind::scripted(vec![stub_output()]);
    let triage = TriageDecision::Mind(std::sync::Arc::new(StubTriageMind::decline_all()));
    let mut cycle = DecisionCycle::new(triage, ShadowSampler::new(0), config());

    let outcome = cycle
        .run(
            "evt-1",
            &mind,
            &[],
            &[edge("KXA", "evt-1", MappingType::Direct, true)],
            &[quote("KXA", 58, 60)],
            t("2026-06-11T12:00:00.000Z"),
        )
        .await
        .unwrap();
    assert_eq!(outcome.triage, TriageVerdict::Declined);
    assert!(!outcome.shadow, "quota 0 => plain decline, no shadow");
    assert!(
        outcome.beliefs.is_empty(),
        "decline => the frontier mind never ran"
    );
    assert!(outcome.candidates.is_empty());
}

#[tokio::test]
async fn mind_triage_cost_is_accounted_even_on_a_plain_decline() {
    // The cheap-tier triage call COSTS even when it declines (no synthesis). The
    // outcome's cost_cents carries the triage spend, so the budget sees it.
    let mind = StubMind::scripted(vec![stub_output()]);
    let triage = TriageDecision::Mind(std::sync::Arc::new(
        StubTriageMind::decline_all().with_cost(7),
    ));
    let mut cycle = DecisionCycle::new(triage, ShadowSampler::new(0), config());

    let outcome = cycle
        .run("evt-1", &mind, &[], &[], &[], t("2026-06-11T12:00:00.000Z"))
        .await
        .unwrap();
    assert_eq!(outcome.triage, TriageVerdict::Declined);
    assert_eq!(
        outcome.cost_cents, 7,
        "the triage call's cost is accounted even with no frontier call"
    );
}

#[tokio::test]
async fn mind_triage_provider_failure_surfaces_as_cycle_error() {
    // A triage provider failure SURFACES (CycleError::Triage) — never silently
    // coerced to accept or decline. The synthesis strategy degrades on it (its own
    // test); here the cycle's contract is pinned.
    let mind = StubMind::scripted(vec![stub_output()]);
    let triage = TriageDecision::Mind(std::sync::Arc::new(StubTriageMind::failing(
        "triage provider down",
    )));
    let mut cycle = DecisionCycle::new(triage, ShadowSampler::new(1), config());

    let err = cycle
        .run("evt-1", &mind, &[], &[], &[], t("2026-06-11T12:00:00.000Z"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CycleError::Triage(_)),
        "a triage failure is a CycleError::Triage, got {err:?}"
    );
}

// ---- E1: the calibration layer ACTUALLY adjusts p in the cycle ----

fn overconfident_platt_params() -> fortuna_cognition::calibration::CalibrationParams {
    use fortuna_cognition::calibration::{fit_platt, CalibrationMethod, CalibrationParams};
    // Claims at 0.9/0.1 resolving at 0.7/0.3: the fit tempers.
    let mut samples = Vec::new();
    for i in 0..60 {
        samples.push((0.9, i % 10 < 7));
        samples.push((0.1, i % 10 < 3));
    }
    CalibrationParams {
        version: 1,
        method: CalibrationMethod::Platt(fit_platt(&samples).unwrap()),
        extremization_k: 1.0,
        fitted_on_n: 120,
    }
}

fn overconfident_belief_output() -> MindOutput {
    serde_json::from_value(serde_json::json!({
        "beliefs": [{
            "event_id": "evt-1",
            "p": 0.90,
            "p_raw": 0.90,
            "horizon": "2026-06-20T18:00:00.000Z",
            "evidence": [{"source": "stub", "ref": "sig-1"}]
        }],
        "proposals": [],
        "journal": null
    }))
    .unwrap()
}

#[tokio::test]
async fn calibration_layer_adjusts_p_before_the_comparator() {
    use fortuna_cognition::cycle::CalibrationContext;
    let mind = StubMind::scripted(vec![overconfident_belief_output()]);
    let mut cycle = DecisionCycle::new(
        TriageDecision::AlwaysAccept,
        ShadowSampler::new(1),
        config(),
    )
    .with_calibration(CalibrationContext {
        params: overconfident_platt_params(),
        resolved_n: 120,
    });

    let outcome = cycle
        .run(
            "evt-1",
            &mind,
            &[],
            &[edge("KXA", "evt-1", MappingType::Direct, true)],
            &[quote("KXA", 58, 60)],
            t("2026-06-11T12:00:00.000Z"),
        )
        .await
        .unwrap();

    // The mind claimed 0.90; the fitted layer tempers toward ~0.70.
    let p = outcome.beliefs[0].p;
    assert!(
        p > 0.6 && p < 0.8,
        "calibrated p tempers the overconfident claim, got {p}"
    );
    assert!(
        (outcome.beliefs[0].p_raw - 0.90).abs() < 1e-9,
        "the raw claim is preserved for scoring"
    );
    // The comparator saw the CALIBRATED p: fair ~70 vs ask 60 => candidate
    // carrying the exact probability for the sizing layer.
    assert_eq!(outcome.candidates.len(), 1);
    let c = &outcome.candidates[0];
    assert!((c.calibrated_p - p).abs() < 1e-12);
    assert!(c.fair_cents >= 60 && c.fair_cents <= 80);
}

#[tokio::test]
async fn uncalibrated_scope_shrinks_to_market_and_cannot_trade() {
    // NO calibration context wired: the conservative rule is full
    // shrinkage toward the market prior (n = 0 => zero autonomous
    // weight) — an uncalibrated scope structurally produces no edge.
    let mind = StubMind::scripted(vec![overconfident_belief_output()]);
    let mut cycle = DecisionCycle::new(
        TriageDecision::AlwaysAccept,
        ShadowSampler::new(1),
        config(),
    );

    let outcome = cycle
        .run(
            "evt-1",
            &mind,
            &[],
            &[edge("KXA", "evt-1", MappingType::Direct, true)],
            &[quote("KXA", 58, 60)],
            t("2026-06-11T12:00:00.000Z"),
        )
        .await
        .unwrap();

    let p = outcome.beliefs[0].p;
    assert!(
        (p - 0.59).abs() < 1e-9,
        "p shrinks to the market mid (58/60 => 0.59), got {p}"
    );
    assert!(
        outcome.candidates.is_empty(),
        "market-prior beliefs price no edge"
    );
}

#[tokio::test]
async fn low_data_scope_gets_little_autonomous_weight() {
    use fortuna_cognition::cycle::CalibrationContext;
    // Params exist but the scope has only 10 resolved beliefs: shrinkage
    // applies (spec 5.10) and the 0.90 claim moves barely off market.
    let mind = StubMind::scripted(vec![overconfident_belief_output()]);
    let mut cycle = DecisionCycle::new(
        TriageDecision::AlwaysAccept,
        ShadowSampler::new(1),
        config(),
    )
    .with_calibration(CalibrationContext {
        params: overconfident_platt_params(),
        resolved_n: 10,
    });

    let outcome = cycle
        .run(
            "evt-1",
            &mind,
            &[],
            &[edge("KXA", "evt-1", MappingType::Direct, true)],
            &[quote("KXA", 58, 60)],
            t("2026-06-11T12:00:00.000Z"),
        )
        .await
        .unwrap();

    let p = outcome.beliefs[0].p;
    // w = 10/50 = 0.2: p = 0.2*0.9 + 0.8*0.59 = 0.652.
    assert!((p - 0.652).abs() < 1e-9, "got {p}");
}

// ---- B3 Part 1: set_calibration / is_calibrated ----

#[test]
fn set_calibration_some_makes_is_calibrated_true() {
    let mut cycle = DecisionCycle::new(
        TriageDecision::AlwaysAccept,
        ShadowSampler::new(0),
        config(),
    );
    assert!(!cycle.is_calibrated(), "fresh cycle is cold");
    cycle.set_calibration(Some(near_identity_calibration()));
    assert!(cycle.is_calibrated(), "after set_calibration(Some) => warm");
}

#[test]
fn set_calibration_none_makes_is_calibrated_false() {
    let mut cycle = DecisionCycle::new(
        TriageDecision::AlwaysAccept,
        ShadowSampler::new(0),
        config(),
    )
    .with_calibration(near_identity_calibration());
    assert!(cycle.is_calibrated(), "wired via with_calibration => warm");
    cycle.set_calibration(None);
    assert!(
        !cycle.is_calibrated(),
        "set_calibration(None) => cold again"
    );
}
