//! ROTA v2 — the read-only operator console (T4.3; build to
//! docs/design/rota-dashboard.md and its binding amendments R1-R12).
//!
//! CAPABILITY-OPTION composition (R1): the only mandatory state is the
//! shared `DashboardSnapshot` arc; the Postgres pool is OPTIONAL. Every
//! Pg-derived surface renders a "degraded/unavailable" state (HTTP 200,
//! NEVER 500) when its capability is absent — ROTA is buildable and
//! servable standalone, before the daemon wires it in.
//!
//! Data plane (R2): handlers read the per-view JSON the composition
//! pre-shapes into `snapshot.views` (no Prometheus-text parsing) plus
//! their OWN ledger queries (the audit tail). Lock rule (R8): clone the
//! needed data out of the snapshot RwLock and release BEFORE any await.
//!
//! Audit tail (R3): a LOSSLESS cursor-polled JSON endpoint, not SSE.
//! Read-only doctrine (operator-binding): there is structurally nothing
//! to mutate — the route-table test asserts 405 on every other method.

use crate::dashboard::DashboardSnapshot;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// ROTA composition state. `pool`/`perishable_dir` are OPTIONAL
/// capabilities (R1); only `snapshot` is mandatory.
#[derive(Clone)]
pub struct RotaState {
    pub snapshot: Arc<RwLock<DashboardSnapshot>>,
    pub pool: Option<PgPool>,
    pub perishable_dir: Option<Arc<PathBuf>>,
}

impl RotaState {
    /// Standalone state (no Postgres, no recorder dir) — every Pg/fs
    /// surface degrades gracefully. The daemon supplies the capabilities.
    pub fn standalone(snapshot: Arc<RwLock<DashboardSnapshot>>) -> RotaState {
        RotaState {
            snapshot,
            pool: None,
            perishable_dir: None,
        }
    }
}

/// The ROTA routes, mergeable into the dashboard Router. All GET; the
/// route-table test pins 405 on every mutating method.
pub fn rota_router(state: RotaState) -> Router {
    Router::new()
        .route("/rota", get(shell))
        .route("/api/rota/v1/health", get(view_health))
        .route("/api/rota/v1/money", get(view_money))
        .route("/api/rota/v1/gates", get(view_gates))
        .route("/api/rota/v1/settlement", get(view_settlement))
        .route("/api/rota/v1/streams", get(view_streams))
        .route("/api/rota/v1/audit", get(audit_tail))
        .with_state(state)
}

/// Read one pre-shaped view out of the snapshot, releasing the lock
/// before returning (R8). Absent view => an explicit unavailable state,
/// never a 500.
async fn read_view(state: &RotaState, name: &str) -> Value {
    let (generated_at, view) = {
        let snap = state.snapshot.read().await;
        (snap.generated_at.clone(), snap.views.get(name).cloned())
    };
    match view {
        Some(v) => v,
        None => json!({
            "generated_at": generated_at,
            "status": "unavailable",
            "detail": format!("view {name:?} not yet populated by the composition"),
        }),
    }
}

async fn view_health(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "health").await)
}
async fn view_money(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "money").await)
}
async fn view_gates(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "gates").await)
}
async fn view_settlement(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "settlement").await)
}
async fn view_streams(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "streams").await)
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Return rows with audit_id strictly AFTER this cursor (lexical
    /// ULID order == time order); absent => the latest page.
    pub after: Option<String>,
    pub limit: Option<i64>,
}

