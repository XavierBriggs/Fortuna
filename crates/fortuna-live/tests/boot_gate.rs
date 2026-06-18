//! Demo-flip Phase 2 — the boot gate's venue×stage matrix. PURE over the
//! committed example config (operators copy it); NEVER reads the process
//! environment and NEVER hits the live Kalshi API (the boot gate is a pure
//! function over config text, by construction).
//!
//! The matrix (boot.rs `validate_bootable`):
//!   - sim   @ sim       => Ok               (the committed default)
//!   - sim   @ paper     => BadConfig        (sim runs only at Stage::Sim)
//!   - kalshi@ paper +[kalshi] => Ok         (the demo: mock funds, real venue)
//!   - kalshi@ paper, no [kalshi]            => BadConfig (no trading universe)
//!   - kalshi@ paper, empty series           => BadConfig
//!   - kalshi@ sim       => VenueNotBootable (requires stage=paper)
//!   - kalshi@ live_min  => VenueNotBootable (promotion needs the I7 gate)
//!   - kalshi@ scaled    => VenueNotBootable (promotion needs the I7 gate)

use fortuna_live::boot::{BootError, DaemonToml};

const EXAMPLE: &str = include_str!("../../../config/fortuna.example.toml");

/// The committed example with `[daemon].venue`/`stage` rewritten and an
/// optional `[kalshi]` block appended. The example ships `venue = "sim"` and
/// `stage = "sim"`; we string-replace both to drive each matrix cell.
fn cfg_with(venue: &str, stage: &str, kalshi_block: Option<&str>) -> String {
    let base = EXAMPLE
        .replace("venue = \"sim\"", &format!("venue = \"{venue}\""))
        .replace("stage = \"sim\"", &format!("stage = \"{stage}\""));
    match kalshi_block {
        Some(b) => format!("{base}\n{b}\n"),
        None => base,
    }
}

fn cfg_with_data_execution(data_source: &str, execution: &str) -> String {
    cfg_with("kalshi", "paper", Some(KALSHI_OK)).replacen(
        "stage = \"paper\"              # I7 validation stage: sim | paper | live_min | scaled.",
        &format!(
            "stage = \"paper\"              # I7 validation stage: sim | paper | live_min | scaled.\n\
             data_source = \"{data_source}\"\n\
             execution = \"{execution}\""
        ),
        1,
    )
}

/// A valid `[kalshi]` block (non-empty series + bracket_sets).
const KALSHI_OK: &str = "[kalshi]\n\
     series = [\"KXHIGHNY\"]\n\
     bracket_sets = [[\"KXHIGHNY-A\", \"KXHIGHNY-B\", \"KXHIGHNY-C\"]]\n";

/// A kalshi@paper config under the current boot contract (boot.rs `validate_bootable`,
/// the venue=kalshi/stage=paper arm): an explicit `[runtime]` (paper_ledger,
/// orders disabled — the safe demo mode) plus the composite
/// `data_source = "kalshi_prod"` / `execution = "paper"` that the
/// live_data_only|paper_ledger modes require. `kalshi_block` is the optional
/// `[kalshi]` section (None = omitted, to test the missing-universe refusal).
fn paper_on_live_cfg(kalshi_block: Option<&str>) -> String {
    cfg_with("kalshi", "paper", kalshi_block).replacen(
        "stage = \"paper\"              # I7 validation stage: sim | paper | live_min | scaled.",
        "stage = \"paper\"              # I7 validation stage: sim | paper | live_min | scaled.\n\
         data_source = \"kalshi_prod\"\n\
         execution = \"paper\"",
        1,
    ) + "\n[runtime]\nstage = \"paper\"\nexecution_mode = \"paper_ledger\"\norders_enabled = false\n"
}

#[test]
fn sim_at_sim_stage_boots() {
    // The committed default: venue = "sim", stage = "sim".
    let cfg = DaemonToml::parse(EXAMPLE).expect("example parses");
    assert_eq!(cfg.daemon.stage, "sim", "the example ships stage = \"sim\"");
    cfg.validate_bootable().expect("sim @ sim boots");
}

#[test]
fn sim_default_stage_is_sim_when_omitted() {
    // Back-compat: a [daemon] WITHOUT a `stage` field defaults to "sim" and
    // still boots (every pre-demo-flip config omits stage).
    let no_stage = EXAMPLE.replace("stage = \"sim\"", "");
    let cfg = DaemonToml::parse(&no_stage).expect("parses without an explicit stage");
    assert_eq!(cfg.daemon.stage, "sim", "absent stage defaults to sim");
    cfg.validate_bootable().expect("sim @ default(sim) boots");
}

#[test]
fn sim_at_paper_stage_is_bad_config() {
    // venue = "sim" + a promoted stage is a mis-wiring, refused as BadConfig.
    let cfg = DaemonToml::parse(&cfg_with("sim", "paper", None)).expect("parses");
    match cfg.validate_bootable() {
        Err(BootError::BadConfig { reason }) => assert!(
            reason.contains("venue = \"sim\" requires stage = \"sim\""),
            "reason must cite the sim/stage cross-check: {reason}"
        ),
        other => panic!("sim @ paper must be BadConfig, got {other:?}"),
    }
}

