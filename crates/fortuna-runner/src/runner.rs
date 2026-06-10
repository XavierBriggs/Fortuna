//! The composed Sim loop (spec Section 4 data flow; Phase 0 exit vehicle).
//!
//! Single-threaded and deterministic: every input lands on the bus first
//! (the byte-exact replay record), strategies run in registration order over
//! newly recorded events, proposals are sized from envelope headroom,
//! pushed through the universal gates, submitted via the order manager, and
//! every artifact (gate verdicts, submissions, fills, halts, group
//! decisions) is both audited and republished onto the bus.
//!
//! Fail-closed wiring proven here: audit-sink failure => global halt (I5);
//! drawdown breach => global halt (I2); both clear only via operator re-arm.

use crate::{AuditSink, CoreHandle, Proposal, RunnerError, Stage, Strategy};
use fortuna_cognition::veto::{
    counterfactual_pnl, FillAssumption, VetoCandidate, VetoMind, VetoVerdict,
};
use fortuna_core::book::FeeModel;
use fortuna_core::bus::{EventBus, EventPayload, Recording};
use fortuna_core::clock::{Clock, SimClock, UtcTimestamp};
use fortuna_core::ids::{IdGen, IntentGroupId, IntentId};
use fortuna_core::market::{ClientOrderId, Contracts, MarketId, StrategyId, VenueId};
use fortuna_core::money::Cents;
use fortuna_exec::{
    decide_complete_or_unwind, CompleteOrUnwind, ExecError, ExecPolicy, GroupDecision,
    GroupTracker, IntentStatus, MemoryJournal, OrderManager, RemainingLeg, SubmitOutcome,
};
use fortuna_gates::{CandidateOrder, GateConfig, GateInputs, GatePipeline, HaltScope};
use fortuna_state::{
    affordable_sets, mark_lots, DrawdownMonitor, DrawdownVerdict, MarkPolicy, PositionBook,
    ReservationLedger,
};
use fortuna_venues::fees::ScheduleFeeModel;
use fortuna_venues::sim::{FaultConfig, SimVenue};
use fortuna_venues::{Cursor, Market, Venue};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct RunnerConfig {
    pub seed: u64,
    pub gate_config: GateConfig,
    pub exec_policy: ExecPolicy,
    pub envelopes: BTreeMap<String, Cents>,
    pub max_daily_loss: Cents,
    pub fee_model: ScheduleFeeModel,
    pub markets: Vec<Market>,
    pub starting_cash: Cents,
    pub faults: FaultConfig,
    pub mark_policy: MarkPolicy,
    /// Max sets per arb proposal (belt to the gates' braces).
    pub max_sets_per_proposal: i64,
    /// The reduce-only model veto (spec Section 6). Strategies listed in
    /// `veto_strategies` have every sized candidate assessed BEFORE the
    /// gates; listing a strategy without providing a mind is a
    /// construction error (the spec ships mech_extremes WITH its veto,
    /// silently skipping it would be a lie).
    pub veto_mind: Option<Arc<dyn VetoMind>>,
    pub veto_strategies: Vec<StrategyId>,
}

#[derive(Debug, Default)]
pub struct TickReport {
    pub events_dispatched: usize,
    pub proposals: usize,
    pub orders_submitted: usize,
    pub gate_rejections: usize,
    pub fills_applied: usize,
    pub group_decisions: usize,
    pub halted: bool,
}

#[derive(Debug)]
pub struct RunnerReport {
    pub recording_jsonl: String,
    pub final_cash: Cents,
    pub realized_pnl: Cents,
    pub fees_paid: Cents,
}

