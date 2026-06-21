//! Phase-0 repos: fills mirror, halt persistence, reservation events.
//! All INSERT-only (triggers enforce); "current state" is a fold.
//! Phase-2 tables (beliefs, events, signals, ...) get their repos in their
//! owning tasks — the schema already exists.

use crate::LedgerError;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::ids::IntentId;
use fortuna_core::money::Cents;
use fortuna_gates::HaltScope;
use fortuna_scoring::Scorecard;
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
    /// (at-least-once delivery upstream). `producer` and `strategy` are
    /// nullable — legacy fills keep NULL; Phase-C forward populates them.
    pub async fn insert(
        &self,
        venue: &str,
        fill: &Fill,
        producer: Option<&str>,
        strategy: Option<&str>,
    ) -> Result<bool, LedgerError> {
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
                side, action, price_cents, qty, fee_cents, is_maker, at,
                producer, strategy)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
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
            fill.at.to_iso8601(),
            producer,
            strategy
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

    /// The earliest fill for a market (the entry for CLV computation).
    /// Returns None when no fills exist (belief proposed but not traded —
    /// CLV stays None, correct).
    pub async fn first_fill_for_market(
        &self,
        market_id: &str,
    ) -> Result<Option<FirstFillRow>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT side, price_cents, at
               FROM fills
               WHERE market_id = $1
               ORDER BY at ASC LIMIT 1"#,
            market_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| FirstFillRow {
            side: r.side,
            price_cents: r.price_cents,
            at: r.at,
        }))
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
    /// The intent that generated this settlement (nullable; NULL for legacy/
    /// non-intent settlements). Used by the partial unique index for dedup.
    pub intent_id: Option<String>,
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

    /// Insert one settlement entry. Returns `Ok(true)` when inserted,
    /// `Ok(false)` when a conflicting initial entry for the same
    /// `(market_id, intent_id)` already exists (idempotent under retry).
    ///
    /// The partial-unique-index `settlement_entries_intent_uniq` covers
    /// only rows where `supersedes IS NULL AND intent_id IS NOT NULL`;
    /// correction rows (`supersedes = Some(...)`) always insert.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_entry(
        &self,
        settlement_id: &str,
        market_id: &str,
        venue: &str,
        amount_cents: i64,
        status: &str,
        supersedes: Option<&str>,
        intent_id: Option<&str>,
        detail: &serde_json::Value,
        at: &str,
    ) -> Result<bool, LedgerError> {
        let result = sqlx::query!(
            r#"INSERT INTO settlement_entries
               (settlement_id, market_id, venue, amount_cents, status,
                supersedes, intent_id, detail, at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               ON CONFLICT (market_id, intent_id)
               WHERE supersedes IS NULL AND intent_id IS NOT NULL
               DO NOTHING"#,
            settlement_id,
            market_id,
            venue,
            amount_cents,
            status,
            supersedes,
            intent_id,
            detail,
            at
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Full chain for a market, oldest first.
    pub async fn chain(&self, market_id: &str) -> Result<Vec<SettlementEntryRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT settlement_id, market_id, venue, amount_cents, status,
                      supersedes, intent_id, detail, at
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
                intent_id: r.intent_id,
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

/// One immutable link between a generated event and a signal row that was in
/// the model context when the event was proposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSourceEvidenceInput {
    pub signal_id: String,
    pub signal_received_at: String,
    pub source: String,
    pub signal_type: String,
    pub content_hash: String,
}

pub struct EventSourceEvidenceRepo {
    pool: PgPool,
}

impl EventSourceEvidenceRepo {
    pub fn new(pool: PgPool) -> EventSourceEvidenceRepo {
        EventSourceEvidenceRepo { pool }
    }

    pub async fn insert_many(
        &self,
        event_id: &str,
        relation: &str,
        created_at: &str,
        evidence: &[EventSourceEvidenceInput],
    ) -> Result<u64, LedgerError> {
        let mut inserted = 0;
        for e in evidence {
            let result = sqlx::query(
                r#"INSERT INTO event_source_evidence
                   (event_id, signal_id, signal_received_at, source, signal_type,
                    content_hash, relation, created_at)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                   ON CONFLICT (event_id, signal_id, signal_received_at) DO NOTHING"#,
            )
            .bind(event_id)
            .bind(&e.signal_id)
            .bind(&e.signal_received_at)
            .bind(&e.source)
            .bind(&e.signal_type)
            .bind(&e.content_hash)
            .bind(relation)
            .bind(created_at)
            .execute(&self.pool)
            .await?;
            inserted += result.rows_affected();
        }
        Ok(inserted)
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

    /// Current (non-superseded) edges for a MARKET — the market-back
    /// discovery dedup query (already-edged listings skip normalization).
    pub async fn current_edges_for_market(
        &self,
        market_id: &str,
    ) -> Result<Vec<EdgeRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT edge_id, market_id, venue, event_id, mapping_type,
                      confidence, proposed_by, confirmed_by, supersedes
               FROM market_event_edges e
               WHERE market_id = $1
                 AND NOT EXISTS (
                     SELECT 1 FROM market_event_edges n
                     WHERE n.supersedes = e.edge_id
                 )
               ORDER BY created_at, edge_id"#,
            market_id
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

    /// All CONFIRMED (confirmed_by IS NOT NULL) and CURRENT (non-superseded)
    /// edges — the daemon synthesis composition's tradeable edge set
    /// (docs/design/synthesis-edge-source-decision.md requirement 1). The
    /// `[synthesis]` config filters (category / venue / max_edges) apply at the
    /// composition, never here; this is the raw confirmed-tier load.
    pub async fn confirmed_edges(&self) -> Result<Vec<EdgeRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT edge_id, market_id, venue, event_id, mapping_type,
                      confidence, proposed_by, confirmed_by, supersedes
               FROM market_event_edges e
               WHERE e.confirmed_by IS NOT NULL
                 AND NOT EXISTS (
                     SELECT 1 FROM market_event_edges n
                     WHERE n.supersedes = e.edge_id
                 )
               ORDER BY created_at, edge_id"#,
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
    /// Book depth at the touch (NULL on legacy rows predating Phase C).
    pub bid_qty: Option<i64>,
    pub ask_qty: Option<i64>,
    pub at: String,
}

/// Minimal fill summary returned by `FillsRepo::first_fill_for_market`.
#[derive(Debug, Clone)]
pub struct FirstFillRow {
    pub side: String,
    pub price_cents: i64,
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
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
               ON CONFLICT (market_id, at) DO NOTHING"#,
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
            r#"SELECT snapshot_id, best_bid_cents, best_ask_cents, bid_qty, ask_qty, at
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
            bid_qty: r.bid_qty,
            ask_qty: r.ask_qty,
            at: r.at,
        }))
    }

    /// All liquid snapshots for a market strictly before the cutoff, ordered
    /// by time ASC — used to find the latest pre-benchmark snapshot for CLV.
    /// Defensive: callers skip/None on any parse error (never panic on `at`).
    pub async fn snapshots_for_market_before(
        &self,
        market_id: &str,
        event_id: &str,
        cutoff_iso: &str,
    ) -> Result<Vec<SnapshotRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT snapshot_id, best_bid_cents, best_ask_cents, bid_qty, ask_qty, at
               FROM price_snapshots
               WHERE market_id = $1 AND event_id = $2
                 AND liquidity_ok AND at < $3
               ORDER BY at ASC"#,
            market_id,
            event_id,
            cutoff_iso
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SnapshotRow {
                snapshot_id: r.snapshot_id,
                best_bid_cents: r.best_bid_cents,
                best_ask_cents: r.best_ask_cents,
                bid_qty: r.bid_qty,
                ask_qty: r.ask_qty,
                at: r.at,
            })
            .collect())
    }
}

/// Append-only signal envelopes (spec 5.11). Point-in-time: rows are
/// INSERT-only (table triggers refuse mutation); received_at is the
/// adapter's receipt time.
pub struct SignalsRepo {
    pool: PgPool,
}

/// One signal read back for downstream context assembly (e.g. a persona run).
/// `kind` is the table's `type` column. `received_at` is the ISO8601 receipt
/// time; ordering is lexicographic, which is chronological for zero-padded UTC.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentSignalRow {
    pub signal_id: String,
    pub source: String,
    pub kind: String,
    pub received_at: String,
    pub content_hash: String,
    pub payload: serde_json::Value,
}

impl SignalsRepo {
    pub fn new(pool: PgPool) -> SignalsRepo {
        SignalsRepo { pool }
    }

