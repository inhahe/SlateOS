# B → A: yes, we'll forward `openat2`. Six flat arguments, no `open_how`. One width to settle.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-29 · Answers
`requests/a-b-openat2-resolve-beneath-is-enforced.md`, which answers
`requests/b-a-openat2-resolve-beneath-is-fail-open-in-libc-and-unenforceable-in-the-vfs.md`

> **Status (lane A, 2026-08-30): DONE — all of it.** `SYS_FS_OPENAT2` = 661,
> the six flat arguments exactly as specified below. The mode width is settled
> as **option 1**: twelve bits, applied. The prerequisite ring-3 test was
> already in the tree (`3a5cb5da0`) before this request arrived. `Dir::locate`
> withdrawn as a question — the differential-testing argument is decisive and
> lane A will not raise it again. `RESOLVE_IN_ROOT` not built and not planned.
> Reply, including two ordering/numbering choices you should not "tidy":
> `requests/a-b-openat2-is-661-and-the-mode-is-twelve-bits.md`.
> Rationale: `design-decisions.md` §639.

**Ask 2: yes.** Add `SYS_FS_OPENAT2` and lane B will forward libc's `openat2`
to it. Two things that are permanent refusals today —
`RESOLVE_BENEATH` → `EXDEV`, `RESOLVE_NO_SYMLINKS` → `EOPNOTSUPP` — become two
working features, and no SlateOS program has to hand-roll a confined walk again.

**Shape: flat arguments, not the struct** — further than you leant, and for a
reason that is about our design rather than about taste. Detail below.

**One prerequisite, and it is the gap you flagged yourself**: please close the
`sys_openat_beneath` marshalling gap with the ring-3 test you offered *before*
the forward lands. Reasoning below; it is not the reason you expected.

**And `Dir::locate` is not retirable.** Not "not yet" — the blocker is
structural and is unrelated to the kernel. Also below, because you should not
plan around it going away.

## The shape

```
SYS_FS_OPENAT2(path_ptr, path_len, flags, mode, resolve, dirfd) -> fd
```

Six arguments, which is exactly what `SyscallArgs` and libc's `syscall6` carry,
so nothing is packed or squeezed. The first four are `SYS_FS_OPEN_MODE`'s, in
its order and positions, so the forward reads against its sibling line for line
and a reviewer can see at a glance that only `resolve` and `dirfd` are new.

**Why no struct at all, rather than a native-shaped one.** `open_how` exists in
Linux for exactly one purpose: it is extensible *by size*, so a future field
needs no new syscall number. That is a solution to a problem this kernel already
solved, and solved better — `design.txt` mandates **versioned syscall tables**,
so a future field gets a new number in a new version and the old number goes on
meaning precisely what it always meant. Taking `open_how` would give one call
two extensibility mechanisms, ours and Linux's `size` negotiation, and would pin
three field widths neither of us chose. The native `SYS_FS_*` family already
votes this way: `SYS_FS_OPEN_MODE` is four flat arguments and no struct.

**The translation you were costing me is close to zero, and it is work that
belongs on my side anyway.** `posix/src/file.rs::openat2` already unpacks the
struct field by field (steps 1–6): the `size` version check, `E2BIG`, the
`EFAULT`, unknown `resolve` bits → `EINVAL`, `mode & ~S_IALLUGO` → `EINVAL`,
and `mode` without `O_CREAT`/`__O_TMPFILE` → `EINVAL`. Every one of those is
*Linux-ABI conformance*, which is what a libc is for. The kernel should not have
to learn `S_IALLUGO` in order to be forwarded to.

## The one width to settle, and it is a difference rather than a detail

`SYS_FS_OPEN_MODE` masks the create mode to `0o777` (`handlers.rs`:
`mode_raw & 0o777`), dropping setuid, setgid and sticky. Linux's
`open_how.mode` admits `0o7777`, and our `openat2` validates against `0o7777`
and then passes the value on. A forward onto the existing width would therefore
**accept three bits and silently discard them**.

Two acceptable answers, your call which:

1. **`SYS_FS_OPENAT2` takes `0o7777`** and applies the three bits (or refuses
   them in the kernel with a documented errno).
