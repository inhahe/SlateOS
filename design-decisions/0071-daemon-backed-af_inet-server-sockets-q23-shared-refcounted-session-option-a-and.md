## 71. Daemon-backed AF_INET **server** sockets (Q23) — **shared refcounted session (option A)**, and a standing "don't gold-plate interim netstack work" guideline

**Date:** 2026-07-18

**Decided by:** Operator (Claude recommended **A**; operator chose **A** and
added a guideline about interim/stop-gap work — see below).

**Context.** In the userspace netstack daemon, a session == one SHM ring; `OP_ACCEPT`
installs the newly-established connection into the *listener's own* session on the
*same* ring, so a listening socket and all its accepted connections physically
share one ring. Linux instead gives every accepted fd a fully independent socket.
This fork gates the final AF_INET/AF_INET6 server socket-fd wiring
(`sys_bind`/`sys_listen`/`sys_accept4`), which in turn is the last gate on
flipping `net.userspace` on by default.

**Decision.** **Option A — shared, refcounted session, no daemon-ABI change.**
The listening `SocketInner` owns the session; each accepted socket is a new fd
holding an `Arc` on the same session with its own conn_id. Per-connection `close`
sends `OP_CLOSE` for that conn_id; the session's `OP_STOP` fires only when the
last reference (listener or any accepted socket) drops — giving Linux-correct
*lifetime* semantics (closing the listener no longer kills already-accepted
connections). The known limitation — all connections under one listener funnel
through one ring/lock, so a *blocking* op on one accepted conn can stall others
until its deadline — is accepted as temporary (a non-issue for the
`accept`+`poll`+non-blocking-I/O server pattern).

**Rationale.** The whole per-op synchronous socket path is explicitly a stepping
stone to the async, always-on socket server, which will replace the ring-per-op
model wholesale. Paying for option B's daemon-ABI complexity (accept-into-a-fresh-
ring, migrating `TcpConn` between sessions) now, only to rework it at the async
cutover, is poor value. A fixes the correctness-critical *lifetime* semantics with
zero protocol change.

**Operator guideline recorded with this decision (applies beyond Q23).** The
operator questioned doing *any* stop-gap netstack work that the async migration
will replace, and picked A specifically because it is the **minimal** interim
step. Standing guidance going forward: **do not gold-plate interim/throwaway
netstack infrastructure.** For the server-socket path, that means A only — do not
invest in per-connection ring independence (option B) or other elaboration before
the async socket server; if genuine per-connection concurrency is ever needed
before that cutover, revisit. (Note: the *client* socket path already built —
connect/recv/send/poll, IPv6 — is interim-but-*used* real functionality, not
throwaway; the async migration replaces the ring-per-op transport mechanism, not
the syscall-level behavior. The part most at risk of rework, and therefore kept
minimal, is exactly this server-socket layer.)

**Alternatives considered.** **B (accept-into-a-fresh-ring, daemon-ABI change)** —
true per-connection independence/concurrency, but a costlier-to-reverse protocol
commitment that the async cutover would largely redo; rejected as poor value for
an interim layer. Deferring server sockets entirely until the async migration —
considered (the operator floated it) but A is cheap enough and unblocks the
`net.userspace` default-flip for server programs now.

**Where it lives.** `kernel/src/net/socket.rs` (`SockState`, `SocketInner`,
`SOCKET_TABLE`; a shared `Arc<Mutex<Session>>`), `kernel/src/net/netstack_client.rs`
(a `Session` abstraction hosting multiple conn_ids), `kernel/src/syscall/linux.rs`
(`sys_bind`/`sys_listen`/`sys_accept4` routing). Tracking: known-issues
D-NETSOCK-SYNC; `net-userspace-migration.md`; the 5.7 default-flip.

**How to reverse.** Switch to B by extending the accept ABI (SQE carries a ring
handle; daemon `OP_RING_TCP`-attaches it and migrates connection state between
session tables) — or skip straight to the async socket server, which supersedes
the whole question.
