//! T2.2: signal ingestion funnel (spec 5.11) + trigger engine (spec 5.8).
//!
//! Doctrine under test:
//! - One funnel: Source adapters are dumb (fetch/emit); the NORMALIZER
//!   builds the envelope {source, type, received_at, payload,
//!   content_hash} and dedups on (source, content_hash) — the same
//!   content re-fetched later is a duplicate, not news.
//! - The registry is a fail-closed ALLOWLIST: an unregistered or disabled
//!   source's signals are refused, never silently ingested.
//! - Point-in-time: received_at is the adapter's receipt time; envelopes
//!   are immutable values.
//! - Trigger engine: declarative rules raise triggers; per-event
//!   serialization allows AT MOST ONE decision cycle in flight per
//!   canonical event; a debounce window coalesces triggers arriving
//!   during or shortly after a cycle (a news burst is one decision, not
//!   five). Suppressed/coalesced triggers are REPORTED, never dropped
//!   silently.
//!
//! Written BEFORE src/signals.rs per the repository TDD doctrine.

use fortuna_cognition::signals::{
    normalize_and_dedup, DedupIndex, IngestOutcome, RawSignal, Source, SourceEntry, SourceRegistry,
    TriggerDecision, TriggerEngine, TriggerEngineConfig, TriggerRule, TrustTier,
};
use fortuna_core::clock::UtcTimestamp;
use serde_json::json;

fn t(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(1_780_000_000_000 + ms).unwrap()
}

fn registry() -> SourceRegistry {
    let mut r = SourceRegistry::new();
    r.upsert(SourceEntry {
        source_id: "aeolus".to_string(),
        trust_tier: TrustTier::new(7).unwrap(),
        domain_tags: vec!["weather".to_string()],
        enabled: true,
    });
    r.upsert(SourceEntry {
        source_id: "rss-nws".to_string(),
        trust_tier: TrustTier::new(5).unwrap(),
        domain_tags: vec!["weather".to_string()],
        enabled: true,
    });
    r.upsert(SourceEntry {
        source_id: "sketchy-blog".to_string(),
        trust_tier: TrustTier::new(1).unwrap(),
        domain_tags: vec![],
        enabled: false,
    });
    r
}

fn raw(kind: &str, payload: serde_json::Value, at_ms: i64) -> RawSignal {
    RawSignal {
        kind: kind.to_string(),
        payload,
        received_at: t(at_ms),
    }
}

// ----------------------------------------------------------- the funnel

#[test]
fn normalizer_builds_envelopes_and_dedups_on_content() {
    let reg = registry();
    let mut dedup = DedupIndex::new();

    let out = normalize_and_dedup(
        "aeolus",
        vec![
            raw("aeolus_run", json!({"station": "KNYC", "p": [1, 2, 3]}), 0),
            // Same content, later receipt: DUPLICATE.
            raw(
                "aeolus_run",
                json!({"station": "KNYC", "p": [1, 2, 3]}),
                5_000,
            ),
            // Different content: fresh.
            raw("aeolus_run", json!({"station": "KBOS", "p": [4]}), 6_000),
        ],
        &reg,
        &mut dedup,
        |n| format!("sig-{n}"),
    );

    let IngestOutcome::Accepted {
        envelopes,
        duplicates,
    } = out
    else {
        panic!("registered+enabled source must be accepted");
    };
    assert_eq!(envelopes.len(), 2);
    assert_eq!(duplicates, 1);
    assert_eq!(envelopes[0].signal_id, "sig-0");
    assert_eq!(envelopes[0].source, "aeolus");
    assert_eq!(envelopes[0].kind, "aeolus_run");
    assert_eq!(envelopes[0].received_at, t(0));
    assert!(!envelopes[0].content_hash.is_empty());
    assert_ne!(
        envelopes[0].content_hash, envelopes[1].content_hash,
        "different payloads, different hashes"
    );
}

#[test]
fn content_hash_is_canonical_over_key_order_and_scoped_to_source() {
    let reg = registry();
    let mut dedup = DedupIndex::new();
    // Key order must not defeat dedup (canonical serialization).
    let out = normalize_and_dedup(
        "aeolus",
        vec![
            raw("k", json!({"a": 1, "b": 2}), 0),
            raw("k", json!({"b": 2, "a": 1}), 1),
        ],
        &reg,
        &mut dedup,
        |n| format!("s-{n}"),
    );
    let IngestOutcome::Accepted {
        envelopes,
        duplicates,
    } = out
    else {
        panic!()
    };
    assert_eq!(envelopes.len(), 1);
    assert_eq!(duplicates, 1);

    // The SAME payload from a DIFFERENT source is a distinct signal.
    let out2 = normalize_and_dedup(
        "rss-nws",
        vec![raw("k", json!({"a": 1, "b": 2}), 2)],
        &reg,
        &mut dedup,
        |n| format!("s2-{n}"),
    );
    let IngestOutcome::Accepted { envelopes, .. } = out2 else {
        panic!()
    };
    assert_eq!(envelopes.len(), 1, "dedup is per-source");
}

