### D-NETSOCK-SYNC. Daemon-backed AF_INET stream sockets (migration 5.5) are synchronous, single-stream, and IPv4-only — TECH DEBT (logged 2026-07-14)

**Where:** `kernel/src/net/socket.rs` + the switch-gated socket arms in
`kernel/src/syscall/linux.rs` (`sys_socket`/`connect`/`sendto`/`recvfrom`/
`getpeername`, `dispatch_socket_read`/`write`). All gated behind
`net.userspace` (default off).

**Status (2026-07-15):** the *TCP-client* Linux-parity line is now essentially
complete. Connected daemon-backed `AF_INET`/`AF_INET6` `SOCK_STREAM` sockets
honour non-blocking recv/send/connect, honest poll/epoll readiness,
`getsockname`/`getpeername`, `shutdown`, `setsockopt`/`getsockopt`
(`SO_ERROR`/`SO_TYPE`/buffer hints/`TCP_NODELAY`), `sendmsg`/`recvmsg`, the
`recvfrom` source-address out-param, and the `MSG_DONTWAIT`/`MSG_WAITALL`/
`MSG_PEEK` per-call flags. The remaining daemon-socket gaps are the large,
non-incremental ones: **(1) server sockets** (`bind`/`listen`/`accept4`) —
**DONE** (corrected 2026-09-09; this line read "gated on operator decision
Q23" long after Q23 was answered). Q23 resolved to **Option A** (§71): a
listening socket keeps one daemon session and each `accept` installs a new
fd sharing that session under its own `conn_id`, so no daemon-ABI change was
needed. What survives is *not* a missing feature but the cost of that
choice: **the listener and every connection it accepts share one SPSC
session behind one lock, so a server's accepted connections are served
strictly one at a time — one slow client holds up the others.** That
**STATUS 2026-09-13: the head-of-line block is GONE and the default is FLIPPED.**

Both halves of A-Q9 option C are on main. `81ef382d0` moved the wait above the
session lock -- all three kernel recv paths ask the daemon for a non-blocking
receive and back off here (16 yields, then 1 ms interruptible sleeps), so no
reply is ever withheld while the shared session is held. `3614fcb14` flipped
`net.userspace` on by default; `net.userspace=0` opts back out.

Validated on a boot that reached QEMU: `[spawn] Starting persistent userspace
netstack daemon (net.userspace on)` with **nothing on the kernel cmdline**, the
daemon claiming the NIC and registering `net.stack`, a real HTTP fetch over IPC
returning `HTTP/1.1 200 OK`, and 0 non-DRM self-test failures.

**The head-of-line removal has ONE witness, and it is the code change itself.**

Stated plainly because 932 requires it. What the passing boot proves is that
the daemon path still works end to end -- HTTP over IPC returned 200. It does
**not** prove that two accepted connections now progress independently, which
is the property the fix exists for. A boot with the fix reverted would look
exactly as green: every existing netstack self-test uses one connection at a
time, so none of them can tell the two states apart.

**Design, worked out 2026-09-13 so the next attempt is implementation rather
than rediscovery.** The whole API needed is already public in `net::socket`:
`create`, `bind_stream`, `listen`, `accept`, `connect`, `send`, `recv`, `close`.

1. `srv = create(AF_INET)`, `bind_stream(srv, PORT)`, `listen(srv, 2)`.
2. Connect and accept **twice** over loopback -- `a1`, `a2` -- so both accepted
   fds share the listener's session, which is the configuration that used to
   serialise (Q23 Option A).
3. `sched::spawn` a task that sets an `AtomicBool` and then does a *blocking*
   `recv(a1, ...)`. `a1` has no data, so under the old code it holds the shared
   session mutex indefinitely.
4. Main task waits for that flag, yields a few times so the spawned task is
   genuinely inside `recv`, then `send(c2, msg)` and a blocking `recv(a2, ...)`
   **with a deadline**.
5. `a2` returning the payload before the deadline is the witness. Missing the
   deadline is the failure.
6. Release the blocked reader by sending on `c1`, join, then close all handles.

**The step that makes or breaks it is 4.** If the main task reaches its read
before the spawned one is inside `recv`, there is no contention and the test
passes whether or not the fix is present -- it would be a test of nothing,
which is the exact failure this entry is about. The flag plus the yields are
not incidental; they are what makes the two states distinguishable.

Deliberately not written at 00:17 after a long session: a racy self-test in the
boot path is worse than no self-test, because it costs three lanes a rerun each
time it flaps and it teaches everyone to ignore a red boot.

**Scoping note, so the next attempt does not start in the wrong place.** The
obvious base is `netstack_client::self_test_listen_accept`, which already does
listen -> connect -> accept -> echo over loopback. It is the wrong base: it
drives `NetstackConn` directly, and the head-of-line block is not there. The
lock is `socket.rs`'s `with_stream_conn` over `SessionRef::Shared`, which only
the fd layer (`net::socket::{create,listen,accept,recv}`) goes through. A test
built on `NetstackConn` would bypass the mutex it exists to exercise and pass
whether or not the fix is present -- the same trap as a marker that names a
neighbour's banner.

It also needs `sched::spawn` for the second task, and a **deadline** on the
second read: under the old code that read never returns, and a self-test that
hangs the boot converts a clear regression into a 2400 s timeout with no
diagnosis. Fail on the deadline, do not wait on it.

What a real witness needs: a listener, two accepted connections, one of them
with no data pending, and an assertion that a read on the *other* completes.
That needs two tasks -- sequentially there is nothing to observe, because a
blocking read on an idle connection has nobody to be blocking. The existing
`self_test_listen_accept` is the shape to build it from, and it is a real piece
of work rather than an addition to that test.

**This belongs in option D's trigger.** Deleting the resident stack on the
strength of a property nothing checks would leave no way to notice if the fix
ever regressed and no fallback when it did. Boots accumulating daemon-default
mileage is necessary; a concurrency witness is what makes the deletion safe.

**What is left is option D: delete the resident stack (~40 files).** It is not
done here on purpose. design-decisions 934 sets the bar as the replacement
having *run* as the default, not merely being it, and at the time of writing it
has done so once. D is also the only step with no way back but a revert.

**The trigger, so this is actionable rather than remembered:** once a handful of
boots across the lanes have run daemon-default with no netstack regression, do
the deletion. Evidence accumulates without anyone arranging it -- every lane's
boot now exercises the daemon. When D lands, `userspace_enabled()` and the
`net.userspace` parameter go with it: there will be nothing to fall back to, and
an opt-out that selects a deleted stack is worse than no opt-out.

**PLAN (2026-09-12, after A-Q9 resolved C-then-D): the fix is smaller than the
phrase 'asynchronous rewrite' suggests, because the daemon is already fair.**
`ring_tcp_recv` routes through `ring_pump` precisely so *concurrent connections
on the same ring can all receive without starving one another*
(D-NETSTACK-RX-DEMUX). Nothing daemon-side serialises. The blocking is entirely
kernel-side, with one cause: `socket.rs::with_stream_conn` takes
`s.session.lock()` and holds it for the whole closure, and a blocking `recv`
passes `aux = 0`, so `submit_and_reap` does a round-trip the daemon does not
answer until data arrives. The session mutex is held across a *network* wait.

**The fix: never ask the daemon to block.** Always submit `RECV_NONBLOCK` at the
ring level; on `ERR_WOULD_BLOCK` drop the lock and wait above it, then retry.
The lock is then held only for a round-trip the daemon answers immediately. No
daemon-ABI change, no change to the one-SQE-per-round model, Q23 Option A's
shared session intact.

The real work is where the waiting goes (`recv`'s caller, not `recv_on`), how it
meets the existing poll/epoll readiness path, and not turning a blocking read
into a spin -- looping on `WOULD_BLOCK` without yielding would remove the
head-of-line block by burning a core instead.

head-of-line blocking is the last thing standing between here and the 5.7
default flip, and removing it is an asynchronous rewrite of the session
layer, not a patch. It is why `open-questions.md` A-Q9 recommends fixing
this *before* the flip rather than after; **(2) UDP `SOCK_DGRAM`**
— now **complete end-to-end**: the daemon datagram-socket layer, ring ABI, kernel
client, *and* the AF_INET socket-fd wiring are all landed. A userspace
`socket(AF_INET, SOCK_DGRAM)` is a real daemon-backed UDP socket:
`bind`/`sendto`/`recvfrom`/`getsockname` and `poll`/`epoll` route to
`net::socket::create_dgram`/`dgram_bind`/`dgram_send_to`/`dgram_recv_from` over
`OP_UDP_BIND`/`OP_UDP_SEND`/`OP_UDP_RECV`, boot-validated by `self_test_udp_dns`.
**AF_INET6 UDP datagrams now work too** (`OP_UDP_SEND6` + v6-aware `OP_UDP_RECV`,
`self_test_udp6_loopback`). **UDP `connect()` default-peer now works too** (v4 and
v6): `connect(2)` on a `SOCK_DGRAM` fd records a default peer (no handshake,
auto-binds a source port) via `net::socket::dgram_connect`, so `send`/`write` (no
explicit destination) target it (`dgram_send_connected`; `EDESTADDRREQ` when
unconnected) and `recv`/`read` filter incoming datagrams to that peer (Linux drops
non-peer datagrams on a connected UDP socket); `getpeername` reports it, and
`connect(AF_UNSPEC)` dissolves it. Boot-validated by `self_test_udp_connect` (both
filter-pass and filter-drop). The last big gap is **(3) send
pipelining** — the daemon's single-outstanding-segment sender is a *deliberate*
minimal-TCP design, so multi-segment/windowed send is a design change with
tradeoffs, not a bug fix.

Known limitations, all deliberate for the 5.5 increment and to be closed as
Phase 5 progresses:

- **`O_NONBLOCK` receive and connect are now honoured (5.6+); send still blocks.**
  The read side no longer stalls: `recvfrom`/`read` on a socket with `O_NONBLOCK`
  set pass the new `netipc::ring::RECV_NONBLOCK` flag to the daemon, which drains
  already-arrived frames once and returns `ERR_WOULD_BLOCK` (→ `KernelError::
  WouldBlock` → `EAGAIN`) rather than polling for the full receive deadline
  (`dispatch_socket_read` → `net::socket::recv(_, nonblock)` →
  `NetstackConn::recv(_, nonblock)`; daemon `ring_tcp_recv`). **Non-blocking
  connect is now implemented (5.6+):** `connect` on an `O_NONBLOCK` socket passes
  `netipc::ring::CONNECT_NONBLOCK`; the daemon transmits the SYN and returns
  `ERR_IN_PROGRESS` (→ `EINPROGRESS`) without waiting, holding the connection in
  `SYN_SENT`. It completes the handshake in the background (the SYN-ACK is
  processed by `ingest_seg` via the RX pump on a later `OP_POLL`/`OP_RECV`, and
  `poll_connect` retransmits a lost SYN up to `TCP_SYN_ATTEMPTS`). The socket
  enters `SockState::Connecting`; a `poll(POLLOUT)` re-probes via `OP_POLL`, whose
  new `POLL_ERR`/`POLL_WRITABLE` bits drive the transition to `Connected`
  (writable) or `Failed` (writable+`POLLERR`). `getsockopt(SOL_SOCKET, SO_ERROR)`
  then reports `0` (success) or `ECONNREFUSED` (`net::socket::take_so_error`,
  one-shot). A repeated non-blocking `connect` while pending returns `EALREADY`;
  on an established socket, `EISCONN`. Boot-validated: `netstack_client::
  self_test_nonblock_connect` runs the EINPROGRESS→POLLOUT sequence over the live
  daemon. A send/recv on a still-connecting socket is rejected (`ENOTCONN`)
  rather than buffered.
- **Non-blocking `send` is honoured (5.6+).** A `send(2)`/`write(2)` on an
  `O_NONBLOCK` socket whose send window is full (the single outstanding segment is
  still unacknowledged) returns `EAGAIN` instead of blocking on the daemon; on a
  window with room it accepts the bytes and returns the count, exactly like Linux.
  Path: `dispatch_socket_write` reads the fd's `O_NONBLOCK` → `net::socket::send(_,
  nonblock)` → `NetstackConn::send(_, nonblock)` (which ORs `netipc::ring::
  SEND_NONBLOCK`); the daemon (`ring_tcp_send`) drains pending ACKs once and, if the
  window is still full, returns `ERR_WOULD_BLOCK`. The window is re-opened by
  `ingest_seg` when the peer's cumulative ACK reaches `snd_nxt` (a single
  outstanding segment). `poll(POLLOUT)` is likewise honest now: `ring_tcp_poll`
  reports `POLL_WRITABLE` only when the window has room. Boot-validated:
  `netstack_client::self_test_nonblock_send`. **Still single-outstanding-segment:**
  only one unacknowledged segment may be in flight; a full window blocks/EAGAINs
  until it is ACKed (no send pipelining yet).
- **`poll`/`epoll` read readiness is now honest (5.6+).** A connected socket's
  `POLLIN`/`POLLOUT` are computed from a real non-destructive probe rather than
  the old "always-ready" placeholder: `poll_revents_from_entry`'s `Socket` arm
  calls `net::socket::poll_ready`, which issues a new `netipc::ring::OP_POLL`
  round-trip; the daemon (`ring_tcp_poll`) drains arrived frames once and reports
  a `POLL_READABLE`/`POLL_WRITABLE` bitmask **without consuming any buffered
  bytes** (a later `recv` still returns them). So `POLLIN` is set only when there
  are buffered bytes or the peer has closed — a poller sleeping on an idle socket
  no longer wakes on a false `POLLIN` and then reads `EAGAIN`. Boot-validated:
  the persistent-daemon parity check polls an idle connected socket
  (`readable=false writable=true`) and then confirms it flips to readable once
  the HTTP response arrives (`netstack_client::self_test_poll_ready`). **Caveat:**
  each poll of a socket fd costs one daemon control round-trip (the poll engine
  re-probes per ~10 ms slice); acceptable for parity, but the async-completion
  socket server (below) will replace it with an edge-triggered readiness signal.
  Proper fix for the remaining gap: async ring completions + a real readiness
  signal (eventfd/completion port) when the always-on socket server lands.
- **Server sockets: now wired end-to-end (Q23 Option A).** The passive side is
  complete through the socket-fd layer. `net::socket` grew `SockState::Listening`,
  `bind_stream`/`listen`/`accept`/`is_listening`/`stream_local_port`, and a shared
  ring session (`SessionRef::Owned`→`Shared`): a listening socket keeps its daemon
  session behind an `Arc<Mutex<NetstackConn>>` addressed under `LISTENER_ID` (100),
  and each `accept` installs a **new** socket that shares that same session/ring
  under its own `conn_id` (`ACCEPT_ID_BASE` = 101+), so no daemon-ABI change is
  needed (Q23 Option A). The Linux syscall layer routes `bind(2)` on a stream fd →
  `bind_stream` (`socket_stream_bind_from_user`, records the port kernel-side for
  the later `OP_LISTEN`), `listen(2)` → `net::socket::listen` (`OP_LISTEN` +
  Owned→Shared conversion), `accept(2)`/`accept4(2)` → `net::socket::accept`
  (`socket_accept_from_user`: installs the accepted socket as a new fd, honours
  `accept4`'s `SOCK_CLOEXEC`/`SOCK_NONBLOCK`, writes the peer `sockaddr_in`/`_in6`
  when the addr ptr is non-NULL, and maps an empty backlog → `EAGAIN`; a *blocking*
  listener retries via `yield_now` since the object-layer `accept` is single-shot),
  and `getsockname(2)` on a listener reports `0.0.0.0`/`[::]`:`port` from
  `stream_local_port`. Accepted-connection teardown sends a per-`conn_id` `OP_CLOSE`
  (`NetstackConn::close_conn` via `SharedConn::Drop`) so a server that accepts+closes
  many connections doesn't leak daemon-side state; the listener's session `OP_STOP`
  still fires on the final `Arc` drop. Boot-validated: the ring-level data path by
  `netstack_client::self_test_listen_accept` and the object-layer state machine
  (bind→listen→accept, port reporting, idempotent re-listen, empty-backlog EAGAIN,
  dgram/listening op rejection) by `net::socket::self_test_server`.
  **Interim concurrency limitation (deliberate, Q23 Option A):** because the
  listener and all its accepted connections share ONE SPSC daemon session serialized
  by a single `Arc<Mutex<>>`, concurrent I/O across accepted connections is
  serialized — one slow/blocked connection can head-of-line-block the others. This
  is acceptable for the interim synchronous-socket path; the future async socket
  server (per-connection completion rings) removes it.
- **UDP `SOCK_DGRAM`: complete end-to-end (daemon+ring+client+socket-fd wiring).**
  The daemon hosts a fixed table of bound connectionless datagram sockets
  (`UdpSock`/`UdpSocks` in `services/netstack/src/main.rs`) served by ring ops
  `OP_UDP_BIND` (ephemeral-port picking + `EADDRINUSE`), `OP_UDP_SEND`
  (`EMSGSIZE` on oversize), and `OP_UDP_RECV` (which prepends a 24-byte in-band
  source-address header — `Sqe::pack_udp_addr`, design-decision #68 — since the
  16-byte CQE has no room for a per-datagram address); `OP_POLL` reports a bound
  socket as always-writable and readable-when-queued; `OP_CLOSE` unbinds. The
  kernel client exposes `NetstackConn::udp_bind`/`udp_send_to`/`udp_recv_from`,
  boot-validated end-to-end by `netstack_client::self_test_udp_dns` (bind an
  ephemeral port, send a real DNS `A`-query to the resolver, read the reply back
  from port 53). The **AF_INET socket-fd layer is now wired**: `net::socket` grew a
  `SockKind::{Stream,Dgram}` transport tag and `create_dgram`/`dgram_bind`/
  `dgram_send_to`/`dgram_recv_from`/`dgram_local_port`/`is_dgram`; the Linux
  syscall layer routes `socket(AF_INET, SOCK_DGRAM)` → `create_dgram`,
  `bind` → `dgram_bind` (`socket_dgram_bind_from_user`), `sendto` →
  `dispatch_dgram_sendto` (destination `sockaddr_in`; `EDESTADDRREQ` on a NULL
  dest — no UDP `connect()` default-peer yet), `recvfrom` →
  `dispatch_dgram_recvfrom` (fills the real per-datagram source address, truncates
  a short buffer like Linux without `MSG_TRUNC`), `getsockname` → `dgram_local_port`
  (reports `0.0.0.0:port` — the daemon owns the interface IP and has no UDP
  `OP_LOCALADDR` yet), and `poll`/`epoll` → `poll_ready` (a bound dgram socket is
  writable + readable-when-queued; unbound is writable-only). Implicit ephemeral
  auto-bind on the first `sendto`/`recvfrom` matches Linux. The socket-gate
  self-tests (`socket EPROTONOSUPPORT gating`) are switch-aware for the
  `SOCK_DGRAM`/UDP case (ENOSYS off, daemon-backed fd on), and only protocol
  `0`/`IPPROTO_UDP` route to the daemon — `IPPROTO_UDPLITE=136` stays ENOSYS
  (unimplemented) rather than being silently aliased to plain UDP. Proven from
  ring-3 by the `services/udpget` capstone (`socket(SOCK_DGRAM)`/`bind`/`sendto`/
  `recvfrom` a DNS `A?` query, exit-code-decoded), spawned by
  `run_persistent_netstack`, boot-validated switch-on (`[udpget] start`→`query
  sent`→`OK: DNS reply`, `ring3 UDP capstone: OK … (exit 0)`). **Remaining
  follow-ups (not blockers):**
  - **IPv6 datagrams — DONE (all five layers landed).** `AF_INET6` UDP
    `SOCK_DGRAM` is now a real daemon-backed v6 datagram socket. **(1) netproto** —
    `udp::write_v6`/`Datagram::parse_v6` compute the RFC 8200 v6 pseudo-header
    checksum (mandatory for v6: `parse_v6` rejects a zero checksum). **(2) daemon**
    (`services/netstack/src/main.rs`) — `udp_sock_send6` frames via `send_ipv6` +
    `ipv4::PROTO_UDP`; `recv_udp_any` grew an `ETHERTYPE_IPV6` arm (parse
    `ipv6::Packet` → `udp::Datagram::parse_v6`); `UdpDatagram`/`UdpSock::push`/`pop`
    now carry a `family` tag + fixed 16-byte source address (one queue serves both
    families), and `OP_UDP_RECV` packs the stored family into the in-band header.
    New opcode `OP_UDP_SEND6` (netipc `ring.rs`, `0x0F`) carries the 16-byte v6
    destination at the front of the data window (`[dst_ip16:16][payload…]`), port
    in `aux`. **(3) client** (`netstack_client.rs`) — `udp_send_to6(&[u8;16],port,
    buf)` and the family-aware `udp_recv_any(&mut buf,nonblock) -> (i32,u16 family,
    [u8;16],u16)` (the old `udp_recv_from` is now a v4-truncating wrapper).
    **(4) socket** (`net::socket`) — `dgram_send_to6`; `dgram_recv_from` returns the
    family + 16-byte address. **(5) linux.rs** — `socket_dgram_bind_from_user`,
    `dispatch_dgram_sendto`, and `dispatch_dgram_recvfrom` dispatch on `sa_family`
    (2→`sockaddr_in`/v4, 10→`sockaddr_in6`/v6); `recvfrom` writes the sockaddr
    matching the *datagram's* family (via `socket_write_peer_addr6` for a v6
    source), since a datagram's family is a packet property, not the socket's.
    Boot-validated by `netstack_client::self_test_udp6_loopback` (bind a fixed
    port, send to the daemon's own link-local `me.ip6`, and confirm the looped-back
    datagram reports `AF_INET6` + the link-local source + the sent payload), and
    **now also from a real ring-3 process** by the `udpget` v6 arm (a bare
    Linux-ABI ELF: `socket(AF_INET6,SOCK_DGRAM)`/`bind([::]:port)`/`sendto([me.ip6]:
    port)`/`recvfrom`, exit-code decoded). The kernel derives `me.ip6` from the NIC
    MAC and passes it as a 32-hex argv + a `"6"` mode flag; `run_ring3_udp6_capstone`
    spawns it and boot-validated switch-on with **exit 0** (`[udpget] OK: v6 loopback
    echo`, `ring3 UDP6 capstone: OK … (exit 0)`) — so the ring-3 `sockaddr_in6`
    sendto/recvfrom dispatch path (user-copy, family parse, fd install, errno map)
    is now exercised live, not just by the kernel-context self-test. With this, all
    documented UDP daemon-socket follow-ups are closed.
  - **UDP `connect()` default-peer — DONE (v4 and v6).** `connect(2)` on a
    `SOCK_DGRAM` fd records a default peer without a handshake (auto-binding a
    source port). `net::socket` grew a `DgramPeer::{V4,V6}` + `SocketInner.
    dgram_peer` field and `dgram_connect`/`dgram_disconnect`/`dgram_peer`/
    `dgram_send_connected`; `dgram_recv_from` filters to the connected peer
    (discarding non-peer datagrams, which Linux drops at input on a connected UDP
    socket). The Linux syscall layer routes `connect(SOCK_DGRAM)` →
    `dgram_connect_from_user` (`AF_UNSPEC` dissolves; 2→v4, 10→v6, else
    `EAFNOSUPPORT`), `send`/`write`/`sendto(NULL dest)` →
    `dispatch_dgram_send_connected` (targets the peer; `EDESTADDRREQ` when
    unconnected), `sendto` *with* an address still sends there (Linux uses the
    supplied address even on a connected UDP socket), `read` routes through the
    filtered receive, and `getpeername` reports the connected peer (`ENOTCONN`
    otherwise). Boot-validated by `netstack_client::self_test_udp_connect` (a
    connected send loops back and passes the peer filter; `getpeername` matches;
    a datagram injected from a *non-peer* port is dropped by the connected
    receive). **Still not done:** UDP `getsockname` reporting the real interface IP
    (needs a UDP `OP_LOCALADDR`) — only a divergence for a socket bound to a
    specific local IP; our sockets bind `INADDR_ANY`, for which Linux's
    `getsockname` correctly reports `0.0.0.0`/`[::]`.
  - **UDP `getsockname` v6-family reporting — DONE.** `net::socket` now records the
    creation `domain` on every socket (`SocketInner.domain`, set by
    `create`/`create_dgram`/`create_kind`, exposed via `socket::domain(handle)`), so
    `sys_getsockname` on an `AF_INET6` datagram socket writes a `sockaddr_in6` with
    the unspecified address (`[::]:port`) instead of always emitting a `sockaddr_in`
    (`0.0.0.0:port`). A v4 datagram socket still reports `0.0.0.0:port`; a stream
    socket is unaffected (it reports the daemon-assigned local endpoint, which
    already carries the family). The domain narrows exactly from the syscall's
    `AF_INET`(2)/`AF_INET6`(10) gate.
  **Limitations:** the
  receive queue is 2 deep per socket and drops the oldest datagram on overflow (UDP
  is lossy); and it inherits the daemon's single-active-phase RX-demux limitation
  (`D-NETSTACK-RX-DEMUX`) — the `udp_pump` drops interleaved TCP frames while
  draining, same as the TCP pump.
- **`recvfrom` source-address out-params now populated (parity fix).**
  `recvfrom`'s `src_addr`/`addrlen` (arg4/arg5) are filled with the connected
  peer's endpoint on a successful receive, matching Linux for a connected stream
  socket (`sys_recvfrom` → `socket_write_src_addr`, reusing the getpeername
  serialisers: `sockaddr_in` for AF_INET, `sockaddr_in6` for AF_INET6; short
  buffers truncate, `*addrlen` written back full). Only touched on a non-negative
  byte count so an EAGAIN leaves the buffer alone. **`MSG_DONTWAIT` (arg3) is now
  honoured** on `send`/`sendto`/`recv`/`recvfrom`: it forces a per-call
  non-blocking transfer (→ `EAGAIN` on a full send window / empty receive)
  regardless of the fd's `O_NONBLOCK`, via a `force_nonblock` arg threaded into
  `dispatch_socket_write`/`dispatch_socket_read`. `MSG_NOSIGNAL` is a no-op (we
  never raise `SIGPIPE`; a broken pipe returns `EPIPE`) -- in the Linux ABI. The
  native C library raises it since 2026-10-06 (design-decisions §1176); the
  Linux ABI's half is `requests/d-a-linux-programs-never-get-sigpipe.md`. Remaining gaps: other
  `MSG_*` flags (`MSG_OOB`, `MSG_TRUNC`) are still ignored. (For a **datagram**
  socket, `recvfrom` now reports the *real* per-datagram source address — see the
  `SOCK_DGRAM` bullet above — via `dispatch_dgram_recvfrom`; the connected-stream
  path below reports the fixed peer.)
  **`MSG_WAITALL` (arg3) is now honoured** on `recv`/`recvfrom`: on a blocking
  socket it loops (`socket_recv_waitall`, ≤4 KiB chunks) until the full request
  is read, terminating early only on EOF or error; under `O_NONBLOCK`/
  `MSG_DONTWAIT` it degrades to the single-shot receive, matching Linux.
  **`MSG_PEEK` (arg3) is now honoured** on `recv`/`recvfrom`/`recvmsg`: it
  threads a `peek` flag through `dispatch_socket_read` → `net::socket::recv` →
  `NetstackConn::recv` into the ring's new `RECV_PEEK` aux flag, and the daemon
  copies buffered bytes out via a non-consuming `peek_rx` (vs the consuming
  `take_rx`), so a subsequent receive returns the same data. `MSG_PEEK` is
  single-shot even when combined with `MSG_WAITALL` (a non-consuming loop would
  re-read forever).
- **`sendmsg`/`recvmsg` now served (parity fix).** The Linux-ABI `sendmsg(2)`/
  `recvmsg(2)` on a connected daemon-backed stream socket no longer terminate in
  EBADF: `socket_sendmsg` gathers the `msg_iov` scatter/gather list into one
  bounded (≤4 KiB, one segment) staging buffer and forwards it; `socket_recvmsg`
  does a single bounded receive and scatters it across the iovecs, fills
  `msg_name` with the peer (`sockaddr_in`/`sockaddr_in6`, via `peer_sockaddr`),
  and clears `msg_controllen`/`msg_flags`. `MSG_DONTWAIT` (the `flags` arg) is
  honoured on both and `MSG_PEEK` on `recvmsg`; `msg_control` (ancillary/cmsg)
  is ignored and `msg_iovlen >
  1024` → `EMSGSIZE`. (The *native*-ABI `SYS_SOCKETPAIR_*` sendmsg/recvmsg path
  is separate and unaffected.) Remaining gap: like the plain send/recv path, only
  one page / one outstanding segment moves per call (no gather beyond 4 KiB, no
  send pipelining).
- **IPv6 connect: daemon+ring+client+socket-fd DONE.** The daemon speaks full
  TCP-over-IPv6 with no state-machine duplication (`TcpConn.dst6: Option<[u8;16]>`
  dispatching `emit`/`recv_one_seg` to `send_tcp6`/`recv_tcp_seg6`; v6 framing,
  `find_by_tuple6`/`route_seg6` demux, `connect6`/`connect_start6`/`accept_syn6`
  constructors, IPv6-aware in-process loopback), the ring ABI has `OP_CONNECT6`,
  and `NetstackConn::connect6`/`accept6` drive it end-to-end — boot-validated by
  `netstack_client::self_test_connect6` (v6 handshake + bidirectional data over the
  `fe80::/64`+EUI-64 loopback, `IPv6 parity ok`). The **AF_INET6 socket-fd layer is
  now wired**: `sys_connect` on an `AF_INET6` sockaddr parses the 28-byte
  `sockaddr_in6` (`sin6_port` BE, `sin6_addr` 16 octets) and routes to
  `net::socket::connect6` → `NetstackConn::connect6` (`socket_connect_from_user`
  dispatches on `sa_family`: 2 → v4, 10 → v6, else `EAFNOSUPPORT`). `getpeername`
  on a v6-connected socket returns a `sockaddr_in6` (`socket_write_peer_addr6`,
  fed by `net::socket::peer6`/`SocketInner.peer_ip6`). **`getsockname` now works
  for daemon sockets (both v4 and v6)** via the new `OP_LOCALADDR` ring op: the
  daemon writes its own interface address + the connection's ephemeral
  `local_port` to the data window (`[ip:4][port_be:2]` v4 / `[ip6:16][port_be:2]`
  v6), surfaced through `NetstackConn::local_addr` → `net::socket::local` →
  `sys_getsockname` (Path-B branch; returns `sockaddr_in`/`sockaddr_in6`,
  `ENOTCONN` on an unconnected socket). Validated by `self_test_connect6` step 6
  (`v6 getsockname: local fe80::…:PORT ok`). Remaining v6 socket-fd gap: the
  server-socket-fd path (bind/listen/accept4) is the separate gap below.
- **`shutdown(2)` DONE (v4 and v6).** A new `OP_SHUTDOWN` ring op carries the
  Linux `how` (`SHUT_RD`/`WR`/`RDWR`) in `aux`; the daemon's `TcpConn` grew
  `write_shut`/`read_shut` flags — `SHUT_WR` emits our FIN exactly once (a later
  `close` sees `write_shut` and skips re-sending) and rejects subsequent `OP_SEND`
  with `ERR_BROKEN_PIPE`, `SHUT_RD` makes subsequent `OP_RECV` report EOF; poll
  bits stay honest (writable when write-shut since send won't block, readable when
  read-shut). Kernel side: `NetstackConn::shutdown` + `net::socket::shutdown`
  (gated on Connected) + `sys_shutdown` Path-B branch (validates `how` after the
  fd lookup, so a bogus fd still wins with EBADF; `KernelError::BrokenPipe`→EPIPE).
  Validated by `self_test_listen_accept` step 6
  (`shutdown(SHUT_WR)→EPIPE + shutdown(SHUT_RD)→EOF ok`).
- **`setsockopt`/`getsockopt` compat DONE (client options).** `sys_setsockopt`
  no longer blanket-returns EBADF for daemon-backed sockets: it accepts the
  options a typical TCP client (curl/wget/glibc) sets during setup —
  `SOL_SOCKET`: `SO_REUSEADDR`/`SO_REUSEPORT`/`SO_KEEPALIVE`/`SO_BROADCAST`/
  `SO_SNDBUF`/`SO_RCVBUF`/`SO_LINGER`, and `IPPROTO_TCP`: `TCP_NODELAY` — as
  no-op successes (the daemon has no per-socket tunables: fixed buffers, always
  sends each segment immediately). Unknown options return `ENOPROTOOPT` (not
  EBADF), so probes feature-detect cleanly. `sys_getsockopt` gained a matching
  read side beyond the existing `SO_ERROR`: `SO_TYPE`→`SOCK_STREAM`,
  `SO_RCVBUF`/`SO_SNDBUF`→65536, `TCP_NODELAY`→1, `SO_KEEPALIVE`/`SO_REUSEADDR`/
  `SO_REUSEPORT`/`SO_BROADCAST`→0, unknown→`ENOPROTOOPT`. Both Path-B branches
  gate on `userspace_enabled()` + a real Socket fd; kernel-context callers and
  non-socket fds keep the prior EBADF terminal. Not a strict 5.7 regression gate
  (the resident path also stubs these), but a real compat gap on the path to
  running unmodified Linux network programs (the HTTP-client capstone).
- **Capacity caps** inherited from `NetstackConn`: send chunked to ≤1024 B,
  recv ≤512 B per call (callers must loop). Not a correctness bug, but small.

Proper fix path: 5.6+ makes the daemon persistent/always-on and grows the ring
client to async multi-stream, at which point nonblock/poll/listen/IPv6 become
implementable. **Progress:** the persistent daemon landed (5.6); the `O_NONBLOCK`
*receive* path is honoured; honest `poll`/`epoll` read readiness landed via the
non-destructive `OP_POLL` peek; **non-blocking `connect` (EINPROGRESS →
`poll(POLLOUT)` → `getsockopt(SO_ERROR)`) now works**; **non-blocking `send`
(EAGAIN on a full send window, honest `POLLOUT`) now works**; and **listen/accept
server sockets now work at the daemon+ring layer** (ring `OP_LISTEN`/`OP_ACCEPT`,
passive-open TCP, in-daemon software loopback; validated by
`self_test_listen_accept`) (see the updated bullets above); **IPv6 connect is now
wired end-to-end through the socket-fd layer** (`sys_connect`/`getpeername` on
`AF_INET6` → `NetstackConn::connect6`). Remaining before the 5.7 default-flip:
route the AF_INET/AF_INET6 socket-fd server path's `bind`/`listen`/`accept` to the
daemon (`SockState::Listening` + `sys_bind`/`sys_listen`/`sys_accept4`) — the last
socket-fd gap. **Q23 is now RESOLVED (2026-07-18, §71 → Option A:** shared,
refcounted session; no daemon-ABI change — an accepted connection shares the
listener socket's ring/session under a distinct `conn_id`, exactly the model the
private `NetstackConn::send_on(conn_id,…)` already uses for the loopback
self-test), with the standing guideline **do not gold-plate this interim path**
(server sockets get Option A only; the concurrency limitation is documented and
temporary, since the whole per-op synchronous socket path is a stepping stone to
the async socket server that will replace the ring-per-op model wholesale). So
this server-path wiring is **unblocked** and is the active task, not operator-gated.
