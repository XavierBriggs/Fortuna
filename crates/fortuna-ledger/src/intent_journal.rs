//! Postgres-backed `IntentJournal` (spec 5.4): the durable intent journal.
//!
//! `append` returns only after the INSERT commits — that is the "persisted
//! BEFORE any network call" guarantee the order manager builds on. Rows are
//! INSERT-only (database triggers enforce it); the fold lives in
//! fortuna-exec and is identical for memory and Postgres journals.

use crate::LedgerError;
use async_trait::async_trait;
use fortuna_core::clock::Clock;
use fortuna_core::ids::IntentId;
use fortuna_exec::{ExecError, IntentEvent, IntentJournal, JournalRow};
use fortuna_venues::Cursor;
use sqlx::PgPool;
use std::sync::Arc;

/// One journal per (process, venue): the cursor checkpoint is venue-scoped.
pub struct PgIntentJournal {
    pool: PgPool,
    venue: String,
    clock: Arc<dyn Clock>,
}

impl PgIntentJournal {
    pub fn new(pool: PgPool, venue: impl Into<String>, clock: Arc<dyn Clock>) -> PgIntentJournal {
        PgIntentJournal {
            pool,
            venue: venue.into(),
            clock,
        }
    }
}

fn exec_err(e: impl std::fmt::Display) -> ExecError {
    ExecError::Journal {
        reason: e.to_string(),
    }
}

#[async_trait]
impl IntentJournal for PgIntentJournal {
    async fn append(&mut self, intent: IntentId, event: IntentEvent) -> Result<(), ExecError> {
        let payload = serde_json::to_value(&event).map_err(exec_err)?;
        sqlx::query!(
            r#"INSERT INTO intent_events (intent_id, event, at) VALUES ($1, $2, $3)"#,
            intent.to_string(),
            payload,
            event.at().to_iso8601()
        )
        .execute(&self.pool)
        .await
        .map_err(exec_err)?;
        Ok(())
    }

    async fn load_all(&self) -> Result<Vec<JournalRow>, ExecError> {
        let rows = sqlx::query!(r#"SELECT seq, intent_id, event FROM intent_events ORDER BY seq"#)
            .fetch_all(&self.pool)
            .await
            .map_err(exec_err)?;
        rows.into_iter()
            .map(|r| {
                let intent: IntentId = r.intent_id.parse().map_err(exec_err)?;
                let event: IntentEvent = serde_json::from_value(r.event).map_err(exec_err)?;
                Ok(JournalRow {
                    seq: r.seq as u64,
                    intent,
                    event,
                })
            })
            .collect()
    }

    async fn cursor(&self) -> Result<Cursor, ExecError> {
        let row = sqlx::query!(
            r#"SELECT cursor FROM exec_cursors WHERE venue = $1"#,
            self.venue
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(exec_err)?;
        Ok(row.map(|r| Cursor(r.cursor)).unwrap_or_else(Cursor::start))
    }

    async fn set_cursor(&mut self, cursor: Cursor) -> Result<(), ExecError> {
        // The single mutable checkpoint table (derived state, not history).
        sqlx::query!(
            r#"INSERT INTO exec_cursors (venue, cursor, updated_at)
               VALUES ($1, $2, $3)
               ON CONFLICT (venue) DO UPDATE
               SET cursor = EXCLUDED.cursor, updated_at = EXCLUDED.updated_at"#,
            self.venue,
            cursor.0,
            self.clock.now().to_iso8601()
        )
        .execute(&self.pool)
        .await
        .map_err(exec_err)?;
        Ok(())
    }
}

/// Keep LedgerError convertible for ledger-internal callers.
impl From<LedgerError> for ExecError {
    fn from(e: LedgerError) -> Self {
        exec_err(e)
    }
}
