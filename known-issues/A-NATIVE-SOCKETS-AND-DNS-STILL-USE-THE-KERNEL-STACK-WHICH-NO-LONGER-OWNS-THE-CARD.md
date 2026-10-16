## A-NATIVE-SOCKETS-AND-DNS-STILL-USE-THE-KERNEL-STACK-WHICH-NO-LONGER-OWNS-THE-CARD (lane A, 2026-10-16) — OPEN

**Status:** OPEN (lane A). Found reading the kernel's remaining users of its
network stack, for decision 934's next slice. By code reading; not yet
reproduced by a test.

**In short:** since 2026-09-12 the network daemon owns the network card by
default, and the kernel's own network stack stops reading what arrives on
it. Programs that use the Linux system calls reach the daemon and work. But
native programs -- everything built on SlateOS's own C library, whose
`connect`, `send`, `recv` and name lookups are the native `SYS_TCP_*`,
`SYS_UDP_*` and `SYS_DNS_RESOLVE*` calls -- are still served by the
kernel's stack. Their packets go out; the replies arrive at the daemon, not
at them. So a native program's connection to anything but this machine
should never complete, and its name lookups should time out, on a default
boot.

### Where

| Call | Served by | Does not reach the network because |
|---|---|---|
| `SYS_TCP_CONNECT`, `_SEND`, `_RECV`, `_LISTEN`, `_ACCEPT`, `_CLOSE` (`syscall/handlers.rs`) | `net::tcp` | `net::poll` drains the card only while no raw owner holds it (`net::raw::is_claimed`) |
| `SYS_UDP_*` | `net::udp` | the same |
| `SYS_DNS_RESOLVE`, `SYS_DNS_RESOLVE2`, the reverse lookups | `net::dns`, over `net::udp` | the same |
| lane D's C library: `posix/src/socket.rs` (`connect` → `SYS_TCP_CONNECT`), `posix/src/hosts.rs` (`SYS_DNS_RESOLVE`) | the calls above | -- |

Loopback (`127.0.0.1`) and container traffic over veths still work: neither
touches the card.

### The fix

The native calls are served by the daemon, as the Linux ones are
(`net::socket`, over `net::netstack_client`): each native socket handle
names a daemon session instead of a kernel slot, and the resolver calls
forward to the daemon's resolver (which `net::netstack_client` already
reaches for `SYS_DNS_RESOLVE` in its self-test). Container namespaces'
sockets stay on the kernel stack until `open-questions/A-Q29.md` is
answered. This is the next part of decision 934's slice 2, and the kernel
stack's TCP and UDP cannot go until it is done.

### A test that would have caught it

A native (C library) program that resolves a name and connects outside the
machine -- QEMU's user network answers on `10.0.2.2` -- run in the default
configuration. Every network test today is a Linux-ABI program, or runs
with the kernel stack owning the card.
