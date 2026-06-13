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
use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo};
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
    /// The verifier's gate-record dir (`docs/reviews/`) for the build badge — a
    /// capability present only when ROTA runs from the checkout (the local
    /// operator console). Absent => the badge renders "unknown" (a deployed
    /// daemon has no `docs/`), never a 500.
    pub reviews_dir: Option<Arc<PathBuf>>,
}

impl RotaState {
    /// Standalone state (no Postgres, no recorder dir, no reviews dir) — every
    /// Pg/fs surface degrades gracefully. The daemon supplies the capabilities.
    pub fn standalone(snapshot: Arc<RwLock<DashboardSnapshot>>) -> RotaState {
        RotaState {
            snapshot,
            pool: None,
            perishable_dir: None,
            reviews_dir: None,
        }
    }
}

/// The ROTA routes, mergeable into the dashboard Router. All GET; the
/// route-table test pins 405 on every mutating method.
pub fn rota_router(state: RotaState) -> Router {
    Router::new()
        .route("/rota", get(shell))
        .route("/favicon.ico", get(favicon))
        .route("/assets/rota/logo.svg", get(logo_asset))
        .route("/api/rota/v1/health", get(view_health))
        .route("/api/rota/v1/money", get(view_money))
        .route("/api/rota/v1/gates", get(view_gates))
        .route("/api/rota/v1/cognition", get(view_cognition))
        .route("/api/rota/v1/settlement", get(view_settlement))
        .route("/api/rota/v1/streams", get(view_streams))
        .route("/api/rota/v1/build", get(view_build))
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
/// The gates panel: the daemon-shaped scalars (total_rejections, by-check) ride
/// the "gates" snapshot view; ROTA's OWN R5-pool query adds `recent_rejections`
/// (§5) — the recent per-check gate REJECTIONS from the audit trail. Absent pool
/// => an explicit unavailable sub-surface, never fabricated zeros.
async fn view_gates(State(s): State<RotaState>) -> impl IntoResponse {
    let mut out = read_view(&s, "gates").await; // R8: snapshot lock released inside.
    let recent = match &s.pool {
        None => ledger_unavailable("postgres capability absent (standalone ROTA)"),
        Some(pool) => match recent_gate_rejections_page(pool, 20).await {
            Ok(rows) => {
                let rows: Vec<Value> = rows
                    .into_iter()
                    .map(|(audit_id, at, intent_ref, check, reason)| {
                        json!({
                            "audit_id": audit_id,
                            "at": at,
                            "check": check,
                            "reason": reason,
                            "intent_ref": intent_ref,
                        })
                    })
                    .collect();
                json!({ "available": true, "rows": rows })
            }
            Err(e) => {
                // Neutral detail only — never raw sqlx text to the view.
                eprintln!("rota: gate-rejections read degraded: {e}");
                ledger_unavailable("gate-rejections read unavailable (dashboard pool degraded)")
            }
        },
    };
    if let Some(obj) = out.as_object_mut() {
        obj.insert("recent_rejections".to_string(), recent);
    }
    Json(out)
}
/// The settlement panel: the daemon-shaped scalars (limbo, overdue, voids,
/// reversals) ride the "settlement" snapshot view; ROTA's OWN R5-pool query adds
/// `recent_watchdog_events` (§5) — the recent settlement-watchdog audit events
/// (settlement_overdue / dispute_freeze / orphaned_position). Absent pool => an
/// explicit unavailable sub-surface, never fabricated.
async fn view_settlement(State(s): State<RotaState>) -> impl IntoResponse {
    let mut out = read_view(&s, "settlement").await; // R8: snapshot lock released inside.
    let recent = match &s.pool {
        None => ledger_unavailable("postgres capability absent (standalone ROTA)"),
        Some(pool) => match recent_watchdog_events_page(pool, 20).await {
            Ok(rows) => {
                let rows: Vec<Value> = rows
                    .into_iter()
                    .map(|(audit_id, at, market_ref, kind)| {
                        json!({
                            "audit_id": audit_id,
                            "at": at,
                            "kind": kind,
                            "market_ref": market_ref,
                        })
                    })
                    .collect();
                json!({ "available": true, "rows": rows })
            }
            Err(e) => {
                // Neutral detail only — never raw sqlx text to the view.
                eprintln!("rota: watchdog-events read degraded: {e}");
                ledger_unavailable("watchdog-events read unavailable (dashboard pool degraded)")
            }
        },
    };
    if let Some(obj) = out.as_object_mut() {
        obj.insert("recent_watchdog_events".to_string(), recent);
    }
    Json(out)
}

/// The build badge (§7 cut it from v1 for "no parser"; T4.5 re-includes it):
/// the LATEST gate verdict parsed from the verifier's `docs/reviews/*.md`. A
/// capability of the LOCAL operator console (a deployed daemon has no `docs/`);
/// absent => "unknown", never a 500.
async fn view_build(State(s): State<RotaState>) -> impl IntoResponse {
    let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8.
    let verdict = match &s.reviews_dir {
        None => json!({
            "available": false,
            "detail": "reviews dir capability absent (not running from a checkout)",
        }),
        Some(dir) => latest_gate_verdict(dir),
    };
    Json(json!({ "generated_at": generated_at, "latest_gate_verdict": verdict }))
}

/// Parse the verdict token from one line, tolerant of every recorded
/// `docs/reviews/` format — line-start (`Verdict: ACCEPT`, `## VERDICT:
/// ACCEPT-SLICE — ...`, `**Verdict: ACCEPT** (...)`) AND mid-line header rows
/// (`Base: ... Head: ... Verdict: ACCEPT-SLICE`). Finds `verdict:` anywhere
/// (case-insensitive), then captures the token after it. The token is VALIDATED
/// against the real verdict vocabulary (ACCEPT* / BLOCK*) so a prose "verdict:"
/// in a review body never false-positives; anything else => None (the caller
/// renders "unknown"). Byte-boundary-safe (no panic).
pub fn parse_verdict_token(line: &str) -> Option<String> {
    let needle = "verdict:";
    let pos = line.to_ascii_lowercase().find(needle)?;
    // `pos` indexes ASCII "verdict:" (case-flipping preserves byte length), so
    // `pos + needle.len()` is a valid char boundary in `line` — no panic.
    let after = &line[pos + needle.len()..];
    let token: String = after
        .trim_start_matches(['*', ' ', '\t'])
        .chars()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '-')
        .collect();
    let token = token.trim_matches('-').to_ascii_uppercase();
    if token.starts_with("ACCEPT") || token.starts_with("BLOCK") {
        Some(token)
    } else {
        None
    }
}

