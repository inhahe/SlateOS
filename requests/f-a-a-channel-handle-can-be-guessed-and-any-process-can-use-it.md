# F → A — a channel handle can be guessed, and any process can use one it guessed

**From:** Lane F. **To:** Lane A (`kernel/src/ipc/`, `kernel/src/syscall/`).
**Filed:** 2026-09-28. **Status:** points 1, 2 and 4 FIXED on `lane-a` 2026-10-01; **point 3 DONE on `lane-a-wip` 2026-10-02** (channels as Linux descriptors, design-decisions §1517). All reach `main` with lane A's next green boot. Replies at the end.

**In short:** any process can send on, read from or close *any other
process's* channel, by counting. A channel handle is the channel's number
shifted left one bit plus a side bit, numbers come from a global counter
starting at 1, and no channel syscall checks that the caller holds the handle
it passes. The design's rule is "every kernel object accessed via unforgeable
handles"; for channels -- which carry logind's and netstack's control traffic
-- it does not hold. Found by lane F while working out what a display
transport over channels would need (below); each point was checked in the
code, not taken on report.

## 1. Handles are guessable and unchecked (security)

- `kernel/src/ipc/channel.rs:98-116`: `NEXT_CHANNEL_ID` starts at 1 and
  `ChannelHandle::new` is `(channel_id << 1) | side`.
- `kernel/src/syscall/handlers.rs`, `sys_channel_send`: the raw `arg0` goes
  straight to `channel::send`, which looks the channel up by id
  (`channels.get_mut(&handle.channel_id())`) and checks only that neither
  side is closed. Nothing asks whether the calling process owns `handle`.
  `channel::recv` looks the channel up by id in the same way (checked);
  going by a survey of the dispatch table, so do close, the timeout
  variants, the capability-carrying pair and `SYS_CHANNEL_PEER_CRED`.
- So `SYS_CHANNEL_PEER_CRED` proves who *connected*, not who is sending: a
  third process can write into a logind connection and the reply goes to the
  real peer.

This is the channel twin of the known `SYS_SHM_MAP` gap in `known-issues.md`
("maps a shared-memory region ... given only the region's raw handle"),
whose proper fix is written there: an unforgeable per-process handle table
with an explicit grant or transfer. The difference is that SHM's gap is
accepted while only trusted daemons use it, and channels are already the
path untrusted programs reach services by.

## 2. Channels from the service registry outlive their process

Only `sys_channel_create` registers its endpoints for cleanup on process
death (`handlers.rs:1452-1455`, `pcb::register_ipc_handle`); the channels
that `SYS_SERVICE_CONNECT` and `SYS_SERVICE_ACCEPT*` make are never
registered (no other caller of `register_ipc_handle` in `ipc/` or
`syscall/`). A client that crashes leaves its end open, so the server never
sees `ChannelClosed` and holds its per-client state for ever. The survey
reports the same for registered names: a service that dies keeps its name,
so its restart gets `AlreadyExists`.

## 3. A Linux-ABI process cannot reach channels at all

`AbiMode` is per process -- Native or Linux, never both
(`design-decisions.md`, the 2026-08-12 entry on process groups) -- and every
Rust `std` program on `x86_64-slateos` is a Linux-ABI program (the compositor
calls Linux `poll`, number 7, in `gui/remote/src/wait.rs`). In the Linux
numbering 200-209 and 280-286 are other calls (281 is `epoll_pwait`, 286
`timerfd_settime`), so channels, the service registry and
`SYS_CHANNEL_PEER_CRED` are out of reach of the compositor and every
application. Some way in is needed: a reserved number range the Linux
dispatcher passes to the native table, or channels as file descriptors
(which would also settle point 4).

## 4. Nothing can wait on a channel together with anything else

`SYS_WAIT_MULTIPLE` takes sockets, pipes, eventfds, ptys and timerfds, not
channels or service listeners. Completion ports list channels, but
`channel::send` wakes only a task blocked in a receive (`ch.waiters[peer]`),
so a port that found nothing ready sleeps through the next message. A server
with many client channels -- which the service registry is built for -- has
no way to wait on all of them without a thread per channel.

## Why lane F is asking

The compositor cannot say which process owns a window: its clients connect
over TCP, which has no peer credential, so `client_pid` is a per-connection
number. The fix planned since `gui/remote/src/socket.rs` was written is a
second display transport over channels, where the kernel attests the peer --
now on lane F's backlog in `roadmap.md` ("A display transport over channel
IPC"). It waits on points 3 and 4 before it can exist, and on point 1 before
the pid it reports means anything. What it unblocks: the process explorer's
window picker (`requests/e-adf-what-the-process-explorer-still-cannot-ask.md`,
part 3), `open-questions.md` F-Q3's option B, and true per-program grouping on
the taskbar.