/// Cursor-polled audit tail (R3): lossless, survives restarts, backed by
/// the audit table's indexes. Degrades to an explicit empty page (HTTP
/// 200) when the Postgres capability is absent — never a 500.
async fn audit_tail(State(s): State<RotaState>, Query(q): Query<AuditQuery>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "available": false,
            "detail": "postgres capability absent (standalone ROTA)",
            "rows": [],
            "next_after": null,
        }));
    };
    // Clamp the page; ascending audit_id (ULID == chronological).
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let after = q.after.unwrap_or_default();
    let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>)>(
        "SELECT audit_id, at, kind, actor, ref_id FROM audit \
         WHERE audit_id > $1 ORDER BY audit_id ASC LIMIT $2",
    )
    .bind(&after)
    .bind(limit)
    .fetch_all(&pool)
    .await;
    match rows {
        Ok(rows) => {
            let next_after = rows.last().map(|r| r.0.clone());
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(|(audit_id, at, kind, actor, ref_id)| {
                    json!({ "audit_id": audit_id, "at": at, "kind": kind,
                            "actor": actor, "ref_id": ref_id })
                })
                .collect();
            Json(json!({ "available": true, "rows": json_rows, "next_after": next_after }))
        }
        Err(e) => Json(json!({
            "available": true,
            "error": e.to_string(),
            "rows": [],
            "next_after": null,
        })),
    }
}

async fn shell() -> impl IntoResponse {
    (StatusCode::OK, Html(ROTA_SHELL))
}

/// The gold-on-black instrument shell (operator tokens, Section 2):
/// vanilla JS short-poll, the panel grid is the only responsive line
/// (R11). Inlined as a const — zero build step, zero CDN (R12 / Section 1).
const ROTA_SHELL: &str = r#"<!doctype html><html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>FORTUNA · ROTA</title>
<style>
  :root{--bg:#0A0A0B;--card:#141416;--gold:#D4AF37;--amber:#FFB84D;
        --text:#EDEDEA;--halt:#FF3B30;--ok:#30D158}
  *{box-sizing:border-box}
  body{margin:0;background:var(--bg);color:var(--text);
       font-family:system-ui,sans-serif}
  header{display:flex;align-items:center;gap:12px;padding:14px 20px;
         border-bottom:1px solid var(--gold)}
  header .mark{color:var(--gold);font-weight:700;letter-spacing:2px}
  header .sub{color:var(--amber);font-size:12px;letter-spacing:3px}
  .grid{display:grid;gap:14px;padding:18px;
        grid-template-columns:repeat(auto-fit,minmax(320px,1fr))}
  .panel{background:var(--card);border:1px solid #2a2a2d;border-radius:6px;padding:14px}
  .panel h2{margin:0 0 8px;color:var(--gold);font-size:13px;
            letter-spacing:1.5px;text-transform:uppercase}
  pre{margin:0;font-family:"JetBrains Mono",ui-monospace,monospace;
      font-size:12px;color:var(--text);white-space:pre-wrap;
      font-variant-numeric:tabular-nums lining-nums}
  #halt{display:none;position:fixed;inset:0;background:var(--halt);
        color:#fff;align-items:center;justify-content:center;
        font-size:28px;letter-spacing:3px;z-index:9}
</style></head><body>
<div id="halt">SYSTEM HALTED</div>
<header><span class="mark">FORTUNA</span><span class="sub">ROTA</span></header>
<div class="grid">
  <div class="panel"><h2>Health</h2><pre id="health">…</pre></div>
  <div class="panel"><h2>Money</h2><pre id="money">…</pre></div>
  <div class="panel"><h2>Gates</h2><pre id="gates">…</pre></div>
  <div class="panel"><h2>Settlement</h2><pre id="settlement">…</pre></div>
  <div class="panel"><h2>Streams</h2><pre id="streams">…</pre></div>
  <div class="panel"><h2>Audit tail</h2><pre id="audit">…</pre></div>
</div>
<script>
const B="/api/rota/v1/";
async function poll(name,el){try{const r=await fetch(B+name);const j=await r.json();
  document.getElementById(el).textContent=JSON.stringify(j,null,2);
  if(name==="health"){document.getElementById("halt").style.display=
    j.halt_active?"flex":"none";}}catch(e){
  document.getElementById(el).textContent="(unreachable: "+e+")";}}
function tick(){poll("health","health");poll("money","money");poll("gates","gates");
  poll("settlement","settlement");poll("streams","streams");poll("audit","audit");}
tick();setInterval(tick,2000);
</script></body></html>"#;
