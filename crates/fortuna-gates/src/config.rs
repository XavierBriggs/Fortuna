//! Gate configuration (spec 5.3 example + I1: "config-driven (TOML),
//! hot-reloadable only by the operator").
//!
//! Safety limits are explicit: a strategy or venue with no configured limits
//! cannot trade (fail-closed at evaluation time), never silently defaulted.

use serde::Deserialize;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GateError {
    #[error("invalid gate config: {reason}")]
    InvalidConfig { reason: String },
    #[error("cannot re-arm {scope}: not halted")]
    RearmNotHalted { scope: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct GateConfig {
    pub global: GlobalLimits,
    #[serde(default)]
    pub per_strategy: BTreeMap<String, StrategyLimits>,
    #[serde(default)]
    pub rate: BTreeMap<String, RateLimits>,
    /// Perps domain (spec 5.15). Additive: configs without it parse and
    /// validate unchanged; perp orders then fail closed at evaluation.
    #[serde(default)]
    pub perp: PerpConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlobalLimits {
    /// Check 2: order cost + open exposure must stay within this.
    pub max_total_exposure_cents: i64,
    /// Drawdown halt threshold (consumed by the drawdown monitor, T0.7;
    /// day boundary 00:00 UTC).
    pub max_daily_loss_cents: i64,
    /// Check 5 bounds.
    pub min_order_contracts: i64,
    pub max_order_contracts: i64,
    /// Check 4: max |limit - reference| in cents (yes-space).
    pub price_band_cents: i64,
    /// Check 4: max marketable crossing depth beyond the touch, in cents.
    pub max_cross_cents: i64,
    /// Check 3 (per-market arm).
    pub per_market_exposure_cents: i64,
    /// Check 9.
    pub per_event_exposure_cents: i64,
    /// Check 9: when true, an order on a market with no canonical-event
    /// mapping is rejected outright.
    pub require_event_mapping: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategyLimits {
    pub max_exposure_cents: i64,
    pub max_order_notional_cents: i64,
    pub min_net_edge_bps: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimits {
    /// Venue-level dual token bucket (I3).
    pub burst: u32,
    pub sustained_per_min: u32,
    /// Per-market bucket on the same venue.
    pub market_burst: u32,
    pub market_sustained_per_min: u32,
}

/// Perps gate configuration (spec 5.15 "New gates"): per-venue envelopes and
/// per-asset risk parameters. Fail-closed like everything else: a perp order
/// on a venue or asset with no entry here is rejected at evaluation.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PerpConfig {
    #[serde(default)]
    pub venues: BTreeMap<String, PerpVenueLimits>,
    #[serde(default)]
    pub assets: BTreeMap<String, PerpAssetLimits>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PerpVenueLimits {
    /// Per-venue perps notional cap (spec 5.15 gate (d)).
    pub max_total_notional_cents: i64,
    /// Perp order size bounds (the venue's own min is 1 contract,
    /// fractional disabled — research §3).
    pub min_order_contracts: i64,
    pub max_order_contracts: i64,
    /// Price sanity: max |limit - conservative mark| as bps of the mark.
    pub price_band_bps: i64,
    /// Fee-trap rule (spec 5.15): edge floors evaluate at this ASSUMED
    /// post-promo fee (bps of notional). Validation refuses 0 — promo-$0
    /// economics never reach edge math.
    pub assumed_fee_bps: i64,
    /// Funding drag (spec 5.15 gate (c)): assumed worst-case funding cost
    /// in bps of notional per 8h window, debited from edge over the
    /// candidate's intended holding windows.
    pub funding_drag_bps_per_window: i64,
    /// Liquidation-distance floor (spec 5.15 gate (a)), in bps of the
    /// conservative mark. 0 disables the floor (headroom still binds).
    pub min_liquidation_distance_bps: i64,
    /// Safety multiplier (percent) applied to the approximated maintenance
    /// margin. Must be >= 100: below that would WEAKEN the venue's own
    /// requirement. The venue's IM = 1.3 x MM suggests >= 130.
    pub mm_safety_multiplier_pct: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PerpAssetLimits {
    /// Per-asset leverage cap (spec 5.15 gate (b)), fixed-point x10
    /// (50 = 5.0x). Config-set at or below venue maxima.
    pub max_leverage_x10: i64,
    /// Per-asset worst-case position notional cap.
    pub max_notional_cents: i64,
    /// Maintenance-margin approximation (spec 5.15: the venue formula is
    /// UNPUBLISHED; this curve comes from recorded venue leverage_estimates
    /// by notional). Ascending (max_notional_cents, mm_bps) tiers; a
    /// notional beyond the last tier cannot be bounded and is REFUSED.
    pub mm_curve: Vec<(i64, i64)>,
}

impl GateConfig {
    pub fn validate(&self) -> Result<(), GateError> {
        let g = &self.global;
        let err = |reason: String| Err(GateError::InvalidConfig { reason });
        if g.max_total_exposure_cents <= 0 {
            return err("max_total_exposure_cents must be positive".into());
        }
        if g.min_order_contracts < 0 || g.max_order_contracts < g.min_order_contracts {
            return err("order contract bounds inverted or negative".into());
        }
        if g.price_band_cents < 0 || g.max_cross_cents < 0 {
            return err("price sanity bounds must be non-negative".into());
        }
        if g.per_market_exposure_cents <= 0 || g.per_event_exposure_cents <= 0 {
            return err("per-market/per-event exposure caps must be positive".into());
        }
        for (name, s) in &self.per_strategy {
            if s.max_exposure_cents <= 0 || s.max_order_notional_cents <= 0 {
                return err(format!("strategy {name}: caps must be positive"));
            }
        }
        for (venue, v) in &self.perp.venues {
            if v.max_total_notional_cents <= 0 {
                return err(format!("perp venue {venue}: notional cap must be positive"));
            }
            if v.min_order_contracts < 0 || v.max_order_contracts < v.min_order_contracts {
                return err(format!(
                    "perp venue {venue}: order contract bounds inverted or negative"
                ));
            }
            if v.price_band_bps < 0 {
                return err(format!(
                    "perp venue {venue}: price band must be non-negative"
                ));
            }
            if v.assumed_fee_bps < 1 {
                return err(format!(
                    "perp venue {venue}: assumed_fee_bps must be >= 1 (fee-trap rule, spec 5.15: \
                     promo-$0 fees never reach edge math)"
                ));
            }
            if v.funding_drag_bps_per_window < 0 {
                return err(format!(
                    "perp venue {venue}: funding drag must be non-negative"
                ));
            }
            if v.min_liquidation_distance_bps < 0 {
                return err(format!(
                    "perp venue {venue}: liquidation-distance floor must be non-negative"
                ));
            }
            if v.mm_safety_multiplier_pct < 100 {
                return err(format!(
                    "perp venue {venue}: mm_safety_multiplier_pct must be >= 100 (a smaller \
                     value would weaken the venue's own margin requirement)"
                ));
            }
        }
        for (asset, a) in &self.perp.assets {
            if a.max_leverage_x10 < 1 {
                return err(format!("perp asset {asset}: leverage cap must be >= 0.1x"));
            }
            if a.max_notional_cents <= 0 {
                return err(format!("perp asset {asset}: notional cap must be positive"));
            }
            if a.mm_curve.is_empty() {
                return err(format!(
                    "perp asset {asset}: mm_curve must be non-empty (no curve, no bound, no orders)"
                ));
            }
            let mut prev = 0i64;
            for (threshold, mm_bps) in &a.mm_curve {
                if *threshold <= prev {
                    return err(format!(
                        "perp asset {asset}: mm_curve thresholds must be positive and strictly ascending"
                    ));
                }
                if !(1..=10_000).contains(mm_bps) {
                    return err(format!(
                        "perp asset {asset}: mm_curve bps must be in [1, 10000]"
                    ));
                }
                prev = *threshold;
            }
        }
        Ok(())
    }
}
