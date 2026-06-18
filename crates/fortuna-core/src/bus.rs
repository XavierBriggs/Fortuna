//! `BusEvent` (bus message; distinct from canonical events, spec 5.12),
//! deterministic single-threaded dispatch, replay recorder/player.
//!
//! Determinism contract (spec 5.1): FIFO queue, handlers dispatched in
//! registration order, dense `seq` assigned and timestamp stamped at dispatch
//! from the injected `Clock`, every dispatched event recorded. Replay
//! re-injects recorded external events, regenerates handler-derived events
//! through the same deterministic handlers, and byte-compares against the
//! recording; any divergence is an error naming the first divergent seq.
//!
//! Fail-closed: a handler error aborts dispatch and propagates. A bus error
//! is fatal to the run; there are no resume semantics (the runner halts).

use crate::book::{Fill, OrderBook};
use crate::clock::{Clock, UtcTimestamp};
use crate::market::{MarketId, VenueId};
use crate::perp::{FundingObservation, PerpMarks};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use thiserror::Error;

/// Errors from bus dispatch, recording, and replay.
#[derive(Debug, Error)]
pub enum BusError {
    /// A handler refused an event. Fail-closed: dispatch stops.
    #[error("handler {handler} failed at seq {seq}: {message}")]
    Handler {
        handler: String,
        seq: u64,
        message: String,
    },
    /// Handler ids must be unique (origin attribution would be ambiguous).
    #[error("duplicate handler id {id:?}")]
    DuplicateHandler { id: String },
    /// 2^64 events were dispatched (unreachable; erroring beats wrapping).
    #[error("event sequence number overflow")]
    SeqOverflow,
    /// Recording (de)serialization failure.
    #[error("recording serialization failed at line {line}: {reason}")]
    Serialization { line: usize, reason: String },
    /// Replay produced a stream that differs from the recording.
    #[error("replay diverged at seq {seq}: expected {expected}, got {got}")]
    ReplayDivergence {
        seq: u64,
        expected: String,
        got: String,
    },
    /// The sim clock refused a movement during replay (corrupt recording).
    #[error("replay clock error: {0}")]
    ReplayClock(#[from] crate::clock::ClockError),
}

impl BusError {
    /// Helper for handlers; the bus fills in the seq at the dispatch site.
    pub fn handler(handler: impl Into<String>, message: impl Into<String>) -> Self {
        BusError::Handler {
            handler: handler.into(),
            seq: 0,
            message: message.into(),
        }
    }
}

/// Payload of a bus message. Typed variants are added by the tasks that own
/// them; `Raw` exists for tests, DST scaffolding, and not-yet-typed flows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    Raw {
        kind: String,
        data: serde_json::Value,
    },
    /// Point-in-time book for a tracked market (T0.10: the strategy clock).
    BookSnapshot { venue: VenueId, book: OrderBook },
    /// A deduplicated fill applied to our books (T0.10).
    FillSeen { venue: VenueId, fill: Fill },
    /// Scheduled wakeup (TTL sweeps, group evaluation).
    Timer { name: String },
    /// A market settled at the venue.
    Settled {
        venue: VenueId,
        market: MarketId,
        payout_cents: i64,
    },
    /// Perp market data (spec 5.15; perp-strategies design §2.1): the marks
    /// (settlement + conservative) and a funding observation (the venue
    /// estimate + next_funding_time + reference price), every field grounded
    /// in the WS `ticker` frame + `/funding_rates/estimate` fixtures (§4).
    /// The live recorder publishes these; the Sim/DST/paper harness injects
    /// them. A perp strategy reads them in `on_event` — no `CoreHandle`
    /// surgery. The premium proxy is `marks.venue_settlement −
    /// funding.reference_price`.
    PerpTick {
        venue: VenueId,
        market: MarketId,
        marks: PerpMarks,
        funding: FundingObservation,
    },
}

/// Who put the event on the bus: the outside world (IO edges, the harness)
/// or a handler reacting to an earlier event. Replay re-injects External
/// events and expects Handler events to be regenerated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", content = "id", rename_all = "snake_case")]
pub enum EventOrigin {
    External,
    Handler(String),
}

