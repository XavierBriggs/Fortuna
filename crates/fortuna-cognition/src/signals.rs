//! Signal ingestion funnel (spec 5.11) + trigger engine (spec 5.8).
//!
//! One funnel for all non-venue-execution data: dumb `Source` adapters
//! (fetch, retry, emit) -> the normalizer (envelope + content-hash dedup)
//! -> the append-only signals store. Two governing rules:
//!
//! - POINT-IN-TIME: `received_at` is authoritative (assigned at receipt
//!   from the injected clock by the adapter); envelopes are immutable
//!   values; nothing is updated in place. This is what makes decisions
//!   replayable.
//! - DATA-NOT-INSTRUCTIONS: every ingested payload is untrusted content.
//!   This module only hashes, stores, and pattern-matches it; nothing
//!   here (or anywhere) executes it. The prompt-injection blast radius
//!   is bounded by I6 and the gates regardless.
//!
//! The source REGISTRY is a fail-closed allowlist: unregistered or
//! disabled sources are refused. Trust tiers (0..=10, the Pg schema's
//! CHECK range) feed evidence weighting and are updated on the record by
//! per-source belief attribution (T2.3+).
//!
//! The TRIGGER ENGINE is the cost-control valve (5.8): declarative rules
//! raise triggers; per-event serialization allows at most ONE decision
//! cycle in flight per canonical event; a debounce window coalesces
//! bursts (one decision, not five). Coalesced triggers are counted and
//! reported, never silently dropped.

use async_trait::async_trait;
use fortuna_core::clock::UtcTimestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignalError {
    /// Adapter-level fetch failure (network, parse). The funnel retries
    /// on its own cadence; adapters stay dumb.
    #[error("source {source_id}: {reason}")]
    Fetch { source_id: String, reason: String },
    #[error("trust tier {got} outside the registry range 0..=10")]
    TierRange { got: u8 },
}

/// Trust tier, 0..=10 by construction (matches the source_registry CHECK).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct TrustTier(u8);

impl TrustTier {
    pub fn new(tier: u8) -> Result<Self, SignalError> {
        if tier <= 10 {
            Ok(TrustTier(tier))
        } else {
            Err(SignalError::TierRange { got: tier })
        }
    }

    pub fn raw(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for TrustTier {
    type Error = SignalError;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        TrustTier::new(v)
    }
}

impl From<TrustTier> for u8 {
    fn from(t: TrustTier) -> u8 {
        t.0
    }
}

/// One allowlist entry (mirrors the source_registry row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEntry {
    pub source_id: String,
    pub trust_tier: TrustTier,
    pub domain_tags: Vec<String>,
    pub enabled: bool,
}

/// The curated allowlist. Sources not in it do not exist to the funnel.
#[derive(Debug, Default)]
pub struct SourceRegistry {
    entries: BTreeMap<String, SourceEntry>,
}

impl SourceRegistry {
    pub fn new() -> SourceRegistry {
        SourceRegistry::default()
    }

    pub fn upsert(&mut self, entry: SourceEntry) {
        self.entries.insert(entry.source_id.clone(), entry);
    }

    pub fn get(&self, source_id: &str) -> Option<&SourceEntry> {
        self.entries.get(source_id)
    }
}

/// What a dumb adapter emits: kind + payload + receipt time (from the
/// adapter's injected clock — point-in-time authority).
#[derive(Debug, Clone, PartialEq)]
pub struct RawSignal {
    pub kind: String,
    pub payload: Value,
    pub received_at: UtcTimestamp,
}

/// The abstract acquisition trait (spec 5.11): poll or push, RSS or REST
/// or webhook or MCP plumbing or scraper or file drop — push adapters
/// buffer internally and drain on `fetch`. Adding a source never touches
/// the core; an adapter that wants to be clever is doing the normalizer's
/// or trigger engine's job.
#[async_trait]
pub trait Source: Send {
    fn id(&self) -> &str;
    async fn fetch(&mut self) -> Result<Vec<RawSignal>, SignalError>;
}

/// The common envelope (spec 5.11): {source, type, received_at, payload,
/// content_hash}. Immutable once built.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalEnvelope {
    pub signal_id: String,
    pub source: String,
    pub kind: String,
    pub received_at: UtcTimestamp,
    pub payload: Value,
    pub content_hash: String,
}

/// Dedup index over (source, content_hash). Rebuilt at boot from the
/// append-only store; the same content re-fetched later is a duplicate.
#[derive(Debug, Default)]
pub struct DedupIndex {
    seen: BTreeSet<(String, String)>,
}

impl DedupIndex {
    pub fn new() -> DedupIndex {
        DedupIndex::default()
    }

    pub fn insert(&mut self, source: &str, content_hash: &str) -> bool {
        self.seen
            .insert((source.to_string(), content_hash.to_string()))
    }
}

