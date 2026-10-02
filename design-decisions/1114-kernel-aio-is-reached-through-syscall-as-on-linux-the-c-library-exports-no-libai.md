## 1114. Kernel AIO is reached through `syscall()`, as on Linux; the C library exports no libaio names

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** Linux's kernel asynchronous I/O calls (`io_setup`,
`io_submit`, `io_getevents` and three more) are system calls that glibc does
not wrap: C programs reach them through `syscall(SYS_io_setup, ...)`, or
through the small libaio library, which does exactly that. Our C library
used to export functions under libaio's names instead, answering as
`syscall()` does, while `syscall()` itself said "not implemented" for those
numbers. Now `syscall()` routes them, and the names are gone -- as in glibc.

| Choice | For | Against |
|---|---|---|
| **`syscall()` routes the six numbers; no exported names (chosen)** | glibc's shape exactly: libaio, built from source, works unchanged, and so does code that makes the calls by number | a program that declares `io_setup` itself without linking libaio no longer links -- as on Linux |
| Keep the exports, `syscall()` convention | nothing to change | libaio's declarations promise `-errno` returns, so a libaio program linked against these got -1 for every error, and the names collide with a real libaio at static link time |
| Export libaio's API (negative-errno returns) | ports need no libaio | libaio's header is not in the sysroot either, so a port brings it anyway, and then two copies disagree |

**Also decided with it:** a context id is the address of a ring laid out as
Linux 6.6's `struct aio_ring`, not a small number. libaio's `io_getevents`
reads the ring header at that address to decide whether it may skip the
system call, and a program may reap events itself by advancing `head`; with
a number for an id, the first dereferences address 1.

**How to reverse.** Put the `no_mangle` exports back beside the table
entries; nothing else depends on their absence.
