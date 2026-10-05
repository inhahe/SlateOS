### TD-OILS-COPROC-VARS-SURVIVE-REAPING. bash unsets `NAME`/`NAME_PID` and closes the endpoints when a coproc is reaped; osh keeps them forever — ✅ RESOLVED — 2026-08-01

**Where:** `userspace/oils/src/interp.rs` — `exec_coproc` publishes the array
and the pid variable, and nothing ever takes them away. The endpoints live on
in `coproc_read_fds` / `open_write_fds` too.

**Reproduce:**

```sh
coproc P { read -r x; echo "P<$x>"; }
echo pp >&"${P[1]}"; read -r l <&"${P[0]}"
wait "$P_PID"
echo "after: P_PID=[$P_PID] P=[${P[*]}] set=${P+y}"
```

bash prints `after: P_PID=[] P=[] set=` — the variables are gone, and so are
the two descriptors. osh prints `after: P_PID=[900000] P=[10 11] set=y`.

It is not the `wait` that does it: bash unsets them at whatever reap point
comes first, so a plain `sleep 1; true` after the body has exited is enough.
Only the *tracked* coproc is cleaned up this way — one that a later `coproc`
displaced (see TD-OILS-COPROC-SECOND-IS-NOT-WARNED) keeps its variables for
good, which is exactly why the warning exists.

**Impact.** A script that tests `[[ -v P ]]` or `[ -n "$P_PID" ]` to ask
"is the coproc still there?" gets the wrong answer under osh, and the pipe fds
leak for the life of the shell — an `exec {v}<&"${P[0]}"` after the reap
succeeds under osh and fails under bash (`Bad file descriptor`).

**Proper fix.** Clear the tracked coproc's `NAME`, `NAME_PID` and both fd-table
entries when its job is reaped. The natural home is wherever job reaping
already records a status (the `wait` builtin's reap and the table sweep both
route through it), keyed off the same `coproc_tracked` pair the warning uses;
`unset`-through-`put_var` is not enough — `arrays` and the two fd maps need the
same treatment, and closing the write end has to be visible to `>&"${P[1]}"`.
Note the reap point is asynchronous in bash but synchronous in osh, so the
exact moment the variables vanish will differ for a coproc that exits with no
`wait` and no intervening builtin; keep the corpus case away from that window.

**Fixed.** `coproc_tracked` grew from a `(name, pid)` pair into a
`TrackedCoproc { name, pid, read_fd, write_fd }`, so disposal can close the
endpoints the `coproc` actually opened rather than whatever `NAME` says by then
— a script is free to assign over `NAME`, and bash's `sh_coproc` keeps its own
copy for the same reason. `Shell::dispose_reaped_coproc` hangs off `poll_jobs`,
the one place a finished body is reaped, and `forget_tracked_coproc` drops both
fd-table entries and `unbind_var`s `NAME` and `NAME_PID`. A job that a `wait`
already swept out of the table counts as reaped too, so an absent job disposes
just like one whose handle has been taken; `notify_signalled_jobs` therefore no
longer takes its empty-table shortcut while a coproc is still tracked.

A displaced coproc is deliberately left alone — only the coproc the shell is
still tracking is ever disposed of, which is the other half of the
`execute_coproc` warning. Corpus case
`coproc-is-disposed-of-when-it-is-reaped.sh` covers waiting for one, letting one
finish and running anything at all, the descriptors being *closed* and not just
forgotten (the fd numbers are saved by hand first, since disposal takes `NAME`
away), a coproc read before any command boundary — which still works — and a
displaced one keeping everything.

One deliberate deviation came out of this: neither the command that started a
coproc nor the command in which the shell first notices the body is over may
dispose of it, because osh's body is a thread and can finish before the shell
reaches the next command, where bash's forked child cannot. Without that grace
`coproc C { … }` followed by a line naming `${C[1]}` was a coin flip. See
design-decisions.md §97. The same race had made the older corpus cases and two
`coproc_*` unit tests latently flaky; they now keep their bodies alive with a
trailing `read` instead of betting on the window.

**Two follow-ups the same day, both found by the corpus.** The grace was first
written as "the next reap sweep is a no-op", and that was not enough:
`coproc-is-a-job.sh` failed under the load of a full corpus run with an empty
`$C_PID`, because *two* sweeps separate one top-level command from the next
(`cleanup_dead_jobs` at the input-unit boundary and `notify_signalled_jobs` at
the top of the item). Recounting it in commands (`Shell::item_seq` versus
`TrackedCoproc::born_item`) fixed that case but not
`coproc-second-warns-about-the-first.sh`, where the body is released by the
command *after* the `coproc` — `echo d >&"${D[1]}"` — so the birth grace has
already expired by the time `wait "$D_PID"` needs the name. The rule is now
two-phase: `TrackedCoproc::seen_dead_item` records the item at which the body
was first seen finished, and disposal waits for a strictly later one. Both
halves are arguments from the code rather than probabilities — no sweep count,
no timer.
