## 1115. A NULL C callback is harmless where glibc never calls it; where it would, the call fails if it can, and the process ends if it cannot

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** C lets a program pass NULL for a function the library is to
call back -- the comparison `qsort` sorts by, the routine a new thread starts
in. glibc does not check: it calls the NULL when it gets there, and the
program crashes. Our library used Rust types that cannot hold NULL for
fourteen of these, which is worse than a crash -- it is undefined behaviour.
They hold NULL now, and this is the rule for what happens next.

**The rule.**

1. Where glibc would never call the function for the input it was given --
   an empty array, an empty tree, a `pthread_once` already done, a walk with
   nothing to report -- a NULL is harmless, and the call does what glibc's
   does.
2. Where glibc would call it and fault, and the call has a failure return
   its callers check -- `ftw`, `nftw`, `pthread_create`, `pthread_once`,
   `tsearch` -- it fails with `EFAULT`, this library's substitute for the
   fault (§303), at the fault's place in the order.
3. Where it has none -- `qsort` returns nothing, and `bsearch`, `tfind`,
   `tdelete`, `lfind` and `lsearch` return only answers -- the process ends,
   as glibc's does, with a line on standard error naming the call
   (`libc_fatal`, glibc's `__libc_fatal`). So does
   `__cxa_thread_atexit_impl`, whose result no compiler reads.
4. Where glibc asserts the function is not NULL -- `atexit`,
   `at_quick_exit`, `on_exit`, `__cxa_atexit` (glibc bug 20544) -- the
   call's documented failure: -1 with `EINVAL`, nothing registered. That is
   §300's rule for a crash with a documented error to put in its place.

A NULL *data* pointer glibc dereferences in a call of the third kind
(`qsort`'s array, `lfind`'s count, `lsearch`'s key) is treated as rule 3
treats a NULL function, rather than returning as though nothing were wrong.

| Alternative | For | Against |
|---|---|---|
| **The rule above (chosen)** | glibc's answer wherever it has one; a failure wherever one can be reported; never a plausible wrong answer | three outcomes to remember, and the fatal ones can only be read, not tested on the host |
| `EFAULT`, or "not found", everywhere | one outcome; the program always goes on | `bsearch`, `tfind` and the rest would answer "not found" for a search never made, and `qsort` leave the array unsorted with no way to say so -- a crash turned into silently wrong data |
| End the process everywhere | one outcome, the same as glibc's crash | throws away failure returns callers check (`ftw`'s -1, `pthread_create`'s error number), and goes against §300 for the `atexit` family |
| Register nothing and succeed, for the `atexit` family (the behaviour until this entry) | harmless: a NULL handler has nothing to run | a success glibc never gives -- its assertion ends the program -- and the program is never told |

**How to reverse.** Each call's NULL branch is one `let … else` where glibc
would make the call, and `libc_fatal` is one function; changing an answer is
local to its call.
