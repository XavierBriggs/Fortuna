//! Intent journal: the append-only record of every order intent's life.
//! Spec 5.4: persisted BEFORE any network call; the state machine
//! created -> submitted -> acked -> partially_filled -> filled | cancelled |
//! rejected is the fold of these events, used identically live and during
//! crash recovery (one source of truth for transitions).
//!
//! `MemoryJournal` is the deterministic in-memory impl (DST, tests); the
//! Postgres-backed impl arrives with fortuna-ledger (T0.8) behind the same
//! trait. Recovery NEVER reconstructs a `GatedOrder` from the journal —
//! snapshots are bookkeeping data; new orders only come from the gate
//! pipeline (I1).

use crate::ExecError;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::{IntentGroupId, IntentId};
use fortuna_core::market::{
    Action, ClientOrderId, Contracts, MarketId, Side, StrategyId, VenueId, VenueOrderId,
};
use fortuna_core::money::Cents;
use fortuna_gates::GatedOrder;
use serde::{Deserialize, Serialize};

/// Plain-data copy of a gated order for journaling and recovery matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderSnapshot {
    pub intent_id: IntentId,
    pub strategy: StrategyId,
    pub venue: VenueId,
    pub market: MarketId,
    pub side: Side,
    pub action: Action,
    pub limit_price: Cents,
    pub qty: Contracts,
    pub client_order_id: ClientOrderId,
}

impl From<&GatedOrder> for OrderSnapshot {
    fn from(o: &GatedOrder) -> Self {
        OrderSnapshot {
            intent_id: o.intent_id(),
            strategy: o.strategy().clone(),
            venue: o.venue().clone(),
            market: o.market().clone(),
            side: o.side(),
            action: o.action(),
            limit_price: o.limit_price(),
            qty: o.qty(),
            client_order_id: o.client_order_id().clone(),
        }
    }
}

/// One journaled event in an intent's life. Append-only; never edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IntentEvent {
    Created {
        order: OrderSnapshot,
        group: Option<IntentGroupId>,
        at: UtcTimestamp,
    },
    SubmitAttempted {
        at: UtcTimestamp,
    },
    Acked {
        venue_order_id: VenueOrderId,
        at: UtcTimestamp,
    },
    Rejected {
        reason: String,
        at: UtcTimestamp,
    },
    FillApplied {
        fill_id: String,
        venue_order_id: VenueOrderId,
        price: Cents,
        qty: Contracts,
        fee: Cents,
        is_maker: bool,
        late_after_cancel: bool,
        at: UtcTimestamp,
    },
    CancelRequested {
        at: UtcTimestamp,
    },
    Cancelled {
        reason: String,
        at: UtcTimestamp,
    },
    /// Closed by boot reconciliation (crash before/at submission with no
    /// venue evidence). Strategies re-propose through gates; never resubmit.
    BootClosed {
        reason: String,
        at: UtcTimestamp,
    },
}

impl IntentEvent {
    /// The event's own timestamp (every variant carries one).
    pub fn at(&self) -> UtcTimestamp {
        match self {
            IntentEvent::Created { at, .. }
            | IntentEvent::SubmitAttempted { at }
            | IntentEvent::Acked { at, .. }
            | IntentEvent::Rejected { at, .. }
            | IntentEvent::FillApplied { at, .. }
            | IntentEvent::CancelRequested { at }
            | IntentEvent::Cancelled { at, .. }
            | IntentEvent::BootClosed { at, .. } => *at,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            IntentEvent::Created { .. } => "created",
            IntentEvent::SubmitAttempted { .. } => "submit_attempted",
            IntentEvent::Acked { .. } => "acked",
            IntentEvent::Rejected { .. } => "rejected",
            IntentEvent::FillApplied { .. } => "fill_applied",
            IntentEvent::CancelRequested { .. } => "cancel_requested",
            IntentEvent::Cancelled { .. } => "cancelled",
            IntentEvent::BootClosed { .. } => "boot_closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalRow {
    pub seq: u64,
    pub intent: IntentId,
    pub event: IntentEvent,
}

/// Append-only persistence for intent events plus the venue fill cursor.
/// Async so the Postgres impl (fortuna-ledger) is first-class; the in-memory
/// impl completes immediately. Durability ordering is the caller's contract:
/// `append` returns only after the row is durable (spec 5.4: persisted
/// BEFORE any network call).
#[async_trait::async_trait]
pub trait IntentJournal: Send {
    async fn append(&mut self, intent: IntentId, event: IntentEvent) -> Result<(), ExecError>;
    /// Load the full journal (recovery fold input).
    async fn load_all(&self) -> Result<Vec<JournalRow>, ExecError>;
    async fn cursor(&self) -> Result<fortuna_venues::Cursor, ExecError>;
    async fn set_cursor(&mut self, cursor: fortuna_venues::Cursor) -> Result<(), ExecError>;
}

/// Deterministic in-memory journal. "Durability" in DST = the value
/// surviving while the OrderManager is dropped and rebuilt around it.
#[derive(Debug, Clone, Default)]
pub struct MemoryJournal {
    rows: Vec<JournalRow>,
    cursor: Option<fortuna_venues::Cursor>,
}

impl MemoryJournal {
    /// Event kinds for one intent, in order (test/audit convenience).
    pub fn event_kinds_for(&self, intent: IntentId) -> Vec<&'static str> {
        self.rows
            .iter()
            .filter(|r| r.intent == intent)
            .map(|r| r.event.kind())
            .collect()
    }
}

#[async_trait::async_trait]
impl IntentJournal for MemoryJournal {
    async fn append(&mut self, intent: IntentId, event: IntentEvent) -> Result<(), ExecError> {
        let seq = self.rows.len() as u64;
        self.rows.push(JournalRow { seq, intent, event });
        Ok(())
    }

    async fn load_all(&self) -> Result<Vec<JournalRow>, ExecError> {
        Ok(self.rows.clone())
    }

    async fn cursor(&self) -> Result<fortuna_venues::Cursor, ExecError> {
        Ok(self
            .cursor
            .clone()
            .unwrap_or_else(fortuna_venues::Cursor::start))
    }

    async fn set_cursor(&mut self, cursor: fortuna_venues::Cursor) -> Result<(), ExecError> {
        self.cursor = Some(cursor);
        Ok(())
    }
}
