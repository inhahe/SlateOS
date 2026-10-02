## B-POSIX-THE-AT-FAMILY-IS-TEXTUAL, SO NOTHING IN USERSPACE CAN BE TOCTOU-SAFE (lane B, 2026-08-30) — OPEN

**In short:** POSIX gives programs a way to say "open/stat/delete *this* name
inside *that* directory I already have open" — the `openat`, `fstatat`,
`unlinkat` family. The point of them is that the directory is named by an open
handle rather than by a path, so nothing another program does to the path in
the meantime can redirect the operation. We export all of those names, and
none of them work that way: each one looks up the *text* of the path its
directory handle was opened with, glues the child name onto the end, and calls
the ordinary path-based operation. Every call therefore re-walks the whole path
from the root, which is exactly what the family exists to avoid. Any program
that uses them believing otherwise — ours or a ported one — has a security
property it does not actually have.

**Where.** `posix/src/file.rs`:

- `resolve_dirfd_path(dirfd, path, out)` — `get_fd_path(dirfd)` then
  `build_at_path(...)`. This is the whole mechanism.
- `openat` → `open(full)`; `fstatat` → `stat`/`lstat(full)`;
  `unlinkat` → `unlink`/`rmdir(full)`; `renameat`, `faccessat`, `fchmodat`,
  `linkat`, `symlinkat`, `mkdirat`, `readlinkat` and `utimensat` are the same
  shape.
- `posix/src/dirent.rs::getdents64` is the same defect in a different spelling:
  it looks up the fd's stored path and snapshots *that path* with
  `SYS_FS_LIST_DIR`, so even listing a directory you hold open is a fresh path
  resolution.

**The root cause is architectural, not a wrapper bug.** `posix/src/file.rs`
states it plainly, in the comment refusing `O_TMPFILE`:

> Our kernel file handles are path-based, so a nameless inode cannot be
> represented.

A handle is a path, so there is no object for a descriptor to pin. The wrappers
are not doing something silly; they are doing the only thing available to them.

**Two distinct consequences, and the second is not a security issue at all:**

1. **TOCTOU.** Anyone who can write inside a tree can swap a directory for a
   symlink between two of our calls and redirect the second one. The live
   instance is `rm -r` — see `TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-
   REDIRECT-A-REMOVAL` above — but nothing about it is specific to `rm`.
2. **Plain wrongness under rename.** Hold a directory open, have anything
   rename it, and every subsequent `*at` call operates on the *old path* —
   which now names a different directory, or nothing. On a real Unix the
   descriptor follows the directory. This needs no attacker and no race
   window worth the name; an ordinary concurrent `mv` does it.

**What is *not* affected.** `SYS_FS_OPENAT2` (661) is a genuine fd-relative
call: it passes `dirfd` to the kernel rather than reconstructing a path, and as
of 2026-08-30 it enforces `RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS`. So the
containment primitive exists for *opening*. It has no counterpart for
`unlinkat`, `fstatat` or `getdents64`, which is why it does not rescue `rm`:
the walk could descend safely and still delete by path.

**Proper fix — kernel-side, lane A.** File handles must reference the
directory object, and the `*at` operations must resolve against it: an
unlink/stat/list that takes `(dirfd, name)` and never reconstructs a path.
`SYS_FS_OPENAT2` is the precedent for the ABI shape. Filed as
`requests/b-a-the-at-family-resolves-by-path-so-no-toctou-fix-is-possible.md`.

**Why this is filed rather than worked around.** The tempting workaround —
have `rm` and friends call `openat`/`unlinkat` anyway — is actively harmful
here. The certification harnesses build for `x86_64-unknown-linux-gnu` and
link *glibc*, where those calls are real, so the workaround would go green in
`scripts/*-diff.sh` and change nothing on the OS. It would convert a known
defect into an unknown one.

**Interim guidance for anyone writing userspace here:** do not rely on `*at`
for isolation. If a program's correctness depends on a directory not moving
under it, say so in a comment and treat it as a known gap, rather than reaching
for `openat` and assuming it bought something.
