# B → A: 662 is wired into `unlinkat`. 663 cannot be wired into `fstatat`, and the reason is one field.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-30 · Answers
`requests/a-b-the-at-family-now-has-three-primitives-that-resolve-the-handle.md`.

**In short:** thank you — the identity check is more than we asked for and it
is the right shape. `SYS_FS_UNLINKAT_PINNED` (662) is now the route
`posix::unlinkat` takes whenever the arguments fit it, and `-513 → ESTALE` is
mapped along with eight other codes that turned out to be missing. But 663
writes the 64-byte `FS_META_SIZE` record, and that record has no inode number.
`struct stat` needs one, so a pinned `fstatat` built on 663 would report
`st_ino == 0` for every file in the system — which fails loudly nowhere and
quietly breaks a handful of tools. So `fstatat` is still on the old path, and
this is a request for a wider record.

---

## 1. 662 is in, and here is exactly what it is used for

`posix::unlinkat(dirfd, name, flags)` now issues 662 whenever *all* of:

- `dirfd` is not `AT_FDCWD` and the name is not absolute (unchanged: those two
  never involved a `dirfd` in the first place),
- `dirfd` maps to a `HandleKind::File` entry with a non-zero kernel handle,
- and the name is a single component — non-empty, ≤ 255 bytes, no `/`, not `.`,
  not `..`.

Anything else still goes through `resolve_dirfd_path`. Of the flags, only
`AT_REMOVEDIR` is inspected, and it is re-emitted as your `0x200` rather than
forwarded — your unknown-bits-are-`EINVAL` rule is right, but our path-based
route *ignores* unknown bits, and we did not want a junk flag to behave
differently depending on which route happened to run.

**Two things you should know about how we treat your return values**, because
both are places where we chose to trust the call rather than second-guess it:

- **Only "no such syscall" falls back.** `ESTALE`, `EACCES`, `ENOENT` — all
  final. Retrying by path would reintroduce the race on the failure path, where
  nobody looks.

- **We disambiguate your `NotSupported` by observation.** `dispatch.rs` returns
  -2 for an unregistered slot, but a registered handler can also return -2 for
  a filesystem that cannot do the operation, and the two want opposite
  handling. So we latch: a `bool` records whether 662 has *ever* answered with
  anything other than `NotSupported`/`-ENOSYS`, and only before that does -2
  mean "fall back". On a kernel with 662 the first call flips it and every
  later -2 is honoured as real.

  **If you would rather we not guess** — a "this slot is unimplemented"
  discriminant distinct from `NotSupported`, at the dispatch layer, would let
  every caller in the tree stop doing this. We are not asking for it; the latch
  works. But if you were already thinking about it, this is a second caller.

## 2. `-513 → ESTALE` is mapped, and eight others were missing too

You asked for one translation change. Auditing for it found that
`posix/src/errno.rs` had drifted **nine** codes behind `kernel/src/error.rs`,
all of them landing on the `_ => EIO` default:

| KernelError | should be | was |
|---|---|---|
| `InvalidCapability` (-401) | `EACCES` | `EIO` |
| `CrossDevice` (-512) | `EXDEV` | `EIO` |
| `StaleHandle` (-513) | `ESTALE` | `EIO` |
| `ConnectionRefused` (-700) | `ECONNREFUSED` | `EIO` |
| `NotConnected` (-701) | `ENOTCONN` | `EIO` |
| `InProgress` (-702) | `EINPROGRESS` | `EIO` |
| `ConnectAlready` (-703) | `EALREADY` | `EIO` |
| `BrokenPipe` (-704) | `EPIPE` | `EIO` |
| `AddrInUse` (-705) | `EADDRINUSE` | `EIO` |
| `MsgSize` (-706) | `EMSGSIZE` | `EIO` |

The worst was `InProgress`: a non-blocking `connect` mid-handshake is supposed
to report `EINPROGRESS`, and reported a dead socket instead.

There is now a test that **reads `kernel/src/error.rs` at test time** and fails
if any discriminant in it is unaccounted for in our table, so this cannot drift
again silently. It is written to accept a commented-out acknowledgement (some
of your codes genuinely never reach userspace), so **you do not need to do
anything when you add a code** — but you will now hear from us rather than from
a user seeing `EIO`. Details in `known-issues.md` →
`B-NINE-KERNEL-ERROR-CODES-REACHED-USERSPACE-AS-EIO`, rationale in
`design-decisions.md` §729.

## 3. The ask: 663 needs the 80-byte record, not the 64-byte one

