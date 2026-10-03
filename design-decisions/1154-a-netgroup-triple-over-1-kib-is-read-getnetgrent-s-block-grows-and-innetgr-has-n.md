## 1154. A netgroup triple over 1 KiB is read: `getnetgrent`'s block grows and `innetgr` has no buffer

**Date:** 2026-09-30
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** netgroups are named sets of (host, user, domain) triples in
`/etc/netgroup`. glibc reads a triple into a fixed 1 KiB buffer in two
places -- `getnetgrent`, which returns them one by one, and `innetgr`,
which asks whether a group has one -- and a longer triple is treated as the
end: `getnetgrent`'s enumeration stops there, and `innetgr` looks no further
in that group. Here both read a triple of any length: `getnetgrent`'s block
grows, as every other non-reentrant lookup's does in glibc and here, and
`innetgr` compares the triple where it lies. Every other answer is glibc's
(`netgroup_oracle.txt`).

| | glibc 2.39 | here |
|---|---|---|
| `getnetgrent` at a triple over 1 KiB | 0, as at the end; the triples after it are never given | the triple, and the ones after it |
| `innetgr` for a triple after one over 1 KiB in the same group | 0, the group's scan abandoned | 1 |
| `getnetgrent_r` with a buffer too small | 0, `errno` `ERANGE`, the same triple next time | the same |

**The alternatives:** reproduce the limit, which loses the end of a group
without a word -- no error tells the caller the enumeration was cut short;
or make it an error, which no caller of a function that returns 0 at the
end could see either. glibc's `getpwnam`, `getgrnam` and the rest grow
their buffers; its netgroup functions are the ones that do not.

**Where:** `posix/src/netgroup.rs` (`getnetgrent`, `innetgr`).
