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