/// A dispatched bus message. `seq` is dense from 0; `at` is the injected
/// clock's time at dispatch. Field order is the serialization contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusEvent {
    pub seq: u64,
    pub at: UtcTimestamp,
    pub origin: EventOrigin,
    pub payload: EventPayload,
}

/// Collects events a handler publishes while processing one event.
/// Published events queue FIFO behind everything already pending.
#[derive(Default)]
pub struct Outbox {
    published: Vec<EventPayload>,
}

impl Outbox {
    pub fn publish(&mut self, payload: EventPayload) {
        self.published.push(payload);
    }
}

/// A bus subscriber. `id` must be unique per bus and stable across runs
/// (it is recorded as event origin and checked by replay).
pub trait Handler {
    fn id(&self) -> &str;
    fn on_event(&mut self, ev: &BusEvent, out: &mut Outbox) -> Result<(), BusError>;
}

/// The ordered, append-only record of every dispatched event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    events: Vec<BusEvent>,
}

impl Recording {
    pub fn events(&self) -> &[BusEvent] {
        &self.events
    }

    /// One JSON object per line, in dispatch order, starting from `start`.
    ///
    /// `start` is clamped to `self.events.len()` — values at or beyond the end
    /// produce an empty string without panicking. This is the building block for
    /// incremental segment persist (A6): each segment calls `to_jsonl_from(last)`
    /// where `last` is the count persisted at the previous segment boundary, so
    /// only NEW events are serialized and no event is ever persisted twice.
    pub fn to_jsonl_from(&self, start: usize) -> Result<String, BusError> {
        let clamped = start.min(self.events.len());
        let mut out = String::new();
        for (i, ev) in self.events[clamped..].iter().enumerate() {
            let line = serde_json::to_string(ev).map_err(|e| BusError::Serialization {
                line: clamped + i + 1,
                reason: e.to_string(),
            })?;
            out.push_str(&line);
            out.push('\n');
        }
        Ok(out)
    }

    /// One JSON object per line, in dispatch order (all events).
    ///
    /// Equivalent to `self.to_jsonl_from(0)`.
    pub fn to_jsonl(&self) -> Result<String, BusError> {
        self.to_jsonl_from(0)
    }

    pub fn from_jsonl(input: &str) -> Result<Self, BusError> {
        let mut events = Vec::new();
        for (i, line) in input.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let ev: BusEvent = serde_json::from_str(line).map_err(|e| BusError::Serialization {
                line: i + 1,
                reason: e.to_string(),
            })?;
            events.push(ev);
        }
        Ok(Recording { events })
    }
}

/// Single-threaded deterministic event bus.
pub struct EventBus {
    clock: Arc<dyn Clock>,
    queue: VecDeque<(EventOrigin, EventPayload)>,
    handlers: Vec<Box<dyn Handler>>,
    next_seq: u64,
    recording: Recording,
}

impl EventBus {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        EventBus {
            clock,
            queue: VecDeque::new(),
            handlers: Vec::new(),
            next_seq: 0,
            recording: Recording::default(),
        }
    }

    pub fn subscribe(&mut self, handler: Box<dyn Handler>) -> Result<(), BusError> {
        if self.handlers.iter().any(|h| h.id() == handler.id()) {
            return Err(BusError::DuplicateHandler {
                id: handler.id().to_string(),
            });
        }
        self.handlers.push(handler);
        Ok(())
    }

    /// Enqueue an event from outside the bus (IO edges, the harness).
    pub fn publish_external(&mut self, payload: EventPayload) {
        self.queue.push_back((EventOrigin::External, payload));
    }

    /// Dispatch until the queue is empty. Fail-closed: the first handler
    /// error aborts the run with the queue left as-is.
    pub fn run_until_idle(&mut self) -> Result<(), BusError> {
        while let Some((origin, payload)) = self.queue.pop_front() {
            self.dispatch_one(origin, payload)?;
        }
        Ok(())
    }

    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    /// Stamp, record, and run one event through every handler in
    /// registration order; then enqueue everything they published.
    fn dispatch_one(&mut self, origin: EventOrigin, payload: EventPayload) -> Result<(), BusError> {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.checked_add(1).ok_or(BusError::SeqOverflow)?;
        let ev = BusEvent {
            seq,
            at: self.clock.now(),
            origin,
            payload,
        };
        // Recorded before handlers run: audit truth is "this was dispatched",
        // independent of whether a handler then failed on it.
        self.recording.events.push(ev.clone());

        let mut outbox = Outbox::default();
        for handler in &mut self.handlers {
            let handler_id = handler.id().to_string();
            handler.on_event(&ev, &mut outbox).map_err(|e| match e {
                BusError::Handler {
                    handler, message, ..
                } => BusError::Handler {
                    handler,
                    seq,
                    message,
                },
                other => other,
            })?;
            // Attribute publishes to the handler that made them, preserving
            // per-handler publish order.
            for payload in std::mem::take(&mut outbox.published) {
                self.queue
                    .push_back((EventOrigin::Handler(handler_id.clone()), payload));
            }
        }
        Ok(())
    }
}