/// The Phase 0 composition over the sim venue.
pub struct SimRunner {
    pub clock: Arc<SimClock>,
    bus: EventBus,
    venue: SimVenue,
    gates: GatePipeline,
    manager: OrderManager<MemoryJournal>,
    positions: PositionBook,
    reservations: ReservationLedger,
    drawdown: DrawdownMonitor,
    strategies: Vec<Box<dyn Strategy>>,
    audit: Box<dyn AuditSink>,
    groups: GroupTracker,
    fee_model: ScheduleFeeModel,
    mark_policy: MarkPolicy,
    books: BTreeMap<MarketId, fortuna_core::book::OrderBook>,
    markets: Vec<MarketId>,
    envelope_names: Vec<String>,
    ids: IdGen,
    cursor: Cursor,
    seen_events: usize,
    max_sets_per_proposal: i64,
    /// Set when the audit sink dies: hard stop beyond the gate halt.
    audit_dead: bool,
    veto_mind: Option<Arc<dyn VetoMind>>,
    veto_strategies: std::collections::BTreeSet<StrategyId>,
    /// Vetoed-away quantity awaiting its market's settlement for
    /// counterfactual scoring (spec Section 6: veto value-add is measured,
    /// not believed). Provider-error suppressions are NOT tracked here.
    open_vetoes: Vec<OpenVeto>,
    market_meta: BTreeMap<MarketId, Market>,
}

struct OpenVeto {
    candidate: VetoCandidate,
    removed: Contracts,
}

impl SimRunner {
    pub fn new(
        config: RunnerConfig,
        strategies: Vec<Box<dyn Strategy>>,
        audit: Box<dyn AuditSink>,
        start: UtcTimestamp,
    ) -> Result<SimRunner, RunnerError> {
        // I7 sliver: a Sim runner accepts only Sim-staged strategies.
        for s in &strategies {
            if s.stage() != Stage::Sim {
                return Err(RunnerError::StageViolation {
                    strategy: s.id(),
                    stage: s.stage(),
                });
            }
        }
        // A veto-enrolled strategy with no mind configured must not boot:
        // the veto is part of the strategy's spec'd shape (Section 6), and
        // skipping it silently would un-measure the thing being measured.
        if !config.veto_strategies.is_empty() && config.veto_mind.is_none() {
            return Err(RunnerError::Config {
                reason: format!(
                    "strategies {:?} are veto-enrolled but no veto mind is configured",
                    config
                        .veto_strategies
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                ),
            });
        }
        let clock = Arc::new(SimClock::new(start));
        let venue = SimVenue::new(
            VenueId::new("sim").map_err(|e| RunnerError::Config {
                reason: e.to_string(),
            })?,
            clock.clone(),
            config.fee_model.clone(),
            config.faults,
            config.starting_cash,
        );
        let mut market_ids = Vec::new();
        let mut market_meta = BTreeMap::new();
        for m in &config.markets {
            venue.add_market(m.clone());
            market_ids.push(m.id.clone());
            market_meta.insert(m.id.clone(), m.clone());
        }
        let manager = futures::executor::block_on(OrderManager::recover(
            MemoryJournal::default(),
            clock.clone(),
            config.exec_policy,
        ))?;
        Ok(SimRunner {
            bus: EventBus::new(clock.clone()),
            venue,
            gates: GatePipeline::new(config.gate_config)?,
            manager,
            positions: PositionBook::new(),
            reservations: ReservationLedger::new(config.envelopes.clone()),
            drawdown: DrawdownMonitor::new(config.max_daily_loss),
            strategies,
            audit,
            groups: GroupTracker::default(),
            fee_model: config.fee_model,
            mark_policy: config.mark_policy,
            books: BTreeMap::new(),
            markets: market_ids,
            envelope_names: config.envelopes.keys().cloned().collect(),
            ids: IdGen::new(config.seed),
            cursor: Cursor::start(),
            seen_events: 0,
            max_sets_per_proposal: config.max_sets_per_proposal,
            clock,
            audit_dead: false,
            veto_mind: config.veto_mind,
            veto_strategies: config.veto_strategies.into_iter().collect(),
            open_vetoes: Vec::new(),
            market_meta,
        })
    }

    pub fn venue(&self) -> &SimVenue {
        &self.venue
    }

    pub fn gates(&self) -> &GatePipeline {
        &self.gates
    }

    pub fn gates_mut(&mut self) -> &mut GatePipeline {
        &mut self.gates
    }

    pub fn positions(&self) -> &PositionBook {
        &self.positions
    }

    pub fn manager(&self) -> &OrderManager<MemoryJournal> {
        &self.manager
    }

    pub fn recording(&self) -> &Recording {
        self.bus.recording()
    }

