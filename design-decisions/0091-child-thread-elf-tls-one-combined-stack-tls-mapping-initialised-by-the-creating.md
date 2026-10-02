## §91 — Child-thread ELF TLS: one combined stack+TLS mapping, initialised by the *creating* thread

**Date:** 2026-07-30
**Decided by:** Claude (autonomous)

**Context.** §82 gave the *main* thread a variant-II TLS block but explicitly
deferred child threads, so `pthread_create`'d threads started with `fs_base ==
0`: in a C program the very first instruction of the start routine's prologue
(a stack-protector canary load from `%fs:0x28`) faulted. Fixing it forces two
choices — **where the child's TLS block is allocated**, and **who initialises
and installs it**.

**Decision.** **One mapping, initialised by the parent.** `pthread_create`
mmaps `DEFAULT_THREAD_STACK_SIZE + PT_TLS-reserve` bytes as a single anonymous
region, puts the thread's stack at the bottom and its TLS block + TCB at the
top (the musl layout), copies the `.tdata` init image and writes the TCB
self-pointer *before* the child exists, and passes the resulting thread
pointer to the child as a third word pushed on its stack. The child's only
TLS-related action is `SYS_SET_FS_BASE(tp)`, as the literal first thing
`__pthread_thread_start` does. `ThreadSlot` grew a `map_size` field so the
existing join/detach reclaim protocol unmaps the whole region.

**Alternatives considered.**

- *Separate TLS mapping owned by the child.* Rejected: it creates a second
  lifetime that must be freed in exactly the same three places the stack is,
  and — worse — there is no safe ordering for a self-unmapping thread. If the
  child frees its TLS before `SYS_THREAD_EXIT`, any instruction between the
  two syscalls that touches `%fs` (e.g. a canary check in the epilogue of the
  very function doing the unmapping) faults; if it frees it after, it can't.
- *Child allocates its own TLS.* Rejected: a child that discovers it has no
  memory for TLS has nobody to report to — `pthread_create` has already
  returned success — and cannot safely run the start routine either. Doing it
  in the parent is what makes `EAGAIN` reportable.
- *Kernel-allocated TLS (a `SYS_THREAD_CREATE` flag, `CLONE_SETTLS`-style).*
  Rejected: it would put ELF layout knowledge (`PT_TLS`, variant II, the
  `.tdata` image) inside the microkernel, contradicting §82's "kernel resets
  `fs_base` to 0, userspace sets it up" split, for no gain — the primitive
  already exists.

**Rationale.** *Pro:* exactly one owner and one `munmap`; no new syscall; no
window in which a thread has freed its TLS but still runs `%fs`-touching code;
`pthread_create` can report allocation failure properly; the parent does the
work once, so the child's own startup is a single syscall. *Con:* the stack and
TLS can no longer be sized or reclaimed independently, and `pthread_getattr_np`
must report the *usable stack* (`stack_size`) while reclaim uses the *whole
region* (`map_size`) — two sizes that must not be confused (they are separate
`ThreadSlot` fields for exactly this reason).

**Latent bug this uncovered.** Both the old `crt.rs` code and the first cut of
the shared module computed the block offset as `round_up(p_memsz, max(p_align,
16))`. The linker assigns `__thread` offsets relative to `TP -
round_up(p_memsz, p_align)` using the segment's **own** `p_align`, so any
binary with `p_align < 16` had its whole TLS block placed at the wrong distance
below TP — the init image was written to one address and every access read
another. It had never shown because fastpy binaries happen to have `p_align ==
0x10`; the new C fixture has `p_align == 4` and caught it immediately (exit
21 = "the child's `.tdata` copy is missing the initialiser image"). The
16-byte TCB alignment is now a separate `TlsImage::tp_align()`, and because the
block start may consequently be as weakly aligned as `p_align`,
`pthread_create` rounds the child's `stack_top` down to 16 to preserve the SysV
entry-RSP alignment.

**Where it lives.** `posix/src/tls.rs` (new: `TlsImage`, `image()`,
`init_block()`, `install()`, `setup_main_thread()`), `posix/src/crt.rs` (now
calls `tls::setup_main_thread()`; its own copy deleted), `posix/src/pthread.rs`
(`pthread_create`, `_pthread_trampoline`, `__pthread_thread_start`,
`ThreadSlot::map_size`, the three reclaim sites). Validated on-target by
`services/ctest-tls-thread/` (plain C, `-fstack-protector-all`) +
`kernel/src/proc/spawn.rs::self_test_ctls_thread`, staged into `/tests` by a
new `services/ctest-*` loop in `scripts/create-ext4-rootfs.sh`. Resolves
D-NATIVE-CHILD-THREAD-TLS in known-issues.md; reading the reclaim protocol to
decide where the TLS mapping should be freed also exposed
D-PTHREAD-SLOT-PUBLISH-RACE (logged, not fixed here).

**How to reverse.** To go back to independent stack and TLS mappings, split
`map_size` off `pthread_create`'s single `mmap` and free the TLS region in the
*joiner*/detach paths only (never in the exiting thread) — the ordering hazard
above is why that is the only safe split. To move TLS setup into the kernel,
add a `tls` argument to `SYS_THREAD_CREATE` and have `spawn_user` write
`Task::fs_base`; `tls::image()` would then have to be reimplemented against the
kernel's ELF loader.
