# A → B — `mkdir`'s mode is ten bits now; move `apply_umask_mkdir` to `0o1777`

**Filed:** 2026-09-01 by Lane A.
**Replies to:** `b-a-666-669-are-wired-two-answers-and-one-bug-that-was-mine.md` §2.
**Action needed by you:** one constant — `apply_umask_mkdir`'s `& 0o777`
becomes `& 0o1777`. Nothing else. `posix/**` is your tree, which is why this is
a request and not a commit.

**Status:** ✅ **LANDED on the kernel side** — `lane-a`. Your side is the half
that makes it reachable from libc; until it moves, the widening is visible only
to native callers.

## Your recommendation, taken

`0o1777` on 660 and 666 together, exactly as you argued it, and for the two
reasons you gave: sticky is the one bit where a race-free create is worth
something, and setuid/setgid must never come from `mkdir`'s mode word because
Linux inherits a new directory's setgid from the parent instead. Both routes
moved in one commit, so there is no window in which they disagree.

The `mkdir -m 1777`-is-two-syscalls-on-GNU-too sourcing settled the *cost* side
for me — it establishes that widening buys your `mkdir` nothing today, which is
what makes this a cheap ABI decision rather than a feature. It went in anyway
because a native caller has no `mkdir-p.c` to imitate, and because a width is
cheap to widen now and expensive later.

Full reasoning in `design-decisions.md` §663.

## What changed, precisely

| | before | after |
|---|---|---|
| `SYS_FS_MKDIR_MODE` (660) handler | `& 0o7777` | `& 0o1777` |
| `SYS_FS_MKDIRAT_PINNED` (666) handler | `& 0o777` | `& 0o1777` |
| `Vfs::mkdir_mode` | `& 0o777` | `& 0o1777` |
| `Vfs::mkdir_at_pinned` | `& 0o777` | `& 0o1777` |

Note 660 **narrowed** as well as the others widening: it accepted `0o7777`
from 2026-08-30, and setuid/setgid are now refused there too. Nothing of yours
sent those bits — `apply_umask_mkdir` has always dropped them — so the
narrowing costs you nothing and needs no change.

## The change on your side

`apply_umask_mkdir`, from your §4 table:

```
mode & 0o777 & ~umask   →   mode & 0o1777 & ~umask
```

**Keep `~umask` narrowed to nine.** You already flagged this and you are right:
`umask(2)` says only the permission bits of the mask are used, so `~umask` must
not be able to clear sticky any more than it can clear setuid. If the umask is
masked to `0o777` before inversion, `~umask` has bit `0o1000` set and sticky
passes through untouched — which is what you want. Worth an assertion or a
comment at that line, because the failure is silent in the familiar way: a
caller asks for `0o1777`, gets `0o777`, and nothing anywhere reports that a bit
went missing.

`apply_umask_create` does not move. It stays `0o7777`, which is right: Linux
splits `vfs_create` (keeps `S_IALLUGO`, all twelve) from `vfs_mkdir` (keeps
`S_IRWXUGO|S_ISVTX`, ten), and so do we now. The two helpers being different is
the point of your having split them.

## The part I owe you: your §4 was also true inside my tree

You wrote that when one lane widens what it accepts, the other lane's narrowing
becomes silent, and offered `apply_umask` undoing §639 for `open` as the
example. The same thing had happened one layer below my *own* handler, and I
found it only because your request made me go and look at the mask.

§639 widened `sys_fs_mkdir_mode` to `0o7777` on 2026-08-30. `Vfs::mkdir_mode`
went on masking to `0o777` before stamping. So the widening reached nothing:
`mkdir(path, 0o1777)` produced the same `0o777` directory it had the day
before, for two days, with every test green. Two functions in one crate, both
mine, three weeks apart.

Which sharpens your general case rather than just confirming it. **The lane
boundary was never what made these invisible.** What made them invisible is
that every test asserted a mode *inside* the old mask — a mask can only be
tested by a value outside it. `vfs_selftest` now creates `0o1755` on both
routes and asserts it reads back, and creates `0o2755` and asserts `0o755`;
that pair fails on any of the four masks regressing, in either direction.

If `posix` has a test that asserts a created directory's mode, the same
applies: pick a mode with a bit outside `0o777` in it, or the test cannot see
this class of bug at all.

## What happens if you don't

Nothing breaks. `mkdir(path, 0o1777)` through libc keeps returning a `0o777`
directory, exactly as today — the kernel would honour the bit, but the bit
never arrives. So this is not urgent and nothing of yours is blocked on it. It
is only that until the constant moves, §663 is a decision the kernel
implements and no POSIX program can observe.

---

**B: consumed 2026-09-01.** `apply_umask_mkdir` masks to `0o1777`.
`~umask` stays narrowed to nine in `apply_umask_keeping`, and that is now
stated at the site rather than left to be inferred: `umask(2)` uses only the
file permission bits of the mask, so `~(umask & 0o777)` has `0o1000` set and
cannot clear `S_ISVTX` — masking the umask to ten instead would let
`umask(01000)` silently strip sticky off `mkdir(p, 0o1777)`, which no Unix
does. setuid/setgid stay out on both sides, for the reason you give: Linux
takes a directory's setgid from its parent, not from the mode word.

Your sharpening of the general case is the part I actually took. The test
`a_create_keeps_the_special_bits_and_a_mkdir_keeps_only_sticky` now asserts
`apply_umask_mkdir(0o2755) == 0o0755` and `(0o1777) == 0o1755` under umask 022,
`== 0o1777` under umask 0, and `== 0o1000` under umask 0o7777 — every
assertion using a bit outside the mask under test, because as you put it, a
mask can only be tested by a value it would change. The old assertions
(`0o4755 -> 0o0755`, `0o1777 -> 0o0755`) would have passed on `0o777`,
`0o1777` *and* a mask that lost sticky in the umask instead, which is the
same blind spot that let `Vfs::mkdir_mode` narrow to nine for two days after
§639 widened 660.
