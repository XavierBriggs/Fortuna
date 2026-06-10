//! T3.2: market-back discovery + edge confirmation cards; world-forward
//! watchlist loop with cost cap + unscoreable rule (spec 5.12).
//!
//! Doctrine under test:
//! - Market-back PREFILTER is deterministic and counts its exclusions
//!   (category allowlist, volume floor, resolution clarity, category
//!   calibration record). Tradability is a deterministic score.
//! - The cheap-tier mind NORMALIZES survivors into canonical events via
//!   a strict JSON contract (journal-body vehicle, like the weekly
//!   review): match-before-create, hallucinated matches dropped loudly,
//!   free prose degrades to zero normalizations with a defect.
//! - Every proposed edge gets a CONFIRMATION CARD carrying the model's
//!   confidence AND the deterministic check score; non-direct mappings
//!   and resolution-source mismatches are flagged high-stakes (the
//!   UMA-mode failure). Cards are review-queue items; confirmation is
//!   the operator's.
//! - World-forward candidates MUST declare a resolution source in the
//!   source registry; otherwise the event is UNSCOREABLE (excluded from
//!   watchlist counts and calibration). The loop is budget-capped and
//!   throttles BEFORE spending (first thing throttled under pressure).
//!
//! Written BEFORE src/discovery.rs per the repository TDD doctrine.

use fortuna_cognition::discovery::{
    market_back_discovery, prefilter, tradability_score, watchlist_count, world_forward_discovery,
    DiscoveryBudget, ExistingEventView, MarketView, PrefilterConfig, WatchlistEventView,
};
use fortuna_cognition::events::MappingType;
use fortuna_cognition::mind::{MindOutput, StubMind};
use fortuna_cognition::signals::{SourceEntry, SourceRegistry, TrustTier};
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;
use std::collections::BTreeMap;

fn t(iso: &str) -> UtcTimestamp {
    UtcTimestamp::parse_iso8601(iso).unwrap()
}

fn market(id: &str, category: &str, volume: i64, source: &str) -> MarketView {
    MarketView {
        market_id: id.to_string(),
        venue: "kalshi".to_string(),
        title: format!("market {id}"),
        category: category.to_string(),
        volume_contracts: Some(volume),
        resolution_source: source.to_string(),
        close_at: Some(t("2026-06-20T18:00:00.000Z")),
    }
}

fn config() -> PrefilterConfig {
    PrefilterConfig {
        category_allowlist: vec!["weather".to_string(), "econ".to_string()],
        min_volume_contracts: 100,
        min_category_quality: 0.1,
        category_quality: BTreeMap::from([
            ("weather".to_string(), 0.6),
            ("econ".to_string(), 0.05), // poor record: filtered
        ]),
    }
}

// --------------------------------------------------------------- prefilter

#[test]
fn prefilter_excludes_with_counted_reasons() {
    let markets = vec![
        market("KX-OK", "weather", 5_000, "nws"),
        market("KX-CAT", "politics", 5_000, "ap"), // category not allowed
        market("KX-THIN", "weather", 10, "nws"),   // volume below floor
        market("KX-VAGUE", "weather", 5_000, ""),  // no resolution source
        market("KX-BADCAL", "econ", 5_000, "bls"), // category record poor
    ];
    let outcome = prefilter(&markets, &config());

    assert_eq!(outcome.survivors.len(), 1);
    assert_eq!(outcome.survivors[0].market_id, "KX-OK");
    assert_eq!(outcome.excluded.len(), 4);
    let reasons: BTreeMap<&str, &str> = outcome
        .excluded
        .iter()
        .map(|(id, r)| (id.as_str(), r.as_str()))
        .collect();
    assert!(reasons["KX-CAT"].contains("category"));
    assert!(reasons["KX-THIN"].contains("volume"));
    assert!(reasons["KX-VAGUE"].contains("resolution"));
    assert!(reasons["KX-BADCAL"].contains("calibration"));
}

#[test]
fn tradability_is_deterministic_and_bounded() {
    let m = market("KX-OK", "weather", 5_000, "nws");
    let s = tradability_score(&m, 0.6, 10_000);
    assert!(s > 0.0 && s <= 1.0);
    assert_eq!(s, tradability_score(&m, 0.6, 10_000), "deterministic");

    // Monotone in volume and quality.
    let thin = market("KX-T", "weather", 500, "nws");
    assert!(tradability_score(&thin, 0.6, 10_000) < s);
    assert!(tradability_score(&m, 0.3, 10_000) < s);

    // No checkable resolution source: zero, regardless of volume.
    let vague = market("KX-V", "weather", 50_000, "");
    assert_eq!(tradability_score(&vague, 0.9, 10_000), 0.0);
}

// ----------------------------------------------------- market-back (mind)

fn normalization_body(entries: serde_json::Value) -> MindOutput {
    serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({"normalizations": entries}).to_string()}
    }))
    .unwrap()
}

