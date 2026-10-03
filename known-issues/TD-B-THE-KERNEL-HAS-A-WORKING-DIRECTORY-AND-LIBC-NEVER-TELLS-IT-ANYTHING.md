## `TD-B-THE-KERNEL-HAS-A-WORKING-DIRECTORY-AND-LIBC-NEVER-TELLS-IT-ANYTHING` (lane B, 2026-08-30) — DEBT

**In short:** two different parts of the system each believe they know what
directory this process is "in", and they do not agree. The C library keeps its
answer in its own memory and never mentions it to the kernel; the kernel keeps
its own answer, which changes only through the Linux-compatibility calls that
SlateOS-native programs do not use. Nothing is broken today, because every path
the library hands the kernel is spelled out in full from the root. It becomes a
bug the moment any native call resolves a *relative* path — and it would become
one quietly, since a path resolved against the wrong directory usually still
finds a file.

**Where:**
- `posix/src/unistd.rs::chdir` — validates the target with `SYS_FS_STAT`, then
  stores the normalized absolute path in a libc-side buffer. No syscall tells
  the kernel.
- `kernel/src/proc/pcb.rs::{get_cwd, set_cwd}` — the kernel's copy.
- `kernel/src/syscall/linux.rs` (~42750, ~42821) — the *only* two callers of
  `set_cwd`, both in the Linux ABI (`chdir`/`fchdir`). There is no native
  syscall number that sets it.

**How it bites.** A native program that calls `chdir("/a/b")` and then makes any
syscall whose path is resolved kernel-side against `pcb::get_cwd` gets `/a/b`
from libc's point of view and the spawn-time directory from the kernel's. Today
exactly one native call reads that cwd — `SYS_FS_OPENAT2` with `dirfd == 0` —
and `posix/src/file.rs::openat2_forward` deliberately never passes 0 for a
relative path for this reason (it opens a scratch handle on libc's own cwd
instead; `design-decisions.md` §714, decision 2). Every other native fs call
receives an absolute path from `resolve_or_err`, so the kernel's cwd is never
consulted.

That makes this latent rather than live, and it is worth being precise about
why the latent version is still worth an entry: the failure it produces is a
*wrong answer*, not an error. A relative open resolved against the wrong base
usually succeeds — on the wrong file. And in the one place it interacts with
containment, it fails open: `RESOLVE_BENEATH` against the wrong base still
returns a valid descriptor and still looks like working confinement.

**The rule that holds while this is unfixed:** libc never relies on the
kernel's process working directory. Any use of `dirfd == 0` (or a future
equivalent) must be justified by the base being provably unread, as
`openat2_forward`'s absolute-path case is.

**Trigger to fix:** the first native syscall that resolves a relative path
against the kernel's cwd, or the first native caller that wants `dirfd == 0`
for a relative fragment.

**The proper fix, and why it was not done here.** Either (a) a native
`SYS_FS_SET_CWD` that `chdir`/`fchdir` call, so the two copies stay in step, or
(b) an explicit statement in the native ABI that the kernel's cwd is not part of
it, and the removal of `dirfd == 0`'s cwd meaning in favour of an
`AT_FDCWD`-style sentinel that libc supplies a real handle for. (a) is the
smaller change and creates a second source of truth that must be kept in sync —
including across `fork`, `exec` and `spawn`, which is where it would go wrong.
(b) is the cleaner one and is a request to lane A, not a lane-B change. Neither
belongs inside a change whose job was to stop refusing `RESOLVE_BENEATH`;
shipping a cwd-semantics change hidden inside an `openat2` change is how the
next entry above this one gets written. Filed as
`requests/b-a-the-kernels-cwd-and-libcs-cwd-are-two-different-directories.md`.
