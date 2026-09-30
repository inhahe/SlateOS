# glibc as the oracle

Much of `posix`'s behaviour is specified as "what glibc 2.39 does": the maths
functions' results and `errno`, what `iconv` makes of a malformed byte, how
`getaddrinfo` sorts, which spellings `strtod` accepts. The tests check that
by replaying glibc's own answers, and the programs here are where those
answers come from. Each one builds a C program with gcc under WSL -- whose C
library is the oracle -- runs it, and records what glibc said.

Everything here needs WSL with the `Ubuntu` distribution (24.04, glibc 2.39)
and gcc in it. The sandboxed ones (`accounts`, `gai`, `hosts`, `netdb`, `ifaddrs`) use
`unshare -r`, which needs no root. Build products go in a temporary directory
(`_wsl.workdir`); a run changes nothing in the tree but its own output.

## Which program feeds which test

| Program | Writes | Read by |
|---|---|---|
| `math_harness.py` (cases: `math_cases.py`) | `posix/src/math_oracle.txt` | `math.rs`, `include_str!` |
| `math_modes_harness.py` (the same cases, in the three directed rounding modes) | `posix/src/math_modes_oracle.txt` (the answers that differ from nearest) | `math.rs`, `include_str!` |
| `mathl_harness.py` | `posix/src/mathl_oracle.txt` | `mathl.rs`, `include_str!` |
| `mathl_modes_harness.py` (the same calls, in the three directed rounding modes) | `posix/src/mathl_modes_oracle.txt` (the answers that differ from nearest) | `mathl.rs`, `include_str!` |
| `c23math_harness.py` | `posix/src/c23math_oracle.txt` (C23's exact functions -- `nextup` ... `fminimum_mag_num` -- in all three precisions, and `scalbl`, `ilogbl`, `logbl`: value, flags raised and `errno`) | `c23math.rs`, `include_str!` |
| `narrow_harness.py` | `posix/src/narrow_oracle.txt` (C23's narrowing functions, `fadd` ... `dfmal`, each call in all four rounding directions: value, flags raised and `errno`) | `narrow.rs`, `include_str!` |
| `complex_modes_harness.py` (the same calls, in the three directed rounding modes) | `posix/src/complex_modes_oracle.txt` (the answers that change in kind or in `errno`) | `complex.rs`, `include_str!` |
| `complexl_modes_harness.py` (the same calls, in the three directed rounding modes) | `posix/src/complexl_modes_oracle.txt` (the answers that change in kind or in `errno`) | `complexl.rs`, `include_str!` |
| `glibc_declarations.py` (libclang's Python bindings, not gcc: its docstring says how to get them) | `posix/tools/oracle/glibc_declarations.txt` (for each name `posix/include` declares, the header, the feature-macro settings and the type glibc 2.39's headers give it) and `glibc_layouts.txt` (the size and field offsets glibc gives each type the overlay defines) | `scripts/check-libc-overlay.py` |
| `glibc_constants.py` (the names of `posix`'s public constants, from the rustdoc `check-libc-abi.py` reads) | `posix/tools/oracle/glibc_constants.txt` (for each name, the values glibc 2.39's headers give it, else Linux 6.8's uapi headers do -- each header compiled alone -- or `-`: 39 seconds) | `scripts/check-libc-abi.py`, for the names no musl or overlay header defines |
| `header_audit.py` (as `glibc_declarations.py`; a report, not a table) | nothing: prints where musl's headers, with `posix/include` in front, declare `libc.a`'s names under other feature macros than glibc 2.39's (D-POSIX-MUSL-HEADERS-DECLARE-NARROWER-THAN-GLIBCS) | whoever adds to the overlay |
| `argz_harness.py` | `posix/src/argz_oracle.txt` (every argz and envz call over the edge cases: empty fields, entries first, last and alone, a pointer into an entry's middle or past the vector, empty and repeated replacements, bare names) | `argz.rs`, `include_str!` |
| `dlfcn_harness.py` | `posix/src/dlfcn_oracle.txt` (glibc's `<dlfcn.h>` in a statically linked program: `dlopen`'s handles and mode checks, `dlsym`'s and `dlvsym`'s messages, `RTLD_NEXT`, `dlclose`, `dlinfo`'s requests, `dlmopen`'s namespaces; and, as comments, what `dl_iterate_phdr`, `_dl_find_object` and `dladdr` answered, which the tests hold a synthetic image to) | `dlfcn.rs`, `include_str!` |
| `errfns_harness.py` | `posix/src/errfns_oracle.txt` (`error`, `error_at_line`, `warn`, `warnx`, `err` and `errx`: what each wrote, `stdout` and `stderr` together on one pipe, and how the process ended; `error_one_per_line`, `error_print_progname`, a 5000-byte message; and, as comments, the aliased variables' probes) | `error.rs` and `err.rs`, `include_str!` |
| `fmtmsg_harness.py` | `posix/src/fmtmsg_oracle.txt` (`fmtmsg` and `addseverity`, each case in a process of its own: labels, every combination of the parts on each channel, `MSGVERB` and `SEV_LEVEL` values, `addseverity` sequences, standard error closed -- 804 cases), and `posix/src/fmtmsg_deviations.txt` (the 3 this library answers otherwise, as `fmtmsg_model.py` does: design-decisions section 1150) | `fmtmsg.rs`, `include_str!` |
| `fnmatch_harness.py` | `posix/src/fnmatch_oracle.txt` (`fnmatch` in the C locale: every pattern of one or two of 64 tokens, and 53 longer ones, under ten flag sets, and 354 of ksh's extended patterns under seven, each against 50 strings -- 2,141,400 answers), and `posix/src/fnmatch_deviations.txt` (the 1,443 of them this library answers otherwise, as `fnmatch_model.py` does: design-decisions section 1148) | `fnmatch.rs`, `include_str!` |
| `glob_harness.py` | `posix/src/glob_oracle.txt` (`glob` over a directory tree of the harness's own, through `GLOB_ALTDIRFUNC`: 98 patterns under 16 flag sets -- return, `GLOB_MAGCHAR`, the names in order and each `errfunc` call -- and 50 `glob_pattern_p` cases), and `posix/src/glob_deviations.txt` (the 23 probes this library answers otherwise, as `glob_model.py` does: design-decisions section 1149) | `glob.rs`, `include_str!` |
| `inet6_harness.py` | `posix/src/inet6_oracle.txt` (RFC 3542's option and Type 0 Routing header builders, with every padding size and refusal; RFC 2292's, over glibc's own buffers and a hand-built one with Pad1) | `inet6.rs`, `include_str!` |
| `multibyte_harness.py` | `posix/src/multibyte_oracle.txt` (in glibc's C.UTF-8 locale: `mbrtoc8`, `mbrtoc16` and `mbrtoc32` over 25 inputs, a byte a call and all at once; `c8rtomb`, `c16rtomb` and `c32rtomb` over their units; the NULL-`s` cases, each in a child process, since glibc's crashes on one; `mbsnrtowcs` and `wcsnrtombs` cut short by their limits, counting, and at an error) | `uchar.rs` and `wchar.rs`, `include_str!` |
| `oldcalls_harness.py` | `posix/src/oldcalls_oracle.txt` (the BSD signal masks, `sigstack`, `sigreturn`, `gsignal`, `ssignal`, `getwd`, `group_member`, `revoke`, `setlogin`, `ttyslot`, `profil`, `getpw`, `gtty`, `stty`, `isctype` and glibc's class bits, `isfdtype`, `dysize`, and `execveat`'s refusals; each probe in a child process) | `legacy.rs`, `include_str!` |
| `random_harness.py` | `posix/src/random_oracle.txt` (`rand`, `random` and `rand_r` for eight seeds; `random` after `initstate` at eleven sizes; `setstate`'s switches; `random_r`; the `rand48` family, its `_r` forms included; the two structs' layouts) | `prng.rs`, `include_str!` |
| `qcvt_harness.py` | `posix/src/qcvt_oracle.txt` (`qecvt`, `qfcvt` and `qgcvt` over 70 `long double` values at nine `ndigit`s each -- 2,160 calls -- each beside the value's exact answer, from glibc's own `%.*Le` and `%.*Lf`: glibc's `q` forms scale in `long double` arithmetic and 156 of their answers are not the value's digits, design-decisions section 1135) | `stdlib.rs`, `include_str!` |
| `strfrom_harness.py` | `posix/src/strfrom_oracle.txt` (`strfromd`, `strfromf` and `strfroml` for every conversion C23 allows at eleven precisions over 26 doubles, 19 floats and 19 long doubles, and cut short at five buffer sizes; `timespec_getres` for bases 0 to 5) | `printf.rs` and `time.rs`, `include_str!` |
| `strname_harness.py` | `posix/src/strname_oracle.txt` (`strerror`, `strerrorname_np`, `strerrordesc_np`, `strsignal`, `sigabbrev_np` and `sigdescr_np` for every error number from -2 to 139 and signal from -2 to 69) | `string.rs` and `signal.rs`, `include_str!` |
| `stdbit_harness.py` | `posix/src/stdbit_oracle.txt` (C23's `<stdbit.h>`, the fourteen functions for the five unsigned types: every `unsigned char` and `unsigned short` value as a digest, the edges and a sample of the wider ones as calls) | `stdbit.rs`, `include_str!` |
| `tempfile_harness.py` | `posix/src/tempfile_oracle.txt` (`mkstemp`, `mkostemp`, `mkstemps`, `mkostemps` and their `64` names, `mkdtemp`, `mktemp` over 34 template shapes, each run eight times so that the bytes a name replaces show; the flags the `o` forms take and what the descriptor has; the modes under three umasks; `tmpfile`; `tempnam` over nine directories, six prefixes and four `$TMPDIR`s; `tmpnam`, `tmpnam_r` -- 554 probes, each in a child in a directory of its own under WSL's `/tmp`) | `tempname.rs`, `include_str!` |
| `gshadow_harness.py` | `posix/src/gshadow_oracle.txt` (`sgetsgent` over 27 lines and its `_r` form at nine buffer sizes each, `fgetsgent` and `fgetsgent_r` over a file of entries, blank lines and comments, `putsgent` over 15 entries -- 297 probes) | `gshadow.rs`, `include_str!` |
| `fsttys_harness.py` | `posix/src/fsttys_oracle.txt` (`<fstab.h>` and `<ttyent.h>` over test files -- quoted fields, comments, a line too long for glibc's buffer, the order `getfsspec` and `getttynam` leave the tables in -- and with no files; run statically linked in a user namespace with a tmpfs over `/etc`; 142 probes, and the two files) | `fstab.rs`, `ttyent.rs`, `include_str!` |
| `rpc_harness.py` | `posix/src/rpc_oracle.txt` (`<rpc/netdb.h>` over a test file -- comments, blank lines, the lines glibc's parser refuses, a last line with no newline -- over the built-in copy it reads out of `netdb.rs`, and with no file; run statically linked in a user namespace with a tmpfs over `/etc`; 339 probes, and the two files) | `netdb.rs`, `include_str!` |
| `besl_harness.py` | `posix/src/besl_glibc.txt` (the `long double` Bessel functions at their special and extreme arguments, in all four rounding directions: value, flags raised and `errno`) | `besl.rs`, `include_str!` |
| `besl_tables.py table` (mpmath, not glibc; `check` compares) | the constants, the reciprocals and the zeros' Taylor tables, pasted | `besl.rs` |
| `besl_tables.py oracle` (mpmath, not glibc) | `posix/src/besl_oracle.txt` (the six functions correctly rounded, with the exact value's side of each) | `besl.rs`, `include_str!` |
| `besl_tables.py oracle-large` (mpmath: the recurrences at 80 digits to order 2^16, Debye's expansions at 80 digits beyond) | `posix/src/besl_large_oracle.txt` (`jnl` and `ynl` at orders 600 to 2^31 - 1, across the turning point) | `besl.rs`, `include_str!` |
| `lgammal_zeros.py table 30` (mpmath, not glibc) | the `LGAMMAL_ZEROS` table, pasted | `mathl.rs` (`lgammal_near_zero`) |
| `lgammal_zeros.py oracle` (mpmath, not glibc) | `posix/src/lgammal_zero_oracle.txt` | `mathl.rs`, `include_str!` |
| `ldclass.c` | its output, pasted as `CLASS_ORACLE` | `mathl.rs` |
| `complex_harness.py` | `posix/src/complex_oracle.txt` | `complex.rs`, `include_str!` |
| `complexl_harness.py` | `posix/src/complexl_oracle.txt` | `complexl.rs`, `include_str!` |
| `accounts_harness.py` | `posix/src/accounts_oracle.txt` | `accounts_oracle.rs`, `include_str!` (`fgetpwent` & co., `put*ent`, `sgetspent`, `getusershell`, `getpass`) |
| `conv_harness.py` | `posix/src/conv_oracle.txt` | `printf.rs` and `stdlib.rs`, through `decfloat::CONV_ORACLE` (`printf` and `strto*` of `double`, `float` and `long double`, and `wcstold`, every rounding mode) |
| `cvt_harness.py` | `posix/src/cvt_oracle.txt` | `stdlib.rs`, `include_str!` (`ecvt`, `fcvt`, `gcvt`) |
| `ns_harness.py` | `posix/src/ns_oracle.txt` | `resolv.rs`, `include_str!` (`ns_initparse` & co.) |
| `getdate_harness.py` | `posix/src/getdate_oracle.txt` | `time.rs`, `include_str!` (`getdate`, `getdate_r`) |
| `strptime_harness.py` | `posix/src/strptime_oracle.txt` | `time.rs`, `include_str!` (`strptime`) |
| `timeconv_harness.py` | `posix/src/timeconv_oracle.txt` | `time.rs`, `include_str!` (`gmtime_r`, `localtime_r`, `mktime`, `timegm`, `strftime("%s")`, `asctime`, `ctime`) |
| `strtod_nan_harness.py` | a table, pasted as `GLIBC_NAN` | `stdlib.rs` |
| `cp125x_harness.py` | a table, pasted as `GLIBC_CP125X` | `iconv.rs` |
| `iconv_hand_harness.py` | `posix/src/iconv_hand_oracle.txt` (every byte and every encoder of the 80 single-byte sets not generated from a charmap: the 25 hand-written ones and the 55 with table headers of their own) | `iconv.rs`, `include_str!` |
| `tcvn_harness.py` (cases: `tcvn_cases.py`) | a table, pasted as `GLIBC_TCVN` | `iconv.rs` |
| `prefix_harness.py` (cases: `prefix_cases.py`) | a table, pasted as `GLIBC_PREFIX` | `iconv.rs` |
| `tscii_harness.py` (cases: `tscii_cases.py`) | a table, pasted as `GLIBC_TSCII` | `iconv.rs` |
| `wscanf_harness.py` | a table, pasted as `GLIBC_WSCANF` | `scanf.rs` |
| `wscanf_stream_oracle.c` | eight lines, quoted in its header | `scanf.rs`, asserted by hand |
| `addr_harness.py` (`addr_cases.py`, `addr_oracle.c`) | a table, pasted as `GLIBC` | `inet.rs` |
| `gai_harness.py` (`gai_oracle.c`) | a table, pasted | `gai.rs` |
| `hosts_harness.py` (`hosts_oracle.c`) | a table, pasted | `hosts.rs` |
| `netdb_harness.py` (`netdb_oracle.c`) | a table, pasted | `netdb.rs` |
| `ifaddrs_run.sh` (`ifaddrs_oracle.c`) | its output, pasted by hand as `GLIBC_UP` ... | `socket.rs` |

The four `*_modes_harness.py` share `_modes.py`, which runs a harness's own
program once per rounding mode and checks its to-nearest pass against the
harness's table before writing anything.

## Running one

    python posix/tools/oracle/math_harness.py            # rewrites posix/src/math_oracle.txt
    python posix/tools/oracle/gai_harness.py             # prints the table
    python posix/tools/oracle/gai_harness.py --check     # is gai.rs's copy still glibc's?
    wsl -d Ubuntu -- bash posix/tools/oracle/ifaddrs_run.sh

A table a test carries pasted is updated by pasting the harness's output over
the block that starts with the same `// Generated by` line and running
`cargo fmt`, which reflows it; `--check` compares ignoring that layout. The
`.txt` oracles are regenerated in place; every harness draws its cases from a
fixed list or a seeded generator, so a rerun against the same glibc writes the
same bytes, and `git diff` shows exactly what a new glibc changed.

## History

Until 2026-09-28 these lived in lane D's scratch directory, outside the
repository, and the tests cited them there (`dlm/oracle/...`). Each was
moved here only after it had regenerated, from glibc, exactly the data the
tests carried.
