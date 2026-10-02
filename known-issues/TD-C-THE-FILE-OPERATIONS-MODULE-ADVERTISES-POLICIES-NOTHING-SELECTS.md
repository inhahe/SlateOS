## TD-C-THE-FILE-OPERATIONS-MODULE-ADVERTISES-POLICIES-NOTHING-SELECTS

**Date:** 2026-09-16. **Lane:** C.
**Where:** `apps/explorer/src/fileops.rs` — the module doc, `ConflictPolicy`,
`ErrorPolicy`, `ExecutorConfig`.

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
