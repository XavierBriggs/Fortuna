//! LIVE one-shot persona smoke — operator-run, makes a REAL Anthropic call and a
//! REAL network fetch. This drives the EXACT daemon entry point
//! [`run_due_personas`] with a REAL [`ReqwestMindTransport`] over TODAY'S live
//! Aeolus envelope (and, best-effort, the live NWS Area Forecast Discussion), to
//! produce a genuine meteorologist finding. It is the same code path the daemon
//! runs every cognition tick — the only difference is a single manual tick
//! instead of the daemon loop, and no Postgres persistence (the run returns the
//! draft the daemon would persist).
//!
//! Why this exists: the scripted-`StubMind` persona tests assume the model routes
//! its findings into `journal.body`; NOTHING verifies a real model actually does
//! so against the shipped charter. This smoke closes that gap on live data.
//!
//! GATED — skipped unless `FORTUNA_LIVE_PERSONA_SMOKE=1` AND both
//! `ANTHROPIC_API_KEY` and `AEOLUS_API_TOKEN` are present (env-only secrets,
//! never printed). A normal `cargo test` / CI run makes no network call and
//! spends nothing.
//!
//! Run:
//! ```text
//! set -a; . ./.env; set +a
//! FORTUNA_LIVE_PERSONA_SMOKE=1 cargo test -p fortuna-cognition \
//!     --test persona_live_smoke -- --nocapture
//! ```

use fortuna_cognition::discovery::DiscoveryBudget;
use fortuna_cognition::mind::{
    AnthropicMind, AnthropicMindConfig, CostBudget, Mind, ReqwestMindTransport,
};
use fortuna_cognition::persona::PersonaDef;
use fortuna_cognition::persona_orchestrator::{
    run_due_personas, PersonaSchedule, PersonaScheduleState,
};
use fortuna_cognition::persona_runner::persona_system_charter;
use fortuna_cognition::signals::{content_hash, SignalEnvelope};
use fortuna_core::clock::{Clock, UtcTimestamp};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

// Mirrors daemon.rs SYNTH_MIND_* — personas are wired to ModelTier::Synthesis
// (main.rs:496), i.e. the same config + prices as the deep belief-formation tier.
const MAX_TOKENS: i64 = 16_000;
const IN_PRICE_CENTS_PER_MTOK: i64 = 1_000;
const OUT_PRICE_CENTS_PER_MTOK: i64 = 5_000;
// The daemon uses 30s; a one-shot smoke can wait longer for Opus adaptive
// thinking. Overridable via FORTUNA_PERSONA_TIMEOUT_SECS. (That 30s prod ceiling
// vs. a slow Opus persona call is itself worth a look — noted in loop-close-gaps.)
const DEFAULT_TIMEOUT_SECS: u64 = 180;

// The committed [sources.aeolus_knyc] live feed (read-only; auth via x-api-key).
const AEOLUS_URL: &str = "https://aaa-bloom-acquire-lay.trycloudflare.com/v2/forecasts?station=KNYC&variable=tmax&from=2026-06-19&to=2026-06-24";

/// A pinned clock for the run (the daemon injects `RealClock`; we pin so the
/// signal receipt time and the mind's `now` agree deterministically).
struct FixedClock(UtcTimestamp);
impl Clock for FixedClock {
    fn now(&self) -> UtcTimestamp {
        self.0
    }
}

fn load_meteorologist() -> PersonaDef {
    // CARGO_MANIFEST_DIR is crates/fortuna-cognition; personas live at repo root.
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/personas/meteorologist");
    let md = std::fs::read_to_string(dir.join("persona.md")).expect("persona.md readable");
    let schema = std::fs::read_to_string(dir.join("schema.json")).expect("schema.json readable");
    PersonaDef::parse(&md, &schema).expect("shipped meteorologist parses")
}

/// Today's LIVE Aeolus v2 envelope — the persona's statistical backbone. Returns
/// the furthest-out forecast in the window (the latest, still-unresolved
/// target_date), used verbatim as the `aeolus.forecast` signal payload exactly
/// as the venue would emit it.
async fn fetch_live_aeolus(token: &str) -> Value {
    let client = reqwest::Client::new();
    let resp = client
        .get(AEOLUS_URL)
        .header("x-api-key", token)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .expect("aeolus feed reachable");
    assert!(
        resp.status().is_success(),
        "aeolus feed HTTP {}",
        resp.status()
    );
    let body: Value = resp.json().await.expect("aeolus feed returns JSON");
    let forecasts = body["forecasts"].as_array().cloned().unwrap_or_default();
    forecasts
        .into_iter()
        .max_by(|a, b| a["target_date"].as_str().cmp(&b["target_date"].as_str()))
        .expect("live feed returned at least one forecast")
}

/// Best-effort live NWS Area Forecast Discussion for the NYC office (OKX) — the
/// persona's human-reasoning input. Returns `None` on any failure (the persona
/// degrades to the envelope alone). Trimmed to keep the context bounded.
async fn fetch_live_afd() -> Option<String> {
    let ua = "fortuna-live-smoke (xbriggs03@gmail.com)";
    let client = reqwest::Client::new();
    let list: Value = client
        .get("https://api.weather.gov/products/types/AFD/locations/OKX")
        .header("User-Agent", ua)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let id = list["@graph"][0]["@id"].as_str()?;
    let product: Value = client
        .get(id)
        .header("User-Agent", ua)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    product["productText"]
        .as_str()
        .map(|s| s.chars().take(3000).collect())
}

