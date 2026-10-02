## B-EXPLORER-A-MOVE-DELETED-SOURCES-IT-HAD-NEVER-COPIED (lane C, 2026-08-16) — FIXED

**In short:** Cutting-and-pasting files in the file explorer could destroy them.
If a file could not be copied to the destination — because a file of that name
was already there and the user had chosen "skip", or because the copy simply
failed — the explorer deleted the original anyway. The user's only copy of that
data was gone, and the operation reported success. Fixed in `3b0056f7a`.

**Where:** `apps/explorer/src/fileops.rs`, `OperationExecutor::run_actions`, the
Move source-deletion phase.

**The defect.** The phase ran over every planned action, gated only on
`self.progress.state != OperationState::Failed`:

```rust
if operation == FileOperation::Move && self.progress.state != OperationState::Failed {
    for action in &actions {
        if action.is_dir { continue; }
        let _ = fs::remove_file(&action.src);
    }
    …
}
```

`execute_copy_action` returns `Ok(ActionOutcome::Skipped)` — having copied
nothing — in four situations: `ConflictPolicy::Skip` with the destination
occupied, `OverwriteIfNewer` where the source was not newer, `ConflictPolicy::Ask`
(which currently falls through to a skip because there is no way to ask yet), and
any action whose copy failed under `ErrorPolicy::SkipAndContinue` or that
exhausted `ErrorPolicy::RetryN`. None of those put the source's bytes at the
destination. All of them had the source deleted.

The `Skip` case is the worst of the four because it is the *normal* one: a user
moving a folder into a folder that already holds same-named files, choosing
"skip", and losing every one of those files from the source. What sits at the
destination is a *different* file that happened to share a name.

**Why it was invisible.** `let _ =` on the removal meant a removal that failed
was equally silent, so the only two observable outcomes were "moved" and
"moved" — the summary counted `succeeded` from `completed_files - skipped` and
never consulted what the deletion phase actually did.

**The fix.** The journal now records *whether an action transferred data*, not
merely that it finished: `mark_skipped` writes a ` skip` suffix and
`transferred(index) -> bool` answers the question the deletion phase has to ask.
A source is removed only when `journal.transferred(action.index)`. Removal
failures go through `FileOpEvent::Error` and into the summary's `failed` count.
A source directory left non-empty *because* something under it was skipped is
the already-reported consequence of that skip, so it does not add one error per
ancestor directory.

**Two further defects found in the same read:**

1. **The journal had no identity.** `.fileop-journal` stores *plan-relative*
   action indices, so a journal left in a destination directory by any earlier
   operation would be read as the next operation's progress — every colliding
   index treated as already done, i.e. never copied, and reported as success.
   Reaching that state needed only one interrupted operation, because
   `journal.remove()` was itself a `let _ =`. `OperationJournal::open` now takes
   a plan fingerprint (`OperationPlan::id()`), writes it as a `plan <id>` header,
   and discards any journal it cannot attribute to the running plan.
2. **`atomic_copy_file` leaked its temporary on both error paths.** A failed
   copy of a large file left a full-size `.<name>.fileop-tmp` in the user's
   destination directory under a name they never asked for. Both the `fs::copy`
   and the `fs::rename` error paths now remove it.

**Tests:** six regression tests in `fileops::tests` —
`a_skipped_move_does_not_delete_the_source`,
`a_failed_move_does_not_delete_the_source`,
`a_successful_move_still_deletes_the_source`,
`a_partly_skipped_directory_move_keeps_what_it_did_not_copy`,
`a_journal_from_another_plan_is_discarded`,
`a_stale_journal_does_not_swallow_a_copy`, plus
`journal_distinguishes_a_skip_from_a_copy`. Injection-verified: restoring the
unconditional delete fails exactly the three data-loss tests and nothing else.

**Portable failure injection worth reusing.** To make a copy fail while leaving
its source intact, put a *directory* at the destination path. The final
`fs::rename` of the temporary onto it fails on every platform, and unlike
removing the source or changing permissions it leaves the source exactly where
the deletion phase would find it.

**How it was found:** the `apps/**` bug-hunt sweep, scanning for the pattern
"a user-visible action wired to a function whose return value is the whole
point, with the caller discarding it" (`let _ =`, `.ok();`) before the first
`#[cfg(test)]` line. Three of the four `let _ =` sites in this file were the bug.