    pub async fn insert(
        &self,
        signal_id: &str,
        source: &str,
        kind: &str,
        received_at: &str,
        content_hash: &str,
        payload: &serde_json::Value,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO signals
               (signal_id, source, type, received_at, content_hash, payload)
               VALUES ($1,$2,$3,$4,$5,$6)"#,
            signal_id,
            source,
            kind,
            received_at,
            content_hash,
            payload
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn count(&self) -> Result<i64, LedgerError> {
        let row = sqlx::query!(r#"SELECT COUNT(*) as "n!" FROM signals"#)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.n)
    }

    /// Boot-time rebuild input for the in-memory DedupIndex.
    pub async fn dedup_pairs(&self) -> Result<Vec<(String, String)>, LedgerError> {
        let rows = sqlx::query!(r#"SELECT DISTINCT source, content_hash FROM signals"#)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.source, r.content_hash))
            .collect())
    }

    /// Read recent signals of one of `kinds` whose `received_at >= received_after`
    /// (inclusive), newest first, capped at `limit`. The read-back path that lets
    /// the live daemon assemble a persona's untrusted `<context-item>` blocks (the
    /// SIGNAL stream is data, never instructions — spec 5.11 / design §4). Empty
    /// `kinds` matches nothing. Append-only table, so this is a pure read.
    pub async fn recent_by_kind(
        &self,
        kinds: &[String],
        received_after: &str,
        limit: i64,
    ) -> Result<Vec<RecentSignalRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT signal_id, source, type AS "kind!", received_at, content_hash, payload
               FROM signals
               WHERE type = ANY($1) AND received_at >= $2
               ORDER BY received_at DESC, signal_id DESC
               LIMIT $3"#,
            kinds,
            received_after,
            limit,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| RecentSignalRow {
                signal_id: r.signal_id,
                source: r.source,
                kind: r.kind,
                received_at: r.received_at,
                content_hash: r.content_hash,
                payload: r.payload,
            })
            .collect())
    }
}

/// One source_registry row (the funnel's allowlist).
#[derive(Debug, Clone)]
pub struct SourceRegistryRow {
    pub source_id: String,
    pub trust_tier: i32,
    pub domain_tags: Vec<String>,
    pub enabled: bool,
}

/// The curated source allowlist (spec 5.11): per-source trust tier +
/// domain tags; demotions update the row ON THE RECORD (updated_at), the
/// demotion evidence lives in belief attribution and audit.
pub struct SourceRegistryRepo {
    pool: PgPool,
}

impl SourceRegistryRepo {
    pub fn new(pool: PgPool) -> SourceRegistryRepo {
        SourceRegistryRepo { pool }
    }

    pub async fn upsert(
        &self,
        source_id: &str,
        trust_tier: i32,
        domain_tags: &[String],
        enabled: bool,
        at: &str,
    ) -> Result<(), LedgerError> {
        let tags = serde_json::to_value(domain_tags).unwrap_or_default();
        sqlx::query!(
            r#"INSERT INTO source_registry
               (source_id, trust_tier, domain_tags, enabled, created_at, updated_at)
               VALUES ($1,$2,$3,$4,$5,$5)
               ON CONFLICT (source_id) DO UPDATE
               SET trust_tier = $2, domain_tags = $3, enabled = $4, updated_at = $5"#,
            source_id,
            trust_tier,
            tags,
            enabled,
            at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_all(&self) -> Result<Vec<SourceRegistryRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT source_id, trust_tier, domain_tags, enabled FROM source_registry"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SourceRegistryRow {
                source_id: r.source_id,
                trust_tier: r.trust_tier,
                domain_tags: serde_json::from_value(r.domain_tags).unwrap_or_default(),
                enabled: r.enabled,
            })
            .collect())
    }
}

/// One resolved belief's review stats (T3.1 weekly calibration audit).
#[derive(Debug, Clone)]
pub struct ResolvedStat {
    pub p: f64,
    pub outcome: bool,
    pub brier: f64,
    pub clv_bps: Option<f64>,
}

/// Resolved beliefs attributed to one persona scope (Track E §10/§11). Ledger-native
/// (the repo layer holds no `fortuna-cognition` types); the daemon wraps it into a
/// `persona_scoring::PersonaScopeRecord { scope, samples, clv_bps }` for
/// `score_persona`. `samples` is the calibrated `(p, outcome)` over scoreable resolved
/// events; `clv_bps` drops the unmeasurable (`None`) ones.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPersonaStats {
    pub persona_id: String,
    pub persona_version: i32,
    pub samples: Vec<(f64, bool)>,
    pub clv_bps: Vec<f64>,
}

/// One persisted belief row (spec 5.5).
#[derive(Debug, Clone)]
pub struct BeliefRow {
    pub belief_id: String,
    pub event_id: String,
    pub p: f64,
    pub p_raw: f64,
    pub status: String,
    pub supersedes: Option<String>,
    pub outcome: Option<i32>,
    pub brier: Option<f64>,
    pub clv_bps: Option<f64>,
}

/// One belief as the ROTA cognition panel lists it (T4.3 amendment R7a):
/// the scoreboard fields PLUS the persisted `evidence`/`provenance` JSONB —
/// the model's stated reasoning surfaces to the operator (any payload
/// truncation is the presentation layer's concern, not the ledger's).
#[derive(Debug, Clone)]
pub struct BeliefPanelRow {
    pub belief_id: String,
    pub created_at: String,
    pub event_id: String,
    pub p: f64,
    pub p_raw: f64,
    pub status: String,
    pub brier: Option<f64>,
    pub clv_bps: Option<f64>,
    pub evidence: serde_json::Value,
    pub provenance: serde_json::Value,
}

/// Minimal resolved belief row for the Brier-beats-baseline gate (WS1 slice 8b).
/// One row per forward-resolved, scoreable belief: enough to compute the
/// producer Brier score and (with a snapshot lookup) the market baseline Brier.
#[derive(Debug, Clone)]
pub struct ForwardResolvedBeliefRow {
    pub belief_id: String,
    pub p: f64,
    /// `true` = YES outcome, `false` = NO outcome.
    pub outcome: bool,
    pub event_id: String,
    /// ISO8601 UTC: the benchmark window cutoff — snapshot must precede this.
    pub benchmark_at: String,
}

/// One open Aeolus weather belief that is DUE for resolution (the weather
/// "close-the-loop" bridge, source contract §5 Layer 3). The grading-relevant
/// fields are lifted out of the belief's `provenance` JSONB (`model_id='aeolus'`
/// stamps `nws_station_id`/`variable`/`target_date`), so the live resolver routes
/// the belief to its NWS CLI product and picks the realized °F off the row alone —
/// never by re-parsing the source forecast. `event_id` carries the bracket
/// (`aeolus:{event_hint}`); the resolver recovers `(comparison, threshold)` from it.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenWeatherBelief {
    pub belief_id: String,
    pub event_id: String,
    pub p: f64,
    pub variable: String,
    pub nws_station_id: String,
    pub target_date: String,
    pub horizon: String,
    /// Which pipeline stamped this belief (`provenance->>'producer'`).
    /// `None` only for pre-normalisation rows that predate the producer field;
    /// all beliefs written since Slice 1 carry it.
    pub producer: Option<String>,
}

/// Belief ledger ops (spec 5.5): rows are immutable (DB content guard);
/// an update INSERTS a superseding row and flips the prior's status;
/// scoring fills outcome/brier/clv exactly once (repo-enforced over the
/// guard's field-level protection).
pub struct BeliefsRepo {
    pool: PgPool,
}

impl BeliefsRepo {
    pub fn new(pool: PgPool) -> BeliefsRepo {
        BeliefsRepo { pool }
    }

