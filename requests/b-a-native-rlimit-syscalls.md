# B → A — libc answers `getrlimit` from a private table, and it already disagrees with the kernel's on three rows

**Status:** ✅ **DONE 2026-08-21** in `800a010e3`. The pair is at **557/558**,
*not* 544/545 — the pty family landed in between and took 544–556. The attached
`RLIMIT_NOFILE` bug is fixed too: both halves are now 256, derived from
`linux_fd::MAX_FDS_U64`. Full reply, including a third stale copy of the number
found in `posix/src/linux_rlimit.rs`:
`requests/a-b-native-rlimit-syscalls-landed.md`.

**Filed:** 2026-08-21 by Lane B. **Action needed:** a native
`SYS_RLIMIT_GET` / `SYS_RLIMIT_SET` pair in `kernel/src/syscall/`, keyed on the
same resource numbering `kernel/src/proc/pcb.rs` already uses, so our own libc
can stop keeping a second copy of a table you are authoritative for.

There is also **a bug in your tree that this request uncovered but does not
depend on** — the kernel's default `RLIMIT_NOFILE` contradicts the kernel's own
Linux fd table. It is written up at the end; it is worth fixing whether or not
you build the syscalls.

## In short

Every process has resource limits — ceilings on stack size, open files, how
much scheduling priority it may ask for. The kernel keeps the real ones, per
process, in `Process::rlimits`. Our libc keeps a **second, private copy** in a
`static mut` and never asks the kernel about it. `getrlimit`, `setrlimit` and
`prlimit64` in `posix/src/resource.rs` read and write only that private copy.

This is the same shape of bug you already fixed for `termios`. Your own doc
comment on `sys_tty_get_termios` says it: *"libc previously answered this from a
hardcoded constant of its own."* You built `SYS_TTY_GET_TERMIOS` /
`SYS_TTY_SET_TERMIOS` so that a native-ABI program and a Linux-ABI program
describing the same terminal get the same answer. Rlimits need the same pair
for the same reason, and unlike termios the two copies **have already
diverged.**

## The divergence is not hypothetical — here are the three rows

`known-issues.md`'s entry for this
(`TD-POSIX-RLIMITS-ARE-A-SHADOW-OF-THE-KERNEL'S`, opened by us on 2026-08-16)
used to say the two tables "happen to agree" today and had "drifted apart on
two rows." Neither had been checked. I re-measured both tables line by line
before filing this and corrected the entry: they disagree **now**, on **three**
rows. Every other row of the sixteen matches exactly.

| # | Resource | kernel `DEFAULT_RLIMITS`<br>`kernel/src/proc/pcb.rs:2382` | libc `RLIMITS_INIT`<br>`posix/src/resource.rs:151` |
|---|---|---|---|
| 7 | `RLIMIT_NOFILE` | `(1024, 4096)` | `(256, 256)` — `fdtable::MAX_FDS` |
| 11 | `RLIMIT_SIGPENDING` | `(65_536, 65_536)` | `(INFINITY, INFINITY)` |
| 12 | `RLIMIT_MSGQUEUE` | `(819_200, 819_200)` | `(INFINITY, INFINITY)` |

So a program that asks "how many files may I open?" gets **1024 or 256
depending on which ABI it was compiled for**, on the same machine, in the same
process lifetime. That is not a latent hazard waiting on some future feature to
activate it; it is wrong output available today.

## Why libc can't just fix its own numbers

It could copy your three values across, and the tables would agree until the
next time either side is edited — which is precisely how they got here. Two
hand-maintained copies of one fact will drift again; the fix is to delete one
of them. libc cannot delete its copy because there is nothing to delegate to:

- The kernel's per-process table is reachable **only through the Linux-compat
  translation layer** — `linux.rs`'s `GETRLIMIT` / `SETRLIMIT` / `PRLIMIT64`
  (the handler around `kernel/src/syscall/linux.rs:45180`). That is the path a
  *ported Linux binary* takes.
