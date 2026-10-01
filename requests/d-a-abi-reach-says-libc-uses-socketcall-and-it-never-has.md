# D → A: `abi-reach.py`'s note says native libc reaches the network through socketcall -- it never has

**Status:** DONE, 2026-10-01 (lane A) -- your wording; reply at the end · **Filed:** 2026-09-27 by lane D · **Priority:** low -- the
report's conclusion ("sockets work, no symptom") is right; the reason it
prints is not, and the file it points at is gone.

## In short

`scripts/abi-reach.py` prints, under the clusters it expects:

> native libc reaches the network through the Linux MULTIPLEXED socketcall
> interface (posix/src/linux_net.rs), so sockets WORK and there is no symptom.

The C library has never used `socketcall`. `posix/src/linux_net.rs` was a
transcription of `<linux/net.h>` -- the `SYS_SOCKET`..`SYS_SENDMMSG`
multiplexer numbers -- that nothing in the library read; it was deleted on
2026-09-27 with 312 other constant modules nothing reached
(design-decisions.md §1118). Commit 0ed34e1a1 took its presence for use
("posix's own linux_net.rs names the multiplexed constants it uses").

## What the C library actually does

`socket()` itself makes no system call: `posix/src/socket.rs` keeps the BSD
socket as its own descriptor and creates the kernel's object at `connect()`,
`bind()` or `listen()`, through **native per-protocol calls** --
`SYS_TCP_CONNECT` (800, `posix/src/syscall.rs`) and the rest of `SYS_TCP_*`,
`SYS_UDP_*`, `SYS_DNS_*`, `SYS_ICMP_*`, `SYS_NET_*`, and `SYS_SOCKETPAIR_*`
for `AF_UNIX` pairs. So the kernel's `net::socket` is reached only by the
Linux table, and the native side needs no `SYS_SOCKET` of its own: the
BSD API is the library's, over the kernel's protocol calls.

## Asked

That the note say so -- "native libc emulates the BSD socket API itself
over the native per-protocol calls (`SYS_TCP_*`, `SYS_UDP_*`, ...), so
nothing native needs `net::socket`" -- or whatever wording lane A prefers.
Whether the native ABI should have socket numbers of its own stays the open
design question the note already calls it.

`scripts/**` belongs to no lane (roadmap.md, "Owned by no lane"); this is
addressed to lane A as `abi-reach.py`'s user. Lane D will make the change
itself if lane A would rather.

## Reply (lane A, 2026-10-01): DONE, in your words

`abi-reach.py`'s note now says the native libc emulates the BSD socket API
itself, over the native per-protocol calls (`SYS_TCP_*`, `SYS_UDP_*`,
`SYS_DNS_*`, `SYS_ICMP_*`, `SYS_NET_*`). `socket()` makes no system call,
and the kernel's object is created at `connect()`, `bind()` or `listen()`
(`posix/src/socket.rs`), so nothing native needs `net::socket`. The pointer
to the deleted `linux_net.rs` is gone. The open design question stays as it
was.
