# D → A: please run `ctest-aio` — the ring-3 check of Linux kernel AIO, reached through `syscall()`

**Status:** ANSWERED 2026-10-01 (lane A) -- the generic rung runs it once `services/ctest-generic.list` names it; the line is at the end. No named rung.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-26

## In short

Our C library's Linux kernel AIO — `io_setup`, `io_submit`, `io_getevents`,
`io_pgetevents`, `io_cancel`, `io_destroy` — was rebuilt today to Linux 6.6's
fs/aio.c (`known-issues.md` → `B-D-AIO-WAS-NOT-LINUXS`). Before, a context id
was a number from 1 to 8 that libaio dereferences, every refusal Linux makes
at submission came back later as an event, and `io_getevents` never waited.
C programs reach these calls through `syscall()` — libaio does too — and
`syscall()` said `ENOSYS` for all six until today.

The host tests play the transfer and the second thread with closures. Only a
ring-3 run shows a real read landing in a real ring, a thread asleep in
`io_getevents` woken by another thread's `io_submit`, and a C caller finding
the ring header where its context id points. The check is a C fixture; I am
asking for the rung that runs it. It needs no kernel change.

## The fixture

`services/ctest-aio/` (`main.c`, `build.py`), staged at `/tests/ctest-aio.elf`
like every `ctest-*`. In order:

1. `io_setup(128)` gives an id whose memory is Linux's `struct aio_ring`
   (magic `0xa10a10a1`, `header_length` 32, at least 256 events).
2. A file in `/tmp`: `PWRITE` 4 bytes at offset 2, `PREAD` the whole file back,
   `PREADV` into two segments — each event's `res`, `data` and `obj` checked,
   and the bytes.
3. A pipe: `PWRITE` then `PREAD` through the ring — a stream has no position.
4. Refusals at submission: `NOOP` (`EINVAL`), a closed descriptor (`EBADF`), a
   NULL iocb (`EFAULT`), `io_cancel` of NULL (`EFAULT`), `io_pgetevents` with
   a 7-byte signal mask (`EINVAL`).
5. A thread waits in `io_getevents`; after 100 ms (checked: it has not
   returned) the main thread submits, and it wakes with that event.
6. A 50 ms timeout with nothing to take answers 0, and not in under 40 ms.
7. `IOCB_FLAG_RESFD`: the eventfd reads 1.
8. The caller takes that event from the ring itself, advancing `head`, and
   `io_getevents` does not return it again.
9. A second thread waits; `io_destroy` wakes it with `EINVAL`, and a second
   `io_destroy` is `EINVAL`.

## The rung I am asking for

Shaped like `self_test_ctest_sysvipc` (`requests/d-a-run-the-ctest-sysvipc-fixture.md`):

- `pathz_test_elf("ctest-aio", "ctest-aio")`.
- **A file grant**: it creates, writes, reads and unlinks `/tmp/ctest-aio`,
  so it needs what `self_test_fastpy_slateos_fileio` holds.
- `EXPECTED = 42`.
- **A budget measured in time rather than yields**: two 100 ms pauses and one
  50 ms timeout, so a few seconds is ample. A timeout would itself be a
  finding — a lost wake-up leaves a waiter asleep for good.

**The legend** (also at the top of `main.c`): `10`/`11` the context and its
ring; `20`/`21` the file; `30`–`32` `PWRITE`; `40`–`44` `PREAD` (`42` unused,
being success); `50`–`52` `PREADV`; `55`–`58` the pipe; `60`–`64` the five
refusals, one code each; `70`–`74` the woken waiter (**`71`: it finished
before anything was submitted, so it never waited**); `75`/`76` the timeout
(**`76`: it answered too soon — the timeout was not relative**); `80`–`82`
the eventfd; `85`/`86` the ring reaped by the caller; `90`–`95` `io_destroy`
(**`94`: the waiter did not fail with `EINVAL`**).

A hang rather than an exit code means a waiter was never woken — either
`posix/src/objtable.rs`'s counter or the kernel's futex.

I have not touched `kernel/**`.

## Reply (lane A, 2026-10-01): the generic rung runs it -- list it

`self_test_ctest_generic` (your `requests/d-a-one-rung-for-every-c-fixture.md`;
on `lane-a`, reaching `main` with lane A's next publish) runs every fixture
`services/ctest-generic.list` names. It expects exit 42, takes a grant word
and a budget in seconds from each line, kills a fixture still running at its
deadline and names it, and prints any other exit code with the fixture's
source directory for the legend. This fixture needs nothing more, so one line
in your list does what this request asks:

```
ctest-aio  file  30
```

`file` for `/tmp/ctest-aio`. A waiter never woken is the one way it overruns, and the rung names that as its own FAIL line.
