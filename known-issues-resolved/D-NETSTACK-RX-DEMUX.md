### D-NETSTACK-RX-DEMUX. The netstack daemon had no shared RX demux — concurrent connections couldn't safely receive at once — FIXED 2026-07-14

**Where:** `services/netstack/src/main.rs`.

**Was:** the daemon's TCP receive path read one raw Ethernet frame directly
off the NIC (`raw_rx`) filtered to *one* connection's 4-tuple via
`recv_tcp_seg`, and dropped any frame that didn't match. With the Phase-5
`conn_id`-keyed `RingConns` table letting one ring hold several live
`TcpConn`s, two connections receiving simultaneously would have connection
A's receive loop pop and discard connection B's inbound frames — corrupting
B's stream. The safe envelope was "at most one connection receiving at a
time," which the `multi` self-test respected by fully sequencing conn7's
send/recv/close before conn9 sent.

**Fix:** introduced a **shared RX pump** (`ring_pump`) that drains *every*
pending NIC frame and routes each to its owning connection by 4-tuple
(`recv_tcp_any` returns the peer identity; `RingConns::find_by_tuple` locates
the owner), feeding each segment through a single shared TCP-receive core
`TcpConn::ingest_seg` that buffers in-order payload into a per-conn `rx_buf`,
advances `rcv_nxt`, generates the cumulative ACK, and honors FIN/RST. Each
`TcpConn` now has its own `rx_buf`/`rx_len`; `take_rx` drains it. `OP_RECV`
(`ring_tcp_recv`) polls `ring_pump` — so sibling connections' frames are
delivered to *them*, not dropped — then copies the target conn's buffered
bytes into the SQE data window. The single-connection `TcpConn::recv` (used
by the one-shot `tcp_fetch` control op) shares the same `ingest_seg`/`take_rx`
core. No duplicated TCP receive logic.

**Verified:** new ring-3 self-test `netstack_ring_tcp_demux_roundtrip`
(`kernel/src/proc/spawn.rs`) opens two connections and submits **both SENDs
before both RECVs**, so conn9's response frames arrive while the daemon is
blocked in conn7's RECV — the exact concurrency the old filtered read would
have broken. Boot test 2026-07-14: both connections returned
`HTTP/1.1 200 OK` concurrently (`ring-tcp-demux conn7`/`conn9 HTTP status =
HTTP/1.1 200 OK`), and the existing `multi`/`persist` tests still pass.
