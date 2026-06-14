//! ROTA local bringup harness — track-B mission 2 (TOTAL ROTA OBSERVABILITY),
//! queue item 0 (the keystone). See GAPS.md "TRACK B — RE-MISSIONED ... TOTAL
//! ROTA OBSERVABILITY".
//!
//! Purpose: stand the read-only ROTA console up locally against a SEEDED
//! throwaway Postgres + a representative `DashboardSnapshot`, so the operator
//! (and the screenshot-verification step) can see every existing board render
//! with REAL rows — the north-star "ROTA up locally". It is the reusable rig
//! the later C/D/E boards are screenshot-verified through as their data lands.
//!
//! What it does, in order: resolve+GUARD a throwaway DB URL, connect (runs the
//! migrations), SEED representative rows (one weather event; three beliefs —
//! one resolved-and-scored, one carrying persona provenance for E §20.3; two
//! calibration scopes; five audit rows of distinct kinds), shape a faithful
//! `snapshot.views` (mirroring `fortuna-live/src/views.rs::views_from`), point
//! a temp perishable dir at a today-dated stream file so the Streams board's
//! recorder section is live, then serve the full console at `/rota`.
//!
//! SAFETY: this binary WIPES+SEEDS the database it points at, so it reads ONLY
//! `ROTA_LOCAL_DATABASE_URL` (never the ambient `DATABASE_URL`) and REFUSES any
//! URL whose database name does not contain `rota_local`. Never point it at
//! real data. Read-only doctrine is unviolated — the WRITES here are the test
//! seed, not the dashboard; ROTA itself exposes zero mutating endpoints.
//!
//! Run (one-time DB create, then bring up):
//! ```text
//!   createdb fortuna_rota_local         # or: psql -c 'CREATE DATABASE fortuna_rota_local'
//!   cargo run -p fortuna-ops --example rota_local
//!   # -> open http://127.0.0.1:8799/rota   (override ROTA_LOCAL_ADDR to change)
//! ```

use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_ledger::{
    connect, connect_readonly_pool, AuditWriter, BeliefScoresRepo, BeliefsRepo,
    CalibrationParamsRepo, DomainAnalysesRepo, EventsRepo, LedgerError, PersonasRepo, PgPool,
    ScalarBeliefsRepo,
};
use fortuna_ops::dashboard::{serve_dashboard, DashboardSnapshot};
use fortuna_ops::metrics::MetricsRegistry;
use fortuna_ops::rota::RotaState;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

type BoxErr = Box<dyn std::error::Error>;

