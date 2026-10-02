## §92 — Per-thread libc storage lives in a fixed slot above the thread pointer, not in `#[thread_local]` or a malloc'd block

**Date:** 2026-07-30
**Decided by:** Claude (autonomous)

**Context.** A whole family of POSIX functions returns a pointer to storage the
standard says is *per-thread*: `errno` (via `__errno_location`), `gmtime`/
`localtime`'s `struct tm`, `asctime`/`ctime`'s text buffer, `inet_ntoa`'s
buffer, and the four netdb result structs (`gethostbyname`, `gethostbyaddr`,
`getservby*`, `getprotoby*`), plus the resolver's `h_errno`. Ours were 16
process-wide `static mut`s. That is
a genuine product thread-safety gap on the OS target, and on the host it made
`cargo test -p posix` flaky under the parallel harness (TD-POSIX-TEST-PARALLEL).
Fixing it means picking *where per-thread libc storage lives* — a decision that
binds the whole libc, not just these functions, since everything added later
(`strtok`'s cursor, locale state, …) will go in the same place.

**Decision.** One `#[repr(C)] struct PerThread` (`posix/src/perthread.rs`)
holding every such field, parked at a **fixed offset above the thread
pointer**: `TP + TCB_SIZE`. `TlsImage::reserve()` grew by
`perthread::BLOCK_SIZE`, so the block rides inside the mapping the thread
already owns — the same single mapping §91 established for child threads.
`perthread::current()` is one `mov {}, fs:[0]` plus a constant add. `errno` is
deliberately the **first** field, so `current().cast::<i32>()` *is* the errno
pointer and `__errno_location` — on the hot path of every failing syscall —
compiles to the same two instructions with no field arithmetic.

**Alternatives considered.**

- *`#[thread_local]` statics.* The obvious answer, and rejected only for a
  toolchain reason: the attribute is nightly-only (E0658), and the sysroot
  `libc.a` builds on **stable** rustc targeting `x86_64-unknown-none`. Moving
  the sysroot to nightly for one attribute would split the toolchain and put
  every future sysroot build at the mercy of nightly churn. Worth revisiting if
  the attribute stabilises — it is strictly nicer, since the linker would then
  place and size the storage instead of us.
- *A pointer slot in the TCB pointing at a lazily-`malloc`'d block.* Rejected:
  it introduces an allocation on a path that must not fail or re-enter the
  allocator. `strerror` returning `NULL` because the per-thread block couldn't
  be allocated is not a behaviour any caller handles, and calling `malloc` from
  inside `__errno_location` risks re-entering an allocator that is itself
  mid-`errno`-write. It would also need a free-on-exit hook in all three thread
  reclaim paths.
- *A fixed-size array indexed by thread id.* Rejected outright: caps the thread
  count, and the index lookup is a shared-cache-line read on the hottest path
  in the libc.

**Rationale.** *Pro:* no allocation (hence no failure path and no allocator
re-entrancy), no teardown (`pthread_join`/`pthread_detach` already unmap the
whole thread region), and zero-initialisation for free — the mapping is fresh
anonymous memory and every field's correct initial state happens to be all-zero.
Lookup is two instructions. *Con:* the block is sized at compile time and paid
for by every thread whether or not it ever calls `gmtime`; and "all-zero is a
valid `PerThread`" is now a load-bearing invariant that isn't visible at the
field declarations. The latter is guarded by a test that constructs the struct
via `core::mem::zeroed()`, which makes rustc's `invalid_value` lint a
*compile-time* tripwire if anyone adds a field (a `NonNull`, a reference, an
enum without a zero variant) whose zero bit pattern is invalid. The former is
bounded by a test asserting `BLOCK_SIZE <= 2048`.

**The no-thread-pointer case.** Reading `%fs:0` faults while `fs_base` is still
0, which is the state of any bare-metal `services/` binary that skips the crt.
`tls.rs` therefore keeps a process-global `TP_INSTALLED: AtomicBool` (Release on
a successful `SYS_SET_FS_BASE`, Acquire in `thread_pointer()`), and
`current()` hands back a shared `static mut FALLBACK` when it is clear. A single
*process*-global flag is sound for a *per-thread* question only because the
ordering is fixed: `__libc_start_main` installs the main thread's TP before
anything else runs, and `pthread_create` is unreachable before that, so no
thread can ever observe the flag set without having installed its own TP.
(`asm!` deliberately omits `pure` so the read can't be CSE'd across an
`install()` call.)

**Where it lives.** `posix/src/perthread.rs` (new), `posix/src/tls.rs`
(`reserve()`, `TP_INSTALLED`, `thread_pointer()`), `posix/src/errno.rs`
(`set_errno`/`get_errno`/`__errno_location` rewritten; the `#[cfg(test)]`
`AtomicI32` split deleted), `posix/src/time.rs` (`TM_RESULT`, `ASCTIME_BUF`
deleted), `posix/src/socket.rs` (13 statics replaced by `HostentBuf`/
`ServentBuf`/`ProtoentBuf`, which also deduplicated ~35 lines of copy-pasted
pointer-web assembly between `gethostbyname` and `gethostbyaddr`; and `h_errno`,
whose exported *data* symbol was deleted outright — see BUG-POSIX-H-ERRNO).
Host build
uses a `thread_local!` behind the identical `current()` API. Validated by 10
consecutive clean parallel `cargo test -p posix` runs (20013 tests each) and,
on target, by extending `services/ctest-tls-thread` to assert that each pthread
gets its own `__errno_location()`, distinct from the parent's and starting at 0.
Resolves TD-POSIX-TEST-PARALLEL.

**How to reverse.** If `#[thread_local]` stabilises for our sysroot toolchain,
delete `perthread.rs`, declare each field as its own `#[thread_local] static
mut`, and shrink `TlsImage::reserve()` back (the linker will have folded the
storage into `PT_TLS` itself). Callers change only in that
`(*perthread::current()).x` becomes `X`; `__errno_location` would return
`&raw mut ERRNO`.
