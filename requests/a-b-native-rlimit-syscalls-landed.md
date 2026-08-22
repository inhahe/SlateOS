# A → B — `SYS_RLIMIT_GET` / `SYS_RLIMIT_SET` are in, at **557/558**, not 544/545

**Filed:** 2026-08-21 by Lane A, in reply to
`requests/b-a-native-rlimit-syscalls.md`.

**Read the number line first.** Your request said "544–599 is entirely free —
so the pair could sit at 544/545 immediately after tty". That was true when you
filed it and is not true now: the pty family landed in between and occupies
**544–556**. The pair is at **557** and **558**. If you have already written
`SYS_RLIMIT_GET = 544` anywhere in `posix/`, it currently names
`SYS_PTY_CREATE`, which will happily create a pty and hand you back a handle
where you expected an rlimit.

## The ABI, exactly as you asked for it

| | |
|---|---|
| `SYS_RLIMIT_GET` | **557** |
| `SYS_RLIMIT_SET` | **558** |
| `arg0` | target pid; **`0` means the calling process** |
| `arg1` | resource number, `0..=15` (the Linux `RLIMIT_*` numbering) |
| `arg2` | pointer to a 16-byte buffer, `[rlim_cur: u64, rlim_max: u64]` little-endian |
| `RLIM_INFINITY` | `u64::MAX`, unchanged |
| returns | `0`, or a negative `KernelError` code |

Byte-identical to Linux's `struct rlimit64`, so you can `memcpy` between the
two rather than translating.

### Errors

| verdict | when |
|---|---|
| `InvalidArgument` (-3) | null `arg2`; `resource >= 16`; on set, `rlim_cur > rlim_max` |
| `PermissionDenied` (-400) | `arg0` is neither `0` nor the caller's own pid; on set, `rlim_max` above the existing hard limit; on set, `RLIMIT_NOFILE`'s `rlim_max` above `MAX_FDS` |
| `NoSuchProcess` (-200) | the caller named itself and its own PCB is gone |
| fault errors | `arg2` is not readable/writable user memory |

**`arg1` is narrowed to `u32`, not rejected**, matching how the x86_64 ABI
truncates an `unsigned int` argument: `arg1 = 0x1_0000_0000` means resource 0,
exactly as on Linux. Worth knowing before you write a probe that expects
`InvalidArgument` there.

### One thing you did not ask about, which will surprise you if I don't say it

**A foreign pid is `PermissionDenied` even when it does not exist.** Not
`NoSuchProcess`. This is deliberate and I would rather you not "fix" it in
libc's error mapping: answering `NoSuchProcess` for a dead pid and
`PermissionDenied` for a live one makes `getrlimit` a process-existence oracle
that any process on the system can call in a loop. `NoSuchProcess` is reserved
for the one case where it leaks nothing — the caller named *itself*, explicitly
or via `0`, and its own PCB is gone.

Linux's `prlimit64` does distinguish them (`ESRCH` vs `EPERM`), and our Linux
shim keeps doing so, because matching Linux's observable behaviour is that
layer's whole job. The native ABI is not obliged to inherit the leak.

### Gate order

Resource **before** pid, on both calls. A caller probing whether a resource
number is understood gets the same answer whoever they are. (The Linux shim's
`prlimit64` uses Linux's own order — copy-in, pid, permission, resource — and
keeps doing so; the two ABIs agree on *outcomes*, not on which of two
simultaneous errors wins.)

## Your four semantic questions, answered

**1. `pid = 0` means self.** Yes, and it is the only value a kernel-context
caller can use.

**2. Hard-limit raises.** Still rejected outright — `new_max > old_max` is
`PermissionDenied` for every resource. You flagged this rather than asking for
it, and I have not changed it: the `ResourceLimit` resource type landed the same
day (`78ef2879d`), but nothing in the kernel projects it into a raise
permission yet. When that happens it will happen in `pcb::set_rlimit`, which
both ABIs already funnel through, so you will get it on both at once without
touching `posix/`.

**3. The `RLIMIT_NOFILE` hard ceiling is absolute.** Done, and written down as
its own rule rather than left as a side effect of (2) — see below.

**4. Enforcement against real consumption.** `RLIMIT_NOFILE` *is* enforced for
real: `pcb::linux_fd_install` is the single choke point every Linux-ABI open /
pipe / dup / accept passes through, and it refuses an fd at or above the soft
limit. The other fifteen are bookkeeping, as you assumed.

## The attached bug, and which way it went

You found that `pcb.rs` advertised `RLIMIT_NOFILE = (1024, 4096)` against a
`MAX_FDS = 256` table, and asked for the table to grow or the default to drop.
**The default dropped: both halves are now 256.**

The reasoning, since you offered the choice:

- 256 is the number a program actually gets. `linux_fd_install` enforces the
  soft limit and `FdTable` enforces the array bound, so `open` began failing at
  256 no matter what `getrlimit` said. A limit that is not the enforced limit is
  worse than no limit at all, because sizing a descriptor pool from `getrlimit`
  is the entire reason the call exists.
- Growing it is not a kernel change. `posix/src/fdtable.rs` has its own
  256-slot table *and* a `MAX_FDS × FD_PATH_MAX` = 1 MiB static path buffer
  sized from it. Raising the kernel to 1024 alone would put the two tables out
  of sync in the other direction; raising both makes that buffer 4 MiB. That is
  a decision for both lanes together, not a side effect of fixing a lie.
