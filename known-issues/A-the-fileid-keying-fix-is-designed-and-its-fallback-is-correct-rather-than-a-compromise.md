### [A] The `FileId` keying fix is designed, and its fallback is correct rather than a compromise -- 2026-09-21
**Status:** DESIGNED, not written (needs a compiler and a green base). Resolves the open design question in the path-keyed entries above.

**In short:** five kernel tables remember a file by its name instead of by
the file, so two names for one file get two answers. The fix is to key on
the filesystem identity instead. The question that was open: what to do on
a filesystem that has no such identity. It turns out not to be a problem.

**The mechanism fits without a new resolution step.** `flock_resolved`
already holds an *already-resolved* path, and
`Vfs::file_identity_resolved(path) -> KernelResult<Option<FileId>>` takes
exactly that. So no re-resolution, and no second TOCTOU window opened by
the fix. `FileId` is `{ fs_id: u64, ino: u64 }`.

**Why the `None` case is safe, which is the part that was unclear.**
`file_identity_resolved` returns `Ok(None)` when `ino == 0`, documented as
*filesystem has no stable per-object identity ... lets the caller degrade*.
Degrading means falling back to the name -- which sounds like
reintroducing the bug. Measured which filesystems take that path:

| filesystem | sets `ino: 0` | has hard links |
|---|---|---|
| `ext4` | 0 of 15 sites | yes |
| `memfs` | 0 of 16 | yes |
| `fat` | 1 of 3 | no (FAT has no link count) |
| `devfs` | 5 of 5 | no |
| `procfs` | 30 of 31 | no |
| `sysfs` | 58 of 58 | no |

So every filesystem that can have two names for one file has a stable
inode, and every filesystem without one cannot have two names for one
file. **The fallback is not a weaker path taken reluctantly -- on the
filesystems that take it, a name IS the identity.** That closes the
question the earlier entries left open.

**Two call shapes, and each table needs the right one.** Only `flock`
already holds a resolved path; `sealing`, `capsettings` and `reclock` take
a raw one straight from `kshell` and do no resolution at all (measured: 0
references to `resolve_follow` or `file_identity` between them). So:

| site | call |
|---|---|
| `vfs::flock_resolved` | `file_identity_resolved(path)` -- path already resolved, adds no new lookup |
| `sealing`, `capsettings`, `reclock` | `Vfs::file_identity(path)`, which does `resolve_follow` then the same thing |

Resolving at *each* operation is correct rather than a cost: if a symlink
is repointed between sealing a file and checking the seal, the identity
SHOULD differ -- that is the whole reason for keying on the file. Caching
one identity at seal time would recreate the original bug with an extra
step.

**What to write:** key each of the five on `FileId` where
`file_identity_resolved` yields one, and on the resolved path where it
yields `None`, with a comment at each site giving the reason above so the
fallback is not later read as laziness. `funlock_all(owner)` and
`handle::close` need no change: they iterate by owner, not by key.
