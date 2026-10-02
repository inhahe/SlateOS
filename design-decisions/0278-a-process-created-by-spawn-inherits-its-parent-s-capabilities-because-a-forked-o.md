## §278 — A process created by `spawn` inherits its parent's capabilities, because a forked one already does and the two must not disagree

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** there are two ways to start a new program on this system, and they
were handing the new program wildly different amounts of authority. Starting one
with `fork` gave it a copy of everything its parent was allowed to do; starting
one with `spawn` gave it *nothing at all*, so the very first file it tried to
open was refused. That is why `make` could read its makefile and then die
running its own recipe: the shell it started could not open the C library. The
decision is that `spawn` now copies the parent's permissions, exactly as `fork`
does.

### The asymmetry

`fork_create` clones the parent's `cap_table` wholesale — `CapTable`'s own doc
comment says that is what fork needs, and it has always done it.
`spawn_process` instead calls `pcb::create`, which hands back a fresh empty
`CapTable`, and then grants only what `SpawnOptions::capabilities` names.

For the ~150 in-kernel callers in `spawn.rs` that is correct and deliberate:
each names the authority its test subject should have, which is the delegation
model the field's doc describes ("the parent must have these capabilities to
delegate them").

For userspace it was not a policy at all, because **there is no way to express
it.** `SpawnExArgs` has twelve fields — ELF pointer and length, name, fd map,
argv, envp — and not one of them is a capability array. `sys_process_spawn_ex`
therefore builds `SpawnOptions::new(name).parent(caller_pid())` and stops. Every
child spawned by a user process started with an empty capability table, and
`openat`'s `require_cap_type(File, READ)` refused its first file.

### How it presented, which is most of why it survived

The visible failure was three removes from the cause:

```
/bin/sh: error while loading shared libraries: libc.so.6:
         cannot open shared object file: Permission denied
make: /bin/sh: Permission denied
make: *** [/Makefile:2: all] Error 127
```

Three things conspired to point away from the kernel:

1. **The message is glibc's**, and names a shared library, so it reads as a
   loader or a rootfs-permissions problem.
2. **`ld.so` itself loaded fine** — the log shows
   `loaded interpreter '/lib64/ld-linux-x86-64.so.2' at base=…` — because the
   *kernel's* ELF loader reads the interpreter with kernel authority. Only the
   loader's own ring-3 `openat` was refused. So the same file path worked and
   then did not, within one process startup.
3. **`fork` + `execve` was unaffected throughout**, and so was every directly
   kernel-spawned Path-Z test, including one that exercises this exact code:
   `REAL glibc dynamic execution (ring 3: ld.so mapped libc.so.6, …): OK`. A
   mechanism that fails only for grandchildren, and only via one of two process
   creation paths, has almost no surface to be noticed on.

An earlier form of the failure had been logged as a downstream consequence of
lane B's `BUG-POSIX-SPAWN-FILE-ACTIONS-IS-4624-BYTES-IN-AN-80-BYTE-SLOT`. Lane B
fixed that bug on 2026-08-21; the `make` rungs stayed red -- with a *different*
exit code (2, an `EACCES` path, rather than -8, a fault) -- which is what
disproved the attribution.

**Credit where it is due:** lane B had already tracked this to the right line
and filed it as `BUG-SPAWNED-CHILDREN-INHERIT-NO-CAPABILITIES`, deliberately as
a report rather than a patch, on the grounds that a change to the capability
model belongs to whoever owns the capability model. Their write-up names the
`spawn.rs` Step 5 site, the `require_cap_type(File, READ)` gate in `openat`, and
the discriminator table above; it lists the same three options considered here
and recommends option 1 now with option 3 later, which is exactly what was
implemented. It also asked for "a smaller in-kernel test that asserts directly
that a child of `SYS_PROCESS_SPAWN` holds a `File` capability" -- that is
`test_spawn_inherits_parent_capabilities`. This entry reaches lane B's
conclusion independently and adds the argument for why the answer was not in
fact a difficult security call; the diagnosis was theirs.

