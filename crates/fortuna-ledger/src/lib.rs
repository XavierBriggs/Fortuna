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

pub use audit::{AuditRow, AuditWriter, LatestAudit};
pub use intent_journal::PgIntentJournal;
pub use repos::{
    halt_scope_string, parse_halt_scope, BeliefPanelRow, BeliefRow, BeliefScoreRow,
    BeliefScoresRepo, BeliefsRepo, CalibrationParamsRepo, CalibrationParamsRow,
    CalibrationScopeRow, DiscrepanciesRepo, DomainAnalysesRepo, DomainAnalysisRow, EdgeRow,
    EdgesRepo, EventRow, EventSourceEvidenceInput, EventSourceEvidenceRepo, EventsRepo, FillsRepo,
    FundingRatesHistoricalRepo, HaltsRepo, JournalRepo, JournalRow, LessonRow, LessonsRepo,
    OpenWeatherBelief, PersonaRow, PersonasRepo, RecentSignalRow, RecordingsRepo, ReservationsRepo,
    ResolvedPersonaStats, ResolvedStat, ScalarBeliefRow, ScalarBeliefsRepo, SettlementEntryRow,
    SettlementsRepo, SignalsRepo, SnapshotRow, SnapshotsRepo, SourceRegistryRepo,
    SourceRegistryRow, TradabilityRepo, TradabilityRow,
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

/// A small, ISOLATED read pool for the operator dashboard (ROTA, design R5).
/// It is SEPARATE from the daemon's writer pool BY DESIGN: audit-append failure
/// is a GLOBAL HALT ("no audit, no trading"), so dashboard load must never be
/// able to queue against the audit writer's connections. Bounded to two
/// connections; a short `acquire_timeout` so a saturated pool renders the panel
/// DEGRADED (the handler's error path), never hung; a `statement_timeout` on
/// every connection so a slow read cannot pin a connection. NO migrations — the
/// writer pool owns schema; this handle only reads.
pub async fn connect_readonly_pool(database_url: &str) -> Result<PgPool, LedgerError> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                // 3s ceiling on any dashboard read (R5).
                sqlx::query("SET statement_timeout = 3000")
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await?;
    Ok(pool)
}