/// Read up to `max_bytes` of a file (the Verdict: header sits near the top) so
/// the scan is bounded even on a large doc. Lossy UTF-8 — a split multibyte at
/// the boundary becomes U+FFFD, never a panic.
fn read_prefix(path: &Path, max_bytes: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; max_bytes];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// The latest gate verdict for the build badge: scan `reviews_dir`'s `*.md`,
/// parse each file's first `Verdict:` header, return the NEWEST-by-mtime file
/// carrying a parseable verdict. The rolling GATE-FINDINGS bus + any file with
/// no verdict line are naturally skipped. Dir absent / nothing parseable =>
/// `{available:false}` (the badge renders "unknown"); never a panic.
pub fn latest_gate_verdict(reviews_dir: &Path) -> Value {
    let Ok(entries) = std::fs::read_dir(reviews_dir) else {
        return json!({ "available": false, "detail": "reviews dir unreadable/absent" });
    };
    let mut best: Option<(std::time::SystemTime, String, String)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(content) = read_prefix(&path, 8192) else {
            continue;
        };
        let Some(verdict) = content.lines().find_map(parse_verdict_token) else {
            continue;
        };
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if best.as_ref().is_none_or(|(t, _, _)| mtime > *t) {
            best = Some((mtime, verdict, name));
        }
    }
    match best {
        Some((_, verdict, file)) => json!({ "available": true, "verdict": verdict, "file": file }),
        None => json!({ "available": false, "detail": "no parseable gate verdict found" }),
    }
}

