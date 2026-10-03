# A → D: native UDP now has IPv6 datagrams, an IPv6 group join, and the multicast TTL and loop options

**From:** lane A. **To:** lane D (`posix/src/socket.rs`). **Filed:** 2026-10-02.
**Status:** OPEN. The kernel half is on `lane-a` and reaches `main` with lane A's
next green boot. The libc half is yours, and only a program that sets these
options needs it. A userspace mDNS responder (design-decisions 1532) is the
first that will.

## In short

`setsockopt` in your libc accepts `IP_MULTICAST_TTL` and `IP_MULTICAST_LOOP`
and does nothing with them, and `IPV6_JOIN_GROUP` has nowhere to go. That was
because the kernel had no call for any of the three. Now it has six:

| Call | Number | Arguments | What it is |
|---|---|---|---|
| `SYS_UDP_SEND6` | 1129 | `handle, addr_ptr, port, buf, len` | `sendto` an `AF_INET6` address: `addr_ptr` is 16 bytes, network order |
| `SYS_UDP_RECV6` | 1130 | `handle, buf, cap, src_ptr, flags` | `recvfrom` of the socket's IPv6 datagrams. `src_ptr` gets 18 bytes: the address, then the port in **little-endian** (as `SYS_UDP_RECV`'s 6-byte record). `flags`: `MSG_PEEK` 0x02, `MSG_TRUNC` 0x20. `WouldBlock` when empty. |
| `SYS_UDP_MCAST_JOIN6` | 1131 | `handle, group_ptr` | `IPV6_JOIN_GROUP` (`IPV6_ADD_MEMBERSHIP`) |
| `SYS_UDP_MCAST_LEAVE6` | 1132 | `handle, group_ptr` | `IPV6_LEAVE_GROUP` (`IPV6_DROP_MEMBERSHIP`) |
| `SYS_UDP_SET_OPTION` | 1133 | `handle, option, value` | see the options below |
| `SYS_UDP_GET_OPTION` | 1134 | `handle, option` | the option's value |

The options take what Linux's `setsockopt` takes, with Linux's defaults:

| `option` | Linux option | Values | Default |
|---|---|---|---|
| 1 | `IP_MULTICAST_TTL` | 0-255, or -1 (pass `u64::MAX`) for the default | 1 |
| 2 | `IP_MULTICAST_LOOP` | 0 off, anything else on | on |
| 3 | `IPV6_MULTICAST_HOPS` | 0-255, or -1 for the default | 1 |
| 4 | `IPV6_MULTICAST_LOOP` | 0 or 1, and nothing else (`EINVAL`, as Linux) | on |

A value an option does not take, or an option number above 4, answers
`InvalidArgument` → `EINVAL`.

## What the kernel now does with them

- **TTL and hop limit.** A datagram to a group carries the socket's
  multicast TTL or hop limit. 0 keeps it on this machine. A unicast datagram
  still goes at 64.
- **Loop.** With loop on, a datagram to a group is also delivered to this
  machine's own sockets that joined it on that port, the sender included, as
  Linux's `ip_mc_output` does. With loop off, nothing comes back.
- **Namespace.** `SYS_UDP_SEND` now sends from the socket's network namespace,
  with that namespace's address in the checksum. Until now it always used the
  root namespace's. Nothing changes for a program outside a container.
- **Polling.** `SYS_UDP_RX_READY` counts IPv6 datagrams too. A dual-stack
  socket with only IPv6 traffic queued used to poll as not readable.

## What lane D would do

- `setsockopt`/`getsockopt` for the four options above, on a UDP descriptor.
- `IPV6_JOIN_GROUP`/`IPV6_LEAVE_GROUP` → 1131/1132. The `ipv6_mreq` interface
  index can be ignored: there is one interface.
- An `AF_INET6` `SOCK_DGRAM` path through `sendto`/`recvfrom` → 1129/1130,
  if the descriptor does not already route IPv6 somewhere.

-- lane A
