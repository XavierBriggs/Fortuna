//! Config-driven fee engine. Spec 5.2: "fee schedules are data, not code."
//!
//! One engine interprets versioned schedules: quadratic p(1-p) coefficient |
//! flat bps | tiered, with maker/taker variants, category multipliers, and
//! effective_date selection (latest schedule at or before the trade time).
//! Rounding is ALWAYS up — fees never round in our favor. `Decimal` appears
//! here because this is a conversion boundary; results are integer `Cents`.
//!
//! Schedules are only trusted because they are continuously verified: every
//! fill reconciles charged vs modeled fee (adapter tasks); a mismatch writes
//! a discrepancy.

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{notional, Contracts};
use fortuna_core::money::Cents;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::VenueError;

/// Was the fill resting (maker) or aggressing (taker)?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRole {
    Maker,
    Taker,
}

/// The fee interface venues expose (spec 5.2 `Venue::fee_model`).
pub trait FeeModel: Send + Sync {
    /// Modeled fee for a fill. `price` is the per-contract price in cents on
    /// a $1-payout binary contract; `category` selects multipliers.
    fn fee(
        &self,
        role: FillRole,
        price: Cents,
        qty: Contracts,
        category: Option<&str>,
        at: UtcTimestamp,
    ) -> Result<Cents, VenueError>;
}

