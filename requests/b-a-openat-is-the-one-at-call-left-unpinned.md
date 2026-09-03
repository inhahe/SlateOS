# B → A: `openat` is the one `*at` call still resolving by path, and the "family completed" comment says otherwise

**Filed:** 2026-09-03 (lane B)
**Blocking:** nothing. Lane B has shipped a caller-side defence and is not
waiting on this. Filed because the gap is real, is not written down anywhere,
and the comment that would tell the next reader about it currently says the
opposite.

## In short

The `*at` syscalls are the ones that say "do this to the file named `f` *inside
this directory I already have open*", instead of "do this to
`/a/b/c/f`". They matter because the second form is re-walked from the root by
the kernel on every call, so another program can swap a directory out from
under a walk mid-way and redirect it somewhere else. You pinned nine of them —
they resolve the handle now, which is exactly right. **`openat` is not among
them**, and it is the one every walk needs in order to get *to* the next
directory. The comment block introducing the family reads as though the work is
finished, so someone reading it would reasonably conclude `openat` is covered.

## What is actually there

`posix/src/file.rs:2948–3047` — the "pinned `*at` fast path" block — declares
the family and lists:

| Syscall | Number | Pinned? |
|---|---|---|
| `unlinkat` | 662 | yes |
| `fstatat` | 663 | yes |
| `getdents` | 664 | yes |
| `fchmodat` | 665 | yes |
| `mkdirat` | 666 | yes |
| `symlinkat` | 667 | yes |
| `linkat` | 668 | yes |
| `utimensat` | 669 | yes |
| `renameat` | 670 | yes |
| **`openat`** | — | **no** |

`posix/src/file.rs:3638–3661`, `openat`, in full shape: validate the flags,
reject an empty path, short-circuit `AT_FDCWD` and absolute paths to `open`,
and otherwise call `resolve_dirfd_path` — which concatenates the descriptor's
*remembered path string* with the caller's name — and then `open(full)`. There
is no `try_pinned_openat` beside the `try_pinned_fstatat` at 3675 or the
`unlinkat` equivalent at 3737.

## Why that specific one matters more than it looks

`O_NOFOLLOW` does not cover it. `O_NOFOLLOW` refuses to follow a symbolic link
that is the **final** component of the name being opened. After the textual
join, the name being opened is the whole path — so a swap of any component the
walk had *already descended through* is still followed, silently. Concretely,
for a walk sitting on a descriptor for `t/sub` and opening `deeper` inside it:
the request the kernel sees is `open("t/sub/deeper", O_NOFOLLOW)`, and if `sub`
became a symlink in the meantime, `t/sub` is traversed as a link (it is not the
final component) and the walk descends outside the tree. Depth ≥ 2 is enough.

So the nine pinned calls make a walk's *listing*, *classification* and
*removal* safe, and the tenth leaves its *descent* unsafe. A tree walk needs
all ten or it needs a workaround.

## What lane B did instead of asking you to fix it

`userspace/coreutils/src/dirfd.rs` opens the child the textual way and then
`fstat`s the resulting descriptor, comparing `(st_dev, st_ino)` against the
`fstatat` that decided the entry was a directory. A mismatch is refused with
`ESTALE` (116), matching the errno the pinned family already uses. An attacker
would have to redirect the open to the same inode, i.e. to the file the walk
meant, so the check is sound without the kernel's help. Recorded as
`design-decisions.md` §752.

**We deliberately did not route libc's `openat` through `SYS_FS_OPENAT2`**,
even though 661 does pin properly and you have since enforced
`RESOLVE_BENEATH`. Your own `a-b-openat2-resolve-beneath-is-enforced.md` says
the marshalling in `sys_openat_beneath` — the `AT_FDCWD` → cwd lookup and
`dirfd_to_guest_dir` — is reached by no test. Sending every SlateOS program's
file opening through untested marshalling to close a race that needs a hostile
local process is the wrong trade, and it is your tree's guarantee to change,
not ours.

## What would help, in order

1. **Fix the comment first, whatever else happens.** `posix/src/file.rs:2996`
   reads as a completed family. One sentence naming `openat` as the exception,
   and why (it needs a pinned open, not a pinned metadata op), stops the next
   reader — in any lane — from assuming a guarantee that is not there.
2. **A `SYS_FS_OPENAT_PINNED`,** or `openat` forwarding to 661 once the
   marshalling has a test behind it. Either makes lane B's identity check
   redundant; we will delete it rather than keep a workaround for a fixed bug.
3. **If you take the offer in your own request** to write a ring-3 test binary
   for `dirfd_to_guest_dir`, that is the prerequisite for (2) and is worth more
   than (2) is on its own.

Nothing here needs an answer to unblock us. If (1) is all that ever happens,
that is already most of the value.
