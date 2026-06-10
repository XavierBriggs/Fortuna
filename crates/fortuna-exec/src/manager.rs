//! The order manager. Spec 5.4.
//!
//! Owns the intent journal and the fold of it (intent records). Every
//! mutation is journaled BEFORE the corresponding network call; the fold
//! (state machine) is one function used both live and during recovery, so a
//! rebuilt manager is byte-equivalent to one that never crashed. Delivery is
//! at-least-once everywhere: fill ingestion dedups by fill id; submission
//! dedups by client order id (derived from intent id, spec 5.4).

use crate::journal::{IntentEvent, IntentJournal, JournalRow, OrderSnapshot};
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::ids::{IntentGroupId, IntentId};
use fortuna_core::market::{Contracts, MarketId, Side, StrategyId, VenueOrderId};
use fortuna_venues::{Fill, Venue, VenueError};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecError {
    #[error("unknown intent {intent}")]
    UnknownIntent { intent: IntentId },
    #[error("intent {intent} already journaled")]
    DuplicateIntent { intent: IntentId },
    #[error("a working order already exists for ({strategy}, {market}, {side:?}): {existing}")]
    WorkingOrderExists {
        strategy: StrategyId,
        market: MarketId,
        side: Side,
        existing: IntentId,
    },
    #[error("fill {fill_id} would overfill intent {intent} ({cum} + {add} > {qty})")]
    Overfill {
        intent: IntentId,
        fill_id: String,
        cum: i64,
        add: i64,
        qty: i64,
    },
    #[error("orphan fill {fill_id}: no intent for client order id {client_order_id}")]
    OrphanFill {
        fill_id: String,
        client_order_id: String,
    },
    #[error("illegal transition for intent {intent}: {from} + {event}")]
    Transition {
        intent: IntentId,
        from: &'static str,
        event: &'static str,
    },
    #[error("journal error: {reason}")]
    Journal { reason: String },
    #[error(transparent)]
    Venue(#[from] VenueError),
    #[error(transparent)]
    Money(#[from] fortuna_core::money::MoneyError),
}

/// Folded intent state (the spec 5.4 state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentStatus {
    Created,
    Submitted,
    Acked,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    BootClosed,
}

impl IntentStatus {
    pub fn name(self) -> &'static str {
        match self {
            IntentStatus::Created => "created",
            IntentStatus::Submitted => "submitted",
            IntentStatus::Acked => "acked",
            IntentStatus::PartiallyFilled => "partially_filled",
            IntentStatus::Filled => "filled",
            IntentStatus::Cancelled => "cancelled",
            IntentStatus::Rejected => "rejected",
            IntentStatus::BootClosed => "boot_closed",
        }
    }

    /// Working = may have or acquire venue presence.
    pub fn is_working(self) -> bool {
        matches!(
            self,
            IntentStatus::Submitted | IntentStatus::Acked | IntentStatus::PartiallyFilled
        )
    }
}

#[derive(Debug, Clone)]
pub struct IntentRecord {
    pub order: OrderSnapshot,
    pub group: Option<IntentGroupId>,
    pub status: IntentStatus,
    pub venue_order_id: Option<VenueOrderId>,
    pub cum_filled: Contracts,
    pub cancel_requested: bool,
    pub created_at: UtcTimestamp,
    pub last_event_at: UtcTimestamp,
}

