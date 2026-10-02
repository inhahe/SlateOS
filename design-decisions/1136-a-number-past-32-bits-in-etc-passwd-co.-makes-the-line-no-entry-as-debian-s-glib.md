## 1136. A number past 32 bits in `/etc/passwd` & co. makes the line no entry, as Debian's glibc has it

**Date:** 2026-09-28
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** the account files (`/etc/passwd`, `/etc/group`, `/etc/shadow`)
and the network ones (`/etc/services`, `/etc/protocols`, `/etc/networks`)
hold numbers -- user and group ids, ports, days. When one is too big for 32
bits, or negative (`-1`), upstream glibc quietly turns it into 4294967295,
while the glibc that Debian and Ubuntu ship ignores the whole line. This
library did what upstream does; it now does what Debian's does. The reason
is safety: 4294967295 is `(uid_t) -1`, which to `setresuid` means "leave this
id as it is", so a server that drops root by switching to such a user would
stay root -- and be told the switch worked.

**What changes, observably.** A line like `evil:x:-1:0::/:/bin/sh` or
`big:x:4294967296:1::/:/bin/sh` is no longer an entry: `getpwnam("evil")`
finds nobody, and enumeration skips it, where it used to find a user with
uid 4294967295. `-0`, `+5`, ` 5` and `05` still read as numbers, exactly as
before; only values past `UINT_MAX` change.

**The two glibcs.** Upstream's `files-parse.c` reads a number field with
`strtou32`, which clamps anything past `0xffffffff` to `0xffffffff`.
Debian's `local-nss-overflow.diff` (in Debian's glibc since 2009, so in
every Debian and Ubuntu release since) reads it with `strtoull` and makes
the line no entry when the value passes `UINT_MAX`. The oracle this library
is tested against is Ubuntu 24.04's glibc 2.39 under WSL, so it is the
second: asked, it refuses `4294967296`, `10000000000`, `-1`, `-4294967295`
and `18446744073709551616`, and accepts `4294967295`, `-0`, `+5`, ` 5` and
`05` (`posix/tools/oracle/accounts_harness.py`'s files carry such lines).
The netdb change of 2026-09-27 had chosen upstream's clamp and kept those
lines out of its oracle; this reverses that, for every one of these files at
once, so no two parsers in this library disagree about the same line.

**Found on the way:** `strtoull` answers `ULLONG_MAX` for a number too long
for 64 bits whatever its sign. Both parsers here saturated and then negated,
so `-99999999999999999999` came out as 1. It is past 32 bits now, as it is
in both glibcs.

**Alternatives:**

- **Upstream's clamp** (what this library did). The reference glibc, and
  what Fedora and Arch ship. Rejected: it makes an id of `(uid_t) -1` out of
  a line nobody meant to say that, and the privilege-drop failure above is
  silent; the oracle does not do it either.
- **Clamp for the network files, refuse for the account files.** Only the
  account files carry the `setresuid` hazard. Rejected: glibc -- either one
  -- reads all of them with the same macros, and a port of `-1` is no more
  meaningful than a uid of `-1`.
