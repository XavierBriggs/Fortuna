//! Phase-0 repos: fills mirror, halt persistence, reservation events.
//! All INSERT-only (triggers enforce); "current state" is a fold.
//! Phase-2 tables (beliefs, events, signals, ...) get their repos in their
//! owning tasks — the schema already exists.

use crate::LedgerError;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::IntentId;
use fortuna_core::money::Cents;
use fortuna_gates::HaltScope;
use fortuna_venues::Fill;
use sqlx::PgPool;
use std::collections::BTreeMap;

/// String form of a halt scope for persistence ('global' | 'strategy:<id>'
/// | 'venue:<id>'). The ops runner and the ledger agree on this encoding.
pub fn halt_scope_string(scope: &HaltScope) -> String {
    match scope {
        HaltScope::Global => "global".to_string(),
        HaltScope::Strategy(s) => format!("strategy:{s}"),
        HaltScope::Venue(v) => format!("venue:{v}"),
    }
}

/// Parse the persisted scope string back (inverse of `halt_scope_string`).
pub fn parse_halt_scope(raw: &str) -> Option<HaltScope> {
    if raw == "global" {
        return Some(HaltScope::Global);
    }
    if let Some(s) = raw.strip_prefix("strategy:") {
        return Some(HaltScope::Strategy(s.to_string()));
    }
    raw.strip_prefix("venue:")
        .map(|v| HaltScope::Venue(v.to_string()))
}

/// Execution-mirror fills (Section 7), deduped on the venue fill id.
pub struct FillsRepo {
    pool: PgPool,
}

impl FillsRepo {
    pub fn new(pool: PgPool) -> FillsRepo {
        FillsRepo { pool }
    }

