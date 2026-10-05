## D-SPIKES-PORTS-CONFIGURE-AGAINST-ZIGS-MUSL — the ported programs' configure scripts look for functions in zig's musl, not in our libc, so each port is built for a C library it does not run on (lane D, 2026-10-05)

**Status:** OPEN — CPython's fixed (known-issues-resolved/D-SPIKES-CPYTHON-WAS-CONFIGURED-FOR-MUSL-NOT-FOR-OUR-LIBC.md) make's and bash's (2026-10-05, below); coreutils to do.

**In short:** before a program is built, its `configure` script asks the C
library what it can do -- is there a `renameat2`, an `fts_open`, an
`arc4random` -- and the build uses or replaces each one according to the
answer. The programs ported to SlateOS ask zig's copy of musl (Linux's C
library) instead of SlateOS's own, because zig's compiler is what configure
is given. So each is built for musl's answers: where SlateOS's library has a
function musl lacks, the program carries its own stand-in or does without,
and where the two behave differently, the program was tested against the
wrong one. Nothing fails to build; each port quietly does less, or does it
differently, than our library allows.

**Measured** (2026-10-05; each port's `config.log` against `nm` of our
`libc.a`): functions configure answered "no" to that our `libc.a` defines --

| port | answered "no", ours has it | what that costs |
|---|---|---|
| coreutils 9.5 (gnulib) | `canonicalize_file_name`, `error`, `fts_open`, `group_member`, `random_r`, `rawmemchr`, `renameat2`, `rpmatch`, `sysctl`, `timespec_getres`, `wmempcpy` | gnulib compiles its own replacement for each; its `renameat2` cannot make `RENAME_NOREPLACE` atomic, so `mv -n` can still clobber in a race |
| bash 5.2 | `arc4random`, `argz_count`, `argz_next`, `argz_stringify` | none that shows: `$SRANDOM` reaches `arc4random` only when `getrandom` fails -- fixed, see the end |
| GNU make 4.4.1 | `sigsetmask` | none that shows -- fixed, see the end |
| pkgconf 2.3.0 | none | |
| CPython 3.12.3 | `close_range`, `getwd`, `sem_clockwait`, `tmpnam_r`, and five run tests | fixed: see the resolved entry |

And a test configure *runs* -- does `printf` know `%a`, is `getcwd(NULL, 0)`
an allocation -- measures musl's behaviour, not ours, in a port configured
natively (no `--host`), and takes a guess in a cross-configured one
(`D-SPIKES-BASH-CROSS-CONFIGURE-GUESSED-WHAT-IT-COULD-NOT-RUN`, bash's).

**The proper fix, per port:** configure with `slate_make_link_wrappers`' compiler
(`scripts/lib/worktree.sh`), so configure's link checks are answered by our
`libc.a`, as LLVM's and, since 2026-10-05, CPython's builds are; cross mode, so
nothing linked against our library is run on Linux; and each run test answered
with SlateOS's true answer, measured, as bash's and CPython's are. gnulib
guesses its run tests from the host triplet ("guessing yes on musl" for
`x86_64-linux-musl`); our library's reference is glibc 2.39, so each guess
gets checked against ours rather than taken. A port whose tests need a
runnable binary to check itself (`stdlib.sh`'s control interpreter) gets one
linked against musl from the same objects, with the functions musl lacks
stood in, as CPython's `control-shim.c` does.

**GNU make, fixed 2026-10-05.** `scripts/make-spike/run.sh` configures through
the wrapper, with `--build` and `--host`. Measured first, three ways, each
configure's `config.cache` diffed: given `--host` alone, as before, configure
had seen its test programs run on Linux and decided it was *not*
cross-compiling, so every test it runs ran against musl; through the wrapper
it finds `sigsetmask`, and four run tests are left to answer. Each is now
answered with what the same test gives on glibc 2.39, natively, which is what
ours does -- `ac_cv_func_gettimeofday`, `ac_cv_func_strcoll_works`,
`make_cv_synchronous_posix_spawn` and `am_cv_func_iconv_works`, all `yes` --
and each is pinned by a host test of its probe (`cargo test -p posix
make_configures`). The guesses would have said no to the first three, and so
taken make off `posix_spawn` and onto fork and exec. make links with nothing
missing or duplicated, `USE_POSIX_SPAWN` and `HAVE_SIGSETMASK` set.

Configuring through the wrapper first needed the wrapper fixed: it put
`posix/include` in front of the build's own `-I` directories, which hid make's
own `lib/glob.h` (gnulib's, as coreutils' will be) and stopped the build. It
comes after them now (`scripts/test-link-wrappers.sh` case 5).

**bash, fixed 2026-10-05.** `scripts/bash-spike/cross2.sh` was cross
already, with each run test answered from measured facts; only its compiler
was musl's. Through the wrapper, configure finds `arc4random` (and
`argz.h`), `config.h` otherwise unchanged; the build's own link is a SlateOS
bash, and `slatelink.sh`'s relink has nothing missing or duplicated. Two
consequences handled in the same change: bash 5.2's inverted `strtoimax` test
(it adds its own strtoimax when the C library has one) is undone between
configure and the first make, as `cross3.sh` -- now folded in -- did after a
failed one; and the copy of bash that runs on Linux (`build/spike/
bash-musl.elf`, for measurements under WSL) is now a separate link of the same
objects against musl, with `musl-shim.c` supplying the `arc4random` musl has
not got.
