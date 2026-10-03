### [A] The guest channel boots the whole self-test suite before its agent answers -- 2026-10-03

**Status:** FIXED on `lane-a-wip` 2026-10-03, awaiting a boot and a measured
start time (see "Fixed" below).

**In short:** `python scripts/guest.py start` is meant to give a running
SlateOS to copy programs into in minutes (C-Q11 idea 1, design-decisions
1534). It boots exactly what the last boot test built, kernel command line
included, and that kernel runs every boot self-test before anything else: about
13 minutes under emulation (rq43's QEMU phase was 814 s). Any self-test that
panics, rather than reports a failure, stops the guest before the agent
starts. So the guest is only as available as a green boot.

## What happened

On 2026-10-03, `guest.py start` on rq43's build ran its self-tests for a
while, and would have died at the DMA allocator's self-test panic
("DMA alloc Below16M: OutOfMemory") that ended rq43. It was stopped by hand.
The "~5 min" in lane A's reply to lane C
(`requests/c-a-two-ways-to-test-a-change-without-a-full-boot.md`) is also
wrong by about a factor of three for a guest that runs the suite.

## Fixed

Two boot options, which `guest.py start` adds to the first entry's
`cmdline:` in its own copy of the ESP's `limine.conf`:

- `selftest.skip=1` (`kernel/src/selftest.rs`, `skip`): `selftest::dispatch`
  and `dispatch_debug` return without running the test. They now take the
  test as a closure (`|| x::self_test()`), since an argument is evaluated
  before the call it is passed to; all 925 calls in `kernel_main` were
  rewritten. A check inside a test that reports an already-computed result
  uses `selftest::report_debug` (`proc::spawn`'s eleven).
- `bench.skip=1` (`kernel_main`): the boot benchmark task is not started.

`scripts/boot-test.sh` refuses `selftest.skip` in any form in
`SLATE_CMDLINE`, since a boot test that runs no self-tests would pass on any
kernel, and refuses `bench.skip` under `--bench`.

What is left: measure the time to "the agent answers" on a build with the
change, and put the figure in the reply to lane C. Three self-test calls are
not dispatched and still run (`layout_pad`'s, `mm::swap::self_test_disk`,
`fs::numastat::self_test_adoption`), and `sched::fpu::stress_test`; each is
milliseconds.

## Where

- `kernel/src/selftest.rs`: `skip`, `dispatch`, `report`, `cmdline_flag`.
- `kernel/src/main.rs`: the dispatch calls; the bench task's spawn.
- `scripts/guest.py`: `GUEST_CMDLINE_WORDS`, `guest_limine_conf`, `start`.
- `scripts/boot-test.sh`: the refusal, after the option loop.
