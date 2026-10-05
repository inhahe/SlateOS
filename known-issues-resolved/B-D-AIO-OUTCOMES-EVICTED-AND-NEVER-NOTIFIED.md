### [D] B-D-AIO-OUTCOMES-EVICTED-AND-NEVER-NOTIFIED — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/aio.rs`.

**What it was.** Found in the fifteenth NULL-pointer pass, reading glibc's
rt/ in full:

- Each request's outcome lived in a 16-entry table keyed by the `aiocb`'s
  address, and the 17th request evicted the oldest outcome whether or not
  anyone had collected it. `aio_error` then answered `EINVAL` — "that I/O
  failed with EINVAL" — for a request that had succeeded: a `lio_listio` of
  17 requests lost its first. `aio_return` also dropped the outcome, so the
  common `n = aio_return(cb); if (n < 0) e = aio_error(cb);` read `EINVAL`.
- `aio_sigevent` was ignored, and `lio_listio`'s `sig`: a program waiting for
  its `SIGEV_THREAD` callback or its signal waited forever.
- `aio_cancel` answered `AIO_ALLDONE` to everything, a descriptor that is not
  open (glibc: `EBADF`) and another descriptor's `aiocb` (`EINVAL`) included.
- `aio_fsync(O_DSYNC)` ran `fsync`; a positioned read or write on a pipe or
  socket failed with `ESPIPE` where glibc falls back to a plain one; an
  interrupted one failed with `EINTR` where glibc retries.
- `aio_suspend` never waited, so a request another thread was still
  performing was reported complete.

**Fix.** The outcome lives in the `aiocb`, in the two private words musl keeps
there (`__err`, `__ret`) — no table and no limit — and reads as glibc's does,
`EINPROGRESS` while another thread performs the request. Completion is
notified as `aio_sigevent` asks: `SIGEV_THREAD` on a new detached thread,
`SIGEV_SIGNAL` by `raise`. `aio_suspend` sleeps on a futex until a listed
request completes; `aio_cancel` follows glibc's order and answers
`AIO_NOTCANCELED` for a request being performed. Host tests drive reads and
writes through an eventfd, which also takes the `ESPIPE` fallback.

**What remains, by design.** Requests are still performed in the calling
thread before the call returns, as this module always has — a program gains
no overlap from aio. `SIGEV_SIGNAL` carries no value, since `sigqueue`
delivers none yet (plain `raise` is glibc's own fallback without queued
signals).
