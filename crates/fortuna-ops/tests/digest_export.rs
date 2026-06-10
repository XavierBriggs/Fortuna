//! T1.5: daily digest composer + accounting export (spec Section 8).
//!
//! Doctrine under test:
//! - The digest is a PURE function of its inputs (deterministic text; the
//!   Slack send is the existing audited router's job).
//! - The accounting export writes write-ONCE ledger files (immutable: a
//!   second export for the same date refuses rather than overwrites),
//!   one CSV per concern (fills, settlements, realized PnL summary), with
//!   venue-class columns (tax treatment differs across classes).
//!
//! Written BEFORE the implementation per the repository TDD doctrine.

use fortuna_ops::digest::{compose_daily_digest, DigestInputs, StrategyDigestRow};
use fortuna_ops::export::{write_accounting_export, ExportFill, ExportSettlement};

fn inputs() -> DigestInputs {
    DigestInputs {
        date_utc: "2026-06-10".to_string(),
        stage: "sim".to_string(),
        strategies: vec![
            StrategyDigestRow {
                strategy: "mech_structural".to_string(),
                realized_pnl_cents: 1_234,
                fees_cents: 56,
                fills: 12,
                open_exposure_cents: 9_300,
            },
            StrategyDigestRow {
                strategy: "mech_extremes".to_string(),
                realized_pnl_cents: -200,
                fees_cents: 10,
                fills: 3,
                open_exposure_cents: 4_600,
            },
        ],
        halts_active: 0,
        discrepancies_open: 1,
        settlements_overdue: 0,
        capital_in_limbo_cents: 1_000,
        veto_decisions: 4,
        veto_suppressed: 1,
    }
}

#[test]
fn digest_is_deterministic_and_carries_the_load_bearing_numbers() {
    let a = compose_daily_digest(&inputs());
    let b = compose_daily_digest(&inputs());
    assert_eq!(a, b, "pure function of inputs");

    assert!(a.contains("2026-06-10"));
    assert!(a.contains("mech_structural"), "per-strategy rows");
    assert!(a.contains("$12.34"), "PnL in dollars, cents exact");
    assert!(a.contains("$-2.00"));
    assert!(
        a.contains("discrepancies open: 1"),
        "honesty numbers surface"
    );
    assert!(a.contains("capital in limbo: $10.00"));
    assert!(a.contains("vetoes: 4 (1 suppressed)"));
    // Net across strategies: 1234 - 200 = 1034 gross, fees 66.
    assert!(a.contains("$10.34"));
}

#[test]
fn export_writes_once_and_refuses_overwrite() {
    let dir = std::env::temp_dir().join(format!("fortuna-export-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let fills = vec![ExportFill {
        at: "2026-06-10T13:00:00.000Z".to_string(),
        venue: "sim".to_string(),
        venue_class: "event_contract".to_string(),
        market: "KXS".to_string(),
        side: "yes".to_string(),
        action: "buy".to_string(),
        price_cents: 60,
        qty: 10,
        fee_cents: 1,
        fill_id: "f-1".to_string(),
    }];
    let settlements = vec![ExportSettlement {
        at: "2026-06-10T18:00:00.000Z".to_string(),
        venue: "sim".to_string(),
        venue_class: "event_contract".to_string(),
        market: "KXS".to_string(),
        outcome: "yes".to_string(),
        amount_cents: 1_000,
        status: "confirmed".to_string(),
    }];

    let paths = write_accounting_export(&dir, "2026-06-10", &fills, &settlements).unwrap();
    assert_eq!(paths.len(), 2);
    let fills_csv = std::fs::read_to_string(&paths[0]).unwrap();
    assert!(fills_csv.starts_with(
        "at,venue,venue_class,market,side,action,price_cents,qty,fee_cents,fill_id\n"
    ));
    assert!(
        fills_csv.contains("2026-06-10T13:00:00.000Z,sim,event_contract,KXS,yes,buy,60,10,1,f-1\n")
    );

    // Immutable ledger files: same date refuses, content untouched.
    assert!(write_accounting_export(&dir, "2026-06-10", &fills, &settlements).is_err());
    assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), fills_csv);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_escapes_csv_fields_with_commas_and_quotes() {
    let dir = std::env::temp_dir().join(format!("fortuna-export-esc-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let fills = vec![ExportFill {
        at: "2026-06-10T13:00:00.000Z".to_string(),
        venue: "sim".to_string(),
        venue_class: "event_contract".to_string(),
        market: "KX,\"WEIRD\"".to_string(),
        side: "yes".to_string(),
        action: "buy".to_string(),
        price_cents: 60,
        qty: 1,
        fee_cents: 0,
        fill_id: "f-2".to_string(),
    }];
    let paths = write_accounting_export(&dir, "2026-06-10", &fills, &[]).unwrap();
    let csv = std::fs::read_to_string(&paths[0]).unwrap();
    assert!(
        csv.contains("\"KX,\"\"WEIRD\"\"\""),
        "RFC-4180 escaping: {csv}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
