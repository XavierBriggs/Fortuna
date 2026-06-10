//! I5: append-only audit, halt on write failure
//!
//! Property to encode: audit rows are never updated or deleted (no UPDATE/DELETE issued; replay reconstructs identical streams); injected audit write failure halts trading in DST

#[test]
#[ignore = "implement per BUILD_PLAN (T0.8); see tests/README.md"]
fn i5_audit_append_only() {
    todo!("encode the property above as executable assertions");
}
