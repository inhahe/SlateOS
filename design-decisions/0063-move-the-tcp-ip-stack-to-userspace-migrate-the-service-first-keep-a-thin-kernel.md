## 63. Move the TCP/IP stack to userspace — migrate the *service* first, keep a thin kernel NIC shim (Path B), not full userspace drivers yet (Path A)

**Date:** 2026-07-14
**Decided by:** Claude (operator-approved scope). The operator selected the
"move the TCP/IP stack to a userspace service" initiative from the roadmap fork;
the specific migration *strategy* (Path B vs Path A) is Claude's call and is
recorded here as reviewable/reversible.

**Context.** `design.txt` is explicit: "don't put networking in the kernel." The
whole protocol stack (`kernel/src/net/*.rs`, ~50 files: Ethernet/ARP/IPv4/IPv6/
ICMP(v6)/UDP/TCP/DHCP(v6)/DNS/… plus app-protocol helpers) currently runs
kernel-resident, polling the NIC driver directly. `kernel/src/net/mod.rs` names
this a prototype "to be migrated to userspace once the driver framework supports
device access from user processes." The NIC boundary is already clean: drivers
(`virtio/net.rs`, `e1000.rs`) expose `send(&[u8])` / `recv() -> Option<Vec<u8>>`,
wrapped by `net::send_frame_ns` / `recv_frame`.

**The fork.**
- **Path A — full userspace driver.** Move the NIC *driver itself* to userspace,
  granting MMIO/DMA/IRQ via capabilities + IOMMU sandboxing. Purest microkernel
  end-state and the design's ultimate goal.
- **Path B — userspace *service*, thin kernel NIC shim.** Keep a minimal kernel
  NIC driver exposing only capability-gated raw-frame TX/RX + interface query;
  move the *entire protocol stack* into a userspace `netstack` daemon; socket
  syscalls become IPC to that daemon.

**Decision: Path B first.** Rationale:
- The roadmap item is literally "Move to userspace **service**," and `design.txt`
  treats IOMMU-sandboxed userspace *drivers* as a separate, *optional* speed
  feature ("for when the 5–15% speedup matters"), gated on IOMMU being present/
  enabled. Driver-to-userspace is its own later roadmap track, not a prerequisite
  for de-kernelizing the protocol stack.
- Almost all of `kernel/src/net/` is privilege-free protocol logic (parsers,
  state machines) that can move into a userspace crate largely as-is — the big
  win (kernel attack surface, "restart the network service") is captured by
  Path B alone.
- Path B is incrementally testable and reversible: the kernel-resident stack
  keeps working throughout; the daemon is built alongside and cut over only when
  it reaches parity. Path A requires userspace MMIO/DMA/IRQ/IOMMU plumbing before
  a single packet flows — far higher risk for the same protocol-stack payoff.

**Performance note (net is in the perf-critical table).** Path B adds one IPC
hop app↔netstack and one raw-frame syscall netstack↔kernel per batch. Mitigate
with io_uring-style batched raw-frame TX/RX (submit/complete many frames per
syscall) and shared-memory ring buffers for the socket data path, matching the
design's batching guidance. Measure against the current in-kernel numbers before
cutover; do not regress the perf targets.

**Phased plan** (tracked in `net-userspace-migration.md`):
1. Kernel raw-frame boundary: capability-gated `sys_net_raw_*` (open/tx/rx) +
   interface query. Additive; existing stack untouched.
2. `netstack` userspace crate skeleton: open raw iface, poll loop, prove ARP +
   ICMP echo end-to-end.
3. Port protocol layers into the daemon (IPv4/IPv6, UDP, TCP, DHCP, DNS, …),
   reusing the kernel modules' logic.
4. Socket syscalls → IPC to `netstack` (shared-mem data path).
5. Cut over; delete the kernel-resident stack; keep only the thin NIC shim.

**Alternatives.** Path A now (rejected: higher risk, no extra protocol-stack
payoff, driver-userspace is a separate optional track). Leave in kernel
(rejected: violates the design's core microkernel tenet).

**Where it lives.** New kernel raw-frame syscalls (`kernel/src/syscall/`,
`kernel/src/net/mod.rs` shim), new `netstack/` userspace crate, socket-syscall
IPC bridge. Migration plan + status in `net-userspace-migration.md`.