/// One raw schedule as it appears in config (TOML). Validated and compiled
/// by `ScheduleFeeModel::new`; malformed schedules fail at construction,
/// never at fee time.
#[derive(Debug, Clone, Deserialize)]
pub struct FeeSchedule {
    pub formula: FormulaKind,
    /// ISO8601 timestamp or date-only (interpreted as midnight UTC).
    pub effective_date: String,
    /// Quadratic coefficients as decimal strings (never floats in config).
    pub taker_coeff: Option<String>,
    pub maker_coeff: Option<String>,
    /// Flat basis points on notional.
    pub taker_bps: Option<u32>,
    pub maker_bps: Option<u32>,
    /// Tiered bps by notional bracket; last tier must be unbounded.
    pub taker_tiers: Option<Vec<Tier>>,
    pub maker_tiers: Option<Vec<Tier>>,
    /// Multipliers applied to the raw fee before rounding; absent => 1.
    pub category_multipliers: Option<BTreeMap<String, String>>,
    /// Cent rounding. Default `up` (against us: fees up, rebate magnitudes
    /// down). `half_even` only for venues that document banker's rounding
    /// (Polymarket US, per docs/research/venue/polymarket-fees-2026-06-09).
    pub rounding: Option<RoundingMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundingMode {
    #[default]
    Up,
    HalfEven,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormulaKind {
    Quadratic,
    FlatBps,
    Tiered,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tier {
    /// Inclusive upper notional bound in cents; None = unbounded.
    pub up_to_notional_cents: Option<i64>,
    pub bps: u32,
}

/// A compiled per-role rate.
#[derive(Debug, Clone)]
enum Rate {
    Quadratic(Decimal),
    FlatBps(Decimal),
    Tiered(Vec<(Option<i64>, Decimal)>),
    /// Missing variant: this role pays nothing under this schedule.
    Zero,
}

#[derive(Debug, Clone)]
struct Compiled {
    effective_at: UtcTimestamp,
    taker: Rate,
    maker: Rate,
    multipliers: BTreeMap<String, Decimal>,
    rounding: RoundingMode,
}

/// The one fee engine. Holds every compiled schedule version for a venue and
/// selects by effective date at fee time.
#[derive(Debug, Clone)]
pub struct ScheduleFeeModel {
    /// Sorted ascending by effective_at.
    schedules: Vec<Compiled>,
}

impl ScheduleFeeModel {
    pub fn new(raw: Vec<FeeSchedule>) -> Result<Self, VenueError> {
        let mut schedules = raw
            .into_iter()
            .map(compile)
            .collect::<Result<Vec<_>, _>>()?;
        schedules.sort_by_key(|s| s.effective_at);
        Ok(ScheduleFeeModel { schedules })
    }

    fn effective_at(&self, at: UtcTimestamp) -> Result<&Compiled, VenueError> {
        self.schedules
            .iter()
            .rev()
            .find(|s| s.effective_at <= at)
            .ok_or_else(|| VenueError::NoEffectiveSchedule {
                at: at.to_iso8601(),
            })
    }
}

impl FeeModel for ScheduleFeeModel {
    fn fee(
        &self,
        role: FillRole,
        price: Cents,
        qty: Contracts,
        category: Option<&str>,
        at: UtcTimestamp,
    ) -> Result<Cents, VenueError> {
        if !(0..=100).contains(&price.raw()) {
            return Err(VenueError::Invalid {
                reason: format!("price {} outside [0, 100] cents", price),
            });
        }
        if qty.raw() < 0 {
            return Err(VenueError::Invalid {
                reason: format!("negative quantity {qty}"),
            });
        }
        if qty.raw() == 0 {
            return Ok(Cents::ZERO);
        }
        let schedule = self.effective_at(at)?;
        let rate = match role {
            FillRole::Taker => &schedule.taker,
            FillRole::Maker => &schedule.maker,
        };

        let raw_dollars = match rate {
            Rate::Zero => return Ok(Cents::ZERO),
            Rate::Quadratic(coeff) => {
                let p = price.to_dollars();
                let one_minus_p = Decimal::ONE
                    .checked_sub(p)
                    .ok_or_else(|| overflow("quadratic 1-p"))?;
                coeff
                    .checked_mul(Decimal::from(qty.raw()))
                    .and_then(|x| x.checked_mul(p))
                    .and_then(|x| x.checked_mul(one_minus_p))
                    .ok_or_else(|| overflow("quadratic fee"))?
            }
            Rate::FlatBps(bps) => bps_fee(*bps, price, qty)?,
            Rate::Tiered(tiers) => {
                let n = notional(price, qty).map_err(VenueError::Money)?;
                let bps = tiers
                    .iter()
                    .find(|(bound, _)| bound.is_none_or(|b| n.raw() <= b))
                    .map(|(_, bps)| *bps)
                    .ok_or_else(|| overflow("tier selection"))?;
                bps_fee(bps, price, qty)?
            }
        };

        let multiplier = category
            .and_then(|c| schedule.multipliers.get(c))
            .copied()
            .unwrap_or(Decimal::ONE);
        let adjusted = raw_dollars
            .checked_mul(multiplier)
            .ok_or_else(|| overflow("category multiplier"))?;

        match schedule.rounding {
            // Against us in both directions: fees round up, rebate
            // magnitudes round down (ceil does both).
            RoundingMode::Up => Cents::from_dollars_ceil(adjusted).map_err(VenueError::Money),
            // Only for venues that document banker's rounding.
            RoundingMode::HalfEven => {
                Cents::from_dollars_half_even(adjusted).map_err(VenueError::Money)
            }
        }
    }
}

fn bps_fee(bps: Decimal, price: Cents, qty: Contracts) -> Result<Decimal, VenueError> {
    let n = notional(price, qty).map_err(VenueError::Money)?;
    n.to_dollars()
        .checked_mul(bps)
        .and_then(|x| x.checked_div(Decimal::from(10_000u32)))
        .ok_or_else(|| overflow("bps fee"))
}

fn overflow(what: &str) -> VenueError {
    VenueError::FeeConfig {
        reason: format!("decimal overflow computing {what}"),
    }
}

fn compile(raw: FeeSchedule) -> Result<Compiled, VenueError> {
    let effective_at = parse_effective_date(&raw.effective_date)?;
    let (taker, maker) = match raw.formula {
        FormulaKind::Quadratic => (
            raw.taker_coeff
                .as_deref()
                .map(|c| parse_coeff_non_negative(c).map(Rate::Quadratic))
                .transpose()?
                .unwrap_or(Rate::Zero),
            // Maker coefficients may be negative: per-fill maker REBATES are
            // real (Polymarket US theta = -0.0125, researched 2026-06-09).
            raw.maker_coeff
                .as_deref()
                .map(|c| parse_decimal(c).map(Rate::Quadratic))
                .transpose()?
                .unwrap_or(Rate::Zero),
        ),
        FormulaKind::FlatBps => (
            raw.taker_bps
                .map(|b| Rate::FlatBps(Decimal::from(b)))
                .unwrap_or(Rate::Zero),
            raw.maker_bps
                .map(|b| Rate::FlatBps(Decimal::from(b)))
                .unwrap_or(Rate::Zero),
        ),
        FormulaKind::Tiered => (
            raw.taker_tiers
                .as_deref()
                .map(compile_tiers)
                .transpose()?
                .unwrap_or(Rate::Zero),
            raw.maker_tiers
                .as_deref()
                .map(compile_tiers)
                .transpose()?
                .unwrap_or(Rate::Zero),
        ),
    };
    let mut multipliers = BTreeMap::new();
    for (category, raw_mult) in raw.category_multipliers.unwrap_or_default() {
        multipliers.insert(category, parse_coeff_non_negative(&raw_mult)?);
    }
    Ok(Compiled {
        effective_at,
        taker,
        maker,
        multipliers,
        rounding: raw.rounding.unwrap_or_default(),
    })
}

fn compile_tiers(tiers: &[Tier]) -> Result<Rate, VenueError> {
    if tiers.is_empty() {
        return Err(VenueError::FeeConfig {
            reason: "tiered formula requires at least one tier".into(),
        });
    }
    let mut compiled = Vec::with_capacity(tiers.len());
    let mut prev_bound: Option<i64> = None;
    for (i, tier) in tiers.iter().enumerate() {
        let is_last = i == tiers.len() - 1;
        match tier.up_to_notional_cents {
            Some(bound) => {
                if is_last {
                    return Err(VenueError::FeeConfig {
                        reason: "final tier must be unbounded (no up_to_notional_cents)".into(),
                    });
                }
                if bound <= 0 || prev_bound.is_some_and(|p| bound <= p) {
                    return Err(VenueError::FeeConfig {
                        reason: format!(
                            "tier bounds must be positive and strictly increasing (tier {i})"
                        ),
                    });
                }
                prev_bound = Some(bound);
            }
            None => {
                if !is_last {
                    return Err(VenueError::FeeConfig {
                        reason: format!("only the final tier may be unbounded (tier {i})"),
                    });
                }
            }
        }
        compiled.push((tier.up_to_notional_cents, Decimal::from(tier.bps)));
    }
    Ok(Rate::Tiered(compiled))
}

fn parse_decimal(raw: &str) -> Result<Decimal, VenueError> {
    Decimal::from_str(raw).map_err(|e| VenueError::FeeConfig {
        reason: format!("cannot parse coefficient {raw:?}: {e}"),
    })
}

fn parse_coeff_non_negative(raw: &str) -> Result<Decimal, VenueError> {
    let d = parse_decimal(raw)?;
    if d < Decimal::ZERO {
        return Err(VenueError::FeeConfig {
            reason: format!(
                "coefficient {raw:?} is negative; only maker coefficients may be rebates"
            ),
        });
    }
    Ok(d)
}

fn parse_effective_date(raw: &str) -> Result<UtcTimestamp, VenueError> {
    UtcTimestamp::parse_iso8601(raw)
        .or_else(|_| UtcTimestamp::parse_iso8601(&format!("{raw}T00:00:00.000Z")))
        .map_err(|e| VenueError::FeeConfig {
            reason: format!("cannot parse effective_date {raw:?}: {e}"),
        })
}
