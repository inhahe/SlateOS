## 66. netstack Phase 5 cutover — phased deletion (Q22a→C) + staged cutover behind a default-off switch (Q22b→ii)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended both chosen options; operator
approved both).

**Context.** §63 (Path B) settled that the TCP/IP stack moves to the userspace
`netstack` daemon behind the thin capability-gated kernel NIC shim. Phases 1–4
(NIC boundary, daemon skeleton, shared `netproto` parsers, IPC + zero-copy ring
socket ops) plus the Phase-5 daemon prerequisites (persistent ring session §5.2,
shared RX demux §5.3 — both boot-validated) are done. Phase 5's final cutover —
forwarding the POSIX/Linux socket syscalls to the daemon and **deleting the
~60 K-line `kernel/src/net/`** — had two forks with no obviously-correct answer
and irreversible cost, raised as Q22 in `open-questions.md`:

- **Q22a (deletion scope):** `kernel/src/net/` is ~48 modules. Only the L2–L4
  core (`ethernet, arp, ipv4/ipv6, icmp/icmpv6, tcp, udp, dns, dhcp, frag,
  interface, ndisc`) is what the daemon replaces; the rest are app-level protocol
  servers/clients (ssh, httpd, ftp, smtp, telnet, tftp, ntp, dhcpd, syslog, …)
  that happen to live in-kernel and depend on the in-kernel `tcp`/`udp` APIs.
- **Q22b (cutover mechanism):** given §64's *exclusive* NIC claim, a persistent
  daemon and the still-live kernel stack cannot both reach the uplink — there is
  no true concurrent dual-stack.

**Decision.**
- **Q22a → Option C (phased deletion).** Delete the L2–L4 core first (once the
  daemon proves parity and the forwarders are wired); re-home each app-protocol
  module to userspace in its own dedicated follow-up task, deleting it from
  `kernel/src/net/` as it lands. No single big-bang removal of app features.
- **Q22b → (ii) staged cutover.** Land a persistent daemon + a socket-forwarding
  path behind a **boot/config switch that defaults OFF**, keeping the in-kernel
  stack as the compiled fallback and the NIC owner. Prove parity in QEMU with the
  switch ON, **flip the default to the daemon**, then (only then) delete the
  L2–L4 core. The switch selects *which stack owns the NIC at boot* — not a
  concurrent dual-stack (which §64 forbids).

**Rationale.** Every step stays buildable and boot-testable, and no step is a
giant irreversible leap. The staged switch means the daemon path can be exercised
end-to-end in QEMU while the known-good kernel stack remains one boot-flag away,
so a regression is a flag flip, not a revert of a 60 K-line deletion. Phased
deletion avoids a large temporary feature regression (ssh/http/ftp/… servers
vanishing at once) and gives each app protocol a real userspace re-home rather
than a silent drop. Cost: more increments and a longer calendar span, plus a
transitional period where the kernel still hosts app protocols over a
daemon-provided socket API (added coupling) — accepted as the price of always
being able to build, boot, and bisect.

**Alternatives considered.**
- *Q22a Option A (L2–L4 only, keep app modules in-kernel as-is):* rejected —
  those modules call the in-kernel `tcp`/`udp` APIs being deleted, so they can't
  actually stay unchanged; not cleanly separable without rewiring them onto the
  daemon socket API anyway.
- *Q22a Option B (delete everything at once):* rejected — large, irreversible,
  temporary regression of every app protocol.
- *Q22b (i) big-bang (flip persistence + forwarding + deletion in one commit):*
  rejected — a huge, effectively untestable step; a regression would require
  reverting the deletion.

**Where it lives.**
- Daemon: `services/netstack/src/main.rs` (persistent serve loop, `RingSession`,
  `RingConns`, `TcpConn`, NIC-claim lifecycle in `main`).
- Kernel NIC shim: `kernel/src/net/raw.rs`, `SYS_NET_RAW_*`.
- Socket forwarders: `kernel/src/syscall/linux.rs`
  (`sys_socket`/`sys_connect`/`sys_sendto`/`sys_recvfrom`/`sys_bind`/`sys_listen`/
  `sys_accept`/…), which today dispatch into `kernel/src/net/{tcp,udp,…}` and
  will gain a switch-gated branch that forwards to `net.stack` instead.
- Persistent-spawn path: how init/the service manager launches the daemon at boot
  (today it is spawned only by the bounded kernel self-test in
  `kernel/src/proc/spawn.rs`).
- Deletion target (final step, phased): `kernel/src/net/` L2–L4 core, then each
  app module.

**How to reverse.** While the switch defaults OFF, reverting is a no-op (the
kernel stack is still the default). After the default flips but before deletion,
reverting is flipping the default back. After deletion, the L2–L4 core would have
to be restored from git history — which is precisely why deletion is the *last*
step, gated on proven QEMU parity.

**Tracking.** Increment plan in `net-userspace-migration.md`; roadmap line under
Phase 2 "Move to userspace — Path B".
