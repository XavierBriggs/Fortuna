//! The read-only dashboard server (spec Section 8): "The Instrument"
//! aesthetic, positions/ops boards, served on Tailscale only. Network
//! scoping (Tailscale-only) is the operator's interface config — this
//! server binds whatever listener the caller hands it (the composition
//! root binds loopback/tailnet addresses, never 0.0.0.0) and exposes
//! ONLY GET routes: there is structurally nothing to mutate.
//!
//! The serve loop is an IO-edge concern (tokio); the deterministic core
//! publishes `DashboardSnapshot`s into the shared state and never blocks
//! on the server.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// What the dashboard renders: a point-in-time snapshot the runner (or
/// digest job) publishes. `boards` is pre-shaped JSON (positions board,
/// ops board); `metrics_text` is the exposition-format render.
#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub generated_at: String,
    pub stage: String,
    pub metrics_text: String,
    pub boards: serde_json::Value,
    /// Pre-shaped per-view JSON for ROTA (T4.3, R2): the composition
    /// populates a `health`/`money`/… object from metrics_export plus
    /// boards_json each refresh, and ROTA handlers read those objects
    /// directly (no Prometheus-text parsing). It stays an empty object
    /// until the composition fills it, and a handler then renders
    /// "unavailable" rather than failing.
    #[serde(default)]
    pub views: serde_json::Value,
}

type Shared = Arc<RwLock<DashboardSnapshot>>;

async fn metrics(State(state): State<Shared>) -> impl IntoResponse {
    let snap = state.read().await;
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        snap.metrics_text.clone(),
    )
}

async fn boards(State(state): State<Shared>) -> impl IntoResponse {
    let snap = state.read().await;
    axum::Json(serde_json::json!({
        "generated_at": snap.generated_at,
        "stage": snap.stage,
        "boards": snap.boards,
    }))
}

async fn shell() -> Html<&'static str> {
    Html(INSTRUMENT_SHELL)
}

/// Serve forever on the provided listener. The caller owns address policy
/// and the task's lifetime (spawn + abort).
pub async fn serve_dashboard(
    listener: tokio::net::TcpListener,
    state: Shared,
) -> Result<(), std::io::Error> {
    let app = Router::new()
        .route("/", get(shell))
        .route("/metrics", get(metrics))
        .route("/api/boards", get(boards))
        .with_state(state);
    axum::serve(listener, app).await
}

/// "The Instrument": dark, monospace, numbers-first, zero dependencies.
const INSTRUMENT_SHELL: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>FORTUNA — the instrument</title>
<style>
  body { background:#0b0e11; color:#c8d1d9; font:13px/1.5 ui-monospace,monospace; margin:2rem; }
  h1 { font-size:14px; letter-spacing:.3em; color:#8b949e; }
  .board { border:1px solid #21262d; padding:1rem; margin:1rem 0; }
  .label { color:#8b949e; }
  .num { color:#e6edf3; }
  .bad { color:#f85149; }
  pre { white-space:pre-wrap; }
</style>
</head>
<body>
<h1>FORTUNA</h1>
<div class="board" id="positions"><span class="label">positions</span><pre id="positions-body">…</pre></div>
<div class="board" id="ops"><span class="label">ops</span><pre id="ops-body">…</pre></div>
<script>
async function refresh() {
  try {
    const r = await fetch('/api/boards');
    const d = await r.json();
    document.getElementById('positions-body').textContent =
      JSON.stringify(d.boards.positions ?? [], null, 1);
    document.getElementById('ops-body').textContent =
      'stage: ' + d.stage + '  generated: ' + d.generated_at + '\n' +
      JSON.stringify(d.boards.ops ?? {}, null, 1);
  } catch (e) {
    document.getElementById('ops-body').textContent = 'fetch failed: ' + e;
  }
}
refresh();
setInterval(refresh, 5000);
</script>
</body>
</html>
"#;
