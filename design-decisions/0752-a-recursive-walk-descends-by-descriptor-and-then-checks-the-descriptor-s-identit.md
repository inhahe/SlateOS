## 752. A recursive walk descends by descriptor and then checks the descriptor's identity, rather than waiting for libc's `openat` to stop resolving by path

**Date:** 2026-09-03
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `rm -r` used to delete things by building up path *strings* —
`dir`, then `dir/sub`, then `dir/sub/file` — and every one of those strings is
re-walked from the top by the kernel, so a second program could swap `sub` for
a pointer to somewhere else in the gap and redirect the deletions out of the
tree. The fix is the standard one: keep the directory *open* and delete
"the entry named `f` inside this open directory", a request that has no path in
it to re-point. Doing that needs one call SlateOS does not yet provide honestly
— the one that opens a subdirectory relative to an open directory. Rather than
wait for the kernel, the walk opens the subdirectory the old way and then
**asks the descriptor what it actually opened**, refusing to go on if it is not
the thing that was there a moment ago. That check is cheap, works identically
on Linux and on SlateOS, and needs nothing from another lane.

### The situation

`TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL` was filed on
2026-08-30 and annotated the same day as *blocked on the kernel*: the `*at`
family in `posix/src/file.rs` had the right names but the wrong behaviour,
every member joining the descriptor's remembered path string to the caller's
name and calling the ordinary path-based syscall. The annotation drew the
correct conclusion at the time — that rewriting `rm` would close the race on
the certification target (`x86_64-unknown-linux-gnu`, where these are glibc's
genuinely fd-relative calls) *and nowhere else*, retiring the entry while
shipping the bug.

That is no longer the situation. Lane A pinned the family member by member —
`unlinkat` (662), `fstatat` (663), `getdents` (664), `fchmodat` (665),
`mkdirat`/`symlinkat`/`linkat`/`utimensat` (666–669), `renameat` (670) — each
of which now resolves the *handle*. Reading them one at a time rather than
trusting the summary comment turned up the one member the family's own
"completed" note omits: **`openat` is still textual.**
`posix/src/file.rs:3638` goes straight to `resolve_dirfd_path` and then
`open(full)`. `O_NOFOLLOW` guards only the *final* component of that join, so
an already-descended **ancestor** swap still redirects the open — which is
exactly the residual the original entry described, surviving at depth ≥ 2.

So the walk's four syscalls split: listing, classifying and removing are pinned
and safe; only *descending* is not.

### Option A — make libc's `openat` forward to `SYS_FS_OPENAT2` (rejected)

`SYS_FS_OPENAT2` (661) genuinely pins: `openat2_forward` passes `entry.handle`
as the base, and lane A has since enforced `RESOLVE_BENEATH` in the VFS. Making
`openat` forward to it would fix this for *every* program on SlateOS at once,
not just `rm`, which is the more fundamental fix and would normally win on that
ground alone.

It was rejected on evidence from lane A's own request
(`requests/a-b-openat2-resolve-beneath-is-enforced.md`), which says plainly
that the marshalling in `sys_openat_beneath` between the `AT_FDCWD` → cwd
lookup and `dirfd_to_guest_dir` is **reached by no test**. Routing every
SlateOS program's `openat` — which is to say every program's file opening —
through code that nothing exercises, in order to close a race that needs a
hostile local process, trades a conditional bug for an unconditional one. It is
also not lane B's tree to change the guarantees of.

### Option B — open, then verify what was opened (chosen)

After `openat(fd, name, O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)`, `fstat` the new
descriptor and compare `(st_dev, st_ino)` against the `fstatat` the walk
already took to decide the entry *was* a directory. On mismatch the descriptor
is dropped and the descent refused with `ESTALE` (116) — the same errno the
pinned family already answers for this condition.

What makes this sound is that it does not try to prevent the redirection; it
detects it. An attacker who wants the walk to descend somewhere else has to
make the textual open land on a file whose device and inode match the one the
walk already saw — which is to say, on the file the walk meant. The window
between the `fstatat` and the `fstat` is not closed, but nothing observable
passes through it: no entry is removed on the strength of the pre-check, only
descended into, and the descent is what is being verified.

Its costs, in order of how much they matter:

- **One extra `fstat` per directory entered.** Not per entry — per directory.
  A tree of *n* files costs *n* `fstatat`s either way and gains one `fstat` per
  interior node. That is noise beside the `unlinkat` per file.
- **It is a caller-side workaround for a libc defect,** and so has to be
  remembered when the defect is fixed. Filed to lane A separately; the check
  becomes redundant, not wrong, when `openat` pins — at which point it costs
  one `fstat` per directory and can be deleted at leisure.
- **It cannot be tested by racing.** The tests instead swap the target
  *between* the two lookups deterministically, which is the same code path an
  attacker would win by hitting.

Chosen over doing nothing (which leaves the entry blocked on another lane
indefinitely) and over an `rm`-local copy of `tar`'s private descriptor walk
(see below).

### Where it lives, and why not in `rm.rs`

The layer is `userspace/coreutils/src/dirfd.rs`, shared, not a private helper
in `rm.rs`. `tar` had grown its own descriptor walk to fix
`B-tar-WALKS-THROUGH-A-PRE-EXISTING-SYMLINK-…`, and copying it a second time
would mean two utilities carrying two versions of a *security* invariant, which
is a stronger reason to share code than the interface-agreement reasons already
written into `lib.rs`'s header: two utilities disagreeing about a rendering is
a cosmetic bug, but two disagreeing about whether a descent is verified means
one of them is exploitable and nobody can tell which by reading either file.

The module owns the rule "every step below a walk's starting point is
(open directory, one component)" and nothing else. Tar-specific machinery —
`locate`, symlink-hop budgets, `EXDEV` handling — stays in `tar.rs`.
Converting `tar` onto the shared module is a separate, separately-certified
change; until it lands the duplication is real and is tracked as such.

The walk's *starting point* is still reached by path, because at an operand
there is no descriptor above it to reach it through. That is GNU's position
too, and it is not a gap: the operand is a name the user typed, and re-reading
it means reading what the user asked for.

### Reversing it

Delete the identity comparison in `Dir::open_child` and the module keeps
working — it degrades to exactly what `tar` does today, which is correct on
Linux and textual on SlateOS. Reverse the whole layer by giving `Loc` back a
plain `path` and restoring the `fs::` calls; the `rm-diff.sh` cases are written
against printed output and are unchanged by either direction, which is what
makes them a usable check that the rewrite was a behavioural no-op.
