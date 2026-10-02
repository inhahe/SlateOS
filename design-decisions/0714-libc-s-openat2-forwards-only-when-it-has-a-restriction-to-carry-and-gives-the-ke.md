## 714. libc's `openat2` forwards only when it has a restriction to carry, and gives the kernel a handle on *its own* working directory rather than the number that means the kernel's

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B

**In short:** SlateOS's C library has a call, `openat2`, that opens a file and
can be asked to keep the search confined to one directory (so a path full of
`..` cannot climb out of it). Until today the library had no way to ask the
kernel for that, so it refused the request outright. The kernel now has the
call, so the library forwards. Two choices inside that forwarding are not
obvious, and both are about *not* taking the shorter route: the library still
uses the old ordinary path when no confinement was asked for, and it never uses
the kernel's shorthand for "my current directory", because the kernel's idea of
the current directory and the library's are two different directories.

Answers `requests/a-b-openat2-is-661-and-the-mode-is-twelve-bits.md`. The
counterpart on lane A's side is §639.

### Decision 1 — forward only a non-empty `resolve` word; delegate the rest to `openat`

`plan_resolve` returns one of three things, and `Delegate` is the common case:
a `resolve` word of 0, or one containing only `RESOLVE_NO_XDEV` /
`RESOLVE_NO_MAGICLINKS`, still goes to plain `openat`.

*The alternative*: forward everything, so `openat2` has exactly one path to the
filesystem.

That is the tidier shape and it was rejected, because `openat` is not a thin
wrapper — it is where `/dev/ptmx` and `/dev/pts/<n>` are answered inside libc,
where `O_TMPFILE` is refused, where the umask is applied, and where the fd is
registered with its stored path. Routing every `openat2` through the native
call would have silently removed pty support from `openat2`, which nothing
asked for and no test would have caught. The cost of the choice is real and
should be stated plainly: there are now two paths from `openat2` to the
filesystem, and they can drift. What holds them together is that the forwarding
path re-runs `openat`'s gates in `openat`'s order (`validate_open_flags`, then
the NULL check, then the `O_TMPFILE` refusal) and ends with a copy of `open`'s
fd registration. A third path would not be acceptable; a second is, because the
second exists to carry something the first structurally cannot.

### Decision 2 — `AT_FDCWD` opens a scratch handle on libc's cwd; it does not pass `dirfd == 0`

The native ABI reads handle 0 as "the process working directory". Using it for
`AT_FDCWD` is one line and looks exactly right.

It is wrong here, and the reason is a divergence that is not written down
anywhere else: `unistd::chdir` keeps this libc's working directory in a
libc-side buffer and **never tells the kernel**. The kernel's cwd for the
process is therefore whatever it was at spawn, and after any `chdir` the two
disagree. Confining a walk beneath the kernel's cwd when the caller meant
libc's is a containment check against the wrong base — and it fails *open*: a
wrong base still returns a valid descriptor and still looks like working
containment. That is the exact failure mode `TD-OPENAT2-BENEATH-INROOT` was
opened for, so reintroducing it in the fix for it was not acceptable.

So for a relative path with `AT_FDCWD`, libc opens its own cwd with
`O_DIRECTORY`, uses that handle as the base, and closes it on every exit path.
The cost is one extra open and close per confined `AT_FDCWD` call.

*The alternative considered and rejected*: make `chdir` tell the kernel, so the
two cwds agree and 0 becomes usable. That is the better long-term answer and it
is a bigger change than this one — it makes every `chdir` a syscall, and it has
to decide what happens when the kernel refuses a directory libc accepted. It
should be done, but not inside a change whose job is to stop refusing
`RESOLVE_BENEATH`; doing it here would have meant shipping a cwd-semantics
change hidden inside an `openat2` change. Logged as the remaining half.

`dirfd == 0` *is* passed for an absolute path, and the justification is not
"the kernel ignores it" but something provable: without `BENEATH` the handler
takes the absolute fragment as the whole answer, and with `BENEATH` it refuses
an absolute fragment *before* the handle lookup. The base is unread on both
branches, so there is no value that could be wrong.

### Decision 3 — the bit translation is tested as a pure function, not end to end

Every native syscall is stubbed off-target, so a host test cannot open
anything. An end-to-end assertion about `RESOLVE_BENEATH` therefore reduces to
"the stub returned `ENOSYS`" — which is also what a *deleted* translation would
produce. The five tests that pinned the old refusals could not be inverted into
"must succeed" for the same reason.

`plan_resolve` is consequently a pure function over the `resolve` word, and the
tests assert its output directly, including that every Linux resolve value is
`< 0x40` while both kernel values are `>= 0x40` — the divergence lane A made
deliberate, now pinned from both sides rather than only in
`test_dispatch_openat2_native`. The judgement being recorded is that a testable
seam is worth an extra type (`ResolvePlan`) in code that would otherwise be a
short `if` chain: the alternative was an untestable `if` chain, which is what
lane A's request warned would become untested marshalling under every program
that reaches for a contained open.