/// Canonical content hash: SHA-256 over source, kind, and the payload's
/// canonical JSON (serde_json's default map is sorted, so key order
/// cannot defeat dedup). `received_at` is deliberately EXCLUDED — same
/// content at a different time IS the duplicate.
pub fn content_hash(source: &str, kind: &str, payload: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update([0u8]);
    hasher.update(kind.as_bytes());
    hasher.update([0u8]);
    hasher.update(payload.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// The funnel's verdict for one source batch.
#[derive(Debug)]
pub enum IngestOutcome {
    Accepted {
        envelopes: Vec<SignalEnvelope>,
        duplicates: usize,
    },
    /// Not in the registry: fail closed, nothing ingested.
    RefusedUnregistered,
    /// Registered but disabled: fail closed, nothing ingested.
    RefusedDisabled,
}

/// Normalize one source's batch: allowlist check FIRST (fail closed),
/// then envelope + dedup. `make_id` injects deterministic ids (the
/// runner's seeded IdGen in composition; tests use counters).
pub fn normalize_and_dedup(
    source_id: &str,
    raw: Vec<RawSignal>,
    registry: &SourceRegistry,
    dedup: &mut DedupIndex,
    mut make_id: impl FnMut(usize) -> String,
) -> IngestOutcome {
    let Some(entry) = registry.get(source_id) else {
        return IngestOutcome::RefusedUnregistered;
    };
    if !entry.enabled {
        return IngestOutcome::RefusedDisabled;
    }
    let mut envelopes = Vec::new();
    let mut duplicates = 0usize;
    for (n, r) in raw.into_iter().enumerate() {
        let hash = content_hash(source_id, &r.kind, &r.payload);
        if !dedup.insert(source_id, &hash) {
            duplicates += 1;
            continue;
        }
        envelopes.push(SignalEnvelope {
            signal_id: make_id(n),
            source: source_id.to_string(),
            kind: r.kind,
            received_at: r.received_at,
            payload: r.payload,
            content_hash: hash,
        });
    }
    IngestOutcome::Accepted {
        envelopes,
        duplicates,
    }
}

// ------------------------------------------------------- trigger engine

/// Declarative trigger rules (spec 5.8 fast loop).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerRule {
    /// Live price diverged from an open belief by at least this much.
    PriceBeliefDivergence { min_divergence_cents: i64 },
    /// A new signal of this (source, kind) arrived (e.g. an Aeolus run).
    NewSignalKind { source: String, kind: String },
    /// Case-insensitive keyword scan over payload STRING values (data,
    /// never instructions: matching only, nothing is interpreted).
    KeywordMatch { keywords: Vec<String> },
    /// Scheduled market open (the composition root raises these from the
    /// catalog; carried here so configs are self-describing).
    MarketOpen,
}

#[derive(Debug, Clone)]
pub struct TriggerEngineConfig {
    /// Coalescing window after a cycle completes (spec 5.8: a news burst
    /// is one decision, not five).
    pub debounce_ms: i64,
    pub rules: Vec<TriggerRule>,
}

/// The per-event serialization verdict for one trigger request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerDecision {
    /// No cycle in flight, outside debounce: wake the decision cycle.
    Fire,
    /// A cycle is running for this event: absorbed (counted as pending).
    CoalescedInFlight,
    /// Inside the post-completion debounce window: absorbed.
    CoalescedDebounce,
}

/// Per-event serialization + debounce state. Deterministic: all time is
/// the caller's injected clock.
#[derive(Debug)]
pub struct TriggerEngine {
    config: TriggerEngineConfig,
    in_flight: BTreeSet<String>,
    pending_during_flight: BTreeMap<String, u64>,
    last_completed: BTreeMap<String, UtcTimestamp>,
}

impl TriggerEngine {
    pub fn new(config: TriggerEngineConfig) -> TriggerEngine {
        TriggerEngine {
            config,
            in_flight: BTreeSet::new(),
            pending_during_flight: BTreeMap::new(),
            last_completed: BTreeMap::new(),
        }
    }

    /// Does any rule match this signal? (Pure; the caller maps matched
    /// signals to canonical events via the edges before requesting a
    /// cycle.)
    pub fn signal_matches(&self, source: &str, kind: &str, payload: &Value) -> bool {
        self.config.rules.iter().any(|rule| match rule {
            TriggerRule::NewSignalKind { source: s, kind: k } => s == source && k == kind,
            TriggerRule::KeywordMatch { keywords } => {
                let mut texts = Vec::new();
                collect_strings(payload, &mut texts);
                keywords.iter().any(|kw| {
                    let kw = kw.to_ascii_lowercase();
                    texts.iter().any(|t| t.to_ascii_lowercase().contains(&kw))
                })
            }
            _ => false,
        })
    }

    /// Does the divergence rule fire for this observed gap?
    pub fn divergence_matches(&self, divergence_cents: i64) -> bool {
        self.config.rules.iter().any(|rule| {
            matches!(rule, TriggerRule::PriceBeliefDivergence { min_divergence_cents }
                if divergence_cents.abs() >= *min_divergence_cents)
        })
    }

    /// Request a decision cycle for a canonical event. AT MOST ONE in
    /// flight per event; completions start a debounce window.
    pub fn request_cycle(&mut self, event_id: &str, now: UtcTimestamp) -> TriggerDecision {
        if self.in_flight.contains(event_id) {
            *self
                .pending_during_flight
                .entry(event_id.to_string())
                .or_insert(0) += 1;
            return TriggerDecision::CoalescedInFlight;
        }
        if let Some(done) = self.last_completed.get(event_id) {
            if now.epoch_millis() - done.epoch_millis() <= self.config.debounce_ms {
                return TriggerDecision::CoalescedDebounce;
            }
        }
        TriggerDecision::Fire
    }

    /// Mark the cycle started (idempotent: a second begin while in flight
    /// cannot corrupt the serialization).
    pub fn begin_cycle(&mut self, event_id: &str) {
        self.in_flight.insert(event_id.to_string());
    }

    /// Mark the cycle complete; returns how many triggers coalesced while
    /// it ran (reported, never silent — the caller audits and decides
    /// whether a follow-up cycle is warranted after the debounce).
    pub fn complete_cycle(&mut self, event_id: &str, now: UtcTimestamp) -> u64 {
        self.in_flight.remove(event_id);
        self.last_completed.insert(event_id.to_string(), now);
        self.pending_during_flight.remove(event_id).unwrap_or(0)
    }
}

/// Collect every string value in a JSON tree (keyword scan input).
fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}