#[derive(Debug)]
pub enum SubmitOutcome {
    Acked {
        venue_order_id: VenueOrderId,
    },
    Rejected {
        reason: String,
    },
    /// The venue call's effect is unknown (timeout/outage mid-flight). The
    /// intent stays Submitted; boot/periodic reconciliation resolves it.
    Unknown {
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    Cancelled,
    /// The venue no longer knows the order (filled or already cancelled);
    /// journal says cancelled, fills reconcile the truth.
    AlreadyGone,
    /// Cancel timed out; the order may or may not be live. Status unchanged.
    Unknown,
}

#[derive(Debug, Clone)]
pub struct FillApplication {
    pub intent: IntentId,
    /// False when the fill id was already applied (at-least-once dedup).
    pub applied: bool,
    pub late_after_cancel: bool,
}

#[derive(Debug, Default)]
pub struct BootReport {
    pub adopted: Vec<IntentId>,
    pub orphans_cancelled: Vec<VenueOrderId>,
    pub closed_unsubmitted: Vec<IntentId>,
    pub missing_at_venue: Vec<IntentId>,
    pub recancelled_at_venue: Vec<VenueOrderId>,
    pub fills_applied: usize,
    pub orphan_fills: Vec<String>,
    pub discrepancies: Vec<String>,
}

/// Execution policy knobs (spec 5.4): TTL + one-working-order rule.
#[derive(Debug, Clone)]
pub struct ExecPolicy {
    pub default_ttl_ms: i64,
    pub per_strategy_ttl_ms: BTreeMap<String, i64>,
    /// Strategies explicitly allowed to ladder (stack working orders on one
    /// (strategy, market, side) key).
    pub laddering: BTreeSet<String>,
}

impl Default for ExecPolicy {
    fn default() -> Self {
        ExecPolicy {
            default_ttl_ms: 60_000,
            per_strategy_ttl_ms: BTreeMap::new(),
            laddering: BTreeSet::new(),
        }
    }
}

/// Read access to folded intent records (group tracker, watchdogs).
pub trait IntentView {
    fn intent_record(&self, id: IntentId) -> Option<&IntentRecord>;
}

/// The order manager. One per process; rebuilt from the journal at boot.
pub struct OrderManager<J: IntentJournal> {
    journal: J,
    clock: Arc<dyn Clock>,
    policy: ExecPolicy,
    intents: BTreeMap<IntentId, IntentRecord>,
    by_coid: BTreeMap<String, IntentId>,
    fills_seen: BTreeSet<String>,
}

impl<J: IntentJournal> OrderManager<J> {
    /// Build by folding the journal: THE crash-recovery path, also used for
    /// a fresh (empty) journal. A journal that does not fold cleanly is
    /// corrupt and refuses to load (fail-closed).
    pub fn recover(
        journal: J,
        clock: Arc<dyn Clock>,
        policy: ExecPolicy,
    ) -> Result<Self, ExecError> {
        let mut intents = BTreeMap::new();
        let mut by_coid = BTreeMap::new();
        let mut fills_seen = BTreeSet::new();
        for row in journal.rows() {
            Self::fold(&mut intents, &mut by_coid, &mut fills_seen, row)?;
        }
        Ok(OrderManager {
            journal,
            clock,
            policy,
            intents,
            by_coid,
            fills_seen,
        })
    }

    pub fn journal(&self) -> &J {
        &self.journal
    }

    /// Crash simulation / shutdown: hand the durable journal back.
    pub fn into_journal(self) -> J {
        self.journal
    }

    pub fn intent(&self, id: IntentId) -> Option<&IntentRecord> {
        self.intents.get(&id)
    }

    /// See [`IntentView`].
    pub fn intent_record(&self, id: IntentId) -> Option<&IntentRecord> {
        self.intents.get(&id)
    }

    pub fn intents(&self) -> Vec<(&IntentId, &IntentRecord)> {
        self.intents.iter().collect()
    }

    /// Client order ids known to this journal (gate check 8 input).
    pub fn known_client_order_ids(&self) -> BTreeSet<String> {
        self.by_coid.keys().cloned().collect()
    }

    /// The working order on (strategy, market, side), if any.
    pub fn working_order(
        &self,
        strategy: &StrategyId,
        market: &MarketId,
        side: Side,
    ) -> Option<IntentId> {
        self.intents
            .iter()
            .find(|(_, r)| {
                r.status.is_working()
                    && !r.cancel_requested
                    && &r.order.strategy == strategy
                    && &r.order.market == market
                    && r.order.side == side
            })
            .map(|(id, _)| *id)
    }

