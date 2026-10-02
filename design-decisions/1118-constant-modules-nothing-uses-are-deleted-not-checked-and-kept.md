## 1118. Constant modules nothing uses are deleted, not checked and kept

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** the C library's source held 1,715 files of Linux and glibc
constants -- numbers copied from the kernel's and glibc's headers -- of which
the code the library exports reaches 11. Checking all of them against the
headers would have been a project of its own, for constants nothing reads;
deleting them costs nothing git cannot give back. The 1,703 nothing reaches
are deleted (one, `linux_virtio_types`, waits on the modules that use it),
and the ones in use are checked constant by constant.

**Why delete rather than check.** The files were written in batches without
being read against the headers they copy, and it shows: one was invented, the
ones in use held wrong values, and older entries in known-issues.md are about
others (`linux_perf_types.rs`' bit positions). An unchecked constant is worse
than none: it looks authoritative, and whoever reaches for it later inherits
its error. Each file's tests compared its constants with themselves, so the
tests they added to the count measured nothing.

**Precedent.** §851, for the toolkit's islands: "a component with no consumer
is either a feature the user cannot reach or code to delete" -- and git holds
it.

| Alternative | For | Against |
|---|---|---|
| **Delete the unused, check the used (chosen)** | the constants that remain are checked; 238,000 fewer lines to build and read | a constant someone wants later has to be written then -- and checked then |
| Check all 1,715, keep the correct ones | nothing deleted | a project's worth of checking, for constants nothing reads |
| Leave them | no work | 240,000 lines of unchecked "facts", some known to be wrong |

**How to reverse.** `git show <this commit>^:posix/src/linux_<name>_types.rs`
brings any one back; `lib.rs` wants its `pub mod` line again.
