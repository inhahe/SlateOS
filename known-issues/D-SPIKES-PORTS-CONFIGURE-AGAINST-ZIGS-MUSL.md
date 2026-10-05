## D-SPIKES-PORTS-CONFIGURE-AGAINST-ZIGS-MUSL — the ported programs' configure scripts look for functions in zig's musl, not in our libc, so each port is built for a C library it does not run on (lane D, 2026-10-05)

**Status:** OPEN — CPython's fixed (known-issues-resolved/D-SPIKES-CPYTHON-WAS-CONFIGURED-FOR-MUSL-NOT-FOR-OUR-LIBC.md); make, coreutils and bash to do.

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
| bash 5.2 | `arc4random`, `argz_count`, `argz_next`, `argz_stringify` | none that shows: `$SRANDOM` reaches `arc4random` only when `getrandom` fails |
| GNU make 4.4.1 | `sigsetmask` | none that shows |
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
