## D-SPIKES-PORTS-CONFIGURE-AGAINST-ZIGS-MUSL — the ported programs' configure scripts look for functions in zig's musl, not in our libc, so each port is built for a C library it does not run on (lane D, 2026-10-05)

**Status:** OPEN — CPython's fixed (known-issues-resolved/D-SPIKES-CPYTHON-WAS-CONFIGURED-FOR-MUSL-NOT-FOR-OUR-LIBC.md) make's and bash's (2026-10-05, below); coreutils measured, and waits until the port is staged (below).

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

**coreutils, measured 2026-10-05; not yet done.** Configured two ways, each
with `-C` and the caches diffed: natively on Ubuntu 24.04 (glibc 2.39, every
run test measured), and cross through the wrapper (our `libc.a`, `--build` and
`--host`). Through the wrapper the link tests are ours -- `fpurge`,
`__freadahead`, `__fseterr`, `sysctl`, `stime`, `copy_file_range` found -- and
**150 run tests are left to guesses**. Most guesses already agree with glibc
("guessing yes"), but about 28 decide what gnulib compiles in its place:

| run test | glibc 2.39 | cross guess |
|---|---|---|
| `gl_cv_func_getopt_gnu` | yes | no |
| `gl_cv_func_printf_directive_b`, `_lc`; `printf_sizes_c23`; `printf_enomem`; `printf_infinite_long_double` | yes | no |
| `gl_cv_func_printf_directive_n` | **no** | yes |
| `gl_cv_func_getcwd_succeeds_beyond_4k`, `getcwd_path_max` | yes | no / "partly" |
| `gl_cv_func_fchmodat_works`, `fchownat_empty_filename_works`, `fchownat_nofollow_works`, `futimens_works` | yes | no |
| `gl_cv_func_strtoll_works`, `strtoull_works`, `memchr_works`, `posix_memalign_works` | yes | no |
| `gl_cv_func_working_mktime`, `working_error`, `re_compile_pattern_working`, `unsetenv_works`, `unlink_busy_text`, `realpath_works` | yes | no / "nearly" |
| `gl_cv_func_btowc_consistent` | **no** | yes |
| `gl_cv_func_time_works` | "guessing no" even natively | yes |
| `gl_cv_func_fcntl_f_dupfd_cloexec`, `fflush_stdin`, `header_working_fcntl_h`, `pipes_are_fifos` | measured | "cross" |
| `gl_cv_cc_long_double_expbit0`, `gl_cv_double_slash_root` | `word 2 bit 0`, no | unknown |

**Why it is not done by taking glibc's column wholesale:** glibc's answer is
SlateOS's only where our library *and our kernel* behave as glibc on Linux
does, and some of these probe the kernel (`getcwd` past 4 KiB, `fchmodat` with
`AT_SYMLINK_NOFOLLOW`, `unlink` of a running program). One is not even
glibc's: `printf_directive_n` is "no" because Ubuntu's compiler defaults to
`_FORTIFY_SOURCE`, under which `%n` in a writable format aborts -- ours has no
such check. So each of the ~28 has to be read against posix/src (and, where
it probes the kernel, the kernel's answer) and pinned by a host test of its
probe, as make's four are.

**Why it waits:** GNU coreutils is a link-coverage spike, not on the image --
`/bin`'s utilities are this tree's own -- so no user meets a replacement gnulib
compiled in. What the guesses cost today is the measurement: each replacement
is one fewer of our functions the link exercises. The day the port is staged,
this is the work that has to come first. The two caches and the diff are
reproducible with the configure lines above (about five minutes each).
