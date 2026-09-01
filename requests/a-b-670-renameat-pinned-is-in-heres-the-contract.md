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
