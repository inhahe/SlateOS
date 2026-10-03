### A-TCP-AND-UDP-SOCKETS-ARE-NOT-COUNTED-PER-PROCESS -- 2026-10-01 -- OPEN (lane A)

**Status:** OPEN -- fixed on `lane-a-wip` (option A, 2026-10-02), awaiting a
boot on main. `net::native_socket` keeps every native TCP/UDP handle:
- opaque ids from a counter that never repeats;
- registered per holder as `ResourceType::NativeSocket` (34), and checked by
  all 25 handle-taking calls;
- tied to the slot's generation, with the slot pinned during each call so it
  cannot be reused;
- counted at fork and spawn, released at close, exit and exec.

Tests: `native_socket::self_test` and dispatch's
`test_dispatch_native_socket_possession`.

**In short:** a TCP or UDP socket here belongs to nobody in particular.
Fork does not give the child its own reference (`proc::fork` treats
`ResourceType::Socket` as a permission token, not a per-open object), and
no process records which sockets it holds. So a socket cannot be closed for
one process alone. Closing it closes it for every process that uses it, and
a process that dies leaves its sockets open unless it closed them itself.

**Who it bites:**
- **Close-on-exec.** `SYS_PROCESS_SET_EXEC_CLOSE` (1090) closes a dropped
  descriptor's handle at exec for every other type, but must leave sockets
  open, or a child's exec would close the parent's connection. A
  close-on-exec socket therefore lives in the child until the child exits.
- **Exit.** A program that is killed holding connections leaves them open,
  with nothing to time them out but the peer.
- **Possession.** Any process with the `Socket` capability can name and use
  any socket by its number (`sys_tcp_close` checks the capability type, not
  possession) -- the hole channels had before 2026-10-01.

**Where it applies.** Only the native `SYS_TCP_*` / `SYS_UDP_*` calls on
the in-kernel stack (`kernel/src/net/tcp.rs`, `udp.rs`). Native libc's
sockets (`posix/src/socket.rs`) and about ten native programs (curl, nc,
inetd, ftpd ...) use them. The Linux-ABI socket descriptors
(`net::socket`, `ResourceType::NetSocket`) are already counted, inherited
and released per process.

**A second fact makes the fix bigger.** A handle is a slot index into a
fixed table (`CONNECTIONS`, `LISTENERS`, `SOCKETS`), and slots are reused.
Registration alone would not stop a process whose connection the stack
retired on its own (RST, time-out) from reaching the next connection
given that slot.

**The proper fix, two ways:**
- **A. Harden this API.** Generation-tagged handles (slot plus a counter,
  so a stale number names nothing), each handle registered in the opener's
  `ipc_handles` under a resource type of its own, possession checked in
  all 30 calls, counted duplicates for fork, release at exit. Then
  `close_handle_at_exec` can take them like the others.
- **B. Move native libc's sockets onto `net::socket`**, the counted
  objects the Linux ABI already uses, behind native calls of their own.
  This is where the netstack migration (roadmap 2.4, step 5.7) is heading
  anyway, and the resident stack's handle API would retire with it.

A secures what runs today. B is the destination, and when it happens
(5.7) is the operator's decision. **Lane A takes A next**, as a task of its
own once the 2026-10-01 batch (siginfo frame, nice authority, one id space,
exec close) has a green boot. A touches about 60 table-lock sites in the
5,000-line protocol module, and the networking rungs are its only test, so
it should not share a boot with other suspects.

**Reproduce.** Two processes with the `Socket` capability: one opens a TCP
connection; the other calls `SYS_TCP_CLOSE` with that number, and the first
process's connection is gone.
