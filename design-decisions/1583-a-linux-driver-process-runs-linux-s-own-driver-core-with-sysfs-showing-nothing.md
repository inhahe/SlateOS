## 1583. A Linux driver process runs Linux's own driver core, with sysfs and uevents that show nothing

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); what of Linux's core it compiles is Claude's call, under §1578 · **Lane:** A

**In short:** Every Linux driver hangs off Linux's "driver core": the code
that knows which devices exist, which driver handles each, calls the
driver's start-up and shut-down functions, and frees what a driver
allocated for a device when the device goes. Rather than imitate it, each
driver process now runs Linux's own. That code also publishes every device
in a file system (sysfs, the `/sys` directory on Linux) and announces each
new one to user space (uevents); nothing in a driver process reads either,
so those calls succeed and keep nothing.

### The decision

1. **Linux's own driver core is in every driver process**
   (`linuxdrv/emul/manifest`): `drivers/base/{core,bus,dd,driver,class,
   devres,property,swnode}.c`, `lib/kobject.c` and `klist.c`, and what they
   stand on -- tiny SRCU and `irq_work` (device links read under SRCU),
   notifier chains, read-write semaphores, asynchronous probing, and the
   generic string and number-parsing helpers. Devres (`devm_*`), device
   links, deferred probing and the firmware loader's and component
   framework's later needs all sit on it; i915 and the Wi-Fi drivers lean on
   it heavily.
2. **Its set-up runs where Linux's does.** Before the first initcall the
   layer runs the core of Linux's `driver_init()` -- devices, buses and
   classes -- and makes the kernel's own kobject (`/sys/kernel`), as
   `ksysfs_init()` does. The rest of `driver_init()` registers firmware
   nodes, hypervisors, CPUs, memory blocks, NUMA nodes and the platform bus
   as devices, none of which a driver process has. `system_state` moves as
   `init/main.c` moves it.
3. **sysfs and uevents show nothing** (`linuxdrv/emul/lx_sysfs.c`): every
   call that would create, remove, rename, move or link a sysfs node
   succeeds and keeps nothing, so every object's node stays NULL, which
   Linux's kernfs takes safely; `kobject_uevent()` delivers to no one. What
   only a reader or writer of sysfs reaches -- an attribute removing itself
   from its own `store()`, a file's owner changed for a namespace -- stays a
   generated stop.
4. **`irq_work` runs at the loop's tick**, as Linux runs it on an
   architecture that cannot raise it sooner: in hardirq context with
   interrupts off at every turn, and the loop does not sleep while any is
   queued. `boot_cpu_data` says the CPU has no APIC, so Linux's own code
   takes that path.
5. **What `mm/util.c` offers drivers is written in the layer**: the string
   duplication functions (`kstrdup`, `kstrdup_const`, ...), Linux's code
   line for line. The rest of that file is the memory manager's -- mappings,
   overcommit, folios -- and would need the process to fake one.
6. **No bounce buffers**: `swiotlb_dev_init()` gives a device none. Every
   buffer a driver hands its device will lie in memory the device can reach
   (§1579's arenas), so no copy through a buffer it can reach is ever
   needed.

This supersedes §1582 rule 5: sysfs registration no longer fails, so the
workqueues' (`wq_sysfs_init`) now succeeds through Linux's own `bus.c`.

### The alternatives

- **The layer's own minimal device model** (a `struct device` filled in by
  hand, a driver matched and probed by a loop over its id table, as
  Genode's older ports did): less code compiled in, but devres, device
  links, deferred and asynchronous probing, bus notifiers and the probe and
  remove ordering would each be a reimplementation to keep in step with
  Linux -- the class of bug §1577 named as the layer's risk.
- **`CONFIG_SYSFS=n`**, which gives Linux's own empty sysfs: it needs
  `CONFIG_EXPERT`, which under `allnoconfig` also turns off `BUG`, `BLOCK`,
  `EPOLL` and dozens more -- a different configuration altogether.
- **Linux's own kernfs and sysfs**, compiled in: a whole in-memory file
  system nobody would ever read.

### What it costs

- The hello process grows from about 258 KB to about 407 KB, most of it the
  driver core.
- A driver that reads its own sysfs attributes back through the file system
  -- rare, and a sign of a user-space interface -- finds nothing there.

### Where it lands

`linuxdrv/emul/manifest`, `linuxdrv/emul/lx_sysfs.c` (sysfs, kernfs and
uevents; `kernel_kobj`; the block-device identities), `linuxdrv/emul/
lx_emul.c` (`driver_init()`'s core, `system_state`, `vprintk_emit` for
`dev_*()` messages, `kstrdup` and kin, `ksize` and
`kmalloc_size_roundup`, `swiotlb_dev_init`, `boot_cpu_data`),
`linuxdrv/emul/lx_softirq.c` (`irq_work` at the tick),
`linuxdrv/emul/lx_sched.c` (`synchronize_rcu`, `wakeme_after_rcu`),
`linuxdrv/link.ld` (the read-only data's bounds, for `kfree_const`), and the
hello driver's driver-core test.
