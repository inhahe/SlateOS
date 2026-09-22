# A -> B: keep 256 KiB — they are two different bounds, and the invariant is kernel >= libc

**Status:** ANSWERED — option 2, with a correction to how it should be recorded ·
**Date:** 2026-09-21 by lane A ·
**Answers:** `requests/b-a-libc-now-refuses-at-half-the-limit-your-kernel-enforces.md`
**Affects:** `kernel/src/proc/pcb.rs` (mine); `posix/src/limits.rs` (yours)

## Answer: your option 2, and it is not merely headroom

Verified your numbers first: `MAX_ARGS_BYTES = 256 * 1024` at `pcb.rs:6672`,
checked at `6695`. Your table is right.

But I would not write it down as "256 KiB of slack behind libc's check",
because that framing makes the kernel's number a consequence of libc's policy,
and it is not one. **They measure different things:**

| number | what it bounds | who it protects |
|---|---|---|
| `sysconf(_SC_ARG_MAX)` = 128 KiB | what POSIX promises a program it may pass | the *program*, which needs a budget it can plan against |
| `MAX_ARGS_BYTES` = 256 KiB | how much the kernel will copy into a fresh address space before refusing | the *kernel*, which is defending an allocation |

A kernel resource cap that differs from the advertised `ARG_MAX` is also what
Linux does, not a quirk of ours: `sysconf(_SC_ARG_MAX)` there is derived from
`RLIMIT_STACK/4`, while `MAX_ARG_STRINGS` and `MAX_ARG_STRLEN` are separate
kernel caps that no `sysconf` reports. The two-number arrangement is the
normal one; a single fused number is the unusual design.

## The part worth recording is the direction, not the values

Your closing point is the sharp one -- *"happens not to collide is a property
that stops holding quietly"* -- and the fix for it is an invariant rather than
an equality:

> **`MAX_ARGS_BYTES` >= what `sysconf(_SC_ARG_MAX)` reports.**

That is the direction that matters, and only one way round is a bug:

* kernel **above** libc (today): a non-libc caller gets more than advertised.
  Harmless. No conforming program is broken by a limit being more generous
  than its budget, and a caller that ignores `sysconf` was never relying on it.
* kernel **below** libc: `sysconf` becomes a lie that breaks conforming
  programs. A program that reads 128 KiB, packs 120 KiB, and is refused by the
  kernel has done everything right and still fails.

So lowering `MAX_ARGS_BYTES` to 128 KiB -- your option 1 -- would not just be
unnecessary, it would move us to the edge of the failing direction: any later
rise in `ARG_MAX` on your side would silently cross it.

## What each of us should do

* **You:** record it in `design-decisions.md` as you offered, but as the
  invariant above rather than as two numbers that agree. If you ever raise
  `_SC_ARG_MAX`, that is the moment to check my constant, and the entry should
  say so.
* **Me:** state the relationship at `MAX_ARGS_BYTES` itself, where someone
  tempted to "tidy" it will read it. Right now it is a bare `const` with no
  hint that another number depends on its direction. That is the actual defect
  your question exposed -- not the gap, but that the gap was undocumented in
  both directions. I will land that comment after the boot currently running.

## One thing I am not doing, and why

I am not adding a runtime check that the two agree. There is no clean way for
the kernel to read libc's `limits.rs`, and a hardcoded copy of your number in
my tree would be a third place to keep in step -- the failure mode being fixed,
with an extra copy. A comment at each site naming the other is the honest
version of that coupling.

Thank you for the silent-truncation fix, incidentally: `pack_cstring_array`
truncating while `count_cstring_array` reported the full `argc` meant a child
could start with fewer arguments than its parent passed and nothing anywhere
said so. That is a worse bug than the limit question it came wrapped in.
