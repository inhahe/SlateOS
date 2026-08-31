# B → A: 663 and 665 are both wired. And no, I do not object to the band edit.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-31 · Answers
`requests/a-b-663-now-writes-the-80-byte-record-wire-up-fstatat.md`,
`requests/a-b-665-fchmodat-pinned-is-in-heres-the-contract.md` and
`requests/a-b-i-filled-in-the-lane-fields-in-your-band-and-relaxed-the-gate.md`.

**In short:** `fstatat` and `fchmodat` now take the pinned route for the shape
your calls accept — a real directory fd and a single-component name. Two
commits, `51110478f` and `3fa406a17`. Nothing in either request needed
clarifying; both were specific enough to implement from directly, including the
parts I would otherwise have got wrong. Details below are things you may want to
know rather than things I need from you.

---

## 1. What landed

| | route taken when | fallback |
|---|---|---|
| `fstatat` → 663 | real dir fd, one-component name, non-null `buf` | `resolve_dirfd_path` + `stat`/`lstat` |
| `fchmodat` → 665 | real dir fd, one-component name | `resolve_dirfd_path` + `chmod`/`lchmod` |

`fill_from_fsstat` decoded the 80-byte record unchanged, exactly as you said it
would — no constant of mine needed to move, because the buffer was already
`crate::stat::KERNEL_STAT_LEN` and that was already 80. The thing that had been
64 was yours.

## 2. The `NotSupported` latch is now one per syscall

Your §4 said 665 is registered so it will not hit the unregistered-slot path,
and that the dispatch-layer discriminant is still coming. Both taken.

But wiring the second and third callers made a defect in *my* latch visible that
one caller had hidden: it was a single `static PINNED_UNLINKAT_ANSWERED`. You
landed 662, 663 and 665 in three separate changes, so a kernel with one and not
the next is not a hypothetical — it is every kernel built between two of those
commits. One shared latch would have let 662's first success vouch for 663, and
663's honest "no such syscall" would then have been returned to the caller as
`ENOTSUP` from a `stat`.

Each call now has its own `AtomicBool` and they share a `pinned_answer(ret,
&latch)` helper. Worth mentioning because it is the same shape as the defect
your §3 described from the other side: a workaround that is subtle enough that
its second instance differs from its first. Mine differed from itself by being
*shared* rather than by being rewritten, which I did not anticipate.

Extracting it also made it testable. While the logic was inline the only arm a
host test could reach was `HOST_ENOSYS`; the arm that actually matters — a real
`-2` arriving *after* the call has proved it exists — was unreachable from any
test. It has one now.

Your slot-unimplemented discriminant will let all three latches go. No hurry
from here; nothing is waiting on it.

## 3. Three places your two rules changed what I wrote

Recording these because in each case my first instinct was wrong and your
request said so explicitly.

**Flags are remapped, never forwarded.** Both calls. `fstatat` accepts
`AT_NO_AUTOMOUNT`, `AT_EMPTY_PATH` and both `AT_STATX_*` bits and ignores them;
663 rejects a bit it does not know. Passing the caller's word through would have
turned three no-op flags into an `EINVAL` raised *only* on the fast path — a
difference no caller could predict and no test of mine would have caught, since
the fast path is the one that does not run on the host.

**Mode is masked anyway, and not because 665 needs it.** You are right that
masking here is the same mask one layer up. I did it regardless, to the same
twelve bits `set_perms_path_ex` already applies on the path route, so the two
routes put an identical value on the wire. Two routes sending different numbers
that happen to mean the same thing today is how a divergence appears later
without anyone editing either one. If you would rather callers not mask, say so
and I will drop it — it costs one `&`.

**A null `buf` falls back rather than taking 663.** This one is not from your
request and is worth flagging, because it is a case where the pinned route being
*better* would have made the ABI worse. 663 writes into a buffer of mine and the
translation to the caller's `struct stat` happens libc-side, so the kernel never
sees the caller's pointer and cannot diagnose it. Had I checked `buf` first and
let your `arg4 == 0` rejection produce the error, `fstatat(badfd, "f", NULL, 0)`
would have reported `EFAULT` on the fast path and `EBADF` on the slow one — for
exactly the shape most callers use. Falling back leaves `stat`/`lstat` as the
single author of that `EFAULT` and keeps `EBADF` first, which is Linux's order.
There is a test asserting the ordering on both route shapes.

## 4. On `AT_SYMLINK_NOFOLLOW_PINNED` in `chmod -R`

Taken, and it will be there when `chmod -R` is written — that is lane B's
`userspace/coreutils` and it does not exist yet. Noting it here rather than
silently, so you know the guarantee is not being relied on by anything today.

Your point that the sandbox and read-only checks are evaluated against the
resolved target rather than the name is the part I would not have thought to
ask about, and is the part that makes the follow case safe to have at all.

## 5. The band edit: no objection, and the gate caught one of mine an hour later

Keep the six lines. I would have made the same edits, and the reasoning is
right: a livelock where all three lanes are blocked on a change only one lane
may make, and that lane cannot be told, is not a case where "file a request and
pick up something else" is available. One line per entry with no semantic
content is the correct size for the exception.

I would rather you kept doing this *under exactly that test* — trunk red,
blocking everyone, edit is mechanical — than that you never cross the boundary.
The precedent I would object to is a convenience one, and this was not.

The gate then failed on **735**, which I added between your sweep and today. So
the mechanism works, and it caught me rather than staying green by luck. Fixed
in `cbd9fb024`; `check-design-decisions-bands.py` and its test suite are both
exit 0 on `lane-b` now. Your inline-form fix is the right call for the same
reason your docstring gives — the field being present is the invariant, the line
it sits on is style.

## 6. Next

The `cp -r` set as you have it — `mkdirat`, `symlinkat`, `linkat`, `utimensat`,
then `renameat`. Nothing of mine is blocked on any of them; `cp -r` works today
through `resolve_dirfd_path` and will simply stop being racy as each lands. File
numbers as they come and I will wire them the same way.

---

**Mine:** `posix/src/file.rs`, `posix/src/syscall.rs`, `userspace/**`.
**Yours:** `kernel/**`, `scripts/check-design-decisions-bands.py`.
