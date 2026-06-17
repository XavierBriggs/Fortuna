//! Discovery loops (spec 5.12): market-back (primary, daily) and
//! world-forward (secondary, budget-capped).
//!
//! Market-back: venue catalogs -> deterministic PREFILTER (category
//! allowlist, volume floor, resolution clarity, category calibration
//! record; exclusions counted) -> cheap-tier mind NORMALIZES survivors
//! into canonical events (match-before-create) -> edges proposed with
//! confidence, scored by the deterministic checks, and surfaced as
//! CONFIRMATION CARDS for the operator review queue. Matched events
//! with open beliefs wake the decision cycle (the early-arrival path).
//!
//! World-forward: candidate events synthesized from the signals store,
//! attached beliefs cost no capital. Candidates MUST declare a
//! resolution source present and enabled in the source registry —
//! otherwise the event is UNSCOREABLE: excluded from watchlist counts
//! and calibration, and beliefs on it are REFUSED (no beliefs nobody
//! can grade). Hard daily cost cap, checked BEFORE spending; this loop
//! is the first thing throttled under budget pressure.
//!
//! Both mind contracts ride in the journal body as strict JSON (the
//! same vehicle as the weekly review): free prose degrades to an empty
//! outcome with a recorded defect, never a guess and never a crash.

use crate::beliefs::BeliefDraft;
use crate::context::{assemble_context, AssemblerConfig, ContextItem};
use crate::events::{deterministic_edge_score, EdgeCheckInputs, MappingType};
use crate::mind::Mind;
use crate::signals::SourceRegistry;
use fortuna_core::clock::UtcTimestamp;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("context assembly failed: {0}")]
    Context(#[from] crate::context::ContextError),
}

// ---------------------------------------------------------------- budget

const DAY_MS: i64 = 86_400_000;

/// The discovery loops' hard daily cost cap (spec 5.12). Checked BEFORE
/// each mind call: an exhausted budget throttles the loop without
/// spending. Resets at 00:00 UTC.
#[derive(Debug)]
pub struct DiscoveryBudget {
    cap_cents: i64,
    spent_cents: i64,
    day_epoch: i64,
}

impl DiscoveryBudget {
    pub fn new(cap_cents: i64) -> DiscoveryBudget {
        DiscoveryBudget {
            cap_cents,
            spent_cents: 0,
            day_epoch: -1,
        }
    }

    fn roll(&mut self, now: UtcTimestamp) {
        let day = now.epoch_millis().div_euclid(DAY_MS);
        if day != self.day_epoch {
            self.day_epoch = day;
            self.spent_cents = 0;
        }
    }

    /// True when another call may spend today.
    pub fn allows(&mut self, now: UtcTimestamp) -> bool {
        self.roll(now);
        self.spent_cents < self.cap_cents
    }

    pub fn record_spend(&mut self, cents: i64, now: UtcTimestamp) {
        self.roll(now);
        self.spent_cents = self.spent_cents.saturating_add(cents.max(0));
    }

    pub fn spent_today_cents(&self) -> i64 {
        self.spent_cents
    }
}

// -------------------------------------------------------------- prefilter

/// A venue catalog listing as the prefilter consumes it (built from
/// `fortuna_venues::Market` by the composition).
#[derive(Debug, Clone)]
pub struct MarketView {
    pub market_id: String,
    pub venue: String,
    pub title: String,
    pub category: String,
    pub volume_contracts: Option<i64>,
    pub resolution_source: String,
    pub close_at: Option<UtcTimestamp>,
}

#[derive(Debug, Clone)]
pub struct PrefilterConfig {
    pub category_allowlist: Vec<String>,
    pub min_volume_contracts: i64,
    /// Categories whose calibration record sits below this quality are
    /// excluded (the record says we cannot price them yet).
    pub min_category_quality: f64,
    /// Calibration quality per category (T2.8 `calibration_quality`,
    /// queried by the composition from the resolved record).
    pub category_quality: BTreeMap<String, f64>,
}

#[derive(Debug)]
pub struct PrefilterOutcome {
    pub survivors: Vec<MarketView>,
    /// (market_id, reason) for every exclusion — counted, never silent.
    pub excluded: Vec<(String, String)>,
}

