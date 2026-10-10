## D-POSIX-GETENV-AND-GETCWD-COULD-NOT-BE-REPLACED — a program that brings its own `getenv`, `setenv`, `getcwd` or `mktime` could not link against our libc.a (lane D, 2026-10-01)

**Status:** FIXED 2026-10-05

**In short:** a program may supply its own copy of some C library
functions, and the linker then uses the program's. bash does it for
`getenv` and three others, and GNU packages do it, through gnulib, for many
more. Ours could not be declined: each sat in a piece of the library that
every program loads, so the program's copy and ours both ended up in the
binary, and the link failed. Now each is in a piece of its own.

**What was measured:** bash (`scripts/bash-spike/`), linked in its own
order -- its `lib/sh/libsh.a` ahead of our `libc.a`, as on Linux -- failed
with 5 duplicate symbols: `getenv`, `putenv`, `setenv` and `unsetenv`
(bash's `lib/sh/getenv.c`) and `getcwd` (`lib/sh/getcwd.c`, compiled because
a cross configure cannot run the test that would have found ours sound).
Ours were in two archive members (the units a static link takes whole) that
every program extracts: `posix::environ`'s, with `environ` and the start-up
code's `adopt_initial_envp`, and `posix::unistd`'s, with `abort`. `mktime`
had the same shape in `posix::time`'s member, which `localtime` brings in;
bash carries a replacement `mktime` too, which nothing in bash calls yet.

**Why it had not shown:** zig's cc driver, which every port linked through
until 2026-10-01, moves `-l` libraries behind the archives named by path, so
bash's `-lsh` came after our `libc.a` and ours were taken first; bash's
copies were never extracted (known-issues
D-SPIKES-LINK-ZIGS-MUSL-BEHIND-OUR-LIBC). And `scripts/check-libc-shape.py`
listed `getenv` and `setenv` among the names every program needs -- the
opposite of replaceable -- so nothing held them to a member of their own.

**The fix:** `getenv`, `setenv`, `unsetenv`, `putenv`, `clearenv`,
`secure_getenv` (`posix/src/environ.rs`), `getcwd` (`unistd.rs`) and
`mktime` (`time.rs`) are each a `mod gnu_*` block -- an archive member of
its own, as `string.rs` does for gnulib's string replacements -- and a thin
one: the work is in crate-internal functions (`environ::lookup`, `set`,
`remove`, `unistd::copy_cwd`, `time::mktime_ptr`), which the rest of this
library calls instead of the exported names, as glibc's own calls use
internal names (design-decisions §1169). The shape check holds each to its
own member (REPLACEABLE and STRICT_FAMILIES, so its CHECK 5 also refuses any
other member that reaches into one). bash links with nothing duplicated;
its `getenv` family is its own, as on Linux. Its cross configure is now
given the true answers for `getcwd` and `mktime`
(`scripts/bash-spike/cross2.sh`), so it compiles neither replacement and
uses ours -- `getcwd` from the kernel's record of the directory, where
bash's walks `..` matching inode numbers, and names the wrong directory
under procfs, which reports 0 for every one.
