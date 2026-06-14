//! T4.1 hard requirement 6: belief drafts produced by a strategy drain
//! through the runner and PERSIST to BeliefsRepo (events upserted first
//! for the FK). Driven by a minimal in-test belief-producing strategy
//! (the daemon runs mech_structural, which holds no beliefs; the
//! persistence PATH is what req 6 demands and what this pins). Written
//! red-first against persist_beliefs + the runner belief drain.

use fortuna_cognition::beliefs::BeliefDraft;
use fortuna_core::bus::BusEvent;
use fortuna_core::clock::UtcTimestamp;
use fortuna_core::market::{StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_live::daemon::persist_beliefs;
use fortuna_runner::{
    CoreHandle, MemoryAuditSink, RunnerConfig, SimRunner, Stage, Strategy, StrategyKind,
    StrategyMetrics,
};
use fortuna_state::MarkPolicy;
use fortuna_venues::fees::{FeeSchedule, ScheduleFeeModel};
use fortuna_venues::sim::FaultConfig;
use sqlx::PgPool;
use std::collections::BTreeMap;

fn t0() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-11T12:00:00.000Z").unwrap()
}

fn fee_model() -> ScheduleFeeModel {
    let s: FeeSchedule = toml::from_str(
        "formula = \"quadratic\"\neffective_date = \"2026-01-01\"\ntaker_coeff = \"0.07\"\nmaker_coeff = \"0.0175\"\n",
    )
    .unwrap();
    ScheduleFeeModel::new(vec![s]).unwrap()
}

fn runner_config() -> RunnerConfig {
    RunnerConfig {
        seed: 1,
        gate_config: toml::from_str(
            "[global]\nmax_total_exposure_cents=800000\nmax_daily_loss_cents=50000\nmin_order_contracts=1\nmax_order_contracts=1000\nprice_band_cents=45\nmax_cross_cents=10\nper_market_exposure_cents=100000\nper_event_exposure_cents=150000\nrequire_event_mapping=false\n[rate.sim]\nburst=100\nsustained_per_min=600\nmarket_burst=50\nmarket_sustained_per_min=300\n",
        )
        .unwrap(),
        exec_policy: fortuna_exec::ExecPolicy::default(),
        envelopes: BTreeMap::new(),
        max_daily_loss: Cents::new(50_000),
        fee_model: fee_model(),
        markets: vec![],
        starting_cash: Cents::new(1_000_000),
        faults: Some(FaultConfig::none(1)),
        mark_policy: MarkPolicy {
            max_book_age_ms: 60_000,
            max_spread_cents: 20,
        },
        max_sets_per_proposal: 50,
        kelly_fraction: 0.25,
        veto_mind: None,
        veto_strategies: Vec::new(),
    }
}

/// Emits two belief drafts on its first tick, then none — the minimal
/// req-6 producer (no proposals; beliefs are the product).
struct BeliefProducer {
    id: StrategyId,
    emitted: bool,
}

#[async_trait::async_trait]
impl Strategy for BeliefProducer {
    fn id(&self) -> StrategyId {
        self.id.clone()
    }
    fn kind(&self) -> StrategyKind {
        StrategyKind::Synthesis
    }
    fn stage(&self) -> Stage {
        Stage::Sim
    }
    async fn on_event(
        &mut self,
        _ev: &BusEvent,
        _core: &CoreHandle<'_>,
    ) -> Result<Vec<fortuna_runner::Proposal>, fortuna_runner::RunnerError> {
        Ok(Vec::new())
    }
    fn metrics(&self) -> StrategyMetrics {
        StrategyMetrics::default()
    }
    fn drain_beliefs(&mut self) -> Vec<BeliefDraft> {
        if self.emitted {
            return Vec::new();
        }
        self.emitted = true;
        vec![
            BeliefDraft {
                event_id: "wx:KNYC:2026-06-12:t65".into(),
                p: 0.55,
                p_raw: 0.61,
                horizon: t0(),
                evidence: serde_json::json!([{"source": "test"}]),
                provenance: serde_json::json!({"model_id": "test"}),
            },
            BeliefDraft {
                event_id: "wx:KNYC:2026-06-12:t70".into(),
                p: 0.27,
                p_raw: 0.30,
                horizon: t0(),
                evidence: serde_json::json!([{"source": "test"}]),
                provenance: serde_json::json!({"model_id": "test"}),
            },
        ]
    }
}

#[sqlx::test(migrations = "../fortuna-ledger/migrations")]
async fn drafted_beliefs_drain_through_the_runner_and_persist(pool: PgPool) {
    let _ = VenueId::new("sim").unwrap();
    let mut r = futures::executor::block_on(SimRunner::new_with_journal(
        runner_config(),
        vec![Box::new(BeliefProducer {
            id: StrategyId::new("synth_events").unwrap(),
            emitted: false,
        })],
        Box::new(MemoryAuditSink::default()),
        t0(),
        fortuna_exec::MemoryJournal::default(),
    ))
    .unwrap();

    r.tick().await.unwrap();
    let drained = r.drain_pending_beliefs();
    assert_eq!(drained.len(), 2, "two drafts drained from the strategy");

    // Persist; events upserted, beliefs inserted.
    let n = persist_beliefs(&pool, &drained, "2026-06-11T12:00:00.000Z", 1)
        .await
        .unwrap();
    assert_eq!(n, 2);
    let beliefs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM beliefs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(beliefs, 2, "both belief rows persisted");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(events, 2, "each belief's event upserted for the FK");

    // Draining again yields nothing (drained once, persisted once).
    r.tick().await.unwrap();
    assert!(r.drain_pending_beliefs().is_empty(), "beliefs drain once");

    // A SECOND persist call on the same events (new drafts, disjoint id
    // base) upserts the events idempotently — no duplicate-event error.
    let more = persist_beliefs(&pool, &drained, "2026-06-11T12:05:00.000Z", 100)
        .await
        .unwrap();
    assert_eq!(more, 2, "event upsert is idempotent across drains");
    let events_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        events_after, 2,
        "no duplicate events from the second persist"
    );
}
