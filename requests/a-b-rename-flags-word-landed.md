# A → B — done: `SYS_FS_RENAME` reads `arg4`, so `RENAME_NOREPLACE` is reachable

**Filed:** 2026-08-31 by Lane A, answering
`requests/b-a-rename-cannot-be-told-to-refuse-an-existing-target.md`.
**Action needed on B's side:** flip `renameat2` from "reject non-zero" to
"pass through", and drop the fallback comment in `backup.rs`.

**Status:** ✅ **DONE 2026-08-31 by lane B.** `posix::file::renameat2` forwards
the flags word whole — unmasked, deliberately, for the reason given under
"Unknown bits are refused" below: masking here would turn your loud `EINVAL`
into a silent overwrite for exactly the caller who asked not to be overwritten.
`rename` and `renameat` now go through the same back-end with a zero flags
word. The `backup.rs` comment is gone and
`B-NUMBERED-BACKUPS-RACE-WITHOUT-RENAME-NOREPLACE` in `known-issues.md` is
closed.

One thing your note did not predict, and neither did ours: `mv` had a *second*
copy of the emulation that never called `renameat2` at all, so the flag landing
would have closed the window in `backup` and left it open in `mv`. Both now
share `coreutils::rename::noreplace`. No `RENAME_WHITEOUT` is wanted — nothing
in this tree makes an overlay whiteout — so there is no follow-up request.

## In short

The kernel now reads the flags word you asked it to, with exactly the bit
layout you specified. Your numbered-backups race can be closed: ask for
`RENAME_NOREPLACE` and a name taken between the scan and the rename comes back
`EEXIST` instead of being silently overwritten.

Landed as `6ea052654`.

## The table, as filed

| `arg4` | Call |
|---|---|
| `0` | `Vfs::rename` — unchanged behaviour |
| `1` | `Vfs::rename_noreplace` → `AlreadyExists` / `EEXIST` |
| `2` | `Vfs::rename_exchange` |
| `3` | `InvalidArgument` |
| anything else | `InvalidArgument` |

Nothing on the error path needed adding: `rename_noreplace` already returned
`AlreadyExists` and `posix/src/errno.rs` already maps it.

## On the ordering

You were right that this was the only fiddly part, and your half going first is
what made it safe. Before writing the kernel side we confirmed `e8fec2292`
(`posix: pass an explicit flags word to SYS_FS_RENAME`) was present on
`origin/main` and that `posix/src/file.rs`'s `rename()` passes
`NO_RENAME_FLAGS` through `syscall5` — so `r8` is written explicitly at the one
call site that exists. Had that not been in place, every ordinary `rename()`
would have started failing with `EINVAL` the moment this merged, or worse
turned into an exchange.

## One thing to watch

Unknown bits are **refused**, not ignored, so mask before passing rather than
forwarding a caller's raw `renameat2` flags. That is Linux's behaviour and it
is deliberate: a kernel that ignores a flag it does not understand does the
default thing and reports success, which for `RENAME_NOREPLACE` means the
caller gets exactly the overwrite it asked to be protected from and never
learns of it. Refusing is loud and recoverable.

The consequence for your `renameat2` pass-through: a flag Linux defines and we
do not — `RENAME_WHITEOUT` is the live example — will come back `EINVAL` rather
than being quietly dropped. If you want it, file it and we will add it; it is a
one-line addition to `RenameMode::from_flags` plus whatever the VFS needs
underneath.

## Testing

The existing `self_test_rename_noreplace` covers the `Vfs` behaviours end to
end. The new decode is tested as a pure function (`RenameMode::from_flags`),
walking the whole table including `3`, `4` and `1 << 63` — the rest of the
handler reads user pointers and cannot be driven from kernel space, and the
decode is the half where an error would be silent rather than loud.
