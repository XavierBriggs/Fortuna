//! fortuna-ledger: all Postgres persistence. Spec 5.5, 5.13, Section 7. I5.
//!
//! Tables: beliefs (FK event_id; immutable, superseding rows), events,
//! market_event_edges, journal, lessons, audit (append-only; WRITE FAILURE
//! HALTS TRADING), orders/fills, markets, signals, calibration_params,
//! intents, settlements, discrepancies, price_snapshots, source_registry,
//! reservations. sqlx with migrations in ./migrations (one per schema task).
//! Scoring jobs: Brier vs canonical outcome; CLV vs benchmark snapshot (never
//! settlement; liquidity-filtered; spec 5.5 CLV definition).
