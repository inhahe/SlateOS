### [D] TD-D-FORTIFY-MEM-AND-STR-CHK-IGNORE-THE-OBJECT-SIZE — 2026-09-24 — FIXED 2026-09-25

**Status:** FIXED 2026-09-25. `__chk_fail` exists (`posix/src/fortify.rs`), glibc's message and then `abort()`. The ten memory and string copies check their bound and abort before writing. `__read_chk`/`__pread_chk` clamp to the object instead. When each kind applies, and why, is design-decisions.md §1105. Host tests cover every copy at its exact bound and with an unknown object size. The aborting side needs a process that can die: `services/ctest-fortify-abort`, whose rung is requested in `requests/d-a-run-the-ctest-fortify-abort-fixture.md`. Added later the same day, by the same rule: `__fdelt_chk`/`__fdelt_warn`, the `__open_2` family (it aborts on `O_CREAT`/`O_TMPFILE` without a mode, as glibc does), `__explicit_bzero_chk`, `__poll_chk`/`__ppoll_chk` (abort), and `__recv_chk`, `__recvfrom_chk`, `__gethostname_chk`, `__getlogin_r_chk`, `__ttyname_r_chk`, `__ptsname_r_chk`, `__confstr_chk` and `__getgroups_chk` (clamp). Added 2026-09-26, by the same rule (design-decisions §1105's table): the ten wide-character copies and the eight multibyte conversions (abort — a clamped conversion would hide the truncation its caller tests for), `__fgetws_chk` and `__vswprintf_chk`/`__swprintf_chk` (clamp), with `wcpcpy` and `wcpncpy`, which this libc had lacked, each in its own archive member because gnulib replaces them; the ring-3 cases are in the same fixture. Writing `__vswprintf_chk`'s tests found a bug under it, fixed with it: `vswprintf` read its own formatted output as a string without terminating it (`format_core` does not; `_snprintf_impl` does), so `swprintf` into an uninitialised buffer miscounted or failed with `EILSEQ` — every existing test had used a zeroed one. The wide printf family to streams followed the same day — `fwprintf`, `wprintf`, `vfwprintf` and `vwprintf`, which this libc had lacked altogether, with `__fwprintf_chk`, `__wprintf_chk`, `__vfwprintf_chk` and `__vwprintf_chk` over them (`posix/src/printf.rs`, checked at ring 3 by `services/ctest-printf-streams`). Still absent, so a program using them fails to link rather than fails to check: `__fgetws_unlocked_chk`, for want of `fgetws_unlocked`; `__wcslcpy_chk`/`__wcslcat_chk`, for want of `wcslcpy`/`wcslcat`; `__getwd_chk`, for want of `getwd`; and `__longjmp_chk`, since this libc has no `longjmp` of its own.

*(As filed:)* found while fixing `__getcwd_chk`.

**What:** the `_FORTIFY_SOURCE` entry points for memory and strings —
`__memcpy_chk`, `__memmove_chk`, `__mempcpy_chk`, `__memset_chk`,
`__strcpy_chk`, `__stpcpy_chk`, `__strncpy_chk`, `__stpncpy_chk`,
`__strcat_chk`, `__strncat_chk` in `posix/src/string.rs`, and `__read_chk` /
`__pread_chk` / `__pread64_chk` in `posix/src/file.rs` — ignore the object size
they are passed and do the unchecked operation. There is no `__chk_fail`. So an
object compiled against glibc headers with `-D_FORTIFY_SOURCE` and linked here
gets none of the overflow protection it was built to have.

**Why low:** nothing we build calls them. `zig cc` compiles against musl's
headers, and musl deliberately does not implement `_FORTIFY_SOURCE`, so no C
fixture or port generates these calls; only a prebuilt glibc-compiled object
would. The printf `_chk` family already makes a documented choice — clamp to
the object size rather than abort (`services/ctest-fortify/main.c`) — and
`__readlink_chk` and now `__getcwd_chk` clamp too.

**Proper fix:** add `__chk_fail` (write `*** buffer overflow detected ***:
terminated` to stderr, then `abort()`, as glibc) and have each wrapper check
its bound. For the copy functions clamping is not a meaningful alternative — a
`memcpy` that copies less than asked is a different bug, not a safe one — so
these want the abort even though the printf family clamps.
