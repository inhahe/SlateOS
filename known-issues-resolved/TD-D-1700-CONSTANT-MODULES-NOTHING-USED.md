### [D] TD-D-1700-CONSTANT-MODULES-NOTHING-USED — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/linux_*_types.rs` -- 1,715 modules until today, 12 now.

**In short:** the C library's source carried 1,715 files of Linux and glibc
constants -- 240,374 lines, written in batches long ago, each with tests of
its constants against themselves. Code the library exports reaches 11 of
them, and none had been checked against the headers they copy: one was
invented outright (iconv "encoding IDs as used by glibc internals", which
glibc does not have -- `B-D-ICONV-HAD-THREE-CHARSETS`), and checking the ones
in use found wrong values. 1,703 are deleted (238,411 lines). Twelve are
kept, checked constant by constant: the 11, and `linux_virtio_types`, whose
only users are the `linux_virtio_*` constant modules -- which nothing reaches
either, and which wait with the library's other unreached modules
(`todo.txt`, lane D; design-decisions.md §1118).

**The check,** of the 15 modules other files named -- three of which turned
out to be named only in comments, and went with the rest. gcc on Ubuntu 24.04
printed every name they define
from the real headers (glibc 2.39, the kernel's UAPI), macros and enum
constants both; the names no header defines were read by hand against Linux
6.6 and glibc 2.39:

- **wrong:** `VIRTIO_ID_BT` and `VIRTIO_ID_GPIO` were 28 and 29 -- the
  header's 0x28 and 0x29 read as decimal. Now 40 and 41, and every virtio
  device id is pinned to Linux's by a test; the old test asked only that they
  differ.
- **wrong, and unused:** `PIDFD_THREAD`, 0x10000000 -- Linux's is `O_EXCL`,
  0o200, which `process.rs` had right on its own.
- **invented, and unused -- removed:** two pidfd ioctl numbers Linux does not
  have, `PTHREAD_GUARD_DEFAULT` (4096; a page here is 16 KiB),
  `PTHREAD_STACK_DEFAULT`, `TIMER_RELTIME`, default terminal dimensions,
  getdents buffer sizes, and a second name for `MEMBARRIER_CMD_FLAG_CPU`.
- **right:** the rest -- derived layouts (`dirent64`'s and `utsname`'s
  offsets), syscall numbers, `perf_event_attr`'s bit positions, Linux 6.9's
  pidfd flags among them.

**Kept within reach.** `linux_clock_user_types.rs`, one of the deleted, held
the only field-by-field check of `struct tm`'s layout against glibc's; that
check is in `time.rs`'s own tests now. Any deleted file is one `git show`
away.

**Addendum (2026-09-27): 313 more, not named `*_types`.** The same kind of
module under other names -- `linux_acl.rs` to `linux_zswap.rs`, and `ar`,
`cpio`, `tar`, `sysexits`, `sys_ttydefaults` and the `net_*` header
transcriptions: constants only, each tested against itself, reached by
nothing the library exports or any other crate imports. Deleted as a closed
set -- no module that stays names one -- 47,977 lines, by §1118's rule.
Three constant modules that staying modules do name (`linux_fs.rs`,
`linux_netfilter.rs`, `sys_random.rs`) stay for the review of the rest
(`todo.txt`, lane D). Three doc comments in `syscall.rs` and `resource.rs`
credited `crate::linux_rlimit` -- a header transcription -- with the Linux-ABI
`prlimit64`; that is the kernel's (`kernel/src/syscall/linux.rs`), and they
say so now.

**Closed (2026-09-27).** The review of what else nothing reached is done: of
the 149 modules a scan of that day found unreached, 103 were header
transcriptions, facades and duplicates whose tests reached nothing live, and
went in one commit; the 46 whose tests called live functions went once those
tests had moved to the modules they test -- `getrandom`'s flag checks and
`personality`'s to `unistd.rs`, `reboot`'s to `process.rs` (where one of them
turned out to certify the wrong order), `getifaddrs`'s to `socket.rs` (where
the function turned out to hand every caller the same static list). No module
remains that nothing reaches but `abi_layout.rs`, which a script runs.