    /// Submit a gated order: journal Created + SubmitAttempted BEFORE the
    /// network call, then resolve the venue's answer.
    pub async fn submit(
        &mut self,
        order: fortuna_gates::GatedOrder,
        venue: &dyn Venue,
    ) -> Result<SubmitOutcome, ExecError> {
        self.submit_grouped(order, None, venue).await
    }

    pub async fn submit_grouped(
        &mut self,
        order: fortuna_gates::GatedOrder,
        group: Option<IntentGroupId>,
        venue: &dyn Venue,
    ) -> Result<SubmitOutcome, ExecError> {
        let snapshot = OrderSnapshot::from(&order);
        let intent = snapshot.intent_id;

        let is_resubmission = match self.intents.get(&intent) {
            None => false,
            Some(existing) => {
                // Idempotent at the manager level too: a known venue order
                // resolves without touching the venue.
                if let Some(vid) = &existing.venue_order_id {
                    return Ok(SubmitOutcome::Acked {
                        venue_order_id: vid.clone(),
                    });
                }
                match existing.status {
                    // Crash-resubmission of a maybe-placed intent: legal.
                    IntentStatus::Created | IntentStatus::Submitted => true,
                    // Resubmitting a dead intent is a caller bug.
                    _ => return Err(ExecError::DuplicateIntent { intent }),
                }
            }
        };

        if !is_resubmission {
            // One working order per (strategy, market, side) unless laddering.
            if !self.policy.laddering.contains(snapshot.strategy.as_str()) {
                if let Some(existing) =
                    self.working_order(&snapshot.strategy, &snapshot.market, snapshot.side)
                {
                    return Err(ExecError::WorkingOrderExists {
                        strategy: snapshot.strategy.clone(),
                        market: snapshot.market.clone(),
                        side: snapshot.side,
                        existing,
                    });
                }
            }
            self.append(
                intent,
                IntentEvent::Created {
                    order: snapshot.clone(),
                    group,
                    at: self.clock.now(),
                },
            )?;
        }

        self.append(
            intent,
            IntentEvent::SubmitAttempted {
                at: self.clock.now(),
            },
        )?;

        match venue.place(order).await {
            Ok(venue_order_id) => {
                self.append(
                    intent,
                    IntentEvent::Acked {
                        venue_order_id: venue_order_id.clone(),
                        at: self.clock.now(),
                    },
                )?;
                Ok(SubmitOutcome::Acked { venue_order_id })
            }
            Err(VenueError::AlreadyExists { existing }) => {
                // Crash-resubmission recovery: the order is already live.
                self.append(
                    intent,
                    IntentEvent::Acked {
                        venue_order_id: existing.clone(),
                        at: self.clock.now(),
                    },
                )?;
                Ok(SubmitOutcome::Acked {
                    venue_order_id: existing,
                })
            }
            Err(VenueError::Rejected { reason }) | Err(VenueError::Invalid { reason }) => {
                self.append(
                    intent,
                    IntentEvent::Rejected {
                        reason: reason.clone(),
                        at: self.clock.now(),
                    },
                )?;
                Ok(SubmitOutcome::Rejected { reason })
            }
            Err(VenueError::NotFound { what }) => {
                self.append(
                    intent,
                    IntentEvent::Rejected {
                        reason: format!("venue: not found: {what}"),
                        at: self.clock.now(),
                    },
                )?;
                Ok(SubmitOutcome::Rejected {
                    reason: format!("not found: {what}"),
                })
            }
            Err(ambiguous) => {
                // Timeout / outage / rate-limit: effect unknown. The intent
                // stays Submitted; reconciliation resolves it (spec 5.4).
                Ok(SubmitOutcome::Unknown {
                    error: ambiguous.to_string(),
                })
            }
        }
    }