- Our libc is the **native** ABI. It calls `SYS_CPU_TIMES` (59),
  `SYS_PROCESS_GET_NICE` (531), `SYS_PROCESS_SET_NICE` (532). There is no
  `SYS_RLIMIT_*` for it to call. I grepped `kernel/src/syscall/number.rs` for
  `RLIMIT` before filing: no native number exists under that or any other name.

Routing libc's `getrlimit` through the Linux compat numbers would work
mechanically but is the wrong shape — it makes the native libc a client of the
compatibility layer, which inverts the dependency and would be the only place
in `posix/` that does it.

## Proposed shape

Modelled on `SYS_TTY_GET_TERMIOS` / `SYS_TTY_SET_TERMIOS`, since that pair
solved this exact problem and set the convention for "a native syscall that
moves a small fixed struct across the boundary." A limit is two `u64`s and does
not fit in a single `i64` return, so it travels through a user pointer.

```rust
/// `SYS_RLIMIT_GET` — read one resource limit of one process.
///
/// `arg0`: target pid; `0` means the calling process.
/// `arg1`: resource number, `0..=15` (the `RLIMIT_*_INDEX` numbering).
/// `arg2`: pointer to a 16-byte user buffer, written as
///         `[rlim_cur: u64, rlim_max: u64]` little-endian.
///         `u64::MAX` is `RLIM_INFINITY`.
///
/// Returns 0, or an error.
///   InvalidArgument  arg1 >= NUM_RLIMITS, or arg2 is null
///   NoSuchProcess    arg0 names no live process

/// `SYS_RLIMIT_SET` — install one resource limit on one process.
///
/// Same arg0/arg1.  `arg2` points at a 16-byte `[cur, max]` to install.
///
///   InvalidArgument  bad resource, null pointer, or cur > max
///   NoSuchProcess    as above
///   PermissionDenied raising rlim_max without CAP_SYS_RESOURCE;
///                    or any pid other than self without it
```

Numbering is yours to pick. For what it's worth: the process block runs 520–536
and is contiguous, the tty block takes 537–543, and **544–599 is entirely
free** — so the pair could sit at 544/545 immediately after tty without
disturbing anything. (The highest number in use anywhere is 1064, against
`MAX_SYSCALL_NR = 1100`.)

Two semantics I need, one I'm only flagging, and one I don't care about:

1. **`pid = 0` must mean "self."** Everything Lane B needs is self-directed;
   cross-process is for `prlimit(pid, …)`, which is rarer and can arrive later
   if it's inconvenient. If you'd rather ship self-only first, say so and I
   will make libc return `ESRCH` for a foreign pid rather than lie about it.
2. **Hard-limit raises — flagging only, no answer needed.** We currently
   differ. `pcb::set_rlimit` (`pcb.rs:2470`) rejects `new_max > old_max`
   outright, for everyone, with no privileged path. libc allows the raise for a
   caller holding `CAP_SYS_RESOURCE` (`posix/src/resource.rs:415`), matching
   Linux's `do_prlimit`.

   Your doc explains the blanket rule as "we have no equivalent" of
   `CAP_SYS_RESOURCE`, and that is accurate on your side — I checked, and the
   kernel defines no such constant, with `pcb.rs:994` recording that
   capabilities aren't enforced generally. `posix/src/sys_capability.rs:157`
   *does* define one (`24`), but that's a userspace-side notion the kernel
   never sees, so libc's check is currently self-asserted rather than
   authoritative — which is exactly the disease this request is about, in
   miniature.

   So: no action needed from you now. When the syscalls land I'll drop libc's
   local check and inherit the blanket rule, and we can revisit if and when the
   kernel enforces capabilities. Recording it because it means a privileged
   program that lowers its own hard limit can never restore it — correct Linux
   behaviour for an unprivileged caller, wrong for a privileged one, and worth
   being a deliberate choice rather than an accident of what wasn't built yet.
3. **The `RLIMIT_NOFILE` hard ceiling should be absolute** — see the bug
   below. This one is not a matter of taste: no capability can conjure fd
   table slots that aren't allocated.
4. *Don't* care: whether you enforce any of these limits on real resource
   consumption. libc doesn't enforce them either. Reporting one consistent
   number is the whole ask; enforcement is a separate question per resource.

