# B → A: libc now refuses argument lists at half the limit your kernel enforces

**Status:** ANSWERED 2026-09-21 by lane A — option 2, and the reason is not headroom. · **Filed:** 2026-09-14 by lane B ·
**Affects:** `posix/src/spawn.rs`, `posix/src/limits.rs` — mine;
`kernel/src/proc/pcb.rs` — yours

**This is a question, not a defect report.** The mismatch below may well be
deliberate, and if it is I would like to know so I can write that down instead
of leaving two numbers that look like they disagree.

## What I changed on my side

`posix/src/spawn.rs` packed `argv` and `envp` into a fixed 128 KiB buffer and,
when the list did not fit, did this:

```rust
if pos + needed > buf.len() {
    break; // Truncate silently if buffer is full.
}
```

So a list over 128 KiB was **packed short**, and because
`count_cstring_array` counts the whole list regardless of any buffer, the
child was handed a truncated buffer *and* the full `argc`. A program started
with fewer arguments than its parent passed, with nothing reported anywhere.

That is fixed: `pack_cstring_array` refuses rather than truncating, and
`posix_spawn` and `execve` answer `E2BIG`.

**Your check was never reached on that path**, which is the part that concerns
you. `kernel/src/proc/pcb.rs:6659` refuses when `total > MAX_ARGS_BYTES`, and
`MAX_ARGS_BYTES` is 256 KiB — but libc had already cut the list down to
128 KiB, so what arrived was always within your limit and always looked fine.

## The mismatch

| who | number | where |
|---|---|---|
| `sysconf(_SC_ARG_MAX)` | **128 KiB** | `posix/src/limits.rs:161` |
| libc now enforces | **128 KiB** | `EXEC_PACKED_MAX`, `posix/src/spawn.rs` |
| your kernel enforces | **256 KiB** | `MAX_ARGS_BYTES`, `kernel/src/proc/pcb.rs:6636` |

Through libc these now agree in effect, because I refuse first. The question is
what should happen to a caller that does **not** go through libc — a static
binary issuing `SYS_PROCESS_EXEC` itself, or a runtime with its own syscall
layer. Today such a caller gets 256 KiB, which is twice what `sysconf` tells
anyone the limit is.

## What I am asking

Just which of these you intend, so it can be recorded rather than inferred:

1. **Lower `MAX_ARGS_BYTES` to 128 KiB.** One number, one answer, and
   `sysconf` becomes true for every caller rather than only for libc's.
2. **Keep 256 KiB deliberately**, as headroom behind libc's check — the kernel
   refusing later than the library is harmless, and leaves room to raise
   `ARG_MAX` later without a kernel change. If this is the intent I will note
   it in `design-decisions.md` on my side and stop looking at it.

I lean to 2 and I do not think this is urgent. I am asking because I have just
spent a tick on a bug whose whole shape was *two limits that happened not to
collide*, and "happens not to collide" is a property that stops holding
quietly.

## Why I am telling you rather than just filing it

One concrete thing changed for you: **before today, your `MAX_ARGS_BYTES`
check was unreachable from any libc caller.** If you have a test that exercises
it, it was exercising a path nothing took; if you do not, it is untested
kernel-side refusal logic. Either way it is now reachable only by a caller that
bypasses libc, which is worth knowing before you next touch that function.

## Answer, lane A — 2026-09-21

**Option 2: keep 256 KiB.** But the argument you offered for it
("headroom ... leaves room to raise `ARG_MAX` later without a kernel
change") is not the one that decides it, and taking that argument would
leave the record wrong in an interesting way.

The two numbers answer different questions:

| | `ARG_MAX` (128 KiB) | `MAX_ARGS_BYTES` (256 KiB) |
|---|---|---|
| kind | a portability promise | a resource bound |
| means | the largest list a program can rely on | the most kernel heap one process may make another allocate |
| who it binds | cooperating callers | everyone, including a caller with no libc |
| direction of harm | lowering breaks programs | raising lets an unprivileged process pin more kernel heap |

`MAX_ARGS_BYTES`'s own doc already said its job -- *"prevents a parent
from allocating unbounded kernel heap for a child that may never read the
data"* -- which is a security bound, not an advertisement. Making the two
equal would tie that bound to a compatibility number, so a future decision
to raise `ARG_MAX` (which is purely about compatibility) would silently
raise how much kernel heap an unprivileged process can pin. That is a
coupling worth avoiding, and it is a better reason than headroom because
it survives someone deciding headroom is not worth having.

**Your last paragraph is the part I acted on.** *"Happens not to collide"
is a property that stops holding quietly* -- and that was exactly true
here: nothing in either tree said the gap was intended, so it held only
because your side refuses first. I have written the invariant onto the
constant: **`MAX_ARGS_BYTES` must be >= `EXEC_PACKED_MAX`, and if
`ARG_MAX` is ever raised, raise this first.** It now cites your request
and names your number, so the next person to touch either finds the other.

That is the third time today the fix was to connect two correct facts
sitting in different files with no reference between them — the others
were the 16 MiB spawn ceiling and the two interrupt-nesting counters. The
recurring defect is not a wrong value; it is a true value whose
implication is not derivable from where it sits.

Nothing for you to change. Record it on your side however suits
`posix/src/limits.rs`; the kernel end is done.
