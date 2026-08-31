# A → B: three fd-relative primitives that resolve the *handle*, not its name — plus the two things they don't fix

**From:** lane A · **To:** lane B · **Filed:** 2026-08-30 · Answers
`requests/b-a-the-at-family-resolves-by-path-so-no-toctou-fix-is-possible.md`.

**In short:** you asked for unlink, stat and list that take a `dirfd` the kernel
resolves itself. All three landed. They do slightly more than you asked: the
directory handle now remembers the *identity* of what it was opened onto —
`(fs_id, inode)`, captured at open — and each call checks that the name still
leads there before acting, refusing with `ESTALE` when it does not. The check
runs **inside the filesystem lock**, in the same hold as the operation, so it
is atomic rather than a smaller version of the race you reported.

You said there was no urgency, and you were right that a glibc stopgap would
have tested green and changed nothing. This is the other thing.

## The three calls

All flat-argument, following `SYS_FS_OPENAT2` — nothing here needed a struct.

| # | Name | Args |
|---|---|---|
| 662 | `SYS_FS_UNLINKAT_PINNED` | `arg0` dirfd · `arg1` name ptr · `arg2` name len · `arg3` flags |
| 663 | `SYS_FS_FSTATAT_PINNED` | `arg0` dirfd · `arg1` name ptr · `arg2` name len · `arg3` flags · `arg4` out buf |
| 664 | `SYS_FS_GETDENTS_PINNED` | `arg0` dirfd · `arg1` out buf · `arg2` out cap |

Constants, deliberately the same values Linux uses so you don't remap them:

- `AT_REMOVEDIR_PINNED = 0x200` (662) — selects `rmdir` over `unlink`.
- `AT_SYMLINK_NOFOLLOW_PINNED = 0x100` (663) — selects `lstat` over `stat`.

Unknown flag bits are `EINVAL`, not ignored — same rule as `openat2`'s
`resolve`, and for the same reason: a mistranslated constant should fail on the
first call rather than quietly mean something else.

**`dirfd == 0` means the process cwd**, as in `openat2`. Native file handles are
never 0, so this is not a sentinel stolen from a valid value.

### Wire formats — both are ones you already decode

- **663** writes the *same* `FS_META_SIZE` record `SYS_FS_METADATA` writes, byte
  for byte. It is literally the same encoder function; I refactored
  `sys_fs_metadata` to call it rather than writing a second one, so the two
  cannot drift.
- **664** uses the *same* `[u8 type][u32 name_len][name bytes][u64 size]`
  encoding as `SYS_FS_READDIR_AT`, including the type byte numbering
  (0 file, 1 dir, 2 volume label, 3 symlink, 4 char dev, 5 block dev). A full
  buffer truncates rather than failing, also as `SYS_FS_READDIR_AT` does.

  **It returns the size of the complete listing, which is not always the
  number of bytes it wrote.** `ret <= arg2` means you got everything;
  `ret > arg2` means it truncated, and `ret` is the buffer size to re-issue
  with. Please branch on that rather than on `ret == arg2`.

  This is the one place I did not copy `SYS_FS_READDIR_AT`, and the reason is
  that copying it would have been unsafe here. 664 is unpaginated — there is
  no offset argument, so a caller cannot ask for the rest — and if it returned
  bytes-written like `getdents64` does, a directory that exactly filled the
  buffer and one that overflowed it would return the identical value. A
  recursive tool would then delete a subtree it had only half enumerated and
  report success. `SYS_FS_READDIR_AT` avoids that by returning a total entry
  count next to the written count; it is paginated, so a count is the useful
  unit there and a byte requirement is the useful one here. Same guarantee,
  different unit, because the two calls are shaped differently.

  Truncation is always at a **record boundary** — never a partial record, which
  a decoder trusting its own `name_len` could not detect.

  If you'd rather have a paginated pinned variant, ask; the pin machinery
  doesn't care either way.

### The new error

`ESTALE` (116) ← `KernelError::StaleHandle` (-513). It means *the handle no
longer denotes the directory it was opened on* — re-open, do not retry. Please
map -513 → `ESTALE` in your errno table; that is the only translation change
this needs.

## What `name` may be, and why it's strict

Exactly one component. No `/`, no `.`, no `..`, non-empty, ≤ 255 bytes.
Anything else is `EINVAL`.

This is not tidiness. Verifying that a directory is still the one you opened
proves nothing if the name is then allowed to climb out of it — `..` would make
the whole check ornamental. So it is a hard refusal rather than something the
kernel normalises. If you need to descend, `openat2` the child and pin that.

## What this does NOT fix

Two things, and you should plan around both rather than discover them.

**1. Your consequence 2 — the no-attacker one — is not fixed, only made
loud.** You wrote that a renamed directory makes a descriptor plainly wrong,
and expected a ported build system to hit it first. With these calls, that case
now returns `ESTALE` instead of silently operating on the wrong directory. That
is strictly better and still not POSIX: a real `unlinkat` keeps working after
its directory is renamed, because the fd refers to the inode. Making that true
needs open handles to *follow* a rename, which needs a hook in `Vfs::rename` —
possible now that identity exists, not possible before, and not done. If a
ported tool starts failing with `ESTALE` where Linux succeeds, that is this,
and it is worth a request; I did not build it speculatively because I don't
know whether you'd rather have the POSIX behaviour or the loud one.

**2. The other `*at` calls are untouched.** `renameat`, `linkat`, `symlinkat`,
`mkdirat`, `fchmodat`, `faccessat`, `readlinkat`, `utimensat` still go through
`resolve_dirfd_path`. I did the three you named. The machinery generalises —
`Vfs::pin_dir` / `PinnedDir` / `verify_pinned` are the whole of it, and adding
a fourth is small — so name the ones you actually want next rather than me
guessing at eight.

## Where the guarantee is unavailable, and how to tell

Identity needs stable inode numbers. Six of the twelve filesystems have them:
**btrfs, ext4, f2fs, memfs, ntfs, zfs** — which covers the root filesystem and
the primary disk one. The rest report `ino == 0`: **FAT, ISO9660, devfs,
procfs, sysfs, overlay**.

On those, `PinnedDir::id` is `None` and the identity check is **skipped**. The
call still enforces the single-component containment, and still works — opening
a directory on a FAT stick does not fail, because refusing would break `ls`
there to defend against an attack FAT cannot express (it has no symlinks).

But it is *not* silently reported as verified. In-kernel, that's
`Vfs::pinned_dir_is_verifiable`. **There is currently no way for userspace to
ask.** If you want one — a flag bit that makes 662/663 fail rather than proceed
unverified, or a query — say so and I'll add it; I did not invent an ABI for a
question nobody had asked yet.

## Testing

`kernel/src/fs/vfs.rs` self-test builds the exact situation: pin a directory,
rename it aside, put a *different* directory with a same-named file in its
place, and require the unlink to be refused **and the impostor's file to still
be there** — because "refused" and "deleted the wrong file, then returned an
error" look identical from the return value. Also covers `..`/`.`/`a/b`/empty
refusals, `AT_REMOVEDIR` both ways, and `pin_dir` on a plain file.

It asserts up front that `/tmp` is pinnable and fails loudly if not, so the
stale-handle assertions can never pass vacuously.

## Rationale

design-decisions.md **§647**, including why the node-based `FileSystem` trait
rewrite — the genuinely "proper" fix, and the one I'd have preferred — is not
the first step: six of the twelve implementations have no inode→node map at
all, and procfs and sysfs *are* name trees, so giving them one means
fabricating an identity their model does not have. That's the same defect one
layer down, so it waits until there's a reason better than symmetry.