    /// Audit + tolerate-the-untolerable: an audit failure is a GLOBAL HALT
    /// (I5: no audit, no trading), recorded on the bus (which still works).
    fn audit(&mut self, kind: &str, ref_id: Option<&str>, payload: serde_json::Value) {
        if self.audit_dead {
            return;
        }
        if let Err(e) = self.audit.append(kind, ref_id, payload) {
            self.audit_dead = true;
            self.gates
                .set_halt(HaltScope::Global, format!("audit write failure: {e}"));
            self.bus.publish_external(EventPayload::Raw {
                kind: "audit_failure_halt".into(),
                data: serde_json::json!({ "error": e.to_string() }),
            });
        }
    }

    /// One deterministic cycle.
    pub async fn tick(&mut self) -> Result<TickReport, RunnerError> {
        let mut report = TickReport::default();

        // 1. Venue data enters the bus (point-in-time record).
        for market in self.markets.clone() {
            match self.venue.book(&market).await {
                Ok(book) => {
                    self.books.insert(market.clone(), book.clone());
                    self.bus.publish_external(EventPayload::BookSnapshot {
                        venue: self.venue.id(),
                        book,
                    });
                }
                Err(e) => self.bus.publish_external(EventPayload::Raw {
                    kind: "book_error".into(),
                    data: serde_json::json!({ "market": market.to_string(), "error": e.to_string() }),
                }),
            }
        }
        self.bus.run_until_idle()?;

        // 2. Strategies see newly recorded events, in registration order.
        let new_events: Vec<fortuna_core::bus::BusEvent> = self
            .bus
            .recording()
            .events()
            .iter()
            .skip(self.seen_events)
            .cloned()
            .collect();
        self.seen_events = self.bus.recording().events().len();
        report.events_dispatched = new_events.len();

        let mut proposals: Vec<(StrategyId, Proposal)> = Vec::new();
        for ev in &new_events {
            let core = CoreHandle {
                now: self.clock.now(),
                books: &self.books,
                markets: &self.market_meta,
                fee_model: &self.fee_model,
            };
            for strategy in &mut self.strategies {
                let out = strategy.on_event(ev, &core).await?;
                for p in out {
                    proposals.push((strategy.id(), p));
                }
            }
        }
        report.proposals = proposals.len();

        // 3. Size -> gate -> submit.
        for (strategy_id, proposal) in proposals {
            self.handle_proposal(strategy_id, proposal, &mut report)
                .await?;
        }

        // 4. Fills -> positions/reservations -> bus.
        self.drain_fills(&mut report).await?;

        // 5. Drawdown (conservative marks) -> halt on breach (I2).
        self.check_drawdown().await?;

        // 6. Group policy evaluation -> complete-or-unwind decisions.
        let decisions = self.groups.evaluate(&self.manager, self.clock.now());
        report.group_decisions = decisions.len();
        for decision in decisions {
            self.handle_group_decision(decision, &mut report).await?;
        }

        // 7. TTL sweep (re-quotes come from strategies re-proposing).
        let swept = self.manager.sweep_ttl(&self.venue).await?;
        for intent in swept {
            self.release_if_terminal(intent)?;
            self.audit(
                "order",
                Some(&intent.to_string()),
                serde_json::json!({ "action": "ttl_cancel" }),
            );
        }

        report.halted = self.gates.halts().global_halted().is_some();
        Ok(report)
    }

