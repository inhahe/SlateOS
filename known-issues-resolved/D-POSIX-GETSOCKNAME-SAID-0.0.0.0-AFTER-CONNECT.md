### [D] D-POSIX-GETSOCKNAME-SAID-0.0.0.0-AFTER-CONNECT — 2026-09-27 — FIXED 2026-09-27

**Where:** `posix/src/socket.rs` (`connect`, `accept`, `getsockname`).

**What it was.** A socket's local address was only ever what `bind` set, so
`getsockname` on a connected or accepted socket said `0.0.0.0`. Programs ask
exactly this to learn their own address -- "connect a UDP socket to
8.8.8.8, read its name" is the common idiom -- and FTP's active mode, SIP and
`getaddrinfo`'s own sorting rely on it.

**Fix.** `connect` and `accept` record the address the connection goes out
from, as this system routes it (`route_source`): the loopback's for
127/8 and 0.0.0.0, `eth0`'s for anything its subnet or gateway reaches. A
datagram disconnect forgets it again, as Linux's does, unless `bind` set it.

**What remains.** The route is the C library's reading of `eth0`'s
configuration; a kernel with several interfaces or real routes would need to
say which it used.