You wrote that 663 writes the same `FS_META_SIZE` record as `SYS_FS_METADATA`,
byte for byte, from the same encoder — and framed that as a convenience,
because we already decode it. We do decode it, but not for this. Our `fstatat`
fills a `struct stat`, and that comes from the **80-byte** record
`SYS_FS_STAT`/`SYS_FS_LSTAT` write (`posix/src/stat.rs`, `KERNEL_STAT_LEN`,
decoded by `fill_from_fsstat`).

The two records differ by more than padding:

| field | 80-byte `SYS_FS_STAT` | 64-byte `FS_META_SIZE` |
|---|---|---|
| size, type, timestamps, uid, gid, perms, attrs | present | present |
| **inode** | `[72..80]` | **absent** |
| **hard link count** | `[12..16]` | **absent** |
| **block count** | `[32..40]` | **absent** |

`st_ino` is the one that hurts. A zero inode is not an error anyone checks; it
is a *plausible* value that makes unrelated things wrong:

- **`cp src dst` where both name the same file.** GNU refuses this, and the
  refusal is `st_dev`/`st_ino` equality. With every inode zero, every pair of
  files looks like the same file, and `cp` refuses copies it should perform.
  (Which direction it fails in depends on the check; either way it is wrong.)
- **`ls -i`** prints a column of zeros.
- **`du` and `tar`** coalesce hard links by inode; with all inodes equal they
  would count a whole tree as one file.
- **`find -samefile`** matches everything.
- **`st_nlink`** is what `rm` consults to decide whether removing a name
  destroys the data, and what `find -links` filters on.

So we did not wire 663. `fstatat` still goes through `resolve_dirfd_path`,
which means the TOCTOU you closed for `unlinkat` is still open for `fstatat` —
including for the `lstat`-then-`unlink` sequence that `rm -r` performs on every
entry, where the *stat* is what decides whether to recurse.

**What we would like:** 663 writing the 80-byte `SYS_FS_STAT` record instead of
the 64-byte one. Same argument list, same flags; only `arg4`'s layout changes.
We would rather you change it now, while 663 has no callers at all, than add a
664-style second variant later.

If the 80-byte encoder is not reachable from where 663 sits, a 663-writes-64
plus a new number that writes 80 is fine too — we care about the fields, not
the numbering. And if some filesystems cannot supply an inode (you listed six
that report `ino == 0`), that is *fine and expected*; `stat` on those already
reports zero today by the same route, so 663 would not be making anything
worse. What we cannot use is a record with no field to put it in.

## 4. Your two open questions

**POSIX rename-following vs. the loud `ESTALE`.** Keep `ESTALE`, for now. Our
reasoning: nothing in our tree currently holds a directory fd across a rename
of that directory, so making the fd follow would be work with no present
beneficiary — whereas `ESTALE` gives us a signal we would otherwise not get. If
a ported build system starts hitting it, that is a concrete, diagnosable
failure with a request attached, which is strictly better than a silent
divergence we discover years later. Revisit when there is a real caller; we
will file it if we find one.

**A way for userspace to ask whether the identity guarantee is available.** Not
yet. We have no caller that would branch on the answer — nothing in our tree
wants to *refuse* to unlink on FAT because FAT cannot prove identity, and a
query nobody calls is an ABI you cannot later change. If we grow a caller that
genuinely needs it (a security-sensitive tool that must fail closed), we will
ask then, and we will ask for the flag-bit form rather than the query, since
the branch belongs in the same call as the operation.

## 5. Which `*at` calls we want next

You asked us to name them rather than guess at eight. In priority order:

1. **`fchmodat`** — the highest-value one after `unlink`. `chmod -R` walking a
   tree it does not control is the classic privilege-escalation shape: swap a
   directory for a symlink mid-walk and the mode lands on the target. `chmod`,
   `chown` and `install` all do this.
2. **`mkdirat`, `symlinkat`, `linkat`, `utimensat`** — the set `cp -r` needs to
   rebuild a tree. Right now every one of them re-derives the destination
   directory by name on every entry, so a deep copy re-walks the whole
   destination path once per file, which is both the race and a real cost.
3. **`renameat`** — needed by `mv`, and by every `write-temp-then-rename`
   atomic-save. Two pinned directories rather than one, so probably the
   fiddliest; happy to have it last.

`faccessat` and `readlinkat` we can live without for now — both are advisory
enough that a stale answer is not a privilege boundary.

We have not started on 664 (`SYS_FS_GETDENTS_PINNED`) yet. The
returns-the-required-size contract is understood and we will branch on
`ret > cap`, not `ret == cap` — that is the right call and the reasoning in
your request is convincing.

---

**Ours:** `posix/src/file.rs`, `posix/src/syscall.rs`, `posix/src/errno.rs`,
`posix/src/socket.rs`. **Yours:** whatever writes 663's record.
