//! T0.2 tests: deterministic event bus + replay. Written from spec 5.1 before
//! implementation.
//!
//! Contract: single-threaded deterministic dispatch (FIFO queue, handlers in
//! registration order, dense seq assigned at dispatch, timestamps from the
//! injected Clock), every dispatched event recorded, and replay: re-inject
//! recorded external events, regenerate derived events through the same
//! deterministic handlers, byte-compare against the recording. Same seed +
//! same inputs => byte-identical event stream.

use fortuna_core::bus::{
    replay_verify, BusError, BusEvent, EventBus, EventOrigin, EventPayload, Handler, Outbox,
    Recording,
};
use fortuna_core::clock::{SimClock, UtcTimestamp};
use fortuna_core::ids::IdGen;
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

fn ts(ms: i64) -> UtcTimestamp {
    UtcTimestamp::from_epoch_millis(ms).unwrap()
}

fn raw(kind: &str, data: serde_json::Value) -> EventPayload {
    EventPayload::Raw {
        kind: kind.to_string(),
        data,
    }
}

/// Records every event it sees into a shared log as (seq, handler_id).
struct Probe {
    id: String,
    log: Rc<RefCell<Vec<(u64, String)>>>,
}

impl Handler for Probe {
    fn id(&self) -> &str {
        &self.id
    }

    fn on_event(&mut self, ev: &BusEvent, _out: &mut Outbox) -> Result<(), BusError> {
        self.log.borrow_mut().push((ev.seq, self.id.clone()));
        Ok(())
    }
}

/// On every "tick" event, emits one seeded derived event. Deterministic:
/// identical seed + identical input stream => identical output.
struct Deriver {
    ids: IdGen,
    count: u64,
}

impl Deriver {
    fn new(seed: u64) -> Self {
        Deriver {
            ids: IdGen::new(seed),
            count: 0,
        }
    }
}

impl Handler for Deriver {
    fn id(&self) -> &str {
        "deriver"
    }

    fn on_event(&mut self, ev: &BusEvent, out: &mut Outbox) -> Result<(), BusError> {
        if let EventPayload::Raw { kind, .. } = &ev.payload {
            if kind == "tick" {
                let id = self
                    .ids
                    .next(ev.at)
                    .map_err(|e| BusError::handler("deriver", e.to_string()))?;
                out.publish(raw(
                    "derived",
                    json!({ "id": id.to_string(), "n": self.count }),
                ));
                self.count += 1;
            }
        }
        Ok(())
    }
}

/// Fails on the first event whose Raw kind matches.
struct FailsOn {
    kind: String,
}

impl Handler for FailsOn {
    fn id(&self) -> &str {
        "fails-on"
    }

    fn on_event(&mut self, ev: &BusEvent, _out: &mut Outbox) -> Result<(), BusError> {
        if let EventPayload::Raw { kind, .. } = &ev.payload {
            if *kind == self.kind {
                return Err(BusError::handler("fails-on", format!("refusing {kind}")));
            }
        }
        Ok(())
    }
}

// ---- dispatch mechanics ----

#[test]
fn dispatch_assigns_dense_seq_from_zero_and_stamps_clock_time() {
    let clock = Arc::new(SimClock::new(ts(1_000)));
    let mut bus = EventBus::new(clock.clone());
    bus.publish_external(raw("a", json!({})));
    bus.publish_external(raw("b", json!({})));
    bus.run_until_idle().unwrap();

    let events = bus.recording().events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].seq, 0);
    assert_eq!(events[1].seq, 1);
    assert_eq!(events[0].at, ts(1_000));
    assert_eq!(events[1].at, ts(1_000));
    assert!(matches!(events[0].origin, EventOrigin::External));
}

#[test]
fn timestamps_track_the_injected_clock_across_runs() {
    let clock = Arc::new(SimClock::new(ts(1_000)));
    let mut bus = EventBus::new(clock.clone());
    bus.publish_external(raw("a", json!({})));
    bus.run_until_idle().unwrap();
    clock.advance_millis(500).unwrap();
    bus.publish_external(raw("b", json!({})));
    bus.run_until_idle().unwrap();

    let events = bus.recording().events();
    assert_eq!(events[0].at, ts(1_000));
    assert_eq!(events[1].at, ts(1_500));
}

