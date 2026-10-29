## 1589. A network driver process serves its card to the network stack over a shared-memory ring: the driver a service, the stack its client

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers in processes of their own (§1577) and the network stack as a daemon (§934); design-decisions §1578 names `netring` as how a NIC driver serves it, and the protocol's details are Claude's call · **Lane:** A

**In short:** A network card driven by a Linux driver lives in a process
of its own, and SlateOS's network stack -- the netstack daemon -- lives in
another. They meet as a service and its client: the driver process
publishes its card under a name, the stack connects to it, and frames
then pass between them through a region of memory both have mapped, with
a one-byte message on their connection as the doorbell. Nothing passes
through the kernel per frame.

### The decision

1. **The card is a named service** (`netipc::nicport`): the driver
   process registers `nic.<name>` and serves one client at a time. A stack
   connects by name, so cards are chosen by name -- by the stack, an
   administrator or a test. Both ends are vouched for by capabilities:
   - the driver holds a `Service` capability for `nic.<name>` alone (its
     resource id `ipc::service::key_id(name)`; `SYS_SERVICE_REGISTER` now
     takes a capability for one name as well as one for every name, id 0),
     so it can register its card's name and no other -- not `net.stack`;
   - a stack must hold the card's key (`Service`, `key_id(name)`, `READ`;
     design-decisions 1518): the driver asks `SYS_CHANNEL_PEER_HAS_KEY` of
     whoever connects and turns away a process without it, which would
     otherwise receive every frame on the card and send its own -- what
     the kernel's raw-frame calls keep behind `NetRaw`. A stack turned
     away is told why before the connection closes (item 7's
     `Message::Refused`: no key, or no ring could be made for it), so a
     refusal is told apart from a driver that died.
