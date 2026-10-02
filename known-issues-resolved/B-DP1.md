### B-DP1. `validate_user_range` rejected committed-but-not-yet-faulted-in demand-paged user buffers (EFAULT on large fresh output buffers) — FIXED 2026-06-16

**RESOLVED 2026-06-16.** `kernel/src/mm/user.rs::validate_user_range`
(the core of `validate_user_read`/`validate_user_write`) walked every
4 KiB page of a user buffer and returned `InvalidAddress` the moment
`page_table::translate()` reported a page *not present*. That is wrong
for **demand-paged** memory: a freshly-`malloc`/`mmap`'d buffer is
committed (covered by a VMA) but its pages are not populated until first
touched. A syscall handed such a buffer as an *output* target would
EFAULT on every page past the first, because the process had not yet
written to those pages itself.

**Reproduce:** run `dash -c 'echo /globdir/* > out'` (Path-Z real-glibc
self-test `self_test_linux_real_glibc_shell_glob`). glibc's `opendir`
allocates a 32 KiB dirent buffer and calls `getdents64` into it before
touching it; the buffer's later pages were not present, so
`validate_user_write(dirp, 32768)` returned EFAULT, `readdir` returned
NULL, and dash's glob matched nothing — emitting the literal `/globdir/*`
instead of the three filenames. (The directory open, VFS readdir, and
getdents64 encoding were all proven correct via tracing; the validation
pre-walk was the sole culprit.)

**Fix:** when the pre-walk finds a not-present page, call the new
`try_fault_in_user_page(addr, need_writable)`, which synthesizes an x86
page-fault error code (not-present + user + write-iff-needed) and routes
it through `crate::proc::pcb::try_resolve_fault` — the same demand-paging
resolver the hardware #PF handler uses — then re-checks `translate()`.
This mirrors Linux's `get_user_pages()` faulting pages in before a
kernel-side access. A genuinely unmapped or permission-violating address
still fails (the resolver returns `false`), so invalid pointers are still
rejected. **Validated:** the dash glob self-test now reads back the
expected 45 bytes (`/globdir/a.txt /globdir/b.txt /globdir/c.txt\n`),
exit 0; full boot test passes with no self-test failures.
