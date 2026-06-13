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

use fortuna_cognition::basis::BracketStrike;
use fortuna_cognition::beliefs::calibration_curve;
use fortuna_cognition::calibration::{calibration_quality, CalibrationParams};
use fortuna_cognition::cycle::{CalibrationContext, EdgeView};
use fortuna_cognition::events::{EdgeTier, MappingType};
use fortuna_core::market::MarketId;
use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo, EdgesRepo, LedgerError};
use fortuna_ops::alerts::{degrade_alerts, DegradeSignals, DegradeThresholds};
use fortuna_ops::MessageKind;
use fortuna_runner::perp_event_basis::PerpEventBasisConfig;
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

/// `[funding_forecast]` opt-in (slice 4c): its mere PRESENCE composes the
/// zero-capital perp funding belief-producer (`FundingForecast`) into the
/// daemon — a propose-NOTHING strategy that drafts funding beliefs only. No
/// fields at rung-0 (the producer is config-free). Absent => not composed (fail
/// closed). Like `mech_extremes`, it is INERT in pure-sim: it fires only on
/// `EventPayload::PerpTick`s, which arrive only once a producer injects them
/// (the live kinetics feed, a later sub-slice). The composition is the
/// deliverable here.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FundingForecastSection {
    /// Slice 4e (Sim soak): a path to a `.jsonl` of RECORDED kinetics WS frames.
    /// When set, the daemon feeds the recorded `ticker` frames as `PerpTick`s
    /// one-per-segment (looping) so the perp producers FIRE in pure-sim — the
    /// composition is otherwise inert (no live perp feed). `None` => no feed
    /// (the producers compose but stay idle until a real PerpTick source).
    /// Recorded data ONLY; never fabricated.
    pub ticker_feed_jsonl: Option<String>,
}

/// `[perp_event_basis]` opt-in (slice 4c): its PRESENCE composes the
/// propose-only mechanical perp/bracket basis strategy (`PerpEventBasis`). The
/// bracket LADDER (market -> strike) is config-supplied because the venue
/// `Market` type carries no strike metadata, so the operator declares it. All
/// fields are REQUIRED (no silent default — a basis strategy with a guessed fee
/// trap or empty ladder is a money risk); `build_perp_event_basis_config`
/// validates the ladder STRICTLY. Absent => not composed (fail closed). Like
/// `mech_extremes` it is INERT in pure-sim: it fires only on perp `PerpTick`s
/// (a later kinetics-feed sub-slice injects them). The composition is the
/// deliverable here.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerpEventBasisSection {
    /// The perp whose `PerpTick` triggers the basis comparison (e.g.
    /// `"KXBTCPERP"`).
    pub perp_market: String,
    /// The assumed post-promo round-trip fee floor in dollars (the fee trap the
    /// signed basis must clear); passed straight to the basis kernel.
    pub fee_floor_dollars: f64,
    /// The additional configured edge margin in dollars; the basis must clear
    /// `fee_floor_dollars + min_basis_dollars`.
    pub min_basis_dollars: f64,
    /// The honest fair-value premium (cents) added to the join limit (the gates
    /// re-check net edge from it).
    pub edge_premium_cents: i64,
    /// The KXBTC bracket LADDER: each bracket venue market id (the map KEY) ->
    /// its strike kind. An empty ladder is an error (nothing to trade).
    pub ladder: std::collections::BTreeMap<String, BracketStrikeToml>,
}

/// One ladder rung in TOML (slice 4c): a venue market id (the map KEY in
/// [`PerpEventBasisSection::ladder`]) -> its strike kind. The strike fields are
/// OPTIONAL in the schema because each `kind` requires a DIFFERENT subset;
/// `build_perp_event_basis_config` enforces the per-kind requirements strictly.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BracketStrikeToml {
    /// `"between"` | `"greater"` | `"less"` (exact; any other value is an error).
    pub kind: String,
    /// The lower strike edge in dollars. REQUIRED for `between`/`greater`; MUST
    /// be absent for `less`.
    pub floor_dollars: Option<f64>,
    /// The upper strike edge in dollars. REQUIRED for `between`/`less`; MUST be
    /// absent for `greater`.
    pub cap_dollars: Option<f64>,
}

