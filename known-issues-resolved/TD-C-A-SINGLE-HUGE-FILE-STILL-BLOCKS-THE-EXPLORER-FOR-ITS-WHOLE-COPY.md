## TD-C-A-SINGLE-HUGE-FILE-STILL-BLOCKS-THE-EXPLORER-FOR-ITS-WHOLE-COPY -- FIXED 2026-09-14

**Date:** 2026-09-14. **Lane:** C. **Fixed the same day.**

**What was done.** A megabyte is the unit of interruption now, not a file.
`OperationExecutor` carries a `CopyCursor` -- the open source, the open
temporary, and which action they belong to -- and `copy_chunk` answers a third
outcome, `ActionOutcome::Partial`, on which the executor steps *back* to the
same action without journalling it, counting it, or giving it an undo entry.
None of those are true of a file still being written.

Two details that are not obvious and are the reason it works:

* the chunk is **filled**, not read once. `Read::read` may return fewer bytes
  than asked for at any time, so a short read says nothing; a short *fill* is
  the end of the file. Without that every file would spend one extra step
  discovering its own end, which for a folder of small files is twice the steps
  for no bytes -- and the whole existing suite passed unchanged because of it;
* the partial bytes go to the temporary `atomic_copy_file` already used, and
  the rename that gives them the real name happens only when the last one
  lands. A cancelled copy therefore leaves *nothing* under the name the user
  expects, rather than a truncated file that is indistinguishable from a small
  one. `finish` removes the temporary as well, so a stopped copy leaves nothing
  at all.

**What it deliberately did not change:** `copied_bytes` still advances a whole
file at a time, so the explorer's progress bar -- which is by files anyway --
does not creep within one enormous file. Counting part-files would need the
byte total and the file count to be reconciled at the moment a partial becomes
a whole, and the bar and the words beside it must not disagree. The window
staying alive was the defect; this is a refinement of the picture.

The original entry follows.

**In short:** the file explorer no longer freezes while copying *many* files --
it does a few, draws, does a few more. But it still freezes while copying **one
big** file, for as long as that single copy takes, because the smallest piece
of work it knows how to stop between is a whole file. Copy a folder of ten
thousand photos and the window stays alive throughout; copy one four-gigabyte
disk image and it is frozen until the image is done, with the progress bar
stuck at whatever it said when the file started.

**Where:** `apps/explorer/src/fileops.rs` --
`OperationExecutor::step_action` carries out one `PlannedAction`, and
`execute_copy_action` copies the file with a single `fs::copy`, which does not
return until the whole file is written. `ExplorerState::step_operation` in
`main.rs` has an eight-millisecond budget, and the budget cannot help: it is
checked *between* actions, so an action that takes nine seconds overruns it by
nine seconds.

**What the proper fix looks like.** Copy in chunks and make the chunk the unit
of interruption, which means an action needs to be resumable part-way:

* a `CopyCursor` on the executor -- the open source and destination handles and
  the offset reached -- so `step_action` can copy a bounded number of bytes and
  return with the action unfinished;
* `is_done` and the journal stay as they are: the journal records *completed
  actions*, and a part-copied file is not one, so an interrupted copy still
  resumes by redoing that file from the start. Making the journal record byte
  offsets is a separate and much larger change, and is not needed for this;
* the progress already carries `copied_bytes`, so a part-copied file makes the
  bar move for the first time on exactly the operation where it matters most.

**Why it was not done with the stepping change.** The stepping change was a
text-preserving carve of a path that deletes the user's sources on a Move; a
chunked copy is new code in the same function, and mixing the two would have
meant no diff anyone could read. The two are independent: everything above
works today for the case that is far more common, which is many files rather
than one enormous one.

**What a user sees until then.** A folder copy is smooth. A single large file
freezes the window for its duration and then everything catches up at once.
That is strictly better than before -- it used to be true of *every* copy --
and it is worth knowing that the remaining case exists rather than wondering
why one copy behaves differently from another.
