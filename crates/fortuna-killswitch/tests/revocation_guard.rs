//! Three-way revocation guard (I4 — the CLI re-arm precondition).
//!
//! `is_revoked` is `path.exists()` (lib.rs), which collapses BOTH "absent" and
//! "unreadable/unverifiable" to `false`. The operator re-arm path MUST NOT
//! negate `is_revoked` — that would ALLOW a re-arm when the sentinel's state is
//! unknowable (a `0o000` parent dir, a stat error), re-enabling order-placing
//! capability while the kill state is unverifiable. That is the exact opposite
//! of fail-closed.
//!
//! [`revocation_guard`] is the three-way check the re-arm precondition needs:
//!  - PRESENT (`is_revoked == true`)        → `Refuse` (a standing kill; I4).
//!  - ABSENT and readable (parent stat OK)  → `Allow`  (the normal happy path).
//!  - UNVERIFIABLE (`try_exists` errors)    → `Refuse` (FAIL CLOSED).
//!
//! These tests exercise the real filesystem (a `0o000` directory drives the
//! unverifiable branch) so they pin behavior that a unit test over a mocked
//! `exists` could not.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fortuna_killswitch::{revocation_guard, RevocationGuard};

/// A unique temp dir per test (parallel test binaries must not collide).
fn unique_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fortuna-rearm-guard-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id(),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn guard_allows_when_sentinel_absent_and_readable() {
    let dir = unique_dir("absent");
    let sentinel = dir.join("KILLSWITCH_REVOKED");
    // The file does not exist; the parent dir is readable.
    assert_eq!(
        revocation_guard(&sentinel),
        RevocationGuard::Allow,
        "an absent sentinel under a readable parent is the normal happy path"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn guard_refuses_when_sentinel_present() {
    let dir = unique_dir("present");
    let sentinel = dir.join("KILLSWITCH_REVOKED");
    std::fs::write(&sentinel, b"{\"revoked_at\":\"now\"}\n").unwrap();
    // The sentinel's PRESENCE is a standing kill (I4) — a re-arm must refuse.
    assert_eq!(
        revocation_guard(&sentinel),
        RevocationGuard::Refuse,
        "a present kill sentinel must REFUSE the re-arm (I4)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn guard_refuses_when_sentinel_unverifiable() {
    // The unverifiable branch: a sentinel whose PARENT dir is `0o000`, so a
    // stat/`try_exists` on the child errors with EACCES (it cannot be shown to
    // be either present or absent). This exercises the `try_exists` probe, NOT
    // `is_revoked` (which would silently report `false` and — if negated — ALLOW
    // the re-arm). The guard must FAIL CLOSED → Refuse.
    use std::os::unix::fs::PermissionsExt;

    let dir = unique_dir("unverifiable");
    let locked = dir.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    let sentinel = locked.join("KILLSWITCH_REVOKED");
    // Lock the parent: no read/execute, so the child cannot be stat'd.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    // Sanity: the std `exists()` (what `is_revoked` uses) reports `false` here —
    // proving that NEGATING is_revoked would wrongly ALLOW. The guard must NOT.
    let exists_says = sentinel.exists();

    let verdict = revocation_guard(&sentinel);

    // Restore perms BEFORE asserting so the temp dir always cleans up.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        !exists_says,
        "precondition: std exists() collapses unverifiable→false (that is the bug !is_revoked would inherit)"
    );
    assert_eq!(
        verdict,
        RevocationGuard::Refuse,
        "an UNVERIFIABLE sentinel (parent 0o000 → try_exists errors) must FAIL CLOSED → Refuse"
    );
}