    /// Insert one belief; when `supersedes` is set, the prior row's
    /// status flips to 'superseded' in the same transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        &self,
        belief_id: &str,
        created_at: &str,
        event_id: &str,
        p: f64,
        p_raw: f64,
        horizon: &str,
        evidence: &serde_json::Value,
        provenance: &serde_json::Value,
        supersedes: Option<&str>,
    ) -> Result<(), LedgerError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            r#"INSERT INTO beliefs
               (belief_id, created_at, event_id, p, p_raw, horizon,
                evidence, provenance, supersedes)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
            belief_id,
            created_at,
            event_id,
            p,
            p_raw,
            horizon,
            evidence,
            provenance,
            supersedes
        )
        .execute(&mut *tx)
        .await?;
        if let Some(prior) = supersedes {
            sqlx::query!(
                r#"UPDATE beliefs SET status = 'superseded'
                   WHERE belief_id = $1 AND status = 'open'"#,
                prior
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get(&self, belief_id: &str) -> Result<BeliefRow, LedgerError> {
        let r = sqlx::query!(
            r#"SELECT belief_id, event_id, p, p_raw, status, supersedes,
                      outcome, brier, clv_bps
               FROM beliefs WHERE belief_id = $1"#,
            belief_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(BeliefRow {
            belief_id: r.belief_id,
            event_id: r.event_id,
            p: r.p,
            p_raw: r.p_raw,
            status: r.status,
            supersedes: r.supersedes,
            outcome: r.outcome,
            brier: r.brier,
            clv_bps: r.clv_bps,
        })
    }

    /// R7a (ROTA cognition panel): the newest `limit` beliefs, evidence +
    /// provenance included. ULIDs order lexically == chronologically, so
    /// `belief_id DESC` is newest-first without a timestamp parse. `limit`
    /// clamps to [1, 500] — a read-only panel query never errors on a bad
    /// limit and never fetches unboundedly.
    pub async fn recent(&self, limit: i64) -> Result<Vec<BeliefPanelRow>, LedgerError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query!(
            r#"SELECT belief_id, created_at, event_id, p, p_raw, status,
                      brier, clv_bps, evidence, provenance
               FROM beliefs ORDER BY belief_id DESC LIMIT $1"#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| BeliefPanelRow {
                belief_id: r.belief_id,
                created_at: r.created_at,
                event_id: r.event_id,
                p: r.p,
                p_raw: r.p_raw,
                status: r.status,
                brier: r.brier,
                clv_bps: r.clv_bps,
                evidence: r.evidence,
                provenance: r.provenance,
            })
            .collect())
    }

    /// Resolve + score EXACTLY ONCE: refused unless the belief is still
    /// unscored (outcome IS NULL) and not abandoned.
    pub async fn resolve_and_score(
        &self,
        belief_id: &str,
        outcome: bool,
        brier: f64,
        clv_bps: Option<f64>,
    ) -> Result<(), LedgerError> {
        let res = sqlx::query!(
            r#"UPDATE beliefs
               SET status = 'resolved', outcome = $2, brier = $3, clv_bps = $4
               WHERE belief_id = $1 AND outcome IS NULL
                 AND status IN ('open','superseded')"#,
            belief_id,
            i32::from(outcome),
            brier,
            clv_bps
        )
        .execute(&self.pool)
        .await?;
        if res.rows_affected() != 1 {
            return Err(LedgerError::CorruptRow {
                table: "beliefs",
                reason: format!(
                    "belief {belief_id} not scorable (already scored, abandoned, or missing)"
                ),
            });
        }
        Ok(())
    }

    /// Event died: every open belief on it is abandoned — excluded from
    /// calibration entirely (the world broke the question).
    pub async fn abandon_open_for_event(&self, event_id: &str) -> Result<u64, LedgerError> {
        let res = sqlx::query!(
            r#"UPDATE beliefs SET status = 'abandoned'
               WHERE event_id = $1 AND status = 'open'"#,
            event_id
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Calibration inputs: (p, outcome) for RESOLVED beliefs in a
    /// category (joined through events). Unscoreable events are excluded
    /// (spec 5.12: no beliefs nobody can grade).
    pub async fn resolved_samples(&self, category: &str) -> Result<Vec<(f64, bool)>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT b.p, b.outcome as "outcome!"
               FROM beliefs b JOIN events e ON e.event_id = b.event_id
               WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                 AND e.category = $1 AND NOT e.unscoreable
               ORDER BY b.created_at"#,
            category
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.p, r.outcome == 1)).collect())
    }

    /// Review inputs: full resolved stats (p, outcome, brier, clv) for a
    /// category — the weekly calibration audit's query (T3.1).
    pub async fn resolved_stats(&self, category: &str) -> Result<Vec<ResolvedStat>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT b.p, b.outcome as "outcome!", b.brier as "brier!", b.clv_bps
               FROM beliefs b JOIN events e ON e.event_id = b.event_id
               WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                 AND b.brier IS NOT NULL AND e.category = $1
                 AND NOT e.unscoreable
               ORDER BY b.created_at"#,
            category
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ResolvedStat {
                p: r.p,
                outcome: r.outcome == 1,
                brier: r.brier,
                clv_bps: r.clv_bps,
            })
            .collect())
    }

    /// Per-producer variant of `resolved_stats`: same shape (`p, outcome, brier,
    /// clv_bps`) but restricted to beliefs whose `provenance->>'producer'` equals
    /// `producer`. Enables independent head-to-head calibration audit between
    /// Aeolus and the meteorologist baseline (WS1 slice 6; used by slice 7 to
    /// select the best-calibrated producer per category). The `$producer` param
    /// is never inlined in the SQL — no producer literal appears in the query.
    pub async fn resolved_stats_for_producer(
        &self,
        producer: &str,
        category: &str,
    ) -> Result<Vec<ResolvedStat>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT b.p, b.outcome as "outcome!", b.brier as "brier!", b.clv_bps
               FROM beliefs b JOIN events e ON e.event_id = b.event_id
               WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                 AND b.brier IS NOT NULL AND e.category = $2
                 AND NOT e.unscoreable
                 AND b.provenance->>'producer' = $1
               ORDER BY b.created_at"#,
            producer,
            category
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ResolvedStat {
                p: r.p,
                outcome: r.outcome == 1,
                brier: r.brier,
                clv_bps: r.clv_bps,
            })
            .collect())
    }

    /// §9.1 forward-only resolved count for the GO/NO-GO volume gate (WS1 slice 8a).
    ///
    /// Counts resolved, scored, scoreable beliefs in `category` whose
    /// `provenance->>'source'` IS DISTINCT FROM `'historical-import'` — i.e.
    /// NULL source (forward live beliefs) and any non-import source are counted,
    /// but WS3 backtest seeds are excluded.
    ///
    /// When `producer` is `Some(p)`, restricts to beliefs whose
    /// `provenance->>'producer'` equals `p` (mirrors `resolved_stats_for_producer`).
    /// When `producer` is `None`, counts the merged scope (mirrors `resolved_stats`).
    ///
    /// **No-op today**: no `source='historical-import'` rows exist until WS3
    /// lands, so this returns the same value as `resolved_stats(_for_producer).len()`
    /// until then (byte-identical-today guarantee).
    pub async fn resolved_count_forward(
        &self,
        producer: Option<&str>,
        category: &str,
    ) -> Result<i64, LedgerError> {
        let count = if let Some(prod) = producer {
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) AS "count!"
                   FROM beliefs b JOIN events e ON e.event_id = b.event_id
                   WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                     AND b.brier IS NOT NULL AND e.category = $2
                     AND NOT e.unscoreable
                     AND b.provenance->>'producer' = $1
                     AND b.provenance->>'source' IS DISTINCT FROM 'historical-import'"#,
                prod,
                category
            )
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) AS "count!"
                   FROM beliefs b JOIN events e ON e.event_id = b.event_id
                   WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                     AND b.brier IS NOT NULL AND e.category = $1
                     AND NOT e.unscoreable
                     AND b.provenance->>'source' IS DISTINCT FROM 'historical-import'"#,
                category
            )
            .fetch_one(&self.pool)
            .await?
        };
        Ok(count)
    }

    /// DATA-driven candidate-producer set for a category (WS1 slice 7): the
    /// DISTINCT `provenance->>'producer'` values from resolved, scored, scoreable
    /// beliefs in the given category. Ordered `producer ASC` so the caller's
    /// stable-max over quality gives deterministic tie-breaking without
    /// inspecting producer names (producers are DATA, never literals).
    /// Beliefs with no `producer` key in provenance are excluded (`IS NOT NULL`).
    /// An empty result means no resolved, producer-stamped beliefs exist (cold-start).
    pub async fn producers_for_resolved_category(
        &self,
        category: &str,
    ) -> Result<Vec<String>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT DISTINCT b.provenance->>'producer' AS "producer!"
               FROM beliefs b JOIN events e ON e.event_id = b.event_id
               WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                 AND b.brier IS NOT NULL AND e.category = $1
                 AND NOT e.unscoreable
                 AND b.provenance->>'producer' IS NOT NULL
               ORDER BY b.provenance->>'producer'"#,
            category
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.producer).collect())
    }

    /// Resolved beliefs attributed to one persona scope, keyed by the fan-out
    /// provenance `{persona_id, persona_version}` (`map_persona_analysis` stamps it).
    /// Shaped for `persona_scoring::score_persona` / `propose_promotion` (§10/§11) and
    /// the §20.1 ROTA personas-view: the calibrated `(p, outcome)` samples + CLV over
    /// SCOREABLE, resolved events (mirrors `resolved_stats`, keyed on provenance
    /// instead of category). Non-persona beliefs (no matching provenance) are excluded.
    pub async fn resolved_persona_stats(
        &self,
        persona_id: &str,
        persona_version: i32,
    ) -> Result<ResolvedPersonaStats, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT b.p, b.outcome as "outcome!", b.clv_bps
               FROM beliefs b JOIN events e ON e.event_id = b.event_id
               WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                 AND NOT e.unscoreable
                 AND b.provenance->>'persona_id' = $1
                 AND (b.provenance->>'persona_version')::int = $2
               ORDER BY b.created_at"#,
            persona_id,
            persona_version,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut samples = Vec::with_capacity(rows.len());
        let mut clv_bps = Vec::new();
        for r in rows {
            samples.push((r.p, r.outcome == 1));
            if let Some(c) = r.clv_bps {
                clv_bps.push(c);
            }
        }
        Ok(ResolvedPersonaStats {
            persona_id: persona_id.to_string(),
            persona_version,
            samples,
            clv_bps,
        })
    }

    /// Open weather-bracket beliefs whose window has CLOSED at `now_iso` — the
    /// `resolve_and_score_weather_beliefs` work queue (source contract §5 Layer
    /// 3; mirrors `ScalarBeliefsRepo::unresolved_due`). A row qualifies iff it is
    /// still `open`, carries all three grading keys (`nws_station_id`, `variable`,
    /// `target_date` in its provenance JSON), and is due (`horizon <= $1`).
    /// Selection is **producer-agnostic**: any belief carrying the weather grading
    /// keys qualifies, regardless of which pipeline produced it (Aeolus,
    /// meteorologist, or any future producer). Persona MACRO beliefs and synthesis
    /// beliefs lack the grading keys and are therefore excluded automatically.
    /// `horizon` is ISO8601 TEXT with fixed-ms precision (sorts lexically ==
    /// chronologically), so `<=` is a correct chronological gate and
    /// `ORDER BY horizon ASC` is oldest-due-first. `limit` caps the batch (the
    /// loop drains in bounded chunks; a later run takes the rest).
    pub async fn open_weather_bracket_due(
        &self,
        now_iso: &str,
        limit: i64,
    ) -> Result<Vec<OpenWeatherBelief>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT belief_id, event_id, p,
                      provenance->>'variable'       AS variable,
                      provenance->>'nws_station_id' AS nws_station_id,
                      provenance->>'target_date'    AS target_date,
                      provenance->>'producer'       AS producer,
                      horizon
               FROM beliefs
               WHERE status = 'open'
                 AND provenance->>'nws_station_id' IS NOT NULL
                 AND provenance->>'variable'       IS NOT NULL
                 AND provenance->>'target_date'    IS NOT NULL
                 AND horizon <= $1
               ORDER BY horizon ASC, belief_id ASC
               LIMIT $2"#,
            now_iso,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(OpenWeatherBelief {
                    belief_id: r.belief_id,
                    event_id: r.event_id,
                    p: r.p,
                    variable: r.variable?,
                    nws_station_id: r.nws_station_id?,
                    target_date: r.target_date?,
                    horizon: r.horizon,
                    producer: r.producer,
                })
            })
            .collect())
    }

    /// Forward-only resolved belief rows for the Brier-beats-baseline gate
    /// (WS1 slice 8b; spec §11 + §9.1).
    ///
    /// Returns `(belief_id, p, outcome, event_id, benchmark_at)` for every
    /// resolved, scoreable, forward belief in `category`. "Forward" means
    /// `provenance->>'source' IS DISTINCT FROM 'historical-import'` — this
    /// excludes WS3 backtest rows and is consistent with `resolved_count_forward`.
    ///
    /// The caller is responsible for fetching the benchmark snapshot per belief
    /// (via `SnapshotsRepo::latest_liquid_before`) and computing the per-belief
    /// de-vigged market probability and market baseline Brier contribution.
    /// Beliefs with no mapped market or no pre-benchmark snapshot are SKIPPED
    /// by the caller — they do not contribute to the baseline.
    pub async fn forward_resolved_for_brier_baseline(
        &self,
        category: &str,
    ) -> Result<Vec<ForwardResolvedBeliefRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT b.belief_id, b.p, b.outcome AS "outcome!", b.event_id,
                      e.benchmark_at
               FROM beliefs b JOIN events e ON e.event_id = b.event_id
               WHERE b.status = 'resolved' AND b.outcome IS NOT NULL
                 AND b.brier IS NOT NULL AND e.category = $1
                 AND NOT e.unscoreable
                 AND b.provenance->>'source' IS DISTINCT FROM 'historical-import'
               ORDER BY b.created_at"#,
            category
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ForwardResolvedBeliefRow {
                belief_id: r.belief_id,
                p: r.p,
                outcome: r.outcome == 1,
                event_id: r.event_id,
                benchmark_at: r.benchmark_at,
            })
            .collect())
    }

    /// Test hook proving the DATABASE guard refuses content mutation
    /// (never used by production code).
    pub async fn try_mutate_content_for_test(
        &self,
        belief_id: &str,
        new_p: f64,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"UPDATE beliefs SET p = $2 WHERE belief_id = $1"#,
            belief_id,
            new_p
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// One journal row (episodic memory, spec 5.6; written by the daily
/// reconciliation loop, spec 5.8).
#[derive(Debug, Clone)]
pub struct JournalRow {
    pub journal_id: String,
    pub day: String,
    pub body: serde_json::Value,
}

/// Journal entries: INSERT-only (table trigger refuses mutation), one
/// per UTC day (unique index).
pub struct JournalRepo {
    pool: PgPool,
}

impl JournalRepo {
    pub fn new(pool: PgPool) -> JournalRepo {
        JournalRepo { pool }
    }

    pub async fn insert(
        &self,
        journal_id: &str,
        day: &str,
        body: &serde_json::Value,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO journal (journal_id, day, body, created_at)
               VALUES ($1,$2,$3,$4)"#,
            journal_id,
            day,
            body,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_day(&self, day: &str) -> Result<Option<JournalRow>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT journal_id, day, body FROM journal WHERE day = $1"#,
            day
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| JournalRow {
            journal_id: r.journal_id,
            day: r.day,
            body: r.body,
        }))
    }

    /// Inclusive day window, day-ordered (the weekly review's episodic
    /// input, spec 5.8).
    pub async fn range(
        &self,
        from_day: &str,
        to_day: &str,
    ) -> Result<Vec<JournalRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT journal_id, day, body FROM journal
               WHERE day >= $1 AND day <= $2 ORDER BY day"#,
            from_day,
            to_day
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| JournalRow {
                journal_id: r.journal_id,
                day: r.day,
                body: r.body,
            })
            .collect())
    }
}