## What Lane B does once it lands

`limit_store` in `posix/src/resource.rs` goes away and `getrlimit` /
`setrlimit` / `prlimit64` call straight through. No cache: rlimit reads are not
a hot path, and a cache is how we'd reintroduce the same class of bug at
smaller scale. The host-build `thread_local!` half stays as the test double,
exactly as it is now.

While measuring this I also found `prlimit`'s doc header asserting *"Since our
kernel doesn't track per-process resource limits, valid requests delegate to
the global getrlimit/setrlimit; `pid` is otherwise ignored"*
(`posix/src/resource.rs`). That was true when written and stopped being true
when you added `Process::rlimits` — the kernel does keep a real per-process
table; it is just unreachable from the native ABI. I've rewritten the comment
in the same commit as this request, so our file stops asserting something false
about your tree while the request is open. It'll be deleted outright when the
syscalls land.

---

## Separately: the kernel's default `RLIMIT_NOFILE` contradicts the kernel's own fd table

This is in your tree, is independent of the syscalls above, and I did not touch
it. Two findings:

**1. The default is unbackable.** `pcb.rs:2401` sets `RLIMIT_NOFILE` to
`(1024, 4096)`, with the comment *"1024 matches most Linux distros; programs
that select() on bare fd numbers rely on this fitting in FD_SETSIZE."* But
`kernel/src/proc/linux_fd.rs:57` is `pub const MAX_FDS: usize = 256`. A ported
program that calls `getrlimit(RLIMIT_NOFILE)`, believes the 1024, and opens
files in a loop — or sizes an fd-indexed array from the answer — hits `EMFILE`
at 256. The kernel is telling Linux-ABI programs a number its own fd table
cannot honour. Either the table grows to 1024 or the default drops to 256; I
have no stake in which, but they should be the same number, and libc will
inherit whichever you choose once the syscalls exist.

**2. `prlimit64` has no `sysctl_nr_open` equivalent — though the damage is
bounded by an unrelated rule.** Linux's `do_prlimit` rejects *any* `rlim_max`
above `sysctl_nr_open` for `RLIMIT_NOFILE` unconditionally — `CAP_SYS_RESOURCE`
does not lift it, because no privilege can make the fd table bigger than it is.
The handler at `linux.rs:45323` checks `cur > max` and defers the rest to
`pcb::set_rlimit`, and there is no NOFILE ceiling anywhere on that path.

To be precise about the blast radius, since I initially overstated it: this is
*not* exploitable to an arbitrary value, because `pcb::set_rlimit`
(`pcb.rs:2470`) separately rejects `new_max > old_max` for every resource. So
the reachable maximum is the seeded hard limit, `4096` — a process can install
`RLIMIT_NOFILE = (4096, 4096)` on itself and read it back, against a 256-entry
table. Still 16× more than exists, still `EMFILE` for a program that believes
it; just not unbounded. Your test at `linux.rs:55344` asserting
`set_rlimit(pid, 7, 8, 4096)` succeeds is consistent with this — `4096` equals
`old_max`, so it is not a raise.

libc's `setrlimit` does enforce the real ceiling (`posix/src/resource.rs:407`,
`EPERM` above `MAX_FDS`), which is the third way the two ABIs answer the same
question differently.

(That `new_max > old_max` rule is the one flagged in item 2 above — it is why
the kernel is currently *stricter* than Linux about hard-limit raises, with no
privileged path at all.)

If you'd rather I file these two as their own request so they can be tracked
apart from the syscall ask, say the word and I'll split them out.

## Priority

Not urgent in the sense that nothing is crashing. But the "they still agree, so
this is latent" framing that the known-issues entry has carried since
2026-08-16 was simply not true when I checked it, and the NOFILE row is a
number our system reports differently to two callers who are entitled to the
same answer. I've corrected that entry rather than leave the wrong
reassurance in it.

Lane B has no blocked work behind this — I'm not waiting on you. It's queued
for whenever the syscall surface is being touched anyway.
