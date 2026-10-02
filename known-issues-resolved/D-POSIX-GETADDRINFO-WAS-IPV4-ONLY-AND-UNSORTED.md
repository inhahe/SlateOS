### [D] D-POSIX-GETADDRINFO-WAS-IPV4-ONLY-AND-UNSORTED — 2026-09-27 — FIXED 2026-09-27

**Where:** `posix/src/gai.rs` (was `posix/src/socket.rs`).

**What it was.** `getaddrinfo` knew one family and one address:

- an `AF_INET6` hint was `EAI_FAMILY`, and a numeric IPv6 host (`::1`) was
  sent to the resolver as a name;
- `ai_protocol` was ignored, unknown `ai_flags` were accepted, `AI_CANONNAME`
  without a host was accepted, and `*` meant nothing;
- `AI_ADDRCONFIG`, `AI_V4MAPPED` and `AI_ALL` did nothing; `SOCK_RAW`
  entries were never listed, nor DCCP, UDP-Lite or SCTP when asked for;
- a resolver failure was `EAI_NONAME` whatever it was -- a timeout included;
- a service was read by this library's own rule, not `strtoul`'s (`+80`,
  `70000`, `2147483648` all differ);
- nothing was sorted, and `getnameinfo` refused IPv6 and `AF_UNIX` addresses.

**Fix.** glibc 2.40's `getaddrinfo` and `getnameinfo`, ported, over the hosts
database (§1128), with RFC 3484 sorting and `/etc/gai.conf` (§1129). The
tests replay glibc 2.39's answers to 100 calls under two `gai.conf` files, on
a sandbox with one IPv4 address and no IPv6 (`posix/tools/oracle/gai_oracle.c`).
