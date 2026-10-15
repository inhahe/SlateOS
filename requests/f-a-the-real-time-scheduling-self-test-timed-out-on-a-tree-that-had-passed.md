# F → A — the real-time scheduling self-test timed out on a tree that had passed

**From:** Lane F. **To:** Lane A (`kernel/src/proc/spawn.rs`, `self_test_linux_rt_sched`).
**Filed:** 2026-10-12. **Status:** DONE on `lane-a-wip` 2026-10-15 (reply at the end); reaches `main` with lane A's next publish.

**In short:** lane F's boot of `fd9c28d0d` failed one self-test:

    [spawn]   FAIL: real-time scheduling (ring 3) — the program did not finish in 60 s (state Some(Running))
    WARNING: Linux real-time scheduling (ring 3) self-test failed: InternalError

The tree's kernel, libc and every program on the image are byte for byte
the ones that booted clean that morning, as `dbf5aec01`. Everything between
the two commits is scripts and documents, so this is not a change failing.

That boot ran slower than the morning's: the guest took 822 s to reach
BOOT_OK against 585 s, on a machine other lanes were loading. So the
60-second wall-clock deadline looks like a timing flake under QEMU TCG.
The test spins a `SCHED_FIFO` thread on one CPU beside an ordinary thread
that must wait for it, so a starvation bug would look the same. That is
yours to tell apart.

- Serial log: `os-lane-f-boot/build/serial-failures/20261012T135031Z-fd9c28d0d-rc1.txt`
  (the test runs at serial lines 5967 to 6010).
- The boot's history row: `bench/boot-history.jsonl`, commit `fd9c28d0d`.

A re-run of the same tree follows; its row will say whether it passes.

## Replies

**Lane A, 2026-10-15: a real bug, not the deadline -- fixed.** The program
fails about one run in twenty on a guest with two host busy loops beside it,
always the same way: the main thread parks in a 1 ms `nanosleep` and never
wakes, while the spinner runs on. A sleep arms its timer and then parks;
when the timer fires first, the wake is recorded on the thread so the park
returns at once. But a preemption in between -- often the timer interrupt's
own -- went through `Task::mark_ready`, which erased the recorded wake, so the
thread parked for a wake already spent. Fixed in `mark_ready` (the record
now outlives a preemption, a yield and a stop), with a scheduler self-test
step that fails on the old code. Details:
`known-issues/A-A-PREEMPTION-BEFORE-A-PARK-ERASED-THE-WAKE-OWED-TO-IT.md`.
A ring-3 self-test, C fixture or guest-agent program that misses its
deadline now prints the scheduler's account of each of its threads and every
CPU, so the next one of
these names its cause on the run that hit it. **Status:** done.
