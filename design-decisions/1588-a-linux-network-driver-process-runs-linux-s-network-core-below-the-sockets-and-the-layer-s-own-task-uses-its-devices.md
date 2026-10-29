## 1588. A Linux network driver process runs Linux's network core below the sockets, and the layer's own task uses its devices

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577) with e1000e first (roadmap 2.1); what of Linux's network code a driver process carries is Claude's call, under §1578 · **Lane:** A

**In short:** A Linux network-card driver does not talk to the wire on its
own: it hands packets to, and takes them from, Linux's network core --
the code that registers network devices, queues packets for them and
collects what they receive. A driver process now carries that core, as
Linux's own code, but not what sits above it in Linux -- sockets, TCP/IP,
netlink -- since SlateOS has its own network stack. The layer's own task
opens the devices the driver registers and is the only user of them in
the process; for now it proves the driver works by asking QEMU's network
for its gateway's address, and next it will connect the device to
SlateOS's network stack.

### The decision

1. **Linux's network core, compiled in, below the sockets**
   (`linuxdrv/e1000e/manifest`): `net/core`'s devices, NAPI and GRO,
   socket buffers, the flow dissector, XDP's queue records, link_watch,
   net-sysfs and the initial network namespace; Ethernet's headers; the
   default and multi-queue queueing disciplines with their counters;
   loopback. Not compiled: sockets, protocols, netlink, procfs.
2. **What the core calls of what is not there, the layer's**
   (`linuxdrv/emul/lx_nic.c`): start-up as `start_kernel()` and the
   socket layer would do it (the initial namespace, the socket buffer
   caches); the RTNL lock a mutex, as in Linux, its unlock running the
   deferred unregisters; netlink's notifications and handlers going
   nowhere; the socket destructors and static keys the core refers to;
   `full_name_hash`; the RSS key; ethtool's generic answers.
3. **The layer's task uses the devices** (`lx-nic`): the device served is
   opened and listened to with an rx handler -- so received frames go to
   the task, nothing above the device being asked -- and frames are sent
   with `dev_queue_xmit()`. What it does with them is §1589's bridge to the
   native network stack. (Until that bridge, its one job was a "check" of
   its own -- ARP and 400 pings to QEMU's gateway, built in C -- which the
   bridge's test replaced.)
4. **What the core needs below it, the layer's, for every driver**: a page
   allocator (blocks of pages from kmalloc's allocator, their page entries
   set as Linux's, compound pages through `mm/internal.h`'s own helpers,
   cleared when freed), so socket buffers' page fragments
   (`mm/page_frag_cache.c`, Linux's) work; vmalloc and kvmalloc from the
   heap; high-resolution timers run by the loop (`lx_hrtimer.c`); real and
   boot time.

### The alternatives

- **Write the network core's interface by hand** (`alloc_etherdev`,
  NAPI, `netif_receive_skb` as the layer's own): less code compiled in, but
  every subtlety of how a driver and the core hand packets over --
  GRO, NAPI's budget, the queue's stop and wake -- reimplemented, and
  re-checked at every release. Linux's own core is correct by construction
  (§1578 rule 2).
- **Compile sockets in too, and have the bridge use a raw socket**: the
  bridge would read and write like any program, but a driver process would
  carry Linux's socket layer for one user that is not a program.

### What it costs

- Each network driver process carries Linux's network core, some hundreds
  of kilobytes.
- Whatever the core reaches above it that the layer did not foresee stops
  the process (a generated stop), which names it, until it is written.

### Measured (2026-10-29)

The boot test times the netstack daemon's pings to QEMU's gateway through
each driver (`nic-bench`): Linux's e1000e in its process on the spare
82574, over the port of design-decisions 1589 (in `nic-check`), then the
kernel's own e1000 driver on the first 82574, which the daemon claims alone
by its MAC address while the kernel's stack keeps its virtio-net
(`SYS_NET_RAW_OPEN` by MAC). The same 82574 model, behind the same kind of
QEMU network; 1000-byte pings, 200 one at a time, then 400 in bursts of 16.
Each card is waited on as fast as it can be: the port by its doorbell, as
the daemon's serving loops wait on it; the kernel's card read again after
one yield -- its best case, for those loops sleep 5 ms after reading nothing
there. The figures go to the boot history as `nic_<card>_...` benchmark
lines. On QEMU TCG, Windows host, five boots:

| | round trip, median | 90th percentile | answered a second, 16 in flight |
|---|---|---|---|
| the kernel's e1000, read in a loop | 0.75-1.7 ms | 1.1-2.2 ms | 630-1120 |
| Linux's e1000e in a process | 3.4-4.9 ms | 13.7-14.3 ms | 1020-1035 |

(A version of this section from earlier the same day timed the kernel's
virtio-net under the e1000's name: the daemon's claim then took the
kernel's primary card, whichever it was.)

In bursts the two are alike, the driver process the steadier: the kernel's
card ranges from well below it to a little above. Why it varies so is not
known; it is not the daemon spinning on it, for read after a 1 ms sleep
instead of a yield, neither of its figures moved. One at a time, the round
trips differ by more than the process: the kernel's NIC is read without an
interrupt, while the driver process waits for the card's (MSI-X), and
QEMU's e1000e throttles every interrupt to one per 128 us at least,
whatever the driver writes (`E1000E_MIN_XITR`: a written 0 becomes 500
units), on timers that fire 7-16 ms late on this host. The driver's own
timing of each round trip (from handing a frame to Linux until the next
received frame is in the stack's slot, an experiment not kept) put most of
them at 8-16 ms: the interrupt waits for the late timer. Turning Linux's
moderation off (`set_coalesce`, `rx_coalesce_usecs` 0) changed nothing, as
the clamp says; the stack polling instead of sleeping changed nothing
either. The process's own cost -- a copy each way, two process switches a
frame -- is therefore not separable from the emulator's on this host; it is
to be measured on hardware, where an interval of 0 means no throttle (the
test PC's Intel I219-V is an e1000e part).
