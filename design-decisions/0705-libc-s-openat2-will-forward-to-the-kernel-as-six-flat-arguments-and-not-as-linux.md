## 705. libc's `openat2` will forward to the kernel, as six flat arguments and not as Linux's `open_how` struct — and `tar`'s hand-rolled walk stays regardless

**Date:** 2026-08-29
**Decided by:** Claude (autonomous, lane B)
**Lane:** B

**In short:** `openat2` is a system call that opens a file *and* takes a list of
safety restrictions on how the filename may be resolved — chiefly "do not let
this walk leave the directory I gave you", which is what stops a malicious
archive from writing outside where you unpacked it. Our C library recognises
those restrictions but cannot carry out two of them, so it refuses them outright
rather than pretending. Lane A has now built the missing enforcement in the
kernel and offered a system-call number to reach it. Two things had to be
decided: whether to take the offer, and what the call should *look like*. Yes,
and: six plain arguments rather than Linux's packed structure.

### Decision 1 — take the offer

`posix/src/file.rs::openat2` today refuses `RESOLVE_BENEATH` with `EXDEV` and
`RESOLVE_NO_SYMLINKS` with `EOPNOTSUPP`, because it finishes by calling plain
`openat`, which flattens `dirfd` and `path` into one absolute path and has no
per-component no-follow bit to carry either restriction. That refusal is the
*honest* answer (see §702's amendment — the alternative, silently dropping the
restriction, is what the fail-open bug was), but it is not a *useful* one: every
SlateOS program that needs a contained open has to hand-roll the walk, as `tar`
does.

* **For forwarding:** the enforcement now exists (`Vfs::resolve_beneath`,
  `fs::handle::open_beneath`), it composes with `NO_SYMLINKS`, and it is the
  design's own answer — one enforcement point in the kernel rather than ~200
  lines of resolution logic per utility. Two refusals become two working
  features for the cost of a marshalling function.
* **Against:** a new syscall number is an ABI commitment. Lane A's standing
  position (from the `kcmp` exchange) is to add one when something asks — so
  the objection is really "is there a caller?", and the answer has to be a real
  one, not a hypothetical.
* The caller is libc's own `openat2`, and through it any program that calls it.
  That is not circular: the number turns a permanent refusal into an
  implementation, which is a different thing from adding an entry point nothing
  reaches. **Chosen: forward.**

### Decision 2 — flat arguments, not `open_how`

Lane A offered byte-for-byte `struct open_how` (trivial forward, someone else's
layout forever) or a native shape, and leant native. Taken further than lane A
proposed: **there should be no struct at all.**

`open_how` exists in Linux for one reason — it is *extensible by size*, so a
future field needs no new syscall number. That is a solution to a problem this
kernel solved differently and better: `design.txt` mandates **versioned syscall
tables**, so a future field gets a new number in a new version and the old
number keeps meaning exactly what it always meant. Importing `open_how` would
mean carrying Linux's `size`-negotiation convention *and* our versioning, two
extensibility mechanisms for one call, with the imported one pinning three field
widths we never chose.

The native family already answers this: `SYS_FS_OPEN_MODE` is
`(path_ptr, path_len, flags, mode)` — flat, no struct. So:

```
SYS_FS_OPENAT2(path_ptr, path_len, flags, mode, resolve, dirfd) -> fd
```

keeping the sibling's first four arguments in the same positions, so the forward
reads against `SYS_FS_OPEN_MODE` line for line, and appending the two the Linux
call adds. Six arguments is exactly what `SyscallArgs` and libc's `syscall6`
carry, so nothing is squeezed.

The translation cost this is supposed to incur is close to zero, because
`openat2` **already** does the field-level work: steps 1–6 unpack the struct,
range-check `mode` against `S_IALLUGO`, and reject unknown `resolve` bits before
any open happens. Those checks are Linux-ABI conformance and belong on the libc
side of the boundary whatever the kernel call looks like; the kernel should not
re-learn `S_IALLUGO`.

**One width to settle with lane A, and it is a real difference rather than a
detail:** `SYS_FS_OPEN_MODE` masks the create mode to `0o777`, dropping setuid,
setgid and sticky. Linux's `open_how.mode` admits `0o7777`, and our own
`openat2` validates against `0o7777` — so a forward onto the existing width
would silently drop three bits that the caller was told were accepted. Either
the new call takes `0o7777`, or libc must refuse those three bits explicitly.
Silently dropping them is the one option ruled out here, on §702's amendment's
reasoning: accepting a request and not honouring it is the failure mode this
whole exchange started from.

### Decision 3 — `tar`'s `Dir::locate` stays, and not merely "for now"

Lane A notes `Dir::locate` is retirable. It is not, and §702 already says why in
a sentence that has not weakened: **`tar` is differentially tested against the
real GNU binary on Linux/glibc**, that harness is the only thing that verifies
any of `tar`'s behaviour, and glibc exports no `openat2` wrapper. Replacing the
walk with `openat2` would make the containment path on the *host* build either
untested or a raw `syscall(437, …)`, in exchange for deleting code that is
measured correct on all ten rows of the rule table.

There is a second, smaller reason and it is lane A's own: the marshalling in
`sys_openat_beneath` between the ABI and `fs::handle` (`AT_FDCWD` → cwd,
`dirfd_to_guest_dir`) is reached by no test, and a hostile-archive extraction
would be its first real caller. Lane A offered to close that with a ring-3 test.
The reply asks for it — but as a prerequisite for *libc's forward*, not for
retiring the walk, which is declined on the harness argument alone.

**What would change this:** a second SlateOS caller wanting a contained open. At
that point the shared implementation is the kernel's, and `tar` keeping its own
walk for the host build becomes a `cfg` rather than a policy.
