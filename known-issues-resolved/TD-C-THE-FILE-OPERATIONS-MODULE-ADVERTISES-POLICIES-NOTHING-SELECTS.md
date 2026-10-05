## TD-C-THE-FILE-OPERATIONS-MODULE-ADVERTISES-POLICIES-NOTHING-SELECTS -- FIXED 2026-09-28

**Date:** 2026-09-16. **Lane:** C.
**Where:** `apps/explorer/src/fileops.rs` — the module doc, `ConflictPolicy`,
`ErrorPolicy`, `ExecutorConfig`.

**Status:** FIXED (lane E, 2026-09-28). The blanket allow is gone and the file
builds without a dead-code warning. Each finding was wired or removed on
whether anything should call it:

| finding | now |
|---|---|
| `ConflictPolicy::Ask`, `::Overwrite`, `::OverwriteIfNewer` | wired 2026-09-27: the window asks, and the menu offers the rest |
| `ErrorPolicy` | **wired**: the window's operations stop at a file they cannot carry out and ask -- try again, skip, skip all, stop -- where they used to skip it and say so at the end. `SkipAndContinue` is what "Skip all" makes the policy for the rest, and what the folder menu's "When a file cannot be done" makes it for good, for a copy left running unattended (design-decisions §1228) |
| `ErrorPolicy::StopOnFirst`, `::RetryN` | **removed**: nothing chose them. Stopping is one of the answers, given when the user can see what failed, and "Try again" is a retry the user times -- a drive plugged back in, a file closed elsewhere -- where `RetryN` retried at once, when nothing had changed |
| `ExecutorConfig`, the `Progress`, `Conflict` and `UndoAvailable` events, four `OperationSummary` fields | **removed**: nothing read them. The window reads progress and questions from the executor, not the event stream |

Found on the way and fixed with it: a failure was counted as skipped as well
as failed, so three files with one failure reported one done; a file that
failed part-way now starts again from its first byte when tried again (it
would have gone on after the chunk whose write failed, leaving that chunk
out); the end no longer repeats, in a dialog, a failure the user has just
answered; and the explorer's two prompts stand on a panel -- under borders a
card is an outline alone, and the taken-name prompt's words sat on the
dimmed listing with its rows showing through. `apps/explorer/mutate.py`
covers each.

**In short:** the file-operations module lists what it offers at the top of the
file: conflict resolution policies, per-file error handling (skip, retry,
stop). Several of those settings exist as code and can never be chosen — no
part of the program ever selects them. The list is a promise the file does not
keep, and a blanket `#![allow(dead_code)]` at the top is why nobody noticed.

**Measured** by deleting the suppression and building: ten findings. The ones
that matter:

| never constructed | advertised as |
|---|---|
| `ConflictPolicy::Overwrite`, `::OverwriteIfNewer`, `::Ask` | "Conflict resolution policies" |
| `ErrorPolicy::StopOnFirst`, `::RetryN` | "Per-file error handling (skip, retry, stop)" |
| `ExecutorConfig` | — |
| `OperationEvent::UndoAvailable` | — |

So a copy that hits an existing file cannot be told to overwrite it, and a
failing batch cannot be told to stop on the first error, though both read as
supported from the module's own summary.

**The suppression is the finding as much as the variants are.**
`#![allow(dead_code)]` covers three thousand lines, so the compiler has been
unable to say any of this since the day it was added. It is left in place for
now -- removing it means deciding, variant by variant, between wiring and
deleting -- but it now carries a comment saying what it hides and pointing
here. A silent suppression and a documented one cost the same at runtime and
are not at all the same thing to read.

**Do not simply delete the unused variants.** The pattern today is that a
capability with no caller is either wired or removed, and which one depends on
whether anything should be calling it. A conflict dialog that offers
"overwrite" is a reasonable thing for a file manager to have, and `Ask` exists
because someone expected one. Deleting them decides that question by acting,
which is the trap `TD-C-A-CLEANUP-THAT-REMOVES-A-DISTINCTION` records five
times over.

**Also corrected here:** the module doc claimed a crashed operation could be
resumed from the journal. It cannot -- the journal holds a plan id and finished
indices, not the plan -- and the doc now says so. See `roadmap-detailed.md`
§4.1 under durable bulk operations.