/// Build the runner config from a `[perp_event_basis]` section (slice 4c),
/// validating the ladder STRICTLY. Returns a descriptive error STRING on any
/// violation (the caller maps it to `DaemonError::Compose`). Mirrors the
/// `ReviewSection::to_thresholds` shim shape: a pure, unit-testable mapping from
/// the TOML section to the runner's `PerpEventBasisConfig`.
///
/// Validation (every rule is a refusal, never a silent fixup):
/// - the ladder must be NON-EMPTY (an empty ladder has nothing to trade);
/// - `perp_market` and every ladder KEY must be a valid `MarketId`;
/// - each rung's `kind` is exactly `"between"` | `"greater"` | `"less"`;
/// - `"between"` REQUIRES both `floor_dollars` and `cap_dollars` with
///   `floor < cap`; a missing strike or `floor >= cap` is an error;
/// - `"greater"` REQUIRES `floor_dollars` and `cap_dollars` MUST be absent;
/// - `"less"` REQUIRES `cap_dollars` and `floor_dollars` MUST be absent.
pub(crate) fn build_perp_event_basis_config(
    section: &PerpEventBasisSection,
) -> Result<PerpEventBasisConfig, String> {
    if section.ladder.is_empty() {
        return Err("ladder is empty (nothing to trade)".to_string());
    }
    let perp_market = MarketId::new(section.perp_market.clone())
        .map_err(|e| format!("perp_market {:?}: {e}", section.perp_market))?;

    let mut ladder = std::collections::BTreeMap::new();
    for (market_id, rung) in &section.ladder {
        let mkt = MarketId::new(market_id.clone())
            .map_err(|e| format!("ladder market id {market_id:?}: {e}"))?;
        let strike = match rung.kind.as_str() {
            "between" => {
                let floor = rung.floor_dollars.ok_or_else(|| {
                    format!("ladder rung {market_id:?}: \"between\" requires floor_dollars")
                })?;
                let cap = rung.cap_dollars.ok_or_else(|| {
                    format!("ladder rung {market_id:?}: \"between\" requires cap_dollars")
                })?;
                if floor >= cap {
                    return Err(format!(
                        "ladder rung {market_id:?}: \"between\" requires floor < cap \
                         (got floor={floor}, cap={cap})"
                    ));
                }
                BracketStrike::Between { floor, cap }
            }
            "greater" => {
                let floor = rung.floor_dollars.ok_or_else(|| {
                    format!("ladder rung {market_id:?}: \"greater\" requires floor_dollars")
                })?;
                if rung.cap_dollars.is_some() {
                    return Err(format!(
                        "ladder rung {market_id:?}: \"greater\" must not carry cap_dollars"
                    ));
                }
                BracketStrike::Greater { floor }
            }
            "less" => {
                let cap = rung.cap_dollars.ok_or_else(|| {
                    format!("ladder rung {market_id:?}: \"less\" requires cap_dollars")
                })?;
                if rung.floor_dollars.is_some() {
                    return Err(format!(
                        "ladder rung {market_id:?}: \"less\" must not carry floor_dollars"
                    ));
                }
                BracketStrike::Less { cap }
            }
            other => {
                return Err(format!(
                    "ladder rung {market_id:?}: unknown kind {other:?} \
                     (expected \"between\" | \"greater\" | \"less\")"
                ));
            }
        };
        ladder.insert(mkt, strike);
    }

    Ok(PerpEventBasisConfig {
        perp_market,
        ladder,
        fee_floor_dollars: section.fee_floor_dollars,
        min_basis_dollars: section.min_basis_dollars,
        edge_premium_cents: section.edge_premium_cents,
    })
}

/// `[review]` opt-in: the weekly review's GO/NO-GO thresholds (T4.1/M2; spec
/// 5.8 weekly review). ADVISORY ONLY — the review emits recommendations;
/// promotion is the human act (I7). Its PRESENCE composes the weekly/monthly
/// review cadence into the daemon (the wiring slice); absent => no review (fail
/// closed). Thresholds are REQUIRED — risk gates take no silent default, so a
/// missing field is a loud parse error.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewSection {
    /// A mechanical strategy needs at least this many paper days before a GO.
    pub min_paper_days_mechanical: u32,
    /// A synthesis strategy needs at least this many resolved beliefs before a GO.
    pub min_resolved_beliefs_synthesis: usize,
    /// NO-GO when fees exceed this fraction of realized PnL.
    pub max_fee_pnl_ratio: f64,
}