Lane B also recorded the observation worth carrying forward: this is the
*second* capability-shaped failure in the same self-test, and the two are
distinct bugs with the same symptom shape -- the first was a too-narrow grant
*to the parent* (`METADATA` missing), this one no grant at all *to the child*.
When a Path-Z rung reports something implausible like "file does not exist",
capabilities are the place to look first.

### The decision

`pcb::inherit_caps_from(parent, child)` copies the parent's valid entries into
the child, and `spawn_process` calls it before applying
`options.capabilities` — so explicit grants still layer on top, and in-kernel
callers (`parent: 0`) are untouched.

### Why this is not a widening of authority

The architectural rule is **no ambient authority**: authority must come from
holding an unforgeable token, not from being yourself. Inheritance is not that.
The child receives copies of tokens its parent demonstrably held, and can
receive nothing the parent lacked. It is the same delegation `fork` performs.

The decisive argument is that the restriction bought nothing even before the
change. Any process that can call `spawn` can equally call `fork` + `execve`,
which clones the table in full. So "spawn grants nothing" did not deny an
attacker a single capability — it only broke the caller who used the newer
syscall. A security boundary that is one syscall away from being bypassed is not
a boundary; it is a bug with a rationale attached.

POSIX forces the same answer independently: `posix_spawn` is specified as
equivalent to `fork` + `exec`, and lane B's libc implements it on
`SYS_PROCESS_SPAWN_EX`. A child that cannot open a file is not equivalent to a
forked one.

### Alternatives considered

| Option | Why not |
|---|---|
| Add a capability array to `SpawnExArgs` and require explicit delegation | The principled long-term answer, and it is still wanted (see below) — but it is an ABI change across two lanes, and it *still* needs a default for `posix_spawn`, which by specification must be fork-equivalent. So it would default to inheriting anyway. This decision does not preclude it. |
| Inherit only `ResourceType::File` | Fixes the observed symptom and nothing else. The next spawn that needs a channel or a timer fails the same way, with a symptom equally far from its cause. Fixing the mechanism rather than the reported instance is the habit `FIXED-A-PATH-Z-REAL-MAKE-STAT-OF-MAKEFILE-RETURNS-EACCES` was written to instil, one bug earlier in this very code path. |
| Grant a blanket `File` capability to every process | Ambient authority, straightforwardly. Rejected. |
| Leave it and fix the callers | There is no caller-side fix. The ABI has no field to fix it with. |

### The cost, stated plainly

A spawned child now gets the parent's **entire** table, including any `Process`
capabilities the parent holds over unrelated processes. `fork` has always done
this, so it introduces no case that did not already exist by another route — but
it does mean spawn cannot currently be used to *drop* authority before running
untrusted code, which is a real thing to want.

That is the one genuine gap, and it needs an ABI field to name the subset to
keep. Recorded in `todo.txt`; the honest default until then is "same as fork"
rather than "nothing", because "nothing" is not a safer default, it is an
unusable one, and it drove callers toward `fork` + `execve` — which inherits
everything anyway, with no option to narrow either.

### Generalisable lesson

**When one operation has two implementations, the invariant is that they agree —
and nothing in the type system was ever going to check it.** `fork` and `spawn`
both create a process; they diverged on the single most security-relevant
property of a new process, and the divergence lived for as long as it did
because each path looked correct in isolation. `fork`'s clone is documented on
`CapTable`; `spawn`'s explicit-grant model is documented on
`SpawnOptions::capabilities`. Both docs are true. Neither mentions the other,
and no reader of one is prompted to check the other.

The regression test therefore asserts the *agreement*, not either behaviour on
its own — and its second half asserts the converse (a kernel-spawned process
inherits nothing from PID 0), because "inherit more" and "inherit less" are
independent mistakes and a test that only checks one direction licenses the
other.
