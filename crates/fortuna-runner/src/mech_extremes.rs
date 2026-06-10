//! mech_extremes (spec Section 6 item 2): favorite-longshot fading at
//! price extremes in sub-$100k-volume markets, MAKER-ONLY.
//!
//! The documented retail longshot bias overprices longshots and
//! underprices favorites; in a binary market, fading the longshot IS
//! buying the favorite. The fee curve rewards extremes (quadratic
//! 0.07*p*(1-p) is smallest near 0c/100c), and low-volume markets are
//! where retail flow dominates. This strategy:
//!
//! - watches book snapshots for markets whose FAVORITE side trades at or
//!   above `extreme_min_cents` (in that side's own price space);
//! - requires catalog metadata: Trading status, a known close time at
//!   least `min_ms_to_close` away, and a KNOWN venue-reported volume at
//!   or under `max_volume_contracts` (contracts x $1/pair bounds dollar
//!   volume from above, so 100_000 contracts <=> the spec's sub-$100k
//!   criterion; unknown volume is a SKIP, never assumed small);
//! - proposes a single-leg BUY of the favorite that JOINS the own-side
//!   best bid (never crosses; `Urgency::Passive`), with
//!   `fair_value = limit + bias_premium_cents` (clamped to 99c) as the
//!   honest deterministic edge claim the gates re-check;
//! - fires once per (market, side, limit) book state.
//!
//! This strategy ships WITH the model veto (spec Section 6): enrollment
//! happens at composition time via `RunnerConfig::veto_strategies` — see
//! the veto wiring in `runner.rs` and `fortuna_cognition::veto`.

use crate::{
    CoreHandle, Proposal, ProposedLeg, RunnerError, Stage, Strategy, StrategyKind, Urgency,
};
use async_trait::async_trait;
use fortuna_core::book::OrderBook;
use fortuna_core::bus::{BusEvent, EventPayload};
use fortuna_core::market::{Action, MarketId, Side, StrategyId};
use fortuna_core::money::Cents;
use fortuna_venues::{Market, MarketStatus};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct MechExtremesConfig {
    /// A side is "at an extreme" when its own-space best bid is at or
    /// above this (51..=99; spec intent is the high-90s/low-single-digit
    /// zones, default composition uses 90).
    pub extreme_min_cents: i64,
    /// The honest deterministic edge claim added to the limit for
    /// `fair_value` (>= 1; from the favorite-longshot bias literature,
    /// operator-tuned).
    pub bias_premium_cents: i64,
    /// Volume cap in CONTRACTS. contracts x $1/pair bounds dollar volume
    /// from above: 100_000 here IS the spec's "sub-$100k-volume" rule
    /// under any venue definition of dollar volume.
    pub max_volume_contracts: i64,
    /// Skip markets closing sooner than this (ms): a maker order needs
    /// time to be traded through, and close-adjacent books gap.
    pub min_ms_to_close: i64,
}

/// One observed fade opportunity key: market, favorite side, join price.
type FadeKey = (MarketId, Side, i64);

pub struct MechExtremes {
    id: StrategyId,
    config: MechExtremesConfig,
    proposed: BTreeSet<FadeKey>,
    metrics: crate::StrategyMetrics,
}

impl MechExtremes {
    pub fn new(config: MechExtremesConfig) -> Result<Self, RunnerError> {
        if !(51..=99).contains(&config.extreme_min_cents) {
            return Err(RunnerError::Config {
                reason: format!(
                    "extreme_min_cents {} outside 51..=99 (an 'extreme' below a coin flip \
                     is not one, and 100c is unreachable for a live book)",
                    config.extreme_min_cents
                ),
            });
        }
        if config.bias_premium_cents < 1 {
            return Err(RunnerError::Config {
                reason: format!(
                    "bias_premium_cents {} < 1: without a premium there is no edge claim \
                     and nothing to propose",
                    config.bias_premium_cents
                ),
            });
        }
        if config.max_volume_contracts <= 0 {
            return Err(RunnerError::Config {
                reason: format!(
                    "max_volume_contracts {} filters every market",
                    config.max_volume_contracts
                ),
            });
        }
        if config.min_ms_to_close < 0 {
            return Err(RunnerError::Config {
                reason: format!("min_ms_to_close {} is negative", config.min_ms_to_close),
            });
        }
        Ok(MechExtremes {
            id: StrategyId::new("mech_extremes").map_err(|e| RunnerError::Config {
                reason: e.to_string(),
            })?,
            config,
            proposed: BTreeSet::new(),
            metrics: crate::StrategyMetrics::default(),
        })
    }

