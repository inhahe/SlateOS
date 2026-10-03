## §224 — A canary check halts where a task is about to keep running, and only reports where it is already dead

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Each kernel task's stack has a known value — a "canary" — written
at the very bottom, so that a task which uses more stack than it has overwrites
that value and is caught. When the check finds the value wrong it stops the
whole machine. That is right in the middle of a context switch, where the task
is about to resume on a stack we now know is bad. It is wrong when the task is
already dead and gone, which is where the check *also* ran — there it threw away
the rest of the boot's output to prevent nothing. The decision is that halting
is a property of the **call site**, not of the condition, and the check is now
two functions rather than one.

### What was actually there

The reaper's own comment read:

> Final canary check — if the task overflowed before dying, log a warning
> (the task is already dead so we can't halt, but the corruption may have
> affected other memory).

— immediately above a call to a function whose last line is
`crate::cpu::halt_loop()`. The intent was recorded correctly and the behaviour
had been the opposite of it for as long as the call existed. Nobody noticed
because the check almost never fires; when it finally did, it took down a
boot test at the ~85% mark and destroyed the output that would have said
whether anything *else* was wrong.

### The alternatives

| | **Halt only where the stack will be reused** (chosen) | **Halt on every detection** |
|---|---|---|
| *What changes:* | a dead task's bad canary prints a post-mortem and the boot continues | any bad canary anywhere stops the machine |
| Argument for | the rest of the run's diagnostics survive, and the post-mortem is what identifies the cause | corruption is corruption; continuing runs on a machine whose memory state is unknown |

The second position is not weak and deserves stating properly: a corrupt
canary is evidence that *something* wrote where it should not have, and the
blast radius of a stack overflow is not confined to the stack — the frames
below it belong to somebody. Continuing is, strictly, continuing on a machine
whose memory you can no longer vouch for.

What settles it is that halting **does not undo any of that**. By the time the
reaper looks, the write happened tens of thousands of log lines ago; the task
is dead, removed from the table, and its stack is about to be freed. There is
no future write to prevent. The halt is a response to damage that is already
complete and already bounded — it buys no safety, and it costs the one thing
that could still identify the cause.

Whereas at the context-switch sites the calculus is exactly inverted: the task
is live, its `rsp` points into that stack, and letting it resume means the
*next* push writes past the canary into whatever lies below. There the halt
prevents a specific, imminent, unbounded write. Same condition, opposite
correct answer, because the question is not "how bad is this?" but "is
anything going to run on this stack again?"

So `check_stack_canary()` keeps the halt and keeps the context-switch callers;
`report_stack_canary()` returns a `bool` and takes the reaper.

### The corollary that generalises

**A check that halts inherits its severity from where it is called, not from
what it detects.** Copying an assertion to a second call site silently
re-decides a policy question that was answered for the first one. When the
new site's answer differs, the result is what happened here — a comment
describing the behaviour the author wanted, sitting on top of the behaviour
they got, agreeing with each other nowhere but in the reader's assumption.

### The measurement that was missing, which is the more important half

Fixing the halt makes the failure survivable; it does not make it
understandable. Two unrelated faults produce a corrupt canary — a genuine
overflow, or a stale pointer to a stack that was freed and re-issued to
someone else — and they want opposite fixes. The message that existed
(`Expected: X, Found: Y`) distinguishes them not at all.

Both are now distinguished automatically, from data the kernel was already
generating and discarding: every stack is painted with a sentinel pattern at
creation for watermark tracking. The post-mortem reports the composition of
the bottom *and* the top of the stack, and a real overflow leaves the top full
of ordinary frame data where a recycled slot has been zeroed end to end.

And a **system-wide stack census** now runs on every boot, reporting the five
deepest kernel stacks and warning above 75% of capacity. The tree already had
a `test_stack_watermark` self-test, which proved the watermark API worked by
measuring a purpose-built task that touches 256 bytes — so the question "is any
*real* kernel stack close to overflowing?" had no answer anywhere, despite the
data to answer it being painted into every stack in the system. That is the
same shape as §222: an instrument that exists, is exercised, and is never
pointed at the thing it was built for.

The 75% line is not arbitrary. It leaves 16 KiB of the 64 KiB stack free,
which is roughly the worst case for an interrupt arriving at a task's deepest
point — the IRET frame, the register save and the handler's own frames all land
on the *same* kernel stack. A task sitting quietly above that line is one
badly-timed interrupt away from writing through its own canary, which is a
plausible reading of the failure that prompted all of this.