/// The deterministic market-back prefilter (spec 5.12). Order of checks
/// is fixed so exclusion reasons are stable.
pub fn prefilter(markets: &[MarketView], config: &PrefilterConfig) -> PrefilterOutcome {
    let mut survivors = Vec::new();
    let mut excluded = Vec::new();
    for market in markets {
        if !config.category_allowlist.contains(&market.category) {
            excluded.push((
                market.market_id.clone(),
                format!("category '{}' not in allowlist", market.category),
            ));
            continue;
        }
        let volume = market.volume_contracts.unwrap_or(0);
        if volume < config.min_volume_contracts {
            excluded.push((
                market.market_id.clone(),
                format!(
                    "volume {volume} below floor {}",
                    config.min_volume_contracts
                ),
            ));
            continue;
        }
        if market.resolution_source.trim().is_empty() {
            excluded.push((
                market.market_id.clone(),
                "no checkable resolution source".to_string(),
            ));
            continue;
        }
        let quality = config
            .category_quality
            .get(&market.category)
            .copied()
            .unwrap_or(0.0);
        if quality < config.min_category_quality {
            excluded.push((
                market.market_id.clone(),
                format!(
                    "category calibration record {quality:.2} below {:.2}",
                    config.min_category_quality
                ),
            ));
            continue;
        }
        survivors.push(market.clone());
    }
    PrefilterOutcome {
        survivors,
        excluded,
    }
}

/// Deterministic tradability score in [0,1] (spec 5.12: persisted per
/// market): volume factor (saturating at `volume_norm`) x the category's
/// calibration quality. A market with no checkable resolution source
/// scores zero regardless of liquidity.
pub fn tradability_score(market: &MarketView, category_quality: f64, volume_norm: i64) -> f64 {
    if market.resolution_source.trim().is_empty() {
        return 0.0;
    }
    let volume = market.volume_contracts.unwrap_or(0).max(0) as f64;
    let norm = volume_norm.max(1) as f64;
    let volume_factor = (volume / norm).min(1.0);
    (volume_factor * category_quality.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

// ------------------------------------------------------------ market-back

/// An existing canonical event as the matcher sees it.
#[derive(Debug, Clone)]
pub struct ExistingEventView {
    pub event_id: String,
    pub resolution_source: String,
    pub horizon: Option<UtcTimestamp>,
    pub has_open_belief: bool,
}

/// One normalization entry in the mind's strict contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NormalizationEntry {
    market_id: String,
    matches_event_id: Option<String>,
    statement: Option<String>,
    resolution_criteria: Option<String>,
    resolution_source: String,
    horizon: Option<UtcTimestamp>,
    category: String,
    mapping: MappingType,
    confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NormalizationBatch {
    normalizations: Vec<NormalizationEntry>,
}

/// Strict JSON schema for the market-back normalization batch (spec 5.12). Drives
/// the provider's structured output so a real model emits a conforming batch, not
/// prose. Every property is `required` (the structured-output layer); optionals
/// are nullable. The code is the authority — it re-validates on deserialize
/// (`deny_unknown_fields` + confidence/match-before-create checks).
fn normalization_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["normalizations"],
        "properties": {
            "normalizations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["market_id", "matches_event_id", "statement",
                        "resolution_criteria", "resolution_source", "horizon",
                        "category", "mapping", "confidence"],
                    "properties": {
                        "market_id": {"type": "string"},
                        "matches_event_id": {"type": ["string", "null"]},
                        "statement": {"type": ["string", "null"]},
                        "resolution_criteria": {"type": ["string", "null"]},
                        "resolution_source": {"type": "string"},
                        "horizon": {"type": ["string", "null"]},
                        "category": {"type": "string"},
                        "mapping": {"type": "string",
                            "enum": ["direct", "negation", "bracket_component", "conditional_on"]},
                        "confidence": {"type": "number"}
                    }
                }
            }
        }
    })
}

/// A market matched to an existing canonical event.
#[derive(Debug, Clone)]
pub struct MatchedMarket {
    pub market_id: String,
    pub event_id: String,
}

/// A draft for a NEW canonical event (no existing match).
#[derive(Debug, Clone)]
pub struct NewEventDraft {
    pub market_id: String,
    pub statement: String,
    pub resolution_criteria: String,
    pub resolution_source: String,
    pub horizon: Option<UtcTimestamp>,
    pub category: String,
}

/// The operator review-queue item for one proposed edge (spec 5.12:
/// "#fortuna-review confirms the high-stakes ones"). Carries BOTH the
/// model's confidence and the deterministic check score; confirmation
/// itself is an operator action through EdgesRepo.
#[derive(Debug, Clone)]
pub struct EdgeConfirmationCard {
    pub market_id: String,
    pub event_id: String,
    pub mapping: MappingType,
    pub model_confidence: f64,
    pub deterministic_score: f64,
    /// Non-direct mappings and imperfect deterministic scores need a
    /// human: a wrong equivalence edge converts an arbitrage into an
    /// unhedged position (the UMA-mode failure).
    pub high_stakes: bool,
}

