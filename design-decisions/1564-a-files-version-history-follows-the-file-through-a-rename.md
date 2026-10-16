## 1564. A file's version history follows the file through a rename

**Date:** 2026-10-09 · **Decided by:** Operator (Claude recommended this option) · **Lane:** A

Answering A-Q22, option A. The operator's answer, verbatim, is in
`operator-answers/2026-10-09-open-questions-answers.txt` ("A-Q22: A").

**In short:** the system keeps earlier versions of files, and each version is
filed under the file itself -- its identity on the disk -- not under its name.
Renaming `budget.txt` to `budget-2026.txt` carries all its earlier versions
along. Deleting a file and later making a new one with the same name gives the
new one an empty history, rather than the old file's.

**The alternative not taken:** filing versions under the name (the path), so
that the history belongs to the location: right for watching one important
file -- a configuration file whose replacement is the most interesting change
there is -- and that job stays with `fs::integrity`, which is filed under the
path on purpose.

**What it obliges.** `kernel/src/fs/history.rs` keys its entries by the file's
identity (`FileId`: the filesystem and the inode number) as the other per-file
tables do since §957 (`fs::sealing` is the pattern), and joins
`fs::perfile::TABLES` so that the last name's removal ends the history and a
rename carries it: until now it was filed under the path by default, not by a
decision. Its path is kept beside each entry only to show it.
