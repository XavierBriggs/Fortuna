//! T4.3 ROTA slice 1 tests: the read-only route table (T-1) and the
//! capability-degraded path (R1: every surface is HTTP 200, never 500,
//! when Postgres/recorder capabilities are absent and views are empty).
//! Served against a real loopback listener — no daemon, no Postgres.

use fortuna_core::clock::UtcTimestamp;
use fortuna_ops::dashboard::DashboardSnapshot;
use fortuna_ops::rota::{audit_tail_page, rota_router, scan_recorder, RotaState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// File mtime in epoch ms — the recorder-scan's freshness clock is the file
/// system, never the wall clock; the test reads the same metadata the scan
/// reads so the asserted ages are exact.
fn mtime_ms(p: &Path) -> i64 {
    std::fs::metadata(p)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Build a unique temp base with today/<stream>.jsonl files; returns
/// (base, now_ms_probe, today). "today" is derived from a probe file's real
/// mtime so the test is date-independent (deterministic except within ~200s
/// before UTC midnight, which the production rollover handles separately).
fn temp_perishable(tag: &str) -> (PathBuf, i64, String) {
    let base = std::env::temp_dir().join(format!("fortuna-rota-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let probe = base.join("probe");
    std::fs::write(&probe, b"x").unwrap();
    let now_ms = mtime_ms(&probe);
    let today = UtcTimestamp::from_epoch_millis(now_ms)
        .unwrap()
        .to_iso8601()[0..10]
        .to_string();
    let day = base.join(&today);
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("a_stream.jsonl"), b"l1\nl2\n").unwrap();
    std::fs::write(day.join("b_stream.jsonl"), b"x\n").unwrap();
    (base, now_ms, today)
}

fn empty_snapshot() -> Arc<RwLock<DashboardSnapshot>> {
    Arc::new(RwLock::new(DashboardSnapshot {
        generated_at: "2026-06-11T12:00:00.000Z".to_string(),
        stage: "sim".to_string(),
        metrics_text: String::new(),
        boards: serde_json::json!({}),
        views: serde_json::json!({}),
    }))
}

async fn serve() -> (String, tokio::task::JoinHandle<()>) {
    let state = RotaState::standalone(empty_snapshot());
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), handle)
}

const PATHS: [&str; 26] = [
    "/rota",
    "/favicon.ico",
    "/assets/rota/logo.svg",
    "/api/rota/v1/health",
    "/api/rota/v1/money",
    "/api/rota/v1/gates",
    "/api/rota/v1/cognition",
    "/api/rota/v1/settlement",
    "/api/rota/v1/streams",
    "/api/rota/v1/build",
    "/api/rota/v1/ingest_sources",
    "/api/rota/v1/ingest_feed",
    "/api/rota/v1/ingest_funnel",
    "/api/rota/v1/fills",
    "/api/rota/v1/strategies",
    "/api/rota/v1/working_orders",
    "/api/rota/v1/discovery",
    "/api/rota/v1/personas",
    "/api/rota/v1/persona_scores",
    "/api/rota/v1/persona_pipeline",
    "/api/rota/v1/analyses",
    "/api/rota/v1/forecasts",
    "/api/rota/v1/forecast_feed",
    "/api/rota/v1/db",
    "/api/rota/v1/telemetry",
    "/api/rota/v1/audit",
];

#[tokio::test]
async fn every_path_is_get_only_and_200() {
    let (base, _h) = serve().await;
    let client = reqwest::Client::new();
    for path in PATHS {
        let url = format!("{base}{path}");
        let get = client.get(&url).send().await.unwrap();
        assert_eq!(get.status(), 200, "GET {path} must be 200");

        // Read-only doctrine: every mutating method is 405 (Method Not
        // Allowed), structurally — there is nothing to mutate.
        for method in [
            reqwest::Method::POST,
            reqwest::Method::PUT,
            reqwest::Method::DELETE,
            reqwest::Method::PATCH,
        ] {
            let resp = client
                .request(method.clone(), &url)
                .body("x")
                .send()
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                405,
                "{method} {path} must be 405 (read-only)"
            );
        }
    }
}

