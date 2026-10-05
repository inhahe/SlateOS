## 1145. `argz_replace` counts what glibc's manual says it counts, not what glibc's code counts

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `argz_replace` replaces a piece of text in a list of strings
and can report how many replacements it made. glibc's manual says it adds
"the number of replacements performed"; glibc's code adds one for each
string it changed, however many replacements that string had. This library
does what the manual says.

| Option | *What changes:* |
|---|---|
| **The manual's** (taken) | Replacing `ab` in `ababab` adds 3. |
| glibc's code | It adds 1. |

A program that reads the count only as "did anything change" -- the usual
use -- sees no difference; one that reads the number was told by the manual
it is replacements.

**Where:** `posix/src/argz.rs` (`argz_replace`), its test.
