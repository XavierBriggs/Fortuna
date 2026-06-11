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
use fortuna_core::clock::UtcTimestamp;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
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
        .route("/favicon.ico", get(favicon))
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
/// Cheap liveness stat of the recorder's perishable JSONL streams (the
/// /streams panel's `recorder` section). Reads only file METADATA (mtime +
/// size) — NEVER content — so it is O(streams) and safe on the 15s poll even
/// when a stream file is multiple GB (bracket_quotes.jsonl is ~1.3GB; a
/// line-count would be a self-inflicted DoS). The freshness clock is the
/// snapshot's `generated_at` (the daemon's last clock read): its date prefix
/// selects today's dir and its instant is "now" for the age, so this stays
/// clock-free and deterministic under test. `rows_today` / `key_count` (which
/// need a content read) are deferred — see GAPS. `healthy = age < 120s` (two
/// missed 30s recorder cycles). Absent/unreadable dir => empty array, never a
/// panic (the panel degrades, never 500s).
pub fn scan_recorder(perishable_dir: &Path, generated_at: &str) -> Value {
    let now_ms = UtcTimestamp::parse_iso8601(generated_at)
        .map(|t| t.epoch_millis())
        .unwrap_or(0);
    let today = generated_at.get(0..10).unwrap_or("");
    let day_dir = perishable_dir.join(today);
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(&day_dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false))
            .collect(),
        Err(_) => Vec::new(),
    };
    paths.sort();
    let mut out: Vec<Value> = Vec::with_capacity(paths.len());
    for path in paths {
        let stream = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        let entry = match std::fs::metadata(&path) {
            Ok(md) => {
                let age = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| ((now_ms - d.as_millis() as i64).max(0)) / 1000);
                json!({
                    "stream": stream,
                    "last_capture_age_secs": age,
                    "size_bytes": md.len(),
                    "healthy": age.map(|a| a < 120).unwrap_or(false),
                })
            }
            Err(_) => json!({
                "stream": stream,
                "last_capture_age_secs": Value::Null,
                "size_bytes": Value::Null,
                "healthy": false,
            }),
        };
        out.push(entry);
    }
    Value::Array(out)
}

async fn view_streams(State(s): State<RotaState>) -> impl IntoResponse {
    let mut view = read_view(&s, "streams").await;
    // Merge the recorder liveness scan ONLY when ROTA holds the
    // perishable_dir capability (the daemon supplies it; standalone does
    // not — then the panel degrades without the recorder section). "now" +
    // today come from the snapshot's generated_at so the handler stays
    // clock-free.
    if let Some(dir) = &s.perishable_dir {
        let generated_at = { s.snapshot.read().await.generated_at.clone() };
        let recorder = scan_recorder(dir.as_path(), &generated_at);
        if let Some(obj) = view.as_object_mut() {
            obj.insert("recorder".to_string(), recorder);
        }
    }
    Json(view)
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
/// One audit-tail row: (audit_id, at, kind, actor, ref_id).
type AuditRowTuple = (String, String, String, Option<String>, Option<String>);

/// One page of the append-only audit tail. ABSENT cursor => the LATEST page
/// (the tail's newest `limit` rows, returned chronological/ASC); a PRESENT
/// cursor => the next `limit` rows strictly AFTER it (forward pagination,
/// ASC). The cursorless default is the TAIL, not the head: a live audit
/// *tail* (design R3) must surface NEW rows, and ROTA_SHELL polls this
/// endpoint cursor-less every 2s.
///
/// Gate F1 (2026-06-11, MAJOR): the prior `q.after.unwrap_or_default()` bound
/// `audit_id > ''` into an ASC query and returned the OLDEST page (the first
/// rows ever) — so once a live pool landed the panel would have frozen on the
/// head, an audit tail that never shows new rows. Fixed here: cursorless
/// fetches newest-first then re-sorts ASC.
///
/// Runtime sqlx (not compile-time `query!`) by deliberate choice — see
/// ASSUMPTIONS: this single read-only dashboard query is schema-pinned by the
/// migration, and keeping it runtime avoids coupling the whole crate's build
/// to the sqlx-offline cache for one path. `limit` clamped to [1, 500].
pub async fn audit_tail_page(
    pool: &PgPool,
    after: Option<&str>,
    limit: i64,
) -> Result<Vec<AuditRowTuple>, sqlx::Error> {
    let limit = limit.clamp(1, 500);
    match after {
        Some(cursor) => {
            sqlx::query_as::<_, AuditRowTuple>(
                "SELECT audit_id, at, kind, actor, ref_id FROM audit \
                 WHERE audit_id > $1 ORDER BY audit_id ASC LIMIT $2",
            )
            .bind(cursor)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        None => {
            // Newest `limit` rows, then reverse to ASC so the page reads
            // chronologically exactly like a forward page.
            let mut rows = sqlx::query_as::<_, AuditRowTuple>(
                "SELECT audit_id, at, kind, actor, ref_id FROM audit \
                 ORDER BY audit_id DESC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?;
            rows.reverse();
            Ok(rows)
        }
    }
}

async fn audit_tail(State(s): State<RotaState>, Query(q): Query<AuditQuery>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "available": false,
            "detail": "postgres capability absent (standalone ROTA)",
            "rows": [],
            "next_after": null,
        }));
    };
    let limit = q.limit.unwrap_or(100);
    match audit_tail_page(&pool, q.after.as_deref(), limit).await {
        Ok(rows) => {
            // next_after = the newest id IN this page (rows are ASC), so a
            // cursor-tracking client paginates forward into rows that arrive
            // after this poll.
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

/// Favicon: a 204 No Content stub (rota-slices gate F2). The browser's
/// automatic /favicon.ico probe otherwise 404s — the only console error in
/// the live browser pass, and an R12 pass criterion. 204 clears it with no
/// asset dependency; the real Section 9 cornucopia/wheel mark replaces this in
/// the Phase-3 asset slice.
async fn favicon() -> impl IntoResponse {
    StatusCode::NO_CONTENT
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
