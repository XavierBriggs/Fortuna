//! Composition wiring for cognition (T4.1 requirements 3 + 4) — the two
//! GAPS residue lines, closed as code with call sites:
//!
//! - `DegradeScrape` is the scrape-delta consumer for
//!   `fortuna_ops::alerts::degrade_alerts`: it remembers the last-seen
//!   counter totals, diffs per scrape (saturating — a process restart's
//!   counter reset is not a burst), and returns the alerts the daemon
//!   routes through Slack (every routed message also writes an audit
//!   row at the routing site).
//! - `calibration_for_scope` fetches the scope's latest fitted params +
//!   resolved history from the ledger and produces the
//!   `CalibrationContext` + sizing quality that feed
//!   `SynthesisStrategy` and `SimRunner::set_calibration_quality`. No
//!   params row => `None` (the strategy structurally prices no edge —
//!   that IS the design); a params row that does not PARSE is corrupt
//!   configuration and errors loudly, never a silent "uncalibrated".

use fortuna_cognition::beliefs::calibration_curve;
use fortuna_cognition::calibration::{calibration_quality, CalibrationParams};
use fortuna_cognition::cycle::{CalibrationContext, EdgeView};
use fortuna_cognition::events::{EdgeTier, MappingType};
use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo, EdgesRepo, LedgerError};
use fortuna_ops::alerts::{degrade_alerts, DegradeSignals, DegradeThresholds};
use fortuna_ops::MessageKind;
use sqlx::PgPool;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ComposeError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error(
        "calibration_params row for {scope} does not parse: {reason} (corrupt config; refusing)"
    )]
    CorruptParams { scope: String, reason: String },
    #[error(
        "confirmed edge {edge_id} has unknown mapping_type {mapping_type:?} \
         (data defect; refusing the synthesis edge load)"
    )]
    BadEdge {
        edge_id: String,
        mapping_type: String,
    },
}

/// Reliability-curve bucket count for the quality computation. Ten
/// deciles is the weekly-review convention; quality only needs a stable
/// grouping, not resolution.
const QUALITY_BUCKETS: usize = 10;

/// Fetch the synthesis scope's calibration state from the ledger.
/// Returns the context for `SynthesisConfig.calibration` and the quality
/// for `SimRunner::set_calibration_quality` (both fail-closed shapes).
pub async fn calibration_for_scope(
    params: &CalibrationParamsRepo,
    beliefs: &BeliefsRepo,
    model_id: &str,
    strategy: &str,
    category: &str,
    kind: &str,
) -> Result<(Option<CalibrationContext>, f64), ComposeError> {
    let row = params.latest(model_id, strategy, category, kind).await?;
    let stats = beliefs.resolved_stats(category).await?;
    let resolved_n = stats.len();
    let samples: Vec<(f64, bool)> = stats.iter().map(|s| (s.p, s.outcome)).collect();
    let curve = calibration_curve(&samples, QUALITY_BUCKETS);
    let quality = calibration_quality(&curve, resolved_n);

    let ctx = match row {
        None => None,
        Some(r) => {
            let parsed: CalibrationParams =
                serde_json::from_value(r.params).map_err(|e| ComposeError::CorruptParams {
                    scope: format!("{model_id}/{strategy}/{category}/{kind}"),
                    reason: e.to_string(),
                })?;
            Some(CalibrationContext {
                params: parsed,
                resolved_n,
            })
        }
    };
    Ok((ctx, quality))
}

/// `[synthesis]` config FILTERS for the daemon's confirmed-edge load (the
/// decision: config NARROWS, never DEFINES, the edge set). Absent fields mean
/// "no filter". A `[synthesis]` section's mere PRESENCE is what opts the daemon
/// into synthesis (wired at compose_runner, S3b); these fields only scope it.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SynthesisSection {
    /// Restrict to this venue (None = every venue).
    pub venue: Option<String>,
    /// Cap the edge count, truncating deterministically by edge id
    /// (None = no cap) — the conservative bound on synthesis breadth.
    pub max_edges: Option<usize>,
    // (a category allowlist is deferred to S3b: it needs an events-category
    //  join; the EdgeRow carries `venue` but not the event's category.)
    /// The CALIBRATION scope category (S5a). The synthesis arm prices an edge
    /// only when this is set AND a calibration_params row exists for the scope
    /// (model, "synth_events", this category, "platt"); absent => calibration
    /// None => the arm structurally prices nothing (fail closed). This is the
    /// OPERATOR-declared calibration scope, NOT a per-edge category filter.
    pub category: Option<String>,
}