#[test]
fn kalshi_at_paper_with_kalshi_section_boots() {
    // The demo: venue = "kalshi", stage = "paper", explicit [runtime] (paper_ledger,
    // orders disabled), data_source=kalshi_prod/execution=paper, non-empty [kalshi].series.
    // validate_bootable is Ok (the credential check is in compose, not here).
    let cfg = DaemonToml::parse(&paper_on_live_cfg(Some(KALSHI_OK))).expect("parses");
    let k = cfg
        .kalshi
        .as_ref()
        .expect("the [kalshi] section parsed through to DaemonToml");
    assert_eq!(k.series, vec!["KXHIGHNY".to_string()]);
    assert_eq!(k.bracket_sets.len(), 1);
    cfg.validate_bootable()
        .expect("kalshi paper-on-live with a non-empty [kalshi] boots");
}

#[test]
fn kalshi_prod_data_with_paper_execution_boots() {
    let cfg = DaemonToml::parse(&paper_on_live_cfg(Some(KALSHI_OK))).expect("parses");
    assert_eq!(cfg.daemon.data_source.as_deref(), Some("kalshi_prod"));
    assert_eq!(cfg.daemon.execution.as_deref(), Some("paper"));
    cfg.validate_bootable()
        .expect("kalshi production data with paper execution boots");
}

#[test]
fn kalshi_prod_data_with_real_execution_is_refused() {
    let cfg = DaemonToml::parse(&cfg_with_data_execution("kalshi_prod", "kalshi")).expect("parses");
    match cfg.validate_bootable() {
        Err(BootError::VenueNotBootable { venue, reason }) => {
            assert_eq!(venue, "kalshi");
            assert!(
                reason.contains("execution") && reason.contains("I7"),
                "reason must cite execution refusal and I7: {reason}"
            );
        }
        other => panic!("kalshi_prod data with real execution must refuse, got {other:?}"),
    }
}

#[test]
fn kalshi_at_paper_without_kalshi_section_is_bad_config() {
    // No [kalshi] => no trading universe => refuse (a silently-inert daemon is
    // worse than a loud refusal). The [runtime] + data_source/execution are valid,
    // so validation reaches the [kalshi].series check.
    let cfg = DaemonToml::parse(&paper_on_live_cfg(None)).expect("parses");
    assert!(cfg.kalshi.is_none(), "no [kalshi] section present");
    match cfg.validate_bootable() {
        Err(BootError::BadConfig { reason }) => assert!(
            reason.contains("[kalshi]") && reason.contains("series"),
            "reason must cite the missing [kalshi].series: {reason}"
        ),
        other => panic!("kalshi @ paper with no [kalshi] must be BadConfig, got {other:?}"),
    }
}

#[test]
fn kalshi_at_paper_with_empty_series_is_bad_config() {
    // A present-but-empty series is the same failure: an empty catalog.
    let empty_series = "[kalshi]\nseries = []\nbracket_sets = []\n";
    let cfg = DaemonToml::parse(&paper_on_live_cfg(Some(empty_series))).expect("parses");
    match cfg.validate_bootable() {
        Err(BootError::BadConfig { reason }) => assert!(
            reason.contains("series"),
            "reason must cite the empty series: {reason}"
        ),
        other => panic!("kalshi @ paper with empty series must be BadConfig, got {other:?}"),
    }
}

#[test]
fn kalshi_at_sim_stage_is_not_bootable() {
    // The Kalshi venue does not run the sim world; it requires stage = "paper".
    let cfg = DaemonToml::parse(&cfg_with("kalshi", "sim", Some(KALSHI_OK))).expect("parses");
    match cfg.validate_bootable() {
        Err(BootError::VenueNotBootable { venue, reason }) => {
            assert_eq!(venue, "kalshi");
            assert!(
                reason.contains("stage=paper"),
                "reason must cite the paper-stage requirement: {reason}"
            );
        }
        other => panic!("kalshi @ sim must be VenueNotBootable, got {other:?}"),
    }
}

#[test]
fn kalshi_at_live_min_is_refused_for_the_i7_gate() {
    // Live promotion needs the forward-validation gate (I7), never a config flip.
    // Present a valid [kalshi] so the refusal is the STAGE, not the section.
    let cfg = DaemonToml::parse(&cfg_with("kalshi", "live_min", Some(KALSHI_OK))).expect("parses");
    match cfg.validate_bootable() {
        Err(BootError::VenueNotBootable { venue, reason }) => {
            assert_eq!(venue, "kalshi");
            assert!(
                reason.contains("forward-validation gate") || reason.contains("I7"),
                "reason must cite the I7 promotion gate: {reason}"
            );
        }
        other => panic!("kalshi @ live_min must be VenueNotBootable, got {other:?}"),
    }
}

#[test]
fn kalshi_at_scaled_is_refused_for_the_i7_gate() {
    let cfg = DaemonToml::parse(&cfg_with("kalshi", "scaled", Some(KALSHI_OK))).expect("parses");
    match cfg.validate_bootable() {
        Err(BootError::VenueNotBootable { venue, reason }) => {
            assert_eq!(venue, "kalshi");
            assert!(
                reason.contains("forward-validation gate") || reason.contains("I7"),
                "reason must cite the I7 promotion gate: {reason}"
            );
        }
        other => panic!("kalshi @ scaled must be VenueNotBootable, got {other:?}"),
    }
}
