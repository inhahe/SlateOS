### [A] The guest channel boots the whole self-test suite before its agent answers -- 2026-10-03

**Status:** OPEN

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
The module doc's "~5 min" is also wrong by about a factor of three.

## The proper fix

A boot option, `selftest.skip=1`, that `kernel_main` honours by skipping
the self-test phase. `guest.py start` passes it, through the same
`limine.conf` cmdline the boot test's `SLATE_CMDLINE` writes. The guest then
comes up as soon as the services are up, whatever state the suite is in.

The work is in `kernel_main` (`kernel/src/main.rs`): the self-tests are
interleaved with initialisation, and each `selftest::dispatch*` call
evaluates its test before `dispatch` sees it. So skipping means gating the
test calls themselves, not adding a check inside `dispatch`. That is a
restructure of the self-test sections into blocks under one
`selftest::enabled()`, keeping every initialisation step outside them.
Measure the "agent answers" time after it, and fix the module doc's figure.

## Where

- `kernel/src/main.rs`: the self-test calls in `kernel_main`.
- `kernel/src/selftest.rs`: where `keep_going` is read; `skip` goes there.
- `scripts/guest.py`: `start` and `qemu_command`.
