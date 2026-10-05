## 1119. The numbers a C caller passes are musl's: behaviour glibc's, numbers the header's

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous) -- §1011's rule, applied to constants

**In short:** a C program on this system is compiled against musl's header
files, so every flag, code and item number it passes to the C library -- or
compares the library's answer with -- is musl's. Where this library had
copied glibc's number instead, or invented one, the two sides disagreed
silently: asking for the character set (`CODESET`) got back "Sun". So the
rule is: what the library *does* follows glibc, as it always has; the
*numbers* it reads and hands back are musl's.

**Why musl.** §1011 chose musl as the oracle for structure layouts because
it is the C library every port here compiles against; the same holds for
numbers, and for Rust programs too: the target says `env: "musl"`, so the
`libc` crate hands them musl's definitions. glibc and musl agree on almost
every number; where they differ, a port has musl's.

**How it was checked.** Every top-level `pub const` of the modules the library
still reaches, evaluated from the source, against a probe compiled by
`zig cc --target=x86_64-linux-musl` and run under WSL, printing each name the
headers define. 1,374 of 2,721 constants have a musl name; 94 differed, 83 of
them wrongly (known-issues.md, `D-POSIX-CONSTANTS-WERE-NOT-MUSLS`). The other
eleven are this table, and are not to be "fixed":

| Name | Here | musl | Why it stays |
|---|---|---|---|
| `FD_SETSIZE` | 256 | 1024 | a policy limit, the fd table's size; the `fd_set` layout is musl's 1024 bits (`FD_SET_BITS`, §1011) |
| `PAGE_SIZE`, `SHMLBA`, `NBPG`; `PAGE_MASK` | 16384; ~16383 | 4096; ~4095 | this kernel's pages are 16 KiB; a port that uses the macro instead of `sysconf(_SC_PAGESIZE)` is wrong here whatever the library says (`NBPG` and `PAGE_MASK` added 2026-09-30) |
| `ARG_MAX`, `HOST_NAME_MAX`, `NGROUPS_MAX`, `NGROUPS` | 2 MiB, 64, 65536, 65536 | 128 KiB, 255, 32, 32 | the kernel's real limits (Linux's); musl's header states its own (`NGROUPS`, glibc's `<sys/param.h>`'s name for `NGROUPS_MAX`, added 2026-09-30) |
| `MAXQUOTAS` | 3 | 2 | the kernel has project quotas; musl's header predates them |
| `O_ACCMODE` | 3 | `3 \| O_PATH` | musl folds `O_SEARCH` into the access mode; the library's own masking is glibc's |
| `SIGRTMIN`, `MB_CUR_MAX` | 32, 4 | calls | musl's macros call `__libc_current_sigrtmin` and `__ctype_get_mb_cur_max`, which are this library's and answer 32 and 4 |
| `__WCLONE`, `WEOF` | -2^31, -1 | 2^31, 2^32-1 | the same bits; a signed Rust constant beside an unsigned C one |

| Alternative | For | Against |
|---|---|---|
| **musl's numbers (chosen)** | the header a port includes is the only one that matters to it | where glibc differs, a glibc-built binary would disagree -- none runs against this library |
| glibc's numbers, as the behaviour is glibc's | one library to read | every port compiles against musl's header, so every difference is a silent bug; nftw and nl_langinfo were two |

**What keeps it true.** The constants half of `scripts/check-libc-abi.py`,
since 2026-09-27 (§1130): every public constant whose name a musl header
defines, compared with that header's value on each push that touches
`posix/src` -- since 2026-09-30 every header musl has and every header the
overlay adds, where it was a list of 105 of musl's 183 (known-issues.md,
`D-POSIX-THE-CONSTANTS-OF-78-HEADERS-WERE-NEVER-COMPARED`); and enum
constants besides macros, and, for a name musl's headers do not define,
glibc's and then the kernel's headers through `glibc_constants.txt`
(`D-POSIX-CONSTANTS-NO-MUSL-HEADER-NAMES-HAD-NO-ORACLE`), whose deliberate
differences are `KNOWN_DIFFERENT_GLIBC` -- `PAGE_SHIFT` alone, 16 KiB
pages.  The table above is its `KNOWN_DIFFERENT`, less `__WCLONE` and
`WEOF` -- compared in the bits both sides have, they agree, as the table says
-- and less `SIGRTMIN` and `MB_CUR_MAX`, which with `SIGRTMAX` are its
`NOT_CONSTANT_IN_MUSL`: musl's are calls, which no compile-time check can
evaluate.
