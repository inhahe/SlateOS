### [A] A-BLOCKING-UDP-RECV-DID-NOT-BLOCK: a blocking `recvfrom` on a UDP socket failed EAGAIN unless its datagram was already there -- 2026-09-26
**Status:** FIXED on lane-a 2026-09-26, awaiting a boot.

**In short:** a program that waits for a UDP datagram -- a DNS lookup waiting
for its answer, for instance -- asks the kernel to block until one arrives.
On SlateOS it did not block: if the datagram had not already arrived at the
instant the program asked, the call failed at once with "try again". So every
blocking UDP receive was a race against the network, and on a busy machine it
lost often enough to show up as a flaky boot check (the `udpget` capstone's
IPv4 DNS arm failed one boot and its IPv6 loopback arm the next).

**Where.** `services/netstack/src/main.rs` (`OP_UDP_RECV`) answers would-block
for a blocking receive with nothing queued, and its comment said why: "the
kernel client polls. A blocking wait loop lands with the kernel-side UDP fd
wiring." It never landed. `kernel/src/net/netstack_client.rs` (`udp_recv_any`)
turns the answer into `WouldBlock`, and `kernel/src/net/socket.rs`
(`dgram_recv_from`) returned it with `?` -- while its own doc claimed "on a
blocking socket it waits for the next one". The netstack's own UDP self-test
polled in a bounded loop around the non-blocking receive, so the gap was known
to the test and invisible to every syscall caller.

**The fix.** `dgram_recv_from` waits, in `net::socket::wait_until` -- the
loop every blocking netstack socket call now shares (TCP's receive and send
joined it for `A-BLOCKING-TCP-RECV-REPORTS-EOF-AFTER-2S`). It asks without
blocking, releases the socket, sleeps with a backoff (1 ms doubling to 10 ms)
and asks again, until a datagram arrives or a deliverable signal ends the wait
(`Interrupted` -> `EINTR`). The daemon still never holds a receive open -- one
client's wait would stall every other client's requests -- and its comment now
says the kernel waits. The connected-peer filter drains a non-matching datagram
and asks again at once.

**Tests.** `net::socket::self_test_dgram_blocking_recv`, run in the persistent
netstack battery beside the udp6 check: a helper sends to the socket 50 ms
after the main task has blocked receiving on it; the receive must return that
datagram (before the fix: `WouldBlock` at once), and a non-blocking receive on
the drained socket must still be `WouldBlock`. And
`net::socket::self_test_wait_until`, the loop on synthetic answers: it asks
through would-blocks to the answer, never waits on a non-blocking call, returns
an EOF's `0` and an error at once, and backs off to 10 ms rather than doubling
away. Both counted: a failure reds the boot, as the four loopback checks do.

**Not done here.** `SO_RCVTIMEO` is not implemented for netstack sockets, so a
blocking receive waits until a datagram or a signal. The TCP receive path
("the daemon blocks (polls) up to its receive deadline") should be checked for
the same shape: what a blocking `recv` returns when the daemon's deadline
passes with nothing to read.