/// `[mech_extremes]` opt-in for the favorite-longshot fade strategy (spec
/// Section 6 item 2). Its mere PRESENCE composes mech_extremes into the daemon
/// ALONGSIDE mech_structural, enrolled in the reduce-only model veto (the
/// strategy ships WITH its veto). Absent fields take conservative defaults; an
/// out-of-range value is a LOUD compose error (MechExtremes::new validates).
/// NOTE: sim markets carry no volume/close metadata, so mech_extremes is INERT
/// in pure-sim (it skips ineligible markets) until real markets arrive (T4.2);
/// the composition + veto enrollment is the deliverable here.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct MechExtremesSection {
    /// Own-space best-bid "extreme" threshold (51..=99; default 90).
    pub extreme_min_cents: Option<i64>,
    /// Honest edge premium added to the join limit for fair_value (>=1;
    /// default 2).
    pub bias_premium_cents: Option<i64>,
    /// Volume cap in CONTRACTS (the sub-$100k-volume rule; default 100_000).
    pub max_volume_contracts: Option<i64>,
    /// Skip markets closing sooner than this in ms (default 3_600_000 = 1h).
    pub min_ms_to_close: Option<i64>,
}

/// The daemon synthesis strategy's tradeable edge set
/// (docs/design/synthesis-edge-source-decision.md req 1 + 4): EdgesRepo
/// CONFIRMED-tier edges, mapped to the comparator's `EdgeView`, scoped by the
/// `[synthesis]` filters. CONFIRMED + CURRENT is enforced by `confirmed_edges`;
/// the filters only NARROW. An empty result is a VALID state (the daemon then
/// runs mechanically-only — fail closed, req 3). Returns `Err` on a ledger
/// fault or a corrupt edge row so the per-segment refresh (S4) can keep the
/// last-known set rather than trade a guessed one.
pub async fn synthesis_edges(
    pool: &PgPool,
    cfg: &SynthesisSection,
) -> Result<Vec<EdgeView>, ComposeError> {
    let mut rows = EdgesRepo::new(pool.clone()).confirmed_edges().await?;
    if let Some(venue) = &cfg.venue {
        rows.retain(|r| &r.venue == venue);
    }
    if let Some(max) = cfg.max_edges {
        // Deterministic truncation BY EDGE ID (req 4), independent of the
        // load's (created_at, edge_id) order.
        rows.sort_by(|a, b| a.edge_id.cmp(&b.edge_id));
        rows.truncate(max);
    }
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let mapping = match row.mapping_type.as_str() {
            "direct" => MappingType::Direct,
            "negation" => MappingType::Negation,
            "bracket_component" => MappingType::BracketComponent,
            "conditional_on" => MappingType::ConditionalOn,
            _ => {
                return Err(ComposeError::BadEdge {
                    edge_id: row.edge_id,
                    mapping_type: row.mapping_type,
                })
            }
        };
        views.push(EdgeView {
            market: row.market_id,
            event_id: row.event_id,
            mapping,
            // confirmed_edges only returns confirmed_by IS NOT NULL rows.
            tier: EdgeTier::Confirmed,
        });
    }
    Ok(views)
}

/// The degrade-alert scrape consumer (GAPS residue line 1). One instance
/// lives for the daemon's lifetime; feed it the runner's counter TOTALS
/// each scrape and route what it returns.
pub struct DegradeScrape {
    thresholds: DegradeThresholds,
    last_budget_breaches: u64,
    last_cognition_failures: u64,
}

impl DegradeScrape {
    pub fn new(thresholds: DegradeThresholds) -> DegradeScrape {
        DegradeScrape {
            thresholds,
            last_budget_breaches: 0,
            last_cognition_failures: 0,
        }
    }

    /// Diff the totals against the previous scrape and produce alerts.
    /// Saturating: a counter that went BACKWARD (restart) yields a zero
    /// delta, not an underflowed burst.
    pub fn scrape(
        &mut self,
        budget_breaches_total: u64,
        cognition_failures_total: u64,
    ) -> Vec<(MessageKind, String)> {
        let signals = DegradeSignals {
            budget_breaches_delta: budget_breaches_total.saturating_sub(self.last_budget_breaches),
            cognition_failures_delta: cognition_failures_total
                .saturating_sub(self.last_cognition_failures),
        };
        self.last_budget_breaches = budget_breaches_total;
        self.last_cognition_failures = cognition_failures_total;
        degrade_alerts(&signals, &self.thresholds)
    }
}
