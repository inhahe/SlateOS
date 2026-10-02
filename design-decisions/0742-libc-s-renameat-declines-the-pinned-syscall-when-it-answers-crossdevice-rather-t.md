## 742. libc's `renameat` declines the pinned syscall when it answers `CrossDevice`, rather than forwarding the refusal — one operation must not have two contracts selected by the shape of its arguments

**Date:** 2026-09-01
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** Lane A delivered `SYS_FS_RENAMEAT_PINNED` (670), which renames a
name inside a directory the caller holds a *handle* to, so the kernel checks it
is still the same directory instead of trusting a path the descriptor once had.
670 refuses a rename whose two directories are on different mounts, where the
older path-based `SYS_FS_RENAME` quietly copies the file and deletes the
original instead. libc treats that refusal as "the fast path does not apply here"
and runs the old route, so a cross-mount rename does the same thing it always
did. The refusal is never shown to the caller.

### The question

670 is wired into `renameat` the way the other six pinned calls are: when both
arguments are the shape it accepts — a real directory fd, and a name with no
slashes in it — the pinned call is issued and the path join is not reached at
all. Everything else takes the old route.

That gate is a property of the *arguments*, not of the operation. `renameat(fd,
"f", fd2, "g")` is pinnable; `renameat(fd, "sub/f", fd2, "g")` is not; both are
the same rename. So far that has cost nothing, because both routes did the same
thing. 670 is the first member of the family where they do not.

- 670 answers a cross-mount rename `CrossDevice` → `EXDEV`, which is what Linux
  and POSIX say.
- `SYS_FS_RENAME` answers it by doing a `stat`, a `copy`, a `set_permissions`,
  a `set_owner` and a `remove` — succeeding, at some expense, and never
  mentioning that it was not a rename.

Forward the refusal, and `renameat` returns `EXDEV` for a single-component name
under a directory fd and silently copies for a name with a slash in it.

### Why declining wins

**Lane A already ruled on exactly this question, in the adjacent syscall.**
`SYS_FS_LINKAT_PINNED` (668) reports a cross-mount hard link as
`InvalidArgument`, *not* `CrossDevice`, and the stated reason is that the
path-based `link` has always answered that way: "one operation with two error
contracts depending on which route ran would be worse than one contract that
disagrees with POSIX in a place the translation already handles." The house
rule, written by the lane that owns the kernel side, is that route-consistency
outranks POSIX fidelity. This is the same situation with the routes swapped —
there the pinned call was made to match the path one, here it cannot be, so the
matching has to happen on this side.

**Declining gives up nothing 670 was offering.** This is what makes it a
different case from the family's general rule that a pinned call which answers
has *answered*. That rule exists because retrying an `ESTALE` or an `EACCES` by
path would redo, unpinned, the very operation the pin had just refused to do
unsafely. Here the path route does not redo the rename — there is no rename to
do — it performs a copy, and a copy is a sequence of independent operations each
taking and releasing its own lock. Lane A's own §666 is explicit that no
verification can span it, which is why 670 refuses rather than offering a
guarantee it stops honouring halfway through. The race in the fallback is the
copy's, and it was there before 670 existed.

**Nothing in the tree can observe the difference today, which is the moment to
choose.** No caller in this tree passes a real directory fd to `renameat`:
`coreutils::rename::noreplace` — the one non-trivial user, shared by `mv`'s
speculative rename and `backup`'s numbered retry — passes `AT_FDCWD` on both
sides, as gnulib's `renameatu` does. So this is a decision about what the
contract *will* be for the first caller that does, taken before there is one to
break.

### What was rejected

**Forwarding `EXDEV`.** The honest answer, and the one every other Unix gives.
Rejected because it would arrive as an intermittent error: code that worked with
`renameat(fd, "logs/old", …)` would start failing when someone `openat`'d the
subdirectory and shortened the name. An error that depends on how a path was
spelled is the hardest kind to attribute, and the caller would have no way to
ask for the old behaviour.

**Forwarding `EXDEV` *and* asking lane A to make `SYS_FS_RENAME` refuse too**, so
that both routes agree on the POSIX answer and the copy-then-delete moves up
into `mv` where GNU keeps it. This is the right end state and it is not rejected
so much as *sequenced*: it is a kernel behaviour change affecting every existing
caller of `rename(2)` on this system, it belongs in one commit that changes both
routes together, and doing half of it here — the half that only fires on
pinnable arguments — would be the two-contracts bug wearing a plan as an excuse.
Logged as `B-RENAME-CROSS-MOUNT-COPIES-INSTEAD-OF-ANSWERING-EXDEV`.

**Putting the exception in `pinned_answer`.** One line, and it would hand the
`CrossDevice` fallback to all seven pinned calls. 668 would then fall back on a
cross-mount link — undoing, silently, the agreement lane A built into 668's
error contract on purpose. The exception belongs to 670 alone, so it lives in
`try_pinned_renameat`, and `a_cross_device_answer_is_final_everywhere_but_the_rename`
asserts that the shared function still calls it final.

### The one thing this costs

A caller that wants EXDEV — that would rather be told than have a copy performed
under it — has no way to ask, on either route. That was already true before 670
and this does not change it; the entry above is where it gets fixed.

### Reversal

Three lines in `try_pinned_renameat`:

```rust
if ret == crate::errno::native::CROSS_DEVICE {
    return None;
}
```

Deleting them forwards the refusal. The raw return is compared *before*
`pinned_answer` translates it, so errno is still untouched on the fallback path
and nothing else has to move.
