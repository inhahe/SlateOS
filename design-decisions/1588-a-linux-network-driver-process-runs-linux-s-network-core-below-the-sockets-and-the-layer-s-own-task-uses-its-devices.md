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
3. **The layer's task uses the devices** (`lx-nic`): every device the
   driver registered is opened and listened to with an rx handler -- so
   received frames go to the task, nothing above the device being asked --
   and frames are sent with `dev_queue_xmit()`. This is where the bridge to
   the native network stack (§65's `netring`) goes. Until then its one job
   is the "arp" check the kernel's self-test runs: the link up within 10 s,
   then QEMU's user-network gateway (10.0.2.2) asked for its address from
   10.0.2.15, a request a second until it answers.
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