    /// Ingest one fill (at-least-once delivery; dedup by fill id).
    pub fn ingest_fill(&mut self, fill: &Fill) -> Result<FillApplication, ExecError> {
        let Some(&intent) = self.by_coid.get(fill.client_order_id.as_str()) else {
            return Err(ExecError::OrphanFill {
                fill_id: fill.fill_id.clone(),
                client_order_id: fill.client_order_id.as_str().to_string(),
            });
        };
        if self.fills_seen.contains(&fill.fill_id) {
            let late = self
                .intents
                .get(&intent)
                .map(|r| r.status == IntentStatus::Cancelled)
                .unwrap_or(false);
            return Ok(FillApplication {
                intent,
                applied: false,
                late_after_cancel: late,
            });
        }
        let record = self
            .intents
            .get(&intent)
            .ok_or(ExecError::UnknownIntent { intent })?;
        let cum = record.cum_filled.raw();
        let add = fill.qty.raw();
        let qty = record.order.qty.raw();
        if cum + add > qty {
            return Err(ExecError::Overfill {
                intent,
                fill_id: fill.fill_id.clone(),
                cum,
                add,
                qty,
            });
        }
        let late_after_cancel = matches!(
            record.status,
            IntentStatus::Cancelled | IntentStatus::BootClosed
        );
        self.append(
            intent,
            IntentEvent::FillApplied {
                fill_id: fill.fill_id.clone(),
                venue_order_id: fill.venue_order_id.clone(),
                price: fill.price,
                qty: fill.qty,
                fee: fill.fee,
                is_maker: fill.is_maker,
                late_after_cancel,
                at: self.clock.now(),
            },
        )?;
        Ok(FillApplication {
            intent,
            applied: true,
            late_after_cancel,
        })
    }

    /// Cancel a working intent at the venue.
    pub async fn cancel_intent(
        &mut self,
        intent: IntentId,
        venue: &dyn Venue,
    ) -> Result<CancelOutcome, ExecError> {
        let record = self
            .intents
            .get(&intent)
            .ok_or(ExecError::UnknownIntent { intent })?;
        let Some(venue_order_id) = record.venue_order_id.clone() else {
            // No venue id (Submitted-unknown): cannot cancel yet; the caller
            // reconciles first.
            return Err(ExecError::Transition {
                intent,
                from: record.status.name(),
                event: "cancel (no venue order id)",
            });
        };
        self.append(
            intent,
            IntentEvent::CancelRequested {
                at: self.clock.now(),
            },
        )?;
        match venue.cancel(&venue_order_id).await {
            Ok(()) => {
                self.append(
                    intent,
                    IntentEvent::Cancelled {
                        reason: "cancelled at venue".into(),
                        at: self.clock.now(),
                    },
                )?;
                Ok(CancelOutcome::Cancelled)
            }
            Err(VenueError::NotFound { .. }) => {
                // Already gone (filled or cancelled): journal cancelled;
                // fills reconcile the truth.
                self.append(
                    intent,
                    IntentEvent::Cancelled {
                        reason: "venue no longer knows the order".into(),
                        at: self.clock.now(),
                    },
                )?;
                Ok(CancelOutcome::AlreadyGone)
            }
            Err(VenueError::Timeout { .. }) | Err(VenueError::Outage { .. }) => {
                Ok(CancelOutcome::Unknown)
            }
            Err(other) => Err(other.into()),
        }
    }

