### TD-OILS-FORKED-CHILD-WAIT-N-CAPS-AT-TWO. bash's repeated `wait -n` in a forked child succeeds at most twice, whatever the job count — OPEN (bash bug; not emulated) — 2026-08-03

**Where:** `userspace/oils/src/interp.rs` — `Shell::wait_next`, the
`mark_inherited_dead` path.

**Reproduce** (`n` background `sleep`s, then a substitution that asks `n+1`
times):

```sh
for i in 1 2 3; do sleep 30 >/dev/null 2>&1 & done
echo "$( wait -n; echo "a=$?"; wait -n; echo "b=$?"; wait -n; echo "c=$?" )"
```

bash answers `a=0 b=0 c=127` with two jobs and `a=0 b=127` with one — i.e. it
succeeds `min(n, 2)` times regardless of how many rows the child actually
inherited. osh answers 0 for every row it has and 127 thereafter.

**Why not emulated.** There is no model that produces "at most two" from bash's
own data structures; probing n = 1…4 gives 1, 2, 2, 2. It reads as an accounting
slip in bash's `wait_for_any_job` bookkeeping after `mark_all_jobs_as_dead`, and
reproducing a number with no meaning would make osh's own code unexplainable.
The first `wait -n` — the one a script actually writes — agrees.

**Impact.** Only a script that calls `wait -n` in a loop *inside a fork*, over
jobs it inherited rather than started. The corpus case documents the first two
answers and stops there.
