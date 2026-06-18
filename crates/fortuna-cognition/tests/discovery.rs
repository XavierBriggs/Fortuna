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
        strike_type: None,
        floor_strike: None,
        cap_strike: None,
        status: String::new(),
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

#[tokio::test]
async fn market_back_is_inert_with_no_survivors() {
    // The prefilter excluding every listing is the common steady state. With NO
    // survivors there is nothing to normalize, so the step must make NO mind call
    // and spend NO budget — the shared discovery budget is reserved for the
    // world-forward arm and the segments that DO surface a survivor.
    //
    // PROOF it makes no mind call: script EXACTLY ONE output. If the
    // empty-survivors call consulted the mind it would consume that output, and
    // the SECOND call (with a real survivor) would find nothing scripted and mint
    // no event. The second call minting KX-OK proves the first made no mind call.
    let mind = StubMind::scripted(vec![normalization_body(json!([
        {
            "market_id": "KX-OK",
            "matches_event_id": null,
            "statement": "NYC high temp >= 90F on 2026-06-20",
            "resolution_criteria": "NWS Central Park daily climate report",
            "resolution_source": "nws",
            "horizon": "2026-06-20T18:00:00.000Z",
            "category": "weather",
            "mapping": "direct",
            "confidence": 0.85
        }
    ]))]);
    let mut budget = DiscoveryBudget::new(1_000);

    // Call 1: NO survivors => a clean no-op (no mind call, no spend, no throttle).
    let empty = market_back_discovery(
        &mind,
        &[],
        &[],
        &[],
        &mut budget,
        t("2026-06-11T06:00:00.000Z"),
    )
    .await
    .unwrap();
    assert!(empty.edge_cards.is_empty() && empty.new_events.is_empty() && empty.matched.is_empty());
    assert!(empty.defects.is_empty(), "no survivors is not a defect");
    assert!(!empty.throttled, "no survivors is not a throttle");
    assert_eq!(empty.cost_cents, 0, "no survivors => no mind cost");
    assert_eq!(
        budget.spent_today_cents(),
        0,
        "no survivors => no budget spent"
    );

    // Call 2: the scripted output is STILL there (call 1 never consulted the mind).
    let survivors = vec![market("KX-OK", "weather", 5_000, "nws")];
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
    assert_eq!(
        outcome.new_events.len(),
        1,
        "the scripted normalization survived the no-survivor call"
    );
    assert_eq!(outcome.edge_cards.len(), 1);
    assert_eq!(outcome.edge_cards[0].market_id, "KX-OK");
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

// World-forward now rides the structured-output channel (decide_structured):
// candidates AND their beliefs share ONE payload. StubMind's default
// decide_structured parses journal.body as that payload, so the scripted body
// carries BOTH arrays; top-level output.beliefs is empty (no longer the vehicle).
fn watchlist_body() -> MindOutput {
    serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [
                {
                    "event_hint": "heat-dome-2026-06",
                    "statement": "A heat dome produces 3+ consecutive 95F days in NYC in June 2026",
                    "resolution_criteria": "NWS Central Park daily climate reports",
                    "resolution_source": "nws",
                    "horizon": "2026-06-25T00:00:00.000Z",
                    "category": "weather",
                    "reasoning": "NWS daily climate reports can verify the NYC heat threshold."
                },
                {
                    "event_hint": "alien-disclosure",
                    "statement": "A government discloses alien contact in 2026",
                    "resolution_criteria": "vibes",
                    "resolution_source": "my-cool-blog",
                    "horizon": "2026-12-31T00:00:00.000Z",
                    "category": "politics",
                    "reasoning": "The model claims the event is observable, but the source is not admitted."
                }
            ],
            "beliefs": [{
                "event_id": "watch:heat-dome-2026-06",
                "p": 0.3,
                "p_raw": 0.3,
                "horizon": "2026-06-25T00:00:00.000Z",
                "evidence": [{"source": "nws", "ref": "sig-9"}]
            }]
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
    // Provenance is HARNESS-stamped (spec 5.5) even via the structured channel —
    // the belief carries the model id + context manifest hash, not model-written.
    let prov = &outcome.beliefs[0].provenance;
    assert!(
        prov.get("model_id").is_some() && prov.get("context_manifest_hash").is_some(),
        "world-forward belief carries harness provenance (model_id + manifest hash)"
    );

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
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "alien-disclosure",
                "statement": "A government discloses alien contact in 2026",
                "resolution_criteria": "vibes",
                "resolution_source": "my-cool-blog",
                "horizon": "2026-12-31T00:00:00.000Z",
                "category": "politics",
                "reasoning": "The model claims the event is observable, but the source is not admitted."
            }],
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
            ]
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

