## 82. SlateOS native-ABI main-thread ELF TLS setup (Q31) — **option A (the posix crt sets up TLS in userspace via `__ehdr_start`), plus a native `SYS_SET_FS_BASE`**

**Date:** 2026-07-21
**Decided by:** Operator (operator said "I'll go with A"; Claude recommended A) —
resolves open-question **Q31**, unblocking initiative F's "first real component"
(on-target execution) milestone.

**Context.** Booting a fastpy-built SlateOS binary surfaced a native-ABI gap: the
ELF carries a `PT_TLS` segment because fastpy's C runtime uses compiler
thread-locals (`runtime/threading.h`: `FPY_THREAD_LOCAL __thread`), which the
compiler lowers to `%fs:offset` (x86-64 TLS variant II) and which therefore
require an `%fs` base (thread pointer) before first use. But SlateOS gives a
*native* static binary none: the kernel resets `fs_base` to 0 on exec expecting
userspace to set it up (the Linux/libc model), yet — unlike a Linux crt that
reads the aux vector and calls `arch_prctl(ARCH_SET_FS)` — the posix crt fetches
argv/fds via native syscalls (no aux vector) and never sets up TLS. So the first
`__thread` access faults. This is *additive*, not a reversal of the posix crate's
deliberate "tid-keyed pthread TSD instead of FS/GS TLS" choice (that's *library*
TSD via `pthread_setspecific`; compiler `__thread` is a distinct mechanism that
any real C/C++ on SlateOS needs). Q31 offered **A** (posix crt sets up TLS in
userspace, finding `PT_TLS` via the linker-defined `__ehdr_start` — no aux vector
needed — and setting `fs_base` via the existing kernel primitive), **C** (the
kernel ELF loader sets it up during spawn), or **B** (fastpy drops `__thread` on
SlateOS — a correctness trap for multithreaded programs; recommended against).
Claude recommended A.

**Decision.** **A.** `__libc_start_main` (in `posix/src/crt.rs`), before running
constructors or `main`, locates the program's `PT_TLS` program header via the
linker-defined `__ehdr_start` symbol (present in static non-PIE links), allocates
a variant-II TLS block + a TCB whose first word self-points, copies the `p_filesz`
init image and zeroes the `.tbss` remainder, then sets the thread pointer. Setting
`fs_base` uses a **new native syscall `SYS_SET_FS_BASE`** that calls the kernel's
existing `set_current_task_fs_base` (`kernel/src/sched/mod.rs`) — the native
counterpart of the Linux-ABI `arch_prctl(ARCH_SET_FS)`, so native binaries need no
Linux-table dispatch. The TCB reserves enough space to cover the stack-protector
canary slot (`%fs:0x28`) in case the runtime is built with `-fstack-protector`.

**Rationale.** *Pro:* keeps TLS setup in userspace, matching the kernel's existing
"reset to 0, userspace sets it up" design; keeps the microkernel loader minimal;
consistent with the native crt already doing its own startup (args/fds/environ/
signals/constructors); generalizes to child-thread TLS (`pthread_create`) in the
same layer later. *Con:* implements variant-II TLS layout in the posix crate and
adds one native syscall; child-thread TLS is deferred (documented) until a fastpy
program actually spawns threads.

**Operator's exact words.** "I'll go with A."

**Where it lives.** `posix/src/crt.rs` (`setup_main_thread_tls()`, called at the
top of `__libc_start_main`), `posix/src/syscall.rs` (`SYS_SET_FS_BASE` number +
wrapper), the kernel native syscall dispatch + `kernel/src/sched/mod.rs::
set_current_task_fs_base`; then rebuild the sysroot `libc.a`, rebuild the fastpy
binary, embed + spawn (`services/fastpy-hello`, self-test
`kernel/src/proc/spawn.rs::self_test_fastpy_slateos_tls`), and boot-test.
Supersedes Q31 in open-questions.md (now removed).

**Outcome (2026-07-21) — DONE and validated on-target.** The fastpy binary
`services/fastpy-hello` boots under the kernel and runs to `exit(0)` at ring 3;
the self-test asserts `Zombie` + exit 0 and the full boot-test passes
(BOOT_OK, no self-test failures). Serial: *"fastpy-on-SlateOS TLS … set up
main-thread ELF TLS via SYS_SET_FS_BASE and ran to exit(0): OK"*.

**Latent bug fixed en route.** The first attempts faulted (exit -8): the crt's
TLS `mmap` came back `PermissionDenied` (-400). Root cause was **not** the TLS
logic but a syscall-number skew — `posix/src/syscall.rs` numbered the native
memory syscalls `SYS_MMAP=30`, `SYS_MUNMAP=31`, `SYS_MPROTECT=32`, which alias
the kernel's *capability-gated* IRQ syscalls (`SYS_IRQ_REGISTER=30`,
`SYS_IRQ_WAIT=31`, `SYS_IRQ_RELEASE=32`), while the kernel's real native numbers
are `SYS_MMAP=20`/`SYS_MUNMAP=21`. So a native `mmap()` was rejected by the
capability check *before* `sys_mmap` ran (hence no handler-level trace). Fixed by
correcting posix to 20/21/22 and reserving `SYS_MPROTECT=22` in
`kernel/src/syscall/number.rs`. Note `SYS_MPROTECT=22` has no native handler yet
(native `mprotect()` → `ENOTSUP`, safe); the Linux-ABI mprotect is unaffected.
Tracked in known-issues.md (BUG-NATIVE-MMAP-NUM resolved; TD-NATIVE-MPROTECT
open).

**Follow-up (2026-07-30) — the deferred child-thread half landed; see §91.**
The "generalizes to child-thread TLS later" promise in the rationale above is
now fulfilled: the layout code moved out of `crt.rs` into a shared
`posix/src/tls.rs` that both `__libc_start_main` and `pthread_create` use, so
the `setup_main_thread_tls()` named above no longer exists (it is
`tls::setup_main_thread()`).
