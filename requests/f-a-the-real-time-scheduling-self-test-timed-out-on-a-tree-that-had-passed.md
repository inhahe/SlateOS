# F → A — the real-time scheduling self-test timed out on a tree that had passed

**From:** Lane F. **To:** Lane A (`kernel/src/proc/spawn.rs`, `self_test_linux_rt_sched`).
**Filed:** 2026-10-12. **Status:** open.

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
