### [D] B-D-C-CALLBACKS-COULD-NOT-BE-NULL — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/ftw.rs`, `search.rs`, `stdlib.rs` and `pthread.rs`,
and the exit handlers in `crt.rs`.

**In short:** Many C library calls take a function to call back -- the
comparison `qsort` sorts by, the routine a new thread starts in, the visitor
of a directory walk. C lets a program pass NULL there, by mistake. Fourteen
of ours declared the parameter with a Rust type that cannot be NULL, so a
NULL broke a rule the compiler builds on, and what happened next was
undefined -- possibly nothing visible, possibly memory corruption. Now each
takes NULL and answers it as glibc's behaviour shapes: harmlessly where glibc
never calls the function, and otherwise with an error or, where the call has
no way to report one, by ending the program with a message, as glibc's
crashes.

**What each does with a NULL now** (design-decisions.md §1115):

| call | when glibc would not call it | when it would |
|---|---|---|
| `ftw`, `nftw` (and the `64` names) | a root with nothing to report: the walk's own error | -1 and `EFAULT`, at the first entry |
| `pthread_create` | -- | `EFAULT`, once the thread's memory is had |
| `pthread_once` | done, or being done by another thread: 0 | `EFAULT`, and the once can still be run |
| `tsearch` | an empty tree: the key is inserted | NULL and `EFAULT` |
| `tfind`, `tdelete` | an empty tree: NULL | the process ends |
| `lfind`, `lsearch` | an empty array: NULL, or the key appended | the process ends |
| `qsort`, `qsort_r` | fewer than two elements: nothing | the process ends |
| `bsearch` | an empty array: NULL | the process ends |
| `atexit`, `at_quick_exit`, `on_exit`, `__cxa_atexit` | -- | -1 and `EINVAL`, nothing registered (glibc asserts); they succeeded, registering nothing |
| `__cxa_thread_atexit_impl` | -- | the process ends (glibc's ends when the thread exits); it registered nothing |

**Beside them.** `lfind` and `lsearch` refused a NULL key, a NULL array and
a width of 0 without comparing -- glibc checks none of them, and hands
whatever it is given to `compar` -- and `bsearch` refused a size of 0; those
checks are gone. A NULL count `lfind` would read, a NULL array `qsort` would
write through, and a NULL key or array `lsearch` would copy through end the
process now, where they were "not found" or "nothing to do".

**Tests.** The host tests take every case in the table where the program
goes on. The ones that end it are one line each, a call of `libc_fatal`
where glibc would make the call, and are there to be read: a test cannot
take its own process down.
