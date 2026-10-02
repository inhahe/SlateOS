## 1105. A fortified call that would overflow aborts if it is a copy, and is clamped if the smaller call is still correct

**Date:** 2026-09-25
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** programs built with glibc's `_FORTIFY_SOURCE` have the C library
check that each copy or read fits its destination. glibc stops the program
whenever one does not. Ours had been ignoring the check, which is what
`TD-D-FORTIFY-MEM-AND-STR-CHK-IGNORE-THE-OBJECT-SIZE` recorded. Earlier sessions
had already made the printf family, `getcwd`, `readlink`, `fgets` and `fread`
*shrink* the operation to fit instead of stopping. This decision says where
each of the two answers belongs. A **copy** that does not fit stops the
program, as in glibc. A call whose smaller version is still a correct call
of the function is **shrunk**: `read` asked for more than its buffer holds
reads less.

**The rule.** Clamp when the smaller operation is a result the function's
callers must already handle; abort when it is not.

| | overflow | here | why |
|---|---|---|---|
| `__memcpy_chk` … `__strncat_chk` (10 copies) | abort (`__chk_fail`), as glibc | a `memcpy` that copies fewer bytes than asked, or a `strcpy` whose result is unterminated, is a new bug that the caller carries on from as if it were not there |
| `__read_chk`, `__pread_chk`, `__pread64_chk`, `__fread_chk`, `__fgets_chk` | clamp to the object | a short read is part of each call's contract |
| the printf family | truncate | `snprintf` truncation is its contract, and the return value still reports the full length |
| `__getcwd_chk`, `__readlink_chk`, `__readlinkat_chk` | clamp | `ERANGE` and truncation are answers these calls already give |
| `__recv_chk`, `__recvfrom_chk`, `__gethostname_chk`, `__getlogin_r_chk`, `__ttyname_r_chk`, `__ptsname_r_chk`, `__confstr_chk`, `__getgroups_chk` (added the same day) | clamp | a short receive, a truncated name, `ERANGE`/`EINVAL`: each call's own answer for a small buffer |
| `__fdelt_chk`, `__explicit_bzero_chk`, `__poll_chk`, `__ppoll_chk` (added the same day) | abort | no smaller call is correct: the bit is inside the `fd_set` or it is not; a partial wipe leaves the secret; `poll` on fewer descriptors ignores the rest |
| the `__open_2` family (added the same day) | abort when the flags need a mode | not a size check: `O_CREAT` through the two-argument form would create the file with whatever was in the register |
| the wide copies — `__wmemcpy_chk`, `__wmemmove_chk`, `__wmempcpy_chk`, `__wmemset_chk`, `__wcscpy_chk`, `__wcpcpy_chk`, `__wcsncpy_chk`, `__wcpncpy_chk`, `__wcscat_chk`, `__wcsncat_chk` (added 2026-09-26) | abort, as glibc | the narrow copies' reason, in wide characters |
| the multibyte conversions — `__mbstowcs_chk`, `__mbsrtowcs_chk`, `__mbsnrtowcs_chk`, `__wcstombs_chk`, `__wcsrtombs_chk`, `__wcsnrtombs_chk`, `__wcrtomb_chk`, `__wctomb_chk` (added 2026-09-26) | abort, as glibc | a conversion looks like a read — it stops at a length — but the caller tests *that length* for truncation (`if (mbstowcs(buf, s, n) == n)`), so a clamp below `n` would report a whole conversion that was cut short: a silent truncation, which is the case this rule exists to rule out. `__wcrtomb_chk` aborts only when the encoding itself is longer than the buffer, as glibc 2.39's does; `__wctomb_chk` keeps glibc's older test, `buflen < MB_CUR_MAX` |
| `__fgetws_chk`, `__vswprintf_chk`, `__swprintf_chk` (added 2026-09-26) | clamp | as `__fgets_chk` and the narrow printf family: a line read in pieces is `fgetws`'s contract, and output that does not fit is `swprintf`'s own `-1` |

Either way nothing writes past the object. The rule decides whether the
program can carry on *correctly* afterwards.

**The alternatives.**

- **Abort everywhere, as glibc.** For: exact glibc behaviour, and an overflow
  bug gets fixed rather than tolerated. Against: it reverses five earlier,
  documented choices for calls where clamping is provably safe, and it turns
  a program that would have worked (a `read` loop given an overstated size)
  into one that dies.
- **Clamp everywhere.** For: nothing ever dies. Against: for a copy, clamping
  *is* the bug. The program believes `n` bytes arrived and reads the rest from
  whatever was there before. Silent data corruption is the one outcome
  `_FORTIFY_SOURCE` exists to prevent.

**What it costs.** Where this libc clamps, a fortified glibc program that
would have died on glibc keeps running here. The overflow still cannot happen,
but the bug that caused it goes unreported. If that ever matters, a
`SLATEOS_FORTIFY=strict` switch could make the clamping calls abort too.

**Where:** `posix/src/fortify.rs` (`__chk_fail`, the checks, and this table in
its module docs); the copies in `posix/src/string.rs`; `__read_chk`/
`__pread_chk` in `posix/src/file.rs`. Ring-3 proof of the abort path is
`services/ctest-fortify-abort`, whose rung is requested of lane A.

**Revisit when** a real program is found depending on glibc's abort in one of
the clamping calls. That is unlikely, since dying is what a correct program
never does.