#[derive(Debug, Default)]
pub struct MarketBackOutcome {
    pub matched: Vec<MatchedMarket>,
    pub new_events: Vec<NewEventDraft>,
    pub edge_cards: Vec<EdgeConfirmationCard>,
    /// Matched events with open beliefs: the "market matched to event
    /// with open belief" trigger (the composition wakes the cycle).
    pub wake_events: Vec<String>,
    pub defects: Vec<String>,
    pub throttled: bool,
    pub manifest_hash: String,
    pub cost_cents: i64,
}

/// The market-back normalization step (mind-driven, budget-capped).
/// Survivors come from `prefilter`; existing events come from the
/// composition's category query (match-before-create).
pub async fn market_back_discovery(
    mind: &dyn Mind,
    context_items: &[ContextItem],
    survivors: &[MarketView],
    existing: &[ExistingEventView],
    budget: &mut DiscoveryBudget,
    now: UtcTimestamp,
) -> Result<MarketBackOutcome, DiscoveryError> {
    let mut outcome = MarketBackOutcome::default();
    // No survivors => nothing to normalize. Skip the mind call entirely (no
    // spend, no API round-trip, no throttle): the deterministic prefilter
    // already excluded every listing this segment, so there is no work for the
    // cheap tier to do. This is the common steady-state (most segments surface
    // no NEW un-edged listing), and it keeps the shared discovery budget for the
    // world-forward arm and the segments that DO have survivors.
    if survivors.is_empty() {
        return Ok(outcome);
    }
    if !budget.allows(now) {
        outcome.throttled = true;
        return Ok(outcome);
    }

    let assembler = AssemblerConfig {
        budget_chars: 100_000,
        anonymize: false,
    };
    let ctx = assemble_context(context_items, now, "market_back_discovery", &assembler)?;
    outcome.manifest_hash = ctx.manifest_hash.clone();

    // Structured output (spec 5.12): the provider's schema constraint makes the
    // model emit a NormalizationBatch directly, never free-text prose. StubMind
    // falls back to its scripted journal JSON via the trait default. We still
    // deserialize + validate in code — the schema guides, the code is authority.
    let decision = match mind.decide_structured(&ctx, normalization_schema()).await {
        Ok(decision) => decision,
        Err(e) => {
            outcome
                .defects
                .push(format!("mind failed: {e} (discovery degraded to none)"));
            return Ok(outcome);
        }
    };
    budget.record_spend(decision.cost_cents, now);
    outcome.cost_cents = decision.cost_cents;

    let batch: NormalizationBatch = match serde_json::from_value(decision.value) {
        Ok(batch) => batch,
        Err(e) => {
            outcome.defects.push(format!(
                "normalization body violated the contract (never repaired): {e}"
            ));
            return Ok(outcome);
        }
    };

    let survivor_index: BTreeMap<&str, &MarketView> = survivors
        .iter()
        .map(|m| (m.market_id.as_str(), m))
        .collect();
    let existing_index: BTreeMap<&str, &ExistingEventView> =
        existing.iter().map(|e| (e.event_id.as_str(), e)).collect();

    for entry in batch.normalizations {
        let Some(market) = survivor_index.get(entry.market_id.as_str()) else {
            outcome.defects.push(format!(
                "normalization names market '{}' outside the survivor set",
                entry.market_id
            ));
            continue;
        };
        if !(0.0..=1.0).contains(&entry.confidence) {
            outcome.defects.push(format!(
                "confidence {} outside [0,1] for market '{}'",
                entry.confidence, entry.market_id
            ));
            continue;
        }

        // Match-before-create: a claimed match must name a REAL event.
        let (event_id, event_source, event_horizon) = match &entry.matches_event_id {
            Some(claimed) => match existing_index.get(claimed.as_str()) {
                Some(event) => {
                    outcome.matched.push(MatchedMarket {
                        market_id: entry.market_id.clone(),
                        event_id: event.event_id.clone(),
                    });
                    if event.has_open_belief {
                        outcome.wake_events.push(event.event_id.clone());
                    }
                    (
                        event.event_id.clone(),
                        event.resolution_source.clone(),
                        event.horizon,
                    )
                }
                None => {
                    outcome.defects.push(format!(
                        "normalization for '{}' claims match to nonexistent event '{claimed}' \
                         (hallucinated match dropped)",
                        entry.market_id
                    ));
                    continue;
                }
            },
            None => {
                let (Some(statement), Some(criteria)) =
                    (entry.statement.clone(), entry.resolution_criteria.clone())
                else {
                    outcome.defects.push(format!(
                        "new-event normalization for '{}' missing statement or criteria",
                        entry.market_id
                    ));
                    continue;
                };
                let draft = NewEventDraft {
                    market_id: entry.market_id.clone(),
                    statement,
                    resolution_criteria: criteria,
                    resolution_source: entry.resolution_source.clone(),
                    horizon: entry.horizon,
                    category: entry.category.clone(),
                };
                let id_placeholder = format!("new:{}", entry.market_id);
                outcome.new_events.push(draft);
                (
                    id_placeholder,
                    entry.resolution_source.clone(),
                    entry.horizon,
                )
            }
        };

        // Deterministic checks score every proposal (spec 5.12); the
        // card carries both scores for the reviewer.
        let deterministic = deterministic_edge_score(&EdgeCheckInputs {
            event_resolution_source: &event_source,
            market_resolution_source: &market.resolution_source,
            event_horizon,
            market_close_at: market.close_at,
            horizon_tolerance_ms: DAY_MS,
        });
        let high_stakes = entry.mapping != MappingType::Direct || deterministic < 1.0;
        outcome.edge_cards.push(EdgeConfirmationCard {
            market_id: entry.market_id.clone(),
            event_id,
            mapping: entry.mapping,
            model_confidence: entry.confidence,
            deterministic_score: deterministic,
            high_stakes,
        });
    }
    Ok(outcome)
}

