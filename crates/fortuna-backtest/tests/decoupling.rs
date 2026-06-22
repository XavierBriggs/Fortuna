//! WS3 decoupling regression: structural greps as executable #[test]s (W6b #4).
//!
//! These tests permanently encode the two decoupling invariants so CI catches
//! drift automatically (not just at the boundary gate).
//!
//! ## What they check
//!
//! **Test 1 — no source-name literals in `crates/fortuna-backtest/src/`**
//! (excluding `src/sources/` — the source adapters are ALLOWED to name sources).
//! The backtest core logic must stay source-agnostic so new sources slot in
//! without touching sweep/harness/edge_provider.
//!
//! **Test 2 — `crates/fortuna-scoring` is dependency-pure**: its `Cargo.toml`
//! must NOT add `rand`/`getrandom`/`libm` (no stochastic state in the pure
//! scoring engine) and its `src/` must contain no `sqlx::`/`tokio::`/`async fn`
//! (scoring stays sync + DB-free).
//!
//! ## Mutation proof (verification-methodology §8)
//!
//! The brief requires confirming each test reds on a planted violation and
//! recovers on revert. The mutation-proof was confirmed by the implementer at
//! commit time (plant `"aeolus"` literal in `src/lib.rs` → test 1 reds;
//! plant `rand = "0.8"` in `Cargo.toml` → test 2 reds; revert → green).
//! The mutation-proof is documented HERE (not repeated in-test — the test
//! logic IS the gate; a false-negative would be caught by inspecting any
//! future failure to red on a planted violation).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    // Walk up from the crate manifest dir to the workspace root (the dir
    // that contains the top-level Cargo.toml with [workspace]).
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set (run via cargo test)");
    PathBuf::from(manifest)
        .parent()
        .expect("crate dir has a parent (crates/)")
        .parent()
        .expect("crates/ has a parent (workspace root)")
        .to_path_buf()
}

/// Recursively collect all `*.rs` files under `dir`.
fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    for entry in walkdir_rs(dir) {
        if entry.extension().is_some_and(|e| e == "rs") {
            out.push(entry);
        }
    }
    out
}

/// Minimal recursive dir walker — avoids a new dep.
fn walkdir_rs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walkdir_rs(&path));
        } else {
            out.push(path);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Test 1 — no source-name literals in fortuna-backtest/src/ (excl. sources/)
// ---------------------------------------------------------------------------

/// Source-name literals that must NOT appear in fortuna-backtest/src/ (outside
/// of `src/sources/`). These are the known producer/source identifiers; adding
/// a new one here is intentional (not a test weakening — the set only grows).
const BANNED_SOURCE_LITERALS: &[&str] = &[
    "\"aeolus\"",
    "\"meteorologist\"",
    "\"kalshi\"",
    "\"historical-import\"",
];

#[test]
fn backtest_src_has_no_source_name_literals_outside_sources_dir() {
    let root = workspace_root();
    let backtest_src = root.join("crates/fortuna-backtest/src");
    let sources_dir = backtest_src.join("sources");

    let files = collect_rs_files(&backtest_src);
    assert!(
        !files.is_empty(),
        "no .rs files found under {backtest_src:?} — check the path"
    );

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        // Exclude the sources/ subtree — adapters are allowed to name sources.
        if file.starts_with(&sources_dir) {
            continue;
        }

        let content =
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("cannot read {file:?}: {e}"));

        for &literal in BANNED_SOURCE_LITERALS {
            if content.contains(literal) {
                violations.push(format!(
                    "{}: contains banned source literal {literal}",
                    file.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "fortuna-backtest/src/ (excluding src/sources/) must not contain source-name literals \
         (decoupling invariant). Violations:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Test 2 — fortuna-scoring Cargo.toml + src/ purity
// ---------------------------------------------------------------------------

#[test]
fn scoring_cargo_has_no_stochastic_or_io_deps() {
    let root = workspace_root();
    let cargo_toml_path = root.join("crates/fortuna-scoring/Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path)
        .unwrap_or_else(|e| panic!("cannot read {cargo_toml_path:?}: {e}"));

    // These deps must NOT appear in [dependencies] of fortuna-scoring — they
    // would introduce stochastic state into the pure scoring engine.
    const BANNED_DEPS: &[&str] = &["rand", "getrandom", "libm"];

    let mut violations: Vec<String> = Vec::new();
    for &dep in BANNED_DEPS {
        // Pattern: `dep = ` or `dep = {` on its own line in [dependencies].
        // We check for any occurrence of the dep name followed by a `=` (TOML
        // key) — this is conservative but sufficient given the dep list is tiny.
        let pattern = format!("{dep} =");
        if content.contains(&pattern) {
            violations.push(format!("fortuna-scoring/Cargo.toml: banned dep {dep:?}"));
        }
    }

    assert!(
        violations.is_empty(),
        "fortuna-scoring/Cargo.toml must not contain stochastic/IO deps. Violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn scoring_src_has_no_async_or_db_imports() {
    let root = workspace_root();
    let scoring_src = root.join("crates/fortuna-scoring/src");

    let files = collect_rs_files(&scoring_src);
    assert!(
        !files.is_empty(),
        "no .rs files found under {scoring_src:?} — check the path"
    );

    // These patterns must NOT appear in fortuna-scoring/src/.
    const BANNED_PATTERNS: &[&str] = &["sqlx::", "tokio::", "async fn"];

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        let content =
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("cannot read {file:?}: {e}"));

        for &pattern in BANNED_PATTERNS {
            if content.contains(pattern) {
                violations.push(format!(
                    "{}: contains banned pattern {pattern:?}",
                    file.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "fortuna-scoring/src/ must be sync + DB-free (no sqlx::, tokio::, async fn). \
         Violations:\n{}",
        violations.join("\n")
    );
}
