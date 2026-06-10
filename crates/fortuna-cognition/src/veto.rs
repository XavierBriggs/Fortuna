//! The model veto (spec Section 6, mech_extremes): REDUCE-ONLY by
//! construction. A veto can suppress a sized candidate or shrink it; no type
//! in this module can express adding a trade or growing one. Every
//! assessment is serializable for the append-only audit log, and the
//! suppressed/shrunk quantity is counterfactually scorable against the
//! market's observable settlement outcome, so veto value-add is a measured
//! quantity, not a belief.
//!
//! Phase 1 ships the scaffolding with a deterministic stub mind (BUILD_PLAN
//! T1.3); the Anthropic-backed mind arrives in Phase 2 (T2.5) behind the
//! same trait.

use async_trait::async_trait;
use fortuna_core::book::{FeeModel, FillRole};
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{Action, Contracts, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VetoError {
    /// The veto provider failed (outage, timeout, schema-invalid output).
    /// The RUNNER decides what an unanswered veto means (fail-closed:
    /// suppress, flagged as an error so it is excluded from value-add
    /// scoring); this crate only reports.
    #[error("veto provider error: {reason}")]
    Provider { reason: String },
    #[error("keep_bps out of range: {got} (must be 1..=9999)")]
    KeepBpsRange { got: u16 },
    #[error(
        "counterfactual asked to score {removed} contracts but the candidate \
         had only {candidate_qty} (a score beyond the vetoed quantity would \
         fabricate an audit record)"
    )]
    RemovedExceedsCandidate { removed: i64, candidate_qty: i64 },
    #[error(transparent)]
    Money(#[from] fortuna_core::money::MoneyError),
    #[error(transparent)]
    Fee(#[from] fortuna_core::book::FeeError),
}

/// A shrink factor in basis points, 1..=9999 by construction. 0 cannot be
/// expressed (that is `Suppress`, say so explicitly) and 10000+ cannot be
/// expressed (keep-all is `Allow`; more would be a grow, which the veto
/// must never do). Serde round-trips THROUGH the checked constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct KeepBps(u16);

impl KeepBps {
    pub fn new(bps: u16) -> Result<Self, VetoError> {
        if (1..=9_999).contains(&bps) {
            Ok(KeepBps(bps))
        } else {
            Err(VetoError::KeepBpsRange { got: bps })
        }
    }

    pub fn raw(self) -> u16 {
        self.0
    }

    /// Floor-rounded application: `floor(qty * keep / 10000)`, clamped at
    /// zero. Floor means a shrink NEVER rounds back up to the original
    /// size; a shrink-to-zero is reported as zero and the caller treats it
    /// as a suppression with shrink provenance.
    pub fn apply(self, qty: Contracts) -> Contracts {
        let q = qty.raw().max(0);
        // i64 headroom: q <= i64::MAX, bps <= 9999; q * bps can overflow
        // only for q > ~9.2e14, clamp via i128 to stay exact.
        let kept = (i128::from(q) * i128::from(self.0)) / 10_000;
        // kept < q <= i64::MAX, the cast is lossless.
        Contracts::new(kept as i64)
    }
}

impl TryFrom<u16> for KeepBps {
    type Error = VetoError;
    fn try_from(v: u16) -> Result<Self, Self::Error> {
        KeepBps::new(v)
    }
}

impl From<KeepBps> for u16 {
    fn from(k: KeepBps) -> u16 {
        k.0
    }
}

/// What the veto may do. There is deliberately no variant that increases
/// size or introduces a trade (spec Section 6: "suppress or shrink, never
/// add or grow").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VetoVerdict {
    Allow,
    Shrink { keep: KeepBps, reason: String },
    Suppress { reason: String },
}

/// The sized candidate order the veto is consulted about: a point-in-time
/// snapshot of what the harness intends, sufficient to replay the decision.
/// `thesis` is strategy-authored text and `category`/market metadata are
/// venue-derived — all of it is DATA for the mind, never instructions
/// (spec 5.11 injection discipline applies when a real mind reads it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VetoCandidate {
    pub strategy: StrategyId,
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub limit_price: Cents,
    pub fair_value: Cents,
    pub qty: Contracts,
    pub yes_bid: Option<Cents>,
    pub yes_ask: Option<Cents>,
    pub category: Option<String>,
    pub thesis: String,
    pub as_of: UtcTimestamp,
}

