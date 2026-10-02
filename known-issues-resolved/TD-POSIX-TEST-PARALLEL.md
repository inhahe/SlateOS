### TD-POSIX-TEST-PARALLEL. `cargo test -p posix` is flaky under parallel execution — non-thread-safe libc functions share `static mut` return buffers — 2026-07-22 — ✅ **RESOLVED 2026-07-30**

**What:** Running the posix host suite with the default parallel test runner
(`cargo test -p posix --target x86_64-pc-windows-gnu`) intermittently fails a
*different* small set of tests each run (observed: `socket::tests::
test_getprotobynumber_tcp`/`_udp`, `time::tests::test_gmtime_mktime_roundtrip`).
The set changes run-to-run — the hallmark of a data race, not a logic bug.

**Root cause:** POSIX's `getprotobyname`/`getprotobynumber`/`getservby*`/
`gmtime`/`localtime`/`asctime`/`ctime` (and peers) are, by spec, **not
thread-safe** — each returns a pointer into a single process-wide `static mut`
buffer (that's why the `_r` reentrant variants exist). The product code is
spec-correct for real single-threaded-per-call usage. The *test harness* runs
tests on multiple threads concurrently, so two tests calling the same
static-buffer function race: thread A reads `p_name` after thread B has
overwritten the shared buffer. Nothing to do with any product bug.

**Fix (2026-07-30):** the recommended TLS-buffer fix, not the test-only Mutex.
New module `posix/src/perthread.rs` defines a single `#[repr(C)] PerThread`
block holding *every* piece of storage POSIX says is per-thread: `errno`, the
`gmtime`/`localtime` `Tm`, the `asctime`/`ctime` buffer, `inet_ntoa`'s buffer,
and the four netdb result buffers (`hostent`, `hostent_rev`, `servent`,
`protoent`). 15 `static mut`s across `errno.rs`, `time.rs` and `socket.rs` were
deleted in favour of it.

Placement on the OS target: the block is parked at `TP + TCB_SIZE`, inside the
mapping `TlsImage::reserve()` already covers (`reserve()` grew by
`perthread::BLOCK_SIZE`). That means **no allocation** (so no failure path and
no allocator re-entrancy inside e.g. `strerror`), **no teardown** (`pthread_join`
/`pthread_detach` already unmap the whole thread block), and zero-initialisation
for free — the mapping is fresh anonymous memory and every field's correct
initial state is all-zero (guarded at compile time by a test that builds a
`PerThread` via `core::mem::zeroed()`, so rustc's `invalid_value` lint fires if
anyone ever adds a field whose zero bit pattern is invalid). Lookup is one
`mov {}, fs:[0]` plus a constant offset. On the host build the same `current()`
API is backed by a `thread_local!`. See design-decisions.md §92 for the
alternatives considered (`#[thread_local]`, lazily-`malloc`'d block).

Because reading `%fs:0` faults when `fs_base` is still 0, `tls.rs` gained a
process-global `TP_INSTALLED: AtomicBool` (Release on successful
`SYS_SET_FS_BASE`, Acquire in `thread_pointer()`); bare-metal `services/`
binaries that skip the crt leave it clear forever and share a static `FALLBACK`.
A single global flag is correct here because the ordering is fixed:
`__libc_start_main` installs the main thread's TP before anything else, and
`pthread_create` is unreachable before that, so no thread can observe the flag
set without having installed its own TP.

Verified: host clippy `--all-targets` clean and `x86_64-unknown-none` clippy
clean; **10 consecutive parallel `cargo test -p posix` runs, all 20013 passed /
0 failed** (previously flaky on nearly every run). On target, the ring-3 C
fixture `services/ctest-tls-thread` was extended to assert per-thread `errno`
(each pthread gets its own `__errno_location()`, distinct from the parent's, and
starts at 0), and `scripts/boot-test.sh` passes.

**Follow-on (done):** `h_errno` was migrated into the same block immediately
afterwards — which turned up the much worse bug that nothing had ever *written*
it. See BUG-POSIX-H-ERRNO.

**Note:** while investigating this I also found and fixed a *deterministic*
stale test (`file::tests::translate_no_flags` asserted the pre-
BUG-OPENFLAGS-ENCODING return value `0`; corrected to `N_READ`). That one
always failed and was unrelated to the parallel flakiness.
