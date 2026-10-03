### B-THE-NATIVE-LIBC-AND-THE-LINUX-ABI-DISAGREE-ABOUT-WHAT-EXISTS, AND LIBC'S DOC COMMENTS EXPLAIN IT WITH A REASON THAT STOPPED BEING TRUE — 2026-08-21 — OPEN

**In short.** SlateOS has two syscall ABIs. A binary loaded as a Linux
executable gets `kernel/src/syscall/linux.rs`; a binary linked against our own
`toolchain/sysroot/lib/libc.a` — CPython, bash, make, every `ctest-*` fixture —
gets the native table. Over time lane A implemented a number of operations on
the *Linux* side that the native side has no syscall number for, so our libc
still answers `ENOSYS`. That part is defensible. What is not defensible is that
several of those libc functions carry a doc comment giving the *reason* as "the
kernel does not do this" — and the kernel does.

**The concrete one that prompted the audit.** `posix/src/process.rs`
`process_vm_readv` reads:

> returns `-1` with `errno = ENOSYS`: cross-process memory access isn't part of
> the microkernel's IPC model (programs use channel handles to transfer pages
> explicitly rather than peeking at another task's address space).

`kernel/src/syscall/linux.rs:26529` implements it, in both directions, with a
same-address-space fast path and a cross-address-space path gated on a
`Process` capability carrying the `DEBUG` right (design-decisions §24), using
`copy_from_user_as` / `copy_to_user_as` against the target's PML4. The claim in
the comment is not a simplification; it is the opposite of what the kernel now
does, and anyone reading libc to find out whether SlateOS can do cross-process
introspection gets the wrong answer.

**How this was measured, so it can be re-run — and the two wrong ways I tried
first.** Start by parsing `linux.rs` for `nr::X => sys_x(args)` arms. That is
"a handler exists", which is *not* the same as "it works", and treating it as
such is the trap this entry is about. Cross that against libc functions with an
unconditional `set_errno(ENOSYS)` terminal outside `#[cfg(test)]`.

Classifying each handler is where it goes wrong:

- **Wrong test 1: "does the handler body contain `ENOSYS`?"** This calls
  `sys_mknod_common` a real implementation, because it validates its arguments
  carefully and then returns `linux_err(errno::EPERM)` — no `ENOSYS` anywhere.
  A handler that never says `ENOSYS` can still refuse every call it gets. I
  used this test while *writing this very entry* and it produced a list that
  overstated the gap by a third.
- **Wrong test 2: "can the handler return `SyscallResult::ok`?"** — right
  question, but only if you follow delegation properly. My first
  delegation-follower handled one level and only when the wrapper's body was a
  bare tail call, so `sys_signalfd` (which has statements before delegating to
  `signalfd_common`) and `sys_process_vm_readv` (delegating to
  `process_vm_impl`) both looked like they never succeed. They do:
  `signalfd_common` has two `SyscallResult::ok` sites, `process_vm_impl` seven.

**The right test** is "can this handler, following delegation to its real
implementation, ever return `SyscallResult::ok`?" — verified per call rather
than trusted to a regex. The result, for the libc `ENOSYS` terminals checked:

| Class | Calls |
|---|---|
| **Kernel really implements it; native libc has no number to reach it** | `signalfd`, `signalfd4`, `process_vm_readv`, `process_vm_writev`, `pidfd_open`, `kcmp`, `arch_prctl` |
| **Kernel denies it too — the two differ only in *which* errno** | `mknod`, `mknodat`, `setns`, `mount`, `umount2`, `ptrace`, `chroot`, `swapon`, `swapoff`, `reboot`, `init_module` (all `EPERM`); `mq_notify` (`EBADF`) |
| **Partial: libc's `ENOSYS` is a narrow arm, or the kernel succeeds only in the no-op case** | `unshare(0)`, `iopl(0)`, `ioperm`, `quotactl`, `syslog`, `madvise`, `tee` |
| **Both sides `ENOSYS` — libc is honest** | `clone3`, `getdents`, `seccomp`, `bpf`, `userfaultfd`, `perf_event_open`, `io_uring_setup`, `fanotify_init`, `socket(SOCK_RAW)` |

