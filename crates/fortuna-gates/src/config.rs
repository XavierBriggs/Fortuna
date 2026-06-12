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
        Ok(())
    }
}
