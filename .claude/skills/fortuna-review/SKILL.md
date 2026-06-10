---
name: fortuna-review
description: Adversarial review procedure for FORTUNA - the hostile checklist, evidence rules, severity taxonomy, output template, and phase-gate grading mode. Use whenever reviewing a diff, grading a completed BUILD_PLAN task, or judging a phase exit.
---

# FORTUNA adversarial review procedure

Grounding (why this procedure is shaped this way): self-critique is unreliable on hard
tasks while external executable verifiers are not, so every judgment chains to a command
you ran; LLM verifiers exhibit agreement bias that worsens with single binary verdicts,
so grading is per-criterion; critics hallucinate flaws on sound work, so BLOCK requires
reproduction; rubrics tailored after seeing the work are corrupt, so criteria come only
from spec + task contract + this checklist, all written before the diff existed.

## Procedure

1. Scope: `git diff --stat <base>...HEAD`; list touched crates. If
   crates/fortuna-invariants/ appears: record automatic BLOCK, continue review anyway.
2. Contract: read the BUILD_PLAN task and its cited spec section(s). Write down the
   criteria list BEFORE opening any changed file.
3. Execute: fmt, clippy -D warnings, workspace tests, targeted tests, scripts/run-dst.sh.
   Capture outputs verbatim into the evidence section.
4. Mechanical sweep (grep/Glob; each hit is evidence, judge in context):
   - `unwrap(`, `expect(`, `panic!`, `todo!`, `unimplemented!` in gates/exec/state/venues
   - `SystemTime::now`, `Instant::now`, `Utc::now` outside fortuna-core clock module
   - `f32`/`f64` touching money or prices outside cognition probability fields
   - `HashMap`/`HashSet` iteration feeding the bus, audit ordering, or sizing
     (nondeterminism leak); look for `.iter()` on unordered maps in those paths
   - `#[ignore`, `proptest` case-count reductions, deleted `assert`, loosened
     tolerances anywhere in tests (test-weakening sweep, diff-wide)
   - secrets patterns: `KEY`, `TOKEN`, `SECRET` literals in code/config/fixtures
   - `place(` call sites: every one must take a GatedOrder; any constructor or
     `From`/`Into` for GatedOrder outside fortuna-gates is Critical
5. Adversarial pass (the implementer's blind spots; write a failing test or DST seed
   for any you can land):
   - Boundaries: 0 contracts, 1 cent, exactly-at-cap exposure, TTL expiring on the same
     tick as a fill, p in {0.0, 0.5, 1.0}, empty book, one-sided book
   - Duplication and absence: message delivered twice, never, out of order; fill after
     local cancel; settlement reversed after confirmed
   - Crash points: kill between intent persistence and submission, between submission
     and ack, mid-IntentGroup; verify boot reconciliation and reservation rebuild
   - Fail direction: every error path in gates/exec must fail CLOSED (reject/halt),
     never fall through to acceptance; fee rounding must round against us
   - Spec drift: does the code implement the spec section, or a reasonable-sounding
     neighbor of it? Quote the spec line for anything that diverges.
6. Write the verdict file (template below) to docs/reviews/, return the summary.

## Severity taxonomy

- Critical: invariant violation, money-path defect, fail-open error path, test
  weakening, GatedOrder bypass, nondeterminism in core, secret in repo. Verdict BLOCK.
- Major: spec divergence with citation, missing edge-case handling with reproducing
  test/seed, unverifiable money-path criterion. BLOCK unless operator waives.
- Minor: convention violations, missing docs, non-money-path gaps. ACCEPT-WITH-GAPS;
  must be ledgered in GAPS.md by the implementer.

## Verdict file template

```markdown
# Review: <task-id> — <date>
Base: <commit>  Head: <commit>  Verdict: ACCEPT | ACCEPT-WITH-GAPS | BLOCK
Protected crate touched: yes/no

## Criteria (fixed before reading the diff)
- C1 <criterion> (spec X.Y): PASS | FAIL | UNVERIFIABLE — evidence: <command/output/seed/test path>
- ...

## Findings
- [Critical|Major|Minor] <one-line> — reproduction: <test path | DST seed | spec quote | command output>

## Commands run (verbatim results)
<fmt/clippy/test/dst outputs, trimmed to verdict lines>
```

## Phase-gate grading mode

When invoked as the grader for a phase exit (/goal stop condition or operator request):
the rubric is EXACTLY the phase's EXIT line in BUILD_PLAN.md plus the PROMPT.md
acceptance items it references. Each item is graded independently with executed
evidence (e.g., "DST corpus >= 10,000 seeds zero violations" requires running it at
that N and quoting the summary line). The phase passes only if every item is PASS - no
weighting, no partial credit, no "close enough". An UNVERIFIABLE item fails the gate.
Write the verdict file as phase-<n>-gate-<date>.md.

## Escalation

Disagreements with an invariant test, suspected defective protected tests, or findings
the implementer disputes go to GAPS.md under "Disputed invariant tests" / "Disputed
findings" with both positions stated. The operator adjudicates. The verifier never
negotiates a verdict in-session.