// ---------------------------------------------------------- world-forward

/// One world-forward candidate in the mind's strict contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchlistEntry {
    event_hint: String,
    statement: String,
    resolution_criteria: String,
    resolution_source: String,
    horizon: Option<UtcTimestamp>,
    category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchlistBatch {
    candidates: Vec<WatchlistEntry>,
    /// Zero-capital beliefs the model attaches to its OWN candidates, riding the
    /// SAME structured payload as the candidates (spec 5.12). They were the one
    /// reason world-forward still needed `decide()` + `output.beliefs`; folding
    /// them into the structured batch lets the real model emit a typed payload
    /// (no free-text journal prose) exactly like market-back. The harness still
    /// enforces the unscoreable rule below — the schema guides, the code decides.
    beliefs: Vec<BeliefDraft>,
}

/// Strict JSON schema for the world-forward batch (spec 5.12): candidate events
/// PLUS the zero-capital beliefs on them, in ONE structured payload so a real
/// model emits conforming JSON, not prose (the root cause of "watchlist body
/// violated the contract"). The belief sub-schema mirrors the synthesis output
/// schema (`mind.rs::output_schema`). Every property is `required` (the
/// structured-output layer); `horizon` on a candidate is nullable. The code is
/// the authority — it re-validates on deserialize + enforces the unscoreable rule.
fn watchlist_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["candidates", "beliefs"],
        "properties": {
            "candidates": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["event_hint", "statement", "resolution_criteria",
                        "resolution_source", "horizon", "category"],
                    "properties": {
                        "event_hint": {"type": "string"},
                        "statement": {"type": "string"},
                        "resolution_criteria": {"type": "string"},
                        "resolution_source": {"type": "string"},
                        "horizon": {"type": ["string", "null"]},
                        "category": {"type": "string"}
                    }
                }
            },
            "beliefs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["event_id", "p", "p_raw", "horizon", "evidence"],
                    "properties": {
                        "event_id": {"type": "string"},
                        "p": {"type": "number"},
                        "p_raw": {"type": "number"},
                        "horizon": {"type": "string"},
                        "evidence": {"type": "array", "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["source"],
                            "properties": {
                                "source": {"type": "string"},
                                "ref": {"type": "string"},
                                "weight_note": {"type": "string"}
                            }
                        }}
                    }
                }
            }
        }
    })
}

/// A synthesized watchlist event (no market edges yet).
#[derive(Debug, Clone)]
pub struct WatchlistCandidate {
    /// `watch:{event_hint}` — the harness-owned id namespace.
    pub event_id: String,
    pub statement: String,
    pub resolution_criteria: String,
    pub resolution_source: String,
    pub horizon: Option<UtcTimestamp>,
    pub category: String,
    /// True when the declared resolution source is not a checkable,
    /// enabled registry source: excluded from watchlist counts and
    /// calibration (spec 5.12 — no beliefs nobody can grade).
    pub unscoreable: bool,
}

#[derive(Debug, Default)]
pub struct WatchlistOutcome {
    pub candidates: Vec<WatchlistCandidate>,
    /// Beliefs attached to SCOREABLE declared candidates only.
    pub beliefs: Vec<BeliefDraft>,
    pub defects: Vec<String>,
    pub throttled: bool,
    pub manifest_hash: String,
    pub cost_cents: i64,
}

