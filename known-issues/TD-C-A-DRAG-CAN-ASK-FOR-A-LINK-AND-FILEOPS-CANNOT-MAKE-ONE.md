## TD-C-A-DRAG-CAN-ASK-FOR-A-LINK-AND-FILEOPS-CANNOT-MAKE-ONE (lane C, 2026-08-25) -- **FIXED 2026-09-07, with one path unverified here**

**Fixed 2026-09-07 (lane C).** `FileOperation::Link`, `OperationPlan::plan_link`
and `execute_link_action` exist with the apparatus every other operation has,
and the refusal in `evaluate_drop` is gone -- an Alt-drag now makes a link.

**A link plan makes one action per source and never walks a directory.**
Linking a folder means one link *to* the folder; a plan that recursed would
produce a tree of links to each file inside it, which is not the gesture and is
not undoable as one thing. `total_bytes` is zero, because counting the target's
size would put a progress bar on a transfer that is not going to happen.

**Undo removes the link and never the target.** This is the dangerous case the
entry named, and it needed its own code rather than the copy undo: `is_dir()`
*follows* a symlink, so the copy arm would reach through a link to a folder.
`remove_link_or_file` asks `symlink_metadata`, which does not follow. The same
helper guards replacing an existing link on conflict, where following one would
delete what the old link pointed at to make room for a new one.

**`OverwriteIfNewer` is treated as plain overwrite** and says so at the arm: the
comparison is between contents' timestamps and a link has no contents of its
own.

### The honest part: two tests cannot run on this machine

Windows refuses `symlink_file` without a privilege (error 1314, confirmed by
probing rather than assumed), so **the link-creation success path is
unverified on this host**. Two tests --
`a_link_action_creates_a_link_that_resolves` and
`undoing_a_link_removes_the_link_and_not_its_target` -- probe for symlink
support, print `SKIPPED` with the reason, and return. They are not `#[ignore]`d
and not silently absent: a run on a host that can make links is visibly a
stronger run than one that cannot, and the output says which happened.

What *is* verified here: the plan shape, the zero byte count, both
`remove_link_or_file` branches that do not involve a link, and that an Alt-drag
either produces a symlink or reports a failure -- never a plain copy, which is
the silent downgrade the entry says is worse than doing nothing.

Original entry follows.


**In short:** Holding Alt while dragging a file is the standard way to ask for
a *symbolic link* — a small stand-in file that points at the real one, so the
same file appears in two places without being copied. The explorer understands
the request and the file-operation engine cannot carry it out, so the explorer
refuses it to the user's face instead of pretending. Nothing is broken; a
feature is missing and says so.

**Where it lives.** `apps/explorer/src/dropzone.rs` computes
`DropOperation::Link` from the Alt modifier, correctly — that part is done.
`apps/explorer/src/fileops.rs`'s `FileOperation` enum has `Copy`, `Move`,
`Delete`, `Recycle` and `Restore`, and no `Link`; there is no `plan_link`.
`ExplorerState::evaluate_drop` (`apps/explorer/src/main.rs`) closes the gap by
marking a `Link` drop invalid with the reason `"Links are not supported yet"`,
which the hover feedback shows in red and the status bar repeats on release.
Covered by `an_alt_drag_is_refused_rather_than_silently_doing_nothing`.

**Why it is refused rather than downgraded.** Silently doing a Copy instead
would be worse than doing nothing: the user asked for a stand-in and would get
a second independent file, which then drifts out of step with the original with
no sign that it ever was one.

**The proper fix.** Add `FileOperation::Link` to `fileops` with the same
apparatus every other operation has: an `OperationPlan::plan_link`, a journal
entry so a crash mid-batch is recoverable, per-file error collection, and an
undo entry whose reverse is `remove_file` on the link (never on its target).
Then delete the special case in `evaluate_drop`. The one real design question
is what to do on a filesystem that refuses symlinks — Windows needs a
privilege for them, so the host test suite cannot assume they work — which
argues for `ErrorPolicy::SkipAndContinue` reporting a per-file failure rather
than a plan that refuses up front.

**Cost of leaving it.** One gesture is unavailable and says so. It does not
get worse with time and blocks nothing.
