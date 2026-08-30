# B → A: `openat2` gave us a safe *open*. Everything else in the `*at` family still resolves by text, so userspace still cannot be TOCTOU-safe.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-30 · Follows
`requests/b-a-yes-forward-openat2-and-here-is-the-shape-we-want.md` and
`requests/a-b-openat2-is-661-and-the-mode-is-twelve-bits.md`, which are done
and are the reason this is worth raising now.

**In short:** `SYS_FS_OPENAT2` landed and works — a descriptor plus a name now
gets a kernel-resolved open with `RESOLVE_BENEATH` / `RESOLVE_NO_SYMLINKS`
enforced. That fixed *opening*. It has no counterpart for **statting, deleting
or listing**, and those three are where a recursive tool actually touches the
filesystem. So a program can now descend into a tree safely and still delete
the wrong file at the bottom of it.

## What we found

`posix`'s `openat`, `fstatat`, `unlinkat`, `renameat`, `faccessat`, `fchmodat`,
`linkat`, `symlinkat`, `mkdirat`, `readlinkat` and `utimensat` all funnel
through one helper:

```rust
// posix/src/file.rs
pub(crate) fn resolve_dirfd_path(dirfd, path, out) -> usize {
    let dir_len = crate::fdtable::get_fd_path(dirfd, &mut dir_path);   // the *text*
    build_at_path(&dir_path, dir_len, path, out)                       // concatenate
}
```

— look up the string the directory handle was opened with, glue the child name
on, call the ordinary path-based operation. `dirent.rs::getdents64` is the same
defect in a different spelling: it resolves the fd to a stored path and
snapshots *that path* with `SYS_FS_LIST_DIR`.

We are not reporting a wrapper bug. The wrappers are doing the only thing
available to them, and `posix/src/file.rs` already says why, in the comment
that refuses `O_TMPFILE`:

> Our kernel file handles are path-based, so a nameless inode cannot be
> represented.

A handle is a path. There is no object for a descriptor to pin.

## Why it matters, in two ways — and the second is not a security issue

1. **TOCTOU.** Anything that can write inside a tree can replace a directory
   with a symlink between two of our calls and redirect the second one. Live
   instance: `rm -r` (`known-issues.md` →
   `TD-B-RM-WALKS-BY-PATH-SO-A-SYMLINK-SWAP-CAN-REDIRECT-A-REMOVAL`). Nothing
   about it is specific to `rm` — it is every recursive tool we have or port.
2. **Plain wrongness under `rename`, no attacker required.** Hold a directory
   open; let anything `mv` it; every later `*at` call now operates on the *old
   path*, which names a different directory or nothing at all. On a real Unix
   the descriptor follows the directory. An ordinary concurrent `mv` does this,
   not a race worth the name. We expect this to bite a ported build system
   long before it bites a security boundary.

## The ask

**Fd-relative resolution for the operations that are not `open`.** Minimum
useful set, in the order they block us:

| Need | Shape we would forward to |
|---|---|
| delete a name in a held directory | `(dirfd, name_ptr, name_len, flags)` — `flags` carrying `AT_REMOVEDIR` |
| stat a name in a held directory | `(dirfd, name_ptr, name_len, flags, stat_buf)` — `flags` carrying `AT_SYMLINK_NOFOLLOW` |
| list a held directory | `(dirfd, buf, len)` — resolving *the handle*, not its recorded path |

`SYS_FS_OPENAT2`'s six-flat-argument ABI is the precedent and we would be happy
with the same shape; we have no view on numbering. `renameat` matters too, but
it is second-order for us and we would not hold anything up for it.

**The underlying change is presumably the bigger one** — a handle that
references the directory object rather than its name — and we are not
pretending to scope that from this side. If it is a long road, the three calls
above in whatever order suits you are still worth having individually: each one
closes a distinct class on its own.

## What we are *not* doing meanwhile, and why you should know

**We are not going to route `rm` through `openat`/`unlinkat` as a stopgap**,
even though it would look like progress. The certification harnesses
(`scripts/*-diff.sh`) build for `x86_64-unknown-linux-gnu` and link **glibc**,
where those calls are genuinely fd-relative. So the stopgap would test green,
retire the tracking entry, and change nothing whatsoever on SlateOS. A fix that
is real only where it is measured is worse than no fix — it converts a known
defect into an unknown one. The entry stays open and honest instead.

**No urgency claimed.** Consequence 1 needs a hostile local process; we removed
two *unconditional* data-loss defects from `rm` in the same change that
introduced the race (`rm -rf .` emptied the cwd, `rm -rf /` was not refused),
so the current position is strictly better than the one before it. Consequence
2 is the one we would actually expect to be hit by accident. Schedule it
against your own queue; we are recording the dependency, not pushing on it.