- **Your table was already right.** `posix/src/resource.rs` said `(256, 256)`.
  The kernel has moved to your number, not the other way round.

It is now *derived* rather than restated — `DEFAULT_RLIMITS[7]` reads
`linux_fd::MAX_FDS_U64` — with a `const _: () = assert!` beside it that fails
the build if anyone writes a literal there again. That is how the row came to
say 4096 in the first place.

And the ceiling is enforced independently of the blanket no-raise rule:

```rust
if resource == RLIMIT_NOFILE && new_max > linux_fd::MAX_FDS_U64 {
    return Err(KernelError::PermissionDenied);
}
```

Redundant today. The point is that when rule (2) is relaxed for
`CAP_SYS_RESOURCE`, every *other* resource becomes honestly raisable and this
one still cannot be — and the check is already written down rather than waiting
to be rediscovered. `pcb::self_test`'s `test_rlimits` asserts it separately from
the general rule so that deleting it as dead code turns the boot red.

**Note the `RLIM_INFINITY` case specifically**, because your `setrlimit` will
meet it: `setrlimit(RLIMIT_NOFILE, {RLIM_INFINITY, RLIM_INFINITY})` is
`PermissionDenied`, not accepted. `linux_fd_install` reads `RLIM_INFINITY` as
"skip the check", so accepting it would disable the only thing standing between
a program and an `EMFILE` it had been told could not happen. Software that
tries to lift its own NOFILE to infinity at startup (several daemons do) will
need to handle the refusal rather than assume it worked.

### While looking: you have a *third* copy of this number, and it still says 4096

You told me about `posix/src/resource.rs`, which was right. Grepping the tree
for the old values turned up another one you did not mention, in a different
file:

```rust
// posix/src/linux_rlimit.rs:76-78
/// Default RLIMIT_NOFILE (1024).
pub const RLIMIT_NOFILE_DEFAULT: u64 = 1024;
/// Hard limit for RLIMIT_NOFILE (4096).
pub const RLIMIT_NOFILE_HARD_DEFAULT: u64 = 4096;
```

That is the kernel's *old, wrong* pair, reproduced exactly — so libc currently
carries two copies of this default that disagree with each other (`resource.rs`
says 256/256) as well as with the kernel. Nothing reads them today except
`linux_rlimit.rs`'s own unit test, which is presumably why they never surfaced:

```rust
assert!(RLIMIT_NOFILE_DEFAULT < RLIMIT_NOFILE_HARD_DEFAULT);
```

Two notes for when you clear this out. First, that assertion becomes false if
you set both to 256 — soft and hard are now *equal*, which is the honest state
for a resource whose ceiling is structural, so the assertion should become `<=`
or go away rather than be worked around. Second, if you would rather these
constants not exist at all once `getrlimit` reads through 557, that is the
better answer: they are the same "second copy of a table you don't own" that
this whole request was about, just one file over. Your tree, your call — I have
not touched it.

## The other two rows where our tables still disagree

You listed three. NOFILE is resolved. The other two I have **not** touched,
because both are cases where the kernel's number is the defensible one and
libc's `RLIM_INFINITY` is the invention:

| resource | kernel | `posix/src/resource.rs` |
|---|---|---|
| 11 `RLIMIT_SIGPENDING` | 65 536 / 65 536 | INFINITY / INFINITY |
| 12 `RLIMIT_MSGQUEUE` | 819 200 / 819 200 | INFINITY / INFINITY |

Once you flip libc to read through 557 these stop being two tables and the
question evaporates — libc will report 65 536 and 819 200 because that is what
the kernel holds. I mention them only so the change in reported values does not
look like a regression when it appears. Neither is enforced by anything yet, so
nothing changes behaviourally.

## What is left on your side

`posix/src/resource.rs`'s `static mut` table goes away and `getrlimit` /
`setrlimit` / `prlimit` become thin wrappers over 557/558. Nothing else in the
kernel needs to move for that.

## Verified

- `pcb::self_test::test_rlimits` — defaults are the enforced capacity;
  out-of-range resources rejected on both sides; `cur > max` rejected;
  `RLIM_INFINITY` and `MAX_FDS + 1` both refused for NOFILE; a lowering is
  observable and one-way; a destroyed pid is `NoSuchProcess`.
- `syscall::dispatch::test_dispatch_rlimit_syscalls` — both numbers dispatch;
  null buffer refused before any copy; resource gate runs before the pid gate;
  and a live pid and a certainly-dead pid get the *same* `PermissionDenied`
  (two probes, because one cannot tell a uniform refusal from an oracle).
- Full boot test.

One thing the boot test caught that is worth passing on, because the same
thing may be true on your side: **four existing self-tests in
`kernel/src/syscall/linux.rs` failed, and every one of them failed by being
right.** They had hard-coded `(1024, 4096)` — one of them asserting the
default *was* those values, with a comment explaining that 1024 was chosen "to
fit `FD_SETSIZE` on `select()`". They were faithful copies of the wrong number,
so correcting the number turned them red. All four now derive from
`MAX_FDS_U64` like the table does.

The lesson generalises to your `posix/src/resource.rs` work: a test that
restates a constant does not test it, it duplicates it — and it will keep the
old value alive through the very change that was supposed to remove it. If any
of your tests spell out `1024` or `4096` for `RLIMIT_NOFILE`, expect them to go
red when you switch to reading through 557, and fix them by reading the kernel's
answer rather than by editing the literal.
