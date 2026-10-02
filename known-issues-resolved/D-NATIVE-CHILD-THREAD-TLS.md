### D-NATIVE-CHILD-THREAD-TLS. Native `pthread_create` child threads have no compiler-`__thread` ELF TLS (only the main thread does) — 2026-07-21 — **RESOLVED 2026-07-30**

**Resolution (2026-07-30):** Fixed as designed below, plus one extra
correctness fix the work uncovered.

- New shared module `posix/src/tls.rs` owns the whole variant-II layout
  (`TlsImage`, `image()` — the `__ehdr_start` `PT_TLS` walk —
  `init_block()`, `install()`, `setup_main_thread()`).  `crt.rs`'s
  `setup_main_thread_tls` was deleted and `__libc_start_main` now calls
  `tls::setup_main_thread()`.
- `pthread_create` allocates the child's stack **and** its TLS block/TCB
  as **one** mapping (the musl approach): `map_size =
  DEFAULT_THREAD_STACK_SIZE + img.reserve()`, TLS at the top.  The parent
  initialises the block and passes the thread pointer to the child via a
  third word pushed on its stack; `__pthread_thread_start` installs it
  with `SYS_SET_FS_BASE` as its very first action.  One mapping means one
  owner and one `munmap`, so the existing join/detach reclaim protocol
  frees the TLS too and there is no window where an exiting thread has
  unmapped its TLS but still runs `%fs`-touching code.  `ThreadSlot`
  gained `map_size` (what gets unmapped) alongside `stack_size` (the
  usable-stack prefix reported by `pthread_getattr_np`).
- **Latent bug found while validating** (present in the old
  `setup_main_thread_tls` too, so it also affected *main*-thread TLS):
  the block offset was computed with `max(p_align, 16)`.  The linker
  assigns `__thread` offsets relative to `TP - round_up(p_memsz,
  p_align)` using the segment's *own* `p_align`, so any binary with
  `p_align < 16` had its entire TLS block placed at the wrong distance
  below TP — the init image was copied to one address and every access
  read another.  It never showed because fastpy binaries happen to have
  `p_align == 0x10`.  `normalise_align` now preserves weak alignments and
  the 16-byte TCB floor is a separate `TlsImage::tp_align()`.  Because
  the block start can now be as weakly aligned as `p_align`,
  `pthread_create` rounds the child's `stack_top` down to 16 to keep the
  SysV entry-RSP alignment.
- Regression test: `services/ctest-tls-thread/` — a plain-C fixture
  (`zig cc`, `-fstack-protector-all` so *every* prologue reads
  `%fs:0x28`) that runs two sequential `pthread_create`/`pthread_join`
  cycles and checks the child's `.tdata` init image, zeroed `.tbss`,
  write-back, and isolation from the parent's block; exit 42 == all
  checks passed.  Run on-target by
  `kernel/src/proc/spawn.rs::self_test_ctls_thread`, staged into
  `/tests` by the new `services/ctest-*` loop in
  `scripts/create-ext4-rootfs.sh`.  It reproduced the alignment bug
  (exit 21) before the fix and passes in the QEMU boot test after.

Original report follows.

**Where:** `posix/src/crt.rs` (`setup_main_thread_tls`, added for
initiative F / Q31) sets up variant-II ELF TLS + the thread pointer via
the native `SYS_SET_FS_BASE` syscall **for the main thread only**.
`posix/src/pthread.rs` `pthread_create` mmaps a child stack and starts the
thread via `SYS_THREAD_CREATE`, but does **not** allocate a per-thread
copy of the program's `PT_TLS` block, nor set that child thread's
`fs_base`. The kernel starts the child with `fs_base = 0` (same as the
main thread pre-crt).

**Effect:** In a **native** (non-Linux-ABI) fastpy/C binary that uses
compiler `__thread` storage (fastpy's runtime declares `FPY_THREAD_LOCAL
__thread` — lowered to `%fs:offset`), any *child* thread's first
`__thread` access — or its stack-protector canary at `%fs:0x28` — faults
on the null thread pointer. The **main thread is fine** (the crt sets it
up), so single-threaded fastpy programs — the current initiative-F
milestone — work. This only bites native binaries that both (a) use
compiler `__thread` and (b) spawn threads via `pthread_create`. Note the
posix crate's *own* pthread TSD (`pthread_getspecific`) is tid-keyed, not
fs-based, so it is unaffected — this is specifically about *compiler*
thread-locals in the linked program. (Linux-ABI binaries are also
unaffected: glibc/musl set up child-thread TLS via `CLONE_SETTLS`, which
the Linux syscall path already honors — see F13.)

**Proper fix (userspace-only, no kernel change — the `SYS_SET_FS_BASE`
primitive already exists):** in `pthread_create`, before entering the
child's start routine, allocate a per-thread TLS block + TCB laid out
exactly like `setup_main_thread_tls` (copy the same `PT_TLS` init image
found via `__ehdr_start`; for a single-module static exe every thread's
image is identical), and have the child call `SYS_SET_FS_BASE(tp)` as the
first thing it does (before any `__thread`/canary access). Factor the
main-thread layout code into a shared helper both call. Land it with a
QEMU boot self-test that spawns a native thread which reads/writes a
`__thread` variable and joins, asserting no fault. Deferred now because
the initiative-F milestone (first real fastpy component) is
single-threaded, so this isn't yet on the critical path.