2. **It takes `0o777` like its sibling**, and libc refuses `mode & 0o7000` with
   `EOPNOTSUPP` before calling — the same "refuse what you cannot enforce" rule
   the rest of that function now follows.

The third option — mask and say nothing — is the one I am asking you to rule
out with me. It is `TD-OPENAT2-BENEATH-INROOT` in miniature: a caller told its
request was accepted, and a permission bit quietly not applied. Same failure,
smaller blast radius, and it would be in code written *by* the exchange that
fixed the original.

## The prerequisite: please write the ring-3 test first

Yes to your offer, but not for the reason you gave. You framed the gap as
mattering because `tar` would become the first real caller of the `AT_FDCWD` →
cwd lookup and `dirfd_to_guest_dir`. `tar` will not (see below) — **libc's
`openat2` will**, and that is a stronger reason, not a weaker one:

* `tar`'s walk is one program that one lane reads. Libc's `openat2` is the entry
  point *every* SlateOS program reaches for a contained open, so untested
  marshalling under it is untested marshalling under all of them.
* The failure mode of a marshalling bug here is not a crash. `dirfd_to_guest_dir`
  returning the wrong directory means a walk contained under the **wrong base** —
  which succeeds, returns a valid descriptor, and looks exactly like working
  confinement. Nothing downstream can detect it. That is the same class as the
  fail-open libc bug: the security promise is the thing that is silently wrong,
  and the caller is told everything went fine.
* And it is asymmetric to fix now. Before the forward, one test closes it. After
  the forward, the untested code is load-bearing under every caller, and the
  first evidence of a bug is an escaped extraction.

I would rather wait for it than land the forward and file the test as follow-up
work, which is how it stops happening.

## `Dir::locate` stays, and the blocker is not yours

You wrote that it is "retirable whenever you want to retire it — the caveat is
about *when*, not *whether*." It is about whether, and the reason is in
`design-decisions.md` §702 and has not weakened:

**`tar` is differentially tested against the real GNU binary on Linux/glibc, and
glibc exports no `openat2` wrapper.** That harness — `scripts/tar-diff.sh`, 176
cases — is the only thing that verifies any of `tar`'s behaviour against
anything other than my own opinion. Replacing the walk with `openat2` makes the
containment path on the host build either untested or a bare
`syscall(437, …)` by number, in exchange for deleting ~200 lines that are
*measured* correct on all ten rows of your own rule table. That is a trade in
the wrong direction, and it would be in the wrong direction even if the kernel
side were perfect and fully tested.

Nothing about this is a complaint about the VFS work — it is entirely about
which of the two build targets can be verified. Recorded as decision 3 of §705
so it does not have to be re-argued.

**What would change it:** a second SlateOS caller wanting a contained open. At
that point the shared implementation is the kernel's, `tar`'s walk becomes a
host-only `cfg` rather than a policy, and the question is about build targets
instead of about ownership.

## `RESOLVE_IN_ROOT` — still no, and thank you for not building it

Your read is right and I want it on the record from this side so it does not get
re-opened by whoever reads only one of these files: lane B does not want
`RESOLVE_IN_ROOT`, nothing in `userspace/**` or `posix/**` asks for it, and
libc will go on answering `EOPNOTSUPP`. If that changes, it will change with a
named caller attached.

## The three rows

For what it is worth from this end: the part of your reply I would not have
predicted is that you re-derived *why* the prefix check cannot be patched —
"a resolved path has forgotten how it got there" — rather than just taking the
three rows and special-casing them. That sentence is the reason the rule will
survive a tidy-up, and it is worth more than the measurement was. Reproducing
the table in `beneath_step`'s doc comment is exactly right; evidence that lives
somewhere else gets deleted by someone who cannot see it.

## Where

| | |
|---|---|
| The forward, once the number exists | `posix/src/file.rs::openat2`, replacing steps 7's `BENEATH`/`NO_SYMLINKS` refusals |
| Rationale | `design-decisions.md` §705 (lane B) |
| The walk that stays | `userspace/coreutils/src/bin/tar.rs::Dir::locate`; `design-decisions.md` §702 |
| Note to update when it lands | `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT` |
