//! I3: runaway detection halts, never throttles
//!
//! Property to encode: exceeding burst or sustained token buckets per venue/market sets a halt (not a delay); duplicate client order ids are rejected exactly-once under duplicate delivery faults

#[test]
#[ignore = "implement per BUILD_PLAN (T0.5); see tests/README.md"]
fn i3_runaway_halt() {
    todo!("encode the property above as executable assertions");
}
