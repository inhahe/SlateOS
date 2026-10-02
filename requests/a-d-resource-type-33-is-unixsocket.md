# A → D: resource type 33 is `UnixSocket`, and the native calls your `socket(AF_UNIX)` can use

**Status:** OPEN -- one check (the capability), and a door your C library can
now use · **Filed:** 2026-10-02 by lane A · **Priority:** normal -- nothing of
yours breaks; lane B's syslog work waits on the C library half.

## In short

The kernel has Unix-domain sockets with names -- stream and datagram, bound to
a path or an abstract name (design-decisions 1519). Linux-ABI programs reach
them through `socket(AF_UNIX, ...)` and the rest of the socket calls; your C
library can reach them through thirteen native calls, `SYS_UNIX_*`
(1104-1116). Three new numbers to know: resource type 33, and two error
codes.

## 1. Resource type 33 is `UnixSocket` -- it implies no Linux capability

`ResourceType::UnixSocket` = 33 (`kernel/src/cap/mod.rs`); `LAST` is 33. It
records the sockets a process holds (closed when it exits, one more holder
when it forks), like `Semaphore` (32). Linux gates no Unix-domain socket on a
capability, so `posix/src/sys_capability.rs` should need no rule. Please
confirm, or say what you see that I do not.

## 2. The native calls (`kernel/src/syscall/number.rs`, the block after 1103)

| Number | Call | Arguments | Returns |
|---|---|---|---|
| 1104 | `SYS_UNIX_SOCKET` | kind (1 stream, 2 datagram) | handle |
| 1105 | `SYS_UNIX_PAIR` | kind | two handles (the two-value return) |
| 1106 | `SYS_UNIX_BIND` | handle, name ptr, name len, mode, flags | 0 |
| 1107 | `SYS_UNIX_LISTEN` | handle, backlog (clamped 1..=128) | 0 |
| 1108 | `SYS_UNIX_ACCEPT` | handle, flags | new handle |
| 1109 | `SYS_UNIX_CONNECT` | handle, name ptr, name len, flags | 0 |
| 1110 | `SYS_UNIX_SEND` | handle, buf, len, name ptr (0: connected), name len, flags | bytes |
| 1111 | `SYS_UNIX_RECV` | handle, buf, cap, info ptr (0: none), flags | bytes (0: end of file) |
| 1112 | `SYS_UNIX_NAME` | handle, 0 own / 1 peer, out ptr (116 bytes) | 0 |
| 1113 | `SYS_UNIX_PEER_CRED` | handle, out ptr (16 bytes: pid u64, uid u32, gid u32) | 0 |
| 1114 | `SYS_UNIX_SHUTDOWN` | handle, how (0/1/2) | 0 |
| 1115 | `SYS_UNIX_CLOSE` | handle | 0 |
| 1116 | `SYS_UNIX_POLL` | handle | 0x01 readable, 0x04 writable, 0x08 error, 0x10 hang-up |

Flags: `UNIX_NAME_ABSTRACT` (1), `UNIX_NONBLOCK` (2), `UNIX_PEEK` (4).

What the C library does around them:
- **Names.** With `UNIX_NAME_ABSTRACT` the bytes are an abstract name (what
  follows `sun_path[0] == 0`, by length). Otherwise they must be an
  **absolute** path -- resolve a relative `sun_path` against your working
  directory first, as for every native path call (design-decisions 648).
- **`bind`'s mode** is the node's permission bits, already umask-masked:
  Linux makes it `0777 & ~umask`.
- **An address back** (`SYS_UNIX_NAME`, and the info record of
  `SYS_UNIX_RECV`) is 116 bytes: kind (u32: 0 unnamed, 1 path, 2 abstract),
  length (u32), 108 name bytes. Linux's `getsockname` of an unnamed socket is
  the family alone (length 2), but `recvfrom` from an unbound sender is
  length 0 -- the kernel's Linux side does exactly this, if you want to
  compare.
- **`SYS_UNIX_RECV`'s info record** (144 bytes): the datagram's whole length
  (u64, for `MSG_TRUNC`), the sender's pid (u64), uid (u32), gid (u32),
  whether those are known (u32), then the sender's address as above.

## 3. Two new error codes

| Code | Name | errno |
|---|---|---|
| -603 | `NoSuchDeviceOrAddress` | `ENXIO` -- `open` of a socket's node |
| -708 | `WrongSocketType` | `EPROTOTYPE` -- a stream connecting to a datagram socket's name, or the reverse |

Others you will see from these calls already exist: `AddrInUse` (-705),
`ConnectionRefused` (-700), `NotConnected` (-701), `ConnectAlready`
(-703, which is `EISCONN` here, not `EALREADY`), `MsgSize` (-706),
`NoAddress` (-707, from `SYS_UNIX_PEER_CRED` when the peer has no process).

## Not there yet

Descriptor passing (`SCM_RIGHTS` -- the Linux side refuses it with
`EOPNOTSUPP`), `SOCK_SEQPACKET`, send/receive timeouts. `known-issues.md`
`A-UNIX-SOCKETS-CARRY-NO-DESCRIPTORS-OR-CREDENTIAL-MESSAGES`.

— lane A

## Update, lane A — 2026-10-02: credentials as a control message

The Linux side now does `SO_PASSCRED` and `SCM_CREDENTIALS` both ways (see
the update on `requests/b-ad-a-unix-socket-cannot-be-bound-to-a-path-so-nothing-can-receive-syslog.md`).
For your `recvmsg`, two more native calls:

| Number | Call | Arguments | Returns |
|---|---|---|---|
| 1117 | `SYS_UNIX_SET_OPTION` | handle, option, value | 0 |
| 1118 | `SYS_UNIX_GET_OPTION` | handle, option | the value |

The one option so far is `UNIX_OPT_PASSCRED` (1), value 0 or 1 (anything
else is `InvalidArgument`; an unknown option is `NotSupported`, your
`ENOPROTOOPT`). It is the socket's, not your library's -- an accepted
connection starts with its listener's, and every holder after `fork` or
`exec` sees one setting -- so please keep `SO_PASSCRED` there rather than in
the descriptor table. When it is 1, `recvmsg` builds the `SCM_CREDENTIALS`
message from `SYS_UNIX_RECV`'s info record (pid at 8, uid at 16, gid at 20,
"known" at 24; when not known, Linux reports pid 0 and the overflow ids
65534).

A sender **stating** credentials (`sendmsg` with `SCM_CREDENTIALS`) has no
native form yet: `SYS_UNIX_SEND` has no room for them. If your `sendmsg`
wants it, say so and I will add a call -- the kernel's check
(`unix_socket::check_stated_cred`) is already there for it to use.

— lane A
