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
use fortuna_ledger::{BeliefsRepo, CalibrationParamsRepo, ScalarBeliefsRepo};
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
        .route("/api/rota/v1/ingest_sources", get(view_ingest_sources))
        .route("/api/rota/v1/ingest_feed", get(view_ingest_feed))
        .route("/api/rota/v1/ingest_funnel", get(view_ingest_funnel))
        .route("/api/rota/v1/fills", get(view_fills))
        .route("/api/rota/v1/strategies", get(view_strategies))
        .route("/api/rota/v1/working_orders", get(view_working_orders))
        .route("/api/rota/v1/discovery", get(view_discovery))
        .route("/api/rota/v1/personas", get(view_personas))
        .route("/api/rota/v1/persona_scores", get(view_persona_scores))
        .route("/api/rota/v1/persona_pipeline", get(view_persona_pipeline))
        .route("/api/rota/v1/analyses", get(view_analyses))
        .route("/api/rota/v1/forecasts", get(view_forecasts))
        .route("/api/rota/v1/forecast_feed", get(view_forecast_feed))
        .route("/api/rota/v1/db", get(view_db))
        .route("/api/rota/v1/telemetry", get(view_telemetry))
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

/// D-contract V2 Sources Health (ingestion-observability §4). A pure read of the
/// daemon-shaped `ingest_sources` board envelope — the generic
/// `{title, columns, rows, summary}` shape every ingestion board (V1-V6) shares
/// (§4 "one envelope, rendered generically"). Absent => unavailable: the
/// daemon's OBS-2 publish shapes this from the live `IngestionTelemetry`
/// (fortuna-sources, on main); until that publish is wired (track-A drive seam),
/// the board renders honest-degraded, never a fabricated zero. ROTA stays a pure
/// projection — zero ingestion-crate dependency, exactly like `view_health`.
async fn view_ingest_sources(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "ingest_sources").await)
}

/// D-contract V1 Live Signal Feed (ingestion-observability §4) — the marquee
/// view: the recently ingested/dropped signals newest-first with their actual
/// (redacted) payload summary. The same generic board envelope as V2, served
/// from `snapshot.views["ingest_feed"]`; the daemon's OBS-2 publish shapes it
/// from `IngestionTelemetry.recent` (the SignalRecord ring). Honest-degraded
/// until that lands. Untrusted summary text stays quoted data, esc()'d in the
/// renderer (spec 5.11) — never interpreted.
async fn view_ingest_feed(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "ingest_feed").await)
}

/// D-contract V3 Ingest Funnel (ingestion-observability §4) — the process at a
/// glance: the pipeline stages (fetched → validated → normalized → persisted)
/// with per-stage retention % and drop-offs, so the operator sees WHERE signal
/// is lost. Same generic board envelope (a stage table); served from
/// `snapshot.views["ingest_funnel"]`. The daemon's OBS-2 publish shapes it from
/// `IngestionTelemetry.funnel` — and (CONTRACT) emits the loop-side stages as
/// null until the ingestion loop feeds them, so an unwired stage reads "—", never
/// a fabricated 0 that would look like "everything dropped after validation".
async fn view_ingest_funnel(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "ingest_funnel").await)
}

/// Strategy P&L (mission item 3) — per-strategy realized PnL, fees, fill count,
/// and open exposure, shaped daemon-side from `runner.digest_snapshot()` (the
/// same attribution the daily digest uses) into `snapshot.views["strategies"]`;
/// price columns render as dollars (the `cents` flag). Unrealized PnL is the
/// mark-loop gap (Money board) — realized only. Absent => unavailable.
async fn view_strategies(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "strategies").await)
}

/// Working orders (mission item 3: "trades being executed" — the LIVE side) — the
/// intents currently resting at the venue (submitted / acked / partially-filled, not
/// yet terminal), shaped daemon-side from `runner.manager().intents()` filtered by
/// `IntentStatus::is_working()` into `snapshot.views["working_orders"]`; the limit
/// renders as dollars (the `cents` flag) and the status as a pill. Empty when nothing
/// rests (honest). Absent => unavailable.
async fn view_working_orders(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "working_orders").await)
}

/// Telemetry (mission item 6: "the Prometheus stack on the console") — the metric
/// SERIES the daemon exports (the same `MetricsRegistry` the `/metrics` exposition is
/// rendered from), grouped by subsystem, shaped daemon-side into
/// `snapshot.views["telemetry"]` (`MetricsRegistry::telemetry_board`). A read_view
/// passthrough — ROTA never parses Prometheus text (R2). Absent => unavailable.
async fn view_telemetry(State(s): State<RotaState>) -> impl IntoResponse {
    Json(read_view(&s, "telemetry").await)
}