/// Evidence payloads are operator-readable JSONB of unbounded size; the
/// panel truncates at 4KB per row (design §5 operator amendment). The
/// LEDGER row stays whole — only this payload is cut, and the cut is
/// explicit (`truncated: true` + byte count), never silent.
fn truncate_evidence(evidence: &Value) -> Value {
    let serialized = evidence.to_string();
    if serialized.len() <= 4096 {
        return evidence.clone();
    }
    let mut end = 4096;
    while !serialized.is_char_boundary(end) {
        end -= 1;
    }
    json!({
        "truncated": true,
        "bytes_total": serialized.len(),
        "preview": &serialized[..end],
    })
}

/// An unavailable ledger sub-surface (uniform shape for the two arrays).
fn ledger_unavailable(detail: &str) -> Value {
    json!({ "available": false, "detail": detail, "rows": [] })
}

/// R7 — the cognition panel. Counters and budgets ride the daemon-shaped
/// "cognition" view, which stays ABSENT until synthesis-in-main composes a
/// cognition strategy: structurally-zero counters would read as "all
/// clear" to an operator (the verifier's escalating vacuous-data class),
/// so absence renders as an explicit status, never fabricated zeros. The
/// belief listing (evidence + provenance, click-to-expand in the shell)
/// and the calibration-scope enumeration are ROTA's OWN R7 ledger queries
/// over the R5 read pool. RotaState deliberately gains NO budget fields:
/// the daemon constructs RotaState as a struct literal (fortuna-live
/// main.rs — track A's file), so budgets ride the view when synthesis
/// wires them rather than breaking that construction site.
async fn view_cognition(State(s): State<RotaState>) -> impl IntoResponse {
    let (generated_at, counters) = {
        let snap = s.snapshot.read().await;
        (
            snap.generated_at.clone(),
            snap.views.get("cognition").cloned(),
        )
    }; // R8: snapshot lock released before any query below.
    let mut out = match counters {
        Some(Value::Object(map)) => Value::Object(map),
        _ => json!({
            "counters_status": "unavailable",
            "detail": "no cognition strategy composed yet (synthesis-in-main pending)",
        }),
    };
    let (beliefs, scopes) = match &s.pool {
        None => (
            ledger_unavailable("postgres capability absent (standalone ROTA)"),
            ledger_unavailable("postgres capability absent (standalone ROTA)"),
        ),
        Some(pool) => {
            let beliefs = match BeliefsRepo::new(pool.clone()).recent(20).await {
                Ok(rows) => {
                    let rows: Vec<Value> = rows
                        .into_iter()
                        .map(|r| {
                            json!({
                                "belief_id": r.belief_id,
                                "created_at": r.created_at,
                                "event_id": r.event_id,
                                "p": r.p,
                                "p_raw": r.p_raw,
                                "status": r.status,
                                "brier": r.brier,
                                "clv_bps": r.clv_bps,
                                "evidence": truncate_evidence(&r.evidence),
                                "provenance": r.provenance,
                            })
                        })
                        .collect();
                    json!({ "available": true, "rows": rows })
                }
                Err(e) => {
                    // Neutral detail only — never raw sqlx text to the view.
                    eprintln!("rota: cognition belief read degraded: {e}");
                    ledger_unavailable("belief read unavailable (dashboard pool degraded)")
                }
            };
            let scopes = match CalibrationParamsRepo::new(pool.clone()).scopes().await {
                Ok(rows) => {
                    let rows: Vec<Value> = rows
                        .into_iter()
                        .map(|r| {
                            json!({
                                "model_id": r.model_id,
                                "strategy": r.strategy,
                                "category": r.category,
                                "kind": r.kind,
                                "version": r.version,
                                "effective_at": r.effective_at,
                            })
                        })
                        .collect();
                    json!({ "available": true, "rows": rows })
                }
                Err(e) => {
                    eprintln!("rota: cognition scope read degraded: {e}");
                    ledger_unavailable("calibration read unavailable (dashboard pool degraded)")
                }
            };
            (beliefs, scopes)
        }
    };
    if let Some(obj) = out.as_object_mut() {
        obj.insert("generated_at".to_string(), json!(generated_at));
        obj.insert("recent_beliefs".to_string(), beliefs);
        obj.insert("calibration_scopes".to_string(), scopes);
    }
    Json(out)
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
    // A malformed timestamp leaves "now" UNKNOWN — never default it to 0 (that
    // clamps every age to 0 => a fabricated healthy:true, audit-tail-fix gate
    // finding #1). None here flows through to unhealthy + null age below.
    let now_ms = UtcTimestamp::parse_iso8601(generated_at)
        .map(|t| t.epoch_millis())
        .ok();
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
                // age only when BOTH the file mtime AND a parseable "now" are
                // known; otherwise None => unhealthy + null age (never faked).
                let age = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .zip(now_ms)
                    .map(|(mtime, now)| ((now - mtime).max(0)) / 1000);
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

/// Recent gate REJECTIONS for the /gates panel (§5 `recent_rejections`): the
/// audit `gate_decision` rows whose serialized `GateCheckRecord` verdict is
/// `Reject` (the per-check trail `runner.rs` writes via the I5 audit path),
/// newest-first. The wanted fields are extracted as TEXT in SQL
/// (`payload->>'check'` etc.) so no JSONB decode / sqlx-json feature is needed —
/// the same deliberate runtime-sqlx choice as [`audit_tail_page`] (a
/// schema-pinned read-only dashboard query, kept off the sqlx-offline cache).
/// NOTE: the bus `gate_reject` event is a SEPARATE kind (the live event stream)
/// — the audit TABLE carries `gate_decision`, which is what this queries.
/// Returns `(audit_id, at, intent_ref, check, reason)`; `limit` clamped to [1, 200].
pub async fn recent_gate_rejections_page(
    pool: &PgPool,
    limit: i64,
) -> Result<
    Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )>,
    sqlx::Error,
