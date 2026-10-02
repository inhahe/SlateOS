## TD-C-THE-FILE-OPERATION-QUEUE-CANNOT-BE-PER-DRIVE-UNTIL-COPIES-CAN-OVERLAP -- **WITHDRAWN: it could, and it now is**

**Date:** 2026-09-14. **Lane:** C. **Corrected the same day.**

**The correction, and it is the useful part.** This entry said the per-drive
rule "would change nothing a user could measure" because `fs::copy` blocks and
the explorer runs on one thread, so two operations admitted at once would
merely interleave. That skipped a step. **Writes go to a page cache** --
`design.txt` discusses swappiness against it -- so a write returns before the
device has the data, and two interleaved copies on one thread *can* keep two
devices busy. Whether they do on this system is a **measurement nobody has
made**. "Inert" was a confident claim resting on an unchecked assumption, which
is the failure this file has more entries about than any other.

**Done 2026-09-14.** The queue is keyed on drives: `OperationPlan::drives`
resolves every path a plan touches to a `DriveSet`, `start_operation` admits an
operation whose set shares nothing with anything in flight and queues one that
does, `admit_pending` scans *past* a blocked operation so one on a free drive
does not wait behind it, and the frame's budget is split between however many
are running. No threads were needed.

**What remains unmeasured, and is now the only open part:** whether two
operations interleaved on one thread actually keep two devices busy. It rests
on writes returning before the device has the data. The *correctness* half --
never two operations on one drive -- does not depend on the answer and is what
the rule delivers today.

**What that changes.** The blocker named in the title does not exist. Multiple
operations in flight need a `Vec<RunningOperation>` and a round-robin of the
eight-millisecond budget the explorer already has -- no threads, no `Send`
audit of the executor, no channels, none of the work listed below. The
threading plan stays recorded because it is the right answer *if* a
measurement later shows the interleave buys nothing; it is not a prerequisite.

**And the half that never depended on any of this:** not running two operations
on one drive is implementable today, is the rule roadmap 4.1 actually asks for,
and is the half with the rationale that does not rest on throughput at all --
two operations on one disk interleave two access patterns into one device
queue, and fewer in flight on a drive is fewer left half-done when it is
yanked.

**How to settle the throughput question when a machine with two drives is
available:** copy two large files, once one after the other and once
interleaved a chunk at a time, on (a) one drive and (b) two. The same-drive
case should show the interleave *slower*, which is 4.1's first claim; the
two-drive case is the one this entry guessed at.

The original entry follows, and its reasoning about threads is still right
about threads -- it is only wrong that threads are required first.

**In short:** start a second copy in the file explorer while one is running and
it now waits its turn instead of being refused. What `roadmap.md` 4.1 actually
asks for is finer: a copy on *a different drive* should start **immediately**,
and only one that would touch a drive already in use should wait. That part is
not built, and building it now would change nothing a user could measure --
because the explorer can only carry out one copy at a time anyway, so "start
immediately" and "wait" reach the same finish line.

**Why the drive check would be inert.** `fs::copy` blocks until the file is
written, and the explorer runs its operations on the one thread that also draws
the window. Two operations admitted at once would therefore *interleave* their
steps rather than run in parallel: a slice of one, a slice of the other. Two
copies on two different disks finish no sooner that way than one after the
other -- and slightly later, since they alternate. The rule would be real code
with a real test and no effect on the machine, which is the shape this file has
several entries warning about.

**What it needs first: an operation that runs off the drawing thread.** Not
threads in general -- one worker per in-flight operation, with:

* `OperationExecutor` made `Send`, which means auditing what it holds: the
  journal's file handle, the plan, the collected events and errors. Nothing in
  it is obviously thread-hostile, but "obviously" is not an audit;
* progress and events crossing back over a channel instead of being read
  directly, so `ExplorerState::step_operation` becomes a drain rather than a
  stepper;
* cancellation crossing the other way -- an `AtomicBool` the executor checks
  where `is_done` checks `stopped` today;
* a cap on how many run at once that is about *drives*, not a number: the whole
  point of 4.1 is that two operations on one disk are slower than the same work
  done in turn.

**The parts that are already done, so this is smaller than it looks.**
`apps/explorer/src/drives.rs` resolves a path to its backing device properly
(and `same_drive` already answers `Option<bool>` with the "don't know" case
split, which the queue reads as "assume it collides"). The engine is stepped,
so an operation is already a sequence of interruptible units rather than one
blocking call. What is missing is only that the units run somewhere other than
the drawing thread.

**Until then.** The queue is a plain FIFO, and that is honest: a second
operation is accepted, visible in the status bar as "(N waiting)", and can be
cancelled for free before it starts. The user-visible loss against 4.1 is that
a copy to a USB stick waits behind a copy to the internal disk when it did not
have to. The user-visible *gain* over what was there before is that it is
accepted at all.
