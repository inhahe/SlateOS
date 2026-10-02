## 658. The pinned `*at` set a recursive copy needs: what the pin covers, what following a symlink gives up, and why `linkat` has no flags

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** Copying a directory tree means creating a lot of new files inside
a destination directory. Today each of those creations names the destination by
its *text* — "put it in `/home/me/backup/photos`" — and looks that text up again
from scratch every single time. If someone renames that directory partway
through the copy and puts a different one in its place, every remaining file
lands in the replacement, and nothing reports a problem. This adds four kernel
calls (`mkdirat`, `symlinkat`, `linkat`, `utimensat`, numbers 666–669) that take
a *handle* to the destination instead of its name, remember which directory that
handle was opened on, and refuse with "stale handle" if it is no longer the same
one. It also decides three smaller things: a symlink's target is deliberately
left unrestricted, `linkat` deliberately has no flags argument, and following a
symlink deliberately gives up the guarantee.

Requested by lane B in
`requests/b-a-662-is-wired-in-663-cannot-be-until-its-record-carries-an-inode.md`
§5, priority 2: "the set `cp -r` needs to rebuild a tree." Continues §647
(the pinned family and the two-pass discipline) and §648 (no `dirfd == 0`
ambient cwd).

### Why this set is a different risk from `unlinkat` and `fchmodat`

§647 justified the family on a *read* and a *modify*: a stale `fstatat` reports
the wrong object, a stale `unlinkat` deletes the wrong one, a stale `fchmodat`
sets a mode on the wrong one. These four are the first where a stale handle
causes an object to be **created** somewhere the caller never named.

That is a strictly worse shape, for a reason worth writing down. The earlier
calls all operate on something that already exists, so the attacker's gain is
bounded by what is already there and what the caller already had the authority
to touch. A creation is unbounded: the attacker chooses the destination and the
caller supplies the contents. A `cp -r` of a source tree the attacker can also
write is then a general-purpose "write this file anywhere the copying process
can write" primitive, and the copying process is very often more privileged than
whoever supplied the tree — that is what a backup restore, a package install and
an archive extraction all are.

The cost side is real too and was lane B's own framing: the destination path is
re-walked in full once per entry, so a deep copy is quadratic in path depth for
no reason.

### The decisions

1. **Four separate syscalls, not one `cp -r` syscall.** The alternative — a
   single kernel-side recursive copy — would be faster still and would close the
   race on the *source* as well. Rejected: it puts policy (what to do about
   permissions, ownership, sparse files, hard-link coalescing, symlinks,
   cross-device boundaries, and errors partway through) inside the kernel, where
   every future `cp` flag becomes an ABI change. The thin-primitive model is
   this project's standing choice and there is no reason for this to be its
   exception.

2. **A symlink target is not validated. Only the link *name* is.** This is the
   one asymmetry in the family and it looks wrong at a glance, so: the
   single-component rule (`check_at_name`) exists to guarantee that the object
   being created is *inside* the pinned directory. That is a statement about the
   name. A symlink's target is not a name being created — it is arbitrary text
   stored verbatim and interpreted only when something later walks through the
   link, at which point the ordinary traversal-time checks apply, exactly as
   they do for a link created by the path-based `symlink`. Restricting it would
   therefore secure nothing while making `symlinkat` unable to reproduce the
   relative links (`../lib/libfoo.so`) that any real tree contains. Pinned in
   the self-test in both directions: `../src/f` is accepted as a *target* and
   refused as a *name*.

3. **`linkat` takes no flags, so it cannot follow a symlink.** Six registers are
   spent on two handles and two counted names, so there is physically no room —
   but the constraint and the right answer coincide, which is why no struct
   argument was added to escape it. `AT_SYMLINK_FOLLOW`'s effect is to link a
   symlink's *target*, and following is by definition leaving the pinned
   directory, so the flag asks this call to stop providing the guarantee it
   exists for. A caller that genuinely wants the followed form wants the
   path-based route, where it was never getting a guarantee anyway.
   `AT_SYMLINK_FOLLOW_PINNED` and the VFS primitive's `follow` parameter are
   defined regardless, so the decision is reversible without an ABI change to
   anything already shipped.

4. **Following a trailing symlink gives up the pin, and says so rather than
   pretending.** `utimensat` without `AT_SYMLINK_NOFOLLOW` may end up stamping
   an object on another mount. Rather than refuse (which would break the POSIX
   default) or pretend the pin still covers it (which would be a lie), the
   policy checks are re-run against the *resolved* target and the write goes
   through the target's own mount — the same structure §647 settled on for
   `fchmodat`. The residual race on the link's own contents is the one plain
   `utimensat` has too; what the pin removes is the much larger race on the
   directory.

5. **`mkdirat` stamps its mode under the same lock that created the
   directory.** The path-based `mkdir_mode` creates with the filesystem's 0o755
   default and chmods in a second, separate lock acquisition, so a directory
   requested as 0o700 is briefly world-readable and *openable* by anyone who is
   watching. Closing that is nearly free here because the pinned route already
   holds the lock, so it was closed rather than copied. This is a deliberate
   divergence from the path route in the safer direction; the path route keeps
   the window and should be fixed separately.

6. **Nine mode bits for `mkdirat`, twelve for `fchmodat`.** §639 established
   twelve for `fchmodat` because silently masking away a requested setuid bit is
   the worst way for a permission request to fail. Creation is the opposite
   case: a directory that is setgid or sticky from the instant it exists is a
   policy decision the caller can still make explicitly with a following
   `fchmodat`, where it is a separate, separately-auditable request.
   `SYS_FS_MKDIR_MODE` already masks to nine and consistency with it is worth
   more than consistency with `fchmodat`.

7. **Cross-mount `linkat` reports `InvalidArgument`, not `CrossDevice`.** POSIX
   callers want `EXDEV`. The path-based `link` has always returned
   `InvalidArgument` here, and the POSIX layer already translates it; making the
   pinned route differ would mean one operation with two error contracts
   depending on which route ran, which is the specific failure lane B described
   in their §1 about flag handling. If this changes it should change for both
   routes at once.

### What this does *not* fix

`linkat`'s source side is pinned, but a hard link's real risk — that a name
kept alive somewhere the caller cannot see keeps a deleted file's data alive —
is not a race and is not addressed here.

More importantly, `cp -r` also walks a **source** tree, and nothing here pins
the source directory across the walk. `readdir_pinned` (664) exists and lane B
has not wired it yet; until they do, a recursive copy still re-derives its
source directory by name. That is a read-side race (the copy reads the wrong
file) rather than a write-side one, which is why it is not blocking, but the set
is not complete without it.

`renameat` is not here. It needs two pinned directories like `linkat` but with
a mutation on both sides, and lane B explicitly asked for it last.

### How to reverse it

Delete the four `*_at_pinned` methods in `kernel/src/fs/vfs.rs`, the four
handlers in `kernel/src/syscall/handlers.rs`, their four `dispatch.rs`
registrations, and the numbers in `number.rs`. Unregistering the numbers is
sufficient on its own: §656 made an unregistered slot answer `NoSuchSyscall`
(-10) rather than `NotSupported`, so a caller that probes gets a clean answer
and falls back to the path route. The `mkdir_at_pinned` mode-stamp improvement
(decision 5) is the only piece with no path-based equivalent to fall back to,
and it affects only a window, not a result.