// ------------------------------------------------------------------------
// calibration params (T2.8, spec 5.10)
// ------------------------------------------------------------------------

/// One versioned calibration parameter set for a (model, strategy,
/// category, kind) scope.
#[derive(Debug, Clone)]
pub struct CalibrationParamsRow {
    pub param_id: String,
    pub model_id: String,
    pub strategy: String,
    pub category: String,
    pub kind: String,
    pub params: serde_json::Value,
    pub version: i32,
    pub effective_at: String,
}

/// Versioned calibration parameters (spec 5.10: "deterministic code with
/// versioned parameters; parameter updates are config changes recorded
/// in audit"). INSERT-only: an update is a NEW version row; the UNIQUE
/// (model, strategy, category, kind, version) key refuses re-issues and
/// the T0.8 trigger refuses mutation.
pub struct CalibrationParamsRepo {
    pool: PgPool,
}

impl CalibrationParamsRepo {
    pub fn new(pool: PgPool) -> CalibrationParamsRepo {
        CalibrationParamsRepo { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        &self,
        param_id: &str,
        model_id: &str,
        strategy: &str,
        category: &str,
        kind: &str,
        params: &serde_json::Value,
        version: i32,
        effective_at: &str,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO calibration_params
               (param_id, model_id, strategy, category, kind, params,
                version, effective_at, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
            param_id,
            model_id,
            strategy,
            category,
            kind,
            params,
            version,
            effective_at,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The highest-version parameter set for the scope, if any.
    pub async fn latest(
        &self,
        model_id: &str,
        strategy: &str,
        category: &str,
        kind: &str,
    ) -> Result<Option<CalibrationParamsRow>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT param_id, model_id, strategy, category, kind,
                      params, version, effective_at
               FROM calibration_params
               WHERE model_id = $1 AND strategy = $2 AND category = $3
                 AND kind = $4
               ORDER BY version DESC LIMIT 1"#,
            model_id,
            strategy,
            category,
            kind
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| CalibrationParamsRow {
            param_id: r.param_id,
            model_id: r.model_id,
            strategy: r.strategy,
            category: r.category,
            kind: r.kind,
            params: r.params,
            version: r.version,
            effective_at: r.effective_at,
        }))
    }

