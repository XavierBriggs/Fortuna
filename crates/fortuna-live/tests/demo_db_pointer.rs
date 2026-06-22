//! W4 verifier fix — Important 2: daemon-boot pointer-write.
//!
//! F11 spec: the **daemon** writes the live DATABASE_URL to
//! `{runtime_dir}/current-demo-db-url` on boot, NOT the CLI.
//!
//! These tests prove:
//!   1. `maybe_write_demo_db_pointer(runtime_dir, db_url, PaperLedger)` WRITES
//!      the pointer file.
//!   2. `maybe_write_demo_db_pointer(runtime_dir, db_url, <other_mode>)` is a
//!      no-op — the file is NOT created.
//!   3. The write is idempotent / atomic: a second call with a different URL
//!      overwrites the previous one cleanly.
//!
//! The function under test lives in `fortuna_live::boot`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fortuna_live::boot::{maybe_write_demo_db_pointer, ExecutionMode};

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fortuna-demo-ptr-daemon-test-{}-{tag}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// PaperLedger mode MUST write `current-demo-db-url` containing the DB URL.
#[test]
fn paper_ledger_writes_pointer_on_daemon_boot() {
    let runtime_dir = scratch("paper-writes");
    let db_url = "postgres://fortuna_app@localhost/fortuna_demo";

    maybe_write_demo_db_pointer(&runtime_dir, db_url, ExecutionMode::PaperLedger)
        .expect("must not fail on a writable directory");

    let pointer = runtime_dir.join("current-demo-db-url");
    assert!(
        pointer.exists(),
        "current-demo-db-url must exist after daemon boot in PaperLedger mode"
    );
    let contents = std::fs::read_to_string(&pointer).expect("should read pointer file");
    assert_eq!(
        contents.trim(),
        db_url,
        "pointer file must contain exactly the connected DATABASE_URL"
    );

    let _ = std::fs::remove_dir_all(&runtime_dir);
}

/// Non-PaperLedger modes must NOT write the pointer (the file must not appear).
#[test]
fn non_paper_ledger_modes_do_not_write_pointer() {
    for mode in [
        ExecutionMode::LiveDataOnly,
        ExecutionMode::DryRun,
        ExecutionMode::DemoOrders,
        ExecutionMode::ProductionOrders,
    ] {
        let runtime_dir = scratch(&format!("no-write-{}", mode.as_str()));
        let db_url = "postgres://fortuna_app@localhost/fortuna";

        maybe_write_demo_db_pointer(&runtime_dir, db_url, mode)
            .expect("must not error even for non-paper modes");

        let pointer = runtime_dir.join("current-demo-db-url");
        assert!(
            !pointer.exists(),
            "{} must NOT write current-demo-db-url (only PaperLedger writes it)",
            mode.as_str()
        );

        let _ = std::fs::remove_dir_all(&runtime_dir);
    }
}

/// Idempotent overwrite: a second PaperLedger boot with a different URL
/// replaces the pointer atomically.
#[test]
fn pointer_write_is_idempotent_and_overwrites() {
    let runtime_dir = scratch("idempotent");
    let url1 = "postgres://fortuna_app@localhost/fortuna_first";
    let url2 = "postgres://fortuna_app@localhost/fortuna_second";

    maybe_write_demo_db_pointer(&runtime_dir, url1, ExecutionMode::PaperLedger)
        .expect("first write");
    maybe_write_demo_db_pointer(&runtime_dir, url2, ExecutionMode::PaperLedger)
        .expect("second write");

    let pointer = runtime_dir.join("current-demo-db-url");
    let contents = std::fs::read_to_string(&pointer).unwrap();
    assert_eq!(
        contents.trim(),
        url2,
        "second write must atomically overwrite the first"
    );

    let _ = std::fs::remove_dir_all(&runtime_dir);
}
