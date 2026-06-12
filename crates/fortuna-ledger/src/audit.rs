//! The append-only audit writer (I5).
//!
//! Every model call, belief, proposal, gate decision, order, fill, config
//! change, halt, and kill-switch test lands here. Rows are never updated or
//! deleted — the repo is INSERT-only and the database triggers reject
//! mutation outright.
//!
//! THE I5 CONTRACT: an `Err` from `append` means trading halts. The writer
//! cannot halt anything itself (it owns no gates); the runner wires
//! `append`-failure => `GatePipeline::set_halt(Global, ...)` and the DST
//! asserts it (T0.10). Callers must never swallow these errors.

use crate::LedgerError;
use fortuna_core::clock::{Clock, UtcTimestamp};
use fortuna_core::ids::{AuditId, IdGen};
use sqlx::PgPool;
use std::sync::{Arc, Mutex, PoisonError};

/// The newest audit row's timestamp + kind (the A8 status crash-tell).
#[derive(Debug, Clone)]
pub struct LatestAudit {
    pub at: String,
    pub kind: String,
}

/// One audit record as read back.
#[derive(Debug, Clone)]
pub struct AuditRow {
    pub audit_id: String,
    pub at: UtcTimestamp,
    pub kind: String,
    pub actor: Option<String>,
    pub ref_id: Option<String>,
    pub payload: serde_json::Value,
}

/// INSERT-only audit writer. One per process.
pub struct AuditWriter {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    ids: Mutex<IdGen>,
}

impl AuditWriter {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, id_seed: u64) -> AuditWriter {
        AuditWriter {
            pool,
            clock,
            ids: Mutex::new(IdGen::new(id_seed)),
        }
    }

    /// Append one record. `Err` => the caller HALTS trading (I5).
    pub async fn append(
        &self,
        kind: &str,
        actor: Option<&str>,
        ref_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<AuditId, LedgerError> {
        let at = self.clock.now();
        let id = {
            let mut ids = self.ids.lock().unwrap_or_else(PoisonError::into_inner);
            AuditId::new(ids.next(at)?)
        };
        sqlx::query!(
            r#"INSERT INTO audit (audit_id, at, kind, actor, ref_id, payload)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
            id.to_string(),
            at.to_iso8601(),
            kind,
            actor,
            ref_id,
            payload
        )
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    /// The newest audit row of ANY kind (ULID order == insertion order);
    /// None on an empty table. T4.4 A8: `fortuna status` renders its age —
    /// a stale age beside a live daemon pidfile is the crash tell. Kind-
    /// agnostic by design: a kind-filtered variant would read a healthy
    /// daemon writing only cognition/veto rows as stale (a false crash
    /// tell is worse than none).
    pub async fn latest_at(&self) -> Result<Option<LatestAudit>, LedgerError> {
        let row = sqlx::query!(r#"SELECT at, kind FROM audit ORDER BY audit_id DESC LIMIT 1"#)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| LatestAudit {
            at: r.at,
            kind: r.kind,
        }))
    }

    /// Most recent records of a kind (audit query tooling; newest first).
    pub async fn recent(&self, kind: &str, limit: i64) -> Result<Vec<AuditRow>, LedgerError> {
        let rows = sqlx::query!(
            r#"SELECT audit_id, at, kind, actor, ref_id, payload
               FROM audit WHERE kind = $1
               ORDER BY at DESC, audit_id DESC LIMIT $2"#,
            kind,
            limit
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                Ok(AuditRow {
                    audit_id: r.audit_id,
                    at: UtcTimestamp::parse_iso8601(&r.at).map_err(|e| {
                        LedgerError::CorruptRow {
                            table: "audit",
                            reason: e.to_string(),
                        }
                    })?,
                    kind: r.kind,
                    actor: r.actor,
                    ref_id: r.ref_id,
                    payload: r.payload,
                })
            })
            .collect()
    }
}