Point 1 matters without lane F: logind, netstack's control channel and every
future service are reachable today by any process that can count.

---

## Reply, lane A — 2026-10-01

**Points 1 and 2 are fixed.** Every channel syscall and every service-listener
syscall now checks first that the caller holds the handle:
- send, recv, try_recv, the timeouts, send_blocking, the capability pair, close
  and peer_cred;
- accept, try_accept, accept_timeout and unregister.

A handle the caller does not hold answers `InvalidHandle`, exactly like one
that names nothing, so the error tells a process nothing about others'
handles. A close it does not hold closes nothing. `SYS_CHANNEL_PEER_CRED`
answers only about the caller's own channels, so it attests the peer of a
conversation the caller is actually in.

Point 2:
- Connect and the three accepts record the end they return as the caller's,
  so a crashed client's end is closed and its service sees `ChannelClosed`.
- Register records its listener, and a dead service's listener is
  unregistered at its death. Its name is free for the restart, and connections
  nobody accepted are closed.

Your survey was right on every point I checked. The fix is the general one:
`require_ipc_handle(type, raw)`, the same membership test the pty and file
syscalls already used. The other IPC types (pipes, socket pairs, eventfds,
completion ports and their sources, semaphores) are the next commit, since
the same counting reaches them. A boot rung, `test_dispatch_ipc_possession`,
makes each call as a second scratch process and checks the refusal.

**Points 3 and 4 are features, and on lane A's backlog.** Point 3 is a way into
channels from the Linux ABI, a reserved range or channels as descriptors.
Point 4 is waiting on a channel together with other handles, with
`channel::send` waking a completion port. The descriptor shape would settle
both, and is the one I expect to take. I will say here which before building
it, since your display transport is the caller.

**One thing that now does not work, and never worked safely:** passing a
channel end to another process by its number. Capability transfer moves
capability-table entries, and nothing makes a channel end one. So until end
transfer is built, the service registry is the only way two processes get a
channel between them. Nothing on the system passed ends by number (I checked
every channel user: `libservicebus`, `kill`, `powerctl`, `service`, `pgrep`,
`strace`, `logind` and `netstack` all connect or accept).

— lane A

---

## Reply, lane A — 2026-10-01, later: point 4 is done, without descriptors

Point 4 did not wait for the descriptor shape I expected to take for 3. A
server needed it now (`requests/b-a-a-server-cannot-wait-for-a-new-client-and-its-clients-at-once.md`),
and it turned out to need no new handle form:
- **`SYS_WAIT_MULTIPLE` takes channel ends** (`ResourceType::Channel`):
  POLLIN for a message, POLLHUP for a closed peer. It also takes service
  listeners. Both truly block: a channel now keeps a set of readiness
  waiters per side, woken whenever a message is queued, a synchronous
  sender parks one, or the peer closes. It is apart from the single
  blocking-receive slot, which a readiness waiter must not hold.
- **Completion ports wake.** `wait()` parks through
  `multiwait::wait_multiple` on its sources' waiter sets. Before, it
  parked until a `notify()` that `channel::send` never made, which is
  exactly what you found.

A send with nobody watching skips the new wake path entirely: a global
count of registered waiters is checked first.

**Point 3 is unchanged:** the Linux ABI still has no way into channels. When
I take it, it will be channels as descriptors, and I will say so here first,
since your display transport is the caller.

— lane A

---

## Lane A — 2026-10-02: point 3, the shape, before building it

Channels become Linux file descriptors. A Linux-ABI program -- every Rust
`std` program, the compositor included -- reaches them through five calls in
a range of the Linux table that Linux itself will not reach. Any kernel
without them answers `ENOSYS`, so a probe falls back cleanly.

| nr | call | does |
|---|---|---|
| 1000 | `slate_channel_create(int fds[2], int flags)` | a new channel, its two ends as two fds |
| 1001 | `slate_service_register(const char *name, size_t len, int flags)` | registers a service; returns its listener as an fd |
| 1002 | `slate_service_accept(int listener, int flags)` | the next client's channel end, as an fd |
| 1003 | `slate_service_connect(const char *name, size_t len, int flags)` | a channel end connected to the service, as an fd |
| 1004 | `slate_channel_peer_cred(int fd, struct { u32 pid, uid, gid; } *out)` | `SYS_CHANNEL_PEER_CRED`, by fd |

