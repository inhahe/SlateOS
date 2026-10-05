### TD-OILS-CORPUS-JOBS-LIFETIME-RACE. `tests/corpus/jobs-lifetime.sh` raced on a 0.2 s margin — ✅ RESOLVED 2026-07-30 (flaky test, not a shell bug)

**Where:** `userspace/oils/tests/corpus/jobs-lifetime.sh`, the two lines under
`=== losing a marked job costs both markers, losing an unmarked one neither`.

**Symptom.** A full `scripts/osh-bash-diff.py` run reported
`143 matched, 0 waived, 1 failed` with `X jobs-lifetime`; re-running that one
case gave `1 matched, 0 failed`. Unlike TD-OILS-CORPUS-JOBS-TIMEOUT this was
**not** a timeout — both shells finished and produced full output, differing in
one job's *state*:

```
bash: … [1]   Done                    sleep 0.5\n[1]-  Running                 sleep 0.5 &\n …
osh : … [1]+  Running                 sleep 0.5 &\n[1]-  Running                 sleep 0.5 &\n …
```

**Cause.** The case was the only place in the corpus needing a *timed* gate to
land strictly between two live jobs:

```sh
sleep 0.5 & sleep 0.05 & sleep 0.3; jobs %2; echo "--"; jobs; wait; disown -a
```

For the assertion to hold, `sleep 0.05` must be finished and `sleep 0.5` must
still be running when `jobs` runs. The first half had 0.25 s of margin; the
second had only ~0.2 s (0.5 − 0.3), and the elapsed time before `jobs` is not
0.3 s but `spawn + spawn + 0.3 + spawn`. Three MSYS process spawns on a loaded
machine cost well over 200 ms, so `sleep 0.5` finishes first and reads `Done`.
Which shell tips over is pure luck — bash ran first in the harness and so ate the
extra latency of the cold page cache.

**Fix.** Raised the running job from `sleep 0.5` to `sleep 1.2` on both lines,
giving ~0.9 s of margin on each side instead of 0.2 s. Nothing about what the
case asserts changes (the differ compares osh against bash, not against a golden
file, and the duration only appears inside the echoed command text, identically
for both). Cost: the trailing `wait` pays ~0.7 s per line, so the case went from
18.2 s to 21.1 s for *both* shells — ~10.5 s each, still only half the harness's
20 s per-case budget, unlike `jobs-listing`. The other timed gates in the file
were checked and are all robust: they either wait for every job to finish, or run
`jobs` immediately after the spawn of the job that must still be running, or (as
on line 54) require only that *both* short jobs be done.

**Lesson for new cases.** When a case needs job A running and job B finished at
the same instant, budget for the process spawns between them, not just the sleep
durations — on this platform a spawn is ~100 ms and there are usually several.
