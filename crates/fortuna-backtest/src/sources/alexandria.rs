//! `AlexandriaSource` — the JSONL-backed [`HistoricalSource`] (Alexandria's
//! published-contract reader).
//!
//! Alexandria's weather domain publishes the four `HistoricalSource` streams as
//! JSONL files — each line is exactly one `serde_json::to_string(&record)` of a
//! `fortuna-backtest` record type (the canonical boundary format; see
//! [`crate::records`]). This adapter is the deserialize counterpart to
//! [`super::aeolus_archive`]'s SQLite read: `aeolus_archive` reads the legacy
//! `aeolus_kalshi.db` directly; `AlexandriaSource` reads Alexandria's *published*
//! output. They are designed as byte-parity oracles for each other — run both
//! over the same source DB and the emitted records are identical (modulo the
//! documented half-away-from-zero cents rounding, conformed Alexandria-side).
//!
//! ## What it reads — a publish directory
//!
//! - `beliefs.jsonl`   → [`HistoricalBelief`] per line
//! - `snapshots.jsonl` → [`HistoricalSnapshot`] per line
//! - `outcomes.jsonl`  → [`HistoricalOutcome`] per line
//! - `universe.jsonl`  → [`EngagedMarket`] per line (assembled into a [`UniverseManifest`])
//! - `trades.jsonl`    → OPTIONAL; [`HistoricalTrade`] per line. Absent in the
//!   weather contract (paper-only, out of scope); read **iff present**, and the
//!   paper-only invariant (`orders == 0`) is re-enforced at the read boundary
//!   because serde bypasses the [`HistoricalTrade::new`] constructor.
//! - `manifest.json`   → OPTIONAL discovery manifest (see [`DomainManifest`]).
//!   When present it is the source of truth for the stream PATHS (the indirection
//!   that lets the physical layout change without touching this reader) and its
//!   `schema_version` major is checked fail-closed. When absent, the conventional
//!   filenames above are used — so a bare four-file directory still reads.
//!
//! Contract doc: `docs/coordination/2026-06-25-alexandria-manifest-contract.md`.
//!
//! ## PIT / firewall — this reader maps no time and forms no belief
//!
//! Every record is already canonical with its knowledge-time stamps set by the
//! producer (`available_at` = forecast issuance, `decided_at` = target). The
//! firewall lives upstream in Alexandria (ADR 0008); the reader only
//! deserializes. The harness enforces G-PIT (`available_at < decided_at`)
//! exactly as it does for `aeolus_archive`.
//!
//! ## Source-name literals
//!
//! Like every adapter under `src/sources/`, this file legitimately names the
//! concrete producer (`"alexandria"`); the decoupling grep gate excludes
//! `src/sources/`. Those literals must NOT appear elsewhere under
//! `crates/fortuna-backtest/src/`.

use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::manifest::{EngagedMarket, UniverseManifest};
use crate::records::{HistoricalBelief, HistoricalOutcome, HistoricalSnapshot, HistoricalTrade};
use crate::source::{HistoricalSource, SourceError};

/// Conventional stream filenames within a publish directory (the no-manifest
/// fallback).
const BELIEFS_FILE: &str = "beliefs.jsonl";
const SNAPSHOTS_FILE: &str = "snapshots.jsonl";
const OUTCOMES_FILE: &str = "outcomes.jsonl";
const UNIVERSE_FILE: &str = "universe.jsonl";
const TRADES_FILE: &str = "trades.jsonl";
const MANIFEST_FILE: &str = "manifest.json";

/// The published-contract schema MAJOR version this reader understands. The
/// reader gates on MAJOR only: additive (MINOR) changes keep working; a MAJOR
/// bump is breaking and is refused until the reader is updated in lockstep.
pub const SUPPORTED_SCHEMA_MAJOR: &str = "1";

// ---------------------------------------------------------------------------
// The published manifest (discovery + integrity)
// ---------------------------------------------------------------------------

