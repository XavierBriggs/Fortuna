//! S3 (CONSTITUTION structural invariant): deterministic core — time only via
//! the injected `Clock`, no RNG in any decision path.
//!
//! Backtest and shadow reproducibility depend on the decision crates reading no
//! wall clock and rolling no dice. The single sanctioned `Utc::now()` lives in
//! `RealClock` (fortuna-core/src/clock.rs); everything downstream takes the time
//! as an argument. This guard scans the production src of the decision crates
//! (gates, exec, state, cognition) for direct wall-clock reads and unseeded RNG,
//! and fails on any hit. It is the static sibling of the DST corpus (which
//! proves replay determinism dynamically).
//!
//! EXCLUSIONS: tests/ (fixtures may stamp times), kalshi/ adapter dirs. fortuna-core
//! is intentionally NOT scanned: it OWNS `RealClock` and its `Utc::now()`.
//!
//! ADDITIONS-ONLY (protected crate): never weaken these assertions.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("crate parent")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn read_src_excluding_tests(dir: &Path, out: &mut String) {
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
            read_src_excluding_tests(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                out.push_str(&content);
                out.push('\n');
            }
        }
    }
}

/// Strip `//` line comments so a doc comment naming `Utc::now` is not flagged.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wall-clock and RNG markers forbidden in a decision crate's production src.
const FORBIDDEN: &[&str] = &[
    "SystemTime::now",
    "Instant::now",
    "Utc::now",
    "Local::now",
    "thread_rng",
    "rand::random",
    "SmallRng",
];

fn scan_decision_crate(crate_name: &str) -> Vec<String> {
    let src_dir = workspace_root().join("crates").join(crate_name).join("src");
    let mut content = String::new();
    read_src_excluding_tests(&src_dir, &mut content);
    // Non-vacuity: a wrong/empty path would pass the guard for the WRONG reason.
    assert!(
        !content.trim().is_empty(),
        "S3 guard scanned 0 bytes of {} — path wrong or empty; the guard would pass \
         vacuously. Fix the scan path.",
        src_dir.display()
    );
    let stripped = strip_line_comments(&content);
    let mut hits = Vec::new();
    for (i, line) in stripped.lines().enumerate() {
        for marker in FORBIDDEN {
            if line.contains(marker) {
                hits.push(format!("{crate_name}/src line ~{}: {}", i + 1, line.trim()));
            }
        }
    }
    hits
}

fn assert_clean(crate_name: &str) {
    let hits = scan_decision_crate(crate_name);
    assert!(
        hits.is_empty(),
        "{crate_name}/src reads a wall clock or RNG directly (violates S3 determinism — \
         take time from the injected Clock, seed RNG only in the DST harness):\n{}",
        hits.join("\n")
    );
}

#[test]
fn s3_gates_have_no_wallclock_or_rng() {
    assert_clean("fortuna-gates");
}

#[test]
fn s3_exec_has_no_wallclock_or_rng() {
    assert_clean("fortuna-exec");
}

#[test]
fn s3_state_has_no_wallclock_or_rng() {
    assert_clean("fortuna-state");
}

#[test]
fn s3_decision_crates_have_no_wallclock_or_rng() {
    // cognition is the model-facing decision path; it too takes time from the
    // Clock and rolls no dice. Bundled as the named map test for S3.
    assert_clean("fortuna-cognition");
}
