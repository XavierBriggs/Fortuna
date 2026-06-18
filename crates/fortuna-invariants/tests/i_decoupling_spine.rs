//! Decoupling guard (Task A7): asserts the spine crates remain domain-neutral
//! and that fortuna-live carries no Kalshi TYPE leak.
//!
//! WHY THIS TEST EXISTS:
//! - fortuna-gates, fortuna-exec, fortuna-state are the "pure spine" — zero
//!   domain knowledge (no weather/kalshi/aeolus literals in production src).
//!   This guard PINS that invariant: if someone adds a domain literal, this
//!   test fails immediately on the next CI run.
//! - fortuna-live is the COMPOSITION ROOT and legitimately names producers
//!   (AEOLUS_PRODUCER, venue strings, weather belief wiring). We do NOT
//!   grep it for bare domain words. We pin only the C1 structural win: no
//!   KalshiMarket TYPE or kalshi::dto TYPE import in fortuna-live/src (the
//!   runner is now venue-agnostic; concrete types stay in fortuna-venues).
//!
//! EXCLUSIONS (documented here; the scan skips them):
//! - **/tests/** — test files may reference domain names for fixtures
//! - kalshi/ directory under a crate's src — the adapter instance itself
//! - aeolus_venue.rs, weather_source.rs — producer-instance files
//! - config files, .sqlx/ — not Rust source
//!
//! KNOWN GAP (fortuna-ledger NOT asserted clean):
//! fortuna-ledger/src/repos.rs contains domain-coupled query methods:
//! `open_aeolus_weather_due` / `OpenWeatherBelief` / a
//! `provenance->>'model_id' = 'aeolus'` literal (repos.rs ~1349-1374).
//! The ledger is NOT yet domain-neutral. Asserting ledger=0 would fail.
//! See GAPS.md entry: "fortuna-ledger has domain-coupled query methods
//! (open_aeolus_weather_due); generic-ledger refactor deferred".

use std::path::Path;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Read all .rs files under `src_dir` (NOT tests/, NOT excluded dirs/files),
/// concatenate their content, and return it.
fn read_src_excluding_tests(src_dir: &Path, excluded_files: &[&str]) -> String {
    let mut out = String::new();
    read_dir_recursive(src_dir, &mut out, excluded_files);
    out
}

fn read_dir_recursive(dir: &Path, out: &mut String, excluded_files: &[&str]) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "tests" || name == "kalshi" {
                continue;
            }
            read_dir_recursive(&path, out, excluded_files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if excluded_files.contains(&file_name) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                out.push_str(&content);
                out.push('\n');
            }
        }
    }
}

/// Strip `//` line comments from source before scanning for domain literals,
/// so we don't flag doc comments or inline comments.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn workspace_root() -> std::path::PathBuf {
    // The test binary runs from the crate root; workspace root is two levels up.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("crate parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

// ── Part 1: pure spine = ZERO domain literals ────────────────────────────────

/// Scan one spine crate's src/ for case-insensitive domain literals.
/// Returns a list of offending lines (empty = clean).
fn scan_spine_for_domain_literals(crate_name: &str) -> Vec<String> {
    let root = workspace_root();
    let src_dir = root.join("crates").join(crate_name).join("src");
    let content = read_src_excluding_tests(&src_dir, &[]);
    // Non-vacuity: a wrong/missing path would read 0 bytes and the
    // `hits.is_empty()` assertion would pass for the WRONG reason. Fail loudly
    // instead so a crate rename or path drift can never silently disarm the guard.
    assert!(
        !content.trim().is_empty(),
        "decoupling guard scanned 0 bytes of {} — path wrong or empty; the guard \
         would pass vacuously. Fix the scan path.",
        src_dir.display()
    );
    let stripped = strip_line_comments(&content);
    let mut hits = Vec::new();
    for (i, line) in stripped.lines().enumerate() {
        let ll = line.to_lowercase();
        if ll.contains("weather") || ll.contains("kalshi") || ll.contains("aeolus") {
            hits.push(format!("{crate_name}/src line ~{}: {}", i + 1, line.trim()));
        }
    }
    hits
}

#[test]
fn spine_gates_has_zero_domain_literals() {
    let hits = scan_spine_for_domain_literals("fortuna-gates");
    assert!(
        hits.is_empty(),
        "fortuna-gates/src contains domain literals (violates spine purity):\n{}",
        hits.join("\n")
    );
}

#[test]
fn spine_exec_has_zero_domain_literals() {
    let hits = scan_spine_for_domain_literals("fortuna-exec");
    assert!(
        hits.is_empty(),
        "fortuna-exec/src contains domain literals (violates spine purity):\n{}",
        hits.join("\n")
    );
}

#[test]
fn spine_state_has_zero_domain_literals() {
    let hits = scan_spine_for_domain_literals("fortuna-state");
    assert!(
        hits.is_empty(),
        "fortuna-state/src contains domain literals (violates spine purity):\n{}",
        hits.join("\n")
    );
}

// ── Part 2: fortuna-live has no Kalshi TYPE leak ─────────────────────────────

/// Scan fortuna-live/src for KalshiMarket or kalshi::dto TYPE references.
/// Excludes: tests/, kalshi/ dir, aeolus_venue.rs, weather_source.rs.
/// Does NOT grep for bare "kalshi" — the daemon is the composition root
/// and legitimately references kalshi venue strings.
#[test]
fn fortuna_live_has_no_kalshi_type_leak() {
    let root = workspace_root();
    let src_dir = root.join("crates").join("fortuna-live").join("src");
    let content = read_src_excluding_tests(&src_dir, &["aeolus_venue.rs", "weather_source.rs"]);
    // Non-vacuity: fail loudly if the scan read nothing (path drift) rather than
    // pass for the wrong reason — fortuna-live/src is large, so 0 bytes means a
    // broken path, not a clean tree.
    assert!(
        !content.trim().is_empty(),
        "decoupling guard scanned 0 bytes of {} — path wrong; the guard would pass \
         vacuously. Fix the scan path.",
        src_dir.display()
    );
    let stripped = strip_line_comments(&content);
    let mut hits = Vec::new();
    for (i, line) in stripped.lines().enumerate() {
        if line.contains("KalshiMarket") || line.contains("kalshi::dto") {
            hits.push(format!("fortuna-live/src line ~{}: {}", i + 1, line.trim()));
        }
    }
    assert!(
        hits.is_empty(),
        "fortuna-live/src has Kalshi TYPE leak \
         (KalshiMarket or kalshi::dto — C1 regression):\n{}",
        hits.join("\n")
    );
}
