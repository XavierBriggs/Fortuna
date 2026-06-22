//! `AeolusArchiveSource` — the ONLY source-coupled adapter in the crate (S6).
//!
//! Maps the FIRMED real Aeolus archive (`aeolus_kalshi.db`, a SQLite database)
//! onto the generic [`crate::source::HistoricalSource`] contracts. The
//! forecast-pipeline `aeolus.db` `scorecards` table is **post-resolution** and
//! is deliberately **NOT** imported (see "the leak trap" below).
//!
//! ## The post-resolution-leak trap (spec §9 — this adapter's #1 G-PIT risk)
//!
//! Aeolus was built to *trade*, not as a bitemporal research archive, so it
//! mixes pre-decision and post-resolution data in the same database. The
//! mapping below is the load-bearing correctness boundary:
//!
//! - **Beliefs come from `bracket_probability_log`.** A belief's `available_at`
//!   is the forecast-**issuance** instant (`forecast_init_time`, the knowledge
//!   time). It is **NEVER** `target_date` (the event day) and **NEVER**
//!   `market_resolutions.settled_at` (the resolution instant). Mapping
//!   `available_at` to either of those is the silent look-ahead leak the G-PIT
//!   gate cannot detect for us — the correctness belongs here.
//! - **The belief payload is `predicted_prob` only** — the issuance-time
//!   probability. No realized score (CRPS / PIT / `absolute_error` from
//!   `aeolus.db`'s `scorecards`) flows into any belief payload. Those are
//!   outcome-side, post-resolution quantities; FORTUNA **recomputes** scores
//!   through its own `fortuna-scoring` (this is what keeps G-PARITY honest — we
//!   never import Aeolus's precomputed scores).
//! - **Outcomes come from `market_resolutions`.** An outcome's `resolved_at` is
//!   `settled_at` (resolution time). It may *label*, never *decide*.
//!
//! ## Bounded-memory streaming
//!
//! Each accessor returns a paged cursor ([`PagedRowStream`]) that fetches one
//! fixed-size page from SQLite at a time and yields rows lazily. The whole
//! archive is never materialized in memory; a single page (a few hundred rows)
//! is the working-set bound. This is a safe, dependency-free streaming pattern
//! (no `unsafe`, no self-referential structs): each page prepares a short-lived
//! statement, fully drains it into the page buffer, and drops it before the
//! next page.
//!
//! ## Source-name literals
//!
//! This file legitimately names the concrete producer/venue (the decoupling
//! grep gate excludes `src/sources/`). Those literals must NOT appear anywhere
//! else under `crates/fortuna-backtest/src/`.

use std::path::{Path, PathBuf};

use fortuna_core::clock::UtcTimestamp;
use fortuna_core::money::Cents;
use rusqlite::{Connection, OpenFlags};

use crate::manifest::{EngagedMarket, UniverseManifest};
use crate::records::{
    BeliefPayload, HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, HistoricalTrade,
    Provenance,
};
use crate::source::{HistoricalSource, SourceError};

/// Producer-type tag stamped onto every mapped record's provenance.
const PRODUCER_TYPE: &str = "aeolus";
/// Resolution-source tag for outcomes (the venue that settled the market).
const RESOLUTION_SOURCE: &str = "kalshi";
/// Provenance producer-id / import marker (knowledge-time preserved; this row
/// never masquerades as a live decision).
const IMPORT_MARKER: &str = "historical-import";
/// The forecast category these bracket beliefs belong to.
const CATEGORY: &str = "forecast";

/// Page size for the bounded-memory cursor (rows fetched per SQLite round-trip).
const PAGE_SIZE: i64 = 256;

// ---------------------------------------------------------------------------
// TimeRange
// ---------------------------------------------------------------------------

/// An optional `[from, to]` knowledge-time window applied to mapped records.
///
/// Bounds are inclusive and compared against the row's knowledge-time instant
/// (issuance for beliefs, resolution for outcomes, capture for snapshots,
/// creation for trades). `unbounded()` admits every row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRange {
    pub from: Option<UtcTimestamp>,
    pub to: Option<UtcTimestamp>,
}

impl TimeRange {
    /// A range that admits every row.
    pub fn unbounded() -> Self {
        Self {
            from: None,
            to: None,
        }
    }

