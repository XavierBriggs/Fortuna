//! Track E slice 1: the persona registry + the persisted domain-analysis
//! artifact (design §5). Tests written from the design text BEFORE the repos.
//! Each test gets an isolated, migrated database via #[sqlx::test].
//!
//! Headline guarantees (mutation-proven against the live database):
//!   - personas is append-only (UPDATE/DELETE refused) and refuses a version
//!     re-issue (UNIQUE persona_id, version); head() returns the newest version.
//!   - domain_analyses is content-immutable (the replay anchor, 5.7/I5): only
//!     `status` may flip open->superseded; every content field and DELETE are
//!     refused at the database.

use serde_json::json;
use sqlx::PgPool;

#[allow(clippy::too_many_arguments)]
async fn insert_persona(
    repo: &fortuna_ledger::PersonasRepo,
    row_id: &str,
    persona_id: &str,
    version: i32,
    status: &str,
    method_hash: &str,
    supersedes: Option<&str>,
) {
    repo.insert(
        row_id,
        persona_id,
        version,
        "weather",
        &json!(["temperature", "nyc"]),
        &json!(["aeolus.forecast", "nws.observed_high"]),
        "cheap",
        method_hash,
        "findings/v1",
        status,
        supersedes,
        "2026-06-13T00:00:00.000Z",
        "2026-06-13T00:00:00.000Z",
    )
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn personas_insert_and_head_returns_the_newest_version(pool: PgPool) {
    let repo = fortuna_ledger::PersonasRepo::new(pool);

    insert_persona(&repo, "p-1", "meteorologist", 1, "active", "hash-v1", None).await;
    // A method change is a NEW (persona_id, version) row superseding the old.
    insert_persona(
        &repo,
        "p-2",
        "meteorologist",
        2,
        "active",
        "hash-v2",
        Some("p-1"),
    )
    .await;

    let head = repo.head("meteorologist").await.unwrap().unwrap();
    assert_eq!(head.persona_row_id, "p-2");
    assert_eq!(head.version, 2);
    assert_eq!(head.method_hash, "hash-v2");
    assert_eq!(head.tier, "cheap");
    assert_eq!(head.status, "active");
    assert_eq!(head.supersedes.as_deref(), Some("p-1"));
    assert_eq!(
        head.reads_signal_kinds,
        json!(["aeolus.forecast", "nws.observed_high"])
    );

    assert!(
        repo.head("macro-economist").await.unwrap().is_none(),
        "no rows for an unknown persona -> None, never an error"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn personas_refuse_mutation_at_the_database(pool: PgPool) {
    let repo = fortuna_ledger::PersonasRepo::new(pool.clone());
    insert_persona(&repo, "p-1", "meteorologist", 1, "active", "hash-v1", None).await;

    // A registry row is append-only: the method_hash can never be edited in
    // place (a method change is a superseding row); UPDATE and DELETE are
    // refused by the I5 trigger.
    let update =
        sqlx::query("UPDATE personas SET method_hash = 'forged' WHERE persona_row_id = $1")
            .bind("p-1")
            .execute(&pool)
            .await;
    assert!(update.unwrap_err().to_string().contains("append-only"));

    let delete = sqlx::query("DELETE FROM personas WHERE persona_row_id = $1")
        .bind("p-1")
        .execute(&pool)
        .await;
    assert!(delete.unwrap_err().to_string().contains("append-only"));
}

#[sqlx::test(migrations = "./migrations")]
async fn personas_refuse_a_version_reissue(pool: PgPool) {
    let repo = fortuna_ledger::PersonasRepo::new(pool);
    insert_persona(&repo, "p-1", "meteorologist", 1, "active", "hash-v1", None).await;

    // Re-issuing (persona_id, version) with different content is refused by the
    // UNIQUE key — a version is a stable, scoreable identity.
    let reissue = repo
        .insert(
            "p-1-dup",
            "meteorologist",
            1,
            "weather",
            &json!([]),
            &json!([]),
            "cheap",
            "hash-different",
            "findings/v1",
            "active",
            None,
            "2026-06-13T00:00:00.000Z",
            "2026-06-13T00:00:00.000Z",
        )
        .await;
    // Pin that it is the UNIQUE(persona_id, version) key refusing, not an
    // incidental failure.
    let err = reissue.expect_err("re-issuing (persona_id, version) must fail");
    assert!(
        err.to_string().contains("duplicate key") || err.to_string().contains("unique"),
        "expected a unique-violation error, got: {err}"
    );
}

#[allow(clippy::too_many_arguments)]
async fn insert_analysis(
    repo: &fortuna_ledger::DomainAnalysesRepo,
    analysis_id: &str,
    region_key: &str,
    produced_at: &str,
    findings: serde_json::Value,
    content_hash: &str,
    supersedes: Option<&str>,
) {
    repo.insert(
        analysis_id,
        "meteorologist",
        3,
        "weather",
        region_key,
        produced_at,
        &json!([{"signal_id": "sig-1", "content_hash": "sh-1"}]),
        &findings,
        content_hash,
        "manifest-hash-1",
        1,
        supersedes,
        "2026-06-13T00:00:00.000Z",
    )
    .await
    .unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn domain_analyses_insert_round_trips_and_current_returns_the_open_head(pool: PgPool) {
    let repo = fortuna_ledger::DomainAnalysesRepo::new(pool);
    let region = "weather:KNYC:tmax:2026-06-12";
    let findings = json!({"thresholds": [{"ge": 60, "p": 0.92}], "sigma_trend": "tightening"});

    insert_analysis(
        &repo,
        "a-1",
        region,
        "2026-06-12T05:00:00.000Z",
        findings.clone(),
        "ch-1",
        None,
    )
    .await;

    let got = repo.get("a-1").await.unwrap();
    assert_eq!(got.persona_id, "meteorologist");
    assert_eq!(got.persona_version, 3);
    assert_eq!(got.region_key, region);
    assert_eq!(got.findings, findings);
    assert_eq!(
        got.signal_manifest,
        json!([{"signal_id": "sig-1", "content_hash": "sh-1"}])
    );
    assert_eq!(got.content_hash, "ch-1");
    assert_eq!(got.cost_cents, 1);
    assert_eq!(got.status, "open");

    let current = repo
        .current_for_region("weather", region)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.analysis_id, "a-1");

    assert!(
        repo.current_for_region("weather", "weather:KSEA:tmax:2026-06-12")
            .await
            .unwrap()
            .is_none(),
        "no analysis for a region -> None, never an error"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn domain_analyses_content_is_immutable_only_status_may_change(pool: PgPool) {
    let repo = fortuna_ledger::DomainAnalysesRepo::new(pool.clone());
    insert_analysis(
        &repo,
        "a-1",
        "weather:KNYC:tmax:2026-06-12",
        "2026-06-12T05:00:00.000Z",
        json!({"thresholds": []}),
        "ch-1",
        None,
    )
    .await;

    // The findings (the replay anchor) can never be rewritten in place.
    let mutate_findings = sqlx::query(
        "UPDATE domain_analyses SET findings = '{\"p\":0.99}'::jsonb WHERE analysis_id = $1",
    )
    .bind("a-1")
    .execute(&pool)
    .await;
    assert!(mutate_findings
        .unwrap_err()
        .to_string()
        .contains("immutable"));

    // Nor the content hash.
    let mutate_hash =
        sqlx::query("UPDATE domain_analyses SET content_hash = 'forged' WHERE analysis_id = $1")
            .bind("a-1")
            .execute(&pool)
            .await;
    assert!(mutate_hash.unwrap_err().to_string().contains("immutable"));

    // Nor a "boring" non-anchor column: the guard freezes EVERY content column,
    // not just the obvious replay-anchor fields. This pins the full guard chain
    // so a future regression that drops a column from it is caught.
    let mutate_cost =
        sqlx::query("UPDATE domain_analyses SET cost_cents = 999 WHERE analysis_id = $1")
            .bind("a-1")
            .execute(&pool)
            .await;
    assert!(mutate_cost.unwrap_err().to_string().contains("immutable"));

    // DELETE is refused outright.
    let delete = sqlx::query("DELETE FROM domain_analyses WHERE analysis_id = $1")
        .bind("a-1")
        .execute(&pool)
        .await;
    assert!(delete.unwrap_err().to_string().contains("append-only"));

    // The supersession marker `status` MAY flip (the one allowed change).
    sqlx::query("UPDATE domain_analyses SET status = 'superseded' WHERE analysis_id = $1")
        .bind("a-1")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(repo.get("a-1").await.unwrap().status, "superseded");
}

#[sqlx::test(migrations = "./migrations")]
async fn domain_analyses_supersession_flips_the_prior_and_current_tracks_the_head(pool: PgPool) {
    let repo = fortuna_ledger::DomainAnalysesRepo::new(pool);
    let region = "weather:KNYC:tmax:2026-06-12";

    insert_analysis(
        &repo,
        "a-1",
        region,
        "2026-06-12T05:00:00.000Z",
        json!({"v": 1}),
        "ch-1",
        None,
    )
    .await;
    // A fresh analysis for the same region supersedes the prior one.
    insert_analysis(
        &repo,
        "a-2",
        region,
        "2026-06-12T11:00:00.000Z",
        json!({"v": 2}),
        "ch-2",
        Some("a-1"),
    )
    .await;

    assert_eq!(repo.get("a-1").await.unwrap().status, "superseded");
    let a2 = repo.get("a-2").await.unwrap();
    assert_eq!(a2.status, "open");
    assert_eq!(a2.supersedes.as_deref(), Some("a-1"));

    let current = repo
        .current_for_region("weather", region)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.analysis_id, "a-2", "current = the one open head");
}
