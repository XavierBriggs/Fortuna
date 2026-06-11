//! T1.5: the read-only dashboard server (spec Section 8: "read-only web
//! UI served on Tailscale only" — network scoping is the operator's
//! Tailscale config; the server binds the caller-supplied address, tests
//! use loopback, and NO mutating route exists by construction).
//!
//! Written BEFORE src/dashboard.rs per the repository TDD doctrine.

use fortuna_ops::dashboard::{serve_dashboard, DashboardSnapshot};
use fortuna_ops::rota::RotaState;
use std::sync::Arc;
use tokio::sync::RwLock;

fn snapshot() -> DashboardSnapshot {
    DashboardSnapshot {
        generated_at: "2026-06-10T12:00:00.000Z".to_string(),
        stage: "sim".to_string(),
        metrics_text: "# TYPE fortuna_fills_total counter\nfortuna_fills_total 3\n".to_string(),
        boards: serde_json::json!({
            "positions": [
                { "market": "KXS", "yes": 10, "no": 0, "realized_pnl_cents": 400 }
            ],
            "ops": { "halts_active": 0, "discrepancies_open": 0 }
        }),
        views: serde_json::json!({}),
    }
}

#[tokio::test]
async fn dashboard_serves_metrics_boards_and_shell_read_only() {
    let state = Arc::new(RwLock::new(snapshot()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_dashboard(
        listener,
        RotaState::standalone(Arc::clone(&state)),
    ));

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // GET /metrics: Prometheus text exposition.
    let metrics = client.get(format!("{base}/metrics")).send().await.unwrap();
    assert_eq!(metrics.status(), 200);
    assert!(metrics
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    let body = metrics.text().await.unwrap();
    assert!(body.contains("fortuna_fills_total 3"));

    // GET /api/boards: the JSON boards.
    let boards: serde_json::Value = client
        .get(format!("{base}/api/boards"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(boards["stage"], "sim");
    assert_eq!(boards["boards"]["positions"][0]["market"], "KXS");

    // GET /: the Instrument shell.
    let html = client
        .get(&base)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("FORTUNA"));
    assert!(html.contains("/api/boards"), "the shell polls the boards");

    // READ-ONLY: no mutating method is routed anywhere.
    for path in ["/", "/metrics", "/api/boards"] {
        let resp = client
            .post(format!("{base}{path}"))
            .body("x")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 405, "POST {path} must be refused");
    }

    // Updates are visible without restart.
    state.write().await.metrics_text = "updated 1\n".to_string();
    let body = client
        .get(format!("{base}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("updated 1"));

    server.abort();
}

/// T4.3 slice 3: serve_dashboard now MERGES the ROTA console — the live
/// daemon serves /rota + the /api/rota/v1 tree alongside the legacy boards,
/// off the SAME snapshot. Written red-first: before the merge, /rota 404s.
/// Proves the populated slice-2 views are actually SERVED, and that ROTA's
/// read-only doctrine survives the merge.
#[tokio::test]
async fn serve_dashboard_mounts_the_rota_console_alongside_the_instrument() {
    let mut snap = snapshot();
    snap.views = serde_json::json!({
        "health": { "halt_active": false, "ticks_total": 7, "fill_latency_p90_ms": 12 }
    });
    let state = Arc::new(RwLock::new(snap));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_dashboard(
        listener,
        RotaState::standalone(Arc::clone(&state)),
    ));
    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // The legacy Instrument still works (same snapshot).
    let boards = client
        .get(format!("{base}/api/boards"))
        .send()
        .await
        .unwrap();
    assert_eq!(boards.status(), 200, "legacy /api/boards still mounted");

    // ROTA shell is reachable from the live server.
    let rota = client.get(format!("{base}/rota")).send().await.unwrap();
    assert_eq!(rota.status(), 200, "ROTA console mounted at /rota");

    // The populated health view (slice 2) is SERVED through the merged tree.
    let health: serde_json::Value = client
        .get(format!("{base}/api/rota/v1/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["ticks_total"], 7, "{health}");
    assert_eq!(health["fill_latency_p90_ms"], 12);

    // READ-ONLY doctrine survives the merge: ROTA refuses mutation.
    for path in ["/rota", "/api/rota/v1/health"] {
        let resp = client
            .post(format!("{base}{path}"))
            .body("x")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 405, "POST {path} must be refused");
    }
    server.abort();
}
