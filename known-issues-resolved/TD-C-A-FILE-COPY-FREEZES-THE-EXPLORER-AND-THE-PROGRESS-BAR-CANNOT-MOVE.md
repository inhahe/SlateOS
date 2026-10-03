## TD-C-A-FILE-COPY-FREEZES-THE-EXPLORER-AND-THE-PROGRESS-BAR-CANNOT-MOVE -- FIXED 2026-09-14

**Date:** 2026-09-14. **Lane:** C. **Fixed the same day.**

**What was done.** `OperationExecutor` was split into `begin` / `step` /
`finish`, with `execute` kept as a loop over them for the callers that
genuinely want to block (the tests, and undo). The explorer holds the
operation and works at it for eight milliseconds of each frame, so the window
draws, answers clicks, counts the files up in the status bar, and can be
cancelled. `tick_interval` asks for the frame interval while an operation runs
and `None` when it stops, so a finished copy does not hold the desktop awake.

The loop body and the Move source-deletion phase were **moved, not retyped** --
four control-flow edits and nothing else, verified by reading the diff with
indentation stripped -- because that phase deletes the user's sources and its
guard carries a comment about an earlier version that deleted the wrong ones.

**What it did not fix, which is now its own entry:** a step is a *whole file*,
so one enormous file still holds the loop for the length of its copy. See
`TD-C-A-SINGLE-HUGE-FILE-STILL-BLOCKS-THE-EXPLORER-FOR-ITS-WHOLE-COPY`.

The original entry follows.

**In short:** copying or moving files in the file explorer runs the whole job
in one go, without letting the window draw in between. So for as long as the
copy takes -- seconds for a folder of photos, many minutes for a disk's worth
-- the window is frozen: it does not repaint, it does not answer a click, and
the progress bar it has sitting right there never moves, because the code that
would move it does not get a turn until the copy is already finished. There is
no way to cancel, either. The machinery for all of this exists and is tested;
what is missing is that the work is not broken into pieces the event loop can
run between.

**Where:** `apps/explorer/src/fileops.rs` --
`OperationExecutor::execute`, whose own doc comment says it: *"Run the full
operation synchronously, collecting events."* It opens the journal, calls
`run_actions`, and returns every `FileOpEvent` the operation ever emitted, all
at once, after the last byte is copied. `apps/explorer/src/main.rs` calls it in
three places (paste, drop, and the delete path), each

```rust
let mut executor = OperationExecutor::new(plan);
let events = executor.execute();
```

which is a blocking call inside an event handler.

**What is already right, and makes this a smaller job than it looks.**
Everything the engine needs to be step-wise is there:

* `run_actions` is already a `for action in &actions` loop with a
  cancellation check at the top, so the loop *body* is the step;
* `OperationProgress` carries bytes, files, current file, rate and ETA, and is
  updated after every action -- and a `FileOpEvent::Progress(..)` is pushed
  each time round, so the events a progress bar would consume are already
  being produced in the right places, just delivered too late to use;
* `OperationState` already has `Paused` and `Cancelled` variants;
* `OperationJournal` already makes an interrupted operation resumable, which
  is what allows a step to be the unit of interruption.

**What the fix looks like.** Split `run_actions` into `begin` (open the
journal, mark `Running`), `step` (one action, return whether more remain) and
`finish` (the Move source-deletion phase, which has to run after the last
action and nowhere else), with the journal, the cloned action list and the
index held on the executor across steps. `execute` stays, as
`begin(); while self.step() {} finish();` -- tests and the undo path genuinely
want to block. `main.rs` then holds the executor and steps it from the frame
clock, repainting between steps.

**Do this carefully: a Move deletes the user's only copy.** The finish phase
is guarded by `journal.transferred(index)` and the comment there records that
an earlier version deleted sources whose copy had been *skipped* or had
*failed*, destroying data. A refactor that moves that phase must keep it
downstream of every action and must not let it run on a cancelled operation.

**Why this is filed now: it is the prerequisite for a roadmap item that would
otherwise be built on sand.** `roadmap.md` 4.1 asks for *"queue a copy/move
against a drive another operation is already using, instead of running both at
once"*. There are no two operations at once to arbitrate between: `execute`
blocks its caller, so the explorer can only ever have one, and it cannot even
have one *and* a live window. Building the scheduler first would produce a
queue that never has anything in it -- the same mistake as building a settings
page for a setting nothing reads, which this file already has an entry about.

**A second thing found on the way, and it is the usual shape.** There are
**two** copies of the same-device test: `fileops::same_device` (public) and a
private `same_device` in `apps/explorer/src/dropzone.rs`, whose comment says
*"the same heuristic as `fileops::same_device`"*. Both compare the path's first
component. The dropzone's copy decides whether a drag is a **Move or a Copy**,
which is user-visible and destructive if wrong; `move_path` in `fileops`
declines to use either and attempts the rename instead, reacting to `EXDEV` --
which is the correct answer and documents why. The roadmap item above is
explicit that the comparison must be *"on the backing device or volume,
resolved from the path -- not on the mount path, and not on the path prefix"*,
so this wants one resolver in one place before anything else keys a queue on
it. Two editors of one model is fine; two models of one fact is the defect.