/// Recent fills — the trades EXECUTED, from the durable `fills` ledger (mission
/// item 3: "trades being executed"). Runtime sqlx (the audit-tail / belief-
/// lifecycle precedent — read-only, schema-pinned, no offline cache). Shaped into
/// the generic board envelope; price/fee render as dollars (the `cents` column
/// flag). NOTE: a fill carries no `strategy` (attribution lives in the runtime
/// PositionBook, not the row) and no realized PnL — the per-strategy P&L view
/// (a future views_from board) and the honest unrealized-PnL gap (no mark loop,
/// same as the Money board) are ledgered. Degrades to unavailable (HTTP 200)
/// without the pool, never a 500.
async fn view_fills(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match recent_fills(&pool, 50).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let n = rows.len();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(
                    |(market, side, action, _venue, price, qty, fee, maker, at)| {
                        json!({
                            "at": at, "market": market, "side": side, "action": action,
                            "qty": qty, "price_cents": price, "fee_cents": fee,
                            "maker": if maker { "maker" } else { "taker" },
                        })
                    },
                )
                .collect();
            Json(json!({
                "title": "Recent Fills",
                "generated_at": generated_at,
                "columns": [
                    {"key":"at","label":"Time (UTC)"},
                    {"key":"market","label":"Market"},
                    {"key":"side","label":"Side"},
                    {"key":"action","label":"Act"},
                    {"key":"qty","label":"Qty"},
                    {"key":"price_cents","label":"Price","cents":true},
                    {"key":"fee_cents","label":"Fee","cents":true},
                    {"key":"maker","label":"Liq"},
                ],
                "rows": json_rows,
                "summary": {"fills": n},
            }))
        }
        Err(e) => {
            // Never leak raw sqlx/Pg text — degrade like the no-pool case.
            eprintln!("rota: fills read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "fills read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One recent fill row: (market_id, side, action, venue, price_cents, qty,
/// fee_cents, is_maker, at).
type FillRowTuple = (String, String, String, String, i64, i64, i64, bool, String);

/// Recent fills, newest-first (`at` DESC), limit clamped to [1, 200]. Runtime
/// sqlx by deliberate choice (the audit-tail precedent): a read-only dashboard
/// query, schema-pinned by the migration, kept out of the sqlx-offline cache.
pub async fn recent_fills(pool: &PgPool, limit: i64) -> Result<Vec<FillRowTuple>, sqlx::Error> {
    let limit = limit.clamp(1, 200);
    sqlx::query_as::<_, FillRowTuple>(
        "SELECT market_id, side, action, venue, price_cents, qty, fee_cents, is_maker, at \
         FROM fills ORDER BY at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Discovery — the canonical events the system tracks and the markets mapped to
/// each (mission item 4: "the canonical EVENTS we have, the markets/series under
/// them"). Read of the `events` ledger LEFT-JOINed to `market_event_edges`
/// (COUNT DISTINCT market_id = the markets mapped to the event, supersession-
/// safe). Runtime sqlx (the audit-tail precedent). Degrades to unavailable
/// (HTTP 200) without the pool, never a 500.
async fn view_discovery(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match recent_discovery_events(&pool, 50).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let n = rows.len();
            let total_markets: i64 = rows.iter().map(|r| r.5).sum();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(
                    |(event_id, statement, category, status, benchmark_at, markets)| {
                        json!({
                            "event_id": event_id, "statement": statement, "category": category,
                            "status": status, "benchmark_at": benchmark_at, "markets": markets,
                        })
                    },
                )
                .collect();
            Json(json!({
                "title": "Discovery — Events",
                "generated_at": generated_at,
                "columns": [
                    {"key":"statement","label":"Event"},
                    {"key":"category","label":"Category"},
                    {"key":"status","label":"Status"},
                    {"key":"benchmark_at","label":"Benchmark (UTC)"},
                    {"key":"markets","label":"Markets"},
                ],
                "rows": json_rows,
                "summary": {"events": n, "markets_mapped": total_markets},
            }))
        }
        Err(e) => {
            eprintln!("rota: discovery read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "discovery read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One discovery row: (event_id, statement, category, status, benchmark_at,
/// markets_mapped).
type DiscoveryRowTuple = (String, String, String, String, String, i64);

/// Recent canonical events with their mapped-market count (DISTINCT market_id over
/// market_event_edges, supersession-safe), newest-first. Runtime sqlx (audit-tail
/// precedent); limit clamped to [1, 200].
pub async fn recent_discovery_events(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<DiscoveryRowTuple>, sqlx::Error> {
    let limit = limit.clamp(1, 200);
    sqlx::query_as::<_, DiscoveryRowTuple>(
        "SELECT e.event_id, e.statement, e.category, e.status, e.benchmark_at, \
                COUNT(DISTINCT mee.market_id) AS markets \
         FROM events e \
         LEFT JOIN market_event_edges mee ON mee.event_id = e.event_id \
         GROUP BY e.event_id, e.statement, e.category, e.status, e.benchmark_at, e.created_at \
         ORDER BY e.created_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Personas registry (mission item 1: HOW beliefs are formed — the roster of
/// analysts). The §20.1 registry half: every (persona_id, version) with its domain,
/// tier, lifecycle status, method-file integrity hash, the signal kinds it may read,
/// and effective date. The §20.1 SCORECARD half (per-persona Brier/CLV/verdict) is
/// DATA-BLOCKED on track-E persona scoring + persona-dim'd calibration_params — a
/// ledgered follow-on, never a fabricated score. Degrades to unavailable (HTTP 200)
/// without the pool.
async fn view_personas(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match persona_registry(&pool).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let versions = rows.len();
            let active = rows.iter().filter(|r| r.3 == "active").count();
            let mut ids: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            let personas = ids.len();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(
                    |(persona, version, domain, status, tier, method, reads, effective_at)| {
                        json!({
                            "persona": persona, "version": version, "domain": domain,
                            "status": status, "tier": tier, "method": method,
                            "reads": reads, "effective_at": effective_at,
                        })
                    },
                )
                .collect();
            Json(json!({
                "title": "Personas",
                "generated_at": generated_at,
                "columns": [
                    {"key":"persona","label":"Persona"},
                    {"key":"version","label":"Ver"},
                    {"key":"domain","label":"Domain"},
                    {"key":"status","label":"Status","pill":true},
                    {"key":"tier","label":"Tier"},
                    {"key":"method","label":"Method"},
                    {"key":"reads","label":"Reads"},
                    {"key":"effective_at","label":"Effective (UTC)"},
                ],
                "rows": json_rows,
                "summary": {"personas": personas, "versions": versions, "active": active},
            }))
        }
        Err(e) => {
            eprintln!("rota: personas read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "personas read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One persona registry row: (persona_id, version, domain, status, tier,
/// method_hash[..8], reads_signal_kinds joined, effective_at).
type PersonaRowTuple = (String, i32, String, String, String, String, String, String);

/// The persona registry, grouped by persona, newest version first. All columns are
/// operator-authored config (NOT untrusted signal/model data); the method hash is
/// truncated to its 8-char provenance prefix; `reads_signal_kinds` (a JSONB string
/// array) is flattened to a comma list. Runtime sqlx (audit-tail precedent).
pub async fn persona_registry(pool: &PgPool) -> Result<Vec<PersonaRowTuple>, sqlx::Error> {
    sqlx::query_as::<_, PersonaRowTuple>(
        "SELECT persona_id, version, domain, status, tier, \
                substr(method_hash, 1, 8) AS method, \
                array_to_string(ARRAY(SELECT jsonb_array_elements_text(reads_signal_kinds)), ', ') \
                  AS reads, \
                effective_at \
         FROM personas \
         ORDER BY persona_id ASC, version DESC",
    )
    .fetch_all(pool)
    .await
}

/// Persona Scorecard (track-E §20.1 OUTCOMES half: "are the personas any good?").
/// Per persona, the calibration of its RESOLVED beliefs: how many resolved, the mean
/// Brier (LOWER = better) and the mean CLV (closing-line value, bps; HIGHER = better)
/// — aggregated from the binary `beliefs` table grouped by `provenance->>'persona_id'`
/// (the fan-out the persona runner writes, track-E E.4a). A PURE projection (AVG /
/// COUNT): the §11 promote/retire VERDICT + the raw/market baselines +
/// calibration_quality are NOT computed here (cognition logic / unpersisted baselines
/// — ledgered); the board shows only the honest EVALUATING(n/60) progress, never a
/// fabricated promote/retire decision. Runtime sqlx (audit-tail precedent). Empty
/// until the persona runner is daemon-wired → honest unavailable.
async fn view_persona_scores(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match persona_scorecard(&pool).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let resolved_total: i64 = rows.iter().map(|r| r.1).sum();
            let n_personas = rows.len();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(|(persona, n_resolved, brier, clv_bps)| {
                    let brier = (brier * 10_000.0).round() / 10_000.0;
                    let clv = clv_bps.map(|c| (c * 100.0).round() / 100.0);
                    // §11 evaluation progress toward the 60-sample gate. The
                    // PROMOTABLE / RETIRE-CANDIDATE verdict needs the raw/market
                    // baselines (not persisted) — so ROTA shows progress only, never
                    // a fabricated promote/retire decision.
                    let verdict = if n_resolved < 60 {
                        format!("evaluating ({n_resolved}/60)")
                    } else {
                        "n\u{2265}60 \u{00B7} verdict pending baselines".to_string()
                    };
                    json!({
                        "persona": persona, "n_resolved": n_resolved,
                        "brier": brier, "clv_bps": clv, "verdict": verdict,
                    })
                })
                .collect();
            Json(json!({
                "title": "Persona Scorecard",
                "generated_at": generated_at,
                "columns": [
                    {"key":"persona","label":"Persona"},
                    {"key":"n_resolved","label":"Resolved"},
                    {"key":"brier","label":"Mean Brier (lower=better)"},
                    {"key":"clv_bps","label":"Mean CLV bps (higher=better)"},
                    {"key":"verdict","label":"Verdict"},
                ],
                "rows": json_rows,
                "summary": {"personas": n_personas, "resolved": resolved_total},
            }))
        }
        Err(e) => {
            eprintln!("rota: persona_scores read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "persona scores read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One persona-scorecard row: (persona_id, n_resolved, mean_brier, mean_clv_bps).
type PersonaScoreTuple = (String, i64, f64, Option<f64>);

/// Per-persona calibration over RESOLVED+scored beliefs: count, mean Brier, mean CLV —
/// grouped by the belief provenance's `persona_id`. Pure AVG/COUNT projection (no
/// cognition logic, no untrusted-data render; the `persona_id` is operator-authored
/// config). Runtime sqlx (audit-tail precedent).
pub async fn persona_scorecard(pool: &PgPool) -> Result<Vec<PersonaScoreTuple>, sqlx::Error> {
    sqlx::query_as::<_, PersonaScoreTuple>(
        "SELECT provenance->>'persona_id' AS persona, \
                COUNT(*) AS n_resolved, \
                AVG(brier) AS brier, \
                AVG(clv_bps) AS clv_bps \
         FROM beliefs \
         WHERE provenance->>'persona_id' IS NOT NULL \
           AND status = 'resolved' \
           AND brier IS NOT NULL \
         GROUP BY provenance->>'persona_id' \
         ORDER BY provenance->>'persona_id' ASC",
    )
    .fetch_all(pool)
    .await
}

/// Persona pipeline funnel (track-E §20.4): per persona, the cognition PIPELINE at a
/// glance — analyses produced → beliefs fanned out → beliefs resolved. The conversion
/// at each stage is the pipeline-health signal (many analyses but few beliefs = low
/// fanout; many beliefs but few resolved = slow resolution). Counts only — no content
/// exposed. Degrades to unavailable (HTTP 200) without the pool.
async fn view_persona_pipeline(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match persona_pipeline(&pool).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let n = rows.len();
            let analyses_total: i64 = rows.iter().map(|r| r.1).sum();
            let beliefs_total: i64 = rows.iter().map(|r| r.2).sum();
            let resolved_total: i64 = rows.iter().map(|r| r.3).sum();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(|(persona, analyses, beliefs, resolved)| {
                    json!({
                        "persona": persona, "analyses": analyses,
                        "beliefs": beliefs, "resolved": resolved,
                    })
                })
                .collect();
            Json(json!({
                "title": "Persona Pipeline",
                "generated_at": generated_at,
                "columns": [
                    {"key":"persona","label":"Persona"},
                    {"key":"analyses","label":"Analyses"},
                    {"key":"beliefs","label":"Beliefs"},
                    {"key":"resolved","label":"Resolved"},
                ],
                "rows": json_rows,
                "summary": {
                    "personas": n, "analyses": analyses_total,
                    "beliefs": beliefs_total, "resolved": resolved_total,
                },
            }))
        }
        Err(e) => {
            eprintln!("rota: persona_pipeline read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "persona pipeline read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One persona-pipeline row: (persona_id, analyses, beliefs, resolved).
type PersonaPipelineTuple = (String, i64, i64, i64);

/// Per-persona pipeline funnel: analyses produced (`domain_analyses`), beliefs fanned
/// out + resolved (`beliefs.provenance ->> 'persona_id'`), over the persona registry
/// universe (LEFT JOIN so a registered persona with no activity reads honest 0s). All
/// COUNTs → bigint → i64. Runtime sqlx (audit-tail precedent).
pub async fn persona_pipeline(pool: &PgPool) -> Result<Vec<PersonaPipelineTuple>, sqlx::Error> {
    sqlx::query_as::<_, PersonaPipelineTuple>(
        "SELECT p.pid AS persona, \
                COALESCE(a.n, 0)::bigint AS analyses, \
                COALESCE(b.n, 0)::bigint AS beliefs, \
                COALESCE(b.resolved, 0)::bigint AS resolved \
         FROM (SELECT DISTINCT persona_id AS pid FROM personas) p \
         LEFT JOIN (SELECT persona_id, COUNT(*) AS n FROM domain_analyses GROUP BY persona_id) a \
                ON a.persona_id = p.pid \
         LEFT JOIN (SELECT provenance ->> 'persona_id' AS persona_id, COUNT(*) AS n, \
                           COUNT(*) FILTER (WHERE status = 'resolved') AS resolved \
                    FROM beliefs WHERE provenance ->> 'persona_id' IS NOT NULL GROUP BY 1) b \
                ON b.persona_id = p.pid \
         ORDER BY p.pid ASC",
    )
    .fetch_all(pool)
    .await
}

/// Domain-analysis artifacts browser (mission item 1 / track-E §20.2: the "whole
/// process" view — the persisted analyses personas produce that beliefs are built
/// from). The artifact ledger: which persona analysed which region, when, at what
/// cost, the content-hash replay anchor, and the supersession status, newest-first.
/// This view renders STRUCTURAL METADATA ONLY (ids, hashes, region_key, persona,
/// timestamps, cost, status — all escaped by the renderer); the `findings` /
/// `signal_manifest` drill-in (which carries UNTRUSTED model output) is the §20.2
/// expander follow-on, ledgered. Degrades to unavailable (HTTP 200) without the pool.
async fn view_analyses(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match recent_analyses(&pool, 50).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let n = rows.len();
            let open = rows.iter().filter(|r| r.7 == "open").count();
            let cost_total: i64 = rows.iter().map(|r| r.5).sum();
            let beliefs_total: i64 = rows.iter().map(|r| r.8).sum();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(
                    |(
                        analysis_id,
                        persona,
                        domain,
                        region_key,
                        produced_at,
                        cost,
                        hash,
                        status,
                        beliefs,
                    )| {
                        json!({
                            "analysis_id": analysis_id, "persona": persona, "domain": domain,
                            "region_key": region_key, "produced_at": produced_at,
                            "cost_cents": cost, "content_hash": hash, "status": status,
                            "beliefs": beliefs,
                        })
                    },
                )
                .collect();
            Json(json!({
                "title": "Domain Analyses",
                "generated_at": generated_at,
                "columns": [
                    {"key":"persona","label":"Persona"},
                    {"key":"domain","label":"Domain"},
                    {"key":"region_key","label":"Region"},
                    {"key":"produced_at","label":"Produced (UTC)"},
                    {"key":"cost_cents","label":"Cost","cents":true},
                    {"key":"beliefs","label":"Beliefs"},
                    {"key":"content_hash","label":"Hash"},
                    {"key":"status","label":"Status","pill":true},
                ],
                "rows": json_rows,
                "summary": {"analyses": n, "open": open, "cost_cents": cost_total, "beliefs": beliefs_total},
            }))
        }
        Err(e) => {
            eprintln!("rota: analyses read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "analyses read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One analysis row: (analysis_id, persona "id@version", domain, region_key,
/// produced_at, cost_cents, content_hash[..8], status, beliefs_fanout).
type AnalysisRowTuple = (
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    i64,
);

/// Recent domain-analysis artifacts, newest-first. Persona is rendered "id@version";
/// the content hash is truncated to its 8-char replay-anchor prefix; `beliefs` is the
/// §20.2 artifact→belief FANOUT — how many beliefs were built FROM this analysis
/// (`beliefs.provenance ->> 'analysis_id'`), the cognition pipeline's downstream
/// output. STRUCTURAL metadata only — `findings`/`signal_manifest` (untrusted model
/// output) are NOT selected here (the per-belief expander is a further follow-on).
/// Runtime sqlx (audit-tail precedent); limit clamped to [1, 200].
pub async fn recent_analyses(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<AnalysisRowTuple>, sqlx::Error> {
    let limit = limit.clamp(1, 200);
    sqlx::query_as::<_, AnalysisRowTuple>(
        "SELECT da.analysis_id, da.persona_id || '@' || da.persona_version::text AS persona, \
                da.domain, da.region_key, da.produced_at, da.cost_cents, \
                substr(da.content_hash, 1, 8) AS content_hash, da.status, \
                (SELECT COUNT(*) FROM beliefs b \
                   WHERE b.provenance ->> 'analysis_id' = da.analysis_id) AS beliefs \
         FROM domain_analyses da \
         ORDER BY da.produced_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Forecasts scorecard (track-C §9.1: "the outcomes of the whole process" — how
/// well-calibrated each scalar-forecast PRODUCER is). The calibration headline: per
/// (producer, scoring rule), the mean score (CRPS — LOWER is better) over the
/// RESOLVED forecasts, and how many resolved, with the unit so the CRPS scale reads.
/// A `scalar_beliefs ⋈ belief_scores` aggregate (realized only). SCORE METADATA ONLY
/// — the untrusted `quantiles`/`provenance` JSONB (model output) are NOT selected
/// here; the recent-forecast feed (quantile fans + realized), coverage_bps, and the
/// sparkline are §9.1 follow-ons (ledgered). Runtime sqlx (audit-tail precedent).
/// Empty until track-C's daemon persist (slice 4) lands → honest unavailable, never
/// a fabricated score.
async fn view_forecasts(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match forecast_scorecard(&pool).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let scored: i64 = rows.iter().map(|r| r.4).sum();
            let mut producers: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
            producers.sort_unstable();
            producers.dedup();
            let n_producers = producers.len();
            let mut rules: Vec<&str> = rows.iter().map(|r| r.2.as_str()).collect();
            rules.sort_unstable();
            rules.dedup();
            let n_rules = rules.len();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(|(producer, unit, rule_id, mean, resolved_n, coverage)| {
                    // CRPS rounded to 6dp for display (lower is better); the raw f64
                    // stays the honest source — this only trims display noise.
                    let mean_crps = (mean * 1_000_000.0).round() / 1_000_000.0;
                    // Band coverage as a percentage (1dp); a calibrated producer ≈ 80.
                    let coverage_pct = (coverage * 1000.0).round() / 10.0;
                    json!({
                        "producer": producer, "unit": unit, "rule_id": rule_id,
                        "mean_crps": mean_crps, "resolved_n": resolved_n,
                        "coverage_pct": coverage_pct,
                    })
                })
                .collect();
            Json(json!({
                "title": "Forecasts",
                "generated_at": generated_at,
                "columns": [
                    {"key":"producer","label":"Producer"},
                    {"key":"unit","label":"Unit"},
                    {"key":"rule_id","label":"Scorer"},
                    {"key":"mean_crps","label":"Mean CRPS (lower=better)"},
                    {"key":"coverage_pct","label":"Band cover % (~80 ideal)"},
                    {"key":"resolved_n","label":"Resolved"},
                ],
                "rows": json_rows,
                "summary": {"producers": n_producers, "rules": n_rules, "scored": scored},
            }))
        }
        Err(e) => {
            eprintln!("rota: forecasts read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "forecasts read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One forecast-scorecard row: (producer, unit, rule_id, mean_score CRPS, resolved_n,
/// band_coverage ∈ [0,1]).
type ForecastRowTuple = (String, String, String, f64, i64, f64);

/// Per-(producer, scoring rule) calibration over RESOLVED scalar forecasts: mean
/// score (CRPS, lower-better), the resolved count, the unit (a producer's forecasts
/// share one), and the 0.1–0.9 BAND COVERAGE (the fraction of resolved forecasts whose
/// realized outcome fell inside the central 80% interval — a well-calibrated producer
/// is ~0.8). `scalar_beliefs ⋈ belief_scores`, realized only. The coverage reads the
/// q=0.1 / q=0.9 VALUES out of the `quantiles` fan (numbers, for the band check) — the
/// raw fan + `provenance` are still never rendered (the untrusted-data boundary holds).
/// Runtime sqlx (audit-tail precedent).
pub async fn forecast_scorecard(pool: &PgPool) -> Result<Vec<ForecastRowTuple>, sqlx::Error> {
    sqlx::query_as::<_, ForecastRowTuple>(
        "SELECT sb.producer, MIN(sb.unit) AS unit, bs.rule_id, \
                AVG(bs.score) AS mean_score, COUNT(*) AS resolved_n, \
                AVG(CASE WHEN sb.realized_value >= \
                          (SELECT (e->>'v')::float8 FROM jsonb_array_elements(sb.quantiles) e \
                             WHERE (e->>'q')::float8 = 0.1) \
                      AND sb.realized_value <= \
                          (SELECT (e->>'v')::float8 FROM jsonb_array_elements(sb.quantiles) e \
                             WHERE (e->>'q')::float8 = 0.9) \
                     THEN 1.0 ELSE 0.0 END)::float8 AS band_coverage \
         FROM scalar_beliefs sb \
         JOIN belief_scores bs ON bs.belief_id = sb.belief_id \
         WHERE sb.realized_value IS NOT NULL \
         GROUP BY sb.producer, bs.rule_id \
         ORDER BY sb.producer ASC, bs.rule_id ASC",
    )
    .fetch_all(pool)
    .await
}

/// Forecast feed (track-C §9.1 + the operator "completely see the belief and
/// everything" want, 2026-06-13) — the recent individual scalar beliefs,
/// newest-first, each FULLY inspectable. The summary line carries the at-a-glance
/// essentials (producer · event · q=0.5 MEDIAN · unit · resolved/pending · realized);
/// click-to-expand reveals the WHOLE quantile FAN (q/v pairs), the producer's
/// EVIDENCE (its work — e.g. estimate / point_forecast / remaining_candles), and the
/// provenance. The scalar companion to the binary /cognition belief panel; both
/// render each belief as a `<details>` expander (the same `truncate_evidence` +
/// `provenance_summary` precedent).
///
/// UNTRUSTED DATA (spec 5.11): the quantile fan, evidence, and provenance are
/// model+venue output. Only the numeric q/v are read from the fan (`clean_quantiles`
/// drops any malformed entry — never raw-rendered); evidence + provenance are
/// size-capped (`truncate_evidence`) and rendered as DATA (esc'd JSON), never
/// interpreted. The `scalar_beliefs.provenance` column is the daemon's wrapper
/// `{"provenance":…,"evidence":…}` (persist_scalar_beliefs, daemon.rs) — split back
/// here; a non-wrapped row is shown whole as provenance, never hidden. Reads
/// `ScalarBeliefsRepo::recent` (newest-first by ULID belief_id; NO ledger change).
/// Degrades to unavailable (HTTP 200) without the pool.
async fn view_forecast_feed(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match ScalarBeliefsRepo::new(pool).recent(50).await {
        Ok(beliefs) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let n = beliefs.len();
            let resolved = beliefs
                .iter()
                .filter(|b| b.realized_value.is_some())
                .count();
            // Round a displayed forecast/outcome to 6dp; the raw f64 stays the honest
            // source. null (no q=0.5 / unresolved) → "—" at the renderer.
            let round6 = |v: Option<f64>| v.map(|x| (x * 1_000_000.0).round() / 1_000_000.0);
            let json_rows: Vec<Value> = beliefs
                .into_iter()
                .map(|b| {
                    let fan = clean_quantiles(&b.quantiles);
                    let median = fan
                        .iter()
                        .find(|(q, _)| (q - 0.5).abs() < 1e-9)
                        .map(|(_, v)| *v);
                    // The producer wraps {"provenance":…,"evidence":…} into the single
                    // provenance column (persist_scalar_beliefs); split it back. The
                    // wrapper is detected by BOTH keys (its exact contract) — any other
                    // shape is shown WHOLE as provenance, never partially nulled.
                    let col = &b.provenance;
                    let wrapped = col.get("evidence").is_some() && col.get("provenance").is_some();
                    let (evidence, prov_inner) = if wrapped {
                        (
                            col.get("evidence").cloned().unwrap_or(Value::Null),
                            col.get("provenance").cloned().unwrap_or(Value::Null),
                        )
                    } else {
                        (Value::Null, col.clone())
                    };
                    json!({
                        "belief_id": b.belief_id,
                        "producer": b.producer,
                        "event_key": b.event_key,
                        "unit": b.unit,
                        "horizon": b.horizon,
                        "created_at": b.created_at,
                        "quantiles": fan
                            .iter()
                            .map(|(q, v)| json!({"q": *q, "v": *v}))
                            .collect::<Vec<_>>(),
                        "median": round6(median),
                        "realized": round6(b.realized_value),
                        "resolved_at": b.resolved_at,
                        "status": if b.realized_value.is_some() { "resolved" } else { "pending" },
                        "prov": provenance_summary(&prov_inner),
                        "evidence": truncate_evidence(&evidence),
                        "provenance": truncate_evidence(&prov_inner),
                    })
                })
                .collect();
            Json(json!({
                "title": "Forecast Feed",
                "generated_at": generated_at,
                "available": true,
                "rows": json_rows,
                "summary": {"forecasts": n, "resolved": resolved, "pending": n - resolved},
            }))
        }
        Err(e) => {
            eprintln!("rota: forecast_feed read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "forecast feed read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// Extract the quantile fan as clean (q, v) f64 pairs, ascending by q. The
/// `quantiles` JSONB is untrusted model output (spec 5.11) — only the numeric q/v
/// are read; any malformed / extra-keyed entry is skipped, never rendered as raw
/// JSON. Drives both the at-a-glance median (q=0.5) and the expander fan.
fn clean_quantiles(quantiles: &Value) -> Vec<(f64, f64)> {
    let mut fan: Vec<(f64, f64)> = quantiles
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|e| Some((e.get("q")?.as_f64()?, e.get("v")?.as_f64()?)))
                .collect()
        })
        .unwrap_or_default();
    fan.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    fan
}

/// DB visibility (mission item 5: "honest visibility into the actual tables —
/// counts"). An exact-COUNT sweep over every ledger table, busiest-first. Runtime
/// sqlx (the audit-tail precedent). NOTE: exact COUNT(*) is accurate at the current
/// Sim scale; when a table grows large in live trading (audit/signals), switch this
/// to pg reltuples estimates or a less-frequent poll — ledgered in GAPS. Degrades
/// to unavailable (HTTP 200) without the pool.
async fn view_db(State(s): State<RotaState>) -> impl IntoResponse {
    let Some(pool) = s.pool.clone() else {
        return Json(json!({
            "status": "unavailable",
            "detail": "postgres capability absent (standalone ROTA)",
        }));
    };
    match db_table_counts(&pool).await {
        Ok(rows) => {
            let generated_at = { s.snapshot.read().await.generated_at.clone() }; // R8
            let tables = rows.len();
            let total: i64 = rows.iter().map(|r| r.1).sum();
            let json_rows: Vec<Value> = rows
                .into_iter()
                .map(|(table, n)| json!({ "table": table, "rows": n }))
                .collect();
            Json(json!({
                "title": "Database",
                "generated_at": generated_at,
                "columns": [
                    {"key":"table","label":"Table"},
                    {"key":"rows","label":"Rows"},
                ],
                "rows": json_rows,
                "summary": {"tables": tables, "total_rows": total},
            }))
        }
        Err(e) => {
            eprintln!("rota: db read degraded: {e}");
            Json(json!({
                "status": "unavailable",
                "detail": "db read unavailable (dashboard pool degraded)",
            }))
        }
    }
}

/// One DB-inventory row: (table_name, row_count).
type DbCountRow = (String, i64);

/// Exact row counts for every ledger table, busiest-first. Static query (every
/// table name is a literal — no interpolation). A new migration's table must be
/// ADDED here (the list is hardcoded; an absent table just isn't shown, never an
/// error). Runtime sqlx (audit-tail precedent).
pub async fn db_table_counts(pool: &PgPool) -> Result<Vec<DbCountRow>, sqlx::Error> {
    sqlx::query_as::<_, DbCountRow>(
        "SELECT 'audit' t, COUNT(*) n FROM audit \
         UNION ALL SELECT 'belief_scores', COUNT(*) FROM belief_scores \
         UNION ALL SELECT 'beliefs', COUNT(*) FROM beliefs \
         UNION ALL SELECT 'calibration_params', COUNT(*) FROM calibration_params \
         UNION ALL SELECT 'discrepancies', COUNT(*) FROM discrepancies \
         UNION ALL SELECT 'discrepancy_resolutions', COUNT(*) FROM discrepancy_resolutions \
         UNION ALL SELECT 'domain_analyses', COUNT(*) FROM domain_analyses \
         UNION ALL SELECT 'events', COUNT(*) FROM events \
         UNION ALL SELECT 'exec_cursors', COUNT(*) FROM exec_cursors \
         UNION ALL SELECT 'fills', COUNT(*) FROM fills \
         UNION ALL SELECT 'halt_events', COUNT(*) FROM halt_events \
         UNION ALL SELECT 'intent_events', COUNT(*) FROM intent_events \
         UNION ALL SELECT 'journal', COUNT(*) FROM journal \
         UNION ALL SELECT 'lessons', COUNT(*) FROM lessons \
         UNION ALL SELECT 'market_event_edges', COUNT(*) FROM market_event_edges \
         UNION ALL SELECT 'market_snapshots', COUNT(*) FROM market_snapshots \
         UNION ALL SELECT 'personas', COUNT(*) FROM personas \
         UNION ALL SELECT 'price_snapshots', COUNT(*) FROM price_snapshots \
         UNION ALL SELECT 'reservation_events', COUNT(*) FROM reservation_events \
         UNION ALL SELECT 'scalar_beliefs', COUNT(*) FROM scalar_beliefs \
         UNION ALL SELECT 'settlement_entries', COUNT(*) FROM settlement_entries \
         UNION ALL SELECT 'signals', COUNT(*) FROM signals \
         UNION ALL SELECT 'source_registry', COUNT(*) FROM source_registry \
         UNION ALL SELECT 'tradability_scores', COUNT(*) FROM tradability_scores \
         ORDER BY 2 DESC, 1",
    )
    .fetch_all(pool)
    .await
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

/// A LABELED summary of a belief's provenance JSONB (§20.3: "which source/persona,
/// model_id, run_at, cost — the reasoning made legible"). Extracts the known keys —
/// system-authored config, NOT untrusted model output — so the cognition expander can
/// render a clean labeled line; the whole `provenance` is still served alongside.
/// Missing keys are null (the renderer shows "—"); a non-object provenance yields all
/// nulls (never a panic).
fn provenance_summary(prov: &Value) -> Value {
    let get = |k: &str| prov.get(k).cloned().unwrap_or(Value::Null);
    json!({
        "persona_id": get("persona_id"),
        "persona_version": get("persona_version"),
        "model_id": get("model_id"),
        "cost_cents": get("cost_cents"),
        "analysis_id": get("analysis_id"),
        "run_at": get("run_at"),
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
    let (beliefs, scopes, lifecycle) = match &s.pool {
        None => (
            ledger_unavailable("postgres capability absent (standalone ROTA)"),
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
                                "prov": provenance_summary(&r.provenance),
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
            // The belief LIFECYCLE aggregates (status distribution + the
            // resolved beliefs' calibration outcome) — the "is belief formation
            // healthy + are we calibrated" read. Degrades like the others.
            let lifecycle = match belief_lifecycle(pool).await {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("rota: cognition lifecycle read degraded: {e}");
                    ledger_unavailable("belief lifecycle unavailable (dashboard pool degraded)")
                }
            };
            (beliefs, scopes, lifecycle)
        }
    };
    if let Some(obj) = out.as_object_mut() {
        obj.insert("generated_at".to_string(), json!(generated_at));
        obj.insert("recent_beliefs".to_string(), beliefs);
        obj.insert("calibration_scopes".to_string(), scopes);
        obj.insert("belief_lifecycle".to_string(), lifecycle);
    }
    Json(out)
}

/// Belief LIFECYCLE aggregates for the cognition panel — the status distribution
/// (open / resolved / superseded / abandoned) and the resolved beliefs'
/// calibration OUTCOME (n, mean Brier, mean CLV bps): "is belief formation
/// healthy, and are we calibrated with edge?". The D-contract V6 per-belief
/// STRATEGY + realized-dollar-PnL columns are SCHEMA-BLOCKED — there is no
/// belief→trade link on the current schema (GAPS) — so this surfaces the
/// calibration edge proxy (`clv_bps`), NEVER a fabricated PnL. Runtime sqlx (the
/// audit-tail precedent): a read-only dashboard aggregate, schema-pinned by the
/// migration, kept out of the offline cache. The `status` CHECK pins the buckets.
async fn belief_lifecycle(pool: &PgPool) -> Result<Value, sqlx::Error> {
    let counts =
        sqlx::query_as::<_, (String, i64)>("SELECT status, COUNT(*) FROM beliefs GROUP BY status")
            .fetch_all(pool)
            .await?;
    // Pin all four buckets so an absent status reads 0, not missing (honest).
    let mut by_status = serde_json::Map::new();
    for s in ["open", "resolved", "superseded", "abandoned"] {
        by_status.insert(s.to_string(), json!(0));
    }
    for (status, n) in counts {
        by_status.insert(status, json!(n));
    }
    // Calibration outcome over the SCORED resolved beliefs (brier present). No
    // resolved-scored rows => COUNT 0 + NULL averages => null (renders "—").
    let (resolved_scored_n, mean_brier, mean_clv_bps): (i64, Option<f64>, Option<f64>) =
        sqlx::query_as(
            "SELECT COUNT(*), AVG(brier), AVG(clv_bps) FROM beliefs \
             WHERE status = 'resolved' AND brier IS NOT NULL",
        )
        .fetch_one(pool)
        .await?;
    Ok(json!({
        "available": true,
        "by_status": Value::Object(by_status),
        "resolved_scored_n": resolved_scored_n,
        "mean_brier": mean_brier,
        "mean_clv_bps": mean_clv_bps,
    }))
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
  .panel.wide{grid-column:1/-1}
  table.board{width:100%;border-collapse:collapse;font-size:11px;
      font-variant-numeric:tabular-nums lining-nums}
  table.board th{text-align:right;color:var(--gold);font-weight:600;
      padding:3px 8px;border-bottom:1px solid #2a2a2d;white-space:nowrap}
  table.board th:first-child{text-align:left}
  table.board td{text-align:right;padding:3px 8px;border-bottom:1px dotted #222;white-space:nowrap}
  table.board td:first-child{text-align:left;color:var(--text)}
  .bsum{font-size:12px;color:#9a9a96;margin-bottom:8px}
  .bsum b{color:var(--text)}
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
  <div class="panel wide"><h2>Sources Health</h2><div id="ingest_sources">…</div></div>
  <div class="panel wide"><h2>Live Signal Feed</h2><div id="ingest_feed">…</div></div>
  <div class="panel wide"><h2>Ingest Funnel</h2><div id="ingest_funnel">…</div></div>
  <div class="panel wide"><h2>Recent Fills</h2><div id="fills">…</div></div>
  <div class="panel wide"><h2>Working Orders</h2><div id="working_orders">…</div></div>
  <div class="panel wide"><h2>Strategy P&amp;L</h2><div id="strategies">…</div></div>
  <div class="panel wide"><h2>Discovery — Events</h2><div id="discovery">…</div></div>
  <div class="panel wide"><h2>Personas</h2><div id="personas">…</div></div>
  <div class="panel wide"><h2>Persona Scorecard</h2><div id="persona_scores">…</div></div>
  <div class="panel wide"><h2>Persona Pipeline</h2><div id="persona_pipeline">…</div></div>
  <div class="panel wide"><h2>Domain Analyses</h2><div id="analyses">…</div></div>
  <div class="panel wide"><h2>Forecasts</h2><div id="forecasts">…</div></div>
  <div class="panel wide"><h2>Forecast Feed</h2><div id="forecast_feed">…</div></div>
  <div class="panel wide"><h2>Database</h2><div id="db">…</div></div>
  <div class="panel wide"><h2>Telemetry</h2><div id="telemetry">…</div></div>
  <div class="panel"><h2>Audit tail</h2><div id="audit">…</div></div>
</div>
<script>
const B="/api/rota/v1/";
const esc=s=>String(s).replace(/[&<>"]/g,m=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;"}[m]));
function fmtCents(c){if(c===null||c===undefined)return "—";
  return (c/100).toLocaleString("en-US",{style:"currency",currency:"USD"});}
const kv=(k,v,gold)=>`<div class="kv"><span>${esc(k)}</span><b${gold?' class="gold"':''}>${v}</b></div>`;
const pill=(t,c)=>`<span class="pill ${esc(c)}">${esc(t)}</span>`;
const raw=j=>`<details class="raw"><summary>raw</summary><pre>${esc(JSON.stringify(j,null,2))}</pre></details>`;
const asof=j=>j.generated_at?`<div class="asof">as of ${esc(j.generated_at)} UTC</div>`:"";
function gate(j){if(j&&j.status==="unavailable")return `<div class="warn">${esc(j.detail||"unavailable")}</div>`;return null;}
// A status-token pill: green for healthy/accepted, red for quarantined, muted
// otherwise (degraded, dropped:*). Drives any column the envelope flags `pill`.
const valuePill=v=>{if(v==null)return "—";const s=String(v);const c=(s==="healthy"||s==="accepted"||s==="active")?"ok":s==="quarantined"?"bad":"dim";return pill(s,c);};
// §20.3: a LEGIBLE one-line provenance summary (persona · model · cost · analysis ·
// run) from the belief's `prov` block — the labeled "which source/persona drove this".
const provLine=p=>{if(!p)return "";const t=[];
 if(p.persona_id)t.push("persona "+esc(p.persona_id)+(p.persona_version!=null?"@"+esc(p.persona_version):""));
 if(p.model_id)t.push("model "+esc(p.model_id));
 if(p.cost_cents!=null)t.push("cost "+fmtCents(p.cost_cents));
 if(p.analysis_id)t.push("analysis "+esc(String(p.analysis_id).slice(0,8)));
 if(p.run_at)t.push("run "+esc(p.run_at));
 return t.length?`<div class="row dim">${t.join(" · ")}</div>`:"";};
// Generic D-contract board: {title, columns:[{key,label,pill?}], rows, summary}
// rendered as a table — every ingestion board (V1-V6) reuses this with only a new
// view key (§4 "render any board generically"). A column flagged `pill:true`
// renders its value as a status pill; nulls render "—". Cells, labels, and
// summary values are all esc()'d (untrusted ingestion data, spec 5.11).
function boardTable(j){
  const cols=(j&&j.columns)||[],rows=(j&&j.rows)||[];
  if(!cols.length)return `<div class="warn">no columns</div>`;
  let h="";
  if(j.summary)h+=`<div class="bsum">`+Object.entries(j.summary).map(([k,v])=>`${esc(k)} <b>${esc(v)}</b>`).join(" · ")+`</div>`;
  if(!rows.length)return h+`<div class="row">no rows yet</div>`;
  h+=`<table class="board"><thead><tr>`+cols.map(c=>`<th>${esc(c.label||c.key)}</th>`).join("")+`</tr></thead><tbody>`;
  rows.forEach(r=>{h+=`<tr>`+cols.map(c=>{const v=r[c.key];
    return c.pill?`<td>${valuePill(v)}</td>`:c.cents?`<td>${fmtCents(v)}</td>`:`<td>${v==null?"—":esc(v)}</td>`;}).join("")+`</tr>`;});
  h+=`</tbody></table>`;
  return h;
}
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
  const lc=j.belief_lifecycle;
  if(lc&&lc.available){const bs=lc.by_status||{};
   h+=kv("beliefs",`open ${bs.open??0} · resolved ${bs.resolved??0} · superseded ${bs.superseded??0} · abandoned ${bs.abandoned??0}`,1)
    +kv("calibration",lc.resolved_scored_n?`n=${lc.resolved_scored_n} · Brier ${lc.mean_brier!=null?(+lc.mean_brier).toFixed(3):"—"} · CLV ${lc.mean_clv_bps!=null?Math.round(lc.mean_clv_bps)+"bp":"—"}`:"no resolved beliefs yet");}
  else if(lc&&lc.detail)h+=`<div class="warn">lifecycle: ${esc(lc.detail)}</div>`;
  const sc=j.calibration_scopes;
  if(sc&&sc.available)sc.rows.forEach(s=>h+=kv("cal "+esc(s.model_id)+"/"+esc(s.kind),"v"+s.version));
  else if(sc)h+=`<div class="warn">scopes: ${esc(sc.detail)}</div>`;
  const rb=j.recent_beliefs;
  if(rb&&rb.available){rb.rows.forEach(b=>{
    h+=`<details class="belief"><summary>${esc(b.belief_id.slice(-8))} p=${b.p} (${esc(b.status)})</summary>`
     +provLine(b.prov)
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
 ingest_sources(j){return boardTable(j);},
 ingest_feed(j){return boardTable(j);},
 ingest_funnel(j){return boardTable(j);},
 fills(j){return boardTable(j);},
 strategies(j){return boardTable(j);},
 working_orders(j){return boardTable(j);},
 discovery(j){return boardTable(j);},
 personas(j){return boardTable(j);},
 persona_scores(j){return boardTable(j);},
 persona_pipeline(j){return boardTable(j);},
 analyses(j){return boardTable(j);},
 forecasts(j){return boardTable(j);},
 // The scalar-belief feed: each recent forecast as a click-to-expand <details>
 // (the /cognition belief-panel precedent) so the operator can "completely see the
 // belief and everything" — the summary carries producer · event · median · status ·
 // realized; expanding shows the WHOLE quantile fan + the producer's evidence +
 // provenance. All of q/v, evidence, provenance are untrusted model output (5.11):
 // numbers are esc()'d, evidence/provenance are esc()'d JSON, never interpreted.
 forecast_feed(j){let h="";
  if(j.summary)h+=`<div class="bsum">`+Object.entries(j.summary).map(([k,v])=>`${esc(k)} <b>${esc(v)}</b>`).join(" · ")+`</div>`;
  const rows=j.rows||[];
  if(!rows.length)return h+`<div class="row">no forecasts yet</div>`;
  rows.forEach(b=>{
   const st=b.status==="resolved"?pill("resolved","ok"):pill("pending","dim");
   const out=b.realized!=null?` → realized <b>${esc(b.realized)}</b>`:"";
   const fan=(b.quantiles||[]).map(q=>`q${(+q.q).toFixed(2)} ${esc(q.v)}`).join("   ");
   h+=`<details class="belief"><summary>${esc(b.producer)} · ${esc(b.event_key)} · median ${b.median==null?"—":esc(b.median)} ${esc(b.unit)} ${st}${out}</summary>`
    +provLine(b.prov)
    +`<div class="row dim">${esc(String(b.belief_id).slice(-8))} · horizon ${esc(b.horizon)} · fan: ${fan||"—"}</div>`
    +`<pre>evidence: ${esc(JSON.stringify(b.evidence,null,1))}\nprovenance: ${esc(JSON.stringify(b.provenance,null,1))}</pre></details>`;});
  return h;},
 db(j){return boardTable(j);},
 telemetry(j){return boardTable(j);},
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
every(2000,["health","audit"]);every(5000,["money","gates","ingest_sources","ingest_feed","ingest_funnel"]);
every(10000,["cognition","settlement","fills","strategies","working_orders"]);every(15000,["streams","discovery"]);
every(30000,["db","personas","persona_scores","persona_pipeline","analyses","forecasts","forecast_feed","telemetry"]);
</script></body></html>"#;
