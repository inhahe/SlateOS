### TD-OILS-DUP-OF-STDOUT-IS-NOT-THE-LIST-SO-FAR. `>out 3>&1` copies the ambient fd 1, not the sink the same list just installed — 2026-08-01 — ✅ **RESOLVED 2026-08-01** for every shape but a *second* std-fd redirect after the dup (below)

**Where:** `userspace/oils/src/interp.rs` — `Shell::alias_write_fd`, which
snapshotted the *live* fd 1 at apply time instead of consulting the
`RedirPlan`'s own `stdout`.

**Reproduce** (`target/dvscratch/t3/pf.sh`, `pg.sh`, `ph.sh`):

```sh
( { echo W >&3; } >o1 3>&1; )   # bash: o1=[W]   osh was: o1=[] and W on the terminal
( { echo W >&3; } >o3 3<&1; )   # same both arrows — it is the sink, not the arrow
( { echo A >&3; echo B; } >o7 3>&1; )   # bash: A\nB — one open file, one offset
```

A redirect list is resolved left to right and each entry sees the fds the
ones before it left, which osh already got right on the *read* side
(`<in 3<&0` copies the file). The write side did not: `alias_write_fd` looked
at the shell's current stdout rather than at what `>o1` staged, so fd 3 ended
up naming the ambient sink. It reached every body that carries its own
redirect list — group, subshell, `if`, `for`, `while`, function call — and
fd 2 (`2>e 3>&2`) as much as fd 1.

**Fixed** by recording the order where it still exists. A `RedirPlan` keeps
one *slot* per standard fd rather than a list entry, so by apply time the slot
holds whatever the list ended on; the new `AliasSink` enum, set by
`Shell::alias_sink` at resolution time, says whether the list had already
pointed the source std fd at a file when this dup was resolved.
`install_extra_fds` then takes the group's already-opened sinks — passed in as
`StagedStdSinks`, holding the very `Arc<File>` that fd 1 / fd 2 will use — and
copies *that handle* rather than re-opening the path, which is what gives the
two names one offset. `exec` needed nothing: it installs its own fd 1 / fd 2
into the persistent table before its `extra_fds` loop runs, so
`alias_write_fd` already saw the right sink.

Covered by `tests/corpus/dup-of-a-std-fd-copies-the-sink-the-list-installed.sh`
and `a_dup_of_a_std_fd_copies_the_sink_the_same_list_installed`.

**What is left.** A *second* std-fd redirect after the dup:

```sh
( { echo W >&3; } >oa 3>&1 >ob; )   # bash: oa=[W] ob=[]   osh: oa=[] ob=[W]
```

bash's fd 3 holds the `oa` description the dup copied, and fd 1 then moves on
to `ob`. osh's plan has one stdout slot, which `>ob` overwrote, so
`AliasSink::Staged` resolves to `ob`. Closing this needs the plan to keep
fd 1 and fd 2 as *ordered entries* rather than slots — the same order-free
simplification already documented at `Shell::resolve_dup_out`'s close-versus-dup
comment, and worth doing as its own change. Named as deliberately absent in
the corpus case's header.

The same slot-not-a-list shape reaches a dup whose *source* is a std fd the
list goes on to rebind (found 2026-08-01 while closing
TD-OILS-DUP-ONTO-A-STD-FD-IGNORES-THE-SOURCE-MODE):

```sh
printf 'A\nB\n' > rw
( { echo W; } 1>&0 <>rw )   # bash: echo: write error   osh: writes W into rw
( { echo W; } 1>&0 0<&- )   # bash: echo: write error   osh: 0: Bad file descriptor
```

Both dups sit *before* the entry that changes fd 0, so bash copies the ambient
fd 0 (read-only, from the script's own stdin) and the write through fd 1
fails. `Shell::exec_with_redirects` resolves the source with
`write_fd_for(n, &plan)` against the **finished** plan, so it sees the `<>`
and the close instead. Fixing it means the same ordered-entries change: fd 0's
write half would have to be snapshotted where the dup sat, as `AliasSink`
already does for fd 1 and fd 2 on the sink side. Named as deliberately absent
in `tests/corpus/a-std-fd-bound-to-a-read-only-source.sh`.
