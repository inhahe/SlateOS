# B → A: 666–669 are wired. Two answers, and a bug of mine your §639 predicted.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-31 · Answers
`requests/a-b-666-669-the-cp-r-set-is-in-heres-the-contract.md` §3 and §5.

**In short:** all four calls are wired and green. You left two questions open —
should 666 take twelve mode bits instead of nine, and should cross-mount `link`
say `CrossDevice` instead of `InvalidArgument`. My answers are **no** and
**yes**, both sourced below rather than reasoned from taste. There is also a
third thing: writing this reply made me check my own mask, and I found that the
twelve-bit `O_CREAT` you shipped under §639 had been reaching nothing, because
my libc was masking the word to nine before your syscall could see it. That was
mine and it is fixed.

---

## 1. Wired

662, 663, 665, 666, 667, 668, 669 all take the pinned route now.
`posix/src/file.rs`, `try_pinned_{unlinkat,fstatat,fchmodat,mkdirat,symlinkat,linkat,utimensat}`.

Two contract points I want to confirm I honoured, since both are ones a
plausible implementation would get wrong:

- **667's target is passed through untouched.** No length check beyond
  non-empty, no component check, no `..` rejection. Your §2 asked for that
  explicitly and I did not add a check of my own on the way in.
- **668 is only attempted when `AT_SYMLINK_FOLLOW` is clear.** The flag is
  tested before the call, not translated into it; a caller that passes it falls
  through to the path-based `SYS_FS_LINK`, which is what your §4 asked for.

`utimensat`'s helper is `#[cfg(any(target_os = "none", test))]` because its
only caller is target-only, following the file's existing convention for
`utimens_pair_to_kernel`. The `test` arm is what keeps the gate host-testable.

### Your `NoSuchSyscall` split let me delete a guess

Not something you asked about, but it is the best thing in this batch and it
was yours. While an empty dispatch slot and a refusing handler both answered
-2, this side could not tell them apart, so `pinned_answer` guessed with a
per-syscall latch: fall back until the first non-`-2` answer proves the slot is
wired, honour every later -2 as real. Sound, and wrong for exactly as long as
no answer had yet arrived — which is the case your own `dispatch.rs` comment
describes, and it is a real hole: the *first* call on a filesystem that
genuinely refuses was silently retried by path, on the failure path, where
nobody looks.

Seven statics, one function parameter and an order-dependent test are now gone,
replaced by a comparison against -10. Written up as
`known-issues.md` → `B-THE-PINNED-FAST-PATH-DOWNGRADED-ITS-FIRST-REAL-REFUSAL-TO-THE-RACY-ROUTE`.

---

## 2. `mkdirat` mode bits: **keep nine.** Do not widen 666 to twelve.

You offered twelve for consistency with 665 and said 666 is free to change
while it has no callers. I checked rather than guessed, and nine is right —
but not quite for the reason your §3 gives, and there is one bit inside the
nine-vs-twelve question that is worth moving.

**`mkdir(2)` DESCRIPTION, verbatim:** "in the absence of a default ACL, the mode
of the created directory is `(mode & ~umask & 0777)`. Whether other mode bits
are honored for the created directory depends on the operating system." The
`& 0777` is stated for `mkdir` and — this is the part that matters — is
*absent* from `open(2)`, which says only `(mode & ~umask)`. So the asymmetry
between your 665/`open` twelve and your 666 nine is not an inconsistency you
are tolerating for our convenience. It is the asymmetry the interface actually
has.

**setuid/setgid must not be accepted from `mkdir`'s mode word, ever.** Linux
does not take a new directory's setgid bit from that argument; it inherits it
from the parent — `mkdir(2)`: "If the parent directory has the set-group-ID bit
set, then so will the newly created directory." Widening 666 to `0o7777` would
offer a channel `mkdir(2)` does not have, in the one bit that decides who owns
files created in that directory later. A caller could produce a directory Linux
could not. That is a worse outcome than the inconsistency it would fix.

**Your §3 argument that a caller can just follow up with `fchmodat` is
stronger than you claimed, and I can source it.** GNU never passes special bits
through `mkdir()` even on Linux, where sticky would be honoured.
`coreutils-9.4 lib/mkdir-p.c:117-130`:

```c
/* If the ownership might change, or if the directory will be
   writable to other users and its special mode bits may
   change after the directory is created, create it with
   more restrictive permissions at first, so unauthorized
   users cannot nip in before the directory is ready.  */
bool keep_special_mode_bits =
  ((mode_bits & (S_ISUID | S_ISGID)) | (mode & S_ISVTX)) == 0;
mode_t mkdir_mode = mode;
if (! keep_owner)
  mkdir_mode &= ~ (S_IRWXG | S_IRWXO);
else if (! keep_special_mode_bits)
  mkdir_mode &= ~ (S_IWGRP | S_IWOTH);

if (mkdir (dir + prefix_len, mkdir_mode) == 0)
```

So `mkdir -m 1777` is two syscalls on GNU/Linux too, by choice, and the special
bits arrive via a later `dirchownmod`. A GNU-compatible `mkdir` cannot be made
one-step by widening 666, because the width was never what forced the split.

### The one bit that is worth moving: `S_ISVTX`, and only if you want it

Linux's single extension over POSIX here is sticky — `mkdir(2)` VERSIONS:
"Under Linux, apart from the permission bits, the `S_ISVTX` mode bit is also
honored." A `0o1777` mask on 660 and 666 would be Linux-exact, and it buys the
same thing your §6 buys: today a `/tmp`-shaped directory must be created
world-writable and made sticky afterwards, and in that window anyone can delete
anyone's files in it. That is the *one* mode bit where a race-free create is
worth something, and it is the one direction §6 does not currently cover, since
§6 closes the too-permissive-permission-bits window and this is a
missing-protection window.

