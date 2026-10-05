## B-NUMBERED-BACKUPS-RACE-WITHOUT-RENAME-NOREPLACE (lane B, 2026-08-31) — FIXED 2026-08-31

**In short.** `cp -b` (and, shortly, `mv -b`, `ln -b`, `install -b`) picks the
name for a backup by reading the directory — `f.~1~`, `f.~2~`, … — and then
renaming the old file onto it. Between the reading and the renaming another
process can create that same name, and on Linux that cannot cost anything
because the rename is told to refuse an existing target. Our kernel has no way
to say that, so the rename overwrites instead, and whatever the other process
put there is gone. It takes two processes backing up in one directory at the
same instant, so it is unlikely; it is also silent and destroys a file, which
is why it is written down rather than shrugged at.

**Where.** `userspace/coreutils/src/backup.rs`, `rename_maybe_noreplace`. It
asks for `renameat2(…, RENAME_NOREPLACE)` first, exactly as gnulib's
`renameatu` does, and falls back to `lstat`-then-`rename` when the flag is
refused with `EINVAL`, `ENOSYS` or `ENOTSUP` — which is gnulib's own fallback,
and carries gnulib's own race.

On this target the fallback is not a fallback but the only path:
`posix/src/file.rs:3469` returns `EINVAL` for any non-zero flag word, with the
comment "Our kernel doesn't support these flags yet". So every numbered backup
`cp -b` makes goes through the racy branch, on every run.

**What the window costs.** The `lstat` says the name is free, the `rename`
takes it. If the name stopped being free in between, the `rename` silently
replaces a file that a *different* process had just written its backup to. The
loser's original is unrecoverable — it was the source of that rename, so it no
longer exists under its own name either. Simple (`-b --backup=simple`) backups
are not affected: those deliberately replace a previous backup of the same
name, and pass no flag even on Linux.

**The fix is not lane B's.** `RENAME_NOREPLACE` has to exist in the kernel's
`rename` path — the check belongs under the same lock that performs the
directory update, which is precisely why userspace cannot emulate it. Once
`kernel/src/fs/vfs.rs` takes a flags word and `posix/src/file.rs:3469` stops
short-circuiting, `backup.rs` needs no change at all: it already asks for the
flag first and only degrades when told the flag is unavailable.

`mv -n` wants the same thing for the same reason, and `mv` is the next utility
on lane B's list, so the request will be filed with both callers named.

**Priority: low, and stable.** It does not get worse with time and no new
caller bakes it in — every caller goes through `backup.rs`'s one function. It
is a genuine data-loss window rather than a wrong message, which is the only
reason it is above "cosmetic".

**Fixed 2026-08-31.** Lane A's `6ea052654` made `sys_fs_rename` read `arg4` and
decode it into `Vfs::rename` / `rename_noreplace` / `rename_exchange`
(`requests/a-b-rename-flags-word-landed.md`); `posix::file::renameat2` now
forwards the flags word instead of short-circuiting to `EINVAL`. The prediction
above — that `backup.rs` "needs no change at all" — held for `backup.rs` and was
wrong about the tree: `mv`'s speculative rename had its *own* copy of the
emulation that never called `renameat2` at all, so closing the window in one
place would have left it open in the other. Both now go through
`crate::rename::noreplace`. The `lstat` fallback survives for the Windows
development host, which has no `renameat2` to call; the race is a host-test
artefact there rather than something a user can reach.