/// One published stream's manifest entry: where it is and how many rows it has.
///
/// Deliberately NOT `deny_unknown_fields` — the producer may add fields without
/// a MAJOR bump and this reader ignores them (forward-compatible).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StreamEntry {
    /// Path to the stream file, relative to the publish directory.
    pub path: String,
    /// The number of JSONL records (non-blank lines) the producer wrote.
    /// [`AlexandriaSource::verify`] checks the file against this, fail-closed.
    pub rows: u64,
    /// Optional hex SHA-256 of the stream file. Reserved for a stronger
    /// integrity gate; row-count is the v1 check.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// The four (+ optional trades) stream entries a domain manifest declares.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StreamSet {
    pub beliefs: StreamEntry,
    pub snapshots: StreamEntry,
    pub outcomes: StreamEntry,
    pub universe: StreamEntry,
    /// Present only if the producer emits a paper-trade stream (out of scope in
    /// the weather contract).
    #[serde(default)]
    pub trades: Option<StreamEntry>,
}

/// One `(scope, producer)` slice the domain publishes — the discovery unit that
/// tells a consumer which `validate --scope --producer` targets exist (and which
/// are `Insufficient` by `resolved_n`) without scanning a JSONL line.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PublishedSlice {
    /// Matches `provenance.scope` in the records (e.g. `forecast:KNYC`).
    pub scope: String,
    /// Matches `provenance.producer_id` (e.g. `historical-import`).
    #[serde(default)]
    pub producer: Option<String>,
    /// `exploratory` | `trusted` for this slice.
    #[serde(default)]
    pub trust: Option<String>,
    /// Resolved periods in this slice — lets a consumer skip `resolved_n < 30`.
    #[serde(default)]
    pub resolved_n: Option<u64>,
}

/// A published domain manifest (`manifest.json`).
///
/// Contract: `docs/coordination/2026-06-25-alexandria-manifest-contract.md`.
/// Forward-compatible (no `deny_unknown_fields`): the producer may add fields
/// (`source_commit`, `covered_range`, …) without a MAJOR bump.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DomainManifest {
    /// `"MAJOR.MINOR"`; the reader gates on MAJOR (see [`SUPPORTED_SCHEMA_MAJOR`]).
    pub schema_version: String,
    pub domain: String,
    pub generated_at: String,
    /// The Alexandria commit that produced these bytes (reproducibility).
    #[serde(default)]
    pub source_commit: Option<String>,
    pub streams: StreamSet,
    #[serde(default)]
    pub slices: Vec<PublishedSlice>,
}

