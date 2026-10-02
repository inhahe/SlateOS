### TD-OILS-JOB-SWEEP-LINE. A dead job that has been reported leaves osh's job table one operation later than bash's — 2026-07-31 — ✅ RESOLVED 2026-07-31

**Where:** `userspace/oils/src/interp.rs` — `Shell::cleanup_dead_jobs` and its
call sites (`Shell::builtin_jobs`, `Shell::notify_signalled_jobs`), against the
read-eval loop in `Shell::run_source_flow_out`.

**What.** bash sweeps jobs that are both finished and already reported at the
boundary between **top-level commands read from the script**, not between the
commands of one list. osh sweeps only when `jobs` or a signal announcement next
looks at the table. So a reported dead job outlives its bash counterpart by one
line:

```sh
true & sleep 0.3
jobs                    # both: [1]+  Done   true
jobs %1; echo "rc=$?"   # bash: jobs: %1: no such job / rc=1
                        # osh : [1]+  Done   true          / rc=0
```

On one line (`true & sleep 0.3; jobs; jobs %1`) the two agree — bash does not
sweep between `;`-separated commands either. A dead job that was never
*reported* survives the line boundary in both.

Newly visible through `compgen -A job`, which since 2026-07-31 reads the same
table (`Shell::compgen_job_names`); before that only `jobs`/`wait` could see the
extra row, and both sweep on entry, which hid it.

**✅ RESOLVED 2026-07-31.** `run_source_flow_out` now calls
`cleanup_dead_jobs()` once per iteration, immediately after `next_unit` has
actually yielded a unit — that loop *is* bash's read-parse-execute loop, so the
boundary lands exactly where bash's does. Three details had to be measured
rather than assumed:

* **After the unit is obtained, not at the top of the loop.** Reaching the end
  of the input is not a boundary in bash either: `true & sleep 0.2; eval 'jobs';
  jobs %1` still finds `%1`, because the `eval`'s own reader ends without
  sweeping. Sweeping at the top of the loop broke that.
* **Not inside a command substitution.** bash runs one in a child, so nothing it
  does to the table reaches the shell that asked — the same reason
  `notify_signalled_jobs` stays quiet there.
* **A swept row must still answer `wait PID`.** bash's `delete_job` calls
  `bgp_add`, so a reaped pid's status survives the row indefinitely. osh grew
  `Shell::remember_reaped`, called from every path that drops a *background*
  row (`cleanup_dead_jobs`, `notify_signalled_jobs`, `wait -n` and
  `disown_job`), writing the same `reaped_status` map a targeted `wait` already
  used. Without it, sweeping at the line boundary would have turned `jobs; wait
  $p` into `127 / not a child`. `fg` is the one row-dropping path that does
  *not* remember, matching bash: its `delete_job` skips `bgp_add` for a job it
  ran in the foreground, so `set -m; (exit 3) & p=$!; fg %1; wait $p` reports
  `not a child` in both shells.

Granularity is observable and is the *reader's*, not the command's: `jobs; jobs
%1` on one line still finds `%1`; the same two on separate lines do not; an
`eval` or `source` body has boundaries of its own between its lines; a function
body, being one parsed unit, has none inside it. Covered by
`the_sweep_boundary_is_the_readers_unit_not_the_command`,
`a_swept_job_is_still_answerable_by_pid` and the corpus case
`tests/corpus/jobs-sweep-line.sh`.

**Also measured, and deliberately *not* to be copied.** bash's table is left
inconsistent by a bare `wait`: after `sleep & cat /dev/null & wait`, a following
`compgen -A job` still answers `cat` but not `sleep` — the older slot was
cleared and the newer one was not. That is a bash artefact of `js.j_lastj` not
tracking the deletions, not a rule; osh answers nothing, which is what the table
actually holds. The fix does not chase it, and `jobs-sweep-line.sh` steers clear
of it: the case's bare `wait` is the last thing it does.

**Impact (before the fix).** Narrow, and only across a line boundary: a script
that listed a finished job and then named it again on the next line got an
answer from osh where bash reports `no such job`. `compgen-job.sh` still steers
clear of the region on purpose — job *lifetime* is `jobs-sweep-line.sh`'s
subject, not that case's.
