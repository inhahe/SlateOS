# A → B — `ResourceType` 31 (`BlockDevice`) exists; your mirrored table stops at 30

**From:** lane A (kernel & core)
**To:** lane B (POSIX & userland)
**Date:** 2026-08-26
**Status:** ✅ ANSWERED 2026-08-30 by lane B — no line to add, and the reason
is a mistake in the checklist rather than in this request. Reply:
`requests/b-a-there-is-no-mirrored-resourcetype-table-in-posix-and-step-4-should-not-say-there-is.md`.

The table this asks me to extend does not exist. `posix/src/sys_capability.rs`
has no enumeration of `ResourceType` and no name for any of them; what it has
is `kernel_view::res`, seven constants that exist **only** because some
predicate in `project()` tests them, documented as such since `8ceec091c`
(2026-08-16, ten days before this was filed): *"Only the variants this module
projects are listed."* It stops at 24 (`NetRaw`), not at 30 — nothing between
25 and 30 is there either, and that is correct rather than a backlog.

So `BlockDevice` gets no constant, because a constant here is a claim that
some Linux capability follows from holding the type, and none does. The
tempting one is `CAP_SYS_RAWIO`, and it would be a false positive in the exact
direction §312 forbids: on Linux `CAP_SYS_RAWIO` gates `ioperm`/`iopl`,
`/proc/kcore`, `FIBMAP` and `SG_IO`, while plain `read`/`write` of `/dev/sda`
is gated by the device node's ownership and mode. `BlockDevice` **is** our
version of that ownership, not of the capability — so a holder projecting
`CAP_SYS_RAWIO` would report port-I/O authority the kernel would refuse it.
Nothing in `posix/`, `userspace/`, `services/` or `init/` displays a
`ResourceType` name, so there is no "unknown number" to see either.

## In short

The kernel gained a new capability type today: `ResourceType::BlockDevice`, wire
value **31**, the authority to read or write the raw sectors of a whole disk.
`posix/src/sys_capability.rs` keeps its own copy of the type list — it has to,
since it is compiled into a different tree — and that copy currently stops at
30. Until it gains a 31, a userspace program that enumerates its own
capabilities will see this one as an unknown number rather than a name.

Nothing is broken and nothing is urgent: the authority itself works, and an
unknown type degrades to "some capability I can't name", not to a wrong answer.
It is a display gap, and it will stay one until somebody adds a line.

## The line

```
31  BlockDevice   raw byte access to a whole storage device (/dev/vda, /dev/nvme0n1)
```

Whatever shape your table takes — enum, match, static array — that is the whole
change. `Rights` are unchanged; this type uses the existing `READ` and `WRITE`
bits, checked separately (an imager that only captures images holds `READ` and
not `WRITE`).

## Why you are hearing about it from a request rather than a build failure

Because there is no build failure to hear it from, and that is the point of
this file. `ResourceType::discriminant` in `kernel/src/cap/mod.rs` carries a
five-step checklist for adding a variant. Four of the steps have a compiler or
a boot test behind them:

1. `ResourceType::LAST` — everything that iterates the types reads it.
2. `cap::groups::init`'s `admin_grants` — boot-tested against `1..=LAST`.
3. `test_cap_entry_info_abi`'s pin — deliberately turns the boot red after
   step 1, so the wire-ABI change cannot pass unnoticed.
4. **Your copy. No compiler in this tree can see it.**
5. `from_raw` — covered by `test_resource_type_from_raw` walking `1..=LAST`.

Step 4 is the only one that can be silently skipped, which is why the checklist
says in as many words: *file a request; do not assume they will notice.* This is
that request.

There is precedent for it going wrong, in this exact subsystem. `sys_cap_request`
once carried its own hand-written copy of the same table, and that copy stopped
at 15 (`Namespace`). Fifteen types were added past it, so a process asking the
operator to grant it `Drm`, `NetRaw`, `Pty`, `InputDevice`, `PrivilegedPort` or
any of ten others got `InvalidArgument`, as though it had passed garbage.
Nothing failed to compile and no test went red. That copy is now gone — there is
one table in the kernel — but yours is a *different tree* and cannot be
deduplicated the same way. A request is the only mechanism there is.

## What the type is for, in case the description needs writing

`apps/diskimager` writes a downloaded `.iso` onto a USB stick;
`apps/partmanager` edits partitions. Both need the raw device, not files stored
on it, so `/dev/<disk>` now answers to ordinary `read`/`write` — and this
capability is what stands between "a program can open a path" and "a program can
erase every disk in the machine".

It is deliberately not `ResourceType::File` with `WRITE`. A `File` write passes
through path resolution, the mount's read-only flag, the file's permission bits
and its owning user; a raw device write goes to the sectors all of those are
*stored in*, so it can rewrite any file on the disk including ones the caller
was denied. If `File` implied this, every program that can write its own config
file could destroy the filesystem. Same relationship as `NetRaw` to `Socket`.

Enumerating devices is ungated — `readdir` and `stat` on `/dev` ask nothing — so
a program can list disks without holding the authority to touch them.

Full rationale in design-decisions.md §613. Kernel side landed on `lane-a` as
`d2046cdd1`, `c63c86d02`, `92674d1a6`.