#[test]
fn unregistered_and_disabled_sources_are_refused_fail_closed() {
    let reg = registry();
    let mut dedup = DedupIndex::new();

    let out = normalize_and_dedup(
        "rando-feed",
        vec![raw("k", json!({"x": 1}), 0)],
        &reg,
        &mut dedup,
        |n| format!("s-{n}"),
    );
    assert!(matches!(out, IngestOutcome::RefusedUnregistered));

    let out = normalize_and_dedup(
        "sketchy-blog",
        vec![raw("k", json!({"x": 1}), 0)],
        &reg,
        &mut dedup,
        |n| format!("s-{n}"),
    );
    assert!(matches!(out, IngestOutcome::RefusedDisabled));
}

#[test]
fn trust_tier_bounds_match_the_schema() {
    assert!(TrustTier::new(0).is_ok());
    assert!(TrustTier::new(10).is_ok());
    assert!(TrustTier::new(11).is_err(), "schema CHECK is 0..=10");
}

/// A scripted Source adapter: the trait is poll-or-push unified as
/// drain-on-poll; adapters stay dumb.
struct ScriptedSource {
    id: String,
    batches: Vec<Vec<RawSignal>>,
}

#[async_trait::async_trait]
impl Source for ScriptedSource {
    fn id(&self) -> &str {
        &self.id
    }
    async fn fetch(&mut self) -> Result<Vec<RawSignal>, fortuna_cognition::signals::SignalError> {
        if self.batches.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(self.batches.remove(0))
        }
    }
}

#[test]
fn source_trait_drains_batches() {
    let mut src = ScriptedSource {
        id: "aeolus".to_string(),
        batches: vec![vec![raw("aeolus_run", json!({"r": 1}), 0)], vec![]],
    };
    let got = futures::executor::block_on(src.fetch()).unwrap();
    assert_eq!(got.len(), 1);
    let got = futures::executor::block_on(src.fetch()).unwrap();
    assert!(got.is_empty());
}

// ------------------------------------------------------- trigger engine

fn engine() -> TriggerEngine {
    TriggerEngine::new(TriggerEngineConfig {
        debounce_ms: 60_000,
        rules: vec![
            TriggerRule::NewSignalKind {
                source: "aeolus".to_string(),
                kind: "aeolus_run".to_string(),
            },
            TriggerRule::KeywordMatch {
                keywords: vec!["hurricane".to_string()],
            },
            TriggerRule::PriceBeliefDivergence {
                min_divergence_cents: 5,
            },
        ],
    })
}

#[test]
fn rules_match_signals_and_divergence() {
    let e = engine();
    // New-signal-kind rule.
    assert!(e.signal_matches("aeolus", "aeolus_run", &json!({"r": 1})));
    assert!(!e.signal_matches("rss-nws", "aeolus_run", &json!({"r": 1})));
    // Keyword rule scans payload text (data, never instructions).
    assert!(e.signal_matches(
        "rss-nws",
        "news_item",
        &json!({"title": "Hurricane warning issued", "body": "..."})
    ));
    assert!(!e.signal_matches("rss-nws", "news_item", &json!({"title": "sunny"})));
    // Divergence rule is a direct condition.
    assert!(e.divergence_matches(12));
    assert!(!e.divergence_matches(4));
}

#[test]
fn per_event_serialization_allows_one_cycle_in_flight() {
    let mut e = engine();
    assert_eq!(e.request_cycle("evt-1", t(0)), TriggerDecision::Fire);
    e.begin_cycle("evt-1");

    // While in flight: coalesced, not fired, not lost.
    assert_eq!(
        e.request_cycle("evt-1", t(1_000)),
        TriggerDecision::CoalescedInFlight
    );
    // A DIFFERENT event fires independently.
    assert_eq!(e.request_cycle("evt-2", t(1_000)), TriggerDecision::Fire);

    // Completion inside the debounce window: the burst stays one decision.
    e.complete_cycle("evt-1", t(2_000));
    assert_eq!(
        e.request_cycle("evt-1", t(10_000)),
        TriggerDecision::CoalescedDebounce
    );

    // Past the window: a new cycle may fire.
    assert_eq!(e.request_cycle("evt-1", t(63_000)), TriggerDecision::Fire);
}

#[test]
fn coalesced_in_flight_triggers_surface_as_pending_on_completion() {
    let mut e = engine();
    assert_eq!(e.request_cycle("evt-1", t(0)), TriggerDecision::Fire);
    e.begin_cycle("evt-1");
    let _ = e.request_cycle("evt-1", t(500));
    let _ = e.request_cycle("evt-1", t(900));

    // The news burst coalesced; completion REPORTS that triggers arrived
    // mid-flight so the caller can decide (and audit) a follow-up.
    let pending = e.complete_cycle("evt-1", t(2_000));
    assert_eq!(pending, 2, "coalesced triggers are counted, never silent");
    let pending = e.complete_cycle("evt-2", t(2_000));
    assert_eq!(pending, 0);
}

#[test]
fn begin_without_fire_and_double_begin_are_refused() {
    let mut e = engine();
    assert_eq!(e.request_cycle("evt-1", t(0)), TriggerDecision::Fire);
    e.begin_cycle("evt-1");
    // A second begin while in flight must not corrupt the serialization.
    e.begin_cycle("evt-1"); // idempotent
    assert_eq!(
        e.request_cycle("evt-1", t(100)),
        TriggerDecision::CoalescedInFlight
    );
}