    /// `true` iff `at` falls within the (inclusive) window.
    fn admits(&self, at: UtcTimestamp) -> bool {
        if let Some(from) = self.from {
            if at < from {
                return false;
            }
        }
        if let Some(to) = self.to {
            if at > to {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// AeolusArchiveSource
// ---------------------------------------------------------------------------

/// A [`HistoricalSource`] backed by the Aeolus `aeolus_kalshi.db` SQLite
/// archive.
///
/// All belief / outcome / snapshot / trade / manifest records come from
/// `kalshi_db`. `aeolus_db` is optional and its `scorecards` are **NOT**
/// imported (the leak trap, above); it is carried only so a future, explicitly
/// outcome-side consumer could read it — never the belief side.
pub struct AeolusArchiveSource {
    conn: Connection,
    kalshi_db: PathBuf,
    aeolus_db: Option<PathBuf>,
    range: TimeRange,
}

impl AeolusArchiveSource {
    /// Open an archive at `kalshi_db` (the `aeolus_kalshi.db` file), with an
    /// optional `aeolus_db` (NEVER imported as beliefs) and a knowledge-time
    /// window.
    pub fn open(
        kalshi_db: PathBuf,
        aeolus_db: Option<PathBuf>,
        range: TimeRange,
    ) -> Result<Self, SourceError> {
        let conn = Connection::open(&kalshi_db).map_err(|e| SourceError::Io {
            reason: format!("opening archive: {e}"),
        })?;
        Ok(Self {
            conn,
            kalshi_db,
            aeolus_db,
            range,
        })
    }

    /// Build a source from a committed `.sql` fixture loaded into an in-memory
    /// database. Test-only convenience; never used against the live archive.
    pub fn from_sql_fixture(sql_path: &Path, range: TimeRange) -> Result<Self, SourceError> {
        let sql = std::fs::read_to_string(sql_path).map_err(|e| SourceError::Io {
            reason: format!("reading fixture {}: {e}", sql_path.display()),
        })?;
        let conn = Connection::open_in_memory().map_err(|e| SourceError::Io {
            reason: format!("opening in-memory fixture: {e}"),
        })?;
        conn.execute_batch(&sql)
            .map_err(|e| SourceError::Malformed {
                reason: format!("loading fixture SQL: {e}"),
            })?;
        Ok(Self {
            conn,
            kalshi_db: sql_path.to_path_buf(),
            aeolus_db: None,
            range,
        })
    }

    /// Open an archive read-only (`SQLITE_OPEN_READ_ONLY`). This is the
    /// paper-safe path the CLI uses — spec §10 prohibits any write to the
    /// source archive. The connection cannot create or modify tables; any
    /// accidental write attempt from this `AeolusArchiveSource` will error
    /// rather than silently modify the source DB.
    pub fn open_read_only(kalshi_db: PathBuf, range: TimeRange) -> Result<Self, SourceError> {
        let conn = Connection::open_with_flags(
            &kalshi_db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| SourceError::Io {
            reason: format!("opening archive read-only: {e}"),
        })?;
        Ok(Self {
            conn,
            kalshi_db,
            aeolus_db: None,
            range,
        })
    }

    /// The `aeolus_kalshi.db` path backing this source.
    pub fn kalshi_db(&self) -> &Path {
        &self.kalshi_db
    }

    /// The optional forecast-pipeline `aeolus.db` path. Its `scorecards` are
    /// NOT imported (the leak trap); this accessor exists for outcome-side
    /// tooling only.
    pub fn aeolus_db(&self) -> Option<&Path> {
        self.aeolus_db.as_deref()
    }
}

// ---------------------------------------------------------------------------
// Canonical event_linkage
// ---------------------------------------------------------------------------

/// Build the canonical cross-producer join key from the native columns.
///
/// Shape (see `source.rs`): `event://<category>/station-<ST>/bracket-<lo>-<hi>/<target>`.
/// The market ticker is appended so the key is unique per Kalshi market and so
/// the ticker remains recoverable for reconciliation.
fn event_linkage(
    station_id: &str,
    bracket_lo: Option<i64>,
    bracket_hi: Option<i64>,
    target_date: &str,
    market_ticker: &str,
) -> String {
    let lo = bracket_lo
        .map(|v| v.to_string())
        .unwrap_or_else(|| "x".into());
    let hi = bracket_hi
        .map(|v| v.to_string())
        .unwrap_or_else(|| "x".into());
    format!(
        "event://{CATEGORY}/station-{station_id}/bracket-{lo}-{hi}/{target_date}/{market_ticker}"
    )
}

// ---------------------------------------------------------------------------
// Timestamp parsing helpers
// ---------------------------------------------------------------------------

/// Parse a stored TEXT timestamp into a [`UtcTimestamp`]. All time is derived
/// from the stored value — never `SystemTime::now()`.
fn parse_ts(raw: &str) -> Result<UtcTimestamp, SourceError> {
    UtcTimestamp::parse_iso8601(raw)
        .or_else(|_| UtcTimestamp::parse_iso8601_or_date(raw))
        .map_err(|e| SourceError::Malformed {
            reason: format!("timestamp {raw:?}: {e}"),
        })
}

// ---------------------------------------------------------------------------
// PagedRowStream — bounded-memory cursor
// ---------------------------------------------------------------------------

/// A lazy, bounded-memory cursor over a SQL query.
///
/// Holds a borrow of the source's [`Connection`] plus one in-flight page of
/// mapped rows. When the page is exhausted it fetches the next `PAGE_SIZE`
/// rows via a fresh short-lived statement (`LIMIT ? OFFSET ?`). The whole
/// result set is never materialized; the working-set bound is one page.
///
/// Errors are surfaced per row: a failed page fetch yields a single
/// `Err(SourceError)` and then ends the stream.
pub struct PagedRowStream<'a, T> {
    conn: &'a Connection,
    base_sql: String,
    map: fn(&rusqlite::Row<'_>) -> Result<T, SourceError>,
    offset: i64,
    buffer: std::vec::IntoIter<Result<T, SourceError>>,
    done: bool,
}

impl<'a, T> PagedRowStream<'a, T> {
    fn new(
        conn: &'a Connection,
        base_sql: impl Into<String>,
        map: fn(&rusqlite::Row<'_>) -> Result<T, SourceError>,
    ) -> Self {
        Self {
            conn,
            base_sql: base_sql.into(),
            map,
            offset: 0,
            buffer: Vec::new().into_iter(),
            done: false,
        }
    }

    /// Fetch the next page into `buffer`. Returns `false` when the source is
    /// exhausted (a short page) or on error (the error is pushed into the
    /// buffer and the stream is marked done).
    fn fetch_page(&mut self) -> bool {
        if self.done {
            return false;
        }
        let paged = format!(
            "{} LIMIT {} OFFSET {}",
            self.base_sql, PAGE_SIZE, self.offset
        );
        let mut stmt = match self.conn.prepare(&paged) {
            Ok(s) => s,
            Err(e) => {
                self.done = true;
                self.buffer = vec![Err(SourceError::Malformed {
                    reason: format!("preparing page: {e}"),
                })]
                .into_iter();
                return true;
            }
        };
        let map = self.map;
        let rows = stmt.query_map([], |row| Ok(map(row)));
        let collected: Vec<Result<T, SourceError>> = match rows {
            Ok(iter) => {
                let mut out = Vec::new();
                for r in iter {
                    match r {
                        Ok(mapped) => out.push(mapped),
                        Err(e) => out.push(Err(SourceError::Malformed {
                            reason: format!("reading row: {e}"),
                        })),
                    }
                }
                out
            }
            Err(e) => {
                self.done = true;
                vec![Err(SourceError::Malformed {
                    reason: format!("querying page: {e}"),
                })]
            }
        };
        let n = collected.len() as i64;
        // A short page (fewer than PAGE_SIZE rows) means the table is exhausted.
        if n < PAGE_SIZE {
            self.done = true;
        }
        self.offset += n;
        self.buffer = collected.into_iter();
        true
    }
}

impl<T> Iterator for PagedRowStream<'_, T> {
    type Item = Result<T, SourceError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.buffer.next() {
                return Some(item);
            }
            if self.done {
                return None;
            }
            if !self.fetch_page() {
                return None;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Row -> record mappers
// ---------------------------------------------------------------------------

fn map_belief(row: &rusqlite::Row<'_>) -> Result<HistoricalBelief, SourceError> {
    let station_id: String = column(row, 0, "station_id")?;
    let target_date: String = column(row, 1, "target_date")?;
    let forecast_init_time: String = column(row, 2, "forecast_init_time")?;
    let market_ticker: String = column(row, 3, "market_ticker")?;
    let bracket_lo: Option<i64> = column(row, 4, "bracket_lo")?;
    let bracket_hi: Option<i64> = column(row, 5, "bracket_hi")?;
    let predicted_prob: f64 = column(row, 6, "predicted_prob")?;

    // THE TRAP: available_at is the forecast ISSUANCE instant — never target,
    // never resolution.
    let available_at = parse_ts(&forecast_init_time)?;
    // decided_at is the event horizon (strictly after issuance, before
    // settlement): the harness G-PIT rule admits a belief iff
    // available_at < decided_at.
    let decided_at = parse_ts(&target_date)?;

    let linkage = event_linkage(
        &station_id,
        bracket_lo,
        bracket_hi,
        &target_date,
        &market_ticker,
    );

    Ok(HistoricalBelief {
        provenance: Provenance {
            producer_type: PRODUCER_TYPE.to_string(),
            producer_id: IMPORT_MARKER.to_string(),
            mind_id: None,
            mind_version: None,
            strategy_id: market_ticker,
            category: CATEGORY.to_string(),
            scope: format!("{CATEGORY}:{station_id}"),
        },
        // The payload is the ISSUANCE-time probability ONLY. No realized score
        // (crps/pit/absolute_error) ever flows in here.
        payload: BeliefPayload::Binary { p: predicted_prob },
        event_linkage: linkage,
        available_at,
        decided_at,
    })
}

fn map_outcome(row: &rusqlite::Row<'_>) -> Result<HistoricalOutcome, SourceError> {
    let station_id: String = column(row, 0, "station_id")?;
    let target_date: String = column(row, 1, "target_date")?;
    let market_ticker: String = column(row, 2, "market_ticker")?;
    let bracket_lo: Option<i64> = column(row, 3, "bracket_lo")?;
    let bracket_hi: Option<i64> = column(row, 4, "bracket_hi")?;
    let result: String = column(row, 5, "result")?;
    let settled_at: String = column(row, 6, "settled_at")?;

    let outcome = match result.as_str() {
        "yes" => 1.0,
        "no" => 0.0,
        other => {
            return Err(SourceError::Malformed {
                reason: format!("non-binary result {other:?} must be filtered before mapping"),
            })
        }
    };

    Ok(HistoricalOutcome {
        event_linkage: event_linkage(
            &station_id,
            bracket_lo,
            bracket_hi,
            &target_date,
            &market_ticker,
        ),
        outcome,
        // An outcome's knowledge time IS the resolution instant.
        resolved_at: parse_ts(&settled_at)?,
        resolution_source: RESOLUTION_SOURCE.to_string(),
    })
}

fn map_snapshot(row: &rusqlite::Row<'_>) -> Result<HistoricalSnapshot, SourceError> {
    let station_id: String = column(row, 0, "station_id")?;
    let target_date: String = column(row, 1, "target_date")?;
    let market_ticker: String = column(row, 2, "market_ticker")?;
    let bracket_lo: Option<i64> = column(row, 3, "bracket_lo")?;
    let bracket_hi: Option<i64> = column(row, 4, "bracket_hi")?;
    let captured_at: String = column(row, 5, "captured_at")?;
    let yes_mid_cents: f64 = column(row, 6, "yes_mid_cents")?;

    Ok(HistoricalSnapshot {
        // THE JOIN KEY: `market` MUST be the SAME canonical composite linkage the
        // belief/outcome emit for this market_ticker — NOT the bare ticker. The
        // as-of join matches `snapshot.market == belief.event_linkage`, so a bare
        // ticker silently drops every snapshot (the namespace-drift failure
        // source.rs warns about). The bracket/station/target are recovered from
        // bracket_probability_log (snapshot_quotes lacks them) the SAME way the
        // outcome mapping does, and routed through the SAME `event_linkage` helper
        // so the key can never drift from the belief side.
        market: event_linkage(
            &station_id,
            bracket_lo,
            bracket_hi,
            &target_date,
            &market_ticker,
        ),
        // yes_mid_cents is a REAL cent value; round to the nearest integer cent.
        price: Cents::new(yes_mid_cents.round() as i64),
        at: parse_ts(&captured_at)?,
    })
}

fn map_trade(row: &rusqlite::Row<'_>) -> Result<HistoricalTrade, SourceError> {
    let station_id: String = column(row, 0, "station_id")?;
    let target_date: String = column(row, 1, "target_date")?;
    let market_ticker: String = column(row, 2, "market_ticker")?;
    let side: String = column(row, 3, "side")?;
    let reference_price_cents: i64 = column(row, 4, "reference_price_cents")?;
    let contracts: i64 = column(row, 5, "contracts")?;
    let created_at: String = column(row, 6, "created_at")?;

    let contracts_u32 = u32::try_from(contracts).map_err(|_| SourceError::Malformed {
        reason: format!("contracts {contracts} out of range"),
    })?;

    // shadow_intents are SHADOW/paper intents — no real order was ever placed,
    // so orders is invariant-0 (the HistoricalTrade::new constructor enforces
    // this; a nonzero value is rejected).
    HistoricalTrade::new(
        event_linkage(&station_id, None, None, &target_date, &market_ticker),
        side,
        Cents::new(reference_price_cents),
        contracts_u32,
        parse_ts(&created_at)?,
        0,
    )
    .map_err(|e| SourceError::Malformed {
        reason: format!("trade construction: {e}"),
    })
}

/// Read a typed column, mapping any rusqlite error into a `SourceError`.
fn column<T: rusqlite::types::FromSql>(
    row: &rusqlite::Row<'_>,
    idx: usize,
    name: &str,
) -> Result<T, SourceError> {
    row.get::<usize, T>(idx)
        .map_err(|e| SourceError::Malformed {
            reason: format!("column {name}: {e}"),
        })
}

// ---------------------------------------------------------------------------
// HistoricalSource impl
// ---------------------------------------------------------------------------

impl HistoricalSource for AeolusArchiveSource {
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_> {
        // TOTAL deterministic order for stable OFFSET paging + replay. The
        // full PRIMARY KEY of bracket_probability_log is
        // (station_id, target_date, forecast_init_time, market_ticker, side), so
        // appending station_id, target_date makes the ORDER BY a TOTAL order — no
        // duplicates or gaps across page boundaries even when many rows share a
        // forecast_init_time.
        let sql = "SELECT station_id, target_date, forecast_init_time, market_ticker, \
                   bracket_lo, bracket_hi, predicted_prob \
                   FROM bracket_probability_log \
                   ORDER BY forecast_init_time, market_ticker, side, station_id, target_date";
        let range = self.range.clone();
        Box::new(
            PagedRowStream::new(&self.conn, sql, map_belief).filter(move |r| match r {
                Ok(b) => range.admits(b.available_at),
                Err(_) => true,
            }),
        )
    }

    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_> {
        // Join the log (for station/bracket/target → canonical linkage) to
        // resolutions. VOIDED rows (result NOT IN ('yes','no')) are filtered
        // out here: a voided market has no numeric outcome label. DISTINCT
        // collapses the per-side belief rows to one outcome per market.
        let sql = "SELECT DISTINCT b.station_id, b.target_date, b.market_ticker, \
                   b.bracket_lo, b.bracket_hi, r.result, r.settled_at \
                   FROM bracket_probability_log b \
                   JOIN market_resolutions r ON r.market_ticker = b.market_ticker \
                   WHERE r.result IN ('yes','no') \
                   ORDER BY b.market_ticker";
        // ORDER BY b.market_ticker is already TOTAL: market_ticker is the PK of
        // market_resolutions and the DISTINCT/1:1 join collapses to exactly one
        // output row per resolved market — paging is duplicate-/gap-free.
        let range = self.range.clone();
        Box::new(
            PagedRowStream::new(&self.conn, sql, map_outcome).filter(move |r| match r {
                Ok(o) => range.admits(o.resolved_at),
                Err(_) => true,
            }),
        )
    }

    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_> {
        // snapshot_quotes has no timestamp of its own — the snapshot instant is
        // snapshot_batches.captured_at (joined on batch_id). snapshot_quotes also
        // lacks bracket columns, so the canonical `event_linkage` (station +
        // bracket + target) is recovered by JOINing bracket_probability_log on
        // market_ticker — the SAME recovery the outcome mapping uses — so the
        // snapshot's join key matches the belief's exactly. DISTINCT collapses the
        // per-side belief rows to one snapshot per (batch, market).
        //
        // ORDER BY is TOTAL (captured_at, market_ticker, batch_id): batch_id is
        // the snapshot_batches PK and disambiguates batches that share a
        // captured_at, so OFFSET paging is duplicate-/gap-free across page
        // boundaries.
        let sql = "SELECT DISTINCT b.station_id, b.target_date, q.market_ticker, \
                   b.bracket_lo, b.bracket_hi, bt.captured_at, q.yes_mid_cents \
                   FROM snapshot_quotes q \
                   JOIN snapshot_batches bt ON bt.batch_id = q.batch_id \
                   JOIN bracket_probability_log b ON b.market_ticker = q.market_ticker \
                   ORDER BY bt.captured_at, q.market_ticker, bt.batch_id";
        let range = self.range.clone();
        Box::new(
            PagedRowStream::new(&self.conn, sql, map_snapshot).filter(move |r| match r {
                Ok(s) => range.admits(s.at),
                Err(_) => true,
            }),
        )
    }

    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_> {
        // ORDER BY created_at, id is TOTAL: id is the shadow_intents PK, so the
        // order is unique across rows sharing a created_at — OFFSET paging is
        // duplicate-/gap-free. Trades key the as-of join only by ticker token
        // (shadow_intents has no bracket columns); the linkage here recovers
        // station/target/ticker via the same `event_linkage` helper with absent
        // bracket bounds, so the ticker token always reconciles.
        let sql = "SELECT station_id, target_date, market_ticker, side, \
                   reference_price_cents, contracts, created_at \
                   FROM shadow_intents \
                   ORDER BY created_at, id";
        let range = self.range.clone();
        Box::new(
            PagedRowStream::new(&self.conn, sql, map_trade).filter(move |r| match r {
                Ok(t) => range.admits(t.at),
                Err(_) => true,
            }),
        )
    }

    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError> {
        // The full engaged set: every market_ticker the producer logged a belief
        // on, LEFT JOINed to its resolution. VOIDED markets are INCLUDED
        // (voided=true, resolved=false) — they are the canonical survivorship
        // trap and G-DEAD requires them present.
        let sql = "SELECT DISTINCT b.station_id, b.target_date, b.market_ticker, \
                   b.bracket_lo, b.bracket_hi, r.result \
                   FROM bracket_probability_log b \
                   LEFT JOIN market_resolutions r ON r.market_ticker = b.market_ticker \
                   ORDER BY b.market_ticker";
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| SourceError::ManifestUnavailable {
                reason: format!("preparing manifest query: {e}"),
            })?;
        let rows = stmt
            .query_map([], |row| {
                let station_id: String = row.get(0)?;
                let target_date: String = row.get(1)?;
                let market_ticker: String = row.get(2)?;
                let bracket_lo: Option<i64> = row.get(3)?;
                let bracket_hi: Option<i64> = row.get(4)?;
                let result: Option<String> = row.get(5)?;
                Ok((
                    station_id,
                    target_date,
                    market_ticker,
                    bracket_lo,
                    bracket_hi,
                    result,
                ))
            })
            .map_err(|e| SourceError::ManifestUnavailable {
                reason: format!("querying manifest: {e}"),
            })?;

        let mut engaged = Vec::new();
        for r in rows {
            let (station_id, target_date, market_ticker, bracket_lo, bracket_hi, result) = r
                .map_err(|e| SourceError::ManifestUnavailable {
                    reason: format!("reading manifest row: {e}"),
                })?;
            // resolved iff the result is a binary yes/no; a row WITH a result
            // that is neither (e.g. "void") is engaged-but-voided; a row with NO
            // resolution row at all is engaged-but-unresolved (resolved=false,
            // voided=false).
            let (resolved, voided) = match result.as_deref() {
                Some("yes") | Some("no") => (true, false),
                Some(_) => (false, true),
                None => (false, false),
            };
            engaged.push(EngagedMarket {
                event_linkage: event_linkage(
                    &station_id,
                    bracket_lo,
                    bracket_hi,
                    &target_date,
                    &market_ticker,
                ),
                resolved,
                voided,
            });
        }

        Ok(UniverseManifest { engaged })
    }
}
