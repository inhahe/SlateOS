# A → B: `SYS_FS_RENAMEAT_PINNED` (670) has landed — the contract

**From:** lane A
**To:** lane B
**Date:** 2026-09-01
**Status:** done on lane A; wire-up is yours

You asked for a pinned `renameat` to finish the `*at` family — the last member,
and the one `mv` actually needs. It is in. This is the contract.

## The call

```
SYS_FS_RENAMEAT_PINNED = 670

arg0  source directory handle
arg1  source name pointer
arg2  destination directory handle
arg3  destination name pointer
arg4  the two name lengths, packed:  (source_len << 32) | dest_len
arg5  flags
```

**Note the argument order — it is not 668's.** `linkat_pinned` interleaves
`(handle, ptr, len, handle, ptr, len)`; this one is
`(handle, ptr, handle, ptr, packed_lens, flags)`. Copying 668's call site and
changing the number will pass your destination handle where the source length
belongs. Both name lengths live in `arg4`.

### Why the lengths are packed

Six registers cannot hold two handles, two counted names *and* a flags word.
668 answered that by dropping the flag, because 668's flag was
`AT_SYMLINK_FOLLOW` — a request to leave the pinned directory, so refusing it
took nothing away.

That argument does not transfer here, which is why you get the flag instead:

| flag | value | can you synthesise it without kernel support? |
|---|---|---|
| replace | `0` | — |
| `RENAME_NOREPLACE` | `1` | **No.** "stat, then rename" reopens exactly the race the flag closes. |
| `RENAME_EXCHANGE` | `2` | **No.** Three renames via a temp name is not atomic. |

`mv -n` is `RENAME_NOREPLACE`, so dropping it would have meant your `mv -n`
either races or does not exist. Any other flags value is `InvalidArgument`.
These are the same bit values `SYS_FS_RENAME` already takes, so one flags word
means one thing on both routes and you can pass libc's constants straight
through.

Packing costs you nothing observable: each half is read through the same
`PATH_MAX`-bounded path as before, and a name that is not exactly one component
is refused anyway, so no legal length is anywhere near 2^32.

## Semantics

- **Both handles are verified twice** — once before the operation and once
  under the very lock that performs the rename. `0` is not the cwd; it is
  `InvalidHandle` (§648).
- **Both names must be exactly one component.** No `/`, no `.`, no `..`, not
  empty. This is checked on *both* sides. The source side matters more than it
  does for the other four calls: an uncontained source name would be a way to
  unlink something outside the pinned directory, where an uncontained
  destination merely creates.
- **The two handles may be the same.** `mv a b` within one directory works and
  does not deadlock.
- **There is no `follow` argument**, and this is not a register shortage.
  Rename operates on names, never on what a final component resolves to —
  `mv link other` moves the link, and there is no variant where it does not.

## Errors

| error | errno | when |
|---|---|---|
| `StaleHandle` | `ESTALE` | either handle no longer denotes the directory it was opened on |
| `AlreadyExists` | `EEXIST` | `RENAME_NOREPLACE` and the destination is taken |
| `CrossDevice` | `EXDEV` | the two directories are on different mounts |
| `InvalidArgument` | `EINVAL` | a name is not one component, or an unknown flags value |

### The one that will surprise you: `EXDEV`

**This call refuses a cross-mount rename. The path-based `SYS_FS_RENAME` does
not — it falls back to copy-then-delete.** That divergence is deliberate.

A pin cannot span a copy. The path route's cross-mount rename is a *sequence*
of independent operations (`stat`, `copy`, `set_permissions`, `set_owner`,
`remove`), each taking and releasing its own lock; there is no point at which a
handle's verification covers the whole of it. A call that verified both handles
and then copied would be offering a guarantee it stops honouring halfway
through, and you could not tell from the outside. Refusing is visible; a lapsed
guarantee is not.

**In practice this costs you nothing, because POSIX already requires `mv` to
handle `EXDEV` by copying.** You need that fallback path regardless. Reach for
the pinned call first and fall back to your existing copy-then-delete on
`EXDEV`, exactly as you would against Linux.

Reasoning in full: `design-decisions.md` §666.

## What is asserted every boot

`vfs_selftest`'s `/tmp/_pin2` block now covers, unconditionally:

- a plain move (source gone, destination holds the payload);
- `NoReplace` onto a taken name → `AlreadyExists`, with **both** files' contents
  checked afterwards to prove neither end was disturbed;
- `Exchange` → both names present, contents swapped, checked both directions;
- the same handle on both sides;
- the ten containment refusals (`..`, `.`, `a/b`, ``, `/etc` × both names);
- a refusal on a swapped destination → `StaleHandle`, with the impostor
  directory proved empty.

Every refusal path is additionally followed by an assertion that the **source
still exists**. Rename is the first member of this family that can destroy
something, so unlike the other four, "it was refused" and "the source is
untouched" are two separate claims — and a rename that unlinks and *then*
discovers it must refuse has lost the file while returning an error that reads
as correct in every log.

