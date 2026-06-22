# CLAUDE.md — FORTUNA repo rules

> Purpose: tell any agent (human or AI) the repo rules and where truth lives.
> Holds: the closed-set rule, the same-change rule, the no-new-top-level-docs rule, the scratch rule, the exact pre-done commands, and pointers to the owning documents.
> Excludes: knowledge owned by other docs. This file points; it does not restate invariants, architecture, or standards.

## Where truth lives

One fact, one document. Do not duplicate a fact across files; link to its owner.

- What must be true: [CONSTITUTION.md](../CONSTITUTION.md) (invariants, each mapped to a test).
- What it is for: [NORTH_STAR.md](../NORTH_STAR.md).
- How it fits together: [ARCHITECTURE.md](../ARCHITECTURE.md).
- How to write code: [STANDARDS.md](../STANDARDS.md).
- What is running now: [STATE.md](../STATE.md) (generated; never hand-edit).
- Why it is this way: [decisions/](../decisions/).
- Design reference (not source of truth): `docs/spec.md`.

## The closed set

The canonical documents are exactly those listed in [canon.manifest](../canon.manifest). Do not create a new top-level `*.md` outside that set. If you believe a new canonical document is needed, add an ADR proposing it and amend the manifest in the same change; CI (`tools/check-canon.sh`) fails on any unlisted top-level doc.

## The same-change rule

Documentation lives next to code and is updated in the same change as the code. If a change alters an invariant, the crate map, a boundary contract, a standard, or a ramp stage, update the owning canonical document in the same commit. A code change that drifts a canonical fact is incomplete.

## Scratch is non-authoritative

`scratch/` holds working output (reviews, drafts, evidence). Nothing in `scratch/` is canonical or load-bearing. Do not cite it as truth and do not promote from it without explicit operator approval.

## The protected crate

`crates/fortuna-invariants/` is additions-only. Never weaken, delete, rename, or modify the assertion logic of an existing test there. If a test seems defective, stop and record it in `GAPS.md` under "Disputed invariant tests"; leave the test untouched.

## Run before claiming done

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/run-dst.sh 2000
scripts/check-protected-invariants.sh
tools/check-canon.sh
```

All must pass, and any new failure mode discovered must be added to the DST corpus, before a task is done.