#[tokio::test]
async fn world_forward_accepts_belief_event_id_without_watch_prefix() {
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "fed-stress-test-release",
                "statement": "The Federal Reserve releases annual stress test results by June 24, 2026",
                "resolution_criteria": "Federal Reserve Board press release",
                "resolution_source": "nws",
                "horizon": "2026-06-24",
                "category": "macro",
                "reasoning": "The event has a dated public release source and a clear benchmark."
            }],
            "beliefs": [{
                "event_id": "fed-stress-test-release",
                "p": 0.8,
                "p_raw": 0.8,
                "horizon": "2026-06-24",
                "evidence": [{"source": "nws", "ref": "sig-fed"}]
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
        t("2026-06-17T23:16:17.664Z"),
    )
    .await
    .unwrap();

    assert_eq!(outcome.beliefs.len(), 1);
    assert_eq!(outcome.beliefs[0].event_id, "watch:fed-stress-test-release");
}

#[tokio::test]
async fn world_forward_refuses_placeholder_watchlist_candidates() {
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "x",
                "statement": "x",
                "resolution_criteria": "x",
                "resolution_source": "x",
                "horizon": null,
                "category": "x",
                "reasoning": "x"
            }],
            "beliefs": []
        }).to_string()}
    }))
    .unwrap()]);

    let mut budget = DiscoveryBudget::new(500);
    let outcome = world_forward_discovery(
        &mind,
        &[],
        &registry(),
        &mut budget,
        t("2026-06-17T23:16:17.664Z"),
    )
    .await
    .unwrap();

    assert!(
        outcome.candidates.is_empty(),
        "placeholder watchlist events must never persist"
    );
    assert!(
        outcome
            .defects
            .iter()
            .any(|d| d.contains("placeholder") && d.contains("refused")),
        "placeholder refusal is audit-visible: {:?}",
        outcome.defects
    );
}

