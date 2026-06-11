# History rewrite record — 2026-06-11 (gate finding F1, Critical)

Reason: two PEM private keys (`.keys/fortuna-demo-v1.txt`,
`.keys/fortuna-key.txt`) were tracked from the B0 commit onward after a
`>>`-append corrupted the `.gitignore` `.keys/**` line. Full incident +
root cause + operator rotation action: GAPS.md "SECURITY INCIDENT
2026-06-11". This repo has never been pushed; exposure was machine-local
git objects only.

Mechanism: `git filter-branch --index-filter 'git rm -r --cached
--ignore-unmatch .keys data .playwright-mcp' --prune-empty -- 7b00ce6^..HEAD`,
executed 2026-06-11 ~08:30Z.

FINALIZATION (refs/original drop + `reflog expire --expire=now --all` +
`gc --prune=now`) is OPERATOR-GATED — it irreversibly destroys the
recovery data that still lets the old objects be reached. Until the
operator runs/approves it, the key blobs remain recoverable inside .git
(and nowhere else).

## Old -> new commit map

Documents dated before 2026-06-11T08:30Z cite the OLD hashes.

| old | new | subject |
|---|---|---|
| 825d144 | 825d144 (unchanged) | fixture recorder + operator decisions |
| b4fd83a | b4fd83a (unchanged) | signal-contract design note |
| 4213f11 | 4213f11 (unchanged) | perps Phase A landed + plan |
| bab1437 | bab1437 (unchanged) | fixtures RECORDED (60 captures) |
| e464780 | e464780 (unchanged) | Phase B CONFIRMED |
| 7b00ce6 | 94d651a | B0 fortuna-recorder (the commit that swept the keys in) |
| eb189cc | 213e41f | B1 spec v0.9 |
| ad89942 | f551d84 | env-leak fix (cargo [env]) |
| 576d826 | (pruned: empty after purge) | untrack data/ |
| 3e0d34f | 935517a | gitignore data/ |
| 0b8670d | 1259388 | gate remediation F1-F7 surface commit |
| fc1d2f3 | b4c46b1 | operator overnight directive + workspace member |

The gate verdict docs/reviews/perps-b0-b1-fixtures-gate-2026-06-11.md
graded base..head = 825d144..3e0d34f in OLD hashes; its content maps onto
825d144..935517a in the rewritten history.
