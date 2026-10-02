### BUG-POSIX-H-ERRNO. `h_errno` was a process-global `static mut` that *nothing ever wrote* — every resolver failure reported "no error" — 2026-07-30 — ✅ **RESOLVED 2026-07-30**

**What.** Two bugs in one variable, found while migrating the netdb result
buffers (TD-POSIX-TEST-PARALLEL):

1. **Never written.** `posix/src/socket.rs` exported `pub static mut h_errno:
   i32 = 0` and `__h_errno_location()` returning its address, but *no code path
   assigned to it*. `gethostbyname`/`gethostbyname2`/`gethostbyaddr` all
   returned NULL on failure and left `h_errno` at whatever it was — normally 0,
   i.e. "no error". Any C program following the documented idiom (`if (!hp)
   herror(argv[1]);`) printed **"Resolver Error 0 (no error)"** for every
   failed lookup. `herror` itself was therefore useless.
2. **Process-global.** glibc exposes it per-thread via `__h_errno_location()`
   with `#define h_errno (*__h_errno_location())`. Ours was one variable for
   the whole process, so once (1) was fixed two threads doing concurrent
   lookups would clobber each other's error code. This matters more than for
   most shared state because the codes *numerically overlap* `errno`
   (`HOST_NOT_FOUND == EPERM == 1`), so a crossed value looks plausible.

**✅ RESOLVED 2026-07-30.** Storage moved into `perthread::PerThread` (field
`h_errno`, next to `errno`); `__h_errno_location()` now returns
`&raw mut (*perthread::current()).h_errno`. The exported `h_errno` *data*
symbol was **deleted** rather than kept as a stale alias — a program that
declares `extern int h_errno;` and reads it directly now gets a link error
instead of a silently-wrong answer, which is what glibc's macro-only header
effectively achieves. All three lookup functions now set it: `NO_RECOVERY` for
caller errors (null pointer, wrong address family/length), `HOST_NOT_FOUND` for
a name no resolver could answer for, `NO_DATA` for an AF_INET6 lookup (the name
may be fine — we just have no AAAA path), and for a syscall failure a new
`resolver_error_for()` mapping `ENOENT → HOST_NOT_FOUND`,
`EAGAIN`/`ETIMEDOUT`/`ENETUNREACH`/`EHOSTUNREACH` → `TRY_AGAIN`, else
`NO_RECOVERY`. Success resets it to 0. 8 new tests cover the mapping (including
`i64::MIN` and out-of-`i32`-range returns, which must not wrap into a retryable
code), the per-thread isolation, and that `h_errno` and `errno` are distinct
storage.
