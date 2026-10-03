## §666 — pinned `renameat` (670) keeps its flags and packs the two name lengths; cross-mount is refused rather than emulated

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** The kernel already had a way to move a file by name
(`SYS_FS_RENAME`). Lane B's `mv` needs a version that takes two *directory
handles* instead — an opened, verified reference to a directory — so that a
rename cannot be tricked into moving the wrong thing if someone swaps a
directory out from under it mid-operation. A syscall gets six registers to pass
arguments in, and this one needs seven things: two handles, two names, each
name's length, and a flags word. Something had to give. This entry records that
the flags word stayed and the two lengths were squeezed into one register
instead — and that the new call flatly refuses to move a file between two disks,
where the older path-based call quietly copies it.

### The register problem, and why 668's answer does not transfer

`SYS_FS_LINKAT_PINNED` (668) hit exactly this wall a day earlier and resolved
it by **dropping the flag**. That was right there and is wrong here, and the
difference is worth stating because the two calls otherwise look identical.

668's flag was `AT_SYMLINK_FOLLOW`: it asks the kernel to link a symlink's
*target* rather than the symlink. Following a symlink is, definitionally, a
request to go somewhere the handle does not point — so refusing the flag costs
a caller nothing that the pin was providing in the first place. A caller who
wants it wants the path route.

670's flags are `RENAME_NOREPLACE` and `RENAME_EXCHANGE`. Neither leaves the
pinned directory. They are atomicity guarantees *inside* it:

| flag | what it buys | can the caller synthesise it? |
|---|---|---|
| `RENAME_NOREPLACE` | fail rather than clobber an existing destination | **No.** "stat, then rename" reopens precisely the window the flag closes. |
| `RENAME_EXCHANGE` | swap two names in one step | **No.** Three renames through a temporary name is not atomic and can be interrupted halfway. |

And the caller is not hypothetical: `mv -n` *is* `RENAME_NOREPLACE`. Dropping
the flag would have meant lane B's `mv -n` either races or does not exist.

So the flag stays, and the register comes from packing the two name lengths
into `arg4` as `(source_len << 32) | dest_len`. This is not a convention
invented for this call — `SYS_FS_READDIR_AT` already packs `(offset << 32) |
count` into `arg2`, and the GPU calls pack `width | (height << 32)`.

**Alternative considered: a pointer to an argument struct.** Rejected. It
replaces one packed register with an extra user-memory read that can fault,
fail, or be raced against by another thread in the calling process — turning a
value the kernel already holds in a register into one more piece of
attacker-controlled memory to validate. Packing has a real cost (the lengths
are `u32`, not `usize`) and that cost is invisible: `read_user_path` caps each
name at `PATH_MAX` and the one-component check rejects anything longer anyway,
so no legal value comes close to 2^32.

### Cross-mount: refused, where the path route copies

`Vfs::rename` falls back to copy-then-delete when the two paths are on
different mounts. `Vfs::rename_at_pinned` returns `CrossDevice` (`EXDEV`)
instead.

This is a deliberate divergence between the two routes, and the reason is that
**a pin cannot span a copy.** The path route's cross-mount rename is a
*sequence* of independent operations — `stat`, `copy`, `set_permissions`,
`set_owner`, `remove` — each taking and releasing its own lock. There is no
point at which a handle's verification covers the whole of it. A call that
verified both handles, then copied, then deleted, would be offering a guarantee
it stops honouring halfway through, and the caller could not tell the
difference from the outside. That is strictly worse than refusing, because a
refusal is visible.

Refusing also buys the property that makes the locking trivial: `Arc::ptr_eq`
on the two mounts' filesystem handles establishes that **one** guard reaches
both directories, so both pins are re-verified under the very lock that
performs the rename, and there is no lock ordering to get wrong because there
are never two locks. Contrast `link_at_pinned`, which has a case where only one
of its two handles can be re-verified in pass 2.

The cost is nil in practice: POSIX already requires `mv` to handle `EXDEV` by
copying, so lane B's fallback path exists whether or not this call refuses.

### `RenameMode` moved to the VFS

The enum was defined in `syscall::handlers`, next to the flag decode. It now
lives in `fs::vfs`, with `handlers` re-exporting it and keeping
`RenameMode::from_flags` as an inherent impl.

The split is: the *three renames* are a filesystem semantic, and both the path
route and the handle route offer them, so one type for both is what stops the
two drifting into offering different sets. The *bit values* are Linux's and are
a statement about the syscall ABI, so the decode stays with the handlers. The
re-export rather than a bare move is so that the existing callers and the boot
self-test in `syscall::linux`, which name it as
`crate::syscall::handlers::RenameMode`, keep working unchanged.

### What the self-test asserts, and the one assertion the other four did not need

`rename_at_pinned` is the first member of this family that can **destroy**
something. `mkdirat`, `symlinkat`, `linkat` and `utimensat` can only create or
stamp, so for them "the call was refused" and "the source is untouched" are the
same sentence. For rename they are two sentences, and the second is the one
that matters: a rename that unlinks the source and *then* discovers it must
refuse has lost the file and returned an error for it, which reads as correct
in every log.

So every refusal path is checked at both ends — the ten refused containment
renames, and the refusal on a swapped destination, are each followed by an
assertion that `src/f` is still there. The `NoReplace`-onto-a-taken-name case
checks both files' *contents*, not just their existence, for the same reason.
