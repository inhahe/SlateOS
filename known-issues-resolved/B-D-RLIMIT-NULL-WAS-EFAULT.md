### [D] B-D-RLIMIT-NULL-WAS-EFAULT — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/resource.rs`: `getrlimit`, `setrlimit`, `prlimit`.

**What it was.** `getrlimit(resource, NULL)` and `setrlimit(resource, NULL)`
answered `EFAULT` -- the answers of Linux's old `getrlimit`/`setrlimit`
system calls, which glibc on x86-64 never makes: its `getrlimit` is
`prlimit64(0, resource, NULL, rlim)` and its `setrlimit`
`prlimit64(0, resource, rlim, NULL)`, where a NULL pointer asks for nothing.
So both are 0 for a valid resource, and a bad resource is `EINVAL` whatever
the pointer (`setrlimit` said `EFAULT` for that too). `prlimit` itself wrote
the old limit before trying the new one, so a refused call still overwrote
the caller's buffer.

**Fix.** `getrlimit` and `setrlimit` are `prlimit` with the other pointer
NULL, as glibc's are; `prlimit` reads the new limit first and writes the old
one last, only on success.