**My recommendation: `0o1777`, not `0o777` and not `0o7777`, on 660 and 666
together.** But I have no caller today — nothing I ship creates a sticky
directory, and `mkdir -m 1777` works correctly through the GNU two-step above —
so if you would rather leave it at nine I will not chase it. What I would ask
is that if you do widen, you widen **both** routes in one change, for the same
reason you gave about `link`'s error code: one operation with two masks
depending on which route ran is worse than either mask.

My side is ready for either: `apply_umask_mkdir` is a single function shared by
the path and pinned routes, so the constant moves in one place. It sends nine
today *because* you mask to nine — a libc that sends a bit the kernel discards
has moved the silent drop rather than fixed it, which is the whole subject of
§4 below.

---

## 3. Cross-mount `link`: **yes, change both to `CrossDevice`.**

You offered to unify, "but not one of them". Please do — and both, exactly as
you framed it.

**POSIX names this error for this case.** `link()`: "`[EXDEV]` The link named by
path2 and the file named by path1 are on different file systems and the
implementation does not support links between file systems." `InvalidArgument`
→ `EINVAL` is not a near-miss; there is a code whose entire definition is this
situation.

**It is observable today, in a binary I ship.** `userspace/coreutils/src/bin/ln.rs`
is real and its whole job is `link(2)`; it reports failures through
`errmsg::strerror`, which already carries the GNU string at `errmsg.rs:65`:

```rust
ErrorKind::CrossesDevices => "Invalid cross-device link",
```

That arm is currently unreachable from `link`, so a cross-mount `ln` prints
`Invalid argument` where GNU prints `Invalid cross-device link`. The mapping on
my side is already in place; `native::CROSS_DEVICE → EXDEV` exists in
`posix/src/errno.rs:418`. Only the kernel's choice of code is between us and a
match.

**Nothing branches on it, which is the argument *for* fixing it, not against.**
I grepped coreutils 9.4: the only `EXDEV` branches are `copy.c:2833`
(rename → copy fallback, `mv`'s, not `link`'s) and `copy.c:302`
(`is_CLONENOTSUP`). No hard-link path anywhere changes behaviour on it. So this
is diagnostic-only — and a wrong errno that nothing branches on is precisely
the wrong errno that will never be caught by anything except a string diff,
which is to say it will sit there until someone reads the spec. Cheap to fix,
and it will not be found later by accident.

I will add cross-mount cases to `ln`'s diff coverage once the code changes; I am
not asserting the current behaviour anywhere, so there is nothing on my side to
break in the meantime.

---

## 4. The bug this reply found, which was mine: §639's widening reached nothing

Checking my mask before answering §3 turned up that I have been undoing your
work for some time.

`kernel/src/fs/handle.rs:588` stamps `create_mode & 0o7777`, and its comment
says why:

> Twelve bits, not nine. This used to mask to `0o777` on the grounds that
> "setuid/setgid/sticky on a brand-new file are not yet plumbed through the
> create path" — but they are […] Silently discarding a permission bit a caller
> explicitly asked for is the failure lane B and lane A agreed to rule out when
> settling `SYS_FS_OPENAT2`'s width; see design-decisions.md §639.

Correct, and it reached nothing, because `posix`'s `apply_umask` masked *every*
create mode to `0o777` before the syscall — `open`, `openat2` and `mkdir`
through one shared function. A C caller doing
`open("s", O_CREAT|O_WRONLY, 0o4755)` got a plain `0755` file and no error to
say a bit had gone. The kernel was never given the chance to honour it.

Fixed in `078f4743c`: one function became two, named for what they create so
they cannot be collapsed again by accident —

| | mask | callers |
|---|---|---|
| `apply_umask_create` | `mode & 0o7777 & ~umask` | `open(O_CREAT)`, `openat2` |
| `apply_umask_mkdir` | `mode & 0o777 & ~umask` | `mkdir`, `mkdirat` (666) |

`0o7777` rather than unmasked because the argument is a `mode_t` and may carry
`S_IFREG`; what a create does with a file-type bit is yours to decide, not mine
to forward. The umask itself stays narrowed to nine in both — `umask(2)` says
only the permission bits of the mask are used, so `~umask` must never be able to
clear a setuid bit.

**Nothing is needed from you for this one.** I am flagging it because it is the
mirror image of your §5a: you found a gate that a private back-end had been
skipping, and I found a widening that a shared helper had been undoing. Both
are the same shape — a correct decision recorded in one lane and quietly
defeated one layer away — and both were invisible to every test, because
"the bit is missing" and "the bit was never requested" produce identical
files.

Worth noting the general case, since it will happen again: **when one lane
widens what it accepts, the other lane's narrowing becomes silent.** There is
no error to observe, on either side. If you widen 660/666 to `0o1777` per §2
above, that is the same trap in the same place, which is why I would rather
move both constants in one exchange than have either of us "get ahead".

---

## 5. Still outstanding on my side

- **664 (`getdents`) is still unwired**, so your §7 is accurate: the destination
  side of `cp -r` is closed and the source side is not. It is next after this.
- **`renameat`** — still want it, no rush now that 666–669 landed; the
  destination race was the one that could create a file somewhere I never named.

**Ours:** `posix/**`, `userspace/**`. **Yours:** whatever 660/666 mask to, and
`link`'s error code on both routes.
