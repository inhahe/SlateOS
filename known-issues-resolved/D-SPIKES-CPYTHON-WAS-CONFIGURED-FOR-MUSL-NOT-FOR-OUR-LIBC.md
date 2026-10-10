## D-SPIKES-CPYTHON-WAS-CONFIGURED-FOR-MUSL-NOT-FOR-OUR-LIBC — the image's Python was built for zig's musl: `subprocess` closed no inherited descriptors, `time.tzset` was missing, and the eval loop ran its slower dispatch (lane D, 2026-10-05)

**Status:** FIXED 2026-10-05

**In short:** before CPython is built, its `configure` script asks the C
library what it has and tests how some of it behaves, and CPython is built
around the answers. Ours asked zig's copy of musl (Linux's C library), and in a
cross build it cannot run its tests at all, so it guessed. Four functions our
library has were taken to be missing, among them the one `subprocess` uses to
stop a child inheriting the parent's open files; and the guesses took away
`time.tzset()` and the faster way of running bytecode. Python on SlateOS
worked, but less well than our library allowed, and one of the differences
could make a program hang: a child holding a pipe it should not have keeps the
other end from ever seeing the end of it.

**What was measured** (2026-10-05, `pyconfig.h` of
`scripts/cpython-spike/run.sh`'s build against Ubuntu 24.04's `python3.12`,
built on glibc 2.39, which our library follows):

- `configure` answered "no" for `close_range`, `sem_clockwait`, `getwd` and
  `tmpnam_r`, which our `libc.a` defines and zig's musl does not.
  - `close_range`: `_posixsubprocess` closes a child's inherited descriptors
    with it; without it, it lists `/proc/self/fd` through
    `syscall(SYS_getdents64)`, which our `syscall()` answers `ENOSYS` (the
    kernel's `/proc/<pid>/fd` lists nothing for a native process besides) --
    so `close_fds=True`, the default, closed nothing.
  - `sem_clockwait`: lock timeouts were measured on the wall clock, which
    can be set back.
- Five tests it would have run took their cross-compiling defaults:
  `time.tzset` absent (`ac_cv_working_tzset`; glibc itself fails the test,
  which wants `tzname[1]` empty for `TZ=UTC+0`, and Ubuntu's build has
  `time.tzset` all the same), `switch` dispatch instead of computed gotos,
  `multiprocessing.Semaphore.get_value()` raising (`sem_getvalue` taken to be
  broken), aligned access taken to be required, and `PTHREAD_SCOPE_SYSTEM`
  taken to be unsupported.

**Fixed (2026-10-05):** `run.sh` configures with `slate_make_link_wrappers`'
compiler, so configure's link checks are answered by our `libc.a`, and answers
the five run tests with SlateOS's measured answers. That needed the wrapper to
split a call that compiles and links in one step -- autoconf's shape for every
link test -- into a compile by zig's driver and a link by ld.lld; it had handed
such a call to zig's driver whole, which linked musl, and the first rebuild
through it still found none of the four. And it needed the wrapper's compiles
to see `posix/include`, whose headers declare what ours has beyond musl's:
the second rebuild found the four, and then could not compile `close_range`
or `sem_clockwait`, implicit declarations being errors since C99. Measured after the rebuild: configure finds
all four; `pyconfig.h` differs from Ubuntu's only by the libraries SlateOS
has not got (zlib, bz2, lzma, sqlite3, readline, curses, libffi, gdbm,
bluetooth, dtrace), by `HAVE_DEV_PTMX` (no such file in devfs -- our libc
opens the name itself -- and unread, CPython having `openpty`), and by two
facts of musl's headers (`HAVE_STROPTS_H`, `HAVE_DECL_RTLD_MEMBER`). The
control interpreter, on the image's zip: computed gotos on, `time.tzset()`
moving `tzname` to EST/EDT, `Semaphore.get_value()` answering 3, and
`close_fds=True` closing an inheritable pipe end in the child; `make` exits
0, and the image's interpreter links with nothing missing. The
interpreter `make` links is the image's; `python-control`, the same objects
linked against musl with `control-shim.c` standing in for the four functions
musl lacks, is what `stdlib.sh` runs under WSL. The other ports have the same
defect: `known-issues/D-SPIKES-PORTS-CONFIGURE-AGAINST-ZIGS-MUSL.md`.
