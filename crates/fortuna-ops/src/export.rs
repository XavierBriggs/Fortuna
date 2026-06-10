//! Nightly accounting export (spec Section 8): "exports fills, fees,
//! settlements, and realized PnL per venue class to an immutable ledger
//! file (tax treatment differs materially across event contracts, crypto,
//! and equities; the export is the raw material, not tax advice)".
//!
//! Immutability discipline: files are write-ONCE, named by UTC date. A
//! second export for the same date is an ERROR — corrections are new
//! files (a different date or an operator-named amendment), never
//! overwrites.

use crate::OpsError;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ExportFill {
    pub at: String,
    pub venue: String,
    /// Tax class: event_contract | crypto | equity (spec Section 8).
    pub venue_class: String,
    pub market: String,
    pub side: String,
    pub action: String,
    pub price_cents: i64,
    pub qty: i64,
    pub fee_cents: i64,
    pub fill_id: String,
}

#[derive(Debug, Clone)]
pub struct ExportSettlement {
    pub at: String,
    pub venue: String,
    pub venue_class: String,
    pub market: String,
    pub outcome: String,
    pub amount_cents: i64,
    pub status: String,
}

/// RFC-4180 field escaping: quote when the field contains a comma, quote,
/// or newline; double embedded quotes.
fn csv_field(raw: &str) -> String {
    if raw.contains(',') || raw.contains('"') || raw.contains('\n') {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_string()
    }
}

fn write_once(path: &Path, content: &str) -> Result<(), OpsError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true) // refuses existing files: immutable ledger
        .open(path)
        .map_err(|e| OpsError::Export {
            reason: format!(
                "refusing to write {} (immutable ledger files are write-once): {e}",
                path.display()
            ),
        })?;
    file.write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| OpsError::Export {
            reason: format!("writing {}: {e}", path.display()),
        })
}

/// Write the date's ledger files into `dir`; returns the paths written
/// (fills first, then settlements). Errors if either file already exists,
/// WITHOUT touching the existing content.
pub fn write_accounting_export(
    dir: &Path,
    date_utc: &str,
    fills: &[ExportFill],
    settlements: &[ExportSettlement],
) -> Result<Vec<PathBuf>, OpsError> {
    let fills_path = dir.join(format!("fortuna-fills-{date_utc}.csv"));
    let settle_path = dir.join(format!("fortuna-settlements-{date_utc}.csv"));
    // Check BOTH before writing EITHER: a partial export is worse than a
    // refused one.
    for p in [&fills_path, &settle_path] {
        if p.exists() {
            return Err(OpsError::Export {
                reason: format!(
                    "{} already exists; ledger files are write-once",
                    p.display()
                ),
            });
        }
    }

    let mut fills_csv =
        String::from("at,venue,venue_class,market,side,action,price_cents,qty,fee_cents,fill_id\n");
    for f in fills {
        fills_csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            csv_field(&f.at),
            csv_field(&f.venue),
            csv_field(&f.venue_class),
            csv_field(&f.market),
            csv_field(&f.side),
            csv_field(&f.action),
            f.price_cents,
            f.qty,
            f.fee_cents,
            csv_field(&f.fill_id),
        ));
    }
    let mut settle_csv = String::from("at,venue,venue_class,market,outcome,amount_cents,status\n");
    for s in settlements {
        settle_csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv_field(&s.at),
            csv_field(&s.venue),
            csv_field(&s.venue_class),
            csv_field(&s.market),
            csv_field(&s.outcome),
            s.amount_cents,
            csv_field(&s.status),
        ));
    }

    write_once(&fills_path, &fills_csv)?;
    write_once(&settle_path, &settle_csv)?;
    Ok(vec![fills_path, settle_path])
}
