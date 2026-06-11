//! T4.3 ROTA slice 1 tests: the read-only route table (T-1) and the
//! capability-degraded path (R1: every surface is HTTP 200, never 500,
//! when Postgres/recorder capabilities are absent and views are empty).
//! Served against a real loopback listener — no daemon, no Postgres.

use fortuna_ops::dashboard::DashboardSnapshot;
use fortuna_ops::rota::{rota_router, RotaState};
use std::sync::Arc;
use tokio::sync::RwLock;

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

const PATHS: [&str; 7] = [
    "/rota",
    "/api/rota/v1/health",
    "/api/rota/v1/money",
    "/api/rota/v1/gates",
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
