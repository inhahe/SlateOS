### [A] A task resumed from `switch_context` onto a frame holding -1001, and dropping a local faulted -- 2026-10-08

**Status:** OPEN -- seen once (debug boot 13 of lane-a, 2026-10-08), not
reproduced since; fast boots of lane-a-wip that reach the same test are the
check.

**In short:** once, the kernel died with a page fault in the scheduler while a
test program (`spawn-test-device-door`, task 814) was opening an audio
device. The scheduler was finishing a task switch -- the task had just been
switched back in -- and when it cleaned up a local variable, the address it
used for that variable was -1001 instead of somewhere on the task's stack. So
the task came back to a stack frame that had been overwritten while it was
away. Nothing a user does causes it on purpose; when it happens the whole
system halts.

**What the log shows** (`os-lane-a/build/serial-test.txt` of that boot,
kernel `os-lane-a/target/x86_64-unknown-none/debug/kernel` built 16:11):

- `EXCEPTION: Page Fault (#PF) at 0xffffffff82415684, address=0xfffffffffffffc17`,
  kernel read of a not-present page; RIP bytes `48 8b 07 48 c7 07 00 00 00 00`
  -- `mov rax,[rdi]; mov qword [rdi],0`, the `mem::take(&mut self.mask)` in
  `sched::PendingWakeSignals::flush`, with `rdi` = `0xfffffffffffffc17`.
- Symbolized (addr2line on that kernel): `flush` <- `Drop for
  PendingWakeSignals` <- `drop_in_place` <- `sched::schedule_inner`, at the
  end of the function: the drop that runs after `switch_context` returns and
  `finish_task_switch` is done. `flush` had already been called before the
  switch, so the mask was 0 and the drop is a no-op -- if `&mut self` is
  right.
- The stack scan from RSP `0xffffc100000c6fa8` has `0xfffffffffffffc17` in
  five slots, among them where saved frame pointers would sit
  (`[..6fd8]`, `[..7020]`, `[..7040]`). -1001 (`!1000`) is no `KernelError`
  code (they end at -708) and no errno.
- The last lines before it: `[spawn] Ring 3 entry` for task 814, then
  `[mixer] Opened stream 0 ("alsa-pcm")` -- the program opening the PCM device
  through the device door. The audio-out self-test had just run blocking
  writes and a blocking drain against the pump task (813, priority 4).

**Reading:** the frame `schedule_inner` resumed into was not the one it left:
either the task's saved context (its `rbp`) was overwritten while it was
switched out, or something wrote through a stale pointer into memory that is
now this task's kernel stack -- a waiter or result slot that lived on an
earlier task's stack, written after that task returned or exited and the
stack memory was reused. The second fits a value repeated in several slots
better than a single bad register. Not established: no write site for -1001
or `!1000` has been found.

**What would settle it:** a reproduction. The fast boots run since (1 and 2
of lane-a-wip) did not reach the device-door test. If it recurs, the first
step is a guard: poison-check the resumed frame in `schedule_inner` after
`switch_context` (compare a canary written before the switch), so the fault
names the switch rather than a later drop; then audit the audio pump's and
the mixer's waiters for stack-resident wait records that outlive their
callers.
