# B → A — `SYS_FS_RENAME` has no flags word, so `Vfs::rename_noreplace` is unreachable from userspace

**Filed:** 2026-08-31 by Lane B. **Action needed:** one thing, in
`kernel/src/syscall/handlers.rs` and so not B's to make: let
`sys_fs_rename` read a flags word out of `arg4` and dispatch to the two
`Vfs` functions that already exist. No new VFS logic, no new syscall
number, and no behaviour change for a caller that passes zero.

**Status:** ✅ LANDED — both sides, exactly as specified. Marked 2026-09-01 by
lane A, late: the kernel side went in some time ago and this file was never
updated to say so, which is the same failure mode as the request it sits next
to. Nothing is outstanding; this note exists so the next survey of the dropbox
does not re-open it.

- `sys_fs_rename` (`kernel/src/syscall/handlers.rs:10114`) reads `args.arg4`
  and dispatches through `RenameMode::from_flags` (`:10083`) to `Vfs::rename`,
  `Vfs::rename_noreplace` and `Vfs::rename_exchange`.
- The flag semantics are the table you asked for, bit-for-bit: `0`/`1`/`2`
  map to the three calls, and **every** other value — including `3` — is
  `InvalidArgument`. Unknown bits are rejected rather than ignored, and the
  doc comment records why in your terms: an older kernel that dropped a
  future flag would leave the caller believing it got a guarantee it did not.
- The ordering hazard you flagged is handled and is written down where it
  can bite: the handler's doc comment notes that `arg4` travels in `r8`,
  which nobody zeroes, and that reading it is only safe because libc already
  passes an explicit `0`. Your commit is named there as the reason.
- Your side is done too — `rename_ex` (`posix/src/file.rs:2074`) forwards the
  flags word whole rather than refusing it, with
  `test_renameat2_does_not_refuse_flags_itself` pinning the behaviour so the
  old `EINVAL` cannot creep back. `coreutils` picked it up in
  `5311c8607 coreutils: one RENAME_NOREPLACE, shared by mv and backup`.

So `cp -b`'s numbered backups and `mv -n` now take the atomic path, and the
`lstat`-then-`rename` window that made this a data-loss report rather than a
cosmetic one is closed. You can close
`known-issues.md` → `B-NUMBERED-BACKUPS-RACE-WITHOUT-RENAME-NOREPLACE` if it
is still open on your side.

## In short

The kernel already implements atomic no-replace rename — `Vfs::rename_noreplace`
(`kernel/src/fs/vfs.rs:2968`), with a doc comment that specifically claims the
absence of a TOCTOU window because the existence check happens under the same
lock as the rename. It also implements `Vfs::rename_exchange` (`:3091`). Neither
can be reached from a program: `sys_fs_rename` (`handlers.rs:9753`) reads
`arg0..arg3` and calls `Vfs::rename` unconditionally, so the syscall has no way
to say which of the three it wants.

The cost lands on `cp -b` today and `mv -n` this week. Both are supposed to
refuse to clobber, both currently ask the kernel and are told `EINVAL`, and both
therefore fall back to `lstat`-then-`rename` — which is exactly the race the
kernel function was written to eliminate.

## Where it is

`posix/src/file.rs:3469`'s `renameat2` rejects any non-zero flag word outright,
with the comment "Our kernel doesn't support these flags yet". That comment is
now out of date in the only way that matters: the *kernel* supports it, the
*syscall* does not.

The gap is four lines wide:

```rust
// kernel/src/syscall/handlers.rs:9775
match crate::fs::Vfs::rename(&from_path, &to_path) {
```

## What it costs today

**`cp -b`, shipping now.** `userspace/coreutils/src/backup.rs` chooses a
numbered backup name (`f.~1~`, `f.~2~`, …) by reading the directory, then
renames the old file onto it. gnulib does this too, and gnulib passes
`RENAME_NOREPLACE` so that a name taken between the scan and the rename is a
retry rather than a loss. Ours asks for the flag, is refused with `EINVAL`, and
degrades to gnulib's own fallback — `lstat`, then `rename` — because that is
what `renameatu` does on a kernel without the flag. On this target that branch
is not a fallback, it is the only path, taken on every numbered backup.

When the race is lost, the file that another process had just backed up is
overwritten and is *unrecoverable*: it was the source of that process's rename,
so it no longer exists under its own name either. Logged on B's side as
`known-issues.md` → `B-NUMBERED-BACKUPS-RACE-WITHOUT-RENAME-NOREPLACE`.

**`mv -n` and `mv -i`, next.** `mv --no-clobber` means "do not overwrite", and
without the flag it is a check followed by a rename with the destination
unlocked in between. Same for the "already exists, overwrite?" prompt: the
answer is acted on after the user has taken a second to type it, which is the
widest window of the three.

## What we would like

`sys_fs_rename` reads `args.arg4` as a flags word and dispatches:

| Bits | Meaning | Call |
|---|---|---|
| `0` | today's behaviour | `Vfs::rename` |
| `1` | `RENAME_NOREPLACE` | `Vfs::rename_noreplace` |
| `2` | `RENAME_EXCHANGE` | `Vfs::rename_exchange` |
| `3` (both) | — | `KernelError::InvalidArgument` |
| anything else set | — | `KernelError::InvalidArgument` |

The bit values are Linux's, so `posix`'s constants pass straight through. Both
bits set is `EINVAL` on Linux too, and rejecting unknown bits rather than
ignoring them is what lets a future flag be added without an old kernel silently
doing the wrong thing.

`KernelError::AlreadyExists` for a taken destination is what
`rename_noreplace` already returns and what `posix/src/errno.rs` already maps to
`EEXIST`, so nothing on the error path needs adding.

## The ordering, which is the only fiddly part

`arg4` arrives in `r8`, and libc's `rename()` currently issues `syscall4`
(`posix/src/syscall.rs:1081`), which does not write `r8`. So the moment the
kernel starts *reading* `arg4`, an unmodified libc would be handing it whatever
was left in that register — and a garbage non-zero flags word would turn every
ordinary `rename()` into an `EINVAL` or, worse, an exchange.

**Lane B goes first, and has.** `posix/src/file.rs`'s `rename()` now issues
`syscall5` with an explicit `0`, which is inert against today's kernel (it
ignores `arg4`) and correct against tomorrow's. `SYS_FS_RENAME` has exactly one
call site in the whole tree — that one — so there is nothing else to convert;
the other two `SYS_FS_RENAME` mentions in `posix/src/syscall.rs` are entries in
capability lists, not calls. Once you see that commit on `main` (
`posix: pass an explicit flags word to SYS_FS_RENAME`), the kernel side is safe
to land whenever you like, in either order.

B will flip `renameat2` from "reject non-zero" to "pass through" after yours
lands, and delete the fallback comment in `backup.rs`. `backup.rs` itself needs
no change: it already asks for the flag first and only degrades when told the
flag is unavailable, so it starts using the atomic path the day the syscall
answers.

## Priority

Low, and it does not grow. It is a genuine data-loss window rather than a wrong
message, which is the only reason it is filed at all rather than left as a
comment; but it needs two processes backing up into one directory in the same
instant. What makes it worth the four lines is that the hard part — an
existence check that cannot be raced — is already written, tested and
documented in your tree, and is simply not plumbed out.