    /// R7b (ROTA cognition panel): every DISTINCT calibration scope at its
    /// MAX version — one row per (model, strategy, category, kind), the
    /// Postgres `DISTINCT ON` idiom over the version ordering. Empty table
    /// => empty vec, never an error.
    pub async fn scopes(&self) -> Result<Vec<CalibrationScopeRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT DISTINCT ON (model_id, strategy, category, kind)
                      model_id, strategy, category, kind, version, effective_at
               FROM calibration_params
               ORDER BY model_id, strategy, category, kind, version DESC"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| CalibrationScopeRow {
                model_id: r.model_id,
                strategy: r.strategy,
                category: r.category,
                kind: r.kind,
                version: r.version,
                effective_at: r.effective_at,
            })
            .collect())
    }
}

/// One calibration scope at its highest version (T4.3 amendment R7b — the
/// cognition panel's scope enumeration).
#[derive(Debug, Clone)]
pub struct CalibrationScopeRow {
    pub model_id: String,
    pub strategy: String,
    pub category: String,
    pub kind: String,
    pub version: i32,
    pub effective_at: String,
}

// ------------------------------------------------------------------------
// lessons (T3.1, spec 5.6 semantic memory)
// ------------------------------------------------------------------------

/// One semantic-memory lesson row.
#[derive(Debug, Clone)]
pub struct LessonRow {
    pub lesson_id: String,
    pub body: String,
    pub provenance: serde_json::Value,
    pub status: String,
    pub review_at: String,
    pub supersedes: Option<String>,
}

/// Semantic memory (spec 5.6): a bounded list of distilled lessons with
/// provenance and review dates. The table is append-only (T0.8 trigger):
/// confirmation and demotion are SUPERSEDING inserts; the chain head is
/// the live row. Promotion (the initial insert) is an OPERATOR action —
/// the weekly review only drafts candidates.
pub struct LessonsRepo {
    pool: PgPool,
}

impl LessonsRepo {
    pub fn new(pool: PgPool) -> LessonsRepo {
        LessonsRepo { pool }
    }

    /// Insert an operator-approved lesson as ACTIVE.
    pub async fn insert(
        &self,
        lesson_id: &str,
        body: &str,
        provenance: &serde_json::Value,
        review_at: &str,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO lessons
               (lesson_id, body, provenance, status, review_at, created_at)
               VALUES ($1,$2,$3,'active',$4,$5)"#,
            lesson_id,
            body,
            provenance,
            review_at,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Supersede `prior_id` with a new row carrying `status`. Refused
    /// unless the prior row is the live chain head (active and not
    /// already superseded).
    async fn supersede(
        &self,
        prior_id: &str,
        new_id: &str,
        status: &str,
        review_at: Option<&str>,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        let mut tx = self.pool.begin().await?;
        let prior = sqlx::query!(
            r#"SELECT body, provenance, review_at FROM lessons
               WHERE lesson_id = $1 AND status = 'active'
                 AND NOT EXISTS (
                   SELECT 1 FROM lessons l2 WHERE l2.supersedes = lessons.lesson_id
                 )"#,
            prior_id
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(prior) = prior else {
            return Err(LedgerError::CorruptRow {
                table: "lessons",
                reason: format!("lesson {prior_id} is not the live chain head"),
            });
        };
        sqlx::query!(
            r#"INSERT INTO lessons
               (lesson_id, body, provenance, status, review_at, supersedes, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7)"#,
            new_id,
            prior.body,
            prior.provenance,
            status,
            review_at.unwrap_or(&prior.review_at),
            prior_id,
            created_at
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Weekly confirmation: the lesson held up; extend its review date.
    pub async fn confirm(
        &self,
        prior_id: &str,
        new_id: &str,
        new_review_at: &str,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        self.supersede(prior_id, new_id, "active", Some(new_review_at), created_at)
            .await
    }

    /// Monthly decay (spec 5.6): an unconfirmed lesson demotes.
    pub async fn demote(
        &self,
        prior_id: &str,
        new_id: &str,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        self.supersede(prior_id, new_id, "demoted", None, created_at)
            .await
    }

    /// The live semantic memory: active chain heads, oldest first.
    pub async fn active(&self) -> Result<Vec<LessonRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT lesson_id, body, provenance, status, review_at, supersedes
               FROM lessons l
               WHERE status = 'active'
                 AND NOT EXISTS (
                   SELECT 1 FROM lessons l2 WHERE l2.supersedes = l.lesson_id
                 )
               ORDER BY created_at, lesson_id"#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| LessonRow {
                lesson_id: r.lesson_id,
                body: r.body,
                provenance: r.provenance,
                status: r.status,
                review_at: r.review_at,
                supersedes: r.supersedes,
            })
            .collect())
    }
}

// ------------------------------------------------------------------------
// tradability scores (T3.2, spec 5.12 market-back discovery)
// ------------------------------------------------------------------------

/// One persisted tradability scoring run.
#[derive(Debug, Clone)]
pub struct TradabilityRow {
    pub score_id: String,
    pub market_id: String,
    pub venue: String,
    pub score: f64,
    pub components: serde_json::Value,
    pub created_at: String,
}

/// Tradability scores (spec 5.12: persisted per market, append-only —
/// the score history is part of the discovery record).
pub struct TradabilityRepo {
    pool: PgPool,
}

impl TradabilityRepo {
    pub fn new(pool: PgPool) -> TradabilityRepo {
        TradabilityRepo { pool }
    }

    pub async fn insert(
        &self,
        score_id: &str,
        market_id: &str,
        venue: &str,
        score: f64,
        components: &serde_json::Value,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO tradability_scores
               (score_id, market_id, venue, score, components, created_at)
               VALUES ($1,$2,$3,$4,$5,$6)"#,
            score_id,
            market_id,
            venue,
            score,
            components,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The most recent score for a market, if any.
    pub async fn latest(&self, market_id: &str) -> Result<Option<TradabilityRow>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT score_id, market_id, venue, score, components, created_at
               FROM tradability_scores WHERE market_id = $1
               ORDER BY created_at DESC, score_id DESC LIMIT 1"#,
            market_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| TradabilityRow {
            score_id: r.score_id,
            market_id: r.market_id,
            venue: r.venue,
            score: r.score,
            components: r.components,
            created_at: r.created_at,
        }))
    }
}

// ---------- Track E: persona registry + domain-analysis artifact (design §5) ----------

/// One row of the append-only, supersedes-chained persona registry (design §6).
#[derive(Debug, Clone)]
pub struct PersonaRow {
    pub persona_row_id: String,
    pub persona_id: String,
    pub version: i32,
    pub domain: String,
    pub domain_tags: serde_json::Value,
    pub reads_signal_kinds: serde_json::Value,
    pub tier: String,
    pub method_hash: String,
    pub output_schema_version: String,
    pub status: String,
    pub supersedes: Option<String>,
    pub effective_at: String,
    pub created_at: String,
}

/// Versioned persona registry. INSERT-only: a method change is a NEW
/// (persona_id, version) row that supersedes the old; the UNIQUE
/// (persona_id, version) key refuses a re-issue and the migration trigger
/// refuses UPDATE/DELETE (mirrors `calibration_params`/`lessons`). The
/// `method_hash` lets the slice-2 loader prove which method produced an
/// analysis and refuse a config/registry mismatch.
pub struct PersonasRepo {
    pool: PgPool,
}

impl PersonasRepo {
    pub fn new(pool: PgPool) -> PersonasRepo {
        PersonasRepo { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        &self,
        persona_row_id: &str,
        persona_id: &str,
        version: i32,
        domain: &str,
        domain_tags: &serde_json::Value,
        reads_signal_kinds: &serde_json::Value,
        tier: &str,
        method_hash: &str,
        output_schema_version: &str,
        status: &str,
        supersedes: Option<&str>,
        effective_at: &str,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO personas
               (persona_row_id, persona_id, version, domain, domain_tags,
                reads_signal_kinds, tier, method_hash, output_schema_version,
                status, supersedes, effective_at, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)"#,
            persona_row_id,
            persona_id,
            version,
            domain,
            domain_tags,
            reads_signal_kinds,
            tier,
            method_hash,
            output_schema_version,
            status,
            supersedes,
            effective_at,
            created_at,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The highest-version registry row for a persona (the head the loader
    /// hashes against), or None if the persona has no rows. Empty -> None,
    /// never an error.
    pub async fn head(&self, persona_id: &str) -> Result<Option<PersonaRow>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT persona_row_id, persona_id, version, domain, domain_tags,
                      reads_signal_kinds, tier, method_hash, output_schema_version,
                      status, supersedes, effective_at, created_at
               FROM personas
               WHERE persona_id = $1
               ORDER BY version DESC LIMIT 1"#,
            persona_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| PersonaRow {
            persona_row_id: r.persona_row_id,
            persona_id: r.persona_id,
            version: r.version,
            domain: r.domain,
            domain_tags: r.domain_tags,
            reads_signal_kinds: r.reads_signal_kinds,
            tier: r.tier,
            method_hash: r.method_hash,
            output_schema_version: r.output_schema_version,
            status: r.status,
            supersedes: r.supersedes,
            effective_at: r.effective_at,
            created_at: r.created_at,
        }))
    }
}

