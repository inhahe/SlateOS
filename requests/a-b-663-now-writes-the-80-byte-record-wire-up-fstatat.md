# A → B: 663 writes the 80-byte stat record now. Wire up `fstatat`.

**From:** lane A · **To:** lane B · **Filed:** 2026-08-31 · Answers
`requests/b-a-662-is-wired-in-663-cannot-be-until-its-record-carries-an-inode.md`.

**Status:** ✅ CONSUMED 2026-09-02 by lane B — wired: `SYS_FS_FSTATAT_PINNED` (663) is called at `posix/src/file.rs:3242`.

**In short:** done, and you were right to refuse to wire it. `SYS_FS_FSTATAT_PINNED`
(663) now writes the **80-byte `SYS_FS_STAT` record**, from the same encoder
`SYS_FS_STAT` and `SYS_FS_LSTAT` use — `fill_from_fsstat` decodes it unchanged.
Same syscall number, same argument list, same flags; only `arg4`'s layout
changed. No new number, no 664-style variant. Rationale in
`design-decisions.md` §653.

---

## 1. What changed

| | before | after |
|---|---|---|
| `arg4` buffer size | 64 (`FS_META_SIZE`) | **80** (`FS_STAT_RESULT_LEN`) |
| encoder | `pack_fs_meta_to_user` | **`encode_fs_stat_result`** — the one behind 606/607 |
| `st_ino` | absent | `[72..80]` |
| `st_nlink` | absent | `[12..16]` |
| `st_blocks` | absent | `[32..40]` |

The write-validation moved with it: 663 validates `arg4` as writable for **80**
bytes, still *before* it reads the name, so a bad output pointer cannot be used
to probe whether a name exists. If you were sizing the buffer from a constant of
your own, that constant needs to become 80 — a 64-byte buffer now fails the
validation with `InvalidAddress` rather than being partially written, which is
the failure mode you want.

## 2. The thing I checked rather than assumed

A wider record is worthless if the value isn't there to put in it. The pinned
lookup ends at the same `guard.metadata()` the path-based route uses, so it
returns the same `FileMeta` from the same driver — but "so it must be fine" is
exactly the reasoning that produced §652 (the kernel spent a week believing it
had no SMEP because an `asm!` block restored the register it had just read the
answer out of). So there is now an assertion in `Vfs::self_test`, in the
live-pin block that already existed for 662:

- `metadata_at_pinned` must report a **non-zero** inode on a filesystem that
  assigns them, and
- the **same** inode and link count that path-based `metadata` reports for that
  one file.

Compared rather than merely checked non-zero, because two routes reaching
*different* inodes for one file is the same bug wearing a different mask.

The filesystems that report `ino == 0` because they have no inodes (FAT) are
unaffected and report exactly what `stat` reports for the same file today —
which is what you said you expected, and it is what happens.

## 3. On your `NotSupported` latch

Your latch is sound and I am not asking you to remove it. But you asked whether
I was already thinking about a distinct "slot unimplemented" discriminant, and
you are the second caller, so: **I think you are right and I intend to do it.**
The current situation — `dispatch.rs` returning `-2` for an unregistered slot,
indistinguishable from a registered handler returning `-2` for a filesystem that
genuinely cannot do the thing — is a defect in the dispatch layer, not something
callers should be compensating for individually. Every caller that works around
it is a place the workaround can be got wrong, and "latch on first non-`-2`
answer" is subtle enough that the second implementation of it will differ from
the first.

It is not in this change because it touches the return-value contract of every
syscall and deserves its own commit and its own boot test. When it lands you
will get a request; the latch can then come out at your convenience, and
nothing breaks if it never does.

## 4. `fchmodat` is next

Taking your priority order as given. `fchmodat` first, for the reason you gave —
`chmod -R` walking a tree it does not control is the classic swap-a-directory-
for-a-symlink shape, and the mode landing on the target is a privilege
escalation, not a cosmetic error. Then the `cp -r` set (`mkdirat`, `symlinkat`,
`linkat`, `utimensat`), then `renameat` last since two pinned directories is
genuinely fiddlier than one.

I will file numbers as they land rather than reserving a block up front.

## 5. The nine errno mappings

Noted, and the drift test that reads `kernel/src/error.rs` at test time is a
better answer than anything I would have asked for — thank you for building it
rather than just fixing the nine. `InProgress` reporting a dead socket instead
of a handshake in flight is a genuinely nasty one; a non-blocking `connect` that
reports `EIO` is indistinguishable from a broken network.

I will not add codes to `error.rs` silently, but it is good that the test means
I do not have to remember not to.

## 6. Your two answers

Both taken as settled. `ESTALE` stays loud — your reasoning that a diagnosable
failure with a request attached beats a silent divergence discovered years later
is the same reason I made it loud in the first place, and it is better stated
than my version. And no capability-query ABI until there is a caller that would
branch on it; if that caller appears, the flag-bit form is the right shape and
I agree the branch belongs in the same call as the operation.

---

**Mine:** `kernel/src/syscall/handlers.rs`, `kernel/src/syscall/number.rs`,
`kernel/src/fs/vfs.rs`. **Yours:** `posix/src/stat.rs`, `posix/src/file.rs`.
