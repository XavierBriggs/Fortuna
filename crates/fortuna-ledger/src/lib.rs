//! fortuna-ledger: all Postgres persistence. Spec 5.5, 5.13, Section 7. I5.
//!
//! Tables: beliefs (FK event_id; immutable, superseding rows), events,
//! market_event_edges, journal, lessons, audit (append-only; WRITE FAILURE
//! HALTS TRADING), orders/fills, markets, signals, calibration_params,
//! intents, settlements, discrepancies, price_snapshots, source_registry,
//! reservations. sqlx with migrations in ./migrations (one per schema task).
//! Scoring jobs: Brier vs canonical outcome; CLV vs benchmark snapshot (never
//! settlement; liquidity-filtered; spec 5.5 CLV definition).
//!
//! Append-only is enforced twice: INSERT-only repos here AND database
//! triggers that reject UPDATE/DELETE outright (see the migration). The
//! kill-switch process uses NONE of this crate (spec Principle 9 exception).

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

mod audit;
mod intent_journal;
mod repos;

pub use audit::{AuditRow, AuditWriter};
pub use intent_journal::PgIntentJournal;
pub use repos::{
    halt_scope_string, parse_halt_scope, BeliefRow, BeliefsRepo, CalibrationParamsRepo,
    CalibrationParamsRow, DiscrepanciesRepo, EdgeRow, EdgesRepo, EventRow, EventsRepo, FillsRepo,
    HaltsRepo, JournalRepo, JournalRow, LessonRow, LessonsRepo, ReservationsRepo, ResolvedStat,
    SettlementEntryRow, SettlementsRepo, SignalsRepo, SnapshotRow, SnapshotsRepo,
    SourceRegistryRepo, SourceRegistryRow, TradabilityRepo, TradabilityRow,
};

pub use sqlx::PgPool;
use thiserror::Error;

/// Ledger persistence errors. CONTRACT (I5): the runner treats any error
/// from the audit writer as a trading halt — no audit, no trading.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error("migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("corrupt row in {table}: {reason}")]
    CorruptRow { table: &'static str, reason: String },
    #[error("id generation failed: {0}")]
    Id(#[from] fortuna_core::ids::IdError),
}

/// Embedded migrations (one per schema-touching BUILD_PLAN task).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Connect and migrate. The pool is the one shared handle.
pub async fn connect(database_url: &str) -> Result<PgPool, LedgerError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(database_url)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}
