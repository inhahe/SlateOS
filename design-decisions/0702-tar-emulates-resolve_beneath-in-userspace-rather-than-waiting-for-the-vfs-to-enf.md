## 702. `tar` emulates `RESOLVE_BENEATH` in userspace rather than waiting for the VFS to enforce it

**Date:** 2026-08-29
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** Unpacking an archive must put files under the directory you
unpacked it in and nowhere else. The obstacle is a *symbolic link* — a name
that stands for another location, like a shortcut — that already sits somewhere
along a member's path and points outside. Linux has a kernel feature that
enforces "stay under this directory" for you (`RESOLVE_BENEATH`, a flag to the
`openat2` system call), and neither of the two systems this program is built for
actually enforces it. The choice was: ask lane A to build kernel support and
leave `tar` exploitable until it lands, or do the walk by hand in `tar` itself
out of parts that already work everywhere. This is the second.

**The alternatives**

1. **Wait for the kernel.** File a request with lane A to make the SlateOS VFS
   honour `openat2`'s `RESOLVE_*` flags, and use `openat2` once it does.
   * *For:* one enforcement point, shared by every program that ever needs it —
     `cp -r`, `unzip`, an installer, anything that writes a tree it did not
     author. Kernel-side resolution is inherently free of the check-then-use
     races userspace has to design around.
   * *Against:* it is a lane A change to a subsystem lane B does not own, on
     nobody's schedule, and `tar` is exploitable in the meantime with a
     two-archive attack that needs no privileges. It also does not help the
     **host** build at all: this binary is built and differentially tested
     against GNU tar on Linux/glibc, and glibc exports no `openat2` wrapper, so
     that side would need `syscall(437, …)` by number regardless. A fix that
     only works on one of the two targets cannot be verified by the harness that
     verifies everything else here.

2. **Emulate the walk in `tar`.** Hold the destination open, open each parent
   component with `O_DIRECTORY|O_NOFOLLOW` from the descriptor above it, read
   any symlink component with `readlinkat` and judge its target by GNU's rule,
   and keep the descriptors on a stack so `..` pops one and popping the root is
   the refusal. Create the leaf with an `*at()` call through the descriptor that
   came back.
   * *For:* buildable today out of primitives both targets already have
     (`openat`, `readlinkat`, `mkdirat`, `symlinkat`, `linkat`, `mkfifoat`,
     `mknodat`, `unlinkat`, `fchmodat`, `utimensat` — all present in
     `posix/src/file.rs` and in glibc), verifiable today against the real GNU
     binary, and **race-free** for the same reason the kernel version is: the
     caller creates relative to the descriptor the walk returned, so there is no
     second resolution for an attacker to interleave with. It is also GNU's own
     architecture — GNU tar does not use `openat2` either, which is why its
     `mkdir`, `symlink` and `mkfifo` refusals all report `EXDEV`: none of
     `mkdirat`/`symlinkat`/`linkat` takes a resolve flag, so the restriction can
     only live in the resolution of the parent.
   * *Against:* it is ~200 lines of resolution logic living in one utility
     rather than in the kernel, and the next program with the same requirement
     will have to have it again. `O_NOFOLLOW` must genuinely be honoured by the
     VFS for it to hold on SlateOS (it is — `kernel/src/fs/handle.rs`,
     `OpenFlags::NOFOLLOW`). Off unix there is no `openat` to build it from at
     all, so that twin resolves lexically and is weaker.

**Chosen: 2**, with a request filed for 1 as the eventual replacement rather
than the prerequisite. The deciding fact is that the exploit is live and the
kernel work is not, and that option 1 would leave the host build — the only one
the differential harness can run — unfixed either way. The emulation is not a
stopgap that will need unpicking: it is what GNU does, it is measured to match
GNU on all ten rows of the rule table, and if the VFS later enforces
`RESOLVE_BENEATH` the walk can be replaced by a single `openat2` behind the
same `Dir::locate` signature without any caller changing.

**Not decided here:** whether the emulation should be lifted out of `tar.rs`
into a shared helper for the other tree-writing utilities. It should not be
until there is a second caller — one caller is not a pattern, and the exact
shape of the second one's needs (does it want to *follow* in-tree links? does it
need the parent descriptor back, or only a result?) is what should determine the
interface. Recorded in `todo.txt`.

**Amended the same day, and it sharpens option 1 rather than changing the
choice.** Writing the lane-A request this entry promises meant re-reading both
implementations of `openat2`, and they did not agree. The kernel's
`sys_openat2` *refuses* `RESOLVE_BENEATH` with `EXDEV` — safe. libc's
`posix/src/file.rs::openat2` validated the `resolve` word, discarded it, and
delegated to plain `openat`, so a caller asking to be confined received a
working descriptor and no confinement at all. Fail-open, on the side a SlateOS
program actually reaches. Fixed immediately — libc now refuses every
restriction it cannot enforce, with the kernel's exact errnos, pinned by seven
tests; written up in `known-issues.md` → `TD-OPENAT2-BENEATH-INROOT` and filed
as `requests/b-a-openat2-resolve-beneath-is-fail-open-in-libc-and-unenforceable-in-the-vfs.md`.

The bearing on this decision: option 1 was *worse* than it is scored above.
The text says the `RESOLVE_*` flags are "accepted but not enforced" — that was
the libc doc comment's own wording, and it is what made the gap sound like one
subsystem waiting on lane A. In fact the ABI had two implementations
disagreeing about a security promise, so "use `openat2` once the VFS honours
it" would have had to reconcile them first and would *still* have left the host
build — the only one the differential harness can run — unaddressed. Nothing
here argues for revisiting **2**; it removes the last reason to have hesitated
over it.
