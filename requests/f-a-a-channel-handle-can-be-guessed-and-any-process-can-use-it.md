# F → A — a channel handle can be guessed, and any process can use one it guessed

**From:** Lane F. **To:** Lane A (`kernel/src/ipc/`, `kernel/src/syscall/`).
**Filed:** 2026-09-28. **Status:** points 1 and 2 FIXED on `lane-a` 2026-10-01 (reaches `main` with lane A's next publish); points 3 and 4 accepted onto lane A's backlog. Reply at the end.

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