#[test]
fn handlers_dispatch_in_registration_order_per_event() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let clock = Arc::new(SimClock::new(ts(0)));
    let mut bus = EventBus::new(clock);
    bus.subscribe(Box::new(Probe {
        id: "first".into(),
        log: log.clone(),
    }))
    .unwrap();
    bus.subscribe(Box::new(Probe {
        id: "second".into(),
        log: log.clone(),
    }))
    .unwrap();
    bus.publish_external(raw("a", json!({})));
    bus.publish_external(raw("b", json!({})));
    bus.run_until_idle().unwrap();

    // Each event is fully dispatched to all handlers (registration order)
    // before the next event starts.
    assert_eq!(
        *log.borrow(),
        vec![
            (0, "first".to_string()),
            (0, "second".to_string()),
            (1, "first".to_string()),
            (1, "second".to_string()),
        ]
    );
}

#[test]
fn duplicate_handler_ids_are_rejected() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let clock = Arc::new(SimClock::new(ts(0)));
    let mut bus = EventBus::new(clock);
    bus.subscribe(Box::new(Probe {
        id: "dup".into(),
        log: log.clone(),
    }))
    .unwrap();
    let err = bus
        .subscribe(Box::new(Probe {
            id: "dup".into(),
            log,
        }))
        .unwrap_err();
    assert!(matches!(err, BusError::DuplicateHandler { .. }));
}

#[test]
fn handler_published_events_queue_fifo_behind_pending_events() {
    // Queue [e1, e2]; handling e1 publishes d1 -> dispatch order e1, e2, d1.
    let clock = Arc::new(SimClock::new(ts(0)));
    let mut bus = EventBus::new(clock);
    bus.subscribe(Box::new(Deriver::new(7))).unwrap();
    bus.publish_external(raw("tick", json!({})));
    bus.publish_external(raw("other", json!({})));
    bus.run_until_idle().unwrap();

    let kinds: Vec<&str> = bus
        .recording()
        .events()
        .iter()
        .map(|e| match &e.payload {
            EventPayload::Raw { kind, .. } => kind.as_str(),
            other => panic!("unexpected payload {other:?}"),
        })
        .collect();
    assert_eq!(kinds, vec!["tick", "other", "derived"]);
}

#[test]
fn derived_events_carry_the_publishing_handler_as_origin() {
    let clock = Arc::new(SimClock::new(ts(0)));
    let mut bus = EventBus::new(clock);
    bus.subscribe(Box::new(Deriver::new(7))).unwrap();
    bus.publish_external(raw("tick", json!({})));
    bus.run_until_idle().unwrap();

    let events = bus.recording().events();
    assert!(matches!(events[0].origin, EventOrigin::External));
    match &events[1].origin {
        EventOrigin::Handler(id) => assert_eq!(id, "deriver"),
        other => panic!("expected Handler origin, got {other:?}"),
    }
}

#[test]
fn empty_run_is_ok_and_records_nothing() {
    let clock = Arc::new(SimClock::new(ts(0)));
    let mut bus = EventBus::new(clock);
    bus.run_until_idle().unwrap();
    assert!(bus.recording().events().is_empty());
}

// ---- fail-closed handler errors ----

#[test]
fn handler_error_stops_dispatch_and_propagates() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let clock = Arc::new(SimClock::new(ts(0)));
    let mut bus = EventBus::new(clock);
    bus.subscribe(Box::new(FailsOn {
        kind: "poison".into(),
    }))
    .unwrap();
    bus.subscribe(Box::new(Probe {
        id: "after".into(),
        log: log.clone(),
    }))
    .unwrap();
    bus.publish_external(raw("ok", json!({})));
    bus.publish_external(raw("poison", json!({})));
    bus.publish_external(raw("never-dispatched", json!({})));

    let err = bus.run_until_idle().unwrap_err();
    assert!(matches!(err, BusError::Handler { .. }));
    // Fail-closed: the failing event reached handler 1 but not handler 2,
    // and the third event was never dispatched.
    assert_eq!(*log.borrow(), vec![(0, "after".to_string())]);
    assert_eq!(bus.recording().events().len(), 2); // ok + poison were dispatched (audit truth)
}

// ---- recording serialization ----

#[test]
fn recording_round_trips_through_jsonl() {
    let clock = Arc::new(SimClock::new(ts(123)));
    let mut bus = EventBus::new(clock);
    bus.subscribe(Box::new(Deriver::new(7))).unwrap();
    bus.publish_external(raw("tick", json!({"x": 1})));
    bus.run_until_idle().unwrap();

    let jsonl = bus.recording().to_jsonl().unwrap();
    let back = Recording::from_jsonl(&jsonl).unwrap();
    assert_eq!(back.events(), bus.recording().events());
    // Serialization is stable: serializing the parsed copy is byte-identical.
    assert_eq!(back.to_jsonl().unwrap(), jsonl);
}