> {
    let limit = limit.clamp(1, 200);
    sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        "SELECT audit_id, at, ref_id, payload->>'check', payload->>'reason' \
         FROM audit \
         WHERE kind = 'gate_decision' AND payload->>'verdict' = 'Reject' \
         ORDER BY at DESC, audit_id DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Recent settlement-watchdog events for the /settlement panel (§5
/// `recent_watchdog_events`): the audit `watchdog` rows (sub-kinds
/// settlement_overdue / dispute_freeze / orphaned_position — the runner writes
/// them via `self.audit("watchdog", Some(market), {kind: <sub>})`). The §5
/// `kind` is the payload sub-kind (`payload->>'kind'`); `market_ref` is the
/// row's `ref_id`. TEXT-extract in SQL → runtime sqlx (the [`audit_tail_page`]
/// precedent; off the sqlx-offline cache). Returns (audit_id, at, market_ref,
/// kind), newest-first; `limit` clamped to [1, 200].
pub async fn recent_watchdog_events_page(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<(String, String, Option<String>, Option<String>)>, sqlx::Error> {
    let limit = limit.clamp(1, 200);
    sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
        "SELECT audit_id, at, ref_id, payload->>'kind' \
         FROM audit \
         WHERE kind = 'watchdog' \
         ORDER BY at DESC, audit_id DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
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
        Err(e) => {
            // A FAILED read is not "available" — degrade exactly like the
            // no-pool case (available:false + a neutral detail), HTTP 200 not
            // 500. Never leak raw sqlx/Pg error text to the client; the cause
            // is logged server-side (localhost, read-only surface).
            eprintln!("rota: audit-tail read degraded: {e}");
            Json(json!({
                "available": false,
                "detail": "audit read unavailable (dashboard pool degraded)",
                "rows": [],
                "next_after": null,
            }))
        }
    }
}

async fn shell() -> impl IntoResponse {
    // The §9 mark inlines into the header at serve time (compile-time
    // asset, zero fs reads, zero CDN — Section 1/R12).
    (
        StatusCode::OK,
        Html(ROTA_SHELL.replace("<!--ROTA_LOGO-->", ROTA_LOGO_SVG)),
    )
}

/// The §9 cornucopia/wheel mark, committed at assets/rota/logo.svg and
/// baked in at compile time (a missing asset is a build error, not a 404).
const ROTA_LOGO_SVG: &str = include_str!("../../../assets/rota/logo.svg");

/// Favicon = the §9 mark as SVG (browsers accept SVG favicons). Replaces
/// the interim 204 stub (rota-slices gate F2: the bare /favicon.ico probe
/// must never 404 — an R12 console-error criterion).
async fn favicon() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        ROTA_LOGO_SVG,
    )
}