impl ReviewSection {
    /// Map to the cognition layer's GO/NO-GO thresholds (the weekly review
    /// consumes `fortuna_cognition::review::GoNoGoThresholds`).
    pub fn to_thresholds(&self) -> fortuna_cognition::review::GoNoGoThresholds {
        fortuna_cognition::review::GoNoGoThresholds {
            min_paper_days_mechanical: self.min_paper_days_mechanical,
            min_resolved_beliefs_synthesis: self.min_resolved_beliefs_synthesis,
            max_fee_pnl_ratio: self.max_fee_pnl_ratio,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A `[perp_event_basis]` section with the given ladder rungs and otherwise
    /// fixed non-vacuous scalars (so the validation under test is the ladder).
    fn section(ladder: Vec<(&str, BracketStrikeToml)>) -> PerpEventBasisSection {
        PerpEventBasisSection {
            perp_market: "KXBTCPERP".to_string(),
            fee_floor_dollars: 2.0,
            min_basis_dollars: 1.0,
            edge_premium_cents: 2,
            ladder: ladder
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    fn rung(kind: &str, floor: Option<f64>, cap: Option<f64>) -> BracketStrikeToml {
        BracketStrikeToml {
            kind: kind.to_string(),
            floor_dollars: floor,
            cap_dollars: cap,
        }
    }

    #[test]
    fn valid_three_kind_ladder_builds_the_right_strike_mapping() {
        // A well-formed 3-rung ladder (one less, one between, one greater) maps
        // each (market-id, BracketStrikeToml) -> (MarketId, BracketStrike) and
        // carries the scalars through. NON-VACUOUS: the three distinct strike
        // shapes + their dollar values are asserted exactly.
        let sec = section(vec![
            ("KXBTC-LO", rung("less", None, Some(60_000.0))),
            ("KXBTC-MID", rung("between", Some(60_000.0), Some(70_000.0))),
            ("KXBTC-HI", rung("greater", Some(70_000.0), None)),
        ]);
        let cfg = build_perp_event_basis_config(&sec).expect("a well-formed ladder builds");

        assert_eq!(cfg.perp_market, MarketId::new("KXBTCPERP").unwrap());
        assert_eq!(cfg.fee_floor_dollars, 2.0);
        assert_eq!(cfg.min_basis_dollars, 1.0);
        assert_eq!(cfg.edge_premium_cents, 2);
        assert_eq!(cfg.ladder.len(), 3);
        assert_eq!(
            cfg.ladder.get(&MarketId::new("KXBTC-LO").unwrap()),
            Some(&BracketStrike::Less { cap: 60_000.0 })
        );
        assert_eq!(
            cfg.ladder.get(&MarketId::new("KXBTC-MID").unwrap()),
            Some(&BracketStrike::Between {
                floor: 60_000.0,
                cap: 70_000.0
            })
        );
        assert_eq!(
            cfg.ladder.get(&MarketId::new("KXBTC-HI").unwrap()),
            Some(&BracketStrike::Greater { floor: 70_000.0 })
        );
    }

    #[test]
    fn empty_ladder_is_an_error() {
        let err = build_perp_event_basis_config(&section(vec![])).unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn unknown_kind_is_an_error() {
        let sec = section(vec![("KXBTC-X", rung("equal", Some(1.0), Some(2.0)))]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("unknown kind"), "{err}");
    }

    #[test]
    fn between_missing_a_strike_is_an_error() {
        // Missing cap.
        let sec = section(vec![("KXBTC-MID", rung("between", Some(60_000.0), None))]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("cap_dollars"), "{err}");
        // Missing floor.
        let sec = section(vec![("KXBTC-MID", rung("between", None, Some(70_000.0)))]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("floor_dollars"), "{err}");
    }

    #[test]
    fn between_with_floor_ge_cap_is_an_error() {
        // floor == cap.
        let sec = section(vec![(
            "KXBTC-MID",
            rung("between", Some(70_000.0), Some(70_000.0)),
        )]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("floor < cap"), "{err}");
        // floor > cap.
        let sec = section(vec![(
            "KXBTC-MID",
            rung("between", Some(80_000.0), Some(70_000.0)),
        )]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("floor < cap"), "{err}");
    }

    #[test]
    fn greater_with_a_cap_is_an_error() {
        let sec = section(vec![(
            "KXBTC-HI",
            rung("greater", Some(70_000.0), Some(80_000.0)),
        )]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("must not carry cap_dollars"), "{err}");
    }

    #[test]
    fn greater_missing_floor_is_an_error() {
        let sec = section(vec![("KXBTC-HI", rung("greater", None, None))]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("requires floor_dollars"), "{err}");
    }

    #[test]
    fn less_with_a_floor_is_an_error() {
        let sec = section(vec![(
            "KXBTC-LO",
            rung("less", Some(50_000.0), Some(60_000.0)),
        )]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("must not carry floor_dollars"), "{err}");
    }

    #[test]
    fn less_missing_cap_is_an_error() {
        let sec = section(vec![("KXBTC-LO", rung("less", None, None))]);
        let err = build_perp_event_basis_config(&sec).unwrap_err();
        assert!(err.contains("requires cap_dollars"), "{err}");
    }
}