    /// Insert one fill; `Ok(false)` when the fill id was already recorded
    /// (at-least-once delivery upstream).
    pub async fn insert(&self, venue: &str, fill: &Fill) -> Result<bool, LedgerError> {
        let side = match fill.side {
            fortuna_core::market::Side::Yes => "yes",
            fortuna_core::market::Side::No => "no",
        };
        let action = match fill.action {
            fortuna_core::market::Action::Buy => "buy",
            fortuna_core::market::Action::Sell => "sell",
        };
        let result = sqlx::query!(
            r#"INSERT INTO fills
               (fill_id, venue, venue_order_id, client_order_id, market_id,
                side, action, price_cents, qty, fee_cents, is_maker, at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
               ON CONFLICT (fill_id) DO NOTHING"#,
            fill.fill_id,
            venue,
            fill.venue_order_id.to_string(),
            fill.client_order_id.as_str(),
            fill.market.as_str(),
            side,
            action,
            fill.price.raw(),
            fill.qty.raw(),
            fill.fee.raw(),
            fill.is_maker,
            fill.at.to_iso8601()
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn count(&self) -> Result<i64, LedgerError> {
        let row = sqlx::query!(r#"SELECT COUNT(*) as "n!" FROM fills"#)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.n)
    }
}

/// Halt persistence (I2 must survive restarts): set/rearm events, folded to
/// the active set at boot. The runner restores `GatePipeline` flags from
/// `active()` before any strategy wakes.
pub struct HaltsRepo {
    pool: PgPool,
}

impl HaltsRepo {
    pub fn new(pool: PgPool) -> HaltsRepo {
        HaltsRepo { pool }
    }

    pub async fn record_set(
        &self,
        scope: &HaltScope,
        reason: &str,
        actor: &str,
        at: UtcTimestamp,
    ) -> Result<(), LedgerError> {
        self.record(scope, "set", reason, actor, at).await
    }

    /// Re-arm is an OPERATOR action (I2): `actor` is the operator identity
    /// recorded for the audit trail; "system" must never call this.
    pub async fn record_rearm(
        &self,
        scope: &HaltScope,
        reason: &str,
        actor: &str,
        at: UtcTimestamp,
    ) -> Result<(), LedgerError> {
        self.record(scope, "rearm", reason, actor, at).await
    }

    async fn record(
        &self,
        scope: &HaltScope,
        kind: &str,
        reason: &str,
        actor: &str,
        at: UtcTimestamp,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO halt_events (scope, kind, reason, actor, at)
               VALUES ($1,$2,$3,$4,$5)"#,
            halt_scope_string(scope),
            kind,
            reason,
            actor,
            at.to_iso8601()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fold to the currently-active halts (latest event per scope wins).
    pub async fn active(&self) -> Result<Vec<(HaltScope, String)>, LedgerError> {
        let rows = sqlx::query!(r#"SELECT scope, kind, reason FROM halt_events ORDER BY seq"#)
            .fetch_all(&self.pool)
            .await?;
        let mut state: BTreeMap<String, Option<String>> = BTreeMap::new();
        for r in rows {
            match r.kind.as_str() {
                "set" => {
                    state.insert(r.scope, Some(r.reason));
                }
                _ => {
                    state.insert(r.scope, None);
                }
            }
        }
        let mut out = Vec::new();
        for (scope_raw, reason) in state {
            if let Some(reason) = reason {
                let scope = parse_halt_scope(&scope_raw).ok_or(LedgerError::CorruptRow {
                    table: "halt_events",
                    reason: format!("unparseable scope {scope_raw:?}"),
                })?;
                out.push((scope, reason));
            }
        }
        Ok(out)
    }
}

/// Reservation events (spec 5.14: reservations are derived state). The
/// in-memory `ReservationLedger` is authoritative at runtime; these rows are
/// the boot-rebuild input.
pub struct ReservationsRepo {
    pool: PgPool,
}

impl ReservationsRepo {
    pub fn new(pool: PgPool) -> ReservationsRepo {
        ReservationsRepo { pool }
    }

    pub async fn record_reserve(
        &self,
        intent: IntentId,
        strategy: &str,
        amount: Cents,
        at: UtcTimestamp,
    ) -> Result<(), LedgerError> {
        self.record(intent, strategy, "reserve", amount, at).await
    }

    pub async fn record_release(
        &self,
        intent: IntentId,
        strategy: &str,
        amount: Cents,
        at: UtcTimestamp,
    ) -> Result<(), LedgerError> {
        self.record(intent, strategy, "release", amount, at).await
    }

    async fn record(
        &self,
        intent: IntentId,
        strategy: &str,
        kind: &str,
        amount: Cents,
        at: UtcTimestamp,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO reservation_events (intent_id, strategy, kind, amount_cents, at)
               VALUES ($1,$2,$3,$4,$5)"#,
            intent.to_string(),
            strategy,
            kind,
            amount.raw(),
            at.to_iso8601()
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fold to active reservations: (intent, strategy, amount) with a
    /// reserve and no later release.
    pub async fn active(&self) -> Result<Vec<(IntentId, String, Cents)>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT intent_id, strategy, kind, amount_cents
               FROM reservation_events ORDER BY seq"#
        )
        .fetch_all(&self.pool)
        .await?;
        let mut state: BTreeMap<String, Option<(String, i64)>> = BTreeMap::new();
        for r in rows {
            match r.kind.as_str() {
                "reserve" => {
                    state.insert(r.intent_id, Some((r.strategy, r.amount_cents)));
                }
                _ => {
                    state.insert(r.intent_id, None);
                }
            }
        }
        let mut out = Vec::new();
        for (intent_raw, entry) in state {
            if let Some((strategy, amount)) = entry {
                let intent: IntentId = intent_raw.parse().map_err(|_| LedgerError::CorruptRow {
                    table: "reservation_events",
                    reason: format!("unparseable intent id {intent_raw:?}"),
                })?;
                out.push((intent, strategy, Cents::new(amount)));
            }
        }
        Ok(out)
    }
}

/// One persisted settlement-entry row (spec 5.13; mirrors the in-memory
/// `fortuna_state::SettlementEntry` chain shape).
#[derive(Debug, Clone)]
pub struct SettlementEntryRow {
    pub settlement_id: String,
    pub market_id: String,
    pub venue: String,
    pub amount_cents: i64,
    pub status: String,
    pub supersedes: Option<String>,
    pub detail: serde_json::Value,
    pub at: String,
}

/// Settlement entries: INSERT-only superseding rows (the table's triggers
/// refuse UPDATE/DELETE; status transitions are NEW rows).
pub struct SettlementsRepo {
    pool: PgPool,
}

impl SettlementsRepo {
    pub fn new(pool: PgPool) -> SettlementsRepo {
        SettlementsRepo { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_entry(
        &self,
        settlement_id: &str,
        market_id: &str,
        venue: &str,
        amount_cents: i64,
        status: &str,
        supersedes: Option<&str>,
        detail: &serde_json::Value,
        at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO settlement_entries
               (settlement_id, market_id, venue, amount_cents, status,
                supersedes, detail, at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
            settlement_id,
            market_id,
            venue,
            amount_cents,
            status,
            supersedes,
            detail,
            at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Full chain for a market, oldest first.
    pub async fn chain(&self, market_id: &str) -> Result<Vec<SettlementEntryRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT settlement_id, market_id, venue, amount_cents, status,
                      supersedes, detail, at
               FROM settlement_entries WHERE market_id = $1 ORDER BY at, settlement_id"#,
            market_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SettlementEntryRow {
                settlement_id: r.settlement_id,
                market_id: r.market_id,
                venue: r.venue,
                amount_cents: r.amount_cents,
                status: r.status,
                supersedes: r.supersedes,
                detail: r.detail,
                at: r.at,
            })
            .collect())
    }
}

/// Discrepancies (spec 5.13: no silent corrections): open records are
/// resolved ONLY by separate resolution rows (matching entry, adjustment
/// with reason, or operator escalation).
pub struct DiscrepanciesRepo {
    pool: PgPool,
}

impl DiscrepanciesRepo {
    pub fn new(pool: PgPool) -> DiscrepanciesRepo {
        DiscrepanciesRepo { pool }
    }

    pub async fn open(
        &self,
        discrepancy_id: &str,
        kind: &str,
        detail: &serde_json::Value,
        opened_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO discrepancies (discrepancy_id, kind, detail, opened_at)
               VALUES ($1,$2,$3,$4)"#,
            discrepancy_id,
            kind,
            detail,
            opened_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn resolve(
        &self,
        resolution_id: &str,
        discrepancy_id: &str,
        disposition: &str,
        reason: &str,
        ref_id: Option<&str>,
        at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO discrepancy_resolutions
               (resolution_id, discrepancy_id, disposition, reason, ref_id, at)
               VALUES ($1,$2,$3,$4,$5,$6)"#,
            resolution_id,
            discrepancy_id,
            disposition,
            reason,
            ref_id,
            at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Discrepancies with no resolution row (the aging metric input).
    pub async fn open_count(&self) -> Result<i64, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT COUNT(*) as "n!" FROM discrepancies d
               WHERE NOT EXISTS (
                   SELECT 1 FROM discrepancy_resolutions r
                   WHERE r.discrepancy_id = d.discrepancy_id
               )"#
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.n)
    }
}

/// One persisted canonical event row (spec 5.12).
#[derive(Debug, Clone)]
pub struct EventRow {
    pub event_id: String,
    pub statement: String,
    pub resolution_source: String,
    pub benchmark_at: String,
    pub category: String,
    pub status: String,
    pub dead_reason: Option<String>,
    pub unscoreable: bool,
}

/// Canonical events: lifecycle status is mutable state on the row (the
/// 5.13 legality rules live in fortuna-cognition; the repo persists).
pub struct EventsRepo {
    pool: PgPool,
}

impl EventsRepo {
    pub fn new(pool: PgPool) -> EventsRepo {
        EventsRepo { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        event_id: &str,
        statement: &str,
        resolution_criteria: &str,
        resolution_source: &str,
        horizon: Option<&str>,
        benchmark_at: &str,
        category: &str,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO events
               (event_id, statement, resolution_criteria, resolution_source,
                horizon, benchmark_at, category, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)"#,
            event_id,
            statement,
            resolution_criteria,
            resolution_source,
            horizon,
            benchmark_at,
            category,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get(&self, event_id: &str) -> Result<EventRow, LedgerError> {
        let r = sqlx::query!(
            r#"SELECT event_id, statement, resolution_source, benchmark_at,
                      category, status, dead_reason, unscoreable
               FROM events WHERE event_id = $1"#,
            event_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(EventRow {
            event_id: r.event_id,
            statement: r.statement,
            resolution_source: r.resolution_source,
            benchmark_at: r.benchmark_at,
            category: r.category,
            status: r.status,
            dead_reason: r.dead_reason,
            unscoreable: r.unscoreable,
        })
    }

    pub async fn set_status(&self, event_id: &str, status: &str) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"UPDATE events SET status = $2 WHERE event_id = $1"#,
            event_id,
            status
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_dead(&self, event_id: &str, reason: &str) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"UPDATE events SET status = 'dead', dead_reason = $2 WHERE event_id = $1"#,
            event_id,
            reason
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_unscoreable(&self, event_id: &str) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"UPDATE events SET unscoreable = TRUE WHERE event_id = $1"#,
            event_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// One persisted market-event edge row (spec 5.12; superseding inserts).
#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub edge_id: String,
    pub market_id: String,
    pub venue: String,
    pub event_id: String,
    pub mapping_type: String,
    pub confidence: f64,
    pub proposed_by: String,
    pub confirmed_by: Option<String>,
    pub supersedes: Option<String>,
}

pub struct EdgesRepo {
    pool: PgPool,
}

impl EdgesRepo {
    pub fn new(pool: PgPool) -> EdgesRepo {
        EdgesRepo { pool }
    }

    /// INSERT one edge row. Confirmation and confidence corrections are
    /// NEW rows with `supersedes` set (append-only discipline).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_edge(
        &self,
        edge_id: &str,
        market_id: &str,
        venue: &str,
        event_id: &str,
        mapping_type: &str,
        confidence: f64,
        proposed_by: &str,
        confirmed_by: Option<&str>,
        supersedes: Option<&str>,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO market_event_edges
               (edge_id, market_id, venue, event_id, mapping_type, confidence,
                proposed_by, confirmed_by, supersedes, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"#,
            edge_id,
            market_id,
            venue,
            event_id,
            mapping_type,
            confidence,
            proposed_by,
            confirmed_by,
            supersedes,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Current (non-superseded) edges for an event.
    pub async fn current_edges_for_event(
        &self,
        event_id: &str,
    ) -> Result<Vec<EdgeRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT edge_id, market_id, venue, event_id, mapping_type,
                      confidence, proposed_by, confirmed_by, supersedes
               FROM market_event_edges e
               WHERE event_id = $1
                 AND NOT EXISTS (
                     SELECT 1 FROM market_event_edges n
                     WHERE n.supersedes = e.edge_id
                 )
               ORDER BY created_at, edge_id"#,
            event_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| EdgeRow {
                edge_id: r.edge_id,
                market_id: r.market_id,
                venue: r.venue,
                event_id: r.event_id,
                mapping_type: r.mapping_type,
                confidence: r.confidence,
                proposed_by: r.proposed_by,
                confirmed_by: r.confirmed_by,
                supersedes: r.supersedes,
            })
            .collect())
    }
}

/// One persisted CLV price snapshot row (spec 5.5; append-only table).
#[derive(Debug, Clone)]
pub struct SnapshotRow {
    pub snapshot_id: String,
    pub best_bid_cents: Option<i64>,
    pub best_ask_cents: Option<i64>,
    pub at: String,
}

pub struct SnapshotsRepo {
    pool: PgPool,
}

impl SnapshotsRepo {
    pub fn new(pool: PgPool) -> SnapshotsRepo {
        SnapshotsRepo { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        &self,
        snapshot_id: &str,
        market_id: &str,
        venue: &str,
        event_id: Option<&str>,
        kind: &str,
        best_bid_cents: Option<i64>,
        best_ask_cents: Option<i64>,
        bid_qty: Option<i64>,
        ask_qty: Option<i64>,
        liquidity_ok: bool,
        at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO price_snapshots
               (snapshot_id, market_id, venue, event_id, kind, best_bid_cents,
                best_ask_cents, bid_qty, ask_qty, liquidity_ok, at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"#,
            snapshot_id,
            market_id,
            venue,
            event_id,
            kind,
            best_bid_cents,
            best_ask_cents,
            bid_qty,
            ask_qty,
            liquidity_ok,
            at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The CLV benchmark input: the LATEST snapshot strictly before the
    /// cutoff with liquidity_ok (spec 5.5: no liquid snapshot, no CLV).
    /// ISO8601 strings with fixed millisecond precision sort lexically.
    pub async fn latest_liquid_before(
        &self,
        market_id: &str,
        event_id: &str,
        cutoff_iso: &str,
    ) -> Result<Option<SnapshotRow>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT snapshot_id, best_bid_cents, best_ask_cents, at
               FROM price_snapshots
               WHERE market_id = $1 AND event_id = $2
                 AND liquidity_ok AND at < $3
               ORDER BY at DESC LIMIT 1"#,
            market_id,
            event_id,
            cutoff_iso
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| SnapshotRow {
            snapshot_id: r.snapshot_id,
            best_bid_cents: r.best_bid_cents,
            best_ask_cents: r.best_ask_cents,
            at: r.at,
        }))
    }
}