#[tokio::main]
async fn main() -> Result<(), BoxErr> {
    // 1. Resolve + GUARD the target DB. ONLY ROTA_LOCAL_DATABASE_URL — never the
    //    ambient DATABASE_URL — and the name MUST contain `rota_local`, because
    //    this binary seeds (writes) it. A misconfigured env can never reach the
    //    operator's data.
    let url = std::env::var("ROTA_LOCAL_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/fortuna_rota_local".to_string());
    if !url.contains("rota_local") {
        return Err(format!(
            "refusing to seed {url:?}: ROTA_LOCAL_DATABASE_URL must name a throwaway DB whose \
             name contains 'rota_local' (this binary WIPES+SEEDS it). Never point it at real \
             data or the operator's DATABASE_URL."
        )
        .into());
    }
    eprintln!("[rota_local] throwaway DB: {url}");

    // 2. Connect + migrate (MIGRATOR runs inside connect()).
    let pool = connect(&url).await?;

    // 3. Seed representative rows. Idempotent-by-tolerance: a re-run against a
    //    non-fresh DB warns and continues (PK/conflict on already-seeded rows)
    //    rather than aborting the bring-up.
    seed(&pool).await?;

    // 4. A faithful representative `snapshot.views` (the daemon-shaped boards
    //    that need no DB). `generated_at` is the live wall instant so the
    //    Streams recorder age reads fresh; everything stamps it (§5 contract).
    // A dev harness may read the wall clock directly — this is NOT the core
    // money path (the injected Clock rule binds gates/exec/state/venues, not an
    // example bring-up rig). Propagated, never swallowed to a wrong epoch-0.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;
    let generated_at = UtcTimestamp::from_epoch_millis(now_ms)?.to_iso8601();
    let snap = DashboardSnapshot {
        generated_at: generated_at.clone(),
        stage: "sim".to_string(),
        metrics_text: String::new(),
        boards: json!({}),
        views: representative_views(&generated_at),
    };

    // 5. Temp perishable dir with a today-dated stream file so the Streams
    //    board's recorder liveness section renders a live row (scan_recorder
    //    selects the dir by generated_at's date prefix and ages off its mtime).
    let perishable = temp_perishable(&generated_at)?;

    // 6. Read pool (R5 isolated dashboard pool) + RotaState carrying the live
    //    capabilities, so the DB-backed boards (cognition beliefs/scopes, audit
    //    tail) and the recorder section all light up.
    let read_pool = connect_readonly_pool(&url).await?;
    let state = RotaState {
        snapshot: Arc::new(RwLock::new(snap)),
        pool: Some(read_pool),
        perishable_dir: Some(Arc::new(perishable)),
        // The local console runs from the checkout, so the gate-verdict badge can
        // read docs/reviews; absent => "unknown" (degrades, never a 500).
        reviews_dir: Some(Arc::new(std::path::PathBuf::from("docs/reviews"))),
    };

    // 7. Serve the full console (legacy boards + the /rota tree).
    let addr = std::env::var("ROTA_LOCAL_ADDR").unwrap_or_else(|_| "127.0.0.1:8799".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let bound = listener.local_addr()?;
    eprintln!("[rota_local] ROTA console:  http://{bound}/rota");
    eprintln!("[rota_local] view JSON:     http://{bound}/api/rota/v1/health (etc.)");
    eprintln!("[rota_local] Ctrl-C to stop.");
    serve_dashboard(listener, state).await?;
    Ok(())
}

/// Warn-and-continue on a seed error so a re-run against a non-fresh DB (rows
/// already present) does not abort the bring-up.
fn warn_seed<T>(label: &str, r: Result<T, LedgerError>) {
    if let Err(e) = r {
        eprintln!("[rota_local] seed '{label}' skipped: {e}");
    }
}

/// Seed one weather event + the beliefs/calibration/audit rows the cognition
/// and audit boards read. Faithful, render-exercising data — never zeros.
async fn seed(pool: &PgPool) -> Result<(), BoxErr> {
    let event_id = "01J0EVENT0000000000000NYC";
    let events = EventsRepo::new(pool.clone());
    warn_seed(
        "event",
        events
            .create(
                event_id,
                "NYC daily high \u{2265} 65\u{00B0}F on 2026-06-13",
                "NWS CLI observed daily maximum temperature for KNYC",
                "nws.cli",
                Some("2026-06-13"),
                "2026-06-13T16:00:00.000Z",
                "weather",
                "2026-06-12T18:00:00.000Z",
            )
            .await,
    );

    let beliefs = BeliefsRepo::new(pool.clone());
    let evidence = json!({
        "source": "aeolus.forecast",
        "mu": 67.2, "sigma": 3.1, "threshold_f": 65.0,
        "model_version": "emos-sar-semos-v3", "run_at": "2026-06-12T18:05:00.000Z"
    });
    // A plain provenance (no persona) and a persona-shaped provenance (E §20.3):
    // ROTA already serializes provenance whole (rota.rs:175), so the persona
    // block surfaces in the cognition expander with zero handler change.
    let prov_plain = json!({
        "model_id": "emos-sar-semos-v3", "run_at": "2026-06-12T18:05:00.000Z", "cost_cents": 1
    });
    // analysis_id points at the seeded KNYC2 domain-analysis so the Analyses board's
    // belief-fanout column shows this analysis produced a downstream belief (§20.2).
    let prov_persona = json!({
        "model_id": "claude-sonnet-4-6", "run_at": "2026-06-12T18:06:30.000Z", "cost_cents": 2,
        "persona_id": "meteorologist", "persona_version": 3,
        "analysis_id": "01J0ANALYSIS000KNYC2",
        "analysis_content_hash": "a1b2c3d4e5f6a7b8"
    });

    let open_id = "01J0BELIEF000000000001OPEN";
    let resolved_id = "01J0BELIEF0000000002SCORED";
    let persona_id = "01J0BELIEF000000003PERSONA";
    warn_seed(
        "belief.open",
        beliefs
            .insert(
                open_id,
                "2026-06-13T11:40:00.000Z",
                event_id,
                0.71,
                0.66,
                "2026-06-13",
                &evidence,
                &prov_plain,
                None,
            )
            .await,
    );
    warn_seed(
        "belief.scored",
        beliefs
            .insert(
                resolved_id,
                "2026-06-12T18:06:00.000Z",
                event_id,
                0.62,
                0.58,
                "2026-06-13",
                &evidence,
                &prov_plain,
                None,
            )
            .await,
    );
    warn_seed(
        "belief.persona",
        beliefs
            .insert(
                persona_id,
                "2026-06-13T11:55:00.000Z",
                event_id,
                0.44,
                0.41,
                "2026-06-13",
                &evidence,
                &prov_persona,
                None,
            )
            .await,
    );
    // Resolve one belief so the board shows a real Brier + CLV (outcome=YES).
    warn_seed(
        "belief.resolve",
        beliefs
            .resolve_and_score(resolved_id, true, 0.1444, Some(38.0))
            .await,
    );
    // Resolve the meteorologist persona belief + add a scored macro_analyst belief,
    // so the Persona Scorecard (§20.1 outcomes) shows BOTH personas' real Brier/CLV.
    warn_seed(
        "belief.persona.resolve",
        beliefs
            .resolve_and_score(persona_id, false, 0.18, Some(22.0))
            .await,
    );
    let macro_belief = "01J0BELIEF000000004MACRO";
    warn_seed(
        "belief.macro",
        beliefs
            .insert(
                macro_belief,
                "2026-06-13T09:30:00.000Z",
                event_id,
                0.55,
                0.52,
                "2026-06-13",
                &evidence,
                &json!({
                    "model_id": "claude-sonnet-4-6", "run_at": "2026-06-13T09:31:00.000Z",
                    "cost_cents": 2, "persona_id": "macro_analyst", "persona_version": 1,
                    "analysis_id": "01J0ANALYSIS00000MACRO", "analysis_content_hash": "f00dcafe11223344"
                }),
                None,
            )
            .await,
    );
    warn_seed(
        "belief.macro.resolve",
        beliefs
            .resolve_and_score(macro_belief, true, 0.27, Some(-8.0))
            .await,
    );

    // A superseded belief (insert X, then a newer Y that supersedes it → X flips
    // to 'superseded') and an abandoned one (an open belief on a dead event) so
    // the cognition lifecycle shows the full status distribution.
    let superseded_id = "01J0BELIEFSUPERSEDED000001";
    let superseder_id = "01J0BELIEFSUPERSEDER000002";
    warn_seed(
        "belief.superseded",
        beliefs
            .insert(
                superseded_id,
                "2026-06-13T10:00:00.000Z",
                event_id,
                0.50,
                0.50,
                "2026-06-13",
                &evidence,
                &prov_plain,
                None,
            )
            .await,
    );
    warn_seed(
        "belief.superseder",
        beliefs
            .insert(
                superseder_id,
                "2026-06-13T11:30:00.000Z",
                event_id,
                0.55,
                0.52,
                "2026-06-13",
                &evidence,
                &prov_plain,
                Some(superseded_id),
            )
            .await,
    );
    let dead_event = "01J0EVENTDEADBOSTON0000001";
    warn_seed(
        "event.dead",
        events
            .create(
                dead_event,
                "Boston daily high \u{2265} 70\u{00B0}F on 2026-06-13",
                "NWS CLI observed daily maximum temperature for KBOS",
                "nws.cli",
                Some("2026-06-13"),
                "2026-06-13T16:00:00.000Z",
                "weather",
                "2026-06-12T18:00:00.000Z",
            )
            .await,
    );
    let abandoned_id = "01J0BELIEFABANDONED0000001";
    warn_seed(
        "belief.abandon-insert",
        beliefs
            .insert(
                abandoned_id,
                "2026-06-13T10:30:00.000Z",
                dead_event,
                0.40,
                0.40,
                "2026-06-13",
                &evidence,
                &prov_plain,
                None,
            )
            .await,
    );
    warn_seed(
        "belief.abandon",
        beliefs.abandon_open_for_event(dead_event).await.map(|_| ()),
    );

    let cal = CalibrationParamsRepo::new(pool.clone());
    warn_seed(
        "calibration.platt",
        cal.insert(
            "01J0CAL00000000000PLATT",
            "emos-sar-semos-v3",
            "weather_brackets",
            "weather",
            "platt",
            &json!({"a": 1.04, "b": -0.02}),
            2,
            "2026-06-10T00:00:00.000Z",
            "2026-06-10T00:00:00.000Z",
        )
        .await,
    );
    warn_seed(
        "calibration.shrinkage",
        cal.insert(
            "01J0CAL000000000SHRINK",
            "emos-sar-semos-v3",
            "weather_brackets",
            "weather",
            "shrinkage",
            &json!({"lambda": 0.15, "n": 174}),
            1,
            "2026-06-10T00:00:00.000Z",
            "2026-06-10T00:00:00.000Z",
        )
        .await,
    );

    // Personas registry (mission item 1) — a weather persona that has rev'd once
    // (v1 retired, superseded by the active v2) and an active macro persona, so the
    // Personas board renders the grouped/versioned registry with both lifecycle
    // states. Operator-authored config (not untrusted data).
    let personas = PersonasRepo::new(pool.clone());
    warn_seed(
        "persona.meteorologist.v1",
        personas
            .insert(
                "01J0PERSONA00000METEO1",
                "meteorologist",
                1,
                "weather",
                &json!(["temperature"]),
                &json!(["nws.afd"]),
                "cheap",
                "9f1c2d3e4a5b6c7d",
                "findings/v1",
                "retired",
                None,
                "2026-06-09T00:00:00.000Z",
                "2026-06-09T00:00:00.000Z",
            )
            .await,
    );
    warn_seed(
        "persona.meteorologist.v2",
        personas
            .insert(
                "01J0PERSONA00000METEO2",
                "meteorologist",
                2,
                "weather",
                &json!(["temperature", "nyc"]),
                &json!(["aeolus.forecast", "nws.observed_high"]),
                "cheap",
                "a1b2c3d4e5f60718",
                "findings/v1",
                "active",
                Some("01J0PERSONA00000METEO1"),
                "2026-06-12T00:00:00.000Z",
                "2026-06-12T00:00:00.000Z",
            )
            .await,
    );
    warn_seed(
        "persona.macro_analyst.v1",
        personas
            .insert(
                "01J0PERSONA0000MACRO1",
                "macro_analyst",
                1,
                "macro",
                &json!(["cpi", "nfp"]),
                &json!(["calendar.bls", "rss.fed"]),
                "synthesis",
                "deadbeefcafe1234",
                "findings/v1",
                "active",
                None,
                "2026-06-10T00:00:00.000Z",
                "2026-06-10T00:00:00.000Z",
            )
            .await,
    );

    // Domain-analysis artifacts (mission item 1 / §20.2) — two analyses of one NYC
    // region produced by meteorologist@2, where the later supersedes the earlier, so
    // the Analyses board shows the artifact ledger with an open + a superseded row.
    let analyses = DomainAnalysesRepo::new(pool.clone());
    warn_seed(
        "analysis.knyc.early",
        analyses
            .insert(
                "01J0ANALYSIS000KNYC1",
                "meteorologist",
                2,
                "weather",
                "weather:KNYC:tmax:2026-06-13",
                "2026-06-13T05:00:00.000Z",
                &json!([{"signal_id": "aeolus-1", "content_hash": "h-aeolus-1"}]),
                &json!({"thresholds": [{"ge": 86, "p": 0.78}], "sigma_trend": "widening"}),
                "0badc0de5a6b7c8d",
                "ctx-knyc-early",
                4,
                None,
                "2026-06-13T05:00:00.000Z",
            )
            .await,
    );
    warn_seed(
        "analysis.knyc.latest",
        analyses
            .insert(
                "01J0ANALYSIS000KNYC2",
                "meteorologist",
                2,
                "weather",
                "weather:KNYC:tmax:2026-06-13",
                "2026-06-13T11:00:00.000Z",
                &json!([{"signal_id": "aeolus-2", "content_hash": "h-aeolus-2"}]),
                &json!({"thresholds": [{"ge": 86, "p": 0.91}], "sigma_trend": "tightening"}),
                "feedface9e8d7c6b",
                "ctx-knyc-latest",
                6,
                Some("01J0ANALYSIS000KNYC1"),
                "2026-06-13T11:00:00.000Z",
            )
            .await,
    );

    // Scalar forecasts + CRPS scores (track-C §9.1) — two producers' resolved +
    // scored forecasts so the Forecasts scorecard shows per-producer mean CRPS
    // (funding_forecast over two rate forecasts; aeolus_weather over one celsius).
    let scalars = ScalarBeliefsRepo::new(pool.clone());
    let scores = BeliefScoresRepo::new(pool.clone());
    for (i, (id, producer, unit, realized, score)) in [
        (
            "01J0SB00000000000FF1",
            "funding_forecast",
            "rate",
            0.00012,
            0.00003,
        ),
        (
            "01J0SB00000000000FF2",
            "funding_forecast",
            "rate",
            0.00015,
            0.00005,
        ),
        (
            "01J0SB00000000000AW1",
            "aeolus_weather",
            "celsius",
            30.0,
            1.2,
        ),
    ]
    .iter()
    .enumerate()
    {
        // Unit-appropriate quantile fan so the Forecast Feed's median reads sensibly
        // against the realized outcome (a rate forecast ~0.0001; a celsius one ~29).
        let fan = if *unit == "rate" {
            json!([{"q":0.1,"v":0.00005},{"q":0.5,"v":0.0001},{"q":0.9,"v":0.00018}])
        } else {
            json!([{"q":0.1,"v":24.0},{"q":0.5,"v":29.0},{"q":0.9,"v":34.0}])
        };
        warn_seed(
            "scalar.belief",
            scalars
                .insert(
                    id,
                    producer,
                    "ev-key",
                    &fan,
                    unit,
                    "2026-06-13T16:00:00.000Z",
                    &json!({"strategy": producer}),
                    "2026-06-13T15:00:00.000Z",
                )
                .await,
        );
        warn_seed(
            "scalar.resolve",
            scalars
                .resolve(id, *realized, "2026-06-13T16:00:01.000Z")
                .await,
        );
        warn_seed(
            "scalar.score",
            scores
                .insert(
                    &format!("01J0SCORE000000000{i:03}"),
                    id,
                    "crps_pinball",
                    *score,
                    "2026-06-13T16:00:02.000Z",
                )
                .await,
        );
    }
    // A PENDING forecast (no realized value yet) so the Forecast Feed shows the
    // resolved-vs-pending mix — the "did the vendor call it" detail (newest-first).
    warn_seed(
        "scalar.belief.pending",
        scalars
            .insert(
                "01J0SB0000000000PEND1",
                "aeolus_weather",
                "KNYC:tmax:2026-06-14",
                &json!([{"q":0.1,"v":80.0},{"q":0.5,"v":86.0},{"q":0.9,"v":92.0}]),
                "celsius",
                "2026-06-14T16:00:00.000Z",
                &json!({"strategy": "aeolus_weather"}),
                "2026-06-13T17:30:00.000Z",
            )
            .await,
    );

    // A few executed fills for the Recent Fills board (raw INSERT — the fills
    // table is a plain append-only row table; the dashboard reads it read-only).
    for (id, market, side, action, price, qty, fee, maker, at) in [
        (
            "01FILLROTA000000000000001",
            "KXNYCHIGH-26JUN13-B65",
            "yes",
            "buy",
            41i64,
            40i64,
            12i64,
            false,
            "2026-06-13T11:55:10.000Z",
        ),
        (
            "01FILLROTA000000000000002",
            "KXCHIHIGH-26JUN13-B72",
            "no",
            "sell",
            55i64,
            25i64,
            7i64,
            true,
            "2026-06-13T11:56:20.000Z",
        ),
        (
            "01FILLROTA000000000000003",
            "KXNYCHIGH-26JUN13-B65",
            "yes",
            "buy",
            43i64,
            10i64,
            3i64,
            true,
            "2026-06-13T11:57:40.000Z",
        ),
    ] {
        let r = sqlx::query(
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
        .execute(pool)
        .await;
        if let Err(e) = r {
            eprintln!("[rota_local] seed 'fill' skipped: {e}");
        }
    }

    // Discovery context: promote the NYC event to active, mark the Boston event
    // dead (its beliefs were abandoned), and map the NYC event to two markets so
    // the Discovery board shows a non-zero market count.
    warn_seed(
        "event.activate",
        events.set_status(event_id, "active").await,
    );
    warn_seed(
        "event.dead-mark",
        events.mark_dead(dead_event, "source_lost").await,
    );
    for (edge_id, market) in [
        ("01J0EDGE00000000000000NYC1", "KXNYCHIGH-26JUN13-B65"),
        ("01J0EDGE00000000000000NYC2", "KXNYCHIGH-26JUN13-B70"),
    ] {
        let r = sqlx::query(
            "INSERT INTO market_event_edges (edge_id, market_id, venue, event_id, mapping_type, \
             confidence, proposed_by, created_at) \
             VALUES ($1,$2,'sim',$3,'direct',0.92,'discovery','2026-06-12T18:10:00.000Z')",
        )
        .bind(edge_id)
        .bind(market)
        .bind(event_id)
        .execute(pool)
        .await;
        if let Err(e) = r {
            eprintln!("[rota_local] seed 'edge' skipped: {e}");
        }
    }

    // Five audit rows of distinct kinds via the real append path (ULID + clock
    // supplied), advancing a SimClock between them so ids/timestamps order.
    let start = UtcTimestamp::parse_iso8601("2026-06-13T11:58:00.000Z")?;
    let sim = Arc::new(SimClock::new(start));
    let audit = AuditWriter::new(pool.clone(), sim.clone(), 42);
    let rows: [(&str, Option<&str>, Option<&str>, Value); 5] = [
        (
            "gate_decision",
            Some("gate"),
            Some(open_id),
            json!({"check": "net_edge", "result": "pass", "edge_bps": 64}),
        ),
        (
            "gate_decision",
            Some("gate"),
            None,
            json!({"check": "rate_bucket", "result": "reject", "reason": "per_market_bucket_empty"}),
        ),
        (
            "cognition",
            Some("mind"),
            Some(persona_id),
            json!({"event": "belief_drafted", "persona_id": "meteorologist", "cost_cents": 2}),
        ),
        (
            "settlement",
            Some("watchdog"),
            None,
            json!({"event": "settlement_confirmed", "market": "KXNYCHIGH-26JUN13-B65", "cents": 12500}),
        ),
        (
            "rearm",
            Some("operator"),
            None,
            json!({"event": "halt_cleared_in_ledger", "note": "running daemon resumes only on restart"}),
        ),
    ];
    for (kind, actor, ref_id, payload) in rows {
        warn_seed(
            "audit",
            audit.append(kind, actor, ref_id, payload).await.map(|_| ()),
        );
        // Distinct ms per row so the tail orders cleanly.
        sim.advance_millis(1_000)?;
    }
    Ok(())
}

/// The daemon-shaped boards that need no DB — mirrors the JSON shape of
/// `fortuna-live/src/views.rs::views_from` so the rendered boards match the
/// live daemon. Representative, non-zero, render-exercising values.
fn representative_views(generated_at: &str) -> Value {
    json!({
        "health": {
            "generated_at": generated_at,
            "stage": "sim",
            "halt_active": false,
            "halt_reason": Value::Null,
            "rearm_requires_restart": false,
            "ticks_total": 14_820,
            "last_tick_age_ms": Value::Null,
            "fill_latency_p90_ms": 42,
            "fill_latency_p95_ms": 58,
            "fill_latency_p99_ms": 91,
            "dead_man_last_ping_age_secs": Value::Null,
            "venues": [ { "id": "sim", "healthy": true, "api_error_count": 0 } ],
        },
        "money": {
            "generated_at": generated_at,
            "basis": "sim-only",
            "settled_cents": 480_000,
            "committed_cents": 52_000,
            "floating_cents": Value::Null,
            "total_cents": Value::Null,
            "positions": [
                { "market": "KXNYCHIGH-26JUN13-B65", "yes_qty": 40, "no_qty": 0,
                  "realized_pnl_cents": 0, "fees_cents": 120, "lifecycle": "open" },
                { "market": "KXCHIHIGH-26JUN13-B72", "yes_qty": 0, "no_qty": 25,
                  "realized_pnl_cents": 3_100, "fees_cents": 75, "lifecycle": "settling" },
            ],
        },
        "gates": {
            "generated_at": generated_at,
            "total_rejections": 37,
            // Real GateCheck Debug names + index() positions (pipeline.rs:51-104),
            // exactly as views_from derives them — never invented strings.
            "rejections_by_check": [
                { "check": "EdgeFloor", "count": 21, "number": 6 },
                { "check": "RateLimits", "count": 9, "number": 7 },
                { "check": "Halts", "count": 7, "number": 1 },
            ],
        },
        "cognition": {
            "generated_at": generated_at,
            "mind_spend_today_cents": 1_240,
            "daily_budget_cents": 5_000,
            "cognition_failures_total": 0,
            "budget_breaches_total": 0,
        },
        "settlement": {
            "generated_at": generated_at,
            "capital_in_limbo_cents": 125_000,
            "settlements_overdue": 0,
            // The renderer reads discrepancies_open (rota.rs R.settlement);
            // views_from omits it today (track-A gap, ledgered in GAPS) — seeded
            // here to exercise the line with an honest representative value.
            "discrepancies_open": 0,
            "settlement_voids_total": 0,
            "settlement_reversals_total": 0,
        },
        "streams": {
            "generated_at": generated_at,
            "venue_api_errors_total": 0,
            // book_age_ms mirrors the live daemon's documented null (views.rs:171,
            // deferred field) — never a fabricated age.
            "venues": [ { "id": "sim", "book_age_ms": Value::Null, "ws_gap_count": 0, "resync_count": 0 } ],
        },
        // D-contract V2 Sources Health board envelope ({title,columns,rows,
        // summary}). In prod the daemon's OBS-2 publish shapes this from the live
        // IngestionTelemetry (fortuna-sources SourceTelemetry); here it is the
        // representative seed. last_ok_age_s + empty_rate_pct are the daemon-side
        // derivations (now - last_success_at; empty_polls*100/polls). The nws_afd
        // row is the AFD-firehose (huge dropped_over_volume) the board surfaces.
        "ingest_sources": {
            "title": "Sources Health",
            "generated_at": generated_at,
            "columns": [
                { "key": "source_id", "label": "Source" },
                { "key": "health", "label": "Health", "pill": true },
                { "key": "last_ok_age_s", "label": "Last OK" },
                { "key": "polls", "label": "Polls" },
                { "key": "accepted", "label": "Acc" },
                { "key": "dropped_future", "label": "D:fut" },
                { "key": "dropped_republished", "label": "D:rep" },
                { "key": "dropped_over_volume", "label": "D:vol" },
                { "key": "empty_rate_pct", "label": "304%" },
                { "key": "quarantines", "label": "Quar" },
                { "key": "next_due_at", "label": "Next due" },
            ],
            "rows": [
                { "source_id": "nws_alerts", "health": "healthy", "last_ok_age_s": 12,
                  "polls": 420, "accepted": 58, "dropped_future": 3, "dropped_republished": 11,
                  "dropped_over_volume": 0, "empty_rate_pct": 86, "quarantines": 0,
                  "next_due_at": "2026-06-13T12:35:30Z" },
                { "source_id": "nws_afd", "health": "degraded", "last_ok_age_s": 340,
                  "polls": 140, "accepted": 12, "dropped_future": 0, "dropped_republished": 2,
                  "dropped_over_volume": 171, "empty_rate_pct": 52, "quarantines": 1,
                  "next_due_at": "2026-06-13T12:36:00Z" },
                { "source_id": "aeolus_forecast", "health": "healthy", "last_ok_age_s": 48,
                  "polls": 96, "accepted": 96, "dropped_future": 0, "dropped_republished": 0,
                  "dropped_over_volume": 0, "empty_rate_pct": 0, "quarantines": 0,
                  "next_due_at": "2026-06-13T12:40:00Z" },
            ],
            "summary": { "healthy": 2, "degraded": 1, "quarantined": 0, "accepted": 166, "dropped": 188 },
        },
        // D-contract V1 Live Signal Feed — the marquee view: recent signals
        // newest-first with their actual (redacted) payload summary + accept/drop
        // status. In prod the daemon shapes this from IngestionTelemetry.recent
        // (SignalRecord ring). The `summary` cells are UNTRUSTED ingestion data
        // (quoted, esc()'d in the renderer, never interpreted — spec 5.11).
        "ingest_feed": {
            "title": "Live Signal Feed",
            "generated_at": generated_at,
            "columns": [
                { "key": "at", "label": "Time (UTC)" },
                { "key": "source_id", "label": "Source" },
                { "key": "kind", "label": "Kind" },
                { "key": "claimed_time", "label": "Claimed" },
                { "key": "status", "label": "Status", "pill": true },
                { "key": "summary", "label": "Data" },
            ],
            "rows": [
                { "at": "2026-06-13T12:34:58Z", "source_id": "nws_alerts", "kind": "nws.alert",
                  "claimed_time": "2026-06-13T12:34:40Z", "status": "accepted",
                  "summary": "Severe Thunderstorm Warning — Kings County NY until 13:30 EDT" },
                { "at": "2026-06-13T12:34:55Z", "source_id": "nws_afd", "kind": "nws.afd",
                  "claimed_time": "2026-06-13T12:30:00Z", "status": "dropped:over_volume",
                  "summary": "Area Forecast Discussion (NYC) — 14KB, over per-poll volume cap" },
                { "at": "2026-06-13T12:34:31Z", "source_id": "aeolus_forecast", "kind": "aeolus.forecast",
                  "claimed_time": "2026-06-13T12:00:00Z", "status": "accepted",
                  "summary": "KNYC tmax μ=67.2 σ=3.1 (run 2026-06-13T06Z)" },
                { "at": "2026-06-13T12:33:12Z", "source_id": "nws_alerts", "kind": "nws.alert",
                  "claimed_time": "2026-06-13T11:50:00Z", "status": "dropped:republished",
                  "summary": "Flood Watch (re-issue, identity match) — Bronx" },
                { "at": "2026-06-13T12:32:40Z", "source_id": "nws_afd", "kind": "nws.afd",
                  "claimed_time": Value::Null, "status": "dropped:future",
                  "summary": "AFD with claimed_time ahead of now — rejected" },
            ],
            "summary": { "window": 5, "accepted": 2, "dropped": 3 },
        },
        // D-contract V3 Ingest Funnel — the process at a glance: the pipeline
        // stages with retention % + drop-offs (where signal is lost). In prod the
        // daemon shapes this from IngestionTelemetry.funnel (FunnelCounts);
        // CONTRACT: loop-side stages (normalized/persisted) are emitted null until
        // the ingestion loop feeds them, never a fabricated 0. Here the seed shows
        // the fully-wired funnel.
        "ingest_funnel": {
            "title": "Ingest Funnel",
            "generated_at": generated_at,
            "columns": [
                { "key": "stage", "label": "Stage" },
                { "key": "count", "label": "Count" },
                { "key": "retain_pct", "label": "Retain %" },
                { "key": "dropped", "label": "Dropped" },
                { "key": "detail", "label": "Detail" },
            ],
            "rows": [
                { "stage": "Fetched", "count": 1240, "retain_pct": 100, "dropped": 0,
                  "detail": "raw items returned by the adapters" },
                { "stage": "Validated", "count": 1052, "retain_pct": 85, "dropped": 188,
                  "detail": "refused by Layer-1 (future / republished / over_volume)" },
                { "stage": "Normalized", "count": 1052, "retain_pct": 85, "dropped": 0,
                  "detail": "became SignalEnvelopes" },
                { "stage": "Persisted", "count": 1048, "retain_pct": 85, "dropped": 4,
                  "detail": "deduped 4 · persist_failures 0" },
            ],
            "summary": { "fetched": 1240, "persisted": 1048, "retain_pct": 85, "persist_failures": 0 },
        },
        // Strategy P&L (mission item 3) — per-strategy realized PnL / fees / fills
        // / open exposure. In prod the daemon shapes this from
        // runner.digest_snapshot(); here it is the representative seed (a winning
        // and a losing strategy, shown honestly). Money columns render as dollars.
        "strategies": {
            "title": "Strategy P&L",
            "generated_at": generated_at,
            "columns": [
                { "key": "strategy", "label": "Strategy" },
                { "key": "realized_pnl_cents", "label": "Realized", "cents": true },
                { "key": "fees_cents", "label": "Fees", "cents": true },
                { "key": "fills", "label": "Fills" },
                { "key": "open_exposure_cents", "label": "Open exp", "cents": true },
            ],
            "rows": [
                { "strategy": "mech_structural", "realized_pnl_cents": 3100, "fees_cents": 211,
                  "fills": 3, "open_exposure_cents": 0 },
                { "strategy": "perp_basis", "realized_pnl_cents": -450, "fees_cents": 38,
                  "fills": 1, "open_exposure_cents": 12000 },
            ],
            "summary": { "strategies": 2, "fills": 4 },
        },
        // Working orders (mission item 3, live side): the shape views_from produces
        // from runner.manager().intents() — two arb legs resting at the venue (one
        // acked, one partially filled) so the board renders the live order book.
        "working_orders": {
            "title": "Working Orders",
            "generated_at": generated_at,
            "columns": [
                { "key": "market", "label": "Market" },
                { "key": "side", "label": "Side" },
                { "key": "action", "label": "Action" },
                { "key": "limit_cents", "label": "Limit", "cents": true },
                { "key": "qty", "label": "Qty" },
                { "key": "filled", "label": "Filled" },
                { "key": "status", "label": "Status", "pill": true },
                { "key": "created_at", "label": "Submitted (UTC)" },
            ],
            "rows": [
                { "market": "KXNYCHIGH-26JUN13-B65", "side": "yes", "action": "buy",
                  "limit_cents": 41, "qty": 50, "filled": 0, "status": "acked",
                  "created_at": "2026-06-13T15:58:00.000Z" },
                { "market": "KXNYCHIGH-26JUN13-B70", "side": "no", "action": "buy",
                  "limit_cents": 58, "qty": 40, "filled": 12, "status": "partially_filled",
                  "created_at": "2026-06-13T15:57:30.000Z" },
            ],
            "summary": { "working": 2 },
        },
        // Telemetry (mission item 6): exercise the REAL MetricsRegistry::telemetry_board
        // shaping with a representative cross-subsystem registry, so the screenshot
        // matches the daemon path exactly (not a hand-written envelope).
        "telemetry": representative_telemetry(generated_at),
    })
}

/// A representative `MetricsRegistry` across several subsystems, shaped through the
/// SAME `telemetry_board` the daemon calls — so the local Telemetry board renders the
/// real exposition shape with real (non-zero) integer series.
fn representative_telemetry(generated_at: &str) -> Value {
    let mut m = MetricsRegistry::new();
    m.describe_gauge(
        "fortuna_exec_working_orders",
        "intents resting at the venue",
    );
    m.set_gauge("fortuna_exec_working_orders", &[], 2);
    m.describe_counter("fortuna_gate_rejections_total", "orders refused by a gate");
    m.inc_counter(
        "fortuna_gate_rejections_total",
        &[("check", "edge_floor")],
        7,
    )
    .expect("positive representative counter");
    m.inc_counter(
        "fortuna_gate_rejections_total",
        &[("check", "rate_limit")],
        1,
    )
    .expect("positive representative counter");
    m.describe_counter(
        "fortuna_ingest_accepted_total",
        "signals accepted by ingestion",
    );
    m.inc_counter(
        "fortuna_ingest_accepted_total",
        &[("source", "nws_alerts")],
        58,
    )
    .expect("positive representative counter");
    m.describe_gauge("fortuna_state_capital_in_limbo_cents", "unsettled capital");
    m.set_gauge("fortuna_state_capital_in_limbo_cents", &[], 4200);
    m.describe_gauge("fortuna_killswitch_armed", "kill-switch armed (1=armed)");
    m.set_gauge("fortuna_killswitch_armed", &[], 1);
    m.telemetry_board(generated_at)
}

/// Create a temp perishable dir with a today-dated `.jsonl` stream file so the
/// Streams recorder section renders a live row. Returns the BASE dir (which
/// `scan_recorder` joins with `generated_at[0..10]`).
fn temp_perishable(generated_at: &str) -> Result<std::path::PathBuf, BoxErr> {
    let day = generated_at.get(0..10).unwrap_or("1970-01-01");
    let base = std::env::temp_dir().join("fortuna-rota-local-perishable");
    let day_dir = base.join(day);
    std::fs::create_dir_all(&day_dir)?;
    std::fs::write(
        day_dir.join("orders.jsonl"),
        b"{\"seed\":\"rota_local recorder-liveness sample\"}\n",
    )?;
    std::fs::write(
        day_dir.join("bracket_quotes.jsonl"),
        b"{\"seed\":\"rota_local recorder-liveness sample\"}\n",
    )?;
    Ok(base)
}
