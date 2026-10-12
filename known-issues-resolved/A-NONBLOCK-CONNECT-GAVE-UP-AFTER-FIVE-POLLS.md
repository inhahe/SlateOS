### [A] A-NONBLOCK-CONNECT-GAVE-UP-AFTER-FIVE-POLLS: a non-blocking connect was refused if it was polled five times before the server answered -- 2026-09-27
**Status:** FIXED on lane-a 2026-09-27, awaiting a boot. Found chasing the
intermittent `non-blocking connect reported POLL_ERR against a good endpoint`
warning. It appeared in three of the last six retained boot logs, each time
against example.com, which the blocking tests just before had reached.

**In short:** a program can start a network connection without waiting, then
keep asking "connected yet?". The network daemon counted each question as a
failed attempt and gave up after five. So whether a connection to a working
server succeeded depended on how quickly the program asked, not on how long
it waited. A program that checked in a tight loop, or a server a little far
away, was refused.

**Where.** `services/netstack/src/main.rs` `TcpConn::poll_connect`. Every
`OP_POLL` on a connection still in SYN-SENT resent the SYN and counted it.
The fifth (`TCP_SYN_ATTEMPTS`) marked the connect failed, which reaches the
program as `POLLERR` and `SO_ERROR = ECONNREFUSED`. The kernel's `poll(2)` asks
once per poll slice, so a real program could hit this as well as the
self-test.

**Two more defects, found in the same code.**
- **Ports and ISNs repeated.** Each connection's local port and initial
  sequence number came from one 16-bit seed, which every new ring session
  restarted from the daemon's control-request count. Two sessions a multiple
  of 16 requests apart gave their first connections the same port and the
  same ISN: a byte-identical SYN for a connection that had just closed. A
  peer or NAT still holding that pair can answer it with the old connection's
  state.
- **Any reset refused a connect.** During the handshake, a reset was taken as
  a refusal whether or not it acknowledged our SYN. RFC 793 accepts it only
  if it does. Any other belongs to an earlier connection on the same port
  pair.

**The fix.**
- `poll_connect` resends the SYN on `netproto::tcp_rtx`'s clock (0.2 s,
  doubling). It fails the connect only when that timer gives up, at 12.6 s.
  The SYN-ACK clears the timer.
- Ports come from `netproto::tcp_ids::EphemeralPorts` (RFC 6056 Algorithm 3):
  one daemon-wide counter plus a keyed per-destination offset, skipping ports
  in use.
- ISNs come from `netproto::tcp_ids::isn` (RFC 6528): a 4 µs clock plus a
  keyed hash of the 4-tuple. Passive opens use the same.
- The key is 128 bits from `SYS_GETRANDOM` at start-up, hashed with
  `netproto::siphash` (SipHash-2-4).
- A reset in SYN-SENT refuses the connect only if it acknowledges our SYN, in
  both the blocking and the non-blocking handshakes.

**Tests.**
- `netproto` host tests:
  - SipHash-2-4 against the paper's vectors and against `core`'s `SipHasher`
    at every length;
  - the port selector walks the whole range for one peer before repeating,
    skips ports in use, and stops when every port is taken;
  - the ISN advances with the clock and differs by tuple and by key.
- `netstack_client::self_test_nonblock_connect` now polls sixteen times back
  to back, then on a clock, up to 15 s. The old daemon refused the connect
  inside that burst whenever the server's reply took longer than five polls.
