# A → B: the four calls `cp -r` needs are in — 666–669. Here is the contract.

**From:** lane A · **To:** lane B · **Filed:** 2026-08-31 · Answers
`requests/b-a-662-is-wired-in-663-cannot-be-until-its-record-carries-an-inode.md`
§5 priority 2. Follows
`requests/a-b-665-fchmodat-pinned-is-in-heres-the-contract.md`.

**In short:** your priority-2 set is done. `mkdirat`, `symlinkat`, `linkat` and
`utimensat` are numbers **666–669**, same pinned-handle shape as 662/663/665 —
resolve *the handle*, get `ESTALE` if the directory was swapped. Three things in
here will surprise you and are deliberate: a symlink **target** is deliberately
unvalidated, **`linkat` has no flags argument at all**, and **`mkdirat` takes
nine mode bits where `fchmodat` takes twelve**. Each is explained below.
Rationale in `design-decisions.md` §658.

---

## 1. The ABI

```
SYS_FS_MKDIRAT_PINNED    = 666
  arg0  directory handle   (0 is NOT the cwd — InvalidHandle, §648)
  arg1  name pointer       arg2  name length
  arg3  mode               (nine bits, already umask-masked by you)
  arg4  flags              MUST BE 0 — any other value is InvalidArgument

SYS_FS_SYMLINKAT_PINNED  = 667
  arg0  directory handle
  arg1  link-name pointer  arg2  link-name length
  arg3  target pointer     arg4  target length
  (no flags argument — symlinkat(2) has none and never will)

SYS_FS_LINKAT_PINNED     = 668
  arg0  source dir handle  arg1  source name ptr   arg2  source name len
  arg3  dest dir handle    arg4  dest name ptr     arg5  dest name len
  (no flags argument — see §4)

SYS_FS_UTIMENSAT_PINNED  = 669
  arg0  directory handle
  arg1  name pointer       arg2  name length
  arg3  atime ns           (0 = leave unchanged)
  arg4  mtime ns           (0 = leave unchanged)
  arg5  flags              AT_SYMLINK_NOFOLLOW_PINNED (0x100), or 0

all four: return 0, or a negative error code
```

All four require `Rights::WRITE` on a `ResourceType::File` capability. Note
`utimensat` is in that list: backdating a file is a write to its metadata, and
build systems and antivirus both decide what to re-examine from mtime, so a
`METADATA` handle is not enough. This matches 665 and differs from 663.

`name` must be exactly one component — no `/`, neither `.` nor `..`, non-empty,
≤ 255 bytes — for every name argument, **including both of `linkat`'s**. We
check the destination name as well as the source, because the destination is
where the new entry lands and is the more dangerous half to leave unchecked.

## 2. The surprise: a symlink target is *not* a single component, and must not be

`SYS_FS_SYMLINKAT_PINNED` takes two counted byte strings and treats them
completely differently:

| | rule |
|---|---|
| `arg1`/`arg2`, the **link name** | exactly one component, or `InvalidArgument` |
| `arg3`/`arg4`, the **target** | anything non-empty up to `PATH_MAX` — `..`, `/`, absolute, dangling, all fine |

This is not an oversight and please do not add a check of your own on the way
in. The single-component rule exists to guarantee that the object being
*created* is inside the pinned directory, which is a statement about a name. A
symlink target is not a name being created — it is text stored verbatim and
interpreted only when something later walks through the link, at which point
the ordinary traversal checks apply exactly as they do for a link made by the
path-based route. Constraining it would secure nothing and would leave
`symlinkat` unable to reproduce the relative links (`../lib/libfoo.so`) that any
real tree is full of, which is the entire reason you asked for the call.

The one thing we do refuse is a **zero-length** target, as `InvalidArgument`: a
link to nothing is not a link, and storing it would only defer the failure.

## 3. `mkdirat` takes nine mode bits, `fchmodat` takes twelve

Yes, on purpose, and yes, it is the one place in the family where two calls
treat `mode` differently.

- `SYS_FS_FCHMODAT_PINNED` (665) masks to `0o7777` — §639, because silently
  dropping a requested setuid bit is the worst way for a permission request to
  fail.
- `SYS_FS_MKDIRAT_PINNED` (666) masks to `0o777`, matching `SYS_FS_MKDIR_MODE`
  (660), which you are already calling.

A directory that is setgid or sticky *from the instant it exists* is a policy
decision, and the caller can still make it with a following `fchmodat` where it
is a separate and separately-auditable request. Consistency with the `mkdir` you
already use beat consistency with the `chmod` you also use; if that is the wrong
call for you, say so and we will widen it — 666 has no callers yet, which is the
only moment it is free.

As with 665: **mode bits above the mask are ignored, flag bits above the mask
are `InvalidArgument`.** `arg4` on `mkdirat` must be exactly `0`. `mkdirat(2)`
defines no flags, and refusing junk now is what keeps the argument usable if one
is ever defined — a caller tolerated today would break on the day the bit means
something.

## 4. `linkat` has no flags argument, so it cannot follow a symlink

All six registers are spent: two handles, two pointers, two lengths. We did not
add a struct argument to escape that, because the constraint and the right
answer coincide.

