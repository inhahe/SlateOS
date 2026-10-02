## 1108. The eleventh NULL-pointer pass: upstream's stubs stay stubs, and `getdents` writes the syscall's record

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** a seeded sample of twenty of the audit's remaining sites came back
with twelve wrong, so the rest of the tail gets a full sweep rather than being
retired by sampling (`known-issues.md` →
`D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT`, eleventh pass). Most
fixes transcribe upstream's order. Two are choices, recorded here: the STREAMS
calls (a System V messaging interface Linux never had) are glibc's do-nothing
stubs again, and the legacy `getdents` — which answered "not implemented" to
every valid call — writes Linux's legacy directory record.

**1. The STREAMS stubs validate nothing.** glibc 2.39's `putmsg`, `putpmsg`,
`getmsg`, `getpmsg`, `fattach` and `fdetach` set `ENOSYS` whatever their
arguments (posix/streams-compat.c), and `isastream` asks only whether the
descriptor is open. An earlier phase (roadmap: "STREAMS API input validation")
put validators in front — `EBADF`, `EFAULT`, `ENOENT`, `EINVAL` — so "probing
callers' fallback paths fire". A probe calls with placeholder arguments and
tests for `ENOSYS`; the validators told it `EBADF`.

| Option | For | Against |
|---|---|---|
| (a) **glibc's stubs (chosen)** | every caller gets Linux's answer; a probe sees `ENOSYS` | a malformed call is not told which argument was bad — but there is no call to get right |
| (b) keep the validators | "meaningful feedback" | about a call that does not exist; it defeats the one answer callers test for |

**2. `getdents` writes `struct linux_dirent`.** It answered `ENOSYS` to every
valid call because "the legacy record's inode field is 32 bits" — true on
32-bit architectures; on x86-64 `d_ino` and `d_off` are `unsigned long`. glibc
exports no `getdents`; musl and bionic export one that is `getdents64` into
their `struct dirent`.

| Option | For | Against |
|---|---|---|
| (a) **the syscall's record (chosen)** | what the function was always documented to be; shares `getdents64`'s snapshot, order and tests | a program written against musl's declaration would read `d_type` from the wrong byte — but no header here declares `getdents`, so such a program declares it itself |
| (b) musl's and bionic's: `getdents64` under this name | musl-style callers | changes the documented contract; glibc, which this libc follows, has no such function |
| (c) keep `ENOSYS`, fix only the order | smallest change | keeps a refusal whose stated reason is false |

(b) is a one-argument change (`DirentRecord::Linux64`) if a real caller ever
needs it.
