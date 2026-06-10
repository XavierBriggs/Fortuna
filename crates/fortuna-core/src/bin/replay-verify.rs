//! Structural verifier for recorded event streams (scripts/replay.sh).
//!
//! Checks a JSONL recording for: parseability, dense seq from 0,
//! non-decreasing timestamps, and byte-stable re-serialization. Full semantic
//! replay (regenerating derived events through real handlers) requires the
//! composed system and runs through the DST harness (T0.4) and, for live
//! decisions, audit manifests (T0.8+).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]

use fortuna_core::bus::{EventOrigin, Recording};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.as_slice() {
        [_, path] if path != "--help" && path != "-h" => run(path),
        _ => {
            eprintln!("usage: replay-verify <recording.jsonl>");
            eprintln!("Verifies structural integrity of a recorded event stream.");
            eprintln!("Semantic replay of derived events runs via the DST harness.");
            ExitCode::from(2)
        }
    }
}

fn run(path: &str) -> ExitCode {
    let input = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("replay-verify: cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let recording = match Recording::from_jsonl(&input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("replay-verify: FAIL parse: {e}");
            return ExitCode::from(1);
        }
    };

    let events = recording.events();
    let mut externals = 0usize;
    for (i, ev) in events.iter().enumerate() {
        if ev.seq != i as u64 {
            eprintln!(
                "replay-verify: FAIL seq density: position {i} has seq {} (expected {i})",
                ev.seq
            );
            return ExitCode::from(1);
        }
        if i > 0 && ev.at < events[i - 1].at {
            eprintln!(
                "replay-verify: FAIL timestamp order: seq {} at {} precedes seq {} at {}",
                ev.seq,
                ev.at,
                events[i - 1].seq,
                events[i - 1].at
            );
            return ExitCode::from(1);
        }
        if matches!(ev.origin, EventOrigin::External) {
            externals += 1;
        }
    }

    // Byte-stability: parse(serialize(x)) == x and serialize is a fixed point.
    let reserialized = match recording.to_jsonl() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("replay-verify: FAIL re-serialization: {e}");
            return ExitCode::from(1);
        }
    };
    match Recording::from_jsonl(&reserialized) {
        Ok(back) if back.events() == events => {}
        Ok(_) => {
            eprintln!("replay-verify: FAIL round-trip: re-parsed events differ");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("replay-verify: FAIL round-trip parse: {e}");
            return ExitCode::from(1);
        }
    }

    let span = match (events.first(), events.last()) {
        (Some(f), Some(l)) => format!("{} .. {}", f.at, l.at),
        _ => "(empty)".to_string(),
    };
    println!(
        "replay-verify: OK {} events ({} external, {} derived), span {span}",
        events.len(),
        externals,
        events.len() - externals
    );
    ExitCode::SUCCESS
}
