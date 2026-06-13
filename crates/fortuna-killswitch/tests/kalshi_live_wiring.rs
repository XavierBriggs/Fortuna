//! T4.2 item 2(iv) — kill-switch Kalshi LIVE-wiring, the FAIL-CLOSED rails.
//!
//! The freeze MACHINERY over the real `KalshiVenue` is proven in
//! `kalshi_freeze.rs` (mock transport, no live socket). THIS file proves the
//! LIVE `freeze --venue kalshi` path is fail-closed:
//!   - the credential loader is env-only, rejects any missing/empty field, names
//!     the ENV VAR (never a secret value), and requires the base URL explicitly
//!     (prod vs demo is never defaulted — the switch must not cancel on the wrong
//!     environment);
//!   - the BINARY refuses a kalshi freeze without complete credentials and never
//!     reaches the venue (no live cancel attempted).
//!
//! The actual live freeze over a real socket is OPERATOR-run after the 27-item
//! paper clearance (now signed); it is never fabricated here.

use fortuna_killswitch::load_kalshi_creds;

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
}

#[test]
fn all_credentials_absent_is_fail_closed() {
    let err = load_kalshi_creds(None, None, None).unwrap_err();
    assert!(
        err.contains("FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID"),
        "must name the first missing var: {err}"
    );
}

#[test]
fn a_missing_private_key_path_is_fail_closed() {
    let err = load_kalshi_creds(Some("k".into()), None, Some("https://x".into())).unwrap_err();
    assert!(
        err.contains("FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH"),
        "{err}"
    );
}

#[test]
fn a_missing_base_url_is_fail_closed_prod_vs_demo_must_be_explicit() {
    let pem = temp("ks-pem-baseurl.pem");
    std::fs::write(&pem, "PEM").unwrap();
    let err = load_kalshi_creds(
        Some("k".into()),
        Some(pem.to_string_lossy().into_owned()),
        None,
    )
    .unwrap_err();
    assert!(
        err.contains("FORTUNA_KILLSWITCH_KALSHI_BASE_URL"),
        "base url must be explicit, never defaulted: {err}"
    );
    let _ = std::fs::remove_file(&pem);
}

#[test]
fn empty_string_values_are_treated_as_absent() {
    // Defensive: even if an env var is set to "" the loader rejects it (main.rs
    // also filters empty → None, but the loader is the durable guard).
    let err = load_kalshi_creds(
        Some("   ".into()),
        Some("/x".into()),
        Some("https://x".into()),
    )
    .unwrap_err();
    assert!(
        err.contains("FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID"),
        "{err}"
    );
}

#[test]
fn an_unreadable_private_key_file_is_fail_closed_without_leaking_a_secret() {
    let err = load_kalshi_creds(
        Some("k".into()),
        Some("/nonexistent/dir/ks-key.pem".into()),
        Some("https://demo-api.kalshi.co/trade-api/v2".into()),
    )
    .unwrap_err();
    // Names the env var / path, surfaces a read failure — and carries no key material.
    assert!(
        err.contains("FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH"),
        "{err}"
    );
}

#[test]
fn an_empty_private_key_file_is_fail_closed() {
    let pem = temp("ks-pem-empty.pem");
    std::fs::write(&pem, "   \n").unwrap();
    let err = load_kalshi_creds(
        Some("k".into()),
        Some(pem.to_string_lossy().into_owned()),
        Some("https://x".into()),
    )
    .unwrap_err();
    assert!(err.to_lowercase().contains("empty"), "{err}");
    let _ = std::fs::remove_file(&pem);
}

#[test]
fn complete_credentials_load_with_the_pem_contents() {
    let pem = temp("ks-pem-ok.pem");
    std::fs::write(
        &pem,
        "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----\n",
    )
    .unwrap();
    let creds = load_kalshi_creds(
        Some("key-123".into()),
        Some(pem.to_string_lossy().into_owned()),
        Some("https://demo-api.kalshi.co/trade-api/v2".into()),
    )
    .unwrap();
    assert_eq!(creds.api_key_id, "key-123");
    assert!(creds.private_key_pem.contains("BEGIN PRIVATE KEY"));
    assert_eq!(creds.base_url, "https://demo-api.kalshi.co/trade-api/v2");
    let _ = std::fs::remove_file(&pem);
}

#[test]
fn debug_never_leaks_the_private_key() {
    // Secret-safety: a KalshiCreds must never print the PEM via `{:?}` (no
    // secrets in logs / panics / audit). MUTATION check — break the redacting
    // Debug impl and this test reds.
    let pem = temp("ks-pem-debug.pem");
    std::fs::write(
        &pem,
        "-----BEGIN PRIVATE KEY-----\nSUPER_SECRET_KEY_MATERIAL\n-----END PRIVATE KEY-----\n",
    )
    .unwrap();
    let creds = load_kalshi_creds(
        Some("key-123".into()),
        Some(pem.to_string_lossy().into_owned()),
        Some("https://x".into()),
    )
    .unwrap();
    let dbg = format!("{creds:?}");
    assert!(
        !dbg.contains("SUPER_SECRET_KEY_MATERIAL"),
        "Debug leaked the private key: {dbg}"
    );
    assert!(dbg.contains("redacted"), "{dbg}");
    // The non-secret identifiers ARE shown (useful for diagnostics).
    assert!(dbg.contains("key-123"), "{dbg}");
    let _ = std::fs::remove_file(&pem);
}

/// The binary itself refuses a kalshi freeze without credentials — fail-closed,
/// non-zero exit, names the missing var, and NEVER reaches the venue (no live
/// cancel attempted, no freeze journal line written). Mirrors the i4 operational
/// test's subprocess pattern.
#[test]
fn the_binary_refuses_a_kalshi_freeze_without_credentials() {
    let journal = temp("ks-live-refuse.jsonl");
    let _ = std::fs::remove_file(&journal);
    let run = std::process::Command::new(env!("CARGO"))
        .args([
            "run",
            "-q",
            "-p",
            "fortuna-killswitch",
            "--",
            "freeze",
            "--venue",
            "kalshi",
            "--journal",
        ])
        .arg(&journal)
        .env_remove("FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID")
        .env_remove("FORTUNA_KILLSWITCH_KALSHI_PRIVATE_KEY_PATH")
        .env_remove("FORTUNA_KILLSWITCH_KALSHI_BASE_URL")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        !run.status.success(),
        "the switch must REFUSE a live freeze without credentials"
    );
    // Specifically the credential-refusal exit code (4) — not a generic error
    // (1) or compile failure — so a regression that fails for the wrong reason
    // is caught.
    assert_eq!(
        run.status.code(),
        Some(4),
        "credential refusal must exit 4; stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("FORTUNA_KILLSWITCH_KALSHI_API_KEY_ID"),
        "fail-closed message must name the missing var; stderr: {stderr}"
    );
    // No live freeze was attempted: the journal has no freeze-started line.
    let no_freeze = std::fs::read_to_string(&journal)
        .map(|s| !s.contains("freeze_and_cancel_started"))
        .unwrap_or(true);
    assert!(no_freeze, "no live cancel path may run without credentials");
    let _ = std::fs::remove_file(&journal);
}