`AT_SYMLINK_FOLLOW`'s effect is to hard-link a symlink's *target* instead of the
symlink. Following is, by definition, leaving the pinned directory — so the flag
asks 668 to stop providing the one guarantee it exists for. A caller that
genuinely wants the followed form wants the path-based route, where it was never
getting a guarantee anyway.

**So 668 is always the unfollowed form**, which is `link(2)`'s behaviour and
`linkat`'s own default. If you translate `linkat(..., AT_SYMLINK_FOLLOW)`, route
it to the path-based `SYS_FS_LINK`, not here.

The VFS primitive underneath does take a `follow` parameter and
`AT_SYMLINK_FOLLOW_PINNED` (`0x400`) is defined, so this is reversible without
an ABI change to anything shipped. If you find a real caller, file it.

## 5. Two error codes that are not what POSIX would give you

- **Cross-mount `linkat` is `InvalidArgument`, not `CrossDevice`/`EXDEV`.** The
  path-based `link` has always answered this way and you already translate it;
  making the pinned route differ would give one operation two error contracts
  depending on which route ran. If you would rather both said `CrossDevice`, we
  will change both at once — but not one of them.
- **`utimensat` uses zero-means-unchanged, not `UTIME_OMIT`/`UTIME_NOW`.** That
  is this kernel's existing convention (`SYS_FS_SET_TIMES`), and you already
  translate the sentinels for the path-based call. Making the pinned variant
  differ would mean two conventions for one operation. `UTIME_NOW` is yours to
  expand into a real timestamp, as it is today.

## 5a. One change to a call you already ship: `SYS_FS_LINK` can now say `PermissionDenied`

Writing 668 surfaced a hole in the **path-based** `SYS_FS_LINK`, so this is a
behaviour change to something you are already calling, not just to the new
number. Flagging it rather than burying it in a design note.

Hard links were skipping the sandbox path-policy check entirely — the one that
decides which files a restricted process may go near (ACLs and capability
tags). Every other mutation in the VFS ran it; `link` did not, because all its
work is in a private back-end that the gate script only checks `pub fn`s
against. So a sandboxed process could give a file it was forbidden to open a
second name inside a directory it *was* allowed to open, and read it there.

Now gated, on both routes identically:

| | gate |
|---|---|
| the new name | `Write` — an entry is being created there |
| the existing file | `Read` — afterwards the caller reaches it under a name they chose |

**`Read` on the source, deliberately not `Write`.** A rename removes the source
name and so needs `Write`; a link does not touch it, only its link count.
Requiring `Write` would forbid hard-linking a file you may read but not modify,
which is the main honest use of hard links — `cp -l`, dedup backup, and any
content-addressed store.

**What you will see:** `PermissionDenied` from `SYS_FS_LINK` and 668 where a
link previously succeeded, and only when an ACL or a file tag actually exists —
the check returns immediately when none are configured anywhere, which is every
boot today. If you have a translation that assumed `link` could not fail this
way, it can now. Rationale in `design-decisions.md` §659.

## 6. One thing 666 does that the path route does not

`mkdirat` stamps the requested mode **under the same filesystem lock that
created the directory**. The path-based `SYS_FS_MKDIR_MODE` creates with the
filesystem's 0o755 default and chmods in a second, separate acquisition — so a
directory you asked for as `0o700` is briefly world-readable *and openable* by
anyone watching. That window is closed on 666 and still open on 660.

If you have a caller that cares about the window (anything creating a directory
for secrets — `~/.ssh`, a private temp dir, a per-user cache), 666 is strictly
better than 660 even when you have no `dirfd` to speak of, and it is worth
opening the parent to get a handle. We have not fixed 660 because doing it there
means restructuring `mkdir_mode`'s lock discipline; say the word if you want it
and it moves up.

## 7. What is still missing for a race-free `cp -r`

Being straight with you rather than claiming more than we did: **the destination
side is now closed and the source side is not.**

`SYS_FS_GETDENTS_PINNED` (664) exists and you have not wired it yet — you said
so in §5. Until you do, a recursive copy still re-derives its *source* directory
by name on every entry, so the walk itself is racy. That is a read-side race
(you read the wrong file) rather than a write-side one (you create a file
somewhere you never named), which is why we did the destination first, but the
set is not complete without it.

`renameat` is not here. You asked for it last and it is next on our list.

## 8. Testing

There is a new self-test section in `vfs::self_test` (`[vfs] pinned
mkdir/symlink/link/utimens`), which builds the swap and asserts the part a
return value cannot tell you: after all four calls are refused with `ESTALE`,
**the impostor directory is empty**. That is the real assertion — a primitive
that reported `ESTALE` after already creating the entry would satisfy every
return-value check and fail at the only thing that matters. It also pins the
target/name asymmetry in both directions, `linkat` sharing an inode rather than
copying, and `utimensat` leaving a zero argument alone.

It is gated on `/tmp` being mounted and records itself in the skip list if not,
so a boot without `/tmp` will say so rather than silently passing.

---

**Ours:** `kernel/src/fs/vfs.rs`, `kernel/src/syscall/{number,handlers,dispatch}.rs`.
**Yours:** whatever calls these.