fn existing_event(id: &str) -> ExistingEventView {
    ExistingEventView {
        event_id: id.to_string(),
        resolution_source: "nws".to_string(),
        horizon: Some(t("2026-06-20T18:00:00.000Z")),
        has_open_belief: true,
    }
}

#[tokio::test]
async fn market_back_matches_before_creating_and_cards_every_edge() {
    let mind = StubMind::scripted(vec![normalization_body(json!([
        {
            "market_id": "KX-OK",
            "matches_event_id": "evt-known",
            "statement": null, "resolution_criteria": null,
            "resolution_source": "nws",
            "horizon": "2026-06-20T18:00:00.000Z",
            "category": "weather",
            "mapping": "direct",
            "confidence": 0.85
        },
        {
            "market_id": "KX-NEW",
            "matches_event_id": null,
            "statement": "NYC high temp >= 90F on 2026-06-20",
            "resolution_criteria": "NWS Central Park daily climate report",
            "resolution_source": "nws",
            "horizon": "2026-06-20T18:00:00.000Z",
            "category": "weather",
            "mapping": "negation",
            "confidence": 0.7
        },
        {
            "market_id": "KX-HALLU",
            "matches_event_id": "evt-i-made-this-up",
            "statement": null, "resolution_criteria": null,
            "resolution_source": "nws",
            "horizon": "2026-06-20T18:00:00.000Z",
            "category": "weather",
            "mapping": "direct",
            "confidence": 0.9
        }
    ]))]);

    let survivors = vec![
        market("KX-OK", "weather", 5_000, "nws"),
        market("KX-NEW", "weather", 3_000, "nws"),
        market("KX-HALLU", "weather", 2_000, "nws"),
    ];
    let existing = vec![existing_event("evt-known")];
    let mut budget = DiscoveryBudget::new(1_000);

    let outcome = market_back_discovery(
        &mind,
        &[],
        &survivors,
        &existing,
        &mut budget,
        t("2026-06-11T06:00:00.000Z"),
    )
    .await
    .unwrap();

    // Matched: rides the existing event; the open belief wakes the cycle.
    assert_eq!(outcome.matched.len(), 1);
    assert_eq!(outcome.matched[0].event_id, "evt-known");
    assert_eq!(outcome.wake_events, vec!["evt-known".to_string()]);

    // New: a canonical event draft (match-before-create held: it had no
    // matching id).
    assert_eq!(outcome.new_events.len(), 1);
    assert_eq!(outcome.new_events[0].market_id, "KX-NEW");
    assert!(outcome.new_events[0].statement.contains("NYC high"));

    // The hallucinated match is DROPPED with a defect, not created.
    assert_eq!(outcome.defects.len(), 1);
    assert!(outcome.defects[0].contains("evt-i-made-this-up"));

    // Every surviving proposal got a confirmation card; the negation
    // mapping is high-stakes (wrong equivalence = unhedged position).
    assert_eq!(outcome.edge_cards.len(), 2);
    let direct = &outcome.edge_cards[0];
    assert_eq!(direct.market_id, "KX-OK");
    assert_eq!(direct.mapping, MappingType::Direct);
    assert!((direct.model_confidence - 0.85).abs() < 1e-9);
    assert!(
        (direct.deterministic_score - 1.0).abs() < 1e-9,
        "source+horizon match scores 1.0"
    );
    assert!(!direct.high_stakes);
    let negation = &outcome.edge_cards[1];
    assert_eq!(negation.market_id, "KX-NEW");
    assert!(negation.high_stakes, "non-direct mapping needs a human");

    assert!(!outcome.throttled);
    assert!(budget.spent_today_cents() >= 0);
}

#[tokio::test]
async fn market_back_degrades_on_prose_and_throttles_on_budget() {
    // Free prose: zero normalizations, a defect, never a crash.
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [], "proposals": [],
        "journal": {"body": "I think these markets look interesting!"}
    }))
    .unwrap()]);
    let survivors = vec![market("KX-OK", "weather", 5_000, "nws")];
    let mut budget = DiscoveryBudget::new(1_000);
    let outcome = market_back_discovery(
        &mind,
        &[],
        &survivors,
        &[],
        &mut budget,
        t("2026-06-11T06:00:00.000Z"),
    )
    .await
    .unwrap();
    assert!(outcome.matched.is_empty() && outcome.new_events.is_empty());
    assert_eq!(outcome.defects.len(), 1);

    // Budget exhausted: throttled BEFORE the call (no mind consumption).
    let mind = StubMind::scripted(vec![normalization_body(json!([]))]);
    let mut spent = DiscoveryBudget::new(0);
    let outcome = market_back_discovery(
        &mind,
        &[],
        &survivors,
        &[],
        &mut spent,
        t("2026-06-11T06:00:00.000Z"),
    )
    .await
    .unwrap();
    assert!(outcome.throttled);
    assert!(outcome.defects.is_empty(), "throttling is not a defect");
}

