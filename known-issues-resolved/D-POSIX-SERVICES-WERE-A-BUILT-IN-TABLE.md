### [D] D-POSIX-SERVICES-WERE-A-BUILT-IN-TABLE — 2026-09-27 — FIXED 2026-09-27

**Where:** `posix/src/netdb.rs` (was `posix/src/socket.rs`).

**What it was.** `/etc/services` and `/etc/protocols` were never read:
`getservbyname` searched a table of 27 services with no aliases, one of
them under a name no system uses (`dns` for port 53), and `getprotobyname`
compared names ignoring case, where glibc compares them exactly. There was
no `getservbyname_r`, `getservbyport_r`, `getservent_r`, `getprotobyname_r`,
`getprotobynumber_r` or `getprotoent_r`, and no networks or ethers database
at all (`getnetbyname`, `getnetbyaddr`, `getnetent`, `ether_hostton`,
`ether_ntohost`). And a lookup in the middle of `getservent`'s enumeration
rewound it unless `setservent(1)` had been called -- the behaviour of glibc
before 2.33, not after: glibc 2.39, asked, keeps the enumeration's place.

**Fix.** The four databases as glibc 2.40's `nss_files` reads them: the file
when there is one, a built-in copy when there is not (design-decisions.md
§1127), parsed by `files-parse.c`'s rules -- including the service port's
base-0 number (`0x1f/tcp` is 31) that the macro's argument order gives it.
The tests replay glibc 2.39's answers to 100 lookups and enumerations over files built to
exercise the parser (`posix/tools/oracle/netdb_oracle.c`, run under WSL with those
files in place of `/etc`). A number past 32 bits clamped to `0xffffffff` as
upstream glibc clamps it; since 2026-09-28 it makes the line no entry, as
Debian's glibc (its `local-nss-overflow.diff`, and the oracle) has it --
for every one of these files at once (design-decisions §1136).
