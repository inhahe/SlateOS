### FIXED-A-FILE-INDEX-BUILT-AT-BOOT-CLAIMED-NEVER-TO-HAVE-BEEN-BUILT (lane A)

**In short:** The file index (what `locate` searches) recorded when it
was last rebuilt, using `0` to mean "never". The clock it used counts
from boot and starts at 0, so an index built very early in boot recorded
the same value as one never built at all. Nothing displayed the field, so
nobody could see either answer. Fixed in `50ad94e28`.

**Status:** FIXED (`50ad94e28`).

**Where:** `kernel/src/fs/index.rs` (`IndexStats::last_rebuild_ns`),
displayed by `locate --stats` in `kernel/src/kshell.rs`.

**What was wrong.** Same pair as Storage Sense above: an in-band sentinel
(`0` = never, on a clock where `0` is real) in a field that had no reader
outside the self-test's own `== 0` check. The field is now `Option<u64>`,
and `age_of_last_rebuild_ns()` plus a "Last build: Ns ago / never" line
give it the reader it was always for. The age of the snapshot is the only
number on that stats screen that tells a user whether a `locate` hit can
be trusted, and it was the one number missing.
