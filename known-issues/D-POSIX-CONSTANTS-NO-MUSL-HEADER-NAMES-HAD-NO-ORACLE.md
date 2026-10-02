## D-POSIX-CONSTANTS-NO-MUSL-HEADER-NAMES-HAD-NO-ORACLE — a constant whose name no musl header defines, and every enum constant, was compared with nothing; four `UFFD_FEATURE_*` bits and `IORING_OP_LAST` were wrong (lane D, 2026-09-30) — **Status: FIXED 2026-09-30**

**In short:** the library copies the numbers C headers define so that what
a program passes means the same on both sides. The gate that compares them
(`scripts/check-libc-abi.py`) asked musl's headers only, and only for
macros. So a number musl has no name for -- one of glibc's own, or one of
the Linux kernel's interface -- and every number a header gives as an enum
constant rather than a macro was never compared with anything. Of the
library's 2,845 public constants, 1,029 were in that state. Comparing them
found six that disagree, four of them bugs.

| Constant | Was | Is |
|---|---|---|
| `UFFD_FEATURE_EVENT_UNMAP`, `_MISSING_HUGETLBFS`, `_MISSING_SHMEM` | bits 4, 5, 6 | 6, 4, 5, as `<linux/userfaultfd.h>` |
| `UFFD_FEATURE_WP_ASYNC` | bit 14 (`UFFD_FEATURE_POISON`'s) | bit 15; `_WP_UNPOPULATED`, `_POISON`, `_MOVE` added |
| `IORING_OP_LAST` | 64, "generous" | 55, Linux 6.8's |
| `IORING_OP_CANCEL` | 48, a name Linux has not got, with `IORING_OP_SENDMSG_ZC`'s number | gone; the eighteen missing opcodes added |
| `PAGE_SHIFT` | 14 | 14 -- this kernel's 16 KiB pages; `KNOWN_DIFFERENT_GLIBC` |

None of them was read by the library's own code (userfaultfd and io_uring
are validators that refuse, having no kernel behind them), so no call
misbehaved; each was wrong for any program that would use it.

**Fix:** two more comparisons in the gate. A name no musl macro accounts for
is tried in musl's headers as an integer constant expression -- one probe
line each, which compiles only for an enum constant -- and compared as the
macros are: 30 more. And a name no musl or overlay header defines is
compared with `posix/tools/oracle/glibc_constants.txt`, the values a glibc
system's headers give it -- glibc 2.39's, then Linux 6.8's uapi headers,
each header alone -- written by `posix/tools/oracle/glibc_constants.py`
under WSL, so the gate needs no WSL: 521 more. A constant the table has not
looked up is refused until it is regenerated (39 seconds).

**Where:** `scripts/check-libc-abi.py` (`probe_source`, `probed_constants`,
`read_glibc_constants`, `glibc_verdict`, `KNOWN_DIFFERENT_GLIBC`);
`posix/tools/oracle/glibc_constants.py` and `.txt`;
`posix/src/linux_userfaultfd.rs`, `posix/src/linux_io_uring.rs`.
