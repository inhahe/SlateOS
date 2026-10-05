## D-POSIX-EXTENSIONS-HAVE-NO-DECLARATIONS — 263 functions `libc.a` defines are declared by no header a C program here can include, so C cannot call them, and a port's `configure` will say they exist (lane D, 2026-09-29) — **Status: FIXED 2026-09-29 -- `posix/include` declares the 195 of them glibc 2.39's headers declare, where and as glibc's do (held to glibc's by `scripts/check-libc-overlay.py`; design-decisions §1141), and the C fixtures are built with it; `scripts/check-libc-declared.py` now refuses a public name `libc.a` defines that no header declares, and of the rest fifteen stopped being exported (D-POSIX-LIBC-EXPORTED-NAMES-NO-HEADER-DECLARES) and each other one is excused with its reason. Still to come, when there is something to take it: the rootfs's `/usr/include`, for a native toolchain, and each port's build, when it is next rebuilt**

**In short:** C on SlateOS is compiled against musl's headers (`zig cc
--target=x86_64-linux-musl`), and musl's headers declare only what musl
has. The C library here has more: glibc's extensions and C23's additions,
written since -- `j0l`, `clog10`, `nextup`, the narrowing functions, `fts_*`,
`error`, `backtrace`, `close_range`, `renameat2`, `arc4random`, `getcpu` and
some two hundred more. A C program cannot call any of them without writing
its own prototype, because clang refuses a call to an undeclared function.
Worse, a port's `configure` script decides what exists by *linking* a test
program with a dummy declaration of its own -- which succeeds -- and then
the port's real code, calling the function through the headers, does not
compile.

**Measured** (2026-09-29): the functions `libc.a` defines, that glibc 2.39
exports as public interface, and that no header under zig's `generic-musl`
declares with `_GNU_SOURCE`, `_BSD_SOURCE` and `_LARGEFILE64_SOURCE`: 263.
Some are the pattern's false positives (variables such as `stdin`,
`environ`, `signgam`; macros musl makes of `isnan`); the rest, by where they
belong:

| Header | Undeclared |
|---|---|
| `<math.h>` | `j0l` ... `ynl`; C23's `nextup`, `nextdown`, `llogb`, `canonicalize`, `fromfp` ... `ufromfpx`, `getpayload`, `setpayload`, `setpayloadsig`, `totalorder`, `totalordermag`, `fmaximum` ... `fminimum_mag_num`, `roundeven`, and every `f`/`l` form; the narrowing `fadd` ... `dfmal`; `scalbl`, `gammal`, `significandl`, `finitel`, `dreml`; the `f128` functions |
| `<complex.h>` | `clog10`, `clog10f`, `clog10l` |
| `<fenv.h>` | `feenableexcept`, `fedisableexcept`, `fegetexcept`, `fesetexcept`, `fetestexceptflag` |
| no header in musl | `<fts.h>` (`fts_open` ...), `<error.h>` (`error`, `error_at_line` and their variables), `<execinfo.h>` (`backtrace` ...), `<gnu/libc-version.h>` |
| `<stdlib.h>`, `<string.h>`, `<stdio.h>`, `<wchar.h>` | `arc4random`, `arc4random_buf`, `arc4random_uniform`, `canonicalize_file_name`, `ecvt_r`, `fcvt_r`, `on_exit`, `rawmemchr`, `fcloseall`, `tmpnam_r`, `wmempcpy` |
| `<unistd.h>`, `<fcntl.h>`, `<stdio.h>`, `<sys/*.h>` | `close_range`, `closefrom`, `getcpu`, `renameat2`, `pidfd_open`, `pidfd_getfd`, `pidfd_send_signal`, `epoll_pwait2`, `sethostid`, `sysctl`, `arch_prctl`, `capget`, `capset`, `init_module`, `delete_module`, the LFS64 names (`open64`, `stat64` ... which musl 1.2.4 dropped) |
| `<pthread.h>`, `<semaphore.h>` | the `clock*` waits, the `*_np` robust-mutex names, `sem_clockwait` |
| `<malloc.h>`, `<search.h>`, `<time.h>`, others | `mallinfo`, `mallinfo2`, `malloc_trim`, `malloc_stats`, `pvalloc`, `twalk_r`, `timelocal`, `getdate_r`, the `*_r` database iterators |

**The proper fix:** a header overlay -- `posix/include/`, searched before
musl's with `-isystem`, each file `#include_next`ing musl's header of the
same name and adding the declarations for what this library defines,
under the feature macros glibc declares them under (`_GNU_SOURCE`, C23 by
`__STDC_VERSION__`, `__STDC_WANT_IEC_60559_*`); and whole headers for the
families musl has none of. Every C build here -- the `services/` fixtures,
and the rootfs's `/usr/include` for the native toolchain to come -- takes
it. A gate that fails when `libc.a` defines a public name no header
declares, and a C program that includes every overlay header with
`-Wall -Werror` and calls each declared function, so a declaration that
disagrees with the definition's types is caught at compile time.

**Where:** `posix/include/` (new), `services/*/build.py`,
`scripts/create-ext4-rootfs.sh`, a gate beside
`scripts/check-libc-declared.py`.
