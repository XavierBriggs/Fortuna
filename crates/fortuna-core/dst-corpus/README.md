# DST regression corpus

One file per seed, extension `.seed`. Lines starting with `#` describe the
failure mode the seed once exposed (REQUIRED — a seed without its story is
useless in six months); exactly one non-comment line holds the u64 seed.

Rules (CLAUDE.md / fortuna skill):
- NEVER delete a regression seed. The corpus replays before every randomized
  run (`scripts/run-dst.sh`), and red here means a regression.
- Every red DST seed gets minimized, fixed, and committed here.

## Reproducing a failure

```
scripts/replay.sh --seed <S>          # verbose trace of the seed
DST_MASTER_SEED=<M> scripts/run-dst.sh <N>   # re-run a whole randomized batch
```

## Minimization procedure (manual, ~minutes)

1. Reproduce: `scripts/replay.sh --seed <S>` and read the trace tail — the
   violated invariant and the last few actions usually localize the bug.
2. Shrink the fault surface: in `tests/dst.rs::random_faults`, temporarily
   zero fault classes (or hardcode `FaultConfig::none(seed)` plus ONE class)
   and re-run the seed until the failure needs exactly one or two classes.
3. Shrink the action stream: temporarily lower the `n_actions` floor/ceiling
   for that run; the failure is usually reproducible with < 10 actions.
4. Write the minimized story in the seed file comment (which faults, which
   action shape, which invariant), revert the temporary edits, commit the
   `.seed` file. The full-fat scenario must still fail before the fix and
   pass after it.

## File template

```
# I-money violated under dup_fill + cancel_timeout_cancelled:
# double-applied fill when a duplicate arrived after a timed-out cancel.
# Found 2026-06-09, fixed in <commit>.
1234567890123456789
```