#[test]
fn from_jsonl_rejects_malformed_input() {
    assert!(Recording::from_jsonl("not json\n").is_err());
    assert!(Recording::from_jsonl("{\"seq\":0}\n").is_err()); // missing fields
}

#[test]
fn raw_payload_map_keys_serialize_deterministically() {
    // serde_json's default Map is ordered (BTreeMap); insertion order of the
    // source must not leak into bytes.
    let a = raw("k", json!({"zebra": 1, "alpha": 2}));
    let b = raw("k", json!({"alpha": 2, "zebra": 1}));
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

// ---- THE determinism test: same seed => byte-identical stream ----

fn seeded_run(seed: u64) -> String {
    let clock = Arc::new(SimClock::new(ts(1_000)));
    let mut bus = EventBus::new(clock.clone());
    bus.subscribe(Box::new(Deriver::new(seed))).unwrap();
    for i in 0..10 {
        bus.publish_external(raw("tick", json!({ "i": i })));
        bus.run_until_idle().unwrap();
        clock.advance_millis(250).unwrap();
    }
    bus.recording().to_jsonl().unwrap()
}

#[test]
fn same_seed_produces_byte_identical_event_streams() {
    assert_eq!(seeded_run(42), seeded_run(42));
}

#[test]
fn different_seeds_produce_different_streams() {
    assert_ne!(seeded_run(1), seeded_run(2));
}

#[test]
fn recorded_stream_timestamps_are_non_decreasing_and_seq_dense() {
    let jsonl = seeded_run(42);
    let rec = Recording::from_jsonl(&jsonl).unwrap();
    let events = rec.events();
    for (i, ev) in events.iter().enumerate() {
        assert_eq!(ev.seq, i as u64);
        if i > 0 {
            assert!(ev.at >= events[i - 1].at);
        }
    }
}

// ---- replay ----

#[test]
fn replay_regenerates_an_identical_stream_from_externals_only() {
    let jsonl = seeded_run(42);
    let rec = Recording::from_jsonl(&jsonl).unwrap();
    // Same handler construction (same seed) => derived events regenerate
    // identically and byte-compare clean.
    replay_verify(&rec, vec![Box::new(Deriver::new(42))]).unwrap();
}

#[test]
fn replay_detects_divergence_from_a_different_seed() {
    let jsonl = seeded_run(42);
    let rec = Recording::from_jsonl(&jsonl).unwrap();
    let err = replay_verify(&rec, vec![Box::new(Deriver::new(43))]).unwrap_err();
    assert!(matches!(err, BusError::ReplayDivergence { .. }));
}

#[test]
fn replay_detects_a_tampered_payload() {
    let jsonl = seeded_run(42);
    let tampered = jsonl.replacen("\"n\":0", "\"n\":99", 1);
    assert_ne!(jsonl, tampered);
    let rec = Recording::from_jsonl(&tampered).unwrap();
    let err = replay_verify(&rec, vec![Box::new(Deriver::new(42))]).unwrap_err();
    match err {
        BusError::ReplayDivergence { seq, .. } => assert_eq!(seq, 1), // first derived event
        other => panic!("expected divergence, got {other:?}"),
    }
}

#[test]
fn replay_detects_a_truncated_recording() {
    let jsonl = seeded_run(42);
    let full = Recording::from_jsonl(&jsonl).unwrap();
    let n = full.events().len();
    // Drop the final (derived) event: the replayed handlers still produce it,
    // so the recording is incomplete -> divergence.
    let truncated_jsonl: String = jsonl.lines().take(n - 1).fold(String::new(), |mut s, l| {
        s.push_str(l);
        s.push('\n');
        s
    });
    let truncated = Recording::from_jsonl(&truncated_jsonl).unwrap();
    let err = replay_verify(&truncated, vec![Box::new(Deriver::new(42))]).unwrap_err();
    assert!(matches!(err, BusError::ReplayDivergence { .. }));
}

#[test]
fn replay_rejects_a_recording_with_backwards_timestamps() {
    // Corrupt recording: stamps must be non-decreasing (sim clock is
    // monotone). Replay must fail loudly, not reproduce garbage.
    let clock = Arc::new(SimClock::new(ts(2_000)));
    let mut bus = EventBus::new(clock);
    bus.publish_external(raw("a", json!({})));
    bus.publish_external(raw("b", json!({})));
    bus.run_until_idle().unwrap();
    let mut jsonl = bus.recording().to_jsonl().unwrap();
    // Rewind the second event's stamp below the first.
    jsonl = jsonl.replacen("1970-01-01T00:00:02.000Z", "1970-01-01T00:00:01.000Z", 2);
    let first_back = jsonl.replacen("1970-01-01T00:00:01.000Z", "1970-01-01T00:00:02.000Z", 1);
    let rec = Recording::from_jsonl(&first_back).unwrap();
    assert!(replay_verify(&rec, vec![]).is_err());
}

#[test]
fn an_erroring_handlers_pending_publishes_are_discarded() {
    // Fail-closed semantics, pinned: if a handler errors, anything it
    // published while processing that event is dropped with it.
    struct PublishThenFail;
    impl Handler for PublishThenFail {
        fn id(&self) -> &str {
            "publish-then-fail"
        }
        fn on_event(&mut self, ev: &BusEvent, out: &mut Outbox) -> Result<(), BusError> {
            if let EventPayload::Raw { kind, .. } = &ev.payload {
                if kind == "poison" {
                    out.publish(raw("should-never-dispatch", json!({})));
                    return Err(BusError::handler("publish-then-fail", "boom"));
                }
            }
            Ok(())
        }
    }
    let clock = Arc::new(SimClock::new(ts(0)));
    let mut bus = EventBus::new(clock);
    bus.subscribe(Box::new(PublishThenFail)).unwrap();
    bus.publish_external(raw("poison", json!({})));
    assert!(bus.run_until_idle().is_err());
    // Only the poison event itself was dispatched/recorded.
    assert_eq!(bus.recording().events().len(), 1);
    // A subsequent run does not resurrect the discarded publish.
    bus.run_until_idle().unwrap();
    assert_eq!(bus.recording().events().len(), 1);
}

#[test]
fn replay_of_an_empty_recording_is_ok() {
    let rec = Recording::from_jsonl("").unwrap();
    replay_verify(&rec, vec![Box::new(Deriver::new(42))]).unwrap();
}

// ---- PerpTick variant (perp-strategies design §2.1) ----
//
// PerpTick is an ADDITIVE EventPayload variant; these pin that it rides the
// recorder/replay path byte-identically. The replay byte-compare is the whole
// determinism contract, and the embedded FundingObservation carries a
// `Decimal` (serialized as a STRING) — the field most at risk of byte drift.

mod perp_tick {
    use super::*;
    use fortuna_core::market::{MarketId, VenueId};
    use fortuna_core::perp::{FundingObservation, PerpMarks, PerpPrice};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn perp_tick() -> EventPayload {
        EventPayload::PerpTick {
            venue: VenueId::new("kinetics").unwrap(),
            market: MarketId::new("KXBTCPERP").unwrap(),
            marks: PerpMarks {
                venue_settlement: PerpPrice::new(626_010_000),
                conservative: Some(PerpPrice::new(626_005_000)),
            },
            funding: FundingObservation {
                estimate: Decimal::from_str("-0.00012500").unwrap(),
                next_funding_time: ts(1_718_294_400_000),
                reference_price: PerpPrice::new(626_000_000),
                obs_at: ts(1_718_290_800_000),
            },
        }
    }

    #[test]
    fn perp_tick_payload_serde_round_trips_byte_stable() {
        let p = perp_tick();
        let once = serde_json::to_string(&p).unwrap();
        let back: EventPayload = serde_json::from_str(&once).unwrap();
        let twice = serde_json::to_string(&back).unwrap();
        assert_eq!(once, twice, "PerpTick payload JSON is not byte-stable");
        assert_eq!(p, back, "PerpTick did not survive the round trip");
        // The variant tag follows the enum's snake_case contract.
        assert!(
            once.contains("\"type\":\"perp_tick\""),
            "unexpected tag: {once}"
        );
    }

    #[test]
    fn perp_tick_rides_the_recording_jsonl_byte_identically() {
        // A PerpTick dispatched as an external event must survive
        // to_jsonl -> from_jsonl -> to_jsonl unchanged (the replay record's
        // serialization-stability property, now exercised for the Decimal-
        // bearing funding field).
        let clock = Arc::new(SimClock::new(ts(1_718_290_800_000)));
        let mut bus = EventBus::new(clock);
        bus.publish_external(perp_tick());
        bus.run_until_idle().unwrap();

        let jsonl = bus.recording().to_jsonl().unwrap();
        let back = Recording::from_jsonl(&jsonl).unwrap();
        assert_eq!(back.events(), bus.recording().events());
        assert_eq!(back.to_jsonl().unwrap(), jsonl);
    }

    #[test]
    fn perp_tick_replays_byte_for_byte() {
        // The full determinism contract: a recording carrying a PerpTick
        // external event replays without divergence (no handler needed; the
        // event is re-injected and byte-compared against the recording).
        let clock = Arc::new(SimClock::new(ts(1_718_290_800_000)));
        let mut bus = EventBus::new(clock);
        bus.publish_external(perp_tick());
        bus.publish_external(raw("after", json!({"ok": true})));
        bus.run_until_idle().unwrap();

        let recording = bus.recording().clone();
        replay_verify(&recording, vec![]).unwrap();
    }
}

// ---- to_jsonl_from (A6: incremental segment serialization) ----

#[test]
fn to_jsonl_from_zero_equals_to_jsonl() {
    // to_jsonl_from(0) must be byte-identical to to_jsonl() — it's a
    // strict superset of the original (just a refactored call path).
    let clock = Arc::new(SimClock::new(ts(1_000)));
    let mut bus = EventBus::new(clock);
    bus.publish_external(raw("a", json!({"x": 1})));
    bus.publish_external(raw("b", json!({"x": 2})));
    bus.run_until_idle().unwrap();

    let rec = bus.recording();
    assert_eq!(
        rec.to_jsonl_from(0).unwrap(),
        rec.to_jsonl().unwrap(),
        "to_jsonl_from(0) must be byte-identical to to_jsonl()"
    );
}

#[test]
fn to_jsonl_from_k_returns_suffix_events() {
    // to_jsonl_from(k) serializes only events[k..] — the incremental
    // slice for segment-based persist.
    let clock = Arc::new(SimClock::new(ts(1_000)));
    let mut bus = EventBus::new(clock.clone());
    bus.publish_external(raw("a", json!({"x": 1})));
    bus.publish_external(raw("b", json!({"x": 2})));
    bus.publish_external(raw("c", json!({"x": 3})));
    bus.run_until_idle().unwrap();

    let rec = bus.recording();
    let full = rec.to_jsonl().unwrap();
    let suffix_1 = rec.to_jsonl_from(1).unwrap();

    // suffix_1 is a strict suffix of full: full has 3 lines, suffix_1 has 2.
    let full_lines: Vec<&str> = full.lines().collect();
    let suffix_lines: Vec<&str> = suffix_1.lines().collect();
    assert_eq!(full_lines.len(), 3, "full recording has 3 events");
    assert_eq!(suffix_lines.len(), 2, "from(1) has 2 events (events[1..])");
    assert_eq!(
        suffix_lines,
        &full_lines[1..],
        "from(1) is the last two lines of the full recording"
    );

    // Round-trip: from_jsonl of the suffix reconstructs events[1..].
    let partial = Recording::from_jsonl(&suffix_1).unwrap();
    assert_eq!(
        partial.events(),
        &rec.events()[1..],
        "from_jsonl of the suffix reconstructs events[1..]"
    );
}

#[test]
fn to_jsonl_from_at_len_returns_empty_string() {
    // to_jsonl_from(len) — start == len — produces an empty string (no lines).
    let clock = Arc::new(SimClock::new(ts(1_000)));
    let mut bus = EventBus::new(clock);
    bus.publish_external(raw("e", json!({"n": 0})));
    bus.run_until_idle().unwrap();

    let rec = bus.recording();
    let len = rec.events().len();
    let out = rec.to_jsonl_from(len).unwrap();
    assert!(
        out.is_empty(),
        "to_jsonl_from(len) must return an empty string, got {out:?}"
    );
}

#[test]
fn to_jsonl_from_beyond_len_clamps_to_empty() {
    // to_jsonl_from(> len) must clamp to empty, NEVER panic (out-of-bounds).
    let clock = Arc::new(SimClock::new(ts(1_000)));
    let mut bus = EventBus::new(clock);
    bus.publish_external(raw("e", json!({"n": 0})));
    bus.run_until_idle().unwrap();

    let rec = bus.recording();
    let out = rec.to_jsonl_from(rec.events().len() + 99).unwrap();
    assert!(
        out.is_empty(),
        "to_jsonl_from(> len) must clamp to empty, got {out:?}"
    );
}