/// Refuse a manifest whose schema MAJOR this reader does not understand
/// (fail-closed: a mismatch is a hard error, never a silent misparse).
fn check_schema_version(version: &str) -> Result<(), SourceError> {
    // `split('.').next()` yields the whole string for a version with no dot, and
    // `Some("")` for the empty string — never `None`; the `unwrap_or` is a
    // no-panic guard, not a real fallback.
    let major = version.split('.').next().unwrap_or(version);
    if major != SUPPORTED_SCHEMA_MAJOR {
        return Err(SourceError::Malformed {
            reason: format!(
                "unsupported manifest schema_version {version:?}: \
                 this reader supports major {SUPPORTED_SCHEMA_MAJOR}.x"
            ),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// JsonlStream — lazy, bounded-memory line reader
// ---------------------------------------------------------------------------

/// A lazy JSONL reader: one `serde_json::from_str` per non-blank line. The whole
/// file is never materialized; the working-set bound is a single line.
///
/// - Open failure surfaces as the first yielded `Err(SourceError::Io)`, then the
///   stream ends (mirrors `aeolus_archive`'s "single Err then end").
/// - A malformed line surfaces as an `Err(SourceError::Malformed)` at its
///   position; the stream then continues (the harness decides whether to abort).
/// - Blank lines are skipped.
struct JsonlStream<T> {
    lines: Option<Lines<BufReader<File>>>,
    pending_open_error: Option<SourceError>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> JsonlStream<T> {
    fn open(path: &Path) -> Self {
        match File::open(path) {
            Ok(file) => JsonlStream {
                lines: Some(BufReader::new(file).lines()),
                pending_open_error: None,
                _marker: PhantomData,
            },
            Err(e) => JsonlStream {
                lines: None,
                pending_open_error: Some(SourceError::Io {
                    reason: format!("opening {}: {e}", path.display()),
                }),
                _marker: PhantomData,
            },
        }
    }
}

impl<T: DeserializeOwned> Iterator for JsonlStream<T> {
    type Item = Result<T, SourceError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(err) = self.pending_open_error.take() {
            return Some(Err(err));
        }
        let lines = self.lines.as_mut()?;
        loop {
            match lines.next()? {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    return Some(serde_json::from_str::<T>(trimmed).map_err(|e| {
                        SourceError::Malformed {
                            reason: format!("JSONL parse error: {e}"),
                        }
                    }));
                }
                Err(e) => {
                    return Some(Err(SourceError::Io {
                        reason: format!("reading line: {e}"),
                    }))
                }
            }
        }
    }
}

/// Count non-blank lines in a file (the record count the stream would yield).
fn count_records(path: &Path) -> Result<u64, SourceError> {
    let file = File::open(path).map_err(|e| SourceError::Io {
        reason: format!("opening {}: {e}", path.display()),
    })?;
    let mut n: u64 = 0;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| SourceError::Io {
            reason: format!("reading {}: {e}", path.display()),
        })?;
        if !line.trim().is_empty() {
            n += 1;
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// AlexandriaSource
// ---------------------------------------------------------------------------

/// A [`HistoricalSource`] backed by Alexandria's published JSONL streams.
#[derive(Debug)]
pub struct AlexandriaSource {
    beliefs_path: PathBuf,
    snapshots_path: PathBuf,
    outcomes_path: PathBuf,
    universe_path: PathBuf,
    trades_path: Option<PathBuf>,
    /// The parsed manifest, when the publish dir carried one.
    manifest: Option<DomainManifest>,
}

impl AlexandriaSource {
    /// Open a publish directory.
    ///
    /// If `dir/manifest.json` is present it is parsed, its `schema_version` MAJOR
    /// is checked fail-closed, and the stream paths are taken from it (the
    /// indirection). Otherwise the conventional filenames are used and `trades`
    /// is read iff `trades.jsonl` exists. Stream files are opened lazily per
    /// access — this constructor does no record IO and does NOT verify counts
    /// (call [`AlexandriaSource::verify`] for the fail-closed integrity check).
    pub fn open_domain(dir: impl AsRef<Path>) -> Result<Self, SourceError> {
        let dir = dir.as_ref();
        let manifest_path = dir.join(MANIFEST_FILE);
        if manifest_path.exists() {
            let text = std::fs::read_to_string(&manifest_path).map_err(|e| SourceError::Io {
                reason: format!("reading {}: {e}", manifest_path.display()),
            })?;
            let manifest: DomainManifest =
                serde_json::from_str(&text).map_err(|e| SourceError::Malformed {
                    reason: format!("parsing {}: {e}", manifest_path.display()),
                })?;
            check_schema_version(&manifest.schema_version)?;
            let streams = &manifest.streams;
            let trades_path = streams.trades.as_ref().map(|t| dir.join(&t.path));
            let beliefs_path = dir.join(&streams.beliefs.path);
            let snapshots_path = dir.join(&streams.snapshots.path);
            let outcomes_path = dir.join(&streams.outcomes.path);
            let universe_path = dir.join(&streams.universe.path);
            Ok(AlexandriaSource {
                beliefs_path,
                snapshots_path,
                outcomes_path,
                universe_path,
                trades_path,
                manifest: Some(manifest),
            })
        } else {
            let trades = dir.join(TRADES_FILE);
            Ok(AlexandriaSource {
                beliefs_path: dir.join(BELIEFS_FILE),
                snapshots_path: dir.join(SNAPSHOTS_FILE),
                outcomes_path: dir.join(OUTCOMES_FILE),
                universe_path: dir.join(UNIVERSE_FILE),
                trades_path: trades.exists().then_some(trades),
                manifest: None,
            })
        }
    }

    /// The parsed manifest, if the publish dir carried one. The `slices` field is
    /// the discovery surface (which `(scope, producer)` targets exist).
    pub fn manifest(&self) -> Option<&DomainManifest> {
        self.manifest.as_ref()
    }

    /// Fail-closed integrity check: every declared stream's record count must
    /// equal the manifest's `rows`. A mismatch (truncated / torn / half-written
    /// stream) is a hard error, never a silent partial read. A no-op when there
    /// is no manifest (nothing to check against).
    pub fn verify(&self) -> Result<(), SourceError> {
        let Some(manifest) = &self.manifest else {
            return Ok(());
        };
        let streams = &manifest.streams;
        let mut checks: Vec<(&StreamEntry, &Path)> = vec![
            (&streams.beliefs, &self.beliefs_path),
            (&streams.snapshots, &self.snapshots_path),
            (&streams.outcomes, &self.outcomes_path),
            (&streams.universe, &self.universe_path),
        ];
        if let (Some(entry), Some(path)) = (&streams.trades, &self.trades_path) {
            checks.push((entry, path));
        }
        for (entry, path) in checks {
            let got = count_records(path)?;
            if got != entry.rows {
                return Err(SourceError::Malformed {
                    reason: format!(
                        "stream {} row-count mismatch: manifest declares {}, file has {}",
                        path.display(),
                        entry.rows,
                        got
                    ),
                });
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HistoricalSource impl
// ---------------------------------------------------------------------------

impl HistoricalSource for AlexandriaSource {
    fn beliefs(&self) -> Box<dyn Iterator<Item = Result<HistoricalBelief, SourceError>> + '_> {
        Box::new(JsonlStream::<HistoricalBelief>::open(&self.beliefs_path))
    }

    fn outcomes(&self) -> Box<dyn Iterator<Item = Result<HistoricalOutcome, SourceError>> + '_> {
        Box::new(JsonlStream::<HistoricalOutcome>::open(&self.outcomes_path))
    }

    fn snapshots(&self) -> Box<dyn Iterator<Item = Result<HistoricalSnapshot, SourceError>> + '_> {
        Box::new(JsonlStream::<HistoricalSnapshot>::open(
            &self.snapshots_path,
        ))
    }

    fn trades(&self) -> Box<dyn Iterator<Item = Result<HistoricalTrade, SourceError>> + '_> {
        match &self.trades_path {
            // The paper-only invariant (orders == 0) is enforced by
            // HistoricalTrade::new at construction, but serde deserialization
            // bypasses the constructor — so re-check it here at the boundary. A
            // nonzero orders is a hard error, never a silently-carried real order.
            Some(path) => Box::new(JsonlStream::<HistoricalTrade>::open(path).map(|r| {
                r.and_then(|t| {
                    if t.orders == 0 {
                        Ok(t)
                    } else {
                        Err(SourceError::Malformed {
                            reason: format!(
                                "paper-only invariant: trade orders must be 0, got {}",
                                t.orders
                            ),
                        })
                    }
                })
            })),
            None => Box::new(std::iter::empty()),
        }
    }

    fn universe_manifest(&self) -> Result<UniverseManifest, SourceError> {
        let mut engaged = Vec::new();
        for row in JsonlStream::<EngagedMarket>::open(&self.universe_path) {
            let market = row.map_err(|e| SourceError::ManifestUnavailable {
                reason: format!("universe stream: {e}"),
            })?;
            engaged.push(market);
        }
        Ok(UniverseManifest { engaged })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::records::BeliefPayload;
    use std::sync::atomic::{AtomicU64, Ordering};

    // --- the REAL sample lines from the 2026-06-25 publish handoff -----------
    const BELIEF_LINE: &str = r#"{"provenance":{"producer_type":"aeolus","producer_id":"historical-import","mind_id":null,"mind_version":null,"strategy_id":"KXHIGHNY-26JUN11-B88.5","category":"forecast","scope":"forecast:KNYC"},"payload":{"kind":"binary","p":0.055403775350064015},"event_linkage":"event://forecast/station-KNYC/bracket-88-89/2026-06-11/KXHIGHNY-26JUN11-B88.5","available_at":"2026-06-10T12:00:00.000Z","decided_at":"2026-06-11T00:00:00.000Z"}"#;
    const SNAPSHOT_LINE: &str = r#"{"market":"event://forecast/station-KNYC/bracket-92-93/2026-06-11/KXHIGHNY-26JUN11-B92.5","price":49,"at":"2026-06-11T00:00:59.373Z"}"#;
    const OUTCOME_LINE: &str = r#"{"event_linkage":"event://forecast/station-KNYC/bracket-94-95/2026-06-11/KXHIGHNY-26JUN11-B94.5","outcome":0.0,"resolved_at":"2026-06-12T20:02:11.025Z","resolution_source":"kalshi"}"#;
    const UNIVERSE_LINE: &str = r#"{"event_linkage":"event://forecast/station-KNYC/bracket-88-89/2026-06-11/KXHIGHNY-26JUN11-B88.5","resolved":false,"voided":false}"#;

    /// Unique temp directory per test (std-only; no `tempfile` dev-dep). Uniqueness
    /// via pid + an atomic counter so parallel tests never collide.
    fn temp_dir(tag: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fortuna_alexandria_{}_{}_{}",
            std::process::id(),
            tag,
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    /// Write the four bare streams (no manifest) into a fresh dir; returns it.
    fn four_stream_dir(tag: &str) -> PathBuf {
        let dir = temp_dir(tag);
        write(&dir, BELIEFS_FILE, &format!("{BELIEF_LINE}\n"));
        write(&dir, SNAPSHOTS_FILE, &format!("{SNAPSHOT_LINE}\n"));
        write(&dir, OUTCOMES_FILE, &format!("{OUTCOME_LINE}\n"));
        write(&dir, UNIVERSE_FILE, &format!("{UNIVERSE_LINE}\n"));
        dir
    }

    // --- the handoff's "round-trip with zero leftover keys" claim, proven -----

    #[test]
    fn belief_sample_deserializes_into_the_real_type() {
        let b: HistoricalBelief = serde_json::from_str(BELIEF_LINE).unwrap();
        match b.payload {
            BeliefPayload::Binary { p } => assert!((p - 0.055403775350064015).abs() < 1e-15),
            other => panic!("expected Binary, got {other:?}"),
        }
        assert_eq!(b.provenance.producer_type, "aeolus");
        assert_eq!(b.provenance.producer_id, "historical-import");
        assert_eq!(b.provenance.scope, "forecast:KNYC");
        assert_eq!(
            b.event_linkage,
            "event://forecast/station-KNYC/bracket-88-89/2026-06-11/KXHIGHNY-26JUN11-B88.5"
        );
        // available_at = issuance (the PIT knowledge time) strictly < decided_at.
        assert!(b.available_at < b.decided_at);
        assert_eq!(b.available_at.to_iso8601(), "2026-06-10T12:00:00.000Z");
        assert_eq!(b.decided_at.to_iso8601(), "2026-06-11T00:00:00.000Z");
    }

    #[test]
    fn snapshot_sample_price_is_integer_cents() {
        let s: HistoricalSnapshot = serde_json::from_str(SNAPSHOT_LINE).unwrap();
        assert_eq!(s.price.raw(), 49);
        // market carries the full event_linkage (the join key), not a bare ticker.
        assert!(s.market.starts_with("event://forecast/station-KNYC/"));
    }

    #[test]
    fn outcome_and_universe_samples_deserialize() {
        let o: HistoricalOutcome = serde_json::from_str(OUTCOME_LINE).unwrap();
        assert_eq!(o.outcome, 0.0);
        assert_eq!(o.resolution_source, "kalshi");
        let u: EngagedMarket = serde_json::from_str(UNIVERSE_LINE).unwrap();
        assert!(!u.resolved);
        assert!(!u.voided);
    }

    // --- JsonlStream behavior -------------------------------------------------

    #[test]
    fn jsonl_stream_skips_blank_lines_and_reads_records() {
        let dir = temp_dir("blanks");
        write(
            &dir,
            BELIEFS_FILE,
            &format!("{BELIEF_LINE}\n\n{BELIEF_LINE}\n"),
        );
        let got: Vec<_> = JsonlStream::<HistoricalBelief>::open(&dir.join(BELIEFS_FILE)).collect();
        assert_eq!(got.len(), 2, "two records, blank line skipped");
        assert!(got.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn jsonl_stream_malformed_line_is_error_not_panic() {
        let dir = temp_dir("malformed");
        write(&dir, BELIEFS_FILE, "{not valid json}\n");
        let mut it = JsonlStream::<HistoricalBelief>::open(&dir.join(BELIEFS_FILE));
        let first = it.next().unwrap();
        assert!(matches!(first, Err(SourceError::Malformed { .. })));
    }

    #[test]
    fn jsonl_stream_missing_file_yields_io_error_then_ends() {
        let dir = temp_dir("missing");
        let mut it = JsonlStream::<HistoricalBelief>::open(&dir.join("nope.jsonl"));
        assert!(matches!(it.next(), Some(Err(SourceError::Io { .. }))));
        assert!(it.next().is_none(), "ends after the single open error");
    }

    // --- AlexandriaSource over a bare four-file directory (no manifest) --------

    #[test]
    fn open_domain_no_manifest_reads_all_streams() {
        let dir = four_stream_dir("bare");
        let src = AlexandriaSource::open_domain(&dir).unwrap();
        assert!(src.manifest().is_none());

        let beliefs: Result<Vec<_>, _> = src.beliefs().collect();
        assert_eq!(beliefs.unwrap().len(), 1);
        let outcomes: Result<Vec<_>, _> = src.outcomes().collect();
        assert_eq!(outcomes.unwrap().len(), 1);
        let snapshots: Result<Vec<_>, _> = src.snapshots().collect();
        assert_eq!(snapshots.unwrap().len(), 1);

        // No trades.jsonl present → empty (paper-only contract).
        let trades: Vec<_> = src.trades().collect();
        assert!(trades.is_empty());

        // universe.jsonl assembled into the harness manifest.
        let manifest = src.universe_manifest().unwrap();
        assert_eq!(manifest.engaged.len(), 1);
        assert!(!manifest.engaged[0].resolved);

        // No manifest → verify() is a no-op (nothing to check against).
        assert!(src.verify().is_ok());
    }

    // --- AlexandriaSource over a manifest-driven directory --------------------

    fn manifest_json(rows_beliefs: u64) -> String {
        format!(
            r#"{{"schema_version":"1.0","domain":"weather","generated_at":"2026-06-25T18:00:00.000Z","source_commit":"f0ed506","streams":{{"beliefs":{{"path":"beliefs.jsonl","rows":{rows_beliefs},"sha256":null}},"snapshots":{{"path":"snapshots.jsonl","rows":1}},"outcomes":{{"path":"outcomes.jsonl","rows":1}},"universe":{{"path":"universe.jsonl","rows":1}}}},"slices":[{{"scope":"forecast:KNYC","producer":"historical-import","trust":"exploratory","resolved_n":60}}]}}"#
        )
    }

    #[test]
    fn open_domain_with_manifest_uses_paths_and_exposes_slices() {
        let dir = four_stream_dir("withmanifest");
        write(&dir, MANIFEST_FILE, &manifest_json(1));
        let src = AlexandriaSource::open_domain(&dir).unwrap();

        let manifest = src.manifest().expect("manifest present");
        assert_eq!(manifest.domain, "weather");
        assert_eq!(manifest.source_commit.as_deref(), Some("f0ed506"));
        assert_eq!(manifest.slices.len(), 1);
        assert_eq!(manifest.slices[0].scope, "forecast:KNYC");
        assert_eq!(manifest.slices[0].resolved_n, Some(60));

        // Streams still read through the manifest-declared paths.
        let beliefs: Result<Vec<_>, _> = src.beliefs().collect();
        assert_eq!(beliefs.unwrap().len(), 1);
        // Correct counts → verify passes.
        assert!(src.verify().is_ok());
    }

    #[test]
    fn verify_fails_closed_on_row_count_mismatch() {
        let dir = four_stream_dir("badrows");
        // Manifest claims 2 belief rows; the file has 1.
        write(&dir, MANIFEST_FILE, &manifest_json(2));
        let src = AlexandriaSource::open_domain(&dir).unwrap();
        let err = src.verify().unwrap_err();
        assert!(
            matches!(&err, SourceError::Malformed { reason } if reason.contains("row-count mismatch")),
            "got {err:?}"
        );
    }

    #[test]
    fn unsupported_schema_major_is_refused() {
        let dir = four_stream_dir("badschema");
        let m = manifest_json(1).replace(r#""schema_version":"1.0""#, r#""schema_version":"2.0""#);
        write(&dir, MANIFEST_FILE, &m);
        let err = AlexandriaSource::open_domain(&dir).unwrap_err();
        assert!(
            matches!(&err, SourceError::Malformed { reason } if reason.contains("schema_version")),
            "got {err:?}"
        );
    }

    #[test]
    fn unknown_manifest_fields_are_tolerated() {
        // Forward-compat: an additive field (covered_range) must not break parse.
        let dir = four_stream_dir("extrakeys");
        let m = manifest_json(1).replace(
            r#""slices":["#,
            r#""covered_range":{"from":"2026-04-22","to":"2026-06-24"},"slices":["#,
        );
        write(&dir, MANIFEST_FILE, &m);
        let src = AlexandriaSource::open_domain(&dir).unwrap();
        assert!(src.manifest().is_some());
    }

    // --- trades stream: optional + the paper-only invariant re-enforced -------

    #[test]
    fn trades_present_with_orders_zero_reads() {
        let dir = four_stream_dir("trades_ok");
        let trade = r#"{"event_linkage":"event://forecast/station-KNYC/bracket-88-89/2026-06-11/KXHIGHNY-26JUN11-B88.5","side":"yes","price":50,"contracts":3,"at":"2026-06-11T00:00:00.000Z","orders":0}"#;
        write(&dir, TRADES_FILE, &format!("{trade}\n"));
        let src = AlexandriaSource::open_domain(&dir).unwrap();
        let trades: Result<Vec<_>, _> = src.trades().collect();
        assert_eq!(trades.unwrap().len(), 1);
    }

    #[test]
    fn trades_with_nonzero_orders_is_rejected_at_the_boundary() {
        // serde would bypass HistoricalTrade::new; the reader re-enforces orders==0.
        let dir = four_stream_dir("trades_bad");
        let trade = r#"{"event_linkage":"event://forecast/station-KNYC/bracket-88-89/2026-06-11/KXHIGHNY-26JUN11-B88.5","side":"yes","price":50,"contracts":3,"at":"2026-06-11T00:00:00.000Z","orders":1}"#;
        write(&dir, TRADES_FILE, &format!("{trade}\n"));
        let src = AlexandriaSource::open_domain(&dir).unwrap();
        let trades: Vec<_> = src.trades().collect();
        assert_eq!(trades.len(), 1);
        assert!(
            matches!(&trades[0], Err(SourceError::Malformed { reason }) if reason.contains("orders must be 0")),
            "got {:?}",
            trades[0]
        );
    }

    // --- the manifest indirection itself (the contract's reason to exist) -----

    #[test]
    fn manifest_stream_path_indirection_is_honoured() {
        // The reader must read from the manifest-DECLARED path, not the
        // conventional filename. Put the belief stream at a NON-default path,
        // point the manifest at it, and assert the conventional `beliefs.jsonl`
        // does NOT exist — so a reader that ignored `streams.*.path` and
        // hardcoded the filename would yield an Io error here instead of the row.
        let dir = temp_dir("indirection");
        write(&dir, "beliefs.renamed.jsonl", &format!("{BELIEF_LINE}\n"));
        write(&dir, SNAPSHOTS_FILE, &format!("{SNAPSHOT_LINE}\n"));
        write(&dir, OUTCOMES_FILE, &format!("{OUTCOME_LINE}\n"));
        write(&dir, UNIVERSE_FILE, &format!("{UNIVERSE_LINE}\n"));
        let m = r#"{"schema_version":"1.0","domain":"weather","generated_at":"2026-06-25T18:00:00.000Z","streams":{"beliefs":{"path":"beliefs.renamed.jsonl","rows":1},"snapshots":{"path":"snapshots.jsonl","rows":1},"outcomes":{"path":"outcomes.jsonl","rows":1},"universe":{"path":"universe.jsonl","rows":1}},"slices":[]}"#;
        write(&dir, MANIFEST_FILE, m);
        assert!(
            !dir.join(BELIEFS_FILE).exists(),
            "the conventional beliefs.jsonl must be absent so this proves indirection"
        );
        let src = AlexandriaSource::open_domain(&dir).unwrap();
        let beliefs: Result<Vec<_>, _> = src.beliefs().collect();
        assert_eq!(
            beliefs.unwrap().len(),
            1,
            "the reader must follow the manifest-declared path, not the default filename"
        );
        // verify() over the declared (non-default) paths also passes.
        assert!(src.verify().is_ok());
    }

    #[test]
    fn verify_fails_closed_on_trades_row_mismatch() {
        // Exercises the optional trades branch of verify(): a declared trades
        // row-count that the file does not match must fail closed.
        let dir = four_stream_dir("trades_verify");
        let trade = r#"{"event_linkage":"event://forecast/station-KNYC/bracket-88-89/2026-06-11/KXHIGHNY-26JUN11-B88.5","side":"yes","price":50,"contracts":3,"at":"2026-06-11T00:00:00.000Z","orders":0}"#;
        write(&dir, TRADES_FILE, &format!("{trade}\n"));
        // Manifest declares trades.rows = 5; the file has 1.
        let m = r#"{"schema_version":"1.0","domain":"weather","generated_at":"2026-06-25T18:00:00.000Z","streams":{"beliefs":{"path":"beliefs.jsonl","rows":1},"snapshots":{"path":"snapshots.jsonl","rows":1},"outcomes":{"path":"outcomes.jsonl","rows":1},"universe":{"path":"universe.jsonl","rows":1},"trades":{"path":"trades.jsonl","rows":5}},"slices":[]}"#;
        write(&dir, MANIFEST_FILE, m);
        let src = AlexandriaSource::open_domain(&dir).unwrap();
        let err = src.verify().unwrap_err();
        assert!(
            matches!(&err, SourceError::Malformed { reason } if reason.contains("row-count mismatch")),
            "trades row mismatch must fail closed; got {err:?}"
        );
    }
}