Only the first row is a missing feature. The second row is a *divergence*: both
ABIs refuse the operation, but a program probing for support distinguishes
`ENOSYS` from `EPERM`, so it still matters — it is just a much smaller problem,
and one that argues for fixing the errno rather than adding a syscall number.

**Why it bites.** The same program gets different behaviour depending on which
ABI it was linked for, on calls neither side documents as ABI-dependent. Ports
are exactly the programs that probe for features at runtime. And nothing tests
the correspondence: `cargo test -p posix` tests libc against libc's own
expectations, the kernel self-tests exercise the Linux arms directly, and no
test compares the two answers for the same call.

**Proper fix, in the order it should be done.**

1. **Stop the doc comments from lying** — the cheap half, and the half that
   costs nothing to get wrong later. Each libc `ENOSYS` terminal for a row-one
   operation should say *"the native syscall table has no number for this; the
   kernel implements it on the Linux ABI at `linux.rs:<line>`"*, not *"the
   kernel does not do this"*. Done for `process_vm_readv`/`writev` and
   `signalfd`, `pidfd_open`, `kcmp` and `arch_prctl` in the commit that filed
   this entry — **step 1 is done**, that being all seven of row one. Row two
   needs no doc change: libc saying "we don't do this" is true there, and only
   the errno differs.
2. **Decide, per row-one operation, whether the native ABI should reach it at
   all.** This is a real design question and not obviously "yes for
   everything". `signalfd`, `pidfd_open` and `process_vm_readv` are the easy
   yes: each already takes a handle or is gated on a capability, so a native
   number would not widen anyone's authority. `kcmp` is the awkward one — it
   compares two *arbitrary* pids' resources by number, which is ambient
   authority by construction, and is also the least-needed of the set.
   `arch_prctl` is neither — it is per-thread CPU state (`FS`/`GS` base), and
   the question there is whether the native ABI wants a general escape hatch or
   a specific TLS call. Whatever is decided, the answer belongs in
   `design-decisions.md`, because right now the split is an accident of what
   lane A happened to need.
3. **Then add native syscall numbers for whatever survives step 2**, which is a
   lane A change and needs a request. Separately — and much cheaper — row two's
   errno divergence can be closed from either side without a new syscall
   number, by having libc return the kernel's `EPERM` instead of `ENOSYS` for
   operations we deliberately refuse. That is a behaviour change to libc, so it
   wants step 2's decision first rather than being done opportunistically.

**Trigger.** Step 1 is done. Step 2 is worth raising with the operator only if
a port actually needs one of these; none did (CPython, bash and make link clean
without them), which is why this was filed rather than escalated.

**The trigger fired for `pidfd_open` on 2026-10-03.** procps-ng's `pidwait`
is ported (`userspace/coreutils/src/pgrep.rs`, measured by
`scripts/pgrep-diff.sh`) and waits with nothing else: `pidfd_open` on each
process, the descriptors in one epoll set, `epoll_wait` until each reports its
process gone. It is not built for SlateOS while the native call is `ENOSYS`.
`pidfd_open` is the member of row one this entry already called the easy yes,
so it went straight to the lanes that own the change, as
`requests/b-ad-pidwait-needs-pidfd-open-on-the-native-abi.md`; the rest of
row one still waits for a port of its own.
Note that the audit turned up a reason to answer step 2 with "no" for at least
one member of row one — see the `kcmp` entry immediately below.

**If never fixed:** the libc keeps telling readers the kernel cannot do things
it can. That is the same defect class as
`B-WORKSPACE-TEST-IS-RED-SLATEOS-COREUTILS-SHADOW-THE-HOSTS` sitting at
`Status: OPEN` for five days after it was fixed — a wrong statement in a file
that other lanes consult is worse than no statement, because it is acted on.