/// One persisted domain-analysis artifact (design §5). Content-immutable: the
/// `content_hash` over findings + signal_manifest is the replay anchor (5.7/I5).
#[derive(Debug, Clone)]
pub struct DomainAnalysisRow {
    pub analysis_id: String,
    pub persona_id: String,
    pub persona_version: i32,
    pub domain: String,
    pub region_key: String,
    pub produced_at: String,
    pub signal_manifest: serde_json::Value,
    pub findings: serde_json::Value,
    pub content_hash: String,
    pub manifest_hash: String,
    pub cost_cents: i64,
    pub status: String,
    pub supersedes: Option<String>,
    pub created_at: String,
}

/// The append-only domain-analysis store. Content-immutable like `beliefs`:
/// the database guard freezes every content field and refuses DELETE; only
/// `status` may flip open->superseded. A fresh analysis for a region supersedes
/// the prior one (the prior row's status flips in the same transaction, mirroring
/// `BeliefsRepo::insert`).
pub struct DomainAnalysesRepo {
    pool: PgPool,
}

impl DomainAnalysesRepo {
    pub fn new(pool: PgPool) -> DomainAnalysesRepo {
        DomainAnalysesRepo { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        &self,
        analysis_id: &str,
        persona_id: &str,
        persona_version: i32,
        domain: &str,
        region_key: &str,
        produced_at: &str,
        signal_manifest: &serde_json::Value,
        findings: &serde_json::Value,
        content_hash: &str,
        manifest_hash: &str,
        cost_cents: i64,
        supersedes: Option<&str>,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            r#"INSERT INTO domain_analyses
               (analysis_id, persona_id, persona_version, domain, region_key,
                produced_at, signal_manifest, findings, content_hash,
                manifest_hash, cost_cents, supersedes, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)"#,
            analysis_id,
            persona_id,
            persona_version,
            domain,
            region_key,
            produced_at,
            signal_manifest,
            findings,
            content_hash,
            manifest_hash,
            cost_cents,
            supersedes,
            created_at,
        )
        .execute(&mut *tx)
        .await?;
        if let Some(prior) = supersedes {
            sqlx::query!(
                r#"UPDATE domain_analyses SET status = 'superseded'
                   WHERE analysis_id = $1 AND status = 'open'"#,
                prior
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get(&self, analysis_id: &str) -> Result<DomainAnalysisRow, LedgerError> {
        let r = sqlx::query!(
            r#"SELECT analysis_id, persona_id, persona_version, domain, region_key,
                      produced_at, signal_manifest, findings, content_hash,
                      manifest_hash, cost_cents, status, supersedes, created_at
               FROM domain_analyses WHERE analysis_id = $1"#,
            analysis_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(DomainAnalysisRow {
            analysis_id: r.analysis_id,
            persona_id: r.persona_id,
            persona_version: r.persona_version,
            domain: r.domain,
            region_key: r.region_key,
            produced_at: r.produced_at,
            signal_manifest: r.signal_manifest,
            findings: r.findings,
            content_hash: r.content_hash,
            manifest_hash: r.manifest_hash,
            cost_cents: r.cost_cents,
            status: r.status,
            supersedes: r.supersedes,
            created_at: r.created_at,
        })
    }

    /// The current (open) artifact for a region, newest-first, or None. The one
    /// analysis many beliefs reference (design §9); empty -> None, never error.
    pub async fn current_for_region(
        &self,
        domain: &str,
        region_key: &str,
    ) -> Result<Option<DomainAnalysisRow>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT analysis_id, persona_id, persona_version, domain, region_key,
                      produced_at, signal_manifest, findings, content_hash,
                      manifest_hash, cost_cents, status, supersedes, created_at
               FROM domain_analyses
               WHERE domain = $1 AND region_key = $2 AND status = 'open'
               ORDER BY produced_at DESC, analysis_id DESC LIMIT 1"#,
            domain,
            region_key
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| DomainAnalysisRow {
            analysis_id: r.analysis_id,
            persona_id: r.persona_id,
            persona_version: r.persona_version,
            domain: r.domain,
            region_key: r.region_key,
            produced_at: r.produced_at,
            signal_manifest: r.signal_manifest,
            findings: r.findings,
            content_hash: r.content_hash,
            manifest_hash: r.manifest_hash,
            cost_cents: r.cost_cents,
            status: r.status,
            supersedes: r.supersedes,
            created_at: r.created_at,
        }))
    }
}

/// One persisted scalar-belief row (design §1.4). The durable, immutable
/// scalar forecast claim; `realized_value`/`resolved_at` are NULL until the
/// belief resolves (set exactly once). `quantiles`/`provenance` ride as
/// `serde_json::Value` — the caller serializes the cognition
/// `PredictiveDistribution::Scalar` before insert, so this crate never imports
/// it in production code (cognition is dev-only here).
#[derive(Debug, Clone)]
pub struct ScalarBeliefRow {
    pub belief_id: String,
    pub producer: String,
    pub event_key: String,
    pub quantiles: serde_json::Value,
    pub unit: String,
    pub horizon: String,
    pub provenance: serde_json::Value,
    pub created_at: String,
    pub realized_value: Option<f64>,
    pub resolved_at: Option<String>,
}

/// Scalar-belief ledger ops (design §1.4): rows are immutable (the
/// `scalar_beliefs_guard` DB trigger blocks content mutation + DELETE, allows
/// the resolution columns to be set once from NULL). `producer` is first-class
/// so the ROTA §9.1 scorecard groups by it. INSERT-only at the app layer.
pub struct ScalarBeliefsRepo {
    pool: PgPool,
}

impl ScalarBeliefsRepo {
    pub fn new(pool: PgPool) -> ScalarBeliefsRepo {
        ScalarBeliefsRepo { pool }
    }

