## D-POSIX-MUSL-HEADERS-DECLARE-NARROWER-THAN-GLIBCS — functions `libc.a` defines that glibc's headers declare for a program and musl's hide under the same feature macros: `fgetpwent`, `mempcpy`, `ecvt` for a program that asks for nothing, `strdup` and `gmtime_r` for C23 (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 for 31, by declaring them in `posix/include` as glibc does; the LFS64 names left as musl has them, on purpose**

**In short:** a C header decides what it declares by the "feature-test
macros" a program defines -- `_GNU_SOURCE` for everything, none for the
default, `-std=c2x` for C23 and so on -- and musl's headers answer more
narrowly than glibc's in places. A program written on Linux calls
`fgetpwent` or `mempcpy` with the default settings, and glibc's headers
declare them; musl's only for `_GNU_SOURCE`, so here the program did not
compile. Compiled as C23, `strdup`, `strndup`, `memccpy`, `gmtime_r`,
`localtime_r`, `timegm` and `exp10` -- all ISO C now -- were missing
outright, musl's headers predating C23. Measured by
`posix/tools/oracle/header_audit.py` (new): each header both libraries have,
in each of eleven feature-macro settings, glibc's own declarations of
`libc.a`'s names against ours.

**Fixed** -- the overlay declares each under glibc's conditions, and
`scripts/check-libc-overlay.py`, which now counts every name the overlay
declares and not only those it adds, holds them to glibc's headers like
the rest (226 names):

| Where | Names | glibc declares them | musl's header did |
|---|---|---|---|
| `<string.h>` | `strdup` `strndup` `memccpy` | also for C23 | not for C23 |
| `<time.h>` | `gmtime_r` `localtime_r` `timegm` | also for C23 | not for C23 |
| `<math.h>` | `exp10` `exp10f` `exp10l` | for C23 and `_GNU_SOURCE` | only `_GNU_SOURCE` |
| `<string.h>` | `mempcpy` `strchrnul` `strcasestr` | by default | only `_GNU_SOURCE` |
| `<stdlib.h>` | `ecvt` `fcvt` `gcvt` | by default | only `_GNU_SOURCE` |
| `<math.h>` | `lgammal_r` | by default | only `_GNU_SOURCE` |
| `<stdio.h>` | `fopencookie` (and its types) | by default | only `_GNU_SOURCE` |
| `<pwd.h>`, `<grp.h>` | `fgetpwent` `putpwent` `fgetgrent` | by default | only `_GNU_SOURCE` |
| `<strings.h>` | `bcmp` `bcopy` `bzero` `index` `rindex` `ffs` | for strict ISO C too | not for strict ISO C |
| `<netdb.h>` | `gethostbyname` `gethostbyaddr` | always | not for POSIX.1-2008 alone |
| `<malloc.h>` | `reallocarray` | here as in `<stdlib.h>` | only in `<stdlib.h>` |
| `<sys/random.h>` | `getentropy` | here as in `<unistd.h>` | only in `<unistd.h>` |
| `<time.h>` | `clock_adjtime` | here (`_GNU_SOURCE`) | only in `<sys/timex.h>` |

**Left as musl has them, on purpose:** the LFS64 names (`open64`, `stat64`,
`readdir64` and the rest, some twenty), which glibc declares for `_GNU_SOURCE` and musl 1.2.4
gives only to `_LARGEFILE64_SOURCE` -- design-decisions §1141; glibc's GNU
`basename` in `<string.h>`, which never modifies its argument, since this
library's `basename` is POSIX's, which may (a program asking for the GNU one
fails to compile, rather than being handed the other); and
`pidfd_send_signal` in strict ISO C, where musl has no `siginfo_t`. The
report also lists 178 names musl's headers declare *more* widely than
glibc's (`header_audit.py --all`), which only a strictly conforming program
defining one of them itself could notice; none is known to matter.

**Where:** `posix/include/string.h`, `time.h`, `math.h`, `stdlib.h`,
`stdio.h`, `pwd.h`, `grp.h`, `malloc.h`, `netdb.h`, and new `strings.h`,
`sys/random.h`; `scripts/check-libc-overlay.py`;
`posix/tools/oracle/header_audit.py`.