// -------------------------------------------------- world-forward watchlist

fn registry() -> SourceRegistry {
    let mut reg = SourceRegistry::new();
    reg.upsert(SourceEntry {
        source_id: "nws".to_string(),
        trust_tier: TrustTier::new(8).unwrap(),
        domain_tags: vec!["weather".to_string()],
        enabled: true,
    });
    reg
}

fn watchlist_body() -> MindOutput {
    serde_json::from_value(json!({
        "beliefs": [{
            "event_id": "watch:heat-dome-2026-06",
            "p": 0.3,
            "p_raw": 0.3,
            "horizon": "2026-06-25T00:00:00.000Z",
            "evidence": [{"source": "nws", "ref": "sig-9"}]
        }],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [
                {
                    "event_hint": "heat-dome-2026-06",
                    "statement": "A heat dome produces 3+ consecutive 95F days in NYC in June 2026",
                    "resolution_criteria": "NWS Central Park daily climate reports",
                    "resolution_source": "nws",
                    "horizon": "2026-06-25T00:00:00.000Z",
                    "category": "weather"
                },
                {
                    "event_hint": "alien-disclosure",
                    "statement": "A government discloses alien contact in 2026",
                    "resolution_criteria": "vibes",
                    "resolution_source": "my-cool-blog",
                    "horizon": "2026-12-31T00:00:00.000Z",
                    "category": "politics"
                }
            ]
        }).to_string()}
    }))
    .unwrap()
}

#[tokio::test]
async fn world_forward_enforces_the_unscoreable_rule_and_cost_cap() {
    let mind = StubMind::scripted(vec![watchlist_body()]);
    let mut budget = DiscoveryBudget::new(500);

    let outcome = world_forward_discovery(
        &mind,
        &[],
        &registry(),
        &mut budget,
        t("2026-06-11T07:00:00.000Z"),
    )
    .await
    .unwrap();

    assert_eq!(outcome.candidates.len(), 2);
    let scoreable = &outcome.candidates[0];
    assert_eq!(scoreable.event_id, "watch:heat-dome-2026-06");
    assert!(!scoreable.unscoreable, "registry source: scoreable");
    let vibes = &outcome.candidates[1];
    assert!(
        vibes.unscoreable,
        "resolution source outside the registry: unscoreable"
    );

    // Beliefs ride only on scoreable watchlist events; a belief nobody
    // can grade is refused.
    assert_eq!(outcome.beliefs.len(), 1);
    assert_eq!(outcome.beliefs[0].event_id, "watch:heat-dome-2026-06");

    // Watchlist counts exclude unscoreable events.
    let views: Vec<WatchlistEventView> = outcome
        .candidates
        .iter()
        .map(|c| WatchlistEventView {
            event_id: c.event_id.clone(),
            unscoreable: c.unscoreable,
        })
        .collect();
    assert_eq!(watchlist_count(&views), 1);

    // The cap throttles BEFORE spending.
    let mind = StubMind::scripted(vec![watchlist_body()]);
    let mut spent = DiscoveryBudget::new(0);
    let outcome = world_forward_discovery(
        &mind,
        &[],
        &registry(),
        &mut spent,
        t("2026-06-11T07:00:00.000Z"),
    )
    .await
    .unwrap();
    assert!(outcome.throttled);
    assert!(outcome.candidates.is_empty());
}

#[tokio::test]
async fn world_forward_refuses_beliefs_on_unscoreable_or_unknown_events() {
    // The mind attaches a belief to the UNSCOREABLE candidate and one to
    // an event it never declared: both refused with defects.
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [
            {
                "event_id": "watch:alien-disclosure",
                "p": 0.9, "p_raw": 0.9,
                "horizon": "2026-12-31T00:00:00.000Z",
                "evidence": [{"source": "my-cool-blog", "ref": "x"}]
            },
            {
                "event_id": "watch:never-declared",
                "p": 0.5, "p_raw": 0.5,
                "horizon": "2026-12-31T00:00:00.000Z",
                "evidence": [{"source": "nws", "ref": "y"}]
            }
        ],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "alien-disclosure",
                "statement": "A government discloses alien contact in 2026",
                "resolution_criteria": "vibes",
                "resolution_source": "my-cool-blog",
                "horizon": "2026-12-31T00:00:00.000Z",
                "category": "politics"
            }]
        }).to_string()}
    }))
    .unwrap()]);

    let mut budget = DiscoveryBudget::new(500);
    let outcome = world_forward_discovery(
        &mind,
        &[],
        &registry(),
        &mut budget,
        t("2026-06-11T07:00:00.000Z"),
    )
    .await
    .unwrap();

    assert!(outcome.beliefs.is_empty());
    assert_eq!(outcome.defects.len(), 2);
    assert!(outcome.defects.iter().any(|d| d.contains("unscoreable")));
    assert!(outcome.defects.iter().any(|d| d.contains("never-declared")));
}
