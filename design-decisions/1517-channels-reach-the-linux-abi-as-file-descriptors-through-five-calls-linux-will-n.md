## 1517. Channels reach the Linux ABI as file descriptors, through five calls Linux will not number

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** SlateOS's own way for programs to talk -- channels and named
services, with the kernel vouching for who is on the other end -- was out of
reach of every program written against the Linux interface. That means every
Rust program, the compositor (the program that draws every window) among
them. Now a Linux-interface program can make a channel, register or reach a
service, and ask who is at the other end, and it gets back ordinary file
descriptors. Everything a program already knows how to do with a descriptor
then works on them: read, write, wait on several at once, pass them to a
child. Lane F asked for this to build a local display connection whose
clients the kernel identifies
(`requests/f-a-a-channel-handle-can-be-guessed-and-any-process-can-use-it.md`,
point 3).

**What changed:**
- **Five calls, 1000-1004 in the Linux table:**
  - `slate_channel_create(fds[2], flags)`;
  - `slate_service_register(name, len, flags)`, which answers a listener;
  - `slate_service_accept(listener, flags)`;
  - `slate_service_connect(name, len, flags)`;
  - `slate_channel_peer_cred(fd, out)`.
- **What the descriptors do:**
  - `read` and `write` move one whole message each, as `SOCK_SEQPACKET`
    does: a short buffer gets the start and the rest is dropped, and a
    write over 64 KiB is `EMSGSIZE`.
  - A blocking wait can be interrupted by a signal.
  - `poll` and `epoll` see `POLLIN`/`POLLOUT`/`POLLHUP` (a listener:
    `POLLIN`).
  - `fstat` says `S_IFSOCK`.
- **Shared ends:** channel ends and listeners gained holder counts
  (`channel::dup`, `service::dup_listener`), so `fork` shares them and the
  last close releases them. A receive that empties a full queue now wakes
  the writers polling for room.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. Channels as descriptors, five dedicated calls (chosen)** | a program holds fds; every Linux mechanism works on them | poll/epoll, `fork`, `dup`, close-on-exec and `/proc/<pid>/fd` come free; settles point 4 for the Linux ABI too | five numbers of our own in the Linux table; a capability inside a message cannot yet travel as an fd |
| B. A reserved range that forwards to the native table | any native call reachable from the Linux ABI | one mechanism for every future native call | the native calls take native handles, which a Linux program has no way to wait on, share across `fork`, or close with the rest of its descriptors |
| C. `AF_UNIX`-style sockets that happen to be channels | `socket(AF_SLATE, SOCK_SEQPACKET, 0)`, names via `bind`/`connect` | no new call numbers | a fake address family, and `sockaddr` names for what is a registry, for the same five operations |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| Numbers 1000-1004 | the x32 range, or right after Linux's last | Linux's own numbers end in the 400s and grow by a handful a year; x32 is taken. `ENOSYS` on any other kernel makes them probe-able |
| A short read truncates, as `SOCK_SEQPACKET` | refuse with the message left queued | the behaviour socket programs already handle; the limit is fixed and known |
| A closed peer reads 0 and writes `EPIPE`, no `SIGPIPE` | raise `SIGPIPE` | this kernel's Linux layer raises it nowhere; raising it here alone would be a surprise |
| Capabilities attached by a native sender go into the reader's table | drop them | dropping them would lose authority silently; the descriptor form for them is the next step |
| `fork` now shares channel ends and listeners for native processes too | keep them uninherited natively | one rule for a resource whatever the ABI; inherited listening sockets work the same way |

**Revisit** when the display transport needs to pass a channel end or a
buffer inside a message (`SCM_RIGHTS` and the native capability transfer
it would map to).