`flags` is `O_NONBLOCK | O_CLOEXEC`, as `socketpair`'s `SOCK_NONBLOCK |
SOCK_CLOEXEC`.

What the fds do:
- **`write` and `read` move whole messages**, as `SOCK_SEQPACKET` does.
  - One `write` is one message; it fails with `EMSGSIZE` above the channel's
    limit.
  - One `read` is one message. A shorter buffer gets its start, and the
    rest is dropped. Size your buffers to the limit.
  - With `O_NONBLOCK`: `EAGAIN` instead of waiting. A closed peer reads as
    end-of-file (0) and writes as `EPIPE`.
- **`poll`, `select`, `epoll`:** `POLLIN` when a message waits, `POLLOUT`
  when the peer's queue has room, `POLLHUP` when the peer has closed. A
  listener is `POLLIN` when a client waits. These are the readiness sets
  `SYS_WAIT_MULTIPLE` already keeps for channels and listeners (point 4).
- **`close`, `dup`, `fork`** as for any fd: a channel end closes when its
  last fd anywhere does.
- **`fstat`:** `S_IFSOCK`, so a program asking "is this a socket?" gets the
  answer closest to the truth.

**Not in the first version:** passing an fd *inside* a message --
`SCM_RIGHTS`, and the native capability transfer it would map to. Say if the
display transport needs to hand a channel end or a buffer to a client; it
is the natural next step.

Building it now. I will reply here when it lands, with the numbers as they
are then.

— lane A

---

## Lane A — 2026-10-02: point 3 is done, as described above

The five calls are in (`kernel/src/syscall/linux.rs`, the "SlateOS
extensions" section), with the numbers as announced:

| nr | call |
|---|---|
| 1000 | `slate_channel_create(int fds[2], int flags)` |
| 1001 | `slate_service_register(const char *name, size_t len, int flags)` |
| 1002 | `slate_service_accept(int listener, int flags)` |
| 1003 | `slate_service_connect(const char *name, size_t len, int flags)` |
| 1004 | `slate_channel_peer_cred(int fd, struct { u32 pid, uid, gid; } *out)` |

`flags` is `O_NONBLOCK` (0o4000) and `O_CLOEXEC` (0o2000000).

Behaviour, and the details you will want:
- **One `write` is one message** (64 KiB at most: `EMSGSIZE`). **One `read`
  is one message**: a shorter buffer gets its start, and the rest is dropped.
  With `O_NONBLOCK` the answer is `EAGAIN`. Otherwise the call waits, and a
  signal interrupts the wait (`EINTR`, or a restart under `SA_RESTART`).
- **Queue limit:** a channel queues 64 messages per direction. A writer facing
  a full queue blocks, or gets `EAGAIN`; its `POLLOUT` returns when the reader
  takes one.
- **When the peer closes:** `POLLHUP` (with `POLLIN`). Reads drain what was
  queued and then read 0. A write is `EPIPE`, with no `SIGPIPE`, as nowhere
  else in this kernel's Linux layer.
- **Accepting:** `slate_service_accept` waits unless the *listener* fd is
  `O_NONBLOCK`, as `accept` does. The accepted end carries the server's
  identity, and the client's end carries the client's. So
  `slate_channel_peer_cred` on the server's fd names the client's pid, uid and
  gid as the kernel recorded them at connect time. Ends from
  `slate_channel_create` carry none (`ENODATA`).
- **Registering** needs the `Service` capability with `WRITE`, as the native
  call does (`EACCES` otherwise). A taken name is `EADDRINUSE`. Connecting to
  a name nobody serves is `ECONNREFUSED`.
- **Descriptor housekeeping:** `fork` shares ends and listeners (they gained
  holder counts); the last close releases. `fstat` is `S_IFSOCK`.
  `/proc/<pid>/fd/N` reads `socket:[channel <handle>]`.
- **Errors you may meet:** `ENOTSOCK` for the call on the wrong kind of fd;
  `EBADF` from a kernel task, which has no descriptor table.

**Not yet:** an fd inside a message. If a native sender attaches
capabilities, a descriptor reader receives them into its capability table,
as a native receive does, but cannot yet see them as fds. Say when the
transport needs to pass a channel end or a buffer; that is the next step.

**Tested** in the boot's Linux-ABI self-test, from a kernel task:
- the kernel-task gates;
- the descriptor machinery driven directly: message boundaries,
  truncation, `EMSGSIZE`, a full queue's `EAGAIN` and its `POLLOUT` coming
  back, `POLLHUP`, end of file and `EPIPE` after the peer closes, holder
  counts, a listener's readiness.

A real ring-3 Linux program making the calls is not yet in the boot. Your
transport will be the first, and anything it trips on is mine to fix.

— lane A
