# Shared Auditor Brief — FORTUNA Phase-A Deep Audit (READ FIRST)

You are a senior staff engineer running ONE area of a READ-ONLY deep audit of **FORTUNA**, a Rust trading system at `/Users/xavierbriggs/fortuna` (branch `feature/paper-on-live-data`, audit the WORKING TREE).

## Step 1 — read your authoritative protocol
- `.claude/skills/deep-codebase-audit/SKILL.md`
- `.claude/skills/deep-codebase-audit/resources/fortuna_trading_system_profile.md`

Follow that protocol and its **P0–P3** severity scale (P0 money-loss/unsafe-exec/security/data-loss; P1 blocks MVP/demo correctness; P2 maintainability/test/delivery risk; P3 cleanup).

## Hard rules
- **READ-ONLY.** No code edits, no migrations, no git mutations, no order placement, no mutating SQL (SELECT only). Your ONLY write is your findings file.
- **Evidence:** every finding cites an exact `path:line` (or `MISSING: <thing> — expected at <where>`). No unsupported claims. Verify against CODE; never trust README / docs / CLAUDE.md.
- **Readiness lens:** tag EVERY finding `BLOCKS` / `SERVES` / `BLOAT-cut` / `PARK` against the target state:
  > `fortuna start paper-demo` = ONE command boots the closed **paper** loop on **live Kalshi data**, with **NO constructible order-mutation path**, all strategies running in paper, every decision flowing `signal → belief → mapping → proposal → gate → paper-fill → settlement → score`, one view showing the chain, data accruing.
- **Ground-truth DB:** the live demo DB is `fortuna_demo` (`psql -d fortuna_demo`). The pointer file `data/runtime/current-demo-db-url` is STALE — ignore it; verify DB claims only against `fortuna_demo`.
- The working tree has **uncommitted parallel-agent changes** — audit what's on disk; flag any major uncommitted divergence.
- The **SESSION EVIDENCE** in your area assignment = **claims to VERIFY or REFUTE** (strong leads from prior investigation, NOT ground truth). Confirm each with a fresh citation or mark it refuted.

## Output — write to the file path given in your assignment, this exact structure:
```
# Area N — <name>
## Summary
(3–5 sentences: is this area demo-paper-ready? biggest risk?)
## Findings
| Severity | Readiness | Finding | Evidence (path:line) | Why it matters | Root cause | Recommended fix | Suggested test |
## Trace / narrative
(with citations)
## Self-adversarial pass
(attack your own top findings: weak citations? wrong severity? what did you MISS? false positives?)
## Open questions for the Lead
```

After writing the file, **RETURN ONLY**: status (`DONE` or `BLOCKED`), the file path, a ≤6-line summary (top 3 findings with severity + readiness), and counts by severity. Do NOT paste the full findings into your reply.
