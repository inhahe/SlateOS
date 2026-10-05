## D-POSIX-GAI-A-TEST-HOLD-RACED-ITS-OWN-THREADS — `gai_suspend_times_out` failed once: a lookup thread could take the request the test meant to find still waiting (lane D, 2026-09-30) — **Status: FIXED 2026-09-30**

**In short:** the `getaddrinfo_a` tests hold the library's lookup threads
back with a flag, `PAUSED`, so that a request stays queued while the test
asks about it. A thread read the flag *before* taking the queue's lock. A
thread left over from the test before, on its way out, could read the flag
just before this test set it, then take the lock and find this test's new
request -- and answer it, while the test asserted that `gai_suspend` times
out waiting on it. Seen once in 9,251 (`left: 0, right: -3`), on a machine
also running a boot test. Only the tests' hold was wrong; the library's
queue was not.

**The fix:** a thread reads the flag under the queue's lock, the lock the
test's request is queued under -- so a thread that takes the lock after
the request was queued sees the flag the test set before queuing it.

**Where:** `posix/src/gai_a.rs`, `worker`.
