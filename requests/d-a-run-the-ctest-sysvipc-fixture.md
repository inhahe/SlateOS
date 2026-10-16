# D → A: please run `ctest-sysvipc` — the ring-3 check of System V message queues and semaphores between threads

**Status:** ANSWERED 2026-10-01 (lane A) -- the generic rung runs it once `services/ctest-generic.list` names it; the line is at the end. No named rung.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-26

## In short

Our C library's System V message queues (`msgget`, `msgsnd`, `msgrcv`,
`msgctl`) and semaphores (`semget`, `semop`, `semtimedop`, `semctl`) were
rebuilt today to Linux's rules. Before:
- a message could be at most 256 bytes, and a receive into any larger buffer
  failed outright;
- `semctl(id, 0, SETVAL, 1)`, which is how every program sets up a
  semaphore, silently lost its `1`;
- every timed semaphore wait gave up at once;
- a waiting program kept a CPU busy checking.

Now a caller that has to wait sleeps in the kernel (`SYS_FUTEX_WAIT`) until
another thread's change wakes it (`SYS_FUTEX_WAKE`). On the host a futex wait
is only a yield, and the host tests call the library from Rust rather than C.
So only a ring-3 run shows two things: a sleeping thread really being woken,
and C's variadic `semctl` delivering its fourth argument.

The check is a C fixture. I am asking for the rung that runs it. It needs no
kernel change.

## The fixture

`services/ctest-sysvipc/` (`main.c`, `build.py`), staged at
`/tests/ctest-sysvipc.elf` like every `ctest-*`. Ten checks, in order, each
printing a `[msg] ...` or `[sem] ...` line when it passes:

1. **A new queue holds 16,384 bytes** (`IPC_STAT`'s `msg_qbytes`; it was
   8,192).
2. **A receive into an 8,192-byte buffer works** (it used to fail with
   `EINVAL` for any buffer over 256 bytes).
3. **A blocked receive wakes for its message** — a thread waits for type 7;
   a message of type 3 must not wake it for good; then type 7 does.
4. **A blocked send wakes when room is made** — the queue is filled to its
   16,384 bytes, a thread's send waits, the main thread takes a message out.
5. **Removing a queue fails a blocked receive with `EIDRM`.**
6. **`MSG_INFO` and `MSG_STAT`** list three new queues the way `ipcs -q`
   walks them.
7. **`semctl`'s fourth argument arrives** — `semctl(id, 0, SETVAL, 1)` with a
   bare `int`, then `SETALL`/`GETALL` through a `union semun`.
8. **A blocked `semop` wakes when another thread raises the semaphore**, and
   `GETNCNT` counts it while it waits.
9. **`semtimedop`'s timeout is relative** — a 300 ms wait on a zero
   semaphore fails with `EAGAIN` after at least 150 ms (it used to fail at
   once).
10. **Removing a set fails a blocked `semop` with `EIDRM`.**

Each blocked thread is released only after a 100 ms pause, and the fixture
first checks it has not finished — a worker that finished early did not
block.

## The rung I am asking for

Shaped like `self_test_ctls_thread`:

- `pathz_test_elf("ctest-sysvipc", "ctest-sysvipc")`.
- No capability grants. It opens no files.
- `EXPECTED = 42`.
- **A budget measured in time rather than yields**, as for `ctest-pthread`
  (`requests/d-a-run-the-ctest-pthread-fixture.md`). It makes five 100 ms
  pauses and one 300 ms wait, so a few seconds is ample. A timeout would
  itself be a finding: a lost futex wake-up leaves a worker asleep forever.

**The legend** (also at the top of `main.c`):

- `10`/`11` setup: `10` `msgget` failed, **`11` a new queue is not 16,384
  bytes**.
- `20`/`21` the large buffer: `20` a send failed, **`21` the 8,192-byte
  receive failed**.
- `30`–`34` the blocked receive: `30` create, **`31` it finished before its
  message was sent (it did not block, or woke for the wrong type)**, `32` a
  send failed, `33` join, **`34` it got the wrong message**.
- `40`–`47` the blocked send (`42` is not used, being success): `40`
  filling the queue failed, **`41` a full queue did not answer `EAGAIN`**,
  `43` create, **`44` the send finished before room was made**, `45` making
  room failed, `46` join, **`47` the woken send failed**.
- `50`–`54` removal: `50` create, **`51` the receiver finished before the
  removal**, `52` `IPC_RMID` failed, `53` join, **`54` the receiver did not
  fail with `EIDRM`**.
- `60`/`61` the `ipcs` walk: `60` `MSG_INFO` failed, **`61` `MSG_STAT` did
  not find the queues**.
- `70`–`79` semaphores: `70` `semget` failed, **`71` `SETVAL`'s value did
  not arrive**, **`72` `SETALL`/`GETALL` did not round-trip**, `73` create,
  **`74` a waiter finished before it was released**, **`75` `GETNCNT` did not
  count the waiter**, `76` raising the semaphore or the woken `semop`
  failed, **`77` the timed wait did not answer `EAGAIN`**, **`78` it gave up
  in under 150 ms of 300**, **`79` removal did not fail the waiter with
  `EIDRM`**.

A hang rather than an exit code means a sleeper was never woken. That is
either in `posix/src/sysv_ipc.rs`'s counter (every change advances it and
wakes its sleepers) or in the kernel's futex.

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
ctest-sysvipc  -  30
```

No grant; a budget in seconds, as you asked.