/// Verify a recording replays byte-identically: external events are
/// re-injected (the replay clock is driven from recorded stamps), derived
/// events must be regenerated exactly by `handlers` (constructed identically
/// to the original run, e.g. same seeds). Returns the first divergence.
pub fn replay_verify(
    recording: &Recording,
    handlers: Vec<Box<dyn Handler>>,
) -> Result<(), BusError> {
    let events = recording.events();
    let start = match events.first() {
        Some(ev) => ev.at,
        None => return Ok(()), // nothing to verify
    };
    let clock = Arc::new(crate::clock::SimClock::new(start));
    let mut bus = EventBus::new(clock.clone());
    for handler in handlers {
        bus.subscribe(handler)?;
    }

    for expected in events {
        // Recorded time is authoritative during replay.
        clock.set(expected.at)?;
        match &expected.origin {
            EventOrigin::External => {
                bus.dispatch_one(EventOrigin::External, expected.payload.clone())?;
            }
            EventOrigin::Handler(_) => {
                // Must already be sitting at the head of the replay queue.
                match bus.queue.pop_front() {
                    Some((origin, payload)) => bus.dispatch_one(origin, payload)?,
                    None => {
                        return Err(BusError::ReplayDivergence {
                            seq: expected.seq,
                            expected: describe(expected),
                            got: "nothing (replay queue empty)".to_string(),
                        })
                    }
                }
            }
        }
        // Compare what was just dispatched against the recording, byte-wise.
        let got = match bus.recording.events.last() {
            Some(ev) => ev,
            None => {
                return Err(BusError::ReplayDivergence {
                    seq: expected.seq,
                    expected: describe(expected),
                    got: "nothing (no event dispatched)".to_string(),
                })
            }
        };
        if got != expected {
            return Err(BusError::ReplayDivergence {
                seq: expected.seq,
                expected: describe(expected),
                got: describe(got),
            });
        }
    }

    // Anything still pending was produced by handlers but never recorded:
    // the recording is truncated or the handlers diverged.
    if let Some((origin, payload)) = bus.queue.pop_front() {
        return Err(BusError::ReplayDivergence {
            seq: bus.next_seq,
            expected: "end of recording".to_string(),
            got: format!("pending {origin:?} event {payload:?}"),
        });
    }
    Ok(())
}

fn describe(ev: &BusEvent) -> String {
    serde_json::to_string(ev).unwrap_or_else(|e| format!("<unserializable event: {e}>"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{SimClock, UtcTimestamp};

    #[test]
    fn seq_overflow_is_an_error_not_a_wrap() {
        let clock = Arc::new(SimClock::new(UtcTimestamp::from_epoch_millis(0).unwrap()));
        let mut bus = EventBus::new(clock);
        bus.next_seq = u64::MAX;
        bus.publish_external(EventPayload::Raw {
            kind: "k".into(),
            data: serde_json::Value::Null,
        });
        assert!(matches!(bus.run_until_idle(), Err(BusError::SeqOverflow)));
    }
}