    /// Catalog guards: Trading, known far-enough close, known small volume.
    fn market_eligible(&self, meta: &Market, now: fortuna_core::clock::UtcTimestamp) -> bool {
        if meta.status != MarketStatus::Trading {
            return false;
        }
        match meta.close_at {
            // Unknown close time: a maker order could be sitting on a
            // market that closes in seconds. Skip.
            None => return false,
            Some(close) => {
                let remaining = close.epoch_millis() - now.epoch_millis();
                if remaining < self.config.min_ms_to_close {
                    return false;
                }
            }
        }
        match meta.volume_contracts {
            // Unknown volume: never assume small (the bias thesis only
            // holds in low-attention markets; a whale market with a
            // missing field must not slip in).
            None => false,
            Some(v) => v <= self.config.max_volume_contracts,
        }
    }

    /// Find the favorite-side fade on this book, if any: the side whose
    /// own-space best bid sits at/above the extreme threshold. Returns
    /// (side, join_limit_in_own_space). Book validation (bid < ask)
    /// makes a double-extreme book unrepresentable.
    fn extreme_fade(&self, book: &OrderBook) -> Option<(Side, Cents)> {
        let yes_bid = book.yes_bids.first()?.price.raw();
        let yes_ask = book.yes_asks.first()?.price.raw();
        if yes_bid >= self.config.extreme_min_cents {
            // YES is the favorite; join the YES bid (strictly under the
            // ask by book validity => never crosses).
            return Some((Side::Yes, Cents::new(yes_bid)));
        }
        // NO-space mirror: the NO best bid is 100 - yes_ask, the NO ask is
        // 100 - yes_bid; joining the NO bid stays under the NO ask for the
        // same reason.
        let no_bid = 100 - yes_ask;
        if no_bid >= self.config.extreme_min_cents {
            return Some((Side::No, Cents::new(no_bid)));
        }
        None
    }
}

#[async_trait]
impl Strategy for MechExtremes {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }

    fn kind(&self) -> StrategyKind {
        StrategyKind::Mechanical
    }

    fn stage(&self) -> Stage {
        Stage::Sim
    }

    async fn on_event(
        &mut self,
        ev: &BusEvent,
        core: &CoreHandle<'_>,
    ) -> Result<Vec<Proposal>, RunnerError> {
        self.metrics.events_seen += 1;
        let EventPayload::BookSnapshot { book, .. } = &ev.payload else {
            return Ok(Vec::new());
        };
        let Some(meta) = core.markets.get(&book.market) else {
            // A book for a market the catalog never described: no volume,
            // no status, no close time. No trade.
            return Ok(Vec::new());
        };
        if !self.market_eligible(meta, core.now) {
            return Ok(Vec::new());
        }
        let Some((side, limit)) = self.extreme_fade(book) else {
            return Ok(Vec::new());
        };
        // Defensive non-cross re-check in own-side space (book validation
        // already guarantees it; a maker-only strategy keeps its own belt).
        let counter_ask = match side {
            Side::Yes => book.yes_asks.first().map(|l| l.price.raw()),
            Side::No => book.yes_bids.first().map(|l| 100 - l.price.raw()),
        };
        match counter_ask {
            Some(a) if limit.raw() < a => {}
            _ => return Ok(Vec::new()),
        }
        // Honest fair value: limit + premium, clamped to 99c (a binary
        // contract is never worth 100c before settlement). If the clamp
        // eats the whole premium there is no edge claim left.
        let fair = (limit.raw() + self.config.bias_premium_cents).min(99);
        if fair <= limit.raw() {
            return Ok(Vec::new());
        }
        let key: FadeKey = (book.market.clone(), side, limit.raw());
        if !self.proposed.insert(key) {
            return Ok(Vec::new());
        }
        self.metrics.proposals_emitted += 1;
        Ok(vec![Proposal {
            legs: vec![ProposedLeg {
                market: book.market.clone(),
                side,
                action: Action::Buy,
                limit_price: limit,
                fair_value: Cents::new(fair),
            }],
            group_policy: None,
            urgency: Urgency::Passive,
            thesis: format!(
                "favorite-longshot fade: {side:?} favorite at {limit} (extreme >= {}c), \
                 volume {} <= {} contracts, premium {}c",
                self.config.extreme_min_cents,
                meta.volume_contracts.unwrap_or(-1),
                self.config.max_volume_contracts,
                self.config.bias_premium_cents
            ),
        }])
    }

    fn metrics(&self) -> crate::StrategyMetrics {
        self.metrics
    }
}
