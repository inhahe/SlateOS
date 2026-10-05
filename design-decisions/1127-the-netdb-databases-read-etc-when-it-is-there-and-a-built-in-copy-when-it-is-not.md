## 1127. The netdb databases read `/etc` when it is there, and a built-in copy when it is not; their answers are the calling thread's

**Date:** 2026-09-27
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** programs look up services ("http" is port 80), protocols
("tcp" is 6), networks and Ethernet names in four files under `/etc`. The
booted system has no `/etc` of its own yet, so this library answers from a
built-in copy when a file is missing -- as `/etc/passwd` answers with its
`root` entry (§1113) -- and from the file when there is one. And the
functions that answer in library storage use storage of the calling
thread's, where glibc shares one buffer between all threads.

| Question | Chosen | Alternatives |
|---|---|---|
| a missing file | **the built-in copy**: the IANA registries' well-known entries, written for this project in the files' own format and read by the same parser | glibc: no answer at all -- so `getaddrinfo(host, "http")` would fail on a system with no `/etc/services`; or copy Debian's `netbase` files, which are GPL-2 |
| `getservbyname`'s buffer, `getservent`'s place | **the calling thread's** (allocated on first use, freed when it exits) | glibc's: one per process, behind a lock -- two threads enumerating take turns consuming each other's entries |
| a number past 32 bits in a file | **clamped** to `0xffffffff`, upstream glibc's `strtou32` | Debian's glibc refuses the line (a local patch) |

**What would change it.** Staging real `/etc` files on the image makes the
built-in copies unused, with nothing to change here: a file always wins.
