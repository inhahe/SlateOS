## TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL (lane B, 2026-08-30) — FIXED 2026-09-03

**Fixed.** `rm`'s walk is descriptor-relative as of 2026-09-03, through the new
shared `userspace/coreutils/src/dirfd.rs`. The original entry is kept below
unedited, because the *reason* it sat blocked for four days is worth reading
and because the annotation at the end of it was correct when written and is now
wrong — see "What changed" after it.

**In short:** `rm -r dir` walks the tree by building up path *strings* —
`dir`, then `dir/sub`, then `dir/sub/file` — and hands each whole string to the
kernel. Every one of those calls re-walks the path from the top. If another
program on the machine can write inside that tree, it can replace a directory
with a symbolic link (a pointer to somewhere else) in the gap between `rm`
deciding to descend into it and `rm` deleting what is inside it, and the
deletions then land wherever the link points — outside the tree, possibly
anywhere the user can write. GNU's `rm` cannot be tricked this way because it
keeps an open handle to each directory and deletes *relative to that handle*,
so the name it already resolved cannot be re-pointed underneath it.

**Severity: real but conditional.** It needs a second party who can already
write inside the directory being removed, running at the same moment. The
usual shape is a shared or world-writable directory (`/tmp`-like) being cleaned
up by a privileged process. It is not exploitable by a file's *contents* or by
anything `rm`'s own user does.

**Where.** `userspace/coreutils/src/bin/rm.rs`:

- `Rm::remove_tree` lists a directory with `fs::read_dir(as_path(path))` and
  then, for each name, builds `join(path, &name)` and calls
  `fs::symlink_metadata` on the joined string.
- `Rm::remove_nondirectory` → `fs::remove_file(as_path(path))`.
- `Rm::rmdir` → `fs::remove_dir(as_path(path))`.

Each of those is a fresh full-path resolution.

**How to reproduce** (conceptually; not currently in the harness, because it is
a race and would be flaky):

    mkdir -p t/sub && touch t/sub/f victim
    # attacker, in a loop: rm -rf t/sub; ln -s "$PWD" t/sub
    rm -rf t
    # `victim` can be removed, though it was never inside `t`

**Proper fix.** Descend with directory descriptors, as `fts` does under
`FTS_CWDFD` and as Rust's own `fs::remove_dir_all` does on Linux:

- keep an `OwnedFd` for the directory being walked, opened once with
  `O_DIRECTORY | O_NOFOLLOW`;
- list it with `fdopendir`, and for each child use `fstatat(fd, name, …,
  AT_SYMLINK_NOFOLLOW)`, `unlinkat(fd, name, 0)` and `unlinkat(fd, name,
  AT_REMOVEDIR)`;
- open a child directory with `openat(fd, name, O_DIRECTORY | O_NOFOLLOW)`, so
  a name that became a symlink between the `fstatat` and the `openat` fails
  with `ELOOP` instead of being followed.

The path strings stay exactly as they are — they are still what gets *printed*,
and `scripts/rm-diff.sh` certifies that spelling case by case — but they stop
being what gets *resolved*. That separation is the whole change: a `walk`
carrying `(dirfd, path_for_messages)` instead of `path` alone.

**Why it is debt and not a regression.** The version this replaced called
`fs::remove_dir_all`, which *is* descriptor-relative on Linux, so this
particular race is new. It is logged rather than fixed immediately because the
same rewrite removed two *unconditional* data-loss defects — `rm -rf .` emptied
the current directory and `rm -rf /` was not refused — and a conditional race
that needs a hostile local process is a strictly better position than those.
The reasoning is recorded in full in `design-decisions.md` §723, Decision 1.