#[tokio::test]
async fn live_meteorologist_finding_on_todays_real_nyc_forecast() {
    // --- gates: opt-in flag + both secrets present (never printed) ---
    if std::env::var("FORTUNA_LIVE_PERSONA_SMOKE").is_err() {
        eprintln!("SKIP: set FORTUNA_LIVE_PERSONA_SMOKE=1 to run (makes a real Anthropic call).");
        return;
    }
    let aeolus_token = match std::env::var("AEOLUS_API_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            eprintln!("SKIP: AEOLUS_API_TOKEN not set.");
            return;
        }
    };
    if std::env::var("ANTHROPIC_API_KEY")
        .map(|k| k.trim().is_empty())
        .unwrap_or(true)
    {
        eprintln!("SKIP: ANTHROPIC_API_KEY not set.");
        return;
    }

    // --- 1. the real persona (loaded from the shipped charter file) ---
    let def = load_meteorologist();
    let model =
        std::env::var("FORTUNA_PERSONA_MODEL").unwrap_or_else(|_| "claude-opus-4-8".to_string());

    // --- 2. today's LIVE Aeolus envelope ---
    let envelope = fetch_live_aeolus(&aeolus_token).await;
    let target_date = envelope["target_date"].as_str().unwrap_or("?").to_string();
    let nws_station = envelope["nws_station_id"]
        .as_str()
        .unwrap_or("?")
        .to_string();
    let mu = envelope["distribution"]["mu"].as_f64().unwrap_or(0.0);
    let sigma = envelope["distribution"]["sigma"].as_f64().unwrap_or(0.0);
    eprintln!(
        "LIVE Aeolus: {nws_station} tmax {target_date}  mu={mu:.2} sigma={sigma:.2}  model={model}"
    );

    // --- 3. build the signals exactly as the daemon would (real content_hash).
    //        region_key = weather:{nws_station_id}:tmax:{target_date} fills from
    //        these top-level payload scalars; both signals key to one group. ---
    let now = UtcTimestamp::parse_iso8601("2026-06-20T18:00:00.000Z").unwrap();
    // Signals are received STRICTLY BEFORE the tick fires — assemble_context's
    // point-in-time guard (context.rs:159) excludes any item with at >= now.
    let received_at = UtcTimestamp::parse_iso8601("2026-06-20T17:00:00.000Z").unwrap();
    let mut signals = vec![SignalEnvelope {
        signal_id: format!("aeolus-{nws_station}-{target_date}"),
        source: "aeolus".to_string(),
        kind: "aeolus.forecast".to_string(),
        received_at,
        content_hash: content_hash("aeolus", "aeolus.forecast", &envelope),
        payload: envelope,
    }];
    if let Some(afd) = fetch_live_afd().await {
        let payload = json!({
            "nws_station_id": nws_station,
            "target_date": target_date,
            "office": "OKX",
            "kind": "area_forecast_discussion",
            "text": afd,
        });
        signals.push(SignalEnvelope {
            signal_id: format!("nws-afd-{target_date}"),
            source: "nws".to_string(),
            kind: "nws.forecast_discussion".to_string(),
            received_at,
            content_hash: content_hash("nws", "nws.forecast_discussion", &payload),
            payload,
        });
        eprintln!("LIVE NWS AFD: attached ({} signals)", signals.len());
    } else {
        eprintln!("NWS AFD unavailable; running on the Aeolus envelope alone.");
    }

    // --- 4. the REAL mind: Anthropic over reqwest, persona.method as the system
    //        charter (the §4 firewall), synthesis tier — exactly as
    //        persona_mind_from_env builds it in the daemon. ---
    let timeout_secs: u64 = std::env::var("FORTUNA_PERSONA_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    let transport = ReqwestMindTransport::from_env(Duration::from_secs(timeout_secs))
        .expect("ANTHROPIC_API_KEY present");
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(now));
    let mind: Arc<dyn Mind> = Arc::new(AnthropicMind::new(
        AnthropicMindConfig {
            model: model.clone(),
            max_tokens: MAX_TOKENS,
            input_price_cents_per_mtok: IN_PRICE_CENTS_PER_MTOK,
            output_price_cents_per_mtok: OUT_PRICE_CENTS_PER_MTOK,
            system_charter: persona_system_charter(&def).to_string(),
        },
        transport,
        CostBudget::new(3_000, 3_000),
        clock,
    ));
    let mut minds: BTreeMap<String, Arc<dyn Mind>> = BTreeMap::new();
    minds.insert(def.meta.id.clone(), mind);

    // --- 5. drive the REAL orchestrator — one manual tick (fresh signal fires) ---
    let schedules = vec![PersonaSchedule {
        def,
        cadences: vec![],
    }];
    let mut state = PersonaScheduleState::new(0);
    let mut budget = DiscoveryBudget::new(3_000);
    eprintln!("calling {model} on the live data (real Anthropic spend)...");
    let results =
        run_due_personas(now, &schedules, &signals, &mut state, &minds, &mut budget).await;

    // --- 6. show the genuine finding ---
    assert_eq!(results.len(), 1, "exactly one (persona, region) run is due");
    let r = &results[0];
    eprintln!("\n=== region: {} ===", r.region_key);
    eprintln!("cost_cents: {}", r.outcome.cost_cents);
    if !r.outcome.defects.is_empty() {
        eprintln!("DEFECTS:\n{:#?}", r.outcome.defects);
    }
    match &r.outcome.findings {
        Some(f) => {
            eprintln!(
                "content_hash: {}",
                r.outcome.content_hash.as_deref().unwrap_or("?")
            );
            eprintln!("\n=== METEOROLOGIST FINDING (real model, findings/v2) ===");
            eprintln!(
                "{}",
                serde_json::to_string_pretty(f).unwrap_or_else(|_| f.to_string())
            );
        }
        None => eprintln!("NO finding produced (throttled/skipped/degraded — see DEFECTS above)."),
    }
    assert!(
        r.outcome.produced_artifact(),
        "the live persona run must produce a genuine findings artifact"
    );
}
