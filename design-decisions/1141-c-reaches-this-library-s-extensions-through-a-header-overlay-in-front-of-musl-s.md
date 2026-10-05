## 1141. C reaches this library's extensions through a header overlay in front of musl's: `-I posix/include`, each header `#include_next`ing musl's and declaring the rest as glibc 2.39 does

**Date:** 2026-09-29
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a C program here is compiled against musl's headers, and those
declare only what musl has. This library has more -- glibc's extensions
(`error`, `fts_open`, `backtrace`, `close_range`, `arc4random` ...) and
C23's additions (`nextup`, `fadd`, `fegetmode` ...), 195 functions and
variables in all -- and C could call none of them, because clang refuses a
call to a function no header declares. They are declared now by the headers
in `posix/include/`, which a build puts in front of musl's with `-I`: each
includes musl's header of the same name and adds what is missing, under the
feature macros glibc uses (`rawmemchr` only with `_GNU_SOURCE`, `j0l` by
default, `fadd` for C23). Five headers musl has no version of at all --
`<fts.h>`, `<error.h>`, `<execinfo.h>`, `<gnu/libc-version.h>`,
`<sys/pidfd.h>` -- are whole files there. A program written for glibc then
compiles as it does against glibc, and a gate holds every declaration to
glibc's: the same header, the same type, visible under exactly the same
feature macros.

**Alternatives:**

- **Patched copies of musl's headers** in the sysroot. One place per
  declaration and no `#include_next`; but every musl update becomes a merge,
  and the change is a diff against a vendored tree instead of a file that
  says what it adds and why.
- **One SlateOS header** declaring everything. Simple -- but a glibc
  program includes `<stdio.h>` for `renameat2`, not a header of ours, and
  would not compile unchanged, which is the point of having the functions.
- **glibc's headers themselves.** They declare exactly glibc's set; but
  they are LGPL (open-questions D-Q6), declare hundreds of functions this
  library does not have, and lay out types (`struct stat`, `sigset_t`,
  `pthread_mutex_t`) as glibc does, where this library's ABI is musl's.
- **Declarations generated from the Rust definitions.** They could not
  drift from the definitions; but a Rust signature carries neither C's
  `const`, nor the feature macros, nor which header a function belongs in.

**`-I`, not `-isystem`:** zig's driver searches its own libc headers before
any `-isystem` directory, so `-isystem posix/include` would put the overlay
behind the headers it extends -- silently: `#include <stdio.h>` finds
musl's, and nothing fails until a program calls an overlay function. Each
overlay header marks itself a system header (`#pragma GCC system_header`),
as musl's are by where they live, so that a program's `-Werror` is not
about them; the gate turns that off to hold them to `-Wall -Wextra` itself.

**What holds it:** `scripts/check-libc-overlay.py` -- every header compiles,
alone and with the others, in eleven feature-macro settings, C99, gnu89 and
two C++ ones; the names the overlay adds to musl's are exactly the
reference's, which `posix/tools/oracle/glibc_declarations.py` reads out of
glibc 2.39's headers with libclang; and each appears, after its header, in
exactly the settings glibc's does, with a type `__builtin_types_compatible_p`
finds compatible with glibc's; and each type the overlay defines itself
(`femode_t`, `FTS`, `struct mallinfo` ...) has glibc's size and field
offsets. `scripts/check-libc-prototypes.py` checks each declaration against
its Rust definition by the x86-64 calling convention, and
`scripts/check-libc-abi.py`, which compiles against musl's headers with the
overlay in front, the library's Rust types against the overlay's.

**Where it parts from glibc, on purpose:** the LFS64 names (`open64`,
`stat64` ...) stay as musl has them, macros under `_LARGEFILE64_SOURCE`
only -- musl 1.2.4 stopped giving them to `_GNU_SOURCE`, deliberately, and
this does not undo that -- but `fcntl64`, which musl has no macro for, is
declared under both; `pidfd_send_signal` is absent from a strict ISO C
build, where musl has no `siginfo_t` for it to take; and of glibc's
non-portable mutex names only the aliases of musl's own types are there
(`PTHREAD_MUTEX_RECURSIVE_NP` ...), since this library has no adaptive
mutex and its initialisers are musl's.

**Widened too (2026-09-29, the same day):** where musl's own headers declare
a name `libc.a` defines under narrower feature macros than glibc's --
`fgetpwent` and `mempcpy` only for `_GNU_SOURCE` where glibc gives them by
default, `strdup` and `gmtime_r` not for C23, which made them ISO C -- the
overlay declares it again under glibc's (31 names; a redundant declaration
of the same type is legal C), and the gate holds those to glibc's headers as
it does the rest. `posix/tools/oracle/header_audit.py` finds them
(known-issues.md -> D-POSIX-MUSL-HEADERS-DECLARE-NARROWER-THAN-GLIBCS).