#[tokio::test]
async fn world_forward_accepts_date_only_horizons() {
    // REGRESSION (live soak 2026-06-17): real Opus emits a bare YYYY-MM-DD
    // horizon on BOTH the candidate and its belief. The strict parser rejected
    // it ("watchlist body violated the contract: cannot parse 2026-06-24"),
    // killing every world-forward pass. The lenient horizon parser normalizes a
    // date to UTC midnight so the batch survives.
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "heat-dome-2026-06",
                "statement": "A heat dome produces 3+ consecutive 95F days in NYC in June 2026",
                "resolution_criteria": "NWS Central Park daily climate reports",
                "resolution_source": "nws",
                "horizon": "2026-06-25",
                "category": "weather",
                "reasoning": "NWS daily climate reports can verify the NYC heat threshold."
            }],
            "beliefs": [{
                "event_id": "watch:heat-dome-2026-06",
                "p": 0.3, "p_raw": 0.3,
                "horizon": "2026-06-25",
                "evidence": [{"source": "nws", "ref": "sig-9"}]
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

    assert!(
        outcome.defects.is_empty(),
        "a date-only horizon must NOT violate the contract: {:?}",
        outcome.defects
    );
    assert_eq!(outcome.candidates.len(), 1);
    assert_eq!(
        outcome.candidates[0].horizon,
        Some(t("2026-06-25T00:00:00.000Z")),
        "bare date normalized to UTC midnight"
    );
    assert_eq!(outcome.beliefs.len(), 1);
    assert_eq!(outcome.beliefs[0].horizon, t("2026-06-25T00:00:00.000Z"));
}

#[tokio::test]
async fn world_forward_accepts_observed_resolved_date_phrase() {
    // REGRESSION (demo soak 2026-06-17): after date-only support, Opus emitted
    // "resolved 2026-05-22" for a model-facing horizon. Keep that phrase
    // accepted only through the opt-in cognition horizon parser.
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "fed-warsh-oath",
                "statement": "Kevin Warsh takes the Fed chair oath by May 22, 2026",
                "resolution_criteria": "Federal Reserve Board press release",
                "resolution_source": "nws",
                "horizon": "resolved 2026-05-22",
                "category": "macro",
                "reasoning": "A Federal Reserve press release would verify the oath timing."
            }],
            "beliefs": [{
                "event_id": "watch:fed-warsh-oath",
                "p": 0.8, "p_raw": 0.8,
                "horizon": "resolved 2026-05-22",
                "evidence": [{"source": "nws", "ref": "sig-fed-warsh"}]
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

    assert!(
        outcome.defects.is_empty(),
        "the observed resolved-date phrase must not violate the contract: {:?}",
        outcome.defects
    );
    assert_eq!(outcome.candidates.len(), 1);
    assert_eq!(
        outcome.candidates[0].horizon,
        Some(t("2026-05-22T00:00:00.000Z"))
    );
    assert_eq!(outcome.beliefs.len(), 1);
    assert_eq!(outcome.beliefs[0].horizon, t("2026-05-22T00:00:00.000Z"));
}

// -------------------------------------------------- C2: prose resolution_source → scoreable (F4 fix)

fn registry_with_fed() -> SourceRegistry {
    let mut reg = SourceRegistry::new();
    // "rss_fed_press" with domain_tags that prose like "Federal Reserve Board
    // press releases" should match via the fuzzy resolver.
    reg.upsert(SourceEntry {
        source_id: "rss_fed_press".to_string(),
        trust_tier: TrustTier::new(7).unwrap(),
        domain_tags: vec!["federal reserve".to_string(), "fomc".to_string()],
        enabled: true,
    });
    reg.upsert(SourceEntry {
        source_id: "nws".to_string(),
        trust_tier: TrustTier::new(8).unwrap(),
        domain_tags: vec!["weather".to_string()],
        enabled: true,
    });
    reg
}

#[tokio::test]
async fn world_forward_prose_resolution_source_resolves_to_enabled_scoreable() {
    // F4 BUG: before C2, registry.get("Federal Reserve Board press releases") returns
    // None (exact lookup fails) → unscoreable=true → no belief attaches.
    // After C2, registry.resolve(prose) fuzzy-matches "rss_fed_press" via the
    // "federal reserve" domain_tag → unscoreable=false → belief attaches.
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "fed-rate-decision",
                "statement": "The Federal Reserve holds the federal funds rate at 4.25-4.5% at the June 2026 FOMC meeting",
                "resolution_criteria": "Federal Reserve Board official press release",
                "resolution_source": "Federal Reserve Board press releases",
                "horizon": "2026-06-18T18:00:00.000Z",
                "category": "macro",
                "reasoning": "Official FOMC statement resolves this cleanly."
            }],
            "beliefs": [{
                "event_id": "watch:fed-rate-decision",
                "p": 0.85,
                "p_raw": 0.85,
                "horizon": "2026-06-18T18:00:00.000Z",
                "evidence": [{"source": "rss_fed_press", "ref": "fomc-june-2026"}]
            }]
        }).to_string()}
    }))
    .unwrap()]);

    let mut budget = DiscoveryBudget::new(500);
    let outcome = world_forward_discovery(
        &mind,
        &[],
        &registry_with_fed(),
        &mut budget,
        t("2026-06-17T12:00:00.000Z"),
    )
    .await
    .unwrap();

    assert_eq!(outcome.candidates.len(), 1, "one candidate produced");
    let candidate = &outcome.candidates[0];
    // THE F4 FIX: prose resolves to rss_fed_press (enabled) → scoreable
    assert!(
        !candidate.unscoreable,
        "prose resolution_source must resolve to an enabled registry entry and be scoreable (F4)"
    );
    // And the belief must attach (only scoreable events get beliefs)
    assert_eq!(
        outcome.beliefs.len(),
        1,
        "belief must attach to scoreable world-forward event"
    );
    assert_eq!(outcome.beliefs[0].event_id, "watch:fed-rate-decision");
    assert!(
        outcome.defects.is_empty(),
        "no defects: {:?}",
        outcome.defects
    );
}

#[tokio::test]
async fn world_forward_unregistered_prose_resolution_source_remains_unscoreable() {
    // Prose that matches no registry source stays unscoreable — the existing
    // behaviour for truly unknown sources is preserved.
    let mind = StubMind::scripted(vec![serde_json::from_value(json!({
        "beliefs": [],
        "proposals": [],
        "journal": {"body": json!({
            "candidates": [{
                "event_hint": "random-blog-event",
                "statement": "Some event resolved by an obscure blog",
                "resolution_criteria": "My cool blog",
                "resolution_source": "my-cool-blog dot com",
                "horizon": "2026-12-31T00:00:00.000Z",
                "category": "politics",
                "reasoning": "Unregistered source."
            }],
            "beliefs": []
        }).to_string()}
    }))
    .unwrap()]);

    let mut budget = DiscoveryBudget::new(500);
    let outcome = world_forward_discovery(
        &mind,
        &[],
        &registry_with_fed(),
        &mut budget,
        t("2026-06-17T12:00:00.000Z"),
    )
    .await
    .unwrap();

    assert_eq!(outcome.candidates.len(), 1);
    assert!(
        outcome.candidates[0].unscoreable,
        "unregistered prose source must remain unscoreable"
    );
    assert!(outcome.beliefs.is_empty(), "no belief on unscoreable event");
}
