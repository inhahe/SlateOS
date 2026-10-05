## 648. The kernel's working directory is a Linux-ABI concept, so `dirfd == 0` means *no base supplied* — not "wherever I happen to be"

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous), lane A
**Answers:** `requests/b-a-the-kernels-cwd-and-libcs-cwd-are-two-different-directories.md`

**In short:** several new file syscalls let a program say "open `notes.txt`
inside *that* folder", where *that folder* is named by a number the program
already holds. Passing `0` for that number used to mean something different:
"inside whatever folder my program is currently sitting in". Lane B found that
the kernel and the C library each keep their **own idea** of what folder a
program is currently sitting in, and the two are not the same folder — a
SlateOS-native program that changes directory moves the library's idea and not
the kernel's. So `0` meant a directory that was very likely wrong, and a
security check made against the wrong folder does not fail: it *passes*, on the
wrong folder, and returns a perfectly valid handle. The decision is to stop
having `0` mean a folder at all. It now means "I am not supplying a folder",
which is legal exactly where no folder is needed (the path is already a
complete one, starting at `/`) and refused everywhere else.

### The decision

`0` is not a directory. In `SYS_FS_OPENAT2` (661) it means **no base
supplied**:

| `resolve` | fragment | `dirfd == 0` |
|---|---|---|
| none | absolute (`/etc/hosts`) | **allowed** — the base is never read; POSIX `openat(2)` ignores it for an absolute path |
| none | relative (`hosts`) | `InvalidArgument` — a base is needed and none was given |
| `RESOLVE_BENEATH` | absolute | `CrossDevice`, already, from `beneath_fragment_ok` before the base is looked at |
| `RESOLVE_BENEATH` | relative | `InvalidArgument` |

So under containment, `dirfd == 0` is **always** an error — which is what
containment wants, since "confine this walk beneath a directory I did not name"
is not a request that can be honoured.

In `SYS_FS_UNLINKAT_PINNED` / `SYS_FS_FSTATAT_PINNED` / `SYS_FS_GETDENTS_PINNED`
(662/663/664) the branch is deleted outright rather than narrowed: all three
take a single-component name, never an absolute path, so there is no shape in
which they can proceed without a base. `0` falls through to the ordinary handle
lookup and comes back `InvalidHandle`, which is the honest answer — handle `0`
does not exist.

### Why, and why this is not the coin-flip lane B thought it was

Lane B laid out two coherent options and preferred (b) — drop the cwd meaning —
weakly, arguing from the cost of keeping two copies of a cwd in step across
`fork`/`exec`/`spawn`. That argument is correct but it is a cost argument, and
cost arguments are the kind that get re-litigated.

There is a stronger one that is not a preference at all. `CLAUDE.md` lists as
non-negotiable: *"Capability-based security from day one. Every kernel object
accessed via unforgeable handles. **No ambient authority.**"* Ambient authority
is permission you get by *being* you rather than by holding a token. `dirfd ==
0` → "resolve against the directory this process happens to be in" is exactly
that: a base the caller did not name, could not have been denied, and cannot
pass to anyone else. Option (a) — adding `SYS_FS_SET_CWD` so the native ABI has
a first-class cwd — would not fix that; it would *entrench* it, and make a
second syscall whose whole purpose is to move the ambient base around.

So (b) is not the cheaper of two defensible designs. It is the only one of the
two that the architecture permits.

### Why "no base supplied" rather than "always an error"

The first draft of this change made `dirfd == 0` an outright error. Reading
`posix/src/file.rs::openat2_forward` showed that would have broken libc, which
passes `0` today — deliberately, for absolute paths, with the reasoning written
in the code: *"0 is safe for an absolute path, and only for an absolute path,
because the base is then never read."*

That is not libc exploiting a loophole; it is the correct reading of what `0`
means once it stops meaning a directory. An absolute fragment is the complete
answer, so there is no base to be right or wrong about. Making that shape an
error would have forced libc to open, pass and close a handle it provably never
uses, on every absolute-path `openat2` — pure ceremony. So `0` keeps a meaning,
just a much smaller one, and lane B's side needs no change at all.

### What this costs

One `open`+`close` per confined `AT_FDCWD` call — which libc already pays
today, because it could not use `0` for a relative path anyway (that is the
whole content of lane B's report). The native ABI becomes slightly less
convenient for a caller with no libc: a bare kernel task. The old code already
refused `dirfd == 0` for those (`caller_pid()` is `None`), so that population is
zero.

The kernel's `pcb` cwd stays exactly where it is, serving the Linux ABI's
`chdir`/`fchdir` in `kernel/src/syscall/linux.rs` — its only two writers. What
changes is that no *native* call reads it any more, which makes the divergence
lane B documented (`TD-B-THE-KERNEL-HAS-A-WORKING-DIRECTORY-AND-LIBC-NEVER-TELLS-IT-ANYTHING`)
harmless rather than latent: two copies that disagree only matter if something
reads both.

### The test this unblocks

`test_dispatch_openat2_native` previously said, in its own doc comment,
*"Kernel context … there is no cwd — `dirfd == 0` is therefore covered from
ring 3 rather than here."* That was true and unsatisfying: the ring-3 coverage
it pointed at (`self_test_openat2_beneath`) goes through the **Linux** ABI's
`AT_FDCWD`, which is a different mechanism entirely, so native `dirfd == 0` was
in fact covered nowhere.

Once `0` no longer depends on a cwd, it can be tested from kernel context, and
now is — cases (f), (g) and (h): absolute fragment with no base succeeds and
reads the right byte; relative fragment with no base is `InvalidArgument`;
`RESOLVE_BENEATH` with no base is refused rather than confined to something.

### Alternatives considered

| Option | Why not |
|---|---|
| **(a) Make the cwd part of the native ABI** — add `SYS_FS_SET_CWD`, have libc's `chdir` call it | Entrenches ambient authority, which `CLAUDE.md` forbids outright. Also, as lane B noted, the sync bug it invites lands in `fork`/`exec`/`spawn`, not in `chdir` where anyone would look for it. |
| **Keep `dirfd == 0` but document it as "the Linux cwd, use at your own risk"** | A documented trap is still a trap, and this one fails *open*: the wrong base returns a valid handle that looks like working containment. That is `TD-OPENAT2-BENEATH-INROOT`'s failure mode, reintroduced by a footnote. |
| **An explicit `AT_FDCWD`-style sentinel** (e.g. `u64::MAX`) instead of `0` | Lane B was offered this and declined it: the problem is not that `0` is a confusing spelling, it is that the *thing being spelled* should not exist in this ABI. A better-named sentinel for ambient authority is still ambient authority. |
| **Make `dirfd == 0` an error in all cases**, including absolute fragments | Breaks libc's current, correct use, and buys nothing: it forces a handle to be opened and closed for a base that is provably never read. |

### How to reverse it

Restore the `pcb::get_cwd` branch in `sys_fs_openat2` and in `pinned_dir_arg`,
and drop cases (f)–(h) from `test_dispatch_openat2_native`. Note what reversing
reinstates: not a missing feature but a base nobody named — and a
`RESOLVE_BENEATH` walk that succeeds, with a valid descriptor, confined to a
directory the caller did not ask for.
