## 1519. Unix-domain sockets with names live in the kernel, and a name leads to its socket by the node's identity

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** programs ported from Linux meet each other at a path -- the
system log at `/dev/log`, X11 at `/tmp/.X11-unix/X0`, D-Bus, PostgreSQL,
`ssh-agent`, `tmux` -- by a kind of socket SlateOS did not have: one with a
name in the filesystem (`requests/b-ad-a-unix-socket-cannot-be-bound-to-a-path-so-nothing-can-receive-syslog.md`).
It now has them. The kernel keeps the sockets; the filesystem holds only a
marker at the path; and the marker leads to its socket by which file it is,
not by its name, so renaming or deleting it behaves as on Linux.

**What changed:**
- `ipc::unix_socket`: stream and datagram sockets, bound to a path or an
  abstract name; listen/connect/accept, datagram queues, the peer's
  credentials as the kernel recorded them.
- `EntryType::Socket`, made by `bind` on memfs, devfs (`/dev/log`) and ext4
  (`FileSystem::mknod_socket`); `open` of one is `ENXIO`.
- Linux `socket(AF_UNIX, ...)`, `socketpair`, and every call on the
  descriptors; native `SYS_UNIX_*` (1104-1118) for the C library.
- The sender's credentials on receive: `SO_PASSCRED` is the socket's (an
  accepted connection starts with its listener's, as on Linux), and
  `recvmsg` then writes one `SCM_CREDENTIALS` message as Linux's `put_cmsg`
  does; natively `UNIX_OPT_PASSCRED` through `SYS_UNIX_SET_OPTION` /
  `SYS_UNIX_GET_OPTION` (1117/1118), kept on the socket rather than in the C
  library so every holder after `fork` or `exec` sees one setting.
- Credentials a sender states (`SCM_CREDENTIALS` on `sendmsg`, as `logger
  --id` sends them): checked as Linux's `scm_check_creds` checks them -- its
  own pid, uid and gid, or for root (uid 0, as the Linux layer's `set*id`
  calls take it) any live process's pid and any ids -- and carried by the
  datagram in place of the kernel's record. A stream checks them but reports
  its connection's credentials: its bytes keep no write boundaries to hang
  them on.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. In the kernel, with the existing IPC objects (chosen)** | a Unix-domain socket is a kernel object like a pipe or a socketpair end | the connected half *is* a `socketpair` end (`stream_socket`), which already lived here; the name's marker is a filesystem node, which only the kernel can make; IPC is one of the five things the microkernel rule keeps in the kernel | the kernel holds datagram queues (bounded: 128 datagrams / 1 MiB a socket) |
| B. A userspace service that emulates them over channels | the C library talks to a socket service | the kernel stays smaller | every socket call becomes two IPC hops; the service still needs the kernel to make the marker node and to tell it who is calling; `poll` on a descriptor would have to reach into the service |
| C. Emulate in the C library alone (a file and a channel) | no kernel work | -- | Linux programs not built on our C library (Rust `std` on `x86_64-slateos`, any static binary) would get nothing; credentials could be forged |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| A path name leads to its socket by the node's `FileId` (`(fs_id, ino)`) | by the path string | Linux binds the socket to the inode: a renamed node still reaches it, a second hard link too, an unlinked one nothing -- keying by identity gives that for free, keying by text gets all three wrong |
| The table of names is a `fs::perfile` table | entries removed only when the socket closes | an entry that outlived its node would lead the next client to whatever file reuses the inode number (ext4 reuses them) |
| Closing a socket leaves its node, refused to `connect` | remove the node on close | Linux leaves it, and programs expect to `unlink` before `bind`; removing it would break a server that binds, closes and rebinds the same path in a race with a client that checks for the node |
| A full datagram queue makes the sender wait | drop the datagram | Linux waits; a syslog client that drops messages under load loses exactly the messages from the moment something went wrong |
| Credentials are recorded by the kernel at `connect`/`listen` and per datagram; kernel context records none | report uid 0 for kernel-made sockets | as `ipc::service` records a channel's peer: "unknown" must not read as the strongest credential there is |
| `SCM_RIGHTS` is refused (`EOPNOTSUPP`) -- until §1521 built it, the same day | ignore the control message and send the data | a receiver expecting descriptors that never come fails later and more confusingly than a sender told no now |
| Credentials a sender states are checked as Linux checks them, then carried | always the kernel's record, the statement ignored | `logger --id=PID` states a pid on root's authority so the log names the process the line is about; ignoring it would name `logger`, and a claim Linux refuses (`EPERM`) would silently succeed |
| Native names: an absolute path or an abstract name, with a flag | the Linux `struct sockaddr_un` | native calls take absolute paths everywhere (§648: the kernel has no working directory of the C library's to resolve against); the library converts `sun_path` |
| A new resource type, `UnixSocket` (33), for the holder's `ipc_handles` | reuse `StreamSocket` | the cleanup and fork arms dispatch on the type; a listener or a datagram socket is not a stream end |

**Not done yet** (`known-issues.md` `A-UNIX-SOCKETS-CARRY-NO-DESCRIPTORS-OR-CREDENTIAL-MESSAGES`):
nothing, since the same day -- credential control messages on receive,
`SO_RCVTIMEO`/`SO_SNDTIMEO`, descriptor passing (`SCM_RIGHTS`, §1521) and
`SOCK_SEQPACKET`, all first listed here, were built then.

**Revisit** if the kernel's datagram queues become a memory concern (a limit
per process rather than per socket), or if Wayland or D-Bus arrive needing
`SCM_RIGHTS` -- the first thing to build next.
