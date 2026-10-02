### D-NETSTACK-TCP-MINIMAL. Userspace `netstack` TCP client is minimal (slirp-only correctness) — DEBT 2026-07-14

**Where:** `services/netstack/src/main.rs` — `tcp_fetch` / `send_tcp` /
`recv_tcp_seg` (the `OP_TCP_FETCH` control op). Kernel exercises it via
`kernel/src/proc/spawn.rs::netstack_tcp_fetch_roundtrip`.

**What it is:** the Phase-4 one-shot TCP client implements just enough of
RFC 793 to be correct on the loss-free QEMU-slirp path: SYN/SYN-ACK/ACK
handshake, in-order data reception with cumulative ACKs, SYN + request-payload
retransmission (bounded), and a graceful FIN close. Deliberately **omitted**:

- **No out-of-order reassembly.** Out-of-order data segments are dropped and
  dup-ACKed to prompt a retransmit; a genuinely reordering path would stall.
- **No congestion / flow control.** Fixed advertised window (`TCP_WINDOW`),
  no cwnd/ssthresh, no RTT estimation — retransmit timers are fixed poll counts.
- **No outbound segmentation.** The request `payload` must fit a single segment
  (one MSS); larger requests are not split. Fine for the HTTP HEAD self-test.
- **Single fixed ephemeral port + fixed ISN** (`EPHEMERAL_PORT` / `isn`): only
  one connection at a time, and no ISN randomization (no security concern in the
  bounded self-test, but not production-grade).
- **Response capped at the control-path `MSG_CAP` (512 B).** Bodies beyond the
  cap are ACKed (to keep the peer moving) but discarded; only the first ~511
  bytes reach the caller.

**Proper fix:** these all go away with the **Phase-5 shared-memory data ring**
(io_uring-style zero-copy) and a real per-connection TCP state machine (proper
RTO, windowing, reassembly, multiple concurrent sockets, ISN randomization).
Tracked as part of the net-userspace migration; this control-path client is
intentionally the bounded-self-test stand-in until then. See
`net-userspace-migration.md` Phase 4/5 and `design-decisions.md` §64.
