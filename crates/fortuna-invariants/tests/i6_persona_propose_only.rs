//! I6 (propose-only) — persona facet.
//!
//! A persona reasons over ingested data and emits a DATA artifact. Sizing,
//! timing, order type, and execution belong to the harness, never to the model
//! surface (spec §3 I6; design §3 row "I6", §15). This pins that guarantee with
//! the SAME mechanism as the existing `ProposalDraft`/`MindOutput` field-set pin
//! in `i6_propose_only_mind.rs` — an exact serialized-key-set assertion — applied
//! to the two persona surfaces: the in-memory `PersonaOutcome` the runner emits,
//! and the `domain_analyses` table it is persisted to. Adding any order/size/price
//! field to EITHER breaks this test. By design that must arrive as a spec change,
//! never a convenient field.
//!
//! ADD-only (CLAUDE.md protected crate): this file introduces NEW assertions and
//! touches no existing test. It is NOT the I6 dependency-direction check
//! (`i6_propose_only_mind.rs::i6_mind_crate_cannot_name_a_venue_or_mutate_state`),
//! which proves a different I6 facet and remains untouched.

use fortuna_cognition::persona_runner::PersonaOutcome;
use fortuna_core::clock::UtcTimestamp;

fn ts() -> UtcTimestamp {
    UtcTimestamp::parse_iso8601("2026-06-13T00:00:00.000Z")
        .expect("a literal ISO8601 timestamp parses")
}

/// A fully-populated outcome so EVERY field is present in the serialized
/// surface (the `Option` scoring fields are `Some`, so the pin sees the maximal
/// key set — adding a new field will surface here regardless of its type).
fn populated_outcome() -> PersonaOutcome {
    PersonaOutcome {
        persona_id: "meteorologist".to_string(),
        persona_version: 1,
        region_key: "weather:KNYC:tmax:2026-06-13".to_string(),
        produced_at: ts(),
        signal_manifest: Vec::new(),
        findings: Some(serde_json::json!({ "thresholds": [] })),
        content_hash: Some(
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        ),
        manifest_hash: Some(
            "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        ),
        cost_cents: 1,
        throttled: false,
        skipped_no_signals: false,
        defects: Vec::new(),
    }
}

#[test]
fn i6_persona_outcome_surface_is_order_free_and_data_only() {
    let surface = serde_json::to_value(populated_outcome()).expect("PersonaOutcome serializes");
    let obj = surface
        .as_object()
        .expect("PersonaOutcome serializes to a JSON object");

    let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();

    // The artifact surface is EXACTLY this data-only field set. Growing it
    // (e.g. adding `contracts`, `max_price_cents`, `side`) is an I6 violation:
    // the persona must not be able to size, time, or direct execution through a
    // field on its output. Such a change must arrive as a spec amendment.
    assert_eq!(
        keys,
        vec![
            "content_hash",
            "cost_cents",
            "defects",
            "findings",
            "manifest_hash",
            "persona_id",
            "persona_version",
            "produced_at",
            "region_key",
            "signal_manifest",
            "skipped_no_signals",
            "throttled",
        ],
        "PersonaOutcome surface changed: I6 pins it to order-free, data-only \
         fields (sizing/timing/order-type/execution belong to the harness)"
    );

    // Defense in depth: no execution-surface field name can appear, however the
    // type evolves. (`cost_cents` is the harness-stamped spend, not a price.)
    for forbidden in [
        "contracts",
        "size",
        "notional_cents",
        "quantity",
        "order_type",
        "time_in_force",
        "max_price_cents",
        "side",
        "price",
        "urgency",
        "market",
    ] {
        assert!(
            !obj.contains_key(forbidden),
            "I6 violation: PersonaOutcome must not carry the execution field `{forbidden}`"
        );
    }
}

#[test]
fn i6_domain_analyses_table_carries_no_order_or_size_column() {
    // The persisted form of the artifact (design §15 pins PersonaOutcome AND the
    // `domain_analyses` type). The migration is the source of truth for the
    // table; an order/size/price column added here would let the persistence
    // path carry execution intent the in-memory pin forbids.
    let migration = include_str!("../../fortuna-ledger/migrations/20260613000001_personas.sql");
    let lower = migration.to_ascii_lowercase();

    let start = lower
        .find("create table domain_analyses")
        .expect("the domain_analyses table exists in the personas migration");
    // Bound the scan to the CREATE TABLE column block (closes at the first `);`;
    // the `CHECK (...)` constraints use `))`/`),`, never `);`).
    let rest = &lower[start..];
    let rel_end = rest.find(");").unwrap_or(rest.len());
    let block = &rest[..rel_end];

    for forbidden in [
        "contracts",
        "size",
        "max_price",
        "notional",
        "quantity",
        "order_type",
        "time_in_force",
        " side ",
        " price ",
    ] {
        assert!(
            !block.contains(forbidden),
            "I6 violation: the domain_analyses table must not carry the execution column `{}`",
            forbidden.trim()
        );
    }
}
