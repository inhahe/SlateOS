## 730. `unlinkat` takes the pinned syscall only for the shape it fits, falls back only on "no such syscall", and decides which of those it is by observation rather than by guessing

**Date:** 2026-08-30
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** deleting a file "inside a directory you hold open" was, until
now, done by remembering the directory's *name* and gluing the file's name onto
it. If anything moved that directory in the meantime, the delete landed
somewhere else — possibly somewhere an attacker chose. Lane A added a syscall
that hands the kernel the open directory itself, so there is no name to
subvert. This entry is about the three judgement calls in switching over to it:
which calls get the new route, what happens when the new route is unavailable,
and how we tell "unavailable" apart from "refused".

### Background

`posix::unlinkat(dirfd, "x", flags)` was implemented as
`resolve_dirfd_path` — look up the path `dirfd` had when it was opened, append
`/x`, and call the path-based `SYS_FS_DELETE`. Everything the descriptor exists
to provide is discarded in that step. Lane A's
`requests/a-b-the-at-family-now-has-three-primitives-that-resolve-the-handle.md`
answers it with `SYS_FS_UNLINKAT_PINNED` (662), which takes the kernel handle,
accepts exactly one path component, and verifies inside the filesystem lock
that the handle still denotes the directory it was opened on — `ESTALE` if not.

### Decision 1 — the fast path is opt-*in* by shape, and the fallback is by path

662 accepts a single component only. `unlinkat` may be handed `sub/dir/x`.
Options were:

- **A. Take the pinned route where it fits; otherwise the old join.** Chosen.
- **B. Walk multi-component names ourselves** — `openat2` each intermediate
  directory, pin the last, unlink there.
- **C. Refuse multi-component names.**

C is out: it would break callers for whom the old behaviour was correct, to
defend against an attack they may not face. B is the eventually-right answer
and is where this should go, but it is a different and larger change: it needs
a descriptor per component, an unwind path that closes them all on every exit,
and a decision about depth limits — and it would make `unlinkat("a/b/c")`
issue four syscalls where it issues one today. A gets the security benefit for
the shape that overwhelmingly dominates real callers (`rm -r` and `cp -r` walk
one component at a time by construction) at no cost to the rest.

The cost of A is honest and worth stating: **`unlinkat("sub/x")` is still
racy.** It is not *more* racy than before, but a reader could reasonably assume
that "unlinkat is pinned now" means all of it.

### Decision 2 — only "no such syscall" falls back; every real error is final

A pinned call that returns `ESTALE`, `EACCES` or `ENOENT` has *answered*.
Retrying it through `resolve_dirfd_path` would reintroduce the exact race the
call exists to close, and would do it on the failure path, where no test looks
and no log records it. So the fallback is triggered by exactly one condition —
the kernel does not have the call — and never by the call's own verdict.

This is deliberately fail-*closed*: a kernel bug that makes 662 return
`EACCES` for everything will break `rm` loudly rather than silently downgrade
every deletion to the unsafe route.

### Decision 3 — `NotSupported` is ambiguous, so it is resolved by observation

`dispatch.rs` returns `NotSupported` (-2) for an unregistered syscall slot,
which is what a kernel predating 662 does. But a *registered* handler may also
return `NotSupported` for a filesystem that cannot perform the operation. The
two are indistinguishable in the return value, and they want opposite
handling: the first must fall back, the second must not (decision 2).

Options:

- **A. Treat every -2 as "kernel too old".** Simple, and quietly reopens the
  race whenever a filesystem refuses.
- **B. Treat no -2 as "kernel too old".** Safe, but `unlinkat` then fails
  outright on any kernel without 662 — including, at the time of writing,
  every host build.
- **C. Ask the kernel whether 662 exists.** No such query exists, and inventing
  one is a cross-lane ABI change for a transitional problem.
- **D. Latch on first evidence.** Chosen. A single relaxed `AtomicBool`
  records whether 662 has ever answered with something other than
  `NotSupported`/`-ENOSYS`. Before that, -2 means "fall back"; after it, -2 is
  honoured as the real answer.

D converges correctly in both worlds. On a kernel with 662, the first call
returns success or a genuine error, the flag flips, and every later -2 is
final. On a kernel without it every call returns -2, the flag never flips, and
the fallback stays available forever. The race between two threads both
observing `false` is benign — they each fall back once — so no ordering is
required.

The same test covers the host: `syscallN` compiles out the `SYSCALL`
instruction on a host build and returns `-ENOSYS`, so the flag never flips
there and `unlinkat` under `cargo test` behaves exactly as it did before. That
is why `HOST_ENOSYS` moved from a private host-only constant in `syscall.rs` to
a `pub(crate)` one defined for both targets: it is now *recognised* in
`file.rs`, not only produced, and a second copy of `-38` could drift.

### Decision 4 — flags are translated, not forwarded

662 rejects unknown flag bits with `EINVAL`; the path-based route ignores them.
Passing the caller's word through unfiltered would make a junk flag behave
differently depending on which route ran — a difference no caller could
predict. So only `AT_REMOVEDIR` is inspected, and it is re-emitted as 662's own
constant. (That `unlinkat` ignores unknown flags at all is a separate
divergence from Linux, which returns `EINVAL`; it is unchanged here so that
this commit does not smuggle in a behaviour change.)

Likewise `pinned_base` sets no errno when it declines a `dirfd`.
`resolve_dirfd_path` stays the single author of `EBADF`/`ENOTDIR`, so the fast
path cannot be observed in the diagnostics even if it is wrong about which
descriptors it can handle.

### Not done: `fstatat` on 663

663 writes the 64-byte `FS_META_SIZE` record, which has no inode number, no
hard link count and no block count. `Stat` needs all three, and the 80-byte
record `SYS_FS_STAT` writes has them. A pinned `fstatat` built on 663 would
report `st_ino == 0` for every file — which fails nowhere and silently breaks
`cp`'s refusal to copy a file onto itself, `ls -i`, hardlink coalescing in `du`
and `tar`, and `find -samefile`. A slower `fstatat` that answers correctly
beats a faster one that does not, so `fstatat` stays on the textual join until
lane A can write the wider record. Filed as
`requests/b-a-662-is-wired-in-663-cannot-be-until-its-record-carries-an-inode.md`.

**Where this lives:** `posix/src/file.rs` (the "pinned `*at` fast path"
section, `try_pinned_unlinkat`, and `unlinkat`); `posix/src/syscall.rs`
(`SYS_FS_UNLINKAT_PINNED`/`SYS_FS_FSTATAT_PINNED`/`SYS_FS_GETDENTS_PINNED`,
`HOST_ENOSYS`). Answers the fd-relative half of
`requests/a-b-the-at-family-now-has-three-primitives-that-resolve-the-handle.md`;
the errno half is §729.