2. **Attaching**: on a connection, the driver makes the ring's region
   (`SYS_SHM_CREATE`) and lays out a `netipc::ring` in it, lets the stack
   map it (`SYS_SHM_GRANT`, below), and sends the attach message -- the
   card's MAC address and MTU, the ring's slot size and count, and the
   region's handle. The stack maps the region.

   `SYS_SHM_GRANT` (1190) is new: a process lets the process at the other
   end of a channel it holds use a region it may use itself -- the process
   the kernel recorded when that end connected or accepted, as
   `SYS_CHANNEL_PEER_CRED` reports it. Until now only the kernel could
   authorize a process for a region (D-SHM-MAP-NOCAP's resolution), so no
   two processes could share memory. Process ids are never reused, so a
   grant cannot pass to another process. The grantee gets a reference of
   its own, which its `SYS_SHM_CLOSE` or its exit gives back; and a close
   now gives back only a reference the caller holds (it dropped the
   creator's for any process merely authorized).
3. **The ring's roles**: the stack submits and owns the data area, cut
   into fixed slots of 2 KiB: `OP_SEND` names a slot holding a frame to
   send, `OP_RECV` posts an empty slot for a frame to be received into.
   The driver takes every submission in order and completes each with the
   same `user_data` (the slot): a send with its length or a negative
   error, a receive with the received frame's length. A frame arriving
   with no slot posted is dropped and counted, as a card drops one with no
   descriptor. The ring's index ordering is `netring`'s, written once and
   linked into both sides (§65).
4. **Doorbells**: a doorbell is one byte on the connection, and goes only
   to a side that sleeps. Each consumer about to sleep writes a nonzero
   number of its own, new for each sleep, into its queue's sleep word in
   the ring (`netipc::ring::OFF_SQ_SLEEP`, `OFF_CQ_SLEEP`, beside its head),
   fences, and looks at its queue once more; each producer, having pushed,
   fences, reads the word, and rings once for a number it has not rung for.
   One of the two sees the other's store, so no wakeup is lost, and a busy
   driver and stack exchange frames with no system call between them. The
   stack rings after frames to send, never after slots posted to receive
   into: the driver looks for those when a frame comes. When idle, each
   waits on the connection beside its other handles. A doorbell the
   connection has no room for is dropped: the peer has one waiting
   already. The connection closing is the end of the attachment.
5. **The driver's side**: the ring logic is the kit's (Rust, linking
   `netring`: `nicport.rs`, with what the stack submits checked and sorted
   in `frames.rs`), and the layer's `lx_nic.c` calls it -- frames the card
   receives are copied into posted slots from its rx handler, whole (a VLAN
   tag the card took out put back), and the `lx-nic` task sends what the
   stack submits with `dev_queue_xmit()`. The connection is seen to at the
   end of every turn of the loop (`lx_emul_turn_hook`), and the loop's idle
   wait includes it. Nothing the stack does is trusted: a window outside
   the data area, a frame without a header or longer than the card sends,
   a receive slot too short for the longest frame, or another operation,
   is refused at once with an errno; at most a queue's worth of
   submissions is taken a turn and 64 doorbells a look; and a ring whose
   indices the stack wrote past what it holds (`ring::is_broken`, which
   `netring`'s pops refuse) ends the attachment. The card is
   served once its link is up; the driver's argument `serve=<name>` names
   it, and `once` ends the process when the first stack has gone.
6. **The stack's side** (`services/netstack`): a NIC is the kernel's (the
   raw-frame calls, as before) or a port; the daemon's frame calls go to
   whichever it has. With a port it learns its address by DHCP
   (`netproto::dhcp`), the kernel having configured nothing for that card.
   Half the slots are posted to receive into, half are for sending.
7. **Refusals, the link and the multicast filter** (added the same day;
   the attach message's version 2): the connection carries messages
   besides the doorbell, each a channel message whose first byte is its
   kind (`netipc::nicport::Message`). A driver that will not serve a stack
   sends a refusal and its reason in place of the attach message, then
   closes the connection. The driver tells the stack the card's
   link -- Linux's carrier, read at every turn of the loop -- right after
   the attach message and whenever it changes. The stack gives the
   driver the multicast addresses the card must pass, 64 at most or
   "every one", each list replacing the last; the `lx-nic` task makes them
   the device's own (`dev_mc_del` for those leaving, then `dev_mc_add` for
   those coming, so one in both lists never leaves the card's filter, and
   `dev_set_allmulti`, under the RTNL) and the driver answers each, once in
   force or failed (a failure leaves the card passing none of them). One
   list at a time: the daemon keeps the latest it wants and sends it once
   the last is answered, so the card ends with the latest. A stack's groups
   go when it leaves, and the next stack is accepted only after that. The
   daemon reads the driver's messages at every wait on the card, frames
   queued or not. A message a side has no place for -- too long, unreadable, the
   other side's kind, a list before the last was answered -- is the peer
   breaking the protocol, and ends the attachment, as a broken ring does;
   an address in a list that is not a multicast address is such a message
   (a card listing another host's unicast address would hand the stack
   that host's frames). An answer or link the connection has no room for
   is sent at a later turn, not lost.

### The alternatives

- **The kernel in the middle** (the driver registers a virtual NIC with
  the kernel; the stack keeps its raw-frame calls): no change to the
  stack, but every frame crosses the kernel twice, copied each way --
  the opposite of moving the network out of the kernel (§934).
- **The stack as the server** (drivers connect to `net.stack` and offer
  their cards): one rendezvous point, but the stack would take any
  process's offer of a card; a card as a named service is chosen, and a
  driver is its device's server, as elsewhere.
- **A new frame-ring format**: tailored entries, but `netipc::ring`'s
  submission and completion already say "send this", "receive into this"
  and "done", and its ordering logic is audited once for both sides.
- **The region moved as a capability** (`SYS_CHANNEL_SEND_CAPS`, as this
  decision first said): a shared memory region is no capability-table
  entry -- its handle is an IPC handle checked against the region's list
  of authorized processes -- so a capability move cannot carry one without
  making regions capabilities throughout, every `SYS_SHM_*` call taking a
  capability handle. The grant uses the identity the kernel already
  records for a channel's ends, and adds one call.

### What it costs

- A frame is copied once on each side of the ring (into the slot, out of
  it) -- the price of the driver and the stack being separate processes.
- A slot holds at most 2 KiB: jumbo frames need bigger slots, a version
  bump of the attach message.
- One stack per card at a time.
- Frames, the link and the multicast list cross the port, and nothing
  else of a card's: no speed or duplex, no statistics, no offloads, no
  bringing the card down. The daemon learns the link but does nothing yet
  when it changes; its serving modes serve over a port only when named one
  (`serve-dns <service>`), nothing yet naming one on a running system
  (`known-issues/A-NETSTACK-SERVES-SOCKETS-ONLY-OVER-THE-KERNELS-NIC`).
  Over a port the daemon waits by the driver's doorbell, and between
  requests it waits on its clients and the card together
  (`SYS_WAIT_MULTIPLE`), routing what the card brings as it comes.
- A grant is all or nothing: a region shared is writable by both.

### How it is checked

The boot test's linuxdrv self-test runs the e1000e process serving the
spare 82574 (`serve=spare stacks=2`), and beside it the netstack daemon in its
`nic-check nic.spare` mode -- first without the card's key, when the
driver must refuse it for that reason (it then exits 15,
`netipc::nicport::check::TURNED_AWAY`, which no other failure to attach
gives: a connection closed without a refusal, or refused for want of a
ring, is exit 10), then holding it -- which attaches to the card, is
refused passing
on a region it was never granted (`SYS_SHM_GRANT`'s check), takes an
address by DHCP from QEMU's network, resolves the gateway by ARP and has
400 full-size pings answered, 16 at a time, payloads as sent. Then the
link must have been told up, and the card must pass router advertisements
to all nodes -- which QEMU's network sends whenever a router is solicited
-- exactly while the daemon lists the all-nodes group or asks for every
multicast frame: none before, one once listed, one once every frame is
asked for, none once the list is emptied, each list answered before it is
tried, and before the last count a ping's answer read as a barrier, so
that nothing the card took under the earlier lists is counted. Where the
harness can pull the card's cable (the boot test, with its monitor and a
Python for `screen.py`, says `linuxdrv.cable=hmp`; `--no-monitor` runs and
fast boots cannot), the daemon asks it to -- `set_link net3 off`, then
`on`, tried each second until QEMU takes it -- and the link must be told
down, then up, within 20 s each, and the gateway must answer ARP again.
Last, the daemon sends what only a driver sends, and must be let go within
a second. The daemon's exit is the verdict
(`netipc::nicport::check`). Then the daemon serves sockets over the card
(`serve-dns nic.spare`, the driver's second stack): it takes an address by
DHCP there and registers `net.stack`, the kernel asks it to resolve a name
as a socket client asks, and it must answer -- resolved, or unresolved
where the host has no upstream DNS -- and end 0 once idle. The driver must
end 0 once both daemons have gone --
not `SLX_EXIT_WARNED`, which it ends with when Linux code warned anywhere
in the process, a bug even in a run that otherwise passed. (The first
version sent every frame twice, through `net_xmit_eval()`'s double
evaluation of `dev_queue_xmit(skb)`; every check passed, and only Linux's
refcount warnings said so.)
The kit's submission checks and queues have host tests (`frames.rs`), as
do the attach message and the other messages (`netipc::nicport`) and the
ring's data window (`netring`).
