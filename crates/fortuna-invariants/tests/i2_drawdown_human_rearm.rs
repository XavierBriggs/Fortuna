//! I2: drawdown halts with human re-arm
//!
//! Property to encode: for all DST sequences breaching a drawdown threshold, a halt flag is set, no further orders pass gates, and no code path clears the flag without the CLI re-arm action

#[test]
#[ignore = "implement per BUILD_PLAN (T0.7); see tests/README.md"]
fn i2_drawdown_human_rearm() {
    todo!("encode the property above as executable assertions");
}