/// GET /assets/rota/logo.svg (§3 route table).
async fn logo_asset() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        ROTA_LOGO_SVG,
    )
}

/// The gold-on-black instrument shell (operator tokens, Section 2):
/// vanilla JS short-poll at the §0.4 cadences, per-panel renderers with a
/// raw-JSON expander (instrument honesty: the formatted view never hides
/// the payload), cognition evidence as click-to-expand rows (operator
/// amendment), cents rendered as dollars, times labeled UTC (R6). The
/// panel grid line is the ONLY responsive concession (R11). Inlined as a
/// const — zero build step, zero CDN (R12 / Section 1).
const ROTA_SHELL: &str = r#"<!doctype html><html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>FORTUNA · ROTA</title>
<link rel="icon" href="/assets/rota/logo.svg" type="image/svg+xml">
<style>
  :root{--bg:#0A0A0B;--card:#141416;--gold:#D4AF37;--amber:#FFB84D;
        --text:#EDEDEA;--halt:#FF3B30;--ok:#30D158}
  *{box-sizing:border-box}
  body{margin:0;background:var(--bg);color:var(--text);
       font-family:system-ui,sans-serif}
  header{display:flex;align-items:center;gap:12px;padding:12px 20px;
         border-bottom:1px solid var(--gold)}
  header .logo svg{height:28px;width:28px;display:block}
  header .mark{color:var(--gold);font-weight:700;letter-spacing:2px}
  header .sub{color:var(--amber);font-size:12px;letter-spacing:3px}
  .grid{display:grid;gap:14px;padding:18px;
        grid-template-columns:repeat(auto-fit,minmax(320px,1fr))}
  .panel{background:var(--card);border:1px solid #2a2a2d;border-radius:6px;padding:14px}
  .panel h2{margin:0 0 8px;color:var(--gold);font-size:13px;
            letter-spacing:1.5px;text-transform:uppercase}
  .mono,pre,.kv,.row{font-family:"JetBrains Mono",ui-monospace,monospace;
      font-variant-numeric:tabular-nums lining-nums}
  pre{margin:0;font-size:11px;color:var(--text);white-space:pre-wrap}
  .kv{display:flex;justify-content:space-between;gap:10px;font-size:12px;
      padding:2px 0;border-bottom:1px dotted #222}
  .kv span{color:#9a9a96}.kv b{color:var(--text);font-weight:500}
  .kv b.gold{color:var(--gold)}
  .row{font-size:12px;padding:2px 0;color:var(--text)}
  .pill{display:inline-block;padding:1px 8px;border-radius:8px;font-size:11px}
  .pill.ok{border:1px solid var(--ok);color:var(--ok)}
  .pill.bad{border:1px solid var(--halt);color:var(--halt)}
  .pill.dim{border:1px solid #555;color:#999}
  .warn{color:var(--amber);font-size:12px;padding:2px 0}
  .asof{color:#6e6e6a;font-size:10px;margin-top:8px}
  details.raw summary,details.belief summary{cursor:pointer;font-size:11px;color:#8a8a86}
  details.raw{margin-top:6px}
  details.belief{padding:2px 0;border-bottom:1px dotted #222}
  details.belief summary{font-size:12px;color:var(--text)}
  details.belief pre{padding:6px 0 4px 12px;color:#bdbdb8}
  #halt{display:none;position:fixed;inset:0;background:var(--halt);
        color:#fff;align-items:center;justify-content:center;flex-direction:column;
        font-size:28px;letter-spacing:3px;z-index:9}
  #halt .rearm{font-size:13px;letter-spacing:1px;margin-top:16px;opacity:.85;
        max-width:80%;text-align:center}
</style></head><body>
<div id="halt">SYSTEM HALTED<div class="rearm">a re-arm clears the ledger halt, but this running daemon resumes only on restart — run: fortuna stop &amp;&amp; fortuna start</div></div>
<header><span class="logo"><!--ROTA_LOGO--></span>
<span class="mark">FORTUNA</span><span class="sub">ROTA</span></header>
<div class="grid">
  <div class="panel"><h2>Health</h2><div id="health">…</div></div>
  <div class="panel"><h2>Money</h2><div id="money">…</div></div>
  <div class="panel"><h2>Gates</h2><div id="gates">…</div></div>
  <div class="panel"><h2>Cognition</h2><div id="cognition">…</div></div>
  <div class="panel"><h2>Settlement</h2><div id="settlement">…</div></div>
  <div class="panel"><h2>Streams</h2><div id="streams">…</div></div>
  <div class="panel"><h2>Audit tail</h2><div id="audit">…</div></div>
</div>
<script>
const B="/api/rota/v1/";
const esc=s=>String(s).replace(/[&<>"]/g,m=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[m]));
function fmtCents(c){if(c===null||c===undefined)return "—";
  return (c/100).toLocaleString("en-US",{style:"currency",currency:"USD"});}
const kv=(k,v,gold)=>`<div class="kv"><span>${esc(k)}</span><b${gold?' class="gold"':''}>${v}</b></div>`;
const pill=(t,c)=>`<span class="pill ${c}">${esc(t)}</span>`;
const raw=j=>`<details class="raw"><summary>raw</summary><pre>${esc(JSON.stringify(j,null,2))}</pre></details>`;
const asof=j=>j.generated_at?`<div class="asof">as of ${esc(j.generated_at)} UTC</div>`:"";
function gate(j){if(j&&j.status==="unavailable")return `<div class="warn">${esc(j.detail||"unavailable")}</div>`;return null;}
const R={
 health(j){let h=kv("halt",j.halt_active?pill("HALTED","bad"):pill("clear","ok"));
  if(j.halt_reason)h+=kv("reason",esc(j.halt_reason));
  if(j.rearm_requires_restart)h+=kv("re-arm","takes effect only on restart: fortuna stop &amp;&amp; fortuna start");
  h+=kv("ticks",j.ticks_total??"—")
   +kv("fill p90/p95/p99 ms",`${j.fill_latency_p90_ms??"—"} / ${j.fill_latency_p95_ms??"—"} / ${j.fill_latency_p99_ms??"—"}`)
   +kv("dead-man",j.dead_man_last_ping_age_secs==null?pill("external","dim"):esc(j.dead_man_last_ping_age_secs)+"s ago");
  (j.venues||[]).forEach(v=>h+=kv("venue "+esc(v.id),(v.healthy?pill("ok","ok"):pill("errors","bad"))+" "+(v.api_error_count??0)+" err"));
  return h;},
 money(j){let h="";if(j.basis)h+=kv("basis",pill(esc(j.basis),"dim"));
  h+=kv("settled",fmtCents(j.settled_cents),1)+kv("committed",fmtCents(j.committed_cents))
   +kv("floating",fmtCents(j.floating_cents))+kv("total",fmtCents(j.total_cents),1);
  const ps=j.positions||[];h+=kv("positions",ps.length);
  ps.slice(0,8).forEach(p=>h+=`<div class="row">${esc(p.market)} y${p.yes_qty??0}/n${p.no_qty??0} ${fmtCents(p.realized_pnl_cents)}</div>`);
  return h;},
 gates(j){let h=kv("rejections",j.total_rejections??"—",1);
  (j.rejections_by_check||[]).forEach(r=>h+=kv("· "+esc(r.check),r.count));
  return h;},
 cognition(j){let h="";
  if(j.counters_status)h+=`<div class="warn">${esc(j.detail||j.counters_status)}</div>`;
  else{h+=kv("spend today",fmtCents(j.mind_spend_today_cents),1)
   +kv("daily budget",fmtCents(j.daily_budget_cents))
   +kv("failures",j.cognition_failures_total??"—")+kv("breaches",j.budget_breaches_total??"—");}
  const sc=j.calibration_scopes;
  if(sc&&sc.available)sc.rows.forEach(s=>h+=kv("cal "+esc(s.model_id)+"/"+esc(s.kind),"v"+s.version));
  else if(sc)h+=`<div class="warn">scopes: ${esc(sc.detail)}</div>`;
  const rb=j.recent_beliefs;
  if(rb&&rb.available){rb.rows.forEach(b=>{
    h+=`<details class="belief"><summary>${esc(b.belief_id.slice(-8))} p=${b.p} (${esc(b.status)})</summary>`
     +`<pre>evidence: ${esc(JSON.stringify(b.evidence,null,1))}\nprovenance: ${esc(JSON.stringify(b.provenance,null,1))}</pre></details>`;});
   if(!rb.rows.length)h+=`<div class="row">no beliefs yet</div>`;}
  else if(rb)h+=`<div class="warn">beliefs: ${esc(rb.detail)}</div>`;
  return h;},
 settlement(j){return kv("in limbo",fmtCents(j.capital_in_limbo_cents),1)
  +kv("overdue",j.settlements_overdue??"—")+kv("discrepancies",j.discrepancies_open??"—")
  +kv("voids",j.settlement_voids_total??"—")+kv("reversals",j.settlement_reversals_total??"—");},
 streams(j){let h=kv("venue api errors",j.venue_api_errors_total??"—");
  (j.recorder||[]).forEach(r=>h+=kv("rec "+esc(r.stream),
    (r.healthy?pill("live","ok"):pill("stale","bad"))+" "+(r.last_capture_age_secs??"—")+"s"));
  return h;},
 audit(j){if(!j.available)return `<div class="warn">${esc(j.detail||"unavailable")}</div>`;
  let h="";j.rows.slice(-12).forEach(r=>h+=`<div class="row">${esc(r.at)} UTC ${esc(r.kind)}${r.actor?" · "+esc(r.actor):""}</div>`);
  return h||`<div class="row">no audit rows yet</div>`;}
};
async function poll(name){const el=document.getElementById(name);
 try{const r=await fetch(B+name);const j=await r.json();
  if(name==="health")document.getElementById("halt").style.display=j.halt_active?"flex":"none";
  el.innerHTML=(gate(j)??R[name](j))+raw(j)+asof(j);
 }catch(e){el.innerHTML=`<div class="warn">unreachable: ${esc(e)}</div>`;}}
function every(ms,names){names.forEach(poll);setInterval(()=>names.forEach(poll),ms);}
every(2000,["health","audit"]);every(5000,["money","gates"]);
every(10000,["cognition","settlement"]);every(15000,["streams"]);
</script></body></html>"#;