**~~Blocked on nothing.~~ Corrected 2026-08-30: blocked on the kernel.** The
original claim here was that `posix` already has `openat`, `unlinkat` and
`fstatat`, so this was work in `rm.rs` alone. It has the names, but not the
behaviour. All three, and `getdents64` with them, are **textual wrappers**:
each calls `resolve_dirfd_path`, which looks up the *stored path string* of
`dirfd` and concatenates the child name onto it, then calls the ordinary
path-based `open`/`lstat`/`unlink`/`rmdir`. `posix/src/file.rs` says why in as
many words — *"Our kernel file handles are path-based"* — so a descriptor does
not name a directory *object* that a rename or a symlink swap cannot move.

The consequence for this entry is specific and worth being blunt about: doing
the `openat`/`unlinkat` rewrite in `rm.rs` would close the race **on the
certification target and nowhere else**. `scripts/rm-diff.sh` builds for
`x86_64-unknown-linux-gnu`, where those are glibc's genuinely fd-relative
calls, so the fix would test green — while the binary that actually ships on
SlateOS re-resolved every path from the root exactly as it does today. A fix
that is real only where it is measured is worse than no fix, because it retires
the entry.

See **`B-POSIX-THE-AT-FAMILY-IS-TEXTUAL`** below, which is the general defect;
this `rm` race is one symptom of it. Unblocking needs kernel-side fd-relative
resolution, which is lane A's tree — filed as
`requests/b-a-the-at-family-resolves-by-path-so-no-toctou-fix-is-possible.md`.

### What changed (2026-09-03)

Lane A answered that request, member by member: `unlinkat` (662), `fstatat`
(663), `getdents` (664), `fchmodat` (665), `mkdirat`/`symlinkat`/`linkat`/
`utimensat` (666–669) and `renameat` (670) all resolve the *handle* now. So of
the four syscalls this walk makes, three — listing, classifying, removing — are
pinned on SlateOS and were already fd-relative on the certification target.

**One was not, and the family's own summary comment does not say so:**
`openat`. `posix/src/file.rs:3638` still joins the descriptor's remembered path
to the child name and calls `open` on the string. `O_NOFOLLOW` guards only the
*final* component of that join, so a swap of a component the walk had already
descended through is still followed — the residual attack, at depth ≥ 2.

The walk closes that itself rather than waiting: after opening a child
directory it `fstat`s the descriptor and compares `(st_dev, st_ino)` with the
`fstatat` that decided the child *was* a directory, refusing the descent with
`ESTALE` on a mismatch. Redirecting the open past that check would require
landing on the same inode, i.e. on the file the walk meant. It is identical on
both targets, needs nothing from lane A, and is deletable when `openat` pins.
The reasoning, including why forwarding libc's `openat` to `SYS_FS_OPENAT2` was
rejected, is `design-decisions.md` §752; the gap is filed as
`requests/b-a-openat-is-the-one-at-call-left-unpinned.md`.

So the correction above stands as a *rule* — a fix real only where it is
measured is worse than no fix — and the fix that landed obeys it. It is not
"green on Linux, textual on SlateOS": on SlateOS three of the four calls are
pinned in the kernel and the fourth is verified by the caller.

### Where it is now

`userspace/coreutils/src/dirfd.rs` — `Dir` (an owned directory descriptor;
`open_root`, `open_child`, `stat`, `unlink`, `rmdir`, `writable`, `names`) and
`Stat` (a `kind` plus the `(dev, ino)` identity). `rm.rs` reaches every entry
below an operand through `Loc { dir, name, path }`, where `path` is only ever
*printed*. The operand itself is still opened by path, because at the top there
is no descriptor above it — GNU's position too.

Two follow-ups, tracked separately:
`TD-B-TAR-AND-RM-CARRY-TWO-DESCRIPTOR-WALKS` (convert `tar.rs` onto the shared
module, deleting its private copy) and
`TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS` (`rm`'s and `mv`'s). **Both
are now FIXED, 2026-09-03.** The `Loc` described above no longer lives in
`rm.rs` either: the walk moved to `userspace/coreutils/src/remove.rs`, which
`mv` calls too, so `mv`'s path-resolved removal inherited this module's
guarantee rather than needing its own copy of it.
