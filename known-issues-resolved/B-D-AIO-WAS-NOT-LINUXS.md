### [D] B-D-AIO-WAS-NOT-LINUXS — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/linux_aio_abi.rs`; the `syscall()` routes in
`posix/src/sys_syscall.rs`.

**What it was.** Linux's kernel asynchronous I/O -- `io_setup`, `io_submit`,
`io_getevents` and the rest -- as a pool of eight contexts of at most 256
events, whose ids were the numbers 1 to 8. libaio's `io_getevents` reads the
ring header at the id's address, so any libaio program would have faulted at
address 1. Every refusal Linux makes when a request is submitted -- a bad
descriptor, an unknown command, a bad flag, a bad buffer -- came back later
as a completion event instead, with `io_submit` reporting success;
`io_getevents` ignored its timeout and answered `EAGAIN` where Linux waits;
a request blocked in its transfer (a read of an empty pipe) held a spin lock
every other AIO call spun on; `io_cancel` and `io_pgetevents` did not exist;
and the four functions were exported under libaio's names with `syscall()`'s
return convention, which libaio's callers do not expect -- while
`syscall(SYS_io_setup, ...)`, the way C actually reaches them, said `ENOSYS`.

**Fix.** Linux 6.6's fs/aio.c inside one process: a context is a ring laid
out as Linux's `struct aio_ring` and its id is the ring's address; requests
are accounted as `reqs_available` accounts them, including events the caller
reaps by moving `head` itself; `__io_submit_one`'s checks in its order, each
refusal synchronous; `io_getevents` waits on a futex, with a relative
monotonic timeout, and is woken by another thread's `io_submit` or by
`io_destroy`, which in turn waits for the calls still inside the context;
`io_cancel` and `io_pgetevents`; and the six numbers routed through
`syscall()`, with the names no longer exported (design-decisions §1114).

**What remains.** `IOCB_CMD_POLL` is refused (`B-D-AIO-HAS-NO-POLL`); the
transfer is still the ordinary calls, so a file's `pread` moves its shared
position meanwhile (`B-D-PREAD-MOVES-THE-SHARED-FILE-POSITION`); and the
ring-3 check of the whole path is `services/ctest-aio`.