    /// Insert one scalar belief. The belief is unresolved on insert
    /// (`realized_value`/`resolved_at` NULL). Returns `Ok(true)` when
    /// inserted, `Ok(false)` when a belief with the same `(producer,
    /// event_key)` already exists (idempotent under at-least-once delivery).
    /// Append-only: the trigger refuses any later content mutation.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        &self,
        belief_id: &str,
        producer: &str,
        event_key: &str,
        quantiles: &serde_json::Value,
        unit: &str,
        horizon: &str,
        provenance: &serde_json::Value,
        created_at: &str,
    ) -> Result<bool, LedgerError> {
        let result = sqlx::query!(
            r#"INSERT INTO scalar_beliefs
               (belief_id, producer, event_key, quantiles, unit, horizon,
                provenance, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
               ON CONFLICT (producer, event_key) DO NOTHING"#,
            belief_id,
            producer,
            event_key,
            quantiles,
            unit,
            horizon,
            provenance,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn get(&self, belief_id: &str) -> Result<ScalarBeliefRow, LedgerError> {
        let r = sqlx::query!(
            r#"SELECT belief_id, producer, event_key, quantiles, unit, horizon,
                      provenance, created_at, realized_value, resolved_at
               FROM scalar_beliefs WHERE belief_id = $1"#,
            belief_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(ScalarBeliefRow {
            belief_id: r.belief_id,
            producer: r.producer,
            event_key: r.event_key,
            quantiles: r.quantiles,
            unit: r.unit,
            horizon: r.horizon,
            provenance: r.provenance,
            created_at: r.created_at,
            realized_value: r.realized_value,
            resolved_at: r.resolved_at,
        })
    }

    /// Resolve EXACTLY ONCE (mirrors `BeliefsRepo::resolve_and_score`): the
    /// realized value + resolved_at are written iff the belief is still
    /// unresolved (`realized_value IS NULL`). A second resolution — or a
    /// missing belief — affects zero rows and is refused as `CorruptRow`, so a
    /// scalar belief is scored once. The DB trigger ALSO enforces the
    /// set-once transition; this repo guard makes the refusal a typed error.
    pub async fn resolve(
        &self,
        belief_id: &str,
        realized_value: f64,
        resolved_at: &str,
    ) -> Result<(), LedgerError> {
        let res = sqlx::query!(
            r#"UPDATE scalar_beliefs
               SET realized_value = $2, resolved_at = $3
               WHERE belief_id = $1 AND realized_value IS NULL"#,
            belief_id,
            realized_value,
            resolved_at
        )
        .execute(&self.pool)
        .await?;
        if res.rows_affected() != 1 {
            return Err(LedgerError::CorruptRow {
                table: "scalar_beliefs",
                reason: format!(
                    "scalar belief {belief_id} not resolvable (already resolved or missing)"
                ),
            });
        }
        Ok(())
    }

    /// Unresolved beliefs from one `producer` whose window has CLOSED at
    /// `now_iso` (the resolve/score loop's work queue, design §9.1). A row
    /// qualifies iff `producer = $1 AND realized_value IS NULL AND horizon <=
    /// $2`: the window closes at `horizon` (the `next_funding_time` the forecast
    /// resolves at), so `horizon <= now` means it is due. `horizon` is ISO8601
    /// TEXT with fixed millisecond precision, which sorts lexically ==
    /// chronologically, so `ORDER BY horizon ASC` is oldest-due-first and the
    /// `<=` bound is a correct chronological gate. `limit` caps the batch (the
    /// loop drains in bounded chunks; a later run picks up the rest). A belief
    /// still missing its realized rate stays unresolved here and is re-listed by
    /// the NEXT run — being due is necessary but not sufficient to score.
    pub async fn unresolved_due(
        &self,
        producer: &str,
        now_iso: &str,
        limit: i64,
    ) -> Result<Vec<ScalarBeliefRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT belief_id, producer, event_key, quantiles, unit, horizon,
                      provenance, created_at, realized_value, resolved_at
               FROM scalar_beliefs
               WHERE producer = $1 AND realized_value IS NULL AND horizon <= $2
               ORDER BY horizon ASC, belief_id ASC
               LIMIT $3"#,
            producer,
            now_iso,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ScalarBeliefRow {
                belief_id: r.belief_id,
                producer: r.producer,
                event_key: r.event_key,
                quantiles: r.quantiles,
                unit: r.unit,
                horizon: r.horizon,
                provenance: r.provenance,
                created_at: r.created_at,
                realized_value: r.realized_value,
                resolved_at: r.resolved_at,
            })
            .collect())
    }

    /// The newest `limit` scalar beliefs (the ROTA §9.1 scorecard feed). ULIDs
    /// order lexically == chronologically, so `belief_id DESC` is newest-first
    /// without a timestamp parse. `limit` clamps to [1, 500] — a read-only
    /// panel query never errors on a bad limit and never fetches unboundedly.
    pub async fn recent(&self, limit: i64) -> Result<Vec<ScalarBeliefRow>, LedgerError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query!(
            r#"SELECT belief_id, producer, event_key, quantiles, unit, horizon,
                      provenance, created_at, realized_value, resolved_at
               FROM scalar_beliefs ORDER BY belief_id DESC LIMIT $1"#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ScalarBeliefRow {
                belief_id: r.belief_id,
                producer: r.producer,
                event_key: r.event_key,
                quantiles: r.quantiles,
                unit: r.unit,
                horizon: r.horizon,
                provenance: r.provenance,
                created_at: r.created_at,
                realized_value: r.realized_value,
                resolved_at: r.resolved_at,
            })
            .collect())
    }
}

/// One derived, rule-tagged score over an immutable scalar belief (design
/// §1.3). `score` is lower-is-better; `rule_id` is the
/// `ScoringRule::id()` string (e.g. "crps_pinball").
#[derive(Debug, Clone)]
pub struct BeliefScoreRow {
    pub score_id: String,
    pub belief_id: String,
    pub rule_id: String,
    pub score: f64,
    pub scored_at: String,
}

/// Score ledger ops (design §1.3): one row per `(belief_id, rule_id)` — the
/// unique constraint enforces exactly-once per rule, and several scorers run
/// side by side over the same immutable facts. Fully immutable (the blunt
/// `belief_scores_append_only` trigger refuses UPDATE/DELETE). INSERT-only.
pub struct BeliefScoresRepo {
    pool: PgPool,
}

impl BeliefScoresRepo {
    pub fn new(pool: PgPool) -> BeliefScoresRepo {
        BeliefScoresRepo { pool }
    }

    /// Insert one `(belief_id, rule_id)` score. STRICT: a duplicate
    /// `(belief_id, rule_id)` bubbles the unique-violation as a `LedgerError`
    /// (never `ON CONFLICT DO NOTHING` — a re-score is a NEW rule id, not a
    /// silent no-op).
    pub async fn insert(
        &self,
        score_id: &str,
        belief_id: &str,
        rule_id: &str,
        score: f64,
        scored_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO belief_scores
               (score_id, belief_id, rule_id, score, scored_at)
               VALUES ($1,$2,$3,$4,$5)"#,
            score_id,
            belief_id,
            rule_id,
            score,
            scored_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every score for one belief (the per-belief scorecard column — multiple
    /// scorers side by side). Ordered by rule for a stable read.
    pub async fn scores_for_belief(
        &self,
        belief_id: &str,
    ) -> Result<Vec<BeliefScoreRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT score_id, belief_id, rule_id, score, scored_at
               FROM belief_scores WHERE belief_id = $1
               ORDER BY rule_id"#,
            belief_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| BeliefScoreRow {
                score_id: r.score_id,
                belief_id: r.belief_id,
                rule_id: r.rule_id,
                score: r.score,
                scored_at: r.scored_at,
            })
            .collect())
    }

    /// The newest `limit` scores for one rule across beliefs (the §9.1 rolling
    /// calibration feed per `rule_id`). `scored_at` is ULID-free wall time, so
    /// order by it then `score_id` for determinism. `limit` clamps to
    /// [1, 500] like the other read-only feeds.
    pub async fn scores_for_rule(
        &self,
        rule_id: &str,
        limit: i64,
    ) -> Result<Vec<BeliefScoreRow>, LedgerError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query!(
            r#"SELECT score_id, belief_id, rule_id, score, scored_at
               FROM belief_scores WHERE rule_id = $1
               ORDER BY scored_at DESC, score_id DESC LIMIT $2"#,
            rule_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| BeliefScoreRow {
                score_id: r.score_id,
                belief_id: r.belief_id,
                rule_id: r.rule_id,
                score: r.score,
                scored_at: r.scored_at,
            })
            .collect())
    }

    /// The newest `limit` scores for one `(producer, rule_id)` pair, joining
    /// `belief_scores` → `scalar_beliefs` on `belief_id` so the scalar
    /// belief's first-class `producer` column acts as the filter. Enables the
    /// ROTA §9.1 per-producer scorecard: Aeolus vs any other producer measured
    /// independently under the same scoring rule. `$producer` is always a param
    /// — no producer literal in the SQL. `limit` clamps to [1, 500].
    pub async fn scores_for_producer(
        &self,
        producer: &str,
        rule_id: &str,
        limit: i64,
    ) -> Result<Vec<BeliefScoreRow>, LedgerError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query!(
            r#"SELECT bs.score_id, bs.belief_id, bs.rule_id, bs.score, bs.scored_at
               FROM belief_scores bs
               JOIN scalar_beliefs sb ON sb.belief_id = bs.belief_id
               WHERE sb.producer = $1 AND bs.rule_id = $2
               ORDER BY bs.scored_at DESC, bs.score_id DESC LIMIT $3"#,
            producer,
            rule_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| BeliefScoreRow {
                score_id: r.score_id,
                belief_id: r.belief_id,
                rule_id: r.rule_id,
                score: r.score,
                scored_at: r.scored_at,
            })
            .collect())
    }
}

/// Realized-funding ledger ops (design §9.1): the durable record of FINALIZED
/// 8h funding rates from the PUBLIC `GET /margin/funding_rates/historical`.
/// The resolve/score loop reads `realized_rate` to settle a scalar funding
/// belief against ground truth; the poller reads `latest_funding_time` for
/// incremental backfill. INSERT-only at the app layer: a finalized rate never
/// changes, so a re-poll of the same `(market_ticker, funding_time)` is an
/// idempotent no-op (`ON CONFLICT DO NOTHING`) — NOT a mutation, so the
/// append-only `funding_rates_historical_append_only` trigger never fires.
/// UPDATE/DELETE are refused by that trigger.
pub struct FundingRatesHistoricalRepo {
    pool: PgPool,
}

impl FundingRatesHistoricalRepo {
    pub fn new(pool: PgPool) -> FundingRatesHistoricalRepo {
        FundingRatesHistoricalRepo { pool }
    }

