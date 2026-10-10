### A-NETSTACK-CLOSE-BLOCKED-THE-DAEMON-AND-ATE-OTHERS-FRAMES -- 2026-09-27 -- FIXED (lane A)

**In short:** closing a network connection made the network service stop
serving everyone else while it waited to finish the close, and while it
waited it read other connections' incoming data off the wire and threw it
away. Boot rq15's check that a waiting read gets data that arrives late failed
this way in design B: the read waited 12 s for data sent at 2.5 s.

**Where:** `services/netstack/src/main.rs`, `TcpConn::close`. It sent FIN, then
polled up to 40 times for the peer's FIN, sleeping 5 ms whenever nothing came,
inside the request that asked for the close. The daemon serves one request at
a time, so every close held up every socket for at least 200 ms, far longer
under emulation. Its reads were `recv_one_seg`, the per-connection kind, which
drops a frame that is not this connection's: the loopback queue is read first,
so a close ate its siblings' data. Design B closes a whole session per socket
(`OP_STOP` → `close_session`), so the witness before the late-data one left a
run of closes behind it.

**The evidence, and its limit.** rq15's late-data round timed out on the kernel
side at 12 s (`channel::recv_timeout`) -- the daemon had not even accepted it
-- and its completion arrived later and was discarded
(`[netstack-client] discarded 1 late completion(s)`). Blocking closes are the
cause the code supports; no log said which request held the daemon. So the
daemon now says: any request that holds it for 1 s or more prints
`[netstack] slow request: opcode 0x.. held the daemon for N ms`.

**Fix:** a close no longer waits or reads. `TcpConn::begin_close` sends FIN and
returns; the connection moves to `Net::closing` (16 slots, 2 s linger, oldest
dropped when full), where the pump offers it the segments no live connection
claims, after live connections and before listeners, and it ACKs the peer's
FIN (`TcpConn::closing_seg`) and leaves. Every ring-path close goes through
`Closing::retire`.

**Left open:** the legacy one-shot `OP_TCP_FETCH` (`tcp_fetch`) still reads the
wire with the per-connection filtered read while it runs, so it can drop other
connections' frames. It is a blocking request by design, used only by the boot
self-test that fetches over slirp before any ring socket exists. Its close no
longer waits (there is no `Net` to linger in). The proper fix is to route
`tcp_fetch` through `Net` and the pump like the ring paths, or retire the
opcode.
