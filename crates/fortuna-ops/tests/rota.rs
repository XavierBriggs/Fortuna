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

const PATHS: [&str; 9] = [
    "/rota",
    "/assets/rota/logo.svg",
    "/api/rota/v1/health",
    "/api/rota/v1/money",
    "/api/rota/v1/gates",
    "/api/rota/v1/cognition",
    "/api/rota/v1/settlement",
    "/api/rota/v1/streams",
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
    for name in ["health", "money", "gates", "settlement", "streams"] {
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
            &serde_json::json!({"model_id": "claude-fable-5", "cost_cents": 12}),
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
    let scopes = j["calibration_scopes"]["rows"].as_array().unwrap();
    assert_eq!(scopes.len(), 1, "one distinct scope: {j}");
    assert_eq!(scopes[0]["version"], 2, "max version wins: {j}");
    assert_eq!(scopes[0]["model_id"], "claude-fable-5");
    assert_eq!(scopes[0]["kind"], "platt");
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
