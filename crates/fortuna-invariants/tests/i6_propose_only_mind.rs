//! I6: propose-only model interface
//!
//! Property to encode: MindOutput contains no executable side effects; no Mind implementation holds a venue handle or any state-mutating tool; sizing fields in proposals are ignored if present (schema rejects them)
//!
//! Implemented in T2.5's owning slot per tests/README.md (stubs are
//! implemented, never weakened, by their owning BUILD_PLAN task):
//! - The model's entire output surface is DATA: beliefs, proposals,
//!   journal. The proposal shape is pinned to the spec 5.9 field set
//!   (market, side, max_price, thesis, belief_ref, urgency) — adding a
//!   sizing field to the type breaks this test.
//! - Smuggled sizing/execution fields are REJECTED by the schema
//!   (deny_unknown_fields), not silently dropped: a model that tries to
//!   size is schema-invalid and its whole output is discarded (5.9).
//! - "No venue handle, no state-mutating tool" is structural: the crate
//!   that holds every Mind implementation cannot even NAME a venue,
//!   executor, or state-book type (dependency-direction assertion).

use serde_json::json;

fn proposal_json() -> serde_json::Value {
    json!({
        "market": "KXHIGHNY-26JUN12-T65",
        "side": "yes",
        "max_price_cents": 60,
        "thesis": "model text is data, never instructions",
        "belief_ref": "b-evt-1",
        "urgency": "passive"
    })
}

fn output_json() -> serde_json::Value {
    json!({
        "beliefs": [],
        "proposals": [proposal_json()],
        "journal": null
    })
}

#[test]
fn i6_sizing_fields_in_proposals_are_schema_rejected() {
    use fortuna_cognition::mind::ProposalDraft;

    // The clean propose-only shape parses.
    assert!(serde_json::from_value::<ProposalDraft>(proposal_json()).is_ok());

    // Every smuggled sizing/execution field is REJECTED — the model
    // cannot size, time, or direct execution through extra fields.
    for (field, value) in [
        ("contracts", json!(100)),
        ("size", json!(25)),
        ("notional_cents", json!(50_000)),
        ("quantity", json!(7)),
        ("order_type", json!("market")),
        ("time_in_force", json!("ioc")),
    ] {
        let mut smuggled = proposal_json();
        smuggled[field] = value;
        assert!(
            serde_json::from_value::<ProposalDraft>(smuggled).is_err(),
            "a proposal smuggling `{field}` must be schema-rejected, not silently accepted"
        );
    }
}

#[test]
fn i6_mind_output_carries_no_executable_side_effects() {
    use fortuna_cognition::mind::{MindOutput, ProposalDraft};

    // The clean output parses.
    assert!(serde_json::from_value::<MindOutput>(output_json()).is_ok());

    // No top-level escape hatches: orders, tool calls, commands are all
    // schema-rejected.
    for (field, value) in [
        ("orders", json!([{"market": "KXA", "contracts": 10}])),
        ("tool_calls", json!([{"name": "submit_order"}])),
        ("commands", json!(["flatten KXA"])),
    ] {
        let mut smuggled = output_json();
        smuggled[field] = value;
        assert!(
            serde_json::from_value::<MindOutput>(smuggled).is_err(),
            "MindOutput smuggling `{field}` must be schema-rejected"
        );
    }

    // The proposal surface is EXACTLY the spec 5.9 propose-only set.
    // Growing this set (e.g. adding `contracts`) is an I6 violation and
    // must arrive as a spec change, not a convenient field.
    let draft: ProposalDraft = serde_json::from_value(proposal_json()).unwrap();
    let surface = serde_json::to_value(&draft).unwrap();
    let mut keys: Vec<&str> = surface
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "belief_ref",
            "market",
            "max_price_cents",
            "side",
            "thesis",
            "urgency"
        ],
        "ProposalDraft surface changed: I6 pins it to the spec 5.9 field set"
    );

    // The output surface is exactly beliefs + proposals + journal +
    // harness-stamped cost. Nothing here can carry an order.
    let output: MindOutput = serde_json::from_value(output_json()).unwrap();
    let surface = serde_json::to_value(&output).unwrap();
    let mut keys: Vec<&str> = surface
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["beliefs", "cost_cents", "journal", "proposals"],
        "MindOutput surface changed: I6 pins it to data-only fields"
    );
}

#[test]
fn i6_mind_crate_cannot_name_a_venue_or_mutate_state() {
    // Structural enforcement: every Mind implementation lives in
    // fortuna-cognition, and fortuna-cognition cannot DEPEND ON the
    // crates that hold venue handles (fortuna-venues), order execution
    // (fortuna-exec), the position/account books (fortuna-state), or the
    // composed runner (fortuna-runner). A Mind that wanted to mutate
    // external state could not even name the types.
    let manifest_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fortuna-cognition/Cargo.toml"
    );
    let manifest_text = std::fs::read_to_string(manifest_path)
        .expect("fortuna-cognition/Cargo.toml must exist (workspace layout)");
    let manifest: toml::Value = manifest_text.parse().expect("valid TOML");

    let deps = manifest
        .get("dependencies")
        .and_then(|d| d.as_table())
        .expect("[dependencies] table");
    for forbidden in [
        "fortuna-venues",
        "fortuna-exec",
        "fortuna-state",
        "fortuna-runner",
    ] {
        assert!(
            !deps.contains_key(forbidden),
            "I6 violation: fortuna-cognition depends on {forbidden}; \
             the mind crate must not be able to name venue/execution/state types"
        );
    }
}