#[tokio::test]
async fn degraded_surfaces_are_200_with_explicit_unavailable() {
    let (base, _h) = serve().await;
    let client = reqwest::Client::new();

    // Empty views => each metric surface reports unavailable, not 500.
    for name in [
        "health",
        "money",
        "gates",
        "settlement",
        "streams",
        "ingest_sources",
        "ingest_feed",
        "ingest_funnel",
        "fills",
        "strategies",
        "working_orders",
        "discovery",
        "personas",
        "persona_scores",
        "persona_pipeline",
        "analyses",
        "forecasts",
        "db",
        "telemetry",
    ] {
        let j: serde_json::Value = client
            .get(format!("{base}/api/rota/v1/{name}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            j["status"], "unavailable",
            "{name} with empty views reports unavailable"
        );
    }

    // No Postgres => the audit tail is an explicit empty page.
    let audit: serde_json::Value = client
        .get(format!("{base}/api/rota/v1/audit"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(audit["available"], false);
    assert!(audit["rows"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn populated_view_is_served_verbatim() {
    let snap = empty_snapshot();
    {
        let mut s = snap.write().await;
        s.views = serde_json::json!({
            "health": { "halt_active": false, "ticks_total": 42,
                        "fill_latency_p90_ms": 14 }
        });
    }
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(j["ticks_total"], 42);
    assert_eq!(j["fill_latency_p90_ms"], 14);
}

// D-contract V2 Sources Health: the handler serves the daemon-shaped board
// envelope verbatim. POPULATED-path (real rows seeded) — a vacuous "renders
// empty" test would not satisfy the DoD. Asserts the generic {columns,rows,
// summary} envelope round-trips so the frontend boardTable renderer has real
// data to render.
#[tokio::test]
async fn ingest_sources_board_serves_seeded_rows() {
    let snap = empty_snapshot();
    {
        let mut s = snap.write().await;
        s.views = serde_json::json!({
            "ingest_sources": {
                "title": "Sources Health",
                "columns": [
                    {"key":"source_id","label":"Source"},
                    {"key":"health","label":"Health"},
                    {"key":"accepted","label":"Acc"},
                    {"key":"dropped_over_volume","label":"D:vol"}
                ],
                "rows": [
                    {"source_id":"nws_alerts","health":"healthy","accepted":58,"dropped_over_volume":0},
                    {"source_id":"nws_afd","health":"degraded","accepted":12,"dropped_over_volume":171}
                ],
                "summary": {"healthy":1,"degraded":1,"quarantined":0}
            }
        });
    }
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/ingest_sources"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both seeded sources served");
    assert_eq!(j["rows"][0]["source_id"], "nws_alerts");
    assert_eq!(j["rows"][1]["health"], "degraded");
    // The AFD-firehose (huge over-volume drop) the V2 board exists to surface.
    assert_eq!(j["rows"][1]["dropped_over_volume"], 171);
    assert_eq!(j["summary"]["degraded"], 1);
}

// D-contract V1 Live Signal Feed: the marquee feed board serves the recent
// SignalRecords newest-first. POPULATED-path — asserts real seeded rows incl. an
// untrusted summary and a drop status, served verbatim for the boardTable
// renderer (which esc()'s the untrusted summary, spec 5.11).
#[tokio::test]
async fn ingest_feed_board_serves_seeded_signals() {
    let snap = empty_snapshot();
    {
        let mut s = snap.write().await;
        s.views = serde_json::json!({
            "ingest_feed": {
                "title": "Live Signal Feed",
                "columns": [
                    {"key":"at","label":"Time (UTC)"},
                    {"key":"source_id","label":"Source"},
                    {"key":"status","label":"Status","pill":true},
                    {"key":"summary","label":"Data"}
                ],
                "rows": [
                    {"at":"2026-06-13T12:34:58Z","source_id":"nws_alerts","status":"accepted",
                     "summary":"Severe Thunderstorm Warning — Kings County NY"},
                    {"at":"2026-06-13T12:34:55Z","source_id":"nws_afd","status":"dropped:over_volume",
                     "summary":"AFD over per-poll volume cap"}
                ],
                "summary": {"window":2,"accepted":1,"dropped":1}
            }
        });
    }
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/ingest_feed"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both seeded signals served");
    assert_eq!(j["rows"][0]["source_id"], "nws_alerts");
    assert_eq!(j["rows"][0]["status"], "accepted");
    // The untrusted summary is carried verbatim as DATA (the renderer esc()s it).
    assert_eq!(j["rows"][1]["status"], "dropped:over_volume");
    assert_eq!(j["summary"]["dropped"], 1);
}

// D-contract V3 Ingest Funnel: the stage table serves the pipeline funnel with
// retention + drop-offs. POPULATED-path — asserts real stage rows (where signal
// is lost), not a vacuous empty funnel.
#[tokio::test]
async fn ingest_funnel_board_serves_seeded_stages() {
    let snap = empty_snapshot();
    {
        let mut s = snap.write().await;
        s.views = serde_json::json!({
            "ingest_funnel": {
                "title": "Ingest Funnel",
                "columns": [
                    {"key":"stage","label":"Stage"},
                    {"key":"count","label":"Count"},
                    {"key":"retain_pct","label":"Retain %"},
                    {"key":"dropped","label":"Dropped"}
                ],
                "rows": [
                    {"stage":"Fetched","count":1240,"retain_pct":100,"dropped":0},
                    {"stage":"Validated","count":1052,"retain_pct":85,"dropped":188},
                    {"stage":"Persisted","count":1048,"retain_pct":85,"dropped":4}
                ],
                "summary": {"fetched":1240,"persisted":1048,"retain_pct":85}
            }
        });
    }
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/ingest_funnel"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "all funnel stages served");
    assert_eq!(j["rows"][0]["stage"], "Fetched");
    // The validate stage is where 188 items were refused by Layer-1.
    assert_eq!(j["rows"][1]["dropped"], 188);
    assert_eq!(j["summary"]["persisted"], 1048);
}

#[tokio::test]
async fn shell_is_gold_on_black_html() {
    let (base, _h) = serve().await;
    let body = reqwest::get(format!("{base}/rota"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("FORTUNA"));
    assert!(body.contains("#D4AF37"), "gold token present");
    assert!(body.contains("#0A0A0B"), "black background token present");
    assert!(body.contains("SYSTEM HALTED"), "halt takeover present");
}

// ---- T4.3 slice 4: the streams recorder filesystem-scan ----

#[test]
fn scan_recorder_stats_todays_streams_cheaply_and_flags_staleness() {
    let (base, now_ms, _today) = temp_perishable("scan");

    // FRESH: generated_at = now + 5s => ages ~5s, both streams healthy.
    let gen_fresh = UtcTimestamp::from_epoch_millis(now_ms + 5_000)
        .unwrap()
        .to_iso8601();
    let rec = scan_recorder(&base, &gen_fresh);
    let arr = rec.as_array().expect("recorder is an array");
    assert_eq!(arr.len(), 2, "one entry per jsonl stream: {rec}");
    assert_eq!(arr[0]["stream"], "a_stream", "sorted by name");
    assert_eq!(arr[1]["stream"], "b_stream");
    assert_eq!(arr[0]["healthy"], true);
    assert!(arr[0]["last_capture_age_secs"].as_i64().unwrap() < 120);
    // size is a cheap metadata read (NOT a content line-count): the bytes on
    // disk, exactly.
    assert_eq!(arr[0]["size_bytes"], 6, "\"l1\\nl2\\n\" is 6 bytes: {rec}");
    assert_eq!(arr[1]["size_bytes"], 2);
    // rows_today / key_count are deliberately ABSENT — counting them means
    // reading file CONTENT (a stream file is multi-GB); deferred, never faked.
    assert!(arr[0].get("rows_today").is_none());
    assert!(arr[0].get("key_count").is_none());

    // STALE: generated_at = now + 200s => age >= 120 => unhealthy.
    let gen_old = UtcTimestamp::from_epoch_millis(now_ms + 200_000)
        .unwrap()
        .to_iso8601();
    let rec2 = scan_recorder(&base, &gen_old);
    assert_eq!(rec2.as_array().unwrap()[0]["healthy"], false, "{rec2}");
    assert!(
        rec2.as_array().unwrap()[0]["last_capture_age_secs"]
            .as_i64()
            .unwrap()
            >= 120
    );

    // MISSING today-dir => empty array, never a panic, never a 500.
    let absent = base.join("no-such-base");
    assert_eq!(
        scan_recorder(&absent, &gen_fresh).as_array().unwrap().len(),
        0
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn scan_recorder_rejects_a_malformed_generated_at_never_faking_healthy() {
    // audit-tail-fix gate finding #1: a VALID date prefix (so today's dir is
    // found) with an UNPARSEABLE instant must NOT fake healthy. The old
    // `parse_iso8601(...).unwrap_or(0)` then `.max(0)` clamped age to 0 =>
    // healthy:true on garbage. Degraded-never-faked: unknown clock => unhealthy
    // + null age.
    let (base, _now_ms, today) = temp_perishable("malformed");
    let bad = format!("{today}TGARBAGE-NOT-A-TIMESTAMP");
    let rec = scan_recorder(&base, &bad);
    let arr = rec.as_array().expect("recorder array");
    assert_eq!(arr.len(), 2, "today's streams are still listed: {rec}");
    for s in arr {
        assert_eq!(
            s["healthy"], false,
            "a malformed clock must read unhealthy, NEVER faked-healthy: {s}"
        );
        assert!(
            s["last_capture_age_secs"].is_null(),
            "age is unknown on a bad clock, not a fabricated 0: {s}"
        );
        // size is clock-INDEPENDENT metadata — still honestly reported.
        assert!(s["size_bytes"].is_number(), "{s}");
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn streams_handler_merges_recorder_when_perishable_dir_present() {
    let (base, now_ms, _today) = temp_perishable("handler");
    let gen = UtcTimestamp::from_epoch_millis(now_ms + 5_000)
        .unwrap()
        .to_iso8601();

    // The daemon-shaped venue half of the streams view (slice 2 shape).
    let snap = Arc::new(RwLock::new(DashboardSnapshot {
        generated_at: gen,
        stage: "sim".to_string(),
        metrics_text: String::new(),
        boards: serde_json::json!({}),
        views: serde_json::json!({ "streams": { "venue_api_errors_total": 4 } }),
    }));
    let state = RotaState {
        snapshot: snap,
        pool: None,
        perishable_dir: Some(Arc::new(base.clone())),
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/streams"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // The daemon-shaped venue scalar SURVIVES the merge...
    assert_eq!(j["venue_api_errors_total"], 4, "{j}");
    // ...and the recorder liveness array is merged in from the filesystem.
    let rec = j["recorder"].as_array().expect("recorder merged");
    assert_eq!(rec.len(), 2, "{j}");
    assert_eq!(rec[0]["stream"], "a_stream");
    assert_eq!(rec[0]["healthy"], true);

    h.abort();
    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn streams_handler_omits_recorder_without_perishable_capability() {
    // Standalone (no perishable_dir): the recorder array is ABSENT — the
    // panel degrades, never fabricates a scan it cannot perform.
    let snap = Arc::new(RwLock::new(DashboardSnapshot {
        generated_at: "2026-06-11T12:00:00.000Z".to_string(),
        stage: "sim".to_string(),
        metrics_text: String::new(),
        boards: serde_json::json!({}),
        views: serde_json::json!({ "streams": { "venue_api_errors_total": 0 } }),
    }));
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/streams"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(j["venue_api_errors_total"], 0);
    assert!(
        j.get("recorder").is_none(),
        "no capability => no recorder key"
    );
    h.abort();
}

// ---- rota-slices gate F1 (MAJOR): audit-tail cursor pagination ----

async fn insert_audit(pool: &sqlx::PgPool, audit_id: &str, kind: &str) {
    sqlx::query(
        "INSERT INTO audit (audit_id, at, kind, actor, ref_id, payload) \
         VALUES ($1, $2, $3, NULL, NULL, '{}'::jsonb)",
    )
    .bind(audit_id)
    .bind("2026-06-11T12:00:00.000Z")
    .bind(kind)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn audit_tail_cursorless_returns_the_latest_page_not_the_oldest(pool: sqlx::PgPool) {
    // Insert oldest -> newest (ULID audit_id == chronological).
    for (id, kind) in [
        ("01AAAAAAAAAAAAAAAAAAAAAAA0", "old1"),
        ("01BBBBBBBBBBBBBBBBBBBBBBB0", "old2"),
        ("01CCCCCCCCCCCCCCCCCCCCCCC0", "mid"),
        ("01DDDDDDDDDDDDDDDDDDDDDDD0", "new1"),
        ("01EEEEEEEEEEEEEEEEEEEEEEE0", "new2"),
    ] {
        insert_audit(&pool, id, kind).await;
    }

    // ABSENT CURSOR => the LATEST page (the tail's newest rows), chronological
    // ASC — NOT the oldest rows ever (the F1 defect). This is the verdict's
    // required absent-cursor coverage.
    let page = audit_tail_page(&pool, None, 2).await.unwrap();
    assert_eq!(page.len(), 2, "{page:?}");
    assert_eq!(
        page[0].0, "01DDDDDDDDDDDDDDDDDDDDDDD0",
        "latest page is the NEWEST rows, returned ASC: {page:?}"
    );
    assert_eq!(page[1].0, "01EEEEEEEEEEEEEEEEEEEEEEE0");
    // Regression guard: the F1 bug returned the OLDEST page (01AAA/01BBB).
    assert_ne!(
        page[0].0, "01AAAAAAAAAAAAAAAAAAAAAAA0",
        "cursorless must NOT return the oldest page (gate F1)"
    );

    // PRESENT CURSOR => the next rows strictly after it (forward pagination).
    let fwd = audit_tail_page(&pool, Some("01BBBBBBBBBBBBBBBBBBBBBBB0"), 2)
        .await
        .unwrap();
    assert_eq!(fwd[0].0, "01CCCCCCCCCCCCCCCCCCCCCCC0", "{fwd:?}");
    assert_eq!(fwd[1].0, "01DDDDDDDDDDDDDDDDDDDDDDD0");

    // Empty cursorless page is fine when nothing is after the newest row.
    let past_end = audit_tail_page(&pool, Some("01EEEEEEEEEEEEEEEEEEEEEEE0"), 2)
        .await
        .unwrap();
    assert!(
        past_end.is_empty(),
        "nothing after the newest: {past_end:?}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn audit_tail_empty_table_is_an_empty_page_not_an_error(pool: sqlx::PgPool) {
    assert!(audit_tail_page(&pool, None, 100).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn audit_handler_serves_the_live_tail_when_a_pool_is_present(pool: sqlx::PgPool) {
    // R5: with a (dedicated) pool the /audit handler serves the LIVE tail —
    // the available:true path END-TO-END through HTTP (only the degraded
    // pool=None path had coverage). Also pins F1 cursorless-latest at the
    // handler layer, not just the query fn.
    for (id, kind) in [
        ("01AAAAAAAAAAAAAAAAAAAAAAA0", "old"),
        ("01BBBBBBBBBBBBBBBBBBBBBBB0", "mid"),
        ("01CCCCCCCCCCCCCCCCCCCCCCC0", "new"),
    ] {
        insert_audit(&pool, id, kind).await;
    }
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/audit"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(j["available"], true, "pool present => live tail: {j}");
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{j}");
    // cursorless => the NEWEST row is in the page (F1, through the handler).
    assert_eq!(
        j["next_after"], "01CCCCCCCCCCCCCCCCCCCCCCC0",
        "next_after = the newest id in the page: {j}"
    );
    assert!(
        rows.iter()
            .any(|r| r["audit_id"] == "01CCCCCCCCCCCCCCCCCCCCCCC0"),
        "the live tail includes the newest row: {j}"
    );
    h.abort();
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn exhausted_rota_pool_degrades_to_200_while_the_writer_is_unimpeded(
    pool_opts: sqlx::postgres::PgPoolOptions,
    conn_opts: sqlx::postgres::PgConnectOptions,
) {
    // r5-pool gate, finding #1: pin R5's saturation/isolation property so a
    // future refactor cannot silently merge the reader/writer pools back. The
    // ROTA reader is bounded to 2 connections with a short acquire_timeout (the
    // daemon uses 3s; 1s here proves degraded-not-hung fast); the audit WRITER
    // is a SEPARATE pool. When the reader is saturated, the dashboard degrades
    // (HTTP 200, never 500/hung) while the writer's audit append proceeds
    // UNIMPEDED — exactly what connect_readonly_pool buys (never the writer's
    // pool).
    use std::time::{Duration, Instant};
    let rota_pool = pool_opts
        .clone()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(1))
        .connect_with(conn_opts.clone())
        .await
        .unwrap();
    let writer_pool = pool_opts.connect_with(conn_opts).await.unwrap();
    insert_audit(&writer_pool, "01AAAAAAAAAAAAAAAAAAAAAAA0", "seed").await;

    // SATURATE the reader: hold both of its connections for the whole test.
    let _c1 = rota_pool.acquire().await.unwrap();
    let _c2 = rota_pool.acquire().await.unwrap();

    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(rota_pool.clone()),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    // Concurrently: the dashboard read DEGRADES (bounded) while the audit
    // writer PROCEEDS — the isolation property.
    let read = async {
        let t = Instant::now();
        let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/audit"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        (j, t.elapsed())
    };
    let write = async {
        let t = Instant::now();
        insert_audit(&writer_pool, "01BBBBBBBBBBBBBBBBBBBBBBB0", "concurrent").await;
        t.elapsed()
    };
    let ((j, read_dur), write_dur) = tokio::join!(read, write);

    // Exhausted reader => HTTP 200, available:false (degraded like no-pool),
    // bounded — never hung, never 500.
    assert_eq!(j["available"], false, "saturated reader pool degrades: {j}");
    assert!(j["rows"].as_array().unwrap().is_empty(), "{j}");
    assert!(
        read_dur < Duration::from_secs(3),
        "bounded by acquire_timeout, not hung: {read_dur:?}"
    );
    // ISOLATION: the audit writer was UNIMPEDED by the dashboard saturation
    // (distinct pools) — fast, and the row actually committed.
    assert!(
        write_dur < Duration::from_secs(1),
        "the audit writer proceeds despite ROTA saturation: {write_dur:?}"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit")
        .fetch_one(&writer_pool)
        .await
        .unwrap();
    assert_eq!(count, 2, "the concurrent writer append committed");

    drop(_c1);
    drop(_c2);
    h.abort();
}

// ---- rota-slices gate F2: /favicon.ico must not 404 ----
// Phase-3 asset slice: the interim 204 stub (which the original test
// pinned while anticipating its own replacement here) is now the real §9
// mark — the F2 intent (no 404, no console error) holds with STRONGER
// asserts: 200, SVG content type, the wheel actually present.

#[tokio::test]
async fn favicon_serves_the_wheel_mark_never_a_404() {
    let (base, _h) = serve().await;
    let resp = reqwest::get(format!("{base}/favicon.ico")).await.unwrap();
    assert_eq!(resp.status(), 200, "favicon serves the §9 mark, never 404");
    assert_eq!(
        resp.headers()["content-type"],
        "image/svg+xml",
        "SVG favicon content type"
    );
    let body = resp.text().await.unwrap();
    assert!(body.contains("<circle"), "the wheel renders: {body}");
    assert!(body.contains("#D4AF37"), "gold mark");

    // The read-only doctrine holds for this route too.
    let post = reqwest::Client::new()
        .post(format!("{base}/favicon.ico"))
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(
        post.status(),
        405,
        "POST /favicon.ico must be 405 (read-only)"
    );
}

// ---- T4.3 Phase 3 (track B): the §9 logo asset + presentation shell ----

#[tokio::test]
async fn logo_asset_serves_the_section9_geometry() {
    let (base, _h) = serve().await;
    let resp = reqwest::get(format!("{base}/assets/rota/logo.svg"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "image/svg+xml");
    let svg = resp.text().await.unwrap();
    assert!(svg.contains("viewBox=\"0 0 48 48\""), "§9 viewBox: {svg}");
    assert_eq!(
        svg.matches("<line").count(),
        8,
        "eight spokes at 45-degree steps: {svg}"
    );
    assert!(svg.contains("<ellipse"), "cornucopia mouth ellipse: {svg}");
    assert!(svg.contains("#D4AF37"), "all gold");
    assert!(
        !svg.to_lowercase().contains("gradient"),
        "flat gold-line aesthetic — no gradients (§2)"
    );
}

#[tokio::test]
async fn shell_carries_the_presentation_layer() {
    let (base, _h) = serve().await;
    let body = reqwest::get(format!("{base}/rota"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    // The header carries the inlined §9 mark, not a broken placeholder.
    assert!(body.contains("<svg"), "logo inlined in the header");
    assert!(
        !body.contains("<!--ROTA_LOGO-->"),
        "placeholder must be replaced"
    );
    // Panel renderers: formatted money, UTC-labeled freshness, and the
    // cognition click-to-expand (operator amendment: evidence/provenance
    // surface as expandable rows).
    assert!(body.contains("fmtCents"), "money formatter present");
    assert!(body.contains("UTC"), "times render labeled UTC (R6)");
    assert!(
        body.contains("<details") || body.contains("details"),
        "click-to-expand rows for cognition evidence"
    );
    assert!(body.contains("raw"), "raw JSON expander per panel");
}

// ------------------------------------------------------------------ cognition
//
// R7: the cognition panel = the daemon-shaped counters view (absent until
// synthesis-in-main composes a cognition strategy — rendered as an explicit
// status, never fabricated zeros) + ROTA's OWN two ledger queries
// (recent beliefs incl. evidence/provenance; calibration scopes) over the
// R5 pool. Populated-path seeds per the verifier's vacuous-test rule.

async fn seed_event(pool: &sqlx::PgPool, event_id: &str) {
    sqlx::query(
        "INSERT INTO events (event_id, statement, resolution_criteria,
                             resolution_source, benchmark_at, category,
                             unscoreable, created_at)
         VALUES ($1, 'seed', 'seed', 'nws', '2026-06-12T00:00:00.000Z',
                 'weather', FALSE, '2026-06-11T00:00:00.000Z')",
    )
    .bind(event_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_cognition(state: RotaState) -> serde_json::Value {
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/cognition"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    h.abort();
    j
}

#[tokio::test]
async fn cognition_degrades_without_pool_but_stays_200() {
    let j = get_cognition(RotaState::standalone(empty_snapshot())).await;
    assert_eq!(
        j["recent_beliefs"]["available"], false,
        "no pool => beliefs unavailable, explicit: {j}"
    );
    assert_eq!(j["calibration_scopes"]["available"], false, "{j}");
    assert_eq!(j["belief_lifecycle"]["available"], false, "{j}");
    assert!(
        j["counters_status"]
            .as_str()
            .unwrap_or("")
            .contains("unavailable"),
        "counters absent until synthesis populates the view: {j}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn cognition_serves_seeded_beliefs_and_scopes(pool: sqlx::PgPool) {
    use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo};
    seed_event(&pool, "01EVENTROTA000000000000001").await;
    let beliefs = BeliefsRepo::new(pool.clone());
    beliefs
        .insert(
            "01BELIEFROTA00000000000001",
            "2026-06-12T01:00:00.000Z",
            "01EVENTROTA000000000000001",
            0.67,
            0.71,
            "2026-06-13T00:00:00.000Z",
            &serde_json::json!({"reasoning": "structural underpricing of the middle bracket"}),
            &serde_json::json!({"model_id": "claude-fable-5", "cost_cents": 12,
                "persona_id": "meteorologist", "persona_version": 3,
                "analysis_id": "01J0ANALYSIS00000NYC", "run_at": "2026-06-12T00:55:00.000Z"}),
            None,
        )
        .await
        .unwrap();
    let cal = CalibrationParamsRepo::new(pool.clone());
    for version in [1, 2] {
        cal.insert(
            &format!("01CALROTA000000000000000{version}"),
            "claude-fable-5",
            "synth_events",
            "weather",
            "platt",
            &serde_json::json!({"a": 0.1}),
            version,
            "2026-06-11T00:00:00.000Z",
            "2026-06-11T00:00:00.000Z",
        )
        .await
        .unwrap();
    }

    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let j = get_cognition(state).await;

    // Real seeded values — a fabricated/empty panel cannot satisfy these.
    let rows = j["recent_beliefs"]["rows"].as_array().unwrap_or_else(|| {
        panic!("recent_beliefs.rows must be an array: {j}");
    });
    assert_eq!(rows.len(), 1, "{j}");
    assert_eq!(rows[0]["belief_id"], "01BELIEFROTA00000000000001");
    assert_eq!(rows[0]["p"], 0.67, "{j}");
    assert_eq!(rows[0]["p_raw"], 0.71);
    assert_eq!(rows[0]["status"], "open");
    assert_eq!(
        rows[0]["evidence"]["reasoning"], "structural underpricing of the middle bracket",
        "the model's persisted reasoning surfaces: {j}"
    );
    assert_eq!(rows[0]["provenance"]["cost_cents"], 12);
    // §20.3: the LABELED provenance summary surfaces the key fields (which persona/
    // model/analysis/cost drove the belief) for legible rendering — the whole
    // provenance is still served alongside (above).
    assert_eq!(rows[0]["prov"]["model_id"], "claude-fable-5", "{j}");
    assert_eq!(rows[0]["prov"]["cost_cents"], 12);
    assert_eq!(
        rows[0]["prov"]["persona_id"], "meteorologist",
        "the persona that produced the belief is surfaced: {j}"
    );
    assert_eq!(rows[0]["prov"]["persona_version"], 3);
    assert_eq!(rows[0]["prov"]["analysis_id"], "01J0ANALYSIS00000NYC");
    let scopes = j["calibration_scopes"]["rows"].as_array().unwrap();
    assert_eq!(scopes.len(), 1, "one distinct scope: {j}");
    assert_eq!(scopes[0]["version"], 2, "max version wins: {j}");
    assert_eq!(scopes[0]["model_id"], "claude-fable-5");
    assert_eq!(scopes[0]["kind"], "platt");
}

// The belief LIFECYCLE aggregates: status distribution + the resolved beliefs'
// calibration outcome (mean Brier). POPULATED-path over real beliefs of every
// status — a fabricated/empty panel cannot satisfy these counts. (D V6's per-
// belief strategy/PnL columns are schema-blocked; this is the buildable lifecycle.)
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn cognition_lifecycle_aggregates_beliefs_by_status(pool: sqlx::PgPool) {
    use fortuna_ledger::BeliefsRepo;
    seed_event(&pool, "01EVENTROTALIFECYCLE000001").await;
    let beliefs = BeliefsRepo::new(pool.clone());
    let evid = serde_json::json!({"r": "x"});
    let prov = serde_json::json!({"model_id": "m"});
    // 2 open.
    for id in ["01BLIFEOPEN0000000000000001", "01BLIFEOPEN0000000000000002"] {
        beliefs
            .insert(
                id,
                "2026-06-12T01:00:00.000Z",
                "01EVENTROTALIFECYCLE000001",
                0.6,
                0.6,
                "2026-06-13T00:00:00.000Z",
                &evid,
                &prov,
                None,
            )
            .await
            .unwrap();
    }
    // 1 resolved + scored (Brier 0.2).
    beliefs
        .insert(
            "01BLIFERESOLVED00000000001",
            "2026-06-12T01:00:00.000Z",
            "01EVENTROTALIFECYCLE000001",
            0.7,
            0.66,
            "2026-06-13T00:00:00.000Z",
            &evid,
            &prov,
            None,
        )
        .await
        .unwrap();
    beliefs
        .resolve_and_score("01BLIFERESOLVED00000000001", true, 0.2, Some(40.0))
        .await
        .unwrap();
    // 1 superseded (X superseded by Y; Y stays open).
    beliefs
        .insert(
            "01BLIFESUPSEDED0000000001",
            "2026-06-12T01:00:00.000Z",
            "01EVENTROTALIFECYCLE000001",
            0.5,
            0.5,
            "2026-06-13T00:00:00.000Z",
            &evid,
            &prov,
            None,
        )
        .await
        .unwrap();
    beliefs
        .insert(
            "01BLIFESUPSEDER0000000002",
            "2026-06-12T01:00:00.000Z",
            "01EVENTROTALIFECYCLE000001",
            0.5,
            0.5,
            "2026-06-13T00:00:00.000Z",
            &evid,
            &prov,
            Some("01BLIFESUPSEDED0000000001"),
        )
        .await
        .unwrap();

    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let j = get_cognition(state).await;
    let lc = &j["belief_lifecycle"];
    assert_eq!(lc["available"], true, "{j}");
    // open = 2 + the superseder (Y) = 3; resolved 1; superseded 1 (X); abandoned 0.
    assert_eq!(lc["by_status"]["open"], 3, "{j}");
    assert_eq!(lc["by_status"]["resolved"], 1, "{j}");
    assert_eq!(lc["by_status"]["superseded"], 1, "{j}");
    assert_eq!(
        lc["by_status"]["abandoned"], 0,
        "an absent bucket reads 0, never missing: {j}"
    );
    assert_eq!(lc["resolved_scored_n"], 1, "{j}");
    // The calibration outcome is real (AVG over the one scored belief), not fabricated.
    assert_eq!(lc["mean_brier"], 0.2, "{j}");
    assert_eq!(
        lc["mean_clv_bps"], 40.0,
        "the CLV edge proxy is real too: {j}"
    );
}

// The Recent Fills board serves the EXECUTED trades from the durable `fills`
// ledger, newest-first, money columns flagged `cents`. POPULATED-path (real
// seeded fills), not a vacuous empty board.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn fills_board_serves_recent_executed_trades(pool: sqlx::PgPool) {
    for (id, market, side, action, price, qty, fee, maker, at) in [
        (
            "01FILL0000000000000000001",
            "KXNYCHIGH-26JUN13-B65",
            "yes",
            "buy",
            41i64,
            40i64,
            12i64,
            false,
            "2026-06-13T12:30:00.000Z",
        ),
        (
            "01FILL0000000000000000002",
            "KXCHIHIGH-26JUN13-B72",
            "no",
            "sell",
            55i64,
            25i64,
            7i64,
            true,
            "2026-06-13T12:31:00.000Z",
        ),
    ] {
        sqlx::query(
            "INSERT INTO fills (fill_id, venue, venue_order_id, client_order_id, market_id, \
             side, action, price_cents, qty, fee_cents, is_maker, at) \
             VALUES ($1,'sim',$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(id)
        .bind(format!("vo-{id}"))
        .bind(format!("co-{id}"))
        .bind(market)
        .bind(side)
        .bind(action)
        .bind(price)
        .bind(qty)
        .bind(fee)
        .bind(maker)
        .bind(at)
        .execute(&pool)
        .await
        .unwrap();
    }
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/fills"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both seeded fills served: {j}");
    // Newest-first (at DESC): the 12:31 maker sell leads.
    assert_eq!(j["rows"][0]["market"], "KXCHIHIGH-26JUN13-B72");
    assert_eq!(j["rows"][0]["maker"], "maker");
    assert_eq!(j["rows"][0]["price_cents"], 55);
    assert_eq!(
        j["rows"][1]["action"], "buy",
        "the older fill is the buy: {j}"
    );
    assert_eq!(j["summary"]["fills"], 2);
    // Price/fee are money columns (rendered as dollars via the `cents` flag).
    let cols = j["columns"].as_array().unwrap();
    assert!(
        cols.iter()
            .any(|c| c["key"] == "price_cents" && c["cents"] == true),
        "price is a cents column: {j}"
    );
}

// Strategy P&L board: the daemon-shaped per-strategy view serves verbatim,
// money columns flagged `cents`. POPULATED-path (real seeded rows incl. a losing
// strategy shown honestly), not a vacuous empty board.
#[tokio::test]
async fn strategies_board_serves_seeded_per_strategy_pnl() {
    let snap = empty_snapshot();
    {
        let mut s = snap.write().await;
        s.views = serde_json::json!({
            "strategies": {
                "title": "Strategy P&L",
                "columns": [
                    {"key":"strategy","label":"Strategy"},
                    {"key":"realized_pnl_cents","label":"Realized","cents":true},
                    {"key":"fills","label":"Fills"}
                ],
                "rows": [
                    {"strategy":"mech_structural","realized_pnl_cents":3100,"fills":3},
                    {"strategy":"perp_basis","realized_pnl_cents":-450,"fills":1}
                ],
                "summary": {"strategies":2,"fills":4}
            }
        });
    }
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/strategies"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both strategies served: {j}");
    assert_eq!(j["rows"][0]["strategy"], "mech_structural");
    assert_eq!(j["rows"][0]["realized_pnl_cents"], 3100);
    // A losing strategy is shown honestly (negative realized), never hidden.
    assert_eq!(j["rows"][1]["realized_pnl_cents"], -450, "{j}");
    assert_eq!(j["summary"]["strategies"], 2);
    let cols = j["columns"].as_array().unwrap();
    assert!(
        cols.iter()
            .any(|c| c["key"] == "realized_pnl_cents" && c["cents"] == true),
        "realized PnL is a cents column: {j}"
    );
}

// Working Orders board (mission item 3, live side): the daemon-shaped resting-order
// view serves verbatim, limit flagged `cents`, status a pill. POPULATED-path (two
// real seeded orders incl. a partial fill), not a vacuous empty board. The shaping
// itself (from runner.manager().intents()) is proven in fortuna-live/tests/views.rs.
#[tokio::test]
async fn working_orders_board_serves_seeded_resting_orders() {
    let snap = empty_snapshot();
    {
        let mut s = snap.write().await;
        s.views = serde_json::json!({
            "working_orders": {
                "title": "Working Orders",
                "columns": [
                    {"key":"market","label":"Market"},
                    {"key":"side","label":"Side"},
                    {"key":"limit_cents","label":"Limit","cents":true},
                    {"key":"status","label":"Status","pill":true}
                ],
                "rows": [
                    {"market":"KXNYCHIGH-26JUN13-B65","side":"yes","limit_cents":41,
                     "qty":50,"filled":0,"status":"acked"},
                    {"market":"KXNYCHIGH-26JUN13-B70","side":"no","limit_cents":58,
                     "qty":40,"filled":12,"status":"partially_filled"}
                ],
                "summary": {"working":2}
            }
        });
    }
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/working_orders"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both resting orders served: {j}");
    assert_eq!(j["rows"][0]["market"], "KXNYCHIGH-26JUN13-B65");
    assert_eq!(j["rows"][0]["status"], "acked");
    // A partial fill is shown honestly (filled < qty), never hidden.
    assert_eq!(j["rows"][1]["status"], "partially_filled");
    assert_eq!(j["rows"][1]["filled"], 12, "{j}");
    assert_eq!(j["summary"]["working"], 2);
    let cols = j["columns"].as_array().unwrap();
    assert!(
        cols.iter()
            .any(|c| c["key"] == "limit_cents" && c["cents"] == true),
        "limit is a cents column: {j}"
    );
}

// Telemetry board (mission item 6): the daemon-shaped MetricsRegistry view serves
// verbatim. POPULATED-path — the shaping itself (MetricsRegistry::telemetry_board)
// is proven in fortuna-ops/src/metrics.rs; this confirms the read_view handler
// serves the seeded view (the same shape telemetry_board produces).
#[tokio::test]
async fn telemetry_board_serves_seeded_metric_series() {
    let snap = empty_snapshot();
    {
        let mut s = snap.write().await;
        s.views = serde_json::json!({
            "telemetry": {
                "title": "Telemetry",
                "columns": [
                    {"key":"subsystem","label":"Subsystem"},
                    {"key":"metric","label":"Metric"},
                    {"key":"type","label":"Type"},
                    {"key":"value","label":"Value"}
                ],
                "rows": [
                    {"subsystem":"exec","metric":"fortuna_exec_working_orders","type":"gauge","value":3},
                    {"subsystem":"gate","metric":"fortuna_gate_rejections_total{check=\"edge\"}",
                     "type":"counter","value":5}
                ],
                "summary": {"families":2,"series":2}
            }
        });
    }
    let app = rota_router(RotaState::standalone(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/telemetry"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both metric series served: {j}");
    assert_eq!(j["rows"][0]["subsystem"], "exec");
    assert_eq!(j["rows"][0]["value"], 3);
    assert_eq!(j["rows"][1]["type"], "counter");
    assert_eq!(j["summary"]["families"], 2);
}

// Discovery — Events board: the canonical events with their mapped-market count.
// POPULATED-path — two real events, one with two distinct mapped markets (incl. a
// superseding edge on the same market that DISTINCT must collapse), one with none.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn discovery_board_serves_events_with_distinct_market_counts(pool: sqlx::PgPool) {
    // Event A (newer) has markets; event B (older) has none.
    for (id, stmt, status, created_at) in [
        (
            "01EVENTDISC0000000000001",
            "NYC high >= 65F",
            "active",
            "2026-06-12T11:00:00.000Z",
        ),
        (
            "01EVENTDISC0000000000002",
            "CHI high >= 72F",
            "resolved_final",
            "2026-06-12T10:00:00.000Z",
        ),
    ] {
        sqlx::query(
            "INSERT INTO events (event_id, statement, resolution_criteria, resolution_source, \
             benchmark_at, category, status, created_at) \
             VALUES ($1,$2,'crit','nws.cli','2026-06-13T16:00:00.000Z','weather',$3,$4)",
        )
        .bind(id)
        .bind(stmt)
        .bind(status)
        .bind(created_at)
        .execute(&pool)
        .await
        .unwrap();
    }
    // Event A -> M1 (edge1), A -> M1 again (edge2 supersedes edge1), A -> M2: two
    // DISTINCT markets despite three edge rows.
    for (edge_id, market_id, supersedes) in [
        ("01EDGE0000000000000000001", "M1", None::<&str>),
        (
            "01EDGE0000000000000000002",
            "M1",
            Some("01EDGE0000000000000000001"),
        ),
        ("01EDGE0000000000000000003", "M2", None),
    ] {
        sqlx::query(
            "INSERT INTO market_event_edges (edge_id, market_id, venue, event_id, mapping_type, \
             confidence, proposed_by, supersedes, created_at) \
             VALUES ($1,$2,'sim','01EVENTDISC0000000000001','direct',0.9,'discovery',$3,\
             '2026-06-12T11:00:00.000Z')",
        )
        .bind(edge_id)
        .bind(market_id)
        .bind(supersedes)
        .execute(&pool)
        .await
        .unwrap();
    }
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/discovery"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both events served: {j}");
    // Event A is newer (created_at later) → rows[0]; two DISTINCT markets.
    assert_eq!(j["rows"][0]["statement"], "NYC high >= 65F");
    assert_eq!(
        j["rows"][0]["markets"], 2,
        "DISTINCT market count collapses the superseding edge: {j}"
    );
    assert_eq!(
        j["rows"][1]["markets"], 0,
        "an event with no edges shows 0: {j}"
    );
    assert_eq!(j["summary"]["events"], 2);
    assert_eq!(j["summary"]["markets_mapped"], 2);
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn cognition_truncates_evidence_over_4kb(pool: sqlx::PgPool) {
    use fortuna_ledger::BeliefsRepo;
    seed_event(&pool, "01EVENTROTA000000000000002").await;
    let big = "x".repeat(8000);
    BeliefsRepo::new(pool.clone())
        .insert(
            "01BELIEFROTA00000000000002",
            "2026-06-12T01:00:00.000Z",
            "01EVENTROTA000000000000002",
            0.5,
            0.5,
            "2026-06-13T00:00:00.000Z",
            &serde_json::json!({"reasoning": big}),
            &serde_json::json!({"model_id": "m"}),
            None,
        )
        .await
        .unwrap();
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let j = get_cognition(state).await;
    let ev = &j["recent_beliefs"]["rows"][0]["evidence"];
    assert_eq!(ev["truncated"], true, "oversized evidence truncates: {j}");
    let preview = ev["preview"].as_str().unwrap();
    assert!(
        preview.len() <= 4096,
        "preview bounded at 4KB, got {}",
        preview.len()
    );
    assert!(preview.contains("xxx"), "preview carries real content");
}

#[tokio::test]
async fn cognition_carries_daemon_counters_when_the_view_is_populated() {
    let snap = empty_snapshot();
    {
        let mut s = snap.try_write().unwrap();
        s.views = serde_json::json!({
            "cognition": { "mind_spend_today_cents": 1240,
                           "budget_breaches_total": 0 }
        });
    }
    let j = get_cognition(RotaState::standalone(snap)).await;
    assert_eq!(
        j["mind_spend_today_cents"], 1240,
        "daemon counters merge to the top level: {j}"
    );
    assert!(
        j.get("counters_status").is_none(),
        "no status when real: {j}"
    );
    // The ledger arrays still degrade independently (no pool here).
    assert_eq!(j["recent_beliefs"]["available"], false);
}

// Mission item 5 ("honest visibility into the actual tables — counts"): the DB
// inventory board sweeps EVERY ledger table and returns real counts, busiest-
// first. Seeds two tables (events x2, beliefs x1) on a freshly-migrated DB and
// asserts the full 24-table inventory, the exact non-zero counts, the busiest-
// first ordering, the running total, and that a genuinely empty table honestly
// shows 0 (a true COUNT(*), never an omitted row or a fabricated number).
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn db_board_counts_every_ledger_table(pool: sqlx::PgPool) {
    use fortuna_ledger::BeliefsRepo;
    // events: 2 rows; beliefs: 1 row (FK to one event); every other table: 0.
    seed_event(&pool, "01EVENTDB0000000000000001").await;
    seed_event(&pool, "01EVENTDB0000000000000002").await;
    BeliefsRepo::new(pool.clone())
        .insert(
            "01BELIEFDB0000000000000001",
            "2026-06-12T01:00:00.000Z",
            "01EVENTDB0000000000000001",
            0.5,
            0.5,
            "2026-06-13T00:00:00.000Z",
            &serde_json::json!({"reasoning": "seed"}),
            &serde_json::json!({"model_id": "m"}),
            None,
        )
        .await
        .unwrap();
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/db"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    // The full sweep — every ledger table inventoried, not a subset.
    assert_eq!(rows.len(), 24, "all 24 ledger tables inventoried: {j}");
    assert_eq!(j["summary"]["tables"], 24);
    // Busiest-first: events(2) precedes beliefs(1) precedes the empty tables.
    assert_eq!(j["rows"][0]["table"], "events");
    assert_eq!(j["rows"][0]["rows"], 2, "events count is real: {j}");
    assert_eq!(j["rows"][1]["table"], "beliefs");
    assert_eq!(j["rows"][1]["rows"], 1, "beliefs count is real: {j}");
    // total_rows == the sum of every table's count.
    assert_eq!(j["summary"]["total_rows"], 3);
    // A genuinely empty table honestly shows a real 0 (never omitted, never faked).
    let fills = rows
        .iter()
        .find(|r| r["table"] == "fills")
        .expect("fills row present in the inventory");
    assert_eq!(fills["rows"], 0, "an empty table shows a real 0: {j}");
    // The scalar-belief plane (belief_scores + scalar_beliefs, added with the
    // perp/scalar foundation) is swept too — both present, honest 0 when empty.
    // This pins the sweep against a future migration silently escaping the board.
    for t in ["belief_scores", "scalar_beliefs"] {
        let row = rows
            .iter()
            .find(|r| r["table"] == t)
            .unwrap_or_else(|| panic!("{t} row present in the inventory: {j}"));
        assert_eq!(row["rows"], 0, "{t} empty → honest 0: {j}");
    }
}

// Mission item 1 ("how beliefs are formed — the roster of analysts"): the Personas
// registry board (§20.1 registry half) serves every (persona_id, version) grouped by
// persona, NEWEST VERSION FIRST, with the lifecycle status as a pill, the method-file
// integrity hash truncated to its 8-char provenance prefix, and reads_signal_kinds
// flattened to a comma list. Seeds two personas (one with a retired v1 superseded by
// an active v2) and asserts the grouped ordering, the real status values (active vs
// the honestly-retired v1), the joined reads, and the registry summary. (The §20.1
// SCORECARD half — per-persona Brier/CLV/verdict — is data-blocked; GAPS.)
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn personas_board_serves_the_registry_grouped_newest_version_first(pool: sqlx::PgPool) {
    use fortuna_ledger::PersonasRepo;
    let repo = PersonasRepo::new(pool.clone());
    // macro_analyst v1 (active, synthesis tier).
    repo.insert(
        "01PERSONAROW000000000MACRO1",
        "macro_analyst",
        1,
        "macro",
        &serde_json::json!(["cpi", "nfp"]),
        &serde_json::json!(["calendar.bls", "rss.fed"]),
        "synthesis",
        "deadbeefcafe0001",
        "findings/v1",
        "active",
        None,
        "2026-06-10T00:00:00.000Z",
        "2026-06-10T00:00:00.000Z",
    )
    .await
    .unwrap();
    // meteorologist v1 (retired) → superseded by v2 (active).
    repo.insert(
        "01PERSONAROW00000000METEO1",
        "meteorologist",
        1,
        "weather",
        &serde_json::json!(["temperature"]),
        &serde_json::json!(["nws.afd"]),
        "cheap",
        "1111111111110001",
        "findings/v1",
        "retired",
        None,
        "2026-06-09T00:00:00.000Z",
        "2026-06-09T00:00:00.000Z",
    )
    .await
    .unwrap();
    repo.insert(
        "01PERSONAROW00000000METEO2",
        "meteorologist",
        2,
        "weather",
        &serde_json::json!(["temperature", "nyc"]),
        &serde_json::json!(["aeolus.forecast", "nws.observed_high"]),
        "cheap",
        "abcd1234ef567890",
        "findings/v1",
        "active",
        Some("01PERSONAROW00000000METEO1"),
        "2026-06-12T00:00:00.000Z",
        "2026-06-12T00:00:00.000Z",
    )
    .await
    .unwrap();
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/personas"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        3,
        "all three (persona, version) rows served: {j}"
    );
    // Registry summary: 2 distinct personas, 3 versions, 2 active.
    assert_eq!(j["summary"]["personas"], 2);
    assert_eq!(j["summary"]["versions"], 3);
    assert_eq!(j["summary"]["active"], 2);
    // Grouped persona_id ASC, version DESC: macro_analyst, then meteorologist v2, v1.
    assert_eq!(j["rows"][0]["persona"], "macro_analyst");
    assert_eq!(j["rows"][0]["status"], "active");
    assert_eq!(j["rows"][1]["persona"], "meteorologist");
    assert_eq!(
        j["rows"][1]["version"], 2,
        "newest version first within a persona: {j}"
    );
    assert_eq!(j["rows"][1]["status"], "active");
    assert_eq!(j["rows"][2]["version"], 1);
    assert_eq!(
        j["rows"][2]["status"], "retired",
        "the superseded v1 renders honestly as retired: {j}"
    );
    // Method hash is the 8-char provenance prefix; reads is the joined signal kinds.
    assert_eq!(j["rows"][1]["method"], "abcd1234");
    assert_eq!(j["rows"][1]["reads"], "aeolus.forecast, nws.observed_high");
}

// Mission item 1 / §20.2 ("the whole process — the analyses beliefs are built
// from"): the Domain Analyses browser serves the artifact ledger newest-first, with
// the persona as "id@version", cost as cents (dollars in the UI via the cents flag),
// the content-hash replay anchor truncated, and the supersession status. Seeds two
// analyses for one region where the later supersedes the earlier, and asserts the
// produced_at-DESC ordering, the persona render, the per-row cost + hash prefix, the
// honest open-vs-superseded status, and the {analyses,open,cost_cents} summary. (The
// findings/signal_manifest expander — UNTRUSTED model output — is a §20.2 follow-on.)
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn analyses_board_serves_artifacts_newest_first_with_supersession(pool: sqlx::PgPool) {
    use fortuna_ledger::{BeliefsRepo, DomainAnalysesRepo};
    let repo = DomainAnalysesRepo::new(pool.clone());
    // A1: the earlier analysis for the region (cost 3¢) — later superseded.
    repo.insert(
        "01ANALYSISROTA0000000000A1",
        "meteorologist",
        2,
        "weather",
        "weather:KNYC:tmax:2026-06-12",
        "2026-06-12T05:00:00.000Z",
        &serde_json::json!([{"signal_id": "sig-1", "content_hash": "sh-1"}]),
        &serde_json::json!({"thresholds": [{"ge": 60, "p": 0.9}]}),
        "0badc0de11112222",
        "manifest-a1",
        3,
        None,
        "2026-06-12T05:00:00.000Z",
    )
    .await
    .unwrap();
    // A2: the later analysis for the SAME region (cost 5¢), superseding A1 → the
    // repo flips A1 to 'superseded', A2 is 'open'.
    repo.insert(
        "01ANALYSISROTA0000000000A2",
        "meteorologist",
        2,
        "weather",
        "weather:KNYC:tmax:2026-06-12",
        "2026-06-12T11:00:00.000Z",
        &serde_json::json!([{"signal_id": "sig-2", "content_hash": "sh-2"}]),
        &serde_json::json!({"thresholds": [{"ge": 60, "p": 0.95}]}),
        "feedface33334444",
        "manifest-a2",
        5,
        Some("01ANALYSISROTA0000000000A1"),
        "2026-06-12T11:00:00.000Z",
    )
    .await
    .unwrap();
    // Two beliefs were built FROM analysis A2 (their provenance cites it) — the §20.2
    // artifact→belief fanout. A1 produced none. The board's `beliefs` column counts them.
    seed_event(&pool, "01EVENTANALYSESFANOUT00001").await;
    let beliefs = BeliefsRepo::new(pool.clone());
    for bid in ["01BELIEFANALYSESFANOUT0001", "01BELIEFANALYSESFANOUT0002"] {
        beliefs
            .insert(
                bid,
                "2026-06-12T12:00:00.000Z",
                "01EVENTANALYSESFANOUT00001",
                0.7,
                0.65,
                "2026-06-13",
                &serde_json::json!({"source": "aeolus.forecast"}),
                &serde_json::json!({"analysis_id": "01ANALYSISROTA0000000000A2"}),
                None,
            )
            .await
            .unwrap();
    }
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/analyses"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both artifacts served: {j}");
    // Summary: 2 analyses, 1 still open (A2), total cost 8¢.
    assert_eq!(j["summary"]["analyses"], 2);
    assert_eq!(j["summary"]["open"], 1);
    assert_eq!(j["summary"]["cost_cents"], 8);
    // produced_at DESC: A2 (later) first, then A1.
    assert_eq!(
        j["rows"][0]["persona"], "meteorologist@2",
        "persona id@version: {j}"
    );
    assert_eq!(j["rows"][0]["region_key"], "weather:KNYC:tmax:2026-06-12");
    assert_eq!(j["rows"][0]["cost_cents"], 5, "per-row cost in cents: {j}");
    assert_eq!(
        j["rows"][0]["content_hash"], "feedface",
        "8-char hash prefix: {j}"
    );
    assert_eq!(
        j["rows"][0]["status"], "open",
        "the newest analysis is open: {j}"
    );
    // A1 is honestly superseded (the repo flipped it on A2's insert).
    assert_eq!(j["rows"][1]["cost_cents"], 3);
    assert_eq!(
        j["rows"][1]["status"], "superseded",
        "the earlier analysis renders honestly as superseded: {j}"
    );
    // §20.2 artifact→belief fanout: A2 produced two beliefs, A1 produced none.
    assert_eq!(
        j["rows"][0]["beliefs"], 2,
        "A2's downstream belief fanout: {j}"
    );
    assert_eq!(j["rows"][1]["beliefs"], 0, "A1 produced no beliefs: {j}");
    assert_eq!(
        j["summary"]["beliefs"], 2,
        "total fanout across artifacts: {j}"
    );
}

// Track-C §9.1 ("the outcomes of the whole process"): the Forecasts scorecard
// aggregates RESOLVED scalar forecasts per (producer, scoring rule) into the mean
// CRPS (lower = better) and resolved count. Seeds two producers — funding_forecast
// (two resolved rate forecasts) and aeolus_weather (one resolved celsius forecast),
// each scored under crps_pinball — and asserts the producer-ASC ordering, the mean
// CRPS per producer, the resolved counts, the unit, and the {producers,rules,scored}
// summary. (The untrusted quantiles/provenance are never selected; the recent feed +
// coverage are §9.1 follow-ons.)
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn forecasts_scorecard_aggregates_resolved_scores_per_producer(pool: sqlx::PgPool) {
    use fortuna_ledger::{BeliefScoresRepo, ScalarBeliefsRepo};
    let sb = ScalarBeliefsRepo::new(pool.clone());
    let bs = BeliefScoresRepo::new(pool.clone());
    // (belief_id, producer, unit, realized, crps_score)
    let seeds = [
        (
            "01SBROTA000000000000FF1",
            "funding_forecast",
            "rate",
            0.00012,
            0.00003,
        ),
        (
            // realized 0.0005 falls OUTSIDE the [0, 0.0003] band → 50% coverage for
            // funding_forecast (belief 1 in-band, belief 2 out).
            "01SBROTA000000000000FF2",
            "funding_forecast",
            "rate",
            0.0005,
            0.00005,
        ),
        (
            "01SBROTA000000000000AW1",
            "aeolus_weather",
            "celsius",
            30.0,
            1.2,
        ),
    ];
    for (i, (id, producer, unit, realized, score)) in seeds.iter().enumerate() {
        sb.insert(
            id,
            producer,
            "ev-key",
            &serde_json::json!([{"q":0.1,"v":0.0},{"q":0.5,"v":0.0001},{"q":0.9,"v":0.0003}]),
            unit,
            "2026-06-13T16:00:00.000Z",
            &serde_json::json!({"strategy": producer}),
            "2026-06-13T15:00:00.000Z",
        )
        .await
        .unwrap();
        sb.resolve(id, *realized, "2026-06-13T16:00:01.000Z")
            .await
            .unwrap();
        bs.insert(
            &format!("01SCOREROTA00000000000{i:03}"),
            id,
            "crps_pinball",
            *score,
            "2026-06-13T16:00:02.000Z",
        )
        .await
        .unwrap();
    }
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/forecasts"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "one row per (producer, rule): {j}");
    // producer ASC: aeolus_weather, then funding_forecast.
    assert_eq!(j["rows"][0]["producer"], "aeolus_weather");
    assert_eq!(j["rows"][0]["unit"], "celsius");
    assert_eq!(j["rows"][0]["rule_id"], "crps_pinball");
    assert_eq!(j["rows"][0]["resolved_n"], 1);
    let aw_mean = j["rows"][0]["mean_crps"].as_f64().unwrap();
    assert!((aw_mean - 1.2).abs() < 1e-9, "aeolus mean CRPS: {j}");
    assert_eq!(j["rows"][1]["producer"], "funding_forecast");
    assert_eq!(j["rows"][1]["unit"], "rate");
    assert_eq!(j["rows"][1]["resolved_n"], 2, "two resolved forecasts: {j}");
    let ff_mean = j["rows"][1]["mean_crps"].as_f64().unwrap();
    assert!(
        (ff_mean - 0.00004).abs() < 1e-9,
        "funding_forecast mean CRPS = (0.00003+0.00005)/2: {j}"
    );
    // §9.1 band coverage (the fraction of resolved forecasts whose realized outcome
    // fell inside the 0.1–0.9 quantile band): aeolus's single forecast missed the band
    // (0%); funding had 1 of 2 in-band (50%). A real calibration metric, not faked.
    assert_eq!(
        j["rows"][0]["coverage_pct"], 0.0,
        "aeolus realized fell outside its band: {j}"
    );
    assert_eq!(
        j["rows"][1]["coverage_pct"], 50.0,
        "funding had 1 of 2 forecasts in-band: {j}"
    );
    assert_eq!(j["summary"]["producers"], 2);
    assert_eq!(j["summary"]["rules"], 1);
    assert_eq!(j["summary"]["scored"], 3);
}

// Track-E §20.1 OUTCOMES half ("are the personas any good?"): the Persona Scorecard
// aggregates each persona's RESOLVED+scored beliefs (grouped by the belief
// provenance's persona_id) into n_resolved + mean Brier + mean CLV. Seeds two
// personas — meteorologist (two resolved beliefs) and macro_analyst (one) — and
// asserts the persona-ASC ordering, the per-persona MEAN Brier/CLV, the counts, the
// EVALUATING(n/60) verdict, and the summary. (Baselines + the promote/retire verdict
// are omitted — unpersisted; the scorecard never fabricates them.)
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn persona_scorecard_aggregates_resolved_beliefs_by_persona(pool: sqlx::PgPool) {
    use fortuna_ledger::BeliefsRepo;
    seed_event(&pool, "01EVENTPSC0000000000000001").await;
    let repo = BeliefsRepo::new(pool.clone());
    // meteorologist ×2 (brier 0.10, 0.20 → mean 0.15; clv 30, 50 → mean 40),
    // macro_analyst ×1 (brier 0.30; clv -10). All persona-attributed + resolved.
    for (id, persona, brier, clv) in [
        (
            "01BPSC000000000000000MET1",
            "meteorologist",
            0.10_f64,
            30.0_f64,
        ),
        ("01BPSC000000000000000MET2", "meteorologist", 0.20, 50.0),
        ("01BPSC000000000000000MAC1", "macro_analyst", 0.30, -10.0),
    ] {
        repo.insert(
            id,
            "2026-06-12T10:00:00.000Z",
            "01EVENTPSC0000000000000001",
            0.6,
            0.6,
            "2026-06-13",
            &serde_json::json!({"source": "aeolus.forecast"}),
            &serde_json::json!({"persona_id": persona, "persona_version": 1, "analysis_id": "a1"}),
            None,
        )
        .await
        .unwrap();
        repo.resolve_and_score(id, true, brier, Some(clv))
            .await
            .unwrap();
    }
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/persona_scores"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "one row per persona: {j}");
    // persona ASC: macro_analyst, then meteorologist.
    assert_eq!(j["rows"][0]["persona"], "macro_analyst");
    assert_eq!(j["rows"][0]["n_resolved"], 1);
    assert!(
        (j["rows"][0]["brier"].as_f64().unwrap() - 0.30).abs() < 1e-9,
        "macro brier: {j}"
    );
    assert!(
        (j["rows"][0]["clv_bps"].as_f64().unwrap() - (-10.0)).abs() < 1e-9,
        "macro clv (negative shown honestly): {j}"
    );
    assert_eq!(j["rows"][0]["verdict"], "evaluating (1/60)");
    // meteorologist: the MEAN over its two resolved beliefs.
    assert_eq!(j["rows"][1]["persona"], "meteorologist");
    assert_eq!(j["rows"][1]["n_resolved"], 2, "two resolved beliefs: {j}");
    assert!(
        (j["rows"][1]["brier"].as_f64().unwrap() - 0.15).abs() < 1e-9,
        "meteorologist mean Brier = (0.10+0.20)/2: {j}"
    );
    assert!(
        (j["rows"][1]["clv_bps"].as_f64().unwrap() - 40.0).abs() < 1e-9,
        "meteorologist mean CLV = (30+50)/2: {j}"
    );
    assert_eq!(
        j["rows"][1]["verdict"], "evaluating (2/60)",
        "honest §11 progress, never a fabricated promote/retire: {j}"
    );
    assert_eq!(j["summary"]["personas"], 2);
    assert_eq!(j["summary"]["resolved"], 3);
}
// ---- T4.5: /gates recent_rejections (audit-recents; populated-path) ----

/// Seed one `gate_decision` audit row carrying a serialized GateCheckRecord
/// payload (the per-check trail runner.rs writes). Binds the payload as TEXT
/// cast to jsonb so the test crate needs no sqlx json feature.
async fn insert_gate_decision(
    pool: &sqlx::PgPool,
    audit_id: &str,
    at: &str,
    intent: &str,
    check: &str,
    verdict: &str,
    reason: &str,
) {
    let payload = serde_json::json!({
        "check": check, "verdict": verdict, "reason": reason,
        "at": at, "intent_id": intent, "client_order_id": format!("coid-{intent}"),
    })
    .to_string();
    sqlx::query(
        "INSERT INTO audit (audit_id, at, kind, actor, ref_id, payload) \
         VALUES ($1, $2, 'gate_decision', NULL, $3, $4::jsonb)",
    )
    .bind(audit_id)
    .bind(at)
    .bind(intent)
    .bind(payload)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_gates(state: RotaState) -> serde_json::Value {
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/gates"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    h.abort();
    j
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn gates_recent_rejections_surfaces_only_rejects_newest_first(pool: sqlx::PgPool) {
    // The per-check gate trail: 2 Rejects (distinct checks/intents) + 1 Pass,
    // plus a `gate_reject` row (the BUS-event kind, NOT a gate_decision audit
    // row). recent_rejections must surface ONLY the 2 Rejects, newest-first,
    // with the §5 {audit_id,at,check,reason,intent_ref} shape — the Pass and the
    // foreign kind excluded.
    insert_gate_decision(
        &pool,
        "01R1AAAAAAAAAAAAAAAAAAAAAA",
        "2026-06-11T12:00:01.000Z",
        "01INTENT1AAAAAAAAAAAAAAAAA",
        "EdgeFloor",
        "Reject",
        "net edge 42bps < floor 100bps",
    )
    .await;
    insert_gate_decision(
        &pool,
        "01P1AAAAAAAAAAAAAAAAAAAAAA",
        "2026-06-11T12:00:02.000Z",
        "01INTENT2AAAAAAAAAAAAAAAAA",
        "PriceBand",
        "Pass",
        "within band",
    )
    .await;
    insert_gate_decision(
        &pool,
        "01R2AAAAAAAAAAAAAAAAAAAAAA",
        "2026-06-11T12:00:03.000Z",
        "01INTENT3AAAAAAAAAAAAAAAAA",
        "RateLimits",
        "Reject",
        "venue burst exhausted",
    )
    .await;
    insert_audit(&pool, "01XXAAAAAAAAAAAAAAAAAAAAAA", "gate_reject").await;

    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let j = get_gates(state).await;
    let rr = &j["recent_rejections"];
    assert_eq!(rr["available"], serde_json::json!(true), "{j}");
    let rows = rr["rows"].as_array().expect("rows array");
    assert_eq!(
        rows.len(),
        2,
        "only the 2 Rejects (Pass + the gate_reject kind excluded): {rows:?}"
    );
    // newest-first: R2 (12:00:03) then R1 (12:00:01).
    assert_eq!(rows[0]["audit_id"], "01R2AAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(rows[0]["check"], "RateLimits");
    assert_eq!(rows[0]["reason"], "venue burst exhausted");
    assert_eq!(rows[0]["intent_ref"], "01INTENT3AAAAAAAAAAAAAAAAA");
    assert_eq!(rows[0]["at"], "2026-06-11T12:00:03.000Z");
    assert_eq!(rows[1]["audit_id"], "01R1AAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(rows[1]["check"], "EdgeFloor");
    assert_eq!(rows[1]["reason"], "net edge 42bps < floor 100bps");
    assert_eq!(rows[1]["intent_ref"], "01INTENT1AAAAAAAAAAAAAAAAA");
    assert_eq!(rows[1]["at"], "2026-06-11T12:00:01.000Z");
    assert!(
        !rows.iter().any(|r| r["check"] == "PriceBand"),
        "a passing check must never surface as a rejection: {rows:?}"
    );
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn gates_recent_rejections_is_available_but_empty_when_no_rejects(pool: sqlx::PgPool) {
    // Only a Pass seeded → recent_rejections is available + EMPTY (honest empty,
    // never the panel implying a rejection that did not happen).
    insert_gate_decision(
        &pool,
        "01P1AAAAAAAAAAAAAAAAAAAAAA",
        "2026-06-11T12:00:02.000Z",
        "01INTENT2AAAAAAAAAAAAAAAAA",
        "PriceBand",
        "Pass",
        "within band",
    )
    .await;
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let j = get_gates(state).await;
    assert_eq!(j["recent_rejections"]["available"], serde_json::json!(true));
    assert_eq!(
        j["recent_rejections"]["rows"].as_array().unwrap().len(),
        0,
        "no rejects → empty, not fabricated"
    );
}

#[tokio::test]
async fn gates_recent_rejections_degrades_without_a_pool() {
    // R1: standalone (no pool) → 200, never 500; recent_rejections explicitly
    // unavailable, never fabricated as empty-meaning-all-clear.
    let state = RotaState::standalone(empty_snapshot());
    let j = get_gates(state).await;
    assert_eq!(
        j["recent_rejections"]["available"],
        serde_json::json!(false),
        "{j}"
    );
}

// ---- T4.5: /settlement recent_watchdog_events (audit-recents; populated-path) ----

/// Seed one `watchdog` audit row (the table kind the runner writes via
/// self.audit("watchdog", Some(market), {kind: <sub_kind>, ...})). ref_id = the
/// market; the §5 `kind` is the payload sub-kind. TEXT-cast bind (no sqlx json).
async fn insert_watchdog(
    pool: &sqlx::PgPool,
    audit_id: &str,
    at: &str,
    market: &str,
    sub_kind: &str,
) {
    let payload =
        serde_json::json!({ "kind": sub_kind, "expected_by_epoch_ms": 1_780_000_000_000_i64 })
            .to_string();
    sqlx::query(
        "INSERT INTO audit (audit_id, at, kind, actor, ref_id, payload) \
         VALUES ($1, $2, 'watchdog', NULL, $3, $4::jsonb)",
    )
    .bind(audit_id)
    .bind(at)
    .bind(market)
    .bind(payload)
    .execute(pool)
    .await
    .unwrap();
}

async fn get_settlement(state: RotaState) -> serde_json::Value {
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/settlement"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    h.abort();
    j
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn settlement_recent_watchdog_surfaces_only_watchdog_newest_first(pool: sqlx::PgPool) {
    // The audit `watchdog` rows (all three sub-kinds the runner emits) + a
    // NON-watchdog row. recent_watchdog_events must surface ONLY the watchdog
    // rows, newest-first, with §5 {audit_id, at, kind (the sub-kind), market_ref}.
    insert_watchdog(
        &pool,
        "01W1AAAAAAAAAAAAAAAAAAAAAA",
        "2026-06-11T12:00:01.000Z",
        "KXMARKET-A",
        "settlement_overdue",
    )
    .await;
    insert_watchdog(
        &pool,
        "01W2AAAAAAAAAAAAAAAAAAAAAA",
        "2026-06-11T12:00:02.000Z",
        "KXMARKET-B",
        "dispute_freeze",
    )
    .await;
    insert_watchdog(
        &pool,
        "01W3AAAAAAAAAAAAAAAAAAAAAA",
        "2026-06-11T12:00:03.000Z",
        "KXMARKET-C",
        "orphaned_position",
    )
    .await;
    insert_audit(&pool, "01XXAAAAAAAAAAAAAAAAAAAAAA", "settlement").await; // non-watchdog

    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let j = get_settlement(state).await;
    let rw = &j["recent_watchdog_events"];
    assert_eq!(rw["available"], serde_json::json!(true), "{j}");
    let rows = rw["rows"].as_array().expect("rows array");
    assert_eq!(
        rows.len(),
        3,
        "only the 3 watchdog rows (the foreign kind excluded): {rows:?}"
    );
    // newest-first: W3 (12:00:03) -> W2 -> W1.
    assert_eq!(rows[0]["audit_id"], "01W3AAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(rows[0]["kind"], "orphaned_position");
    assert_eq!(rows[0]["market_ref"], "KXMARKET-C");
    assert_eq!(rows[0]["at"], "2026-06-11T12:00:03.000Z");
    assert_eq!(rows[1]["audit_id"], "01W2AAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(rows[1]["kind"], "dispute_freeze");
    assert_eq!(rows[1]["market_ref"], "KXMARKET-B");
    assert_eq!(rows[1]["at"], "2026-06-11T12:00:02.000Z");
    assert_eq!(rows[2]["audit_id"], "01W1AAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(rows[2]["kind"], "settlement_overdue");
    assert_eq!(rows[2]["market_ref"], "KXMARKET-A");
    assert_eq!(rows[2]["at"], "2026-06-11T12:00:01.000Z");
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn settlement_recent_watchdog_is_available_but_empty_when_none(pool: sqlx::PgPool) {
    // Only a non-watchdog row → available + EMPTY (honest, not implying an event).
    insert_audit(&pool, "01XXAAAAAAAAAAAAAAAAAAAAAA", "settlement").await;
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let j = get_settlement(state).await;
    assert_eq!(
        j["recent_watchdog_events"]["available"],
        serde_json::json!(true)
    );
    assert_eq!(
        j["recent_watchdog_events"]["rows"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn settlement_recent_watchdog_degrades_without_a_pool() {
    let state = RotaState::standalone(empty_snapshot());
    let j = get_settlement(state).await;
    assert_eq!(
        j["recent_watchdog_events"]["available"],
        serde_json::json!(false),
        "{j}"
    );
}

// ---- T4.5 slice: gate-verdict badge (docs/reviews/*.md parser; data seam) ----

use fortuna_ops::rota::{latest_gate_verdict, parse_verdict_token};

#[test]
fn parse_verdict_token_handles_the_recorded_formats() {
    // Every shape that occurs in docs/reviews/*.md (bold, # heading, (...) and —
    // suffixes, hyphenated verdicts). Tolerant: non-verdict lines -> None.
    let cases = [
        ("Verdict: ACCEPT", Some("ACCEPT")),
        ("Verdict: BLOCK (2 unledgered Majors)", Some("BLOCK")),
        ("Verdict: ACCEPT-WITH-GAPS", Some("ACCEPT-WITH-GAPS")),
        ("Verdict: **BLOCK** (one Major)", Some("BLOCK")),
        ("**Verdict: ACCEPT** (remediation batch)", Some("ACCEPT")),
        (
            "## VERDICT: ACCEPT-SLICE — dial logic promotion-ready",
            Some("ACCEPT-SLICE"),
        ),
        ("Verdict: ACCEPT — MERGE", Some("ACCEPT")),
        ("   verdict:   accept", Some("ACCEPT")),
        (
            "## VERDICT: ACCEPT-WITH-CONDITIONS — see notes",
            Some("ACCEPT-WITH-CONDITIONS"),
        ),
        // MID-LINE header row (a real docs/reviews format): metadata then Verdict.
        (
            "Base: main  Head: b6ed374 (track-c)  Verdict: ACCEPT-SLICE",
            Some("ACCEPT-SLICE"),
        ),
        ("## Findings", None),
        ("no verdict on this line", None),
        // Prose "verdict:" must NOT false-positive (token is not ACCEPT*/BLOCK).
        ("the verdict: was unclear at first", None),
        ("", None),
        ("Verdict:", None),
    ];
    for (line, want) in cases {
        assert_eq!(parse_verdict_token(line).as_deref(), want, "line {line:?}");
    }
}

fn write_review(dir: &Path, name: &str, content: &str, mtime_epoch_secs: u64) {
    use std::time::{Duration, SystemTime};
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    let f = std::fs::File::options().write(true).open(&path).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_epoch_secs))
        .unwrap();
}

#[test]
fn latest_gate_verdict_picks_the_newest_file_carrying_a_verdict() {
    let dir = std::env::temp_dir().join(format!("fortuna-reviews-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Older verdict file, newer verdict file, a NO-verdict file that is the
    // newest of all (must be skipped), a non-.md file (ignored).
    write_review(
        &dir,
        "gate-old.md",
        "# Gate\nVerdict: BLOCK\n",
        1_780_000_100,
    );
    write_review(
        &dir,
        "gate-new.md",
        "# Gate\nVerdict: ACCEPT-SLICE — ok\n",
        1_780_000_200,
    );
    write_review(
        &dir,
        "GATE-FINDINGS-LATEST.md",
        "# bus\nno single verdict here\n",
        1_780_000_900,
    );
    std::fs::write(dir.join("notes.txt"), "Verdict: ACCEPT\n").unwrap();

    let v = latest_gate_verdict(&dir);
    assert_eq!(v["available"], serde_json::json!(true), "{v}");
    assert_eq!(
        v["verdict"], "ACCEPT-SLICE",
        "newest VERDICT-bearing file wins: {v}"
    );
    assert_eq!(v["file"], "gate-new.md");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn latest_gate_verdict_is_unavailable_when_no_verdict_or_dir_absent() {
    // Dir absent -> unavailable, never a panic.
    let absent =
        std::env::temp_dir().join(format!("fortuna-reviews-absent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&absent);
    assert_eq!(
        latest_gate_verdict(&absent)["available"],
        serde_json::json!(false)
    );

    // Dir present but NO file carries a verdict -> unavailable (never fabricated).
    let dir = std::env::temp_dir().join(format!("fortuna-reviews-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_review(
        &dir,
        "no-verdict.md",
        "# A doc\njust prose, no verdict header\n",
        1_780_000_100,
    );
    assert_eq!(
        latest_gate_verdict(&dir)["available"],
        serde_json::json!(false)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

async fn get_build(state: RotaState) -> serde_json::Value {
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/build"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    h.abort();
    j
}

#[tokio::test]
async fn build_endpoint_serves_the_latest_verdict_and_degrades_without_a_reviews_dir() {
    use std::sync::Arc;
    // With a reviews-dir capability -> the latest verdict, end-to-end through HTTP.
    let dir = std::env::temp_dir().join(format!("fortuna-reviews-ep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_review(&dir, "gate.md", "## VERDICT: ACCEPT\n", 1_780_000_100);
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: None,
        perishable_dir: None,
        reviews_dir: Some(Arc::new(dir.clone())),
    };
    let j = get_build(state).await;
    assert_eq!(
        j["latest_gate_verdict"]["available"],
        serde_json::json!(true),
        "{j}"
    );
    assert_eq!(j["latest_gate_verdict"]["verdict"], "ACCEPT");
    let _ = std::fs::remove_dir_all(&dir);

    // Standalone (no reviews-dir) -> explicit unavailable, never a 500.
    let j = get_build(RotaState::standalone(empty_snapshot())).await;
    assert_eq!(
        j["latest_gate_verdict"]["available"],
        serde_json::json!(false),
        "{j}"
    );
}

// Track-C §9.1 + the operator "completely see the belief and everything" want
// (2026-06-13): the Forecast Feed surfaces the recent scalar beliefs newest-first (by
// ULID belief_id), each FULLY inspectable — the at-a-glance median + status + realized
// outcome PLUS the WHOLE quantile fan and the producer's EVIDENCE + provenance. The
// live daemon wraps {"provenance":…,"evidence":…} into the single `provenance` column
// (persist_scalar_beliefs); the board splits it back. Seeds one resolved
// funding_forecast + one pending aeolus_weather with that wrapped provenance and
// asserts: ordering, the full fan, the median, the realized-vs-honest-null outcome,
// the split-out evidence, and the summary.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn forecast_feed_surfaces_recent_scalar_beliefs_richly(pool: sqlx::PgPool) {
    use fortuna_ledger::ScalarBeliefsRepo;
    let sb = ScalarBeliefsRepo::new(pool.clone());
    // sb1: funding_forecast, RESOLVED. belief_id FF1 > AW1 → newest-first. The
    // provenance column is the daemon wrapper {"provenance":…,"evidence":…}; the
    // producer's work (estimate / point_forecast / remaining_candles) lives under
    // "evidence".
    sb.insert(
        "01SBFEED00000000000000FF1",
        "funding_forecast",
        "KXBTCPERP1:2026-06-13T16:00:00Z",
        &serde_json::json!([{"q":0.1,"v":0.00005},{"q":0.5,"v":0.0001},{"q":0.9,"v":0.00018}]),
        "rate",
        "2026-06-13T16:00:00.000Z",
        &serde_json::json!({
            "provenance": {"model_id":"funding-v1","cost_cents":7},
            "evidence": {"estimate":"0.0001","point_forecast":"0.0001","remaining_candles":42}
        }),
        "2026-06-13T15:00:00.000Z",
    )
    .await
    .unwrap();
    sb.resolve(
        "01SBFEED00000000000000FF1",
        0.00012,
        "2026-06-13T16:00:01.000Z",
    )
    .await
    .unwrap();
    // sb2: aeolus_weather, PENDING (no realized value); a different evidence shape.
    sb.insert(
        "01SBFEED00000000000000AW1",
        "aeolus_weather",
        "KNYC:tmax:2026-06-14",
        &serde_json::json!([{"q":0.1,"v":80.0},{"q":0.5,"v":85.0},{"q":0.9,"v":90.0}]),
        "celsius",
        "2026-06-14T16:00:00.000Z",
        &serde_json::json!({
            "provenance": {},
            "evidence": {"model":"aeolus-emos","station":"KNYC"}
        }),
        "2026-06-13T14:00:00.000Z",
    )
    .await
    .unwrap();
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/forecast_feed"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both beliefs served: {j}");
    assert_eq!(j["summary"]["forecasts"], 2);
    assert_eq!(j["summary"]["resolved"], 1);
    assert_eq!(j["summary"]["pending"], 1);

    // rows[0]: funding_forecast, resolved, newest by belief_id DESC (FF1 > AW1).
    let r0 = &j["rows"][0];
    assert_eq!(r0["producer"], "funding_forecast");
    assert_eq!(r0["status"], "resolved");
    assert!(
        (r0["median"].as_f64().unwrap() - 0.0001).abs() < 1e-12,
        "median is the q=0.5 of the fan: {j}"
    );
    assert!(
        (r0["realized"].as_f64().unwrap() - 0.00012).abs() < 1e-12,
        "realized outcome: {j}"
    );
    // The WHOLE quantile fan is surfaced (not just the median), ascending by q.
    let fan = r0["quantiles"].as_array().unwrap();
    assert_eq!(fan.len(), 3, "the full fan, every q/v pair: {j}");
    assert!((fan[0]["q"].as_f64().unwrap() - 0.1).abs() < 1e-12);
    assert!((fan[2]["q"].as_f64().unwrap() - 0.9).abs() < 1e-12);
    assert!(
        (fan[2]["v"].as_f64().unwrap() - 0.00018).abs() < 1e-12,
        "the q=0.9 tail of the fan is rendered: {j}"
    );
    // The producer's EVIDENCE is split out of the wrapper and surfaced verbatim — the
    // "show me the model's work" the operator asked for.
    assert_eq!(r0["evidence"]["estimate"], "0.0001");
    assert_eq!(r0["evidence"]["point_forecast"], "0.0001");
    assert_eq!(
        r0["evidence"]["remaining_candles"].as_i64().unwrap(),
        42,
        "evidence numbers are rendered as data: {j}"
    );
    // The split is clean: the rendered evidence is the INNER evidence, NOT the whole
    // wrapped column — no nested "evidence"/"provenance" keys leak into it.
    assert!(
        r0["evidence"].get("evidence").is_none() && r0["evidence"].get("provenance").is_none(),
        "evidence is the unwrapped inner payload: {j}"
    );
    // The inner provenance ALSO survives the split: model_id lives under the wrapper's
    // "provenance" key, so reading it back proves the unwrap (a broken split would read
    // the top-level wrapper and find null). Both the raw provenance and the labeled
    // `prov` summary (which drives the provLine) carry it.
    assert_eq!(
        r0["provenance"]["model_id"], "funding-v1",
        "inner provenance surfaced: {j}"
    );
    assert_eq!(
        r0["prov"]["model_id"], "funding-v1",
        "the unwrapped provenance feeds the summary line: {j}"
    );
    assert_eq!(r0["prov"]["cost_cents"].as_i64().unwrap(), 7);

    // rows[1]: aeolus_weather, PENDING — its median + fan + evidence are shown, but the
    // realized outcome is an HONEST null, never a fabricated one.
    let r1 = &j["rows"][1];
    assert_eq!(r1["producer"], "aeolus_weather");
    assert_eq!(r1["status"], "pending");
    assert!(
        r1["realized"].is_null(),
        "an unresolved belief has a null outcome, never a fabricated one: {j}"
    );
    assert!(
        (r1["median"].as_f64().unwrap() - 85.0).abs() < 1e-12,
        "a pending belief still carries its median: {j}"
    );
    assert_eq!(r1["evidence"]["model"], "aeolus-emos");
    assert_eq!(r1["quantiles"].as_array().unwrap().len(), 3);
}

// Track-E §20.4: the Persona Pipeline funnel — per persona, analyses produced →
// beliefs fanned out → beliefs resolved. Seeds two registered personas, two analyses
// by the meteorologist (none by the macro_analyst), and three persona-attributed
// beliefs (2 meteorologist incl. 1 resolved; 1 macro_analyst resolved). Asserts the
// per-persona funnel counts (incl. honest 0 analyses for the macro_analyst) and the
// totals.
#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn persona_pipeline_funnels_analyses_beliefs_resolved_per_persona(pool: sqlx::PgPool) {
    use fortuna_ledger::{BeliefsRepo, DomainAnalysesRepo, PersonasRepo};
    // Registry: two personas (the funnel's universe).
    let personas = PersonasRepo::new(pool.clone());
    for (row_id, pid) in [
        ("01PPLPERSONAROW00000MET", "meteorologist"),
        ("01PPLPERSONAROW00000MAC", "macro_analyst"),
    ] {
        personas
            .insert(
                row_id,
                pid,
                1,
                "weather",
                &serde_json::json!([]),
                &serde_json::json!([]),
                "cheap",
                "methodhash",
                "findings/v1",
                "active",
                None,
                "2026-06-10T00:00:00.000Z",
                "2026-06-10T00:00:00.000Z",
            )
            .await
            .unwrap();
    }
    // meteorologist produced two analyses; macro_analyst produced none.
    let analyses = DomainAnalysesRepo::new(pool.clone());
    for (aid, region) in [
        ("01PPLANALYSIS000000001", "weather:r1"),
        ("01PPLANALYSIS000000002", "weather:r2"),
    ] {
        analyses
            .insert(
                aid,
                "meteorologist",
                1,
                "weather",
                region,
                "2026-06-12T05:00:00.000Z",
                &serde_json::json!([]),
                &serde_json::json!({}),
                &format!("hash-{aid}"),
                "manifest",
                1,
                None,
                "2026-06-12T05:00:00.000Z",
            )
            .await
            .unwrap();
    }
    // beliefs: 2 meteorologist (1 resolved), 1 macro_analyst (resolved).
    seed_event(&pool, "01PPLEVENT00000000000001").await;
    let beliefs = BeliefsRepo::new(pool.clone());
    for (bid, persona, resolve) in [
        ("01PPLBELIEF0000000000MET1", "meteorologist", true),
        ("01PPLBELIEF0000000000MET2", "meteorologist", false),
        ("01PPLBELIEF0000000000MAC1", "macro_analyst", true),
    ] {
        beliefs
            .insert(
                bid,
                "2026-06-12T12:00:00.000Z",
                "01PPLEVENT00000000000001",
                0.6,
                0.6,
                "2026-06-13",
                &serde_json::json!({"source": "x"}),
                &serde_json::json!({"persona_id": persona}),
                None,
            )
            .await
            .unwrap();
        if resolve {
            beliefs
                .resolve_and_score(bid, true, 0.1, Some(10.0))
                .await
                .unwrap();
        }
    }
    let state = RotaState {
        snapshot: empty_snapshot(),
        pool: Some(pool),
        perishable_dir: None,
        reviews_dir: None,
    };
    let app = rota_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _h = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let j: serde_json::Value = reqwest::get(format!("http://{addr}/api/rota/v1/persona_pipeline"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let rows = j["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both registered personas: {j}");
    // persona ASC: macro_analyst, then meteorologist.
    assert_eq!(j["rows"][0]["persona"], "macro_analyst");
    assert_eq!(
        j["rows"][0]["analyses"], 0,
        "macro_analyst produced no analyses (honest 0 via the LEFT JOIN): {j}"
    );
    assert_eq!(j["rows"][0]["beliefs"], 1);
    assert_eq!(j["rows"][0]["resolved"], 1);
    assert_eq!(j["rows"][1]["persona"], "meteorologist");
    assert_eq!(j["rows"][1]["analyses"], 2, "two analyses produced: {j}");
    assert_eq!(j["rows"][1]["beliefs"], 2, "two beliefs fanned out: {j}");
    assert_eq!(j["rows"][1]["resolved"], 1, "one of the two resolved: {j}");
    // Funnel totals.
    assert_eq!(j["summary"]["personas"], 2);
    assert_eq!(j["summary"]["analyses"], 2);
    assert_eq!(j["summary"]["beliefs"], 3);
    assert_eq!(j["summary"]["resolved"], 2);
}
