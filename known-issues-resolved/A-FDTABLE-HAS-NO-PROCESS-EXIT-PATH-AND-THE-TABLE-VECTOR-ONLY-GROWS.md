## `A-FDTABLE-HAS-NO-PROCESS-EXIT-PATH-AND-THE-TABLE-VECTOR-ONLY-GROWS` (lane A, 2026-08-26) — **fixed 2026-08-26**

**In short:** The module that tracks which files each process has open had no
way to represent a process *exiting*. Tables were created on first use and never
removed, and the list of them is capped at 256 — so once 256 different processes
had ever opened a single file, every later open failed permanently, whether or
not any of those processes still existed.

**Where.** `kernel/src/fs/fdtable.rs`. `open` (~line 187) pushes a fresh
`ProcessFdTable` for any pid it has not seen:

```rust
if state.tables.len() >= MAX_PROCESSES {
    return Err(KernelError::ResourceExhausted);
}
state.tables.push(ProcessFdTable { pid, entries: Vec::new(), next_fd: 0, max_fds: DEFAULT_MAX_FDS });
```

`MAX_PROCESSES` is 256 (line 124). The module's public surface was
`init_defaults`, `open`, `close`, `dup`, `list`, `get`, `set_max_fds`,
`list_tables`, `stats`, `self_test` — every one of which either adds a table or
reads one. **Nothing removed a table.** `close` removes a single *entry* and
leaves the table in place, so a process that opened one file and closed it still
occupied one of the 256 slots forever.

**Why this is worse than an ordinary leak.** Process exit is not an edge case in
an fd table — it is the single most frequent event in its life, and the one the
data structure exists to survive. A module that can model open, close and dup
but not exit is not leaking by oversight in a corner; it is missing the main
lifecycle transition. The symptom, moreover, is delayed and unattributable: the
256th distinct pid works fine, the 257th gets `ResourceExhausted`, and nothing
in the error names *exhausted by long-dead processes* as the cause. A reader
would look for a process holding too many descriptors, which is exactly what is
not happening.

**Found by.** Writing `kshell::self_test` rung 94 for the twenty-sixth
option-refusal burn-down batch. The rung needed a descriptor to exist so it
could prove that a refused `close` had not destroyed it, and needed to leave no
fixture behind afterwards — and there was no operation that could do the second
thing. The absence was visible only because something finally asked for it.

**Fixed.** `close_all(pid) -> KernelResult<usize>` removes the process's whole
table, returns how many descriptors were still open, and adds them to
`total_closes` (they *were* closed; an aggregate that disagreed with the
per-process history for no discoverable reason would be its own bug). It uses
`Vec::remove` rather than `swap_remove` so that a process exiting does not
silently reorder the `/proc/fdtable` listing of the ones that did not. A second
exit for the same pid is `NotFound` rather than a silent success — the caller
has lost track of the process either way, and saying so is more useful than
pretending. Reachable from the shell as `fdtable exit <pid>`.

`fdtable::self_test` grew from 8 to 9 tests; test 9 asserts the table
disappears, the surviving tables are untouched, the close tally moves by exactly
the number of descriptors released, a repeat exit errors, and the pid is
genuinely reusable afterwards — `open` re-creates the table and fd numbering
restarts at 0, because `next_fd` went away with it. It is deliberately placed
*after* test 8 so the exact totals that test asserts stay meaningful.

**Not a regression.** True since the module was written.
