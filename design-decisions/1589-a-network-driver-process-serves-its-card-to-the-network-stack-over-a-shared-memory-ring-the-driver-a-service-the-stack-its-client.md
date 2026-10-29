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
   process registers `nic.<name>` (it holds a `Service` capability) and
   serves one client at a time. A stack connects by name, so cards are
   chosen by name -- by the stack, an administrator or a test.
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
   grant cannot pass to another process.
3. **The ring's roles**: the stack submits and owns the data area, cut
   into fixed slots of 2 KiB: `OP_SEND` names a slot holding a frame to
   send, `OP_RECV` posts an empty slot for a frame to be received into.
   The driver takes every submission in order and completes each with the
   same `user_data` (the slot): a send with its length or a negative
   error, a receive with the received frame's length. A frame arriving
   with no slot posted is dropped and counted, as a card drops one with no
   descriptor. The ring's index ordering is `netring`'s, written once and
   linked into both sides (§65).
4. **Doorbells**: after pushing, a side sends one byte on the connection;
   when idle, each waits on the connection beside its other handles. A
   doorbell the connection has no room for is dropped: the peer has one
   waiting already. The connection closing is the end of the attachment.
5. **The driver's side**: the ring logic is the kit's (Rust, linking
   `netring`: `nicport.rs`, with what the stack submits checked and sorted
   in `frames.rs`), and the layer's `lx_nic.c` calls it -- frames the card
   receives are copied into posted slots from its rx handler, whole (a VLAN
   tag the card took out put back), and the `lx-nic` task sends what the
   stack submits with `dev_queue_xmit()`. The connection is seen to at the
   end of every turn of the loop (`lx_emul_turn_hook`), and the loop's idle
   wait includes it. Nothing the stack submits is trusted: a window outside
   the data area, a frame without a header or longer than the card sends,
   or another operation, is refused at once with an errno. The card is
   served once its link is up; the driver's argument `serve=<name>` names
   it, and `once` ends the process when the first stack has gone.
6. **The stack's side** (`services/netstack`): a NIC is the kernel's (the
   raw-frame calls, as before) or a port; the daemon's frame calls go to
   whichever it has. With a port it learns its address by DHCP
   (`netproto::dhcp`), the kernel having configured nothing for that card.
   Half the slots are posted to receive into, half are for sending.

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
- No link state crosses the port yet: the card is served once its link is
  up, and a stack learns of the link going down only by frames ceasing. A
  message on the connection for link changes is the next step.
- A grant is all or nothing: a region shared is writable by both.

### How it is checked

The boot test's linuxdrv self-test runs the e1000e process serving the
spare 82574 (`serve=spare once`), and beside it the netstack daemon in its
`nic-check nic.spare` mode, which attaches to the card, is refused passing
on a region it was never granted (`SYS_SHM_GRANT`'s check), takes an
address by DHCP from QEMU's network, resolves the gateway by ARP and has
400 full-size pings answered, 16 at a time, payloads as sent; the daemon's
exit is the verdict, and the driver must end 0 once the daemon has gone --
not `SLX_EXIT_WARNED`, which it ends with when Linux code warned anywhere
in the process, a bug even in a run that otherwise passed. (The first
version sent every frame twice, through `net_xmit_eval()`'s double
evaluation of `dev_queue_xmit(skb)`; every check passed, and only Linux's
refcount warnings said so.)
The kit's submission checks and queues have host tests (`frames.rs`), as
do the attach message (`netipc::nicport`) and the ring's data window
(`netring`).