    /// Cancel working orders older than their strategy's TTL. Returns the
    /// intents swept (the strategy re-quotes through gates).
    pub async fn sweep_ttl(&mut self, venue: &dyn Venue) -> Result<Vec<IntentId>, ExecError> {
        let now = self.clock.now().epoch_millis();
        let expired: Vec<IntentId> = self
            .intents
            .iter()
            .filter(|(_, r)| {
                matches!(
                    r.status,
                    IntentStatus::Acked | IntentStatus::PartiallyFilled
                ) && !r.cancel_requested
            })
            .filter(|(_, r)| {
                let ttl = self
                    .policy
                    .per_strategy_ttl_ms
                    .get(r.order.strategy.as_str())
                    .copied()
                    .unwrap_or(self.policy.default_ttl_ms);
                now.saturating_sub(r.created_at.epoch_millis()) > ttl
            })
            .map(|(id, _)| *id)
            .collect();
        let mut swept = Vec::new();
        for intent in expired {
            match self.cancel_intent(intent, venue).await? {
                CancelOutcome::Cancelled | CancelOutcome::AlreadyGone => swept.push(intent),
                CancelOutcome::Unknown => {} // retried next sweep
            }
        }
        Ok(swept)
    }

    /// Boot reconciliation (spec 5.4): drain fills, match the journal
    /// against venue open orders, adopt/close/cancel, checkpoint the cursor.
    /// No strategy wakes until this returns.
    pub async fn boot_reconcile(&mut self, venue: &dyn Venue) -> Result<BootReport, ExecError> {
        let mut report = BootReport::default();

        // 1. Drain fills from the journal's cursor (dedup absorbs replays).
        let mut cursor = self.journal.cursor();
        let mut stable = 0;
        for _ in 0..10_000 {
            let page = match venue.fills_since(cursor.clone()).await {
                Ok(p) => p,
                Err(VenueError::Outage { .. }) => continue, // transient: retry
                Err(e) => return Err(e.into()),
            };
            for fill in &page.fills {
                match self.ingest_fill(fill) {
                    Ok(app) if app.applied => report.fills_applied += 1,
                    Ok(_) => {}
                    Err(ExecError::OrphanFill { fill_id, .. }) => {
                        if !report.orphan_fills.contains(&fill_id) {
                            report.orphan_fills.push(fill_id);
                        }
                    }
                    Err(ExecError::Overfill {
                        fill_id, intent, ..
                    }) => {
                        report
                            .discrepancies
                            .push(format!("overfill {fill_id} on {intent}"));
                    }
                    Err(e) => return Err(e),
                }
            }
            let advanced = page.next_cursor != cursor;
            cursor = page.next_cursor;
            if !advanced && page.fills.is_empty() {
                stable += 1;
                if stable >= 3 {
                    break;
                }
            } else {
                stable = 0;
            }
        }
        self.journal.set_cursor(cursor)?;

        // 2. Match venue open orders against the journal (retry transient
        // venue errors: boot must not die to a blip).
        let open = {
            let mut result = None;
            for _ in 0..100 {
                match venue.open_orders().await {
                    Ok(o) => {
                        result = Some(o);
                        break;
                    }
                    Err(VenueError::Outage { .. }) => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            result.ok_or(ExecError::Journal {
                reason: "open_orders: 100 transient errors in a row".into(),
            })?
        };
        for o in &open {
            match self.by_coid.get(o.client_order_id.as_str()).copied() {
                None => {
                    // Orphan: a venue order we never journaled. Cancel + alert.
                    let _ = venue.cancel(&o.venue_order_id).await;
                    report.orphans_cancelled.push(o.venue_order_id.clone());
                }
                Some(intent) => {
                    let status = self.intents.get(&intent).map(|r| r.status);
                    match status {
                        Some(IntentStatus::Submitted) => {
                            // Crash between submission and ack: adopt.
                            self.append(
                                intent,
                                IntentEvent::Acked {
                                    venue_order_id: o.venue_order_id.clone(),
                                    at: self.clock.now(),
                                },
                            )?;
                            report.adopted.push(intent);
                        }
                        Some(IntentStatus::Cancelled)
                        | Some(IntentStatus::Rejected)
                        | Some(IntentStatus::BootClosed) => {
                            // Journal says dead but the venue disagrees:
                            // re-cancel and record.
                            let _ = venue.cancel(&o.venue_order_id).await;
                            report.recancelled_at_venue.push(o.venue_order_id.clone());
                        }
                        _ => {} // Acked/Partial and present: consistent.
                    }
                }
            }
        }

        // 3. Disposition intents with no venue evidence.
        let open_coids: BTreeSet<&str> = open.iter().map(|o| o.client_order_id.as_str()).collect();
        let to_close: Vec<(IntentId, IntentStatus)> = self
            .intents
            .iter()
            .filter(|(_, r)| {
                matches!(r.status, IntentStatus::Created | IntentStatus::Submitted)
                    && !open_coids.contains(r.order.client_order_id.as_str())
            })
            .map(|(id, r)| (*id, r.status))
            .collect();
        for (intent, _) in &to_close {
            self.append(
                *intent,
                IntentEvent::BootClosed {
                    reason: "no venue evidence after crash; strategy re-proposes".into(),
                    at: self.clock.now(),
                },
            )?;
            report.closed_unsubmitted.push(*intent);
        }

        // Acked/Partial with no venue presence and not fully filled: the
        // order vanished (cancelled venue-side or expired). Close it.
        let missing: Vec<IntentId> = self
            .intents
            .iter()
            .filter(|(_, r)| {
                matches!(
                    r.status,
                    IntentStatus::Acked | IntentStatus::PartiallyFilled
                ) && !open_coids.contains(r.order.client_order_id.as_str())
            })
            .map(|(id, _)| *id)
            .collect();
        for intent in missing {
            self.append(
                intent,
                IntentEvent::Cancelled {
                    reason: "missing at venue at boot".into(),
                    at: self.clock.now(),
                },
            )?;
            report.missing_at_venue.push(intent);
        }

        Ok(report)
    }

    /// Test/DST scaffold: journal a Created row WITHOUT submitting, modeling
    /// a crash between intent persistence and submission (spec 5.4 scenario).
    #[doc(hidden)]
    pub fn journal_created_for_test(&mut self, order: &fortuna_gates::GatedOrder) {
        let snapshot = OrderSnapshot::from(order);
        let intent = snapshot.intent_id;
        let _ = self.append(
            intent,
            IntentEvent::Created {
                order: snapshot,
                group: None,
                at: self.clock.now(),
            },
        );
    }

    /// Journal + fold one event, keeping derived state exact.
    fn append(&mut self, intent: IntentId, event: IntentEvent) -> Result<(), ExecError> {
        // Validate by folding into a scratch copy first: an illegal
        // transition must not be journaled.
        let row = JournalRow {
            seq: self.journal.rows().len() as u64,
            intent,
            event,
        };
        Self::fold(
            &mut self.intents,
            &mut self.by_coid,
            &mut self.fills_seen,
            &row,
        )?;
        self.journal.append(intent, row.event)?;
        Ok(())
    }

    /// THE state machine: one fold used live and in recovery. Illegal
    /// transition = error, never a silent coerce.
    fn fold(
        intents: &mut BTreeMap<IntentId, IntentRecord>,
        by_coid: &mut BTreeMap<String, IntentId>,
        fills_seen: &mut BTreeSet<String>,
        row: &JournalRow,
    ) -> Result<(), ExecError> {
        let intent = row.intent;
        match &row.event {
            IntentEvent::Created { order, group, at } => {
                if intents.contains_key(&intent) {
                    return Err(ExecError::DuplicateIntent { intent });
                }
                by_coid.insert(order.client_order_id.as_str().to_string(), intent);
                intents.insert(
                    intent,
                    IntentRecord {
                        order: order.clone(),
                        group: *group,
                        status: IntentStatus::Created,
                        venue_order_id: None,
                        cum_filled: Contracts::ZERO,
                        cancel_requested: false,
                        created_at: *at,
                        last_event_at: *at,
                    },
                );
                Ok(())
            }
            other => {
                let record = intents
                    .get_mut(&intent)
                    .ok_or(ExecError::UnknownIntent { intent })?;
                let from = record.status;
                let illegal = || ExecError::Transition {
                    intent,
                    from: from.name(),
                    event: other.kind(),
                };
                match other {
                    IntentEvent::Created { .. } => unreachable!("handled above"),
                    IntentEvent::SubmitAttempted { at } => {
                        if !matches!(from, IntentStatus::Created | IntentStatus::Submitted) {
                            return Err(illegal());
                        }
                        record.status = IntentStatus::Submitted;
                        record.last_event_at = *at;
                    }
                    IntentEvent::Acked { venue_order_id, at } => {
                        match from {
                            IntentStatus::Submitted => {
                                record.status = IntentStatus::Acked;
                                record.venue_order_id = Some(venue_order_id.clone());
                            }
                            IntentStatus::Acked => {
                                // Idempotent re-ack must agree on the id.
                                if record.venue_order_id.as_ref() != Some(venue_order_id) {
                                    return Err(illegal());
                                }
                            }
                            _ => return Err(illegal()),
                        }
                        record.last_event_at = *at;
                    }
                    IntentEvent::Rejected { at, .. } => {
                        if !matches!(from, IntentStatus::Submitted | IntentStatus::Created) {
                            return Err(illegal());
                        }
                        record.status = IntentStatus::Rejected;
                        record.last_event_at = *at;
                    }
                    IntentEvent::FillApplied {
                        fill_id,
                        venue_order_id,
                        qty,
                        at,
                        ..
                    } => {
                        if fills_seen.contains(fill_id) {
                            return Ok(()); // replayed row; fold is idempotent
                        }
                        match from {
                            IntentStatus::Submitted => {
                                // Fill before our ack: the venue clearly has
                                // the order; adopt its id.
                                record.venue_order_id = Some(venue_order_id.clone());
                                record.status = IntentStatus::Acked;
                            }
                            IntentStatus::Acked
                            | IntentStatus::PartiallyFilled
                            // Late fills after a local cancel OR a boot
                            // close: venue truth arrives late; apply it,
                            // keep the terminal status, surface via flags.
                            | IntentStatus::Cancelled
                            | IntentStatus::BootClosed => {}
                            _ => return Err(illegal()),
                        }
                        let cum = record.cum_filled.raw() + qty.raw();
                        if cum > record.order.qty.raw() {
                            return Err(ExecError::Overfill {
                                intent,
                                fill_id: fill_id.clone(),
                                cum: record.cum_filled.raw(),
                                add: qty.raw(),
                                qty: record.order.qty.raw(),
                            });
                        }
                        record.cum_filled = Contracts::new(cum);
                        fills_seen.insert(fill_id.clone());
                        // Terminal-with-late-fill statuses stay terminal;
                        // otherwise advance partial/filled.
                        if !matches!(
                            record.status,
                            IntentStatus::Cancelled | IntentStatus::BootClosed
                        ) {
                            record.status = if cum == record.order.qty.raw() {
                                IntentStatus::Filled
                            } else {
                                IntentStatus::PartiallyFilled
                            };
                        }
                        record.last_event_at = *at;
                    }
                    IntentEvent::CancelRequested { at } => {
                        if !from.is_working() {
                            return Err(illegal());
                        }
                        record.cancel_requested = true;
                        record.last_event_at = *at;
                    }
                    IntentEvent::Cancelled { at, .. } => {
                        if !from.is_working() {
                            return Err(illegal());
                        }
                        record.status = IntentStatus::Cancelled;
                        record.last_event_at = *at;
                    }
                    IntentEvent::BootClosed { at, .. } => {
                        if !matches!(from, IntentStatus::Created | IntentStatus::Submitted) {
                            return Err(illegal());
                        }
                        record.status = IntentStatus::BootClosed;
                        record.last_event_at = *at;
                    }
                }
                Ok(())
            }
        }
    }
}

impl<J: IntentJournal> IntentView for OrderManager<J> {
    fn intent_record(&self, id: IntentId) -> Option<&IntentRecord> {
        self.intents.get(&id)
    }
}
