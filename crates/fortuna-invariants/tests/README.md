# PROTECTED: Invariant tests (spec Section 3)

These files encode the seven non-negotiable invariants as executable assertions.
Rules (also in CLAUDE.md): tests may be ADDED; existing assertion logic may NEVER be
weakened, renamed, or removed by the implementing agent. A failing invariant test means
the implementation is wrong. Disputes go to GAPS.md under "Disputed invariant tests"
and await operator review.

Each file starts #[ignore]d with a todo!() body and a doc comment stating the property
to encode. Implementing the test (removing #[ignore], writing the real assertions
against the real components) is part of the owning BUILD_PLAN task. The acceptance
checklist requires ZERO ignored tests in this crate at completion.