    /// Insert one finalized funding rate. `mark_price` is the venue's
    /// per-contract dollar STRING, stored verbatim (no float round-trip).
    /// `Ok(true)` when a row was inserted; `Ok(false)` when the
    /// `(market_ticker, funding_time)` was already recorded (idempotent
    /// re-poll — a finalized rate never changes). The conflict is a no-op at
    /// the row level, so the append-only trigger is never reached.
    pub async fn insert(
        &self,
        market_ticker: &str,
        funding_time: &str,
        funding_rate: f64,
        mark_price: &str,
        captured_at: &str,
    ) -> Result<bool, LedgerError> {
        let result = sqlx::query!(
            r#"INSERT INTO funding_rates_historical
               (market_ticker, funding_time, funding_rate, mark_price, captured_at)
               VALUES ($1,$2,$3,$4,$5)
               ON CONFLICT (market_ticker, funding_time) DO NOTHING"#,
            market_ticker,
            funding_time,
            funding_rate,
            mark_price,
            captured_at
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// The finalized `funding_rate` for one `(market_ticker, funding_time)`, or
    /// `None` if it has not been captured yet. The resolve/score loop calls
    /// this to settle a scalar funding belief.
    pub async fn realized_rate(
        &self,
        market_ticker: &str,
        funding_time: &str,
    ) -> Result<Option<f64>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT funding_rate
               FROM funding_rates_historical
               WHERE market_ticker = $1 AND funding_time = $2"#,
            market_ticker,
            funding_time
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.funding_rate))
    }

    /// The newest captured `funding_time` for one market (the poller's
    /// incremental-backfill cursor), or `None` if the market has no rows yet.
    /// `funding_time` is ISO8601 TEXT that sorts lexically == chronologically,
    /// so `MAX` is the latest boundary.
    pub async fn latest_funding_time(
        &self,
        market_ticker: &str,
    ) -> Result<Option<String>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT MAX(funding_time) AS "latest?"
               FROM funding_rates_historical
               WHERE market_ticker = $1"#,
            market_ticker
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.latest)
    }
}

/// Append-only binary-bus event recorder (I5). Stores JSONL segments that
/// can be replayed for audit. The DB trigger refuses UPDATE and DELETE.
pub struct RecordingsRepo {
    pool: PgPool,
}

impl RecordingsRepo {
    pub fn new(pool: PgPool) -> RecordingsRepo {
        RecordingsRepo { pool }
    }

    /// Append one bus-segment row. `recording_id` is a caller-supplied ULID
    /// (unique per segment); `segment_seq` orders segments within a logical
    /// recording session.
    pub async fn append(
        &self,
        recording_id: &str,
        segment_seq: i64,
        jsonl: &str,
        created_at: &str,
    ) -> Result<(), LedgerError> {
        sqlx::query!(
            r#"INSERT INTO bus_recordings (recording_id, segment_seq, jsonl, created_at)
               VALUES ($1,$2,$3,$4)"#,
            recording_id,
            segment_seq,
            jsonl,
            created_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// ---------- A4: trade scores per (market, strategy) from settled fills ----------

/// Aggregate of a market's fills, consumed by `TradeScoresRepo::insert` to
/// compute `pnl_after_fees_cents`.
#[derive(Debug, Clone)]
pub struct FillAggregate {
    pub strategy: Option<String>,
    pub fees_cents: i64,
    pub n_fills: i64,
    pub maker_fills: i64,
}

/// Per-(market, strategy) trade score: realized PnL NET of basis + fill
/// realism (maker/taker ratio). Append-only (the DB trigger refuses
/// UPDATE/DELETE). One row per settled market+strategy (UNIQUE constraint
/// enforces idempotency under restart/replay).
pub struct TradeScoresRepo {
    pool: PgPool,
}

impl TradeScoresRepo {
    pub fn new(pool: PgPool) -> TradeScoresRepo {
        TradeScoresRepo { pool }
    }

    /// Aggregate fill stats for a market from the `fills` table. Returns a
    /// zero-count aggregate (strategy=None, all counts 0) when no fills exist
    /// for the market — the daemon skips insert when n_fills is 0.
    pub async fn fills_aggregate(&self, market_id: &str) -> Result<FillAggregate, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT MAX(strategy)                                                   AS "strategy?",
                      COALESCE(SUM(fee_cents), 0)::BIGINT                            AS "fees_cents!: i64",
                      COUNT(*)::BIGINT                                               AS "n_fills!: i64",
                      COALESCE(SUM(CASE WHEN is_maker THEN 1 ELSE 0 END), 0)::BIGINT AS "maker_fills!: i64"
               FROM fills WHERE market_id = $1"#,
            market_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(FillAggregate {
            strategy: row.strategy,
            fees_cents: row.fees_cents,
            n_fills: row.n_fills,
            maker_fills: row.maker_fills,
        })
    }

    /// Insert one trade score. Returns `Ok(true)` when inserted, `Ok(false)`
    /// when the UNIQUE (market_id, strategy) constraint fires — idempotent
    /// under restart/replay (mirrors `FillsRepo::insert`). The DB trigger
    /// refuses UPDATE/DELETE (append-only I5).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        &self,
        trade_score_id: &str,
        market_id: &str,
        venue: &str,
        strategy: Option<&str>,
        producer: Option<&str>,
        realized_pnl_cents: i64,
        fees_cents: i64,
        pnl_after_fees_cents: i64,
        n_fills: i64,
        maker_fills: i64,
        settled_at: &str,
        scored_at: &str,
    ) -> Result<bool, LedgerError> {
        let result = sqlx::query!(
            r#"INSERT INTO trade_scores
               (trade_score_id, market_id, venue, strategy, producer,
                realized_pnl_cents, fees_cents, pnl_after_fees_cents,
                n_fills, maker_fills, settled_at, scored_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
               ON CONFLICT (market_id, strategy) DO NOTHING"#,
            trade_score_id,
            market_id,
            venue,
            strategy,
            producer,
            realized_pnl_cents,
            fees_cents,
            pnl_after_fees_cents,
            n_fills,
            maker_fills,
            settled_at,
            scored_at
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

/// Append-only snapshot store for the pure `fortuna_scoring::Scorecard` (WS2 S6b;
/// plan Task 6 Step 5). One IMMUTABLE row per recompute — a recompute is a NEW
/// row, never an edit (the DB trigger refuses UPDATE/DELETE, I5). The whole
/// Scorecard rides in the JSONB `payload`, so the schema is source-agnostic and
/// WS3 reuses it unchanged.
pub struct ScorecardsRepo {
    pool: PgPool,
}

impl ScorecardsRepo {
    pub fn new(pool: PgPool) -> ScorecardsRepo {
        ScorecardsRepo { pool }
    }

    /// Insert one Scorecard snapshot at `computed_at`. INSERT-only; idempotent on
    /// the `(scope, producer, window, computed_at)` UNIQUE key (a duplicate
    /// recompute at the same instant is a no-op, not an error). `scope`,
    /// `producer`, and `window` are read off the card itself so the row's columns
    /// always agree with the persisted payload.
    pub async fn insert_scorecard(
        &self,
        id: &str,
        card: &Scorecard,
        computed_at: &str,
    ) -> Result<(), LedgerError> {
        let payload = serde_json::to_value(card)?;
        sqlx::query!(
            r#"INSERT INTO scorecards (id, scope, producer, "window", computed_at, payload)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT (scope, producer, "window", computed_at) DO NOTHING"#,
            id,
            card.scope,
            card.producer.as_deref(),
            card.window,
            computed_at,
            payload,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The newest Scorecard for a `(scope, producer, window)` triple (by
    /// `computed_at` DESC), or `None` when none exists. `producer = None` selects
    /// the merged-scope bucket (the persisted NULL), distinct from any
    /// producer-attributed row — `IS NOT DISTINCT FROM` matches NULL to NULL and a
    /// value to that value. The JSONB payload deserializes back to the exact
    /// Scorecard (a corrupt payload surfaces as `CorruptRow`, never a panic).
    pub async fn latest_scorecard(
        &self,
        scope: &str,
        producer: Option<&str>,
        window: &str,
    ) -> Result<Option<Scorecard>, LedgerError> {
        let row = sqlx::query!(
            r#"SELECT payload AS "payload!: serde_json::Value"
               FROM scorecards
               WHERE scope = $1
                 AND producer IS NOT DISTINCT FROM $2
                 AND "window" = $3
               ORDER BY computed_at DESC
               LIMIT 1"#,
            scope,
            producer,
            window,
        )
        .fetch_optional(&self.pool)
        .await?;
        match row {
            None => Ok(None),
            Some(r) => {
                let card: Scorecard =
                    serde_json::from_value(r.payload).map_err(|e| LedgerError::CorruptRow {
                        table: "scorecards",
                        reason: format!("payload is not a Scorecard: {e}"),
                    })?;
                Ok(Some(card))
            }
        }
    }
}