    async fn handle_proposal(
        &mut self,
        strategy_id: StrategyId,
        proposal: Proposal,
        report: &mut TickReport,
    ) -> Result<(), RunnerError> {
        if proposal.legs.is_empty() {
            return Ok(());
        }
        // Sizing (the harness's job): all-in cost per 1-contract set, then
        // envelope headroom / caps decide the count.
        let mut cost_per_set = Cents::ZERO;
        for leg in &proposal.legs {
            let fee = self
                .fee_model
                .fee(
                    fortuna_core::book::FillRole::Taker,
                    leg.limit_price,
                    Contracts::new(1),
                    None,
                    self.clock.now(),
                )
                .map_err(|e| RunnerError::Config {
                    reason: format!("fee in sizing: {e}"),
                })?
                .max(Cents::ZERO);
            cost_per_set = cost_per_set
                .checked_add(leg.limit_price)
                .and_then(|c| c.checked_add(fee))?;
        }
        let headroom = self
            .reservations
            .headroom(strategy_id.as_str())
            .unwrap_or(Cents::ZERO);
        let mut sets = affordable_sets(headroom, cost_per_set, self.max_sets_per_proposal);
        if sets == 0 {
            self.audit(
                "gate_decision",
                None,
                serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "verdict": "unsized",
                    "reason": format!("no envelope headroom ({headroom}) for set cost {cost_per_set}"),
                }),
            );
            return Ok(());
        }

        // Model veto (spec Section 6): reduce-only, audited, and strictly
        // BEFORE the gates — the gates never consult the model (I1); the
        // model never sees the gates. Only enrolled strategies pay it.
        if self.veto_strategies.contains(&strategy_id) {
            sets = self.consult_veto(&strategy_id, &proposal, sets).await?;
            if sets == 0 {
                return Ok(());
            }
        }

        let group = if proposal.legs.len() > 1 {
            Some(IntentGroupId::new(self.ids.next(self.clock.now())?))
        } else {
            None
        };
        let mut group_legs = Vec::new();

        for leg in &proposal.legs {
            let intent = IntentId::new(self.ids.next(self.clock.now())?);
            let candidate = CandidateOrder {
                intent_id: intent,
                strategy: strategy_id.clone(),
                venue: self.venue.id(),
                market: leg.market.clone(),
                side: leg.side,
                action: leg.action,
                limit_price: leg.limit_price,
                qty: Contracts::new(sets),
                fair_value: leg.fair_value,
                client_order_id: ClientOrderId::from_intent(intent),
            };
            let outcome = self.evaluate_gates(&candidate);
            for record in &outcome.records {
                self.audit(
                    "gate_decision",
                    Some(&intent.to_string()),
                    serde_json::to_value(record).unwrap_or_default(),
                );
            }
            match outcome.gated {
                Err(rejection) => {
                    report.gate_rejections += 1;
                    self.bus.publish_external(EventPayload::Raw {
                        kind: "gate_reject".into(),
                        data: serde_json::json!({
                            "intent": intent.to_string(),
                            "check": format!("{:?}", rejection.check),
                            "reason": rejection.reason,
                        }),
                    });
                    // All-or-nothing groups: drop remaining legs (none
                    // submitted yet for this proposal if the FIRST leg
                    // rejects; if a later leg rejects, the group policy
                    // unwind machinery handles the imbalance).
                    if group.is_some() && group_legs.is_empty() {
                        break;
                    }
                    continue;
                }
                Ok(gated) => {
                    // Reserve BEFORE submission (spec 5.14: reserve at gate
                    // time); exact amount released on terminal states.
                    let leg_cost = notional_plus_worst_fee(
                        &self.fee_model,
                        leg.limit_price,
                        sets,
                        self.clock.now(),
                    )?;
                    self.reservations
                        .reserve(strategy_id.as_str(), intent, leg_cost)?;
                    let submit = self.manager.submit_grouped(gated, group, &self.venue).await;
                    match submit {
                        Ok(SubmitOutcome::Acked { .. }) => {
                            report.orders_submitted += 1;
                            group_legs.push(intent);
                        }
                        Ok(SubmitOutcome::Rejected { reason }) => {
                            let _ = self.reservations.release(intent)?;
                            self.audit(
                                "order",
                                Some(&intent.to_string()),
                                serde_json::json!({ "venue_rejected": reason }),
                            );
                        }
                        Ok(SubmitOutcome::Unknown { error }) => {
                            // Reservation stays (the order may be live);
                            // reconciliation resolves it.
                            report.orders_submitted += 1;
                            group_legs.push(intent);
                            self.audit(
                                "order",
                                Some(&intent.to_string()),
                                serde_json::json!({ "submit_unknown": error }),
                            );
                        }
                        Err(ExecError::WorkingOrderExists { .. }) => {
                            let _ = self.reservations.release(intent)?;
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }

        if let (Some(gid), policy) = (group, proposal.group_policy) {
            if !group_legs.is_empty() {
                let policy = policy.ok_or(RunnerError::Config {
                    reason: "multi-leg proposal without group policy".into(),
                })?;
                self.groups.open(gid, policy, group_legs, self.clock.now());
            }
        }
        let _ = proposal.urgency; // v0: arbs go taker via crossing limits
        Ok(())
    }

    /// Assess one sized proposal with the veto mind. Returns the kept
    /// quantity (0 = suppressed). Reduce-only is structural: the verdict
    /// type cannot express growth, and this method only ever returns
    /// `min(kept, sets)` quantities derived from `KeepBps::apply`.
    ///
    /// Failure semantics (ASSUMPTIONS, T1.3): an unanswered veto fails
    /// CLOSED (suppress — within the veto's reduce-only authority, risking
    /// zero capital) but is flagged `veto_error` and NEVER counterfactually
    /// scored: a provider outage is not model judgment. Multi-leg
    /// proposals from veto-enrolled strategies are suppressed whole —
    /// partial-group vetoes would manufacture unhedged legs, and no
    /// spec'd strategy needs group vetoes this phase.
    async fn consult_veto(
        &mut self,
        strategy_id: &StrategyId,
        proposal: &Proposal,
        sets: i64,
    ) -> Result<i64, RunnerError> {
        let Some(mind) = self.veto_mind.clone() else {
            // Construction guard makes this unreachable; fail closed anyway.
            return Ok(0);
        };
        if proposal.legs.len() != 1 {
            self.audit(
                "veto_decision",
                None,
                serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "veto_unsupported_multileg": true,
                    "legs": proposal.legs.len(),
                    "qty_before": sets,
                    "qty_after": 0,
                    "veto_error": false,
                }),
            );
            self.bus.publish_external(EventPayload::Raw {
                kind: "veto_decision".into(),
                data: serde_json::json!({
                    "strategy": strategy_id.to_string(),
                    "suppressed_multileg": proposal.legs.len(),
                }),
            });
            return Ok(0);
        }
        let leg = &proposal.legs[0];
        let book = self.books.get(&leg.market);
        let candidate = VetoCandidate {
            strategy: strategy_id.clone(),
            market: leg.market.clone(),
            side: leg.side,
            action: leg.action,
            limit_price: leg.limit_price,
            fair_value: leg.fair_value,
            qty: Contracts::new(sets),
            yes_bid: book.and_then(|b| b.yes_bids.first()).map(|l| l.price),
            yes_ask: book.and_then(|b| b.yes_asks.first()).map(|l| l.price),
            category: self
                .market_meta
                .get(&leg.market)
                .map(|m| m.category.clone()),
            thesis: proposal.thesis.clone(),
            as_of: self.clock.now(),
        };
        match mind.assess(&candidate).await {
            Ok(assessment) => {
                let kept = match &assessment.verdict {
                    VetoVerdict::Allow => sets,
                    VetoVerdict::Shrink { keep, .. } => keep.apply(Contracts::new(sets)).raw(),
                    VetoVerdict::Suppress { .. } => 0,
                };
                self.audit(
                    "veto_decision",
                    None,
                    serde_json::json!({
                        "candidate": serde_json::to_value(&candidate).unwrap_or_default(),
                        "assessment": serde_json::to_value(&assessment).unwrap_or_default(),
                        "qty_before": sets,
                        "qty_after": kept,
                        "veto_error": false,
                    }),
                );
                if kept < sets {
                    self.bus.publish_external(EventPayload::Raw {
                        kind: "veto_decision".into(),
                        data: serde_json::json!({
                            "market": leg.market.to_string(),
                            "qty_before": sets,
                            "qty_after": kept,
                        }),
                    });
                    self.open_vetoes.push(OpenVeto {
                        candidate,
                        removed: Contracts::new(sets - kept),
                    });
                }
                Ok(kept)
            }
            Err(e) => {
                self.audit(
                    "veto_decision",
                    None,
                    serde_json::json!({
                        "candidate": serde_json::to_value(&candidate).unwrap_or_default(),
                        "error": e.to_string(),
                        "qty_before": sets,
                        "qty_after": 0,
                        "veto_error": true,
                    }),
                );
                self.bus.publish_external(EventPayload::Raw {
                    kind: "veto_decision".into(),
                    data: serde_json::json!({
                        "market": leg.market.to_string(),
                        "veto_error": e.to_string(),
                    }),
                });
                Ok(0)
            }
        }
    }

    fn evaluate_gates(&mut self, candidate: &CandidateOrder) -> fortuna_gates::GateOutcome {
        // Exposure views: worst case = active reservations + open cost
        // basis (long binary risk = premium paid). Spec 5.13/5.14.
        let reserved_total: Cents = {
            let mut sum = Cents::ZERO;
            for s in &self.envelope_names {
                sum = sum
                    .checked_add(self.reservations.active_total(s))
                    .unwrap_or(sum);
            }
            sum
        };
        let positions_cost: Cents = self
            .positions
            .positions()
            .map(|p| {
                p.yes
                    .cost_basis
                    .checked_add(p.no.cost_basis)
                    .unwrap_or(Cents::ZERO)
            })
            .fold(Cents::ZERO, |acc, c| acc.checked_add(c).unwrap_or(acc));
        let open_exposure = reserved_total
            .checked_add(positions_cost)
            .unwrap_or(reserved_total);
        let market_exposure = self
            .positions
            .position(&candidate.market)
            .map(|p| {
                p.yes
                    .cost_basis
                    .checked_add(p.no.cost_basis)
                    .unwrap_or(Cents::ZERO)
            })
            .unwrap_or(Cents::ZERO);
        let strategy_exposure = self.reservations.active_total(candidate.strategy.as_str());
        let recent = self.manager.known_client_order_ids();
        let own_resting: Vec<fortuna_gates::RestingOrderView> = self
            .manager
            .intents()
            .iter()
            .filter(|(_, r)| r.status.is_working())
            .map(|(_, r)| fortuna_gates::RestingOrderView {
                market: r.order.market.clone(),
                side: r.order.side,
                action: r.order.action,
                price: r.order.limit_price,
            })
            .collect();
        let inputs = GateInputs {
            now: self.clock.now(),
            open_exposure_cents: open_exposure,
            market_exposure_cents: market_exposure,
            strategy_exposure_cents: strategy_exposure,
            event_exposure_cents: Cents::ZERO,
            event_id: None,
            book: self.books.get(&candidate.market),
            last_trade_price: None,
            fee_model: &self.fee_model,
            category: None,
            own_resting: &own_resting,
            recent_client_order_ids: &recent,
        };
        self.gates.evaluate(candidate, &inputs)
    }

    async fn drain_fills(&mut self, report: &mut TickReport) -> Result<(), RunnerError> {
        for _ in 0..1_000 {
            let page = match self.venue.fills_since(self.cursor.clone()).await {
                Ok(p) => p,
                Err(fortuna_venues::VenueError::Outage { .. }) => break, // next tick
                Err(e) => return Err(e.into()),
            };
            let advanced = page.next_cursor != self.cursor;
            for fill in &page.fills {
                let app = self.manager.ingest_fill(fill).await?;
                if app.applied {
                    self.positions.apply_fill(fill)?;
                    report.fills_applied += 1;
                    self.bus.publish_external(EventPayload::FillSeen {
                        venue: self.venue.id(),
                        fill: fill.clone(),
                    });
                    self.audit(
                        "fill",
                        Some(&fill.fill_id),
                        serde_json::to_value(fill).unwrap_or_default(),
                    );
                    self.release_if_terminal(app.intent)?;
                }
            }
            self.cursor = page.next_cursor;
            if !advanced && page.fills.is_empty() {
                break;
            }
        }
        self.bus.run_until_idle()?;
        self.seen_events = self.bus.recording().events().len();
        Ok(())
    }

    fn release_if_terminal(&mut self, intent: IntentId) -> Result<(), RunnerError> {
        let terminal = self
            .manager
            .intent(intent)
            .map(|r| {
                matches!(
                    r.status,
                    IntentStatus::Filled
                        | IntentStatus::Cancelled
                        | IntentStatus::Rejected
                        | IntentStatus::BootClosed
                )
            })
            .unwrap_or(false);
        if terminal {
            let _ = self.reservations.release(intent)?;
        }
        Ok(())
    }

    async fn check_drawdown(&mut self) -> Result<(), RunnerError> {
        // Equity = venue cash (total incl. reserved) + conservative marks.
        let (cash, _, _, _) = self.venue.inspect_totals();
        let mut marks = Cents::ZERO;
        for p in self.positions.positions() {
            let mark = mark_lots(
                p.yes.qty,
                p.no.qty,
                self.books.get(&p.market),
                self.clock.now(),
                &self.mark_policy,
            )?;
            marks = marks.checked_add(mark.value)?;
        }
        let equity = cash.checked_add(marks)?;
        if let DrawdownVerdict::Breach { loss, limit } =
            self.drawdown.check(self.clock.now(), equity)?
        {
            if self.gates.halts().global_halted().is_none() {
                self.gates.set_halt(
                    HaltScope::Global,
                    format!("drawdown breach: loss {loss} >= limit {limit}"),
                );
                self.audit(
                    "halt",
                    None,
                    serde_json::json!({
                        "scope": "global",
                        "reason": format!("drawdown breach: loss {loss} >= limit {limit}"),
                    }),
                );
                self.bus.publish_external(EventPayload::Raw {
                    kind: "halt".into(),
                    data: serde_json::json!({ "reason": "drawdown" }),
                });
            }
        }
        Ok(())
    }

    async fn handle_group_decision(
        &mut self,
        decision: GroupDecision,
        report: &mut TickReport,
    ) -> Result<(), RunnerError> {
        match decision {
            GroupDecision::Complete { group } => {
                self.groups.mark_closed(group);
                self.audit(
                    "order",
                    None,
                    serde_json::json!({ "group": group.to_string(), "group_complete": true }),
                );
                Ok(())
            }
            GroupDecision::Breached {
                group,
                reason,
                unfilled_legs,
            } => {
                self.audit(
                    "order",
                    None,
                    serde_json::json!({
                        "group": group.to_string(),
                        "group_breached": reason,
                    }),
                );
                // Deterministic complete-or-unwind over current books.
                let remaining: Vec<RemainingLeg> = unfilled_legs
                    .iter()
                    .filter_map(|leg| self.manager.intent(*leg))
                    .map(|r| RemainingLeg {
                        market: r.order.market.clone(),
                        side: r.order.side,
                        action: r.order.action,
                        remaining: Contracts::new(r.order.qty.raw() - r.cum_filled.raw()),
                    })
                    .collect();
                let policy = match self.groups.group(group) {
                    Some(g) => g.policy.clone(),
                    None => return Ok(()),
                };
                let verdict = decide_complete_or_unwind(
                    &remaining,
                    &self.books,
                    &self.fee_model,
                    &policy,
                    self.clock.now(),
                );
                match verdict {
                    CompleteOrUnwind::TakerComplete {
                        est_cost,
                        net_edge_bps,
                    } => {
                        // Completion orders go BACK through the gates as
                        // fresh candidates (I1 applies to recovery too).
                        self.audit(
                            "order",
                            None,
                            serde_json::json!({
                                "group": group.to_string(),
                                "taker_complete": { "est_cost": est_cost.raw(), "bps": net_edge_bps },
                            }),
                        );
                        // v0: cancel stale resting legs; strategies re-propose
                        // against fresh books (the scan fires again).
                        for leg in unfilled_legs {
                            let _ = self.manager.cancel_intent(leg, &self.venue).await;
                            self.release_if_terminal(leg)?;
                        }
                        self.groups.mark_closed(group);
                    }
                    CompleteOrUnwind::Unwind { reason } => {
                        // v0 unwind = freeze the group: cancel unfilled legs,
                        // HOLD filled lots (panic-selling thin books is the
                        // flatten planner's veto philosophy), alert loudly.
                        for leg in unfilled_legs {
                            let _ = self.manager.cancel_intent(leg, &self.venue).await;
                            self.release_if_terminal(leg)?;
                        }
                        self.groups.mark_unwinding(group);
                        self.audit(
                            "order",
                            None,
                            serde_json::json!({
                                "group": group.to_string(),
                                "unwind_frozen": reason,
                            }),
                        );
                        self.bus.publish_external(EventPayload::Raw {
                            kind: "group_unwind_frozen".into(),
                            data: serde_json::json!({ "group": group.to_string() }),
                        });
                    }
                }
                report.group_decisions += 1;
                Ok(())
            }
        }
    }

    /// Final accounting for reports/tests.
    pub fn report(&self) -> Result<RunnerReport, RunnerError> {
        let (cash, _, _, _) = self.venue.inspect_totals();
        let mut realized = Cents::ZERO;
        let mut fees = Cents::ZERO;
        for p in self.positions.positions() {
            realized = realized.checked_add(p.realized_pnl)?;
            fees = fees.checked_add(p.fees_paid)?;
        }
        Ok(RunnerReport {
            recording_jsonl: self.bus.recording().to_jsonl()?,
            final_cash: cash,
            realized_pnl: realized,
            fees_paid: fees,
        })
    }

    /// Apply a venue settlement to local books (sim convenience mirroring
    /// what the settlement processors automate at T1.4).
    pub fn apply_settlement(
        &mut self,
        market: &MarketId,
        winner: fortuna_core::market::Side,
        payout: Cents,
    ) -> Result<(), RunnerError> {
        // Counterfactually score vetoed-away quantity FIRST (spec Section
        // 6): a fully suppressed trade left no position behind, but the
        // settlement outcome is observable either way and the veto's
        // value-add must be measured. Exactly-once: scored entries drain.
        let drained = std::mem::take(&mut self.open_vetoes);
        let (due, keep): (Vec<OpenVeto>, Vec<OpenVeto>) = drained
            .into_iter()
            .partition(|v| &v.candidate.market == market);
        self.open_vetoes = keep;
        for v in due {
            let payload = match counterfactual_pnl(
                &v.candidate,
                v.removed,
                winner,
                Cents::new(100),
                &self.fee_model,
                FillAssumption::FilledAtLimit,
            ) {
                Ok(pnl) => serde_json::json!({
                    "candidate": serde_json::to_value(&v.candidate).unwrap_or_default(),
                    "removed": v.removed.raw(),
                    "winner": format!("{winner:?}"),
                    "hypothetical_pnl_cents": pnl.raw(),
                    "fill_assumption": serde_json::to_value(FillAssumption::FilledAtLimit)
                        .unwrap_or_default(),
                }),
                Err(e) => serde_json::json!({
                    "candidate": serde_json::to_value(&v.candidate).unwrap_or_default(),
                    "removed": v.removed.raw(),
                    "winner": format!("{winner:?}"),
                    "score_error": e.to_string(),
                }),
            };
            self.audit("veto_counterfactual", Some(market.as_str()), payload);
        }
        // Markets settle whether we hold them or not; the strict state
        // layer (untracked-market settlement is an error there) is only
        // consulted when a position actually exists.
        let owed = if self.positions.position(market).is_some() {
            self.positions
                .apply_settlement(market, winner, Cents::new(100))?
        } else {
            Cents::ZERO
        };
        self.bus.publish_external(EventPayload::Settled {
            venue: self.venue.id(),
            market: market.clone(),
            payout_cents: payout.raw(),
        });
        self.audit(
            "settlement",
            Some(market.as_str()),
            serde_json::json!({ "winner": format!("{winner:?}"), "owed": owed.raw() }),
        );
        self.bus.run_until_idle()?;
        self.seen_events = self.bus.recording().events().len();
        Ok(())
    }
}

fn notional_plus_worst_fee(
    fees: &ScheduleFeeModel,
    price: Cents,
    qty: i64,
    at: UtcTimestamp,
) -> Result<Cents, RunnerError> {
    use fortuna_core::book::FillRole;
    let cost = price.checked_mul(qty)?;
    let taker = fees
        .fee(FillRole::Taker, price, Contracts::new(qty), None, at)
        .map_err(|e| RunnerError::Config {
            reason: format!("fee: {e}"),
        })?;
    let maker = fees
        .fee(FillRole::Maker, price, Contracts::new(qty), None, at)
        .map_err(|e| RunnerError::Config {
            reason: format!("fee: {e}"),
        })?;
    Ok(cost.checked_add(taker.max(maker).max(Cents::ZERO))?)
}