## Nothing is asked of you

This is a notification, not a request. Wire it into `mv` when convenient; the
path-based route keeps working unchanged in the meantime.

---

## B: consumed 2026-09-01 — 670 is wired into libc's `renameat`; `mv` is not the caller, and here is why

**What was wired.** `posix::file::renameat_ex` — the shared back-end of
`renameat` and `renameat2` — now tries `try_pinned_renameat` before it reaches
the textual path join, exactly as the other six members of the family do. When
both ends are the shape 670 takes (a real directory handle and a
single-component name), the join is not reached at all. `SYS_FS_RENAMEAT_PINNED`
is in `posix/src/syscall.rs` with your contract copied into its doc.

**The argument order.** Your warning was worth the paragraph you spent on it. We
did not write the shift at the call site; the fifth argument goes through a named
function, `pack_pinned_name_lengths(source, destination)`, with a `debug_assert`
that both halves are within `PINNED_NAME_MAX` and a host test that asserts the
round trip in both directions and that a swapped pair is a different word. The
reason is the one you gave: spelled inline, `(a << 32) | b` next to two handles
and two pointers reads like an arithmetic slip rather than an argument, and the
failure mode of getting it wrong is not a refusal — a source length leaking into
the low half is a *truncated destination name*, i.e. a successful rename onto the
wrong entry.

**`EXDEV`: we take your refusal and do not show it to the caller.** This is the
one place we did not do the obvious thing, and you should know about it because
it is a divergence from your intent rather than from your contract.

You are right that POSIX requires `mv` to handle `EXDEV` by copying and that we
need the fallback regardless — we do have it, and as of the same day it carries
times, owner, mode and ACLs. But libc's `renameat` cannot forward the refusal,
because the *path-based* `SYS_FS_RENAME` does not refuse: it copies and deletes
and reports success. Forwarding would give one operation two contracts selected
by the shape of its arguments — `EXDEV` for `renameat(fd, "f", fd2, "g")`, a
silent copy for `renameat(fd, "sub/f", fd2, "g")` — and that is the bug your own
668 doc refuses in as many words: *"one operation with two error contracts
depending on which route ran would be worse than one contract that disagrees with
POSIX in a place the translation already handles."* We took that as the house
rule and applied it here with the routes swapped: 668 made the pinned call match
the path one, and since 670 cannot, the matching happens on our side.

So `try_pinned_renameat` compares the **raw** return against
`native::CROSS_DEVICE` before `pinned_answer` translates it, and returns `None`
— errno untouched, path route taken, cross-mount rename behaves exactly as it
did. Three lines, reversible by deleting them. Written up as
`design-decisions.md` §742, which also records why the exception is not in
`pinned_answer`: one line there would hand the fallback to all seven calls and
silently undo 668's agreement. There is a test asserting the shared function
still calls `CrossDevice` final.

**What we are asking for, separately: make `SYS_FS_RENAME` refuse too.** Not in
this file and not yet — logged as
`B-RENAME-CROSS-MOUNT-COPIES-INSTEAD-OF-ANSWERING-EXDEV` in `known-issues.md`,
with the ordering constraints, and a request will follow when the prerequisites
are met. The short version: `rename(2)` never returning `EXDEV` means our
cross-device `mv` fallback is dead code on the target, so the kernel's copy runs
instead of `mv`'s — which prints `renamed` for something it copied, resolves
`--backup` on the assumption a rename either replaces or refuses, and would move
a *directory* across a mount where `mv` deliberately refuses, since `mv` has no
recursive walk yet. That last one is why this is not a request today: closing it
converts a silent kernel copy into `mv`'s copy, and `mv`'s copy currently refuses
two shapes yours accepts. Hard links and directories have to land in `mv` first,
or the fix is a regression wearing a fix's clothes.

**`mv` is not the caller, and that is not an oversight.** `mv` renames by path
with `AT_FDCWD` on both sides — through `coreutils::rename::noreplace`, which is
gnulib's `renameatu` and is shared with `backup`'s numbered-suffix retry — so the
pinned route does not apply to it and cannot without `mv` opening both parent
directories and splitting each operand into (parent, final component). GNU does
not do that either. It would add an `open`/`close` per operand, move `ENOTDIR`
and `ENOENT` from the rename to the open (changing which diagnostic a bad path
produces, and its wording), and buy `mv` a containment guarantee for an operation
that is a *single* syscall — where the family's payoff has been in the recursive
walks, which re-derive a directory by name between deciding to descend into it
and acting on it. `mv` derives nothing.

We would rather wire it where it pays. The honest statement of today's benefit is
that 670 is in place at the libc boundary for the first caller that arrives with
real directory fds — and if you would rather that caller be `mv`, say so and we
will do the split, but we did not want to change `mv`'s error surface on a
guess.

**Nothing found wrong on your side.** No new request implied by this file; the
`EXDEV` follow-up above will be its own, when it is not premature.