/// An assessment: the verdict plus what answering cost (model spend is
/// tracked from day one; the stub costs zero).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VetoAssessment {
    pub verdict: VetoVerdict,
    pub cost_cents: i64,
}

/// The veto interface. Mirrors the spec 5.9 `Mind` shape (`&self`,
/// `Send + Sync`, async) so the Phase 2 Anthropic-backed implementation
/// drops in behind it.
#[async_trait]
pub trait VetoMind: Send + Sync {
    fn id(&self) -> &str;
    async fn assess(&self, candidate: &VetoCandidate) -> Result<VetoAssessment, VetoError>;
}

enum StubMode {
    AllowAll,
    Scripted(BTreeMap<MarketId, VetoVerdict>),
    Failing(String),
}

/// Deterministic stand-in mind (DST and Phase 1). Same construction + same
/// inputs => same outputs, no clocks, no randomness, zero cost.
pub struct StubVetoMind {
    mode: StubMode,
}

impl StubVetoMind {
    pub fn allow_all() -> Self {
        StubVetoMind {
            mode: StubMode::AllowAll,
        }
    }

    /// Verdicts keyed by market; unscripted markets get `Allow` (the
    /// veto's null action is not interfering).
    pub fn scripted(verdicts: Vec<(MarketId, VetoVerdict)>) -> Self {
        StubVetoMind {
            mode: StubMode::Scripted(verdicts.into_iter().collect()),
        }
    }

    /// Always errors: exercises the runner's provider-down path.
    pub fn failing(reason: impl Into<String>) -> Self {
        StubVetoMind {
            mode: StubMode::Failing(reason.into()),
        }
    }
}

#[async_trait]
impl VetoMind for StubVetoMind {
    fn id(&self) -> &str {
        "stub-veto"
    }

    async fn assess(&self, candidate: &VetoCandidate) -> Result<VetoAssessment, VetoError> {
        let verdict = match &self.mode {
            StubMode::AllowAll => VetoVerdict::Allow,
            StubMode::Scripted(map) => map
                .get(&candidate.market)
                .cloned()
                .unwrap_or(VetoVerdict::Allow),
            StubMode::Failing(reason) => {
                return Err(VetoError::Provider {
                    reason: reason.clone(),
                })
            }
        };
        Ok(VetoAssessment {
            verdict,
            cost_cents: 0,
        })
    }
}

/// How the counterfactual assumes the removed order would have executed.
/// v1 assumes a maker fill at the limit price — optimistic for the trade
/// (and therefore the HARSHEST framing for the veto when the trade would
/// have won); whether the resting order would actually have filled is
/// unknowable, so the assumption is recorded alongside every score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillAssumption {
    FilledAtLimit,
}

/// The hypothetical net PnL (cents) of the REMOVED quantity, scored at
/// settlement against the market's observable outcome:
///
/// - Buy:  `removed x (settle_value - limit) - maker_fee`
/// - Sell: `removed x (limit - settle_value) - maker_fee`
///
/// where `settle_value` is `payout_per_contract` when the candidate's side
/// won, else zero. Positive = the veto forfeited profit; negative = the
/// veto avoided a loss. Maker fee per the maker-only doctrine; fees are
/// incurred on fill regardless of outcome.
pub fn counterfactual_pnl(
    candidate: &VetoCandidate,
    removed: Contracts,
    winner: Side,
    payout_per_contract: Cents,
    fees: &dyn FeeModel,
    assumption: FillAssumption,
) -> Result<Cents, VetoError> {
    let FillAssumption::FilledAtLimit = assumption;
    if removed.raw() <= 0 {
        return Ok(Cents::ZERO);
    }
    if removed.raw() > candidate.qty.raw().max(0) {
        return Err(VetoError::RemovedExceedsCandidate {
            removed: removed.raw(),
            candidate_qty: candidate.qty.raw(),
        });
    }
    let settle_value = if winner == candidate.side {
        payout_per_contract
    } else {
        Cents::ZERO
    };
    let per_contract = match candidate.action {
        Action::Buy => settle_value.checked_sub(candidate.limit_price)?,
        Action::Sell => candidate.limit_price.checked_sub(settle_value)?,
    };
    let gross = per_contract.checked_mul(removed.raw())?;
    let fee = fees.fee(
        FillRole::Maker,
        candidate.limit_price,
        removed,
        candidate.category.as_deref(),
        candidate.as_of,
    )?;
    Ok(gross.checked_sub(fee)?)
}