/// A persisted watchlist event as the counter sees it.
#[derive(Debug, Clone)]
pub struct WatchlistEventView {
    pub event_id: String,
    pub unscoreable: bool,
}

/// Watchlist size for budgeting and review: unscoreable events do not
/// count (spec 5.12).
pub fn watchlist_count(events: &[WatchlistEventView]) -> usize {
    events.iter().filter(|e| !e.unscoreable).count()
}

/// The world-forward loop (spec 5.12): synthesize candidate events from
/// the signals store, attach zero-capital beliefs. Hard daily cost cap
/// checked BEFORE spending — this loop throttles first under pressure.
pub async fn world_forward_discovery(
    mind: &dyn Mind,
    context_items: &[ContextItem],
    registry: &SourceRegistry,
    budget: &mut DiscoveryBudget,
    now: UtcTimestamp,
) -> Result<WatchlistOutcome, DiscoveryError> {
    let mut outcome = WatchlistOutcome::default();
    if !budget.allows(now) {
        outcome.throttled = true;
        return Ok(outcome);
    }

    let assembler = AssemblerConfig {
        budget_chars: 100_000,
        anonymize: false,
    };
    let ctx = assemble_context(context_items, now, "world_forward_discovery", &assembler)?;
    outcome.manifest_hash = ctx.manifest_hash.clone();

    // Structured output (spec 5.12): the provider's schema constraint makes the
    // model emit a WatchlistBatch (candidates + their zero-capital beliefs)
    // directly, never free-text prose. StubMind falls back to its scripted
    // journal JSON via the trait default. We still deserialize + validate in
    // code — the schema guides, the code is authority (the unscoreable rule below).
    let decision = match mind.decide_structured(&ctx, watchlist_schema()).await {
        Ok(decision) => decision,
        Err(e) => {
            outcome
                .defects
                .push(format!("mind failed: {e} (watchlist degraded to none)"));
            return Ok(outcome);
        }
    };
    budget.record_spend(decision.cost_cents, now);
    outcome.cost_cents = decision.cost_cents;

    let batch: WatchlistBatch = match serde_json::from_value(decision.value) {
        Ok(batch) => batch,
        Err(e) => {
            outcome.defects.push(format!(
                "watchlist body violated the contract (never repaired): {e}"
            ));
            return Ok(outcome);
        }
    };

    for entry in batch.candidates {
        // The unscoreable rule: the declared source must be a checkable,
        // ENABLED registry source at creation.
        let unscoreable = registry
            .get(&entry.resolution_source)
            .map(|s| !s.enabled)
            .unwrap_or(true);
        outcome.candidates.push(WatchlistCandidate {
            event_id: format!("watch:{}", entry.event_hint),
            statement: entry.statement,
            resolution_criteria: entry.resolution_criteria,
            resolution_source: entry.resolution_source,
            horizon: entry.horizon,
            category: entry.category,
            unscoreable,
        });
    }

    let scoreable: BTreeMap<&str, ()> = outcome
        .candidates
        .iter()
        .filter(|c| !c.unscoreable)
        .map(|c| (c.event_id.as_str(), ()))
        .collect();
    let unscoreable_ids: BTreeMap<&str, ()> = outcome
        .candidates
        .iter()
        .filter(|c| c.unscoreable)
        .map(|c| (c.event_id.as_str(), ()))
        .collect();

    // Provenance is HARNESS knowledge (spec 5.5), stamped post-call — the model
    // never writes its own. `decide()` stamps it for the synthesis path; the
    // structured channel hands back a raw Value, so we stamp it here so a
    // world-forward belief carries the same {model_id, context_manifest_hash,
    // cost_cents} audit trail it did before the structured-output cutover.
    let provenance = json!({
        "model_id": mind.id(),
        "context_manifest_hash": ctx.manifest_hash,
        "cost_cents": decision.cost_cents,
    });
    for mut belief in batch.beliefs {
        if scoreable.contains_key(belief.event_id.as_str()) {
            belief.provenance = provenance.clone();
            outcome.beliefs.push(belief);
        } else if unscoreable_ids.contains_key(belief.event_id.as_str()) {
            outcome.defects.push(format!(
                "belief on unscoreable event '{}' refused (no beliefs nobody can grade)",
                belief.event_id
            ));
        } else {
            outcome.defects.push(format!(
                "belief on undeclared event '{}' refused",
                belief.event_id
            ));
        }
    }
    Ok(outcome)
}
