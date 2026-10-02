### [A] A-BLOCKING-TCP-RECV-REPORTS-EOF-AFTER-2S: a blocking TCP receive on a quiet connection returned 0 -- "the peer closed" -- after two seconds, and a blocking send said EAGAIN -- 2026-09-26
**Status:** FIXED on lane-a 2026-09-26, awaiting a boot. Found reading the
daemon while fixing its UDP twin (`A-BLOCKING-UDP-RECV-DID-NOT-BLOCK`); no boot
had shown it.

**In short:** when a program waited for data on a network connection and none
arrived for two seconds, SlateOS told it the other side had hung up. The
connection was in fact still open, so a slow server, a keep-alive connection
idle between requests, or an interactive session all looked closed after two
quiet seconds. Sending had the same two-second limit: a program told to wait
until its data went out was told instead to try again. And for those two
seconds the network service answered no other program.

**Where.**
- `kernel/src/net/socket.rs` `recv` and `send` passed a blocking call to the
  daemon as a blocking `OP_RECV`/`OP_SEND`.
- `services/netstack/src/main.rs` `ring_tcp_recv` polled until data, the
  peer's FIN, or `TCP_DATA_ITERS` (400 x 5 ms = 2 s), then fell through to the
  copy-out and returned 0 bytes. `recv(2)` returned that 0, which means end
  of stream.
- `ring_tcp_send` waited up to the same 2 s for the window, then answered
  would-block, which became `EAGAIN` from a blocking socket.
- `recv`'s own doc said "`0` = peer closed / no data": the conflation was
  documented.

**Why it was not the UDP fix again.** The daemon's only TCP retransmission
(`maybe_retransmit`) lived inside those blocking loops. It resent the buffered
segment at the 40th idle 5 ms poll, at most three times, and only before any
reply had arrived, on a counter local to one call. A kernel that waited by
itself, asking without blocking, would never have reached it: a lost segment
would never be resent.

**The fix.**
1. `netproto::tcp_rtx`: the retransmission policy, host-tested. It follows
   RFC 6298's shape: 200 ms doubling, five resends, and a timeout declared at
   12.6 s.
2. The daemon runs each connection's timer whenever it serves the ring at all.
   `ring_pump` services every live connection, and `service_retransmit`
   replaces `maybe_retransmit`. (`ring_pump` is `pump` since A-Q15's
   increment 2, 2026-09-27: one pump for TCP and UDP over every session.)
3. A connection whose resends all went unanswered is `timed_out`: receive and
   send answer the new `netipc::ring::ERR_TIMED_OUT` (-110), and poll reports
   `POLL_ERR`.
4. A receive whose own wait expires answers `ERR_WOULD_BLOCK`, never 0: an
   `OP_RECV` result of 0 now means EOF only.
5. An ACK that also covers our FIN (`snd_nxt + 1`, since the FIN takes a
   sequence number `snd_nxt` does not count) clears the in-flight segment.
   Before, a FIN'd connection with data outstanding would have been resent into
   a timeout.
6. The kernel waits for both directions in `net::socket::wait_until`, the same
   loop the UDP fix introduced. A blocking receive returns data, 0 on a real
   FIN, `ETIMEDOUT` or `EINTR`. A blocking send writes everything, as Linux's
   does, returning the short count only on a signal or an error.
7. `SO_ERROR` is a pending errno, like Linux's `sk_err`: ECONNREFUSED for a
   failed connect (as before), and ETIMEDOUT once for a timed-out connection,
   consumed by whichever of a poll-then-`getsockopt` or a `recv`/`send` learns
   it first.

**Tests.**
- `netproto::tcp_rtx`: 8 host tests. The resends land at 0.2, 0.6, 1.4, 3.0
  and 6.2 s, and the connection gives up at 12.6 s. Also covered: an ACK stops
  the timer, a clock behind the send is no time, giving up is final, and
  asking twice in one instant resends once.
- `netipc`: `ERR_TIMED_OUT` is distinct from every other completion code,
  above all from 0.
- `net::socket::self_test_wait_until`, which drives the shared wait loop with
  synthetic answers.
- The three ring tests in `proc/spawn.rs` read any short `OP_RECV` result as
  "no data came back", so the would-block that replaced the 0 leaves them
  unchanged.

**The end-to-end TCP witness: added 2026-09-27.**
`net::socket::self_test_blocking_recv_waits_for_late_data` sets up a
listener, a client and the accepted connection. A task sends on the client
2.5 s later, while the test sits in a blocking `recv` on the accepted
connection. The `recv` must return exactly those bytes, no earlier than the
send.

It needed a listener and a client alive at once, which A-Q15's shared ring
(design A, 2026-09-27) made possible. The same change lets the head-of-line
witness run.

**Still not done:** loss injection for the retransmit. The daemon has no hook
for it.
