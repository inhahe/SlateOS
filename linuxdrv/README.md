# linuxdrv -- Linux's own drivers, as SlateOS driver processes

Hardware with no public standard -- GPUs, WiFi, Bluetooth, laptop audio DSPs
-- runs Linux's own drivers, unchanged, each in a driver process of its own
whose device can reach only its own memory through the IOMMU
(design-decisions 1577, the operator's decision). This directory is the layer
that lets them run there. How it is built is design-decisions 1578; in one
paragraph:

The drivers are compiled against Linux's real headers, from an unmodified
tree of the release `linux.pin` names, configured by `linux.config`. Linux's
own generic code is compiled in wherever it can run as it is. Only the bottom
is written here: a C half that implements the Linux functions a driver calls
(`kmalloc`, `request_irq`, `schedule`, ...) over a small `slx_*` interface,
and a Rust half that implements `slx_*` on SlateOS (the `udriver` syscalls,
the scheduler of Linux's tasks, timers, IPC to the native service). Every
other symbol the link leaves undefined is a generated stop that names itself
when reached. Each driver process runs Linux's tasks cooperatively on one
thread, under a uniprocessor, non-preemptive configuration, so Linux's own
locking is correct as compiled.

## What is here

| Path | What |
|---|---|
| `linux.pin` | The Linux release, by version and SHA-256 |
| `linux.config` | The configuration, applied on top of `allnoconfig`; every line is checked to have taken |
| `shadow/` | Linux headers that assume kernel mode, replaced by their include guards (design-decisions 1580) |
| `emul/` | The Linux-facing half, C: Linux's functions over `slx_*` (`slx.h`) |
| `kit/` | The system-facing half, Rust: `slx_*` on SlateOS -- entry, memory, console, clock, arguments, tasks, PCI functions through their device handles, exit |
| `link.ld` | The driver process's layout: linked at 4 MiB, Linux's initcalls in order |
| `hello/` | The first driver process, which drives nothing: the layer's own test |

## Preparing the Linux tree

    bash scripts/linux-tree.sh prepare

fetches the pinned tarball from kernel.org, refuses it unless its hash
matches, unpacks it, and runs Linux's own `allnoconfig` and `make prepare` out
of tree with clang. From Git Bash on Windows the script runs itself in WSL:
the tree has file names that differ only in case, so it lives in WSL's
filesystem, at `~/slateos-build/linux` (`$SLATEOS_LINUX_ROOT`), with a
`README.txt` there. It needs make, clang and the LLVM tools, bc, perl, xz,
curl, flex, bison and m4.

    bash scripts/linux-tree.sh needs drivers/net/ethernet/intel/e1000e

builds one directory of Linux code with Linux's own build and lists what it
uses and does not define, grouped by the Linux file that exports each symbol:
the inventory a driver's port starts from.

## Building a driver process

    (cd linuxdrv/kit && cargo build --release)
    bash scripts/linux-tree.sh driver hello

builds the process `linuxdrv/hello/manifest` describes, on top of what
every driver process is built from (`linuxdrv/emul/manifest`: the layer's C
half and the Linux code it stands on), into
`build/linuxdrv/hello.elf`: Linux's own objects by Linux's make (with the
small code model and the shadow headers, design-decisions 1580), the layer's
C files with the same flags, a generated stop for every function nothing
defines, and the Rust half. The boot test builds every process it loads and
puts it beside the kernel as a Limine module (`module_cmdline:
linuxdrv/<name>`), where the kernel finds it (`boot::module`).

## Status

The layer's first process runs: `hello`, Linux's headers and its own
vsprintf in a process of their own, its initcalls run, and a generated stop
reached on purpose (`kernel/src/linuxdrv.rs`). Linux's tasks are coroutines
on the process's one thread (`kit/src/sched.rs`, `kit/src/task.rs`), under
Linux's own wait queues, completions and mutexes (`emul/lx_sched.c`
implements schedule() and try_to_wake_up() for them, sleeping, call_rcu(),
whose grace period is a switch, and the kernel_thread() and do_exit() that
Linux's own kthread.c and its kthreadd run on), each on a 64 KiB stack of
its own above a guard page; Linux's own timer wheel, softirqs and tasklets,
with the loop as their tick and ksoftirqd (`emul/lx_softirq.c`,
design-decisions 1581); Linux's own workqueues, over a slab and per-CPU
memory the layer keeps on kmalloc (design-decisions 1582); Linux's own
driver core -- devices, drivers, buses, devres, device links -- its sysfs
and uevents showing nothing (`emul/lx_fs.c`, design-decisions 1583); Linux's
own PCI core, given the functions the process was started with
(`pci=BB:DD.F`) through the kernel's device handle, their BARs shadowed and
mapped by `ioremap` (`emul/lx_pci.c`), and the heap laid out as Linux's
direct map (`emul/lx_mm.c`, design-decisions 1584); their MSI or MSI-X --
a vector per table entry -- delivered to the handlers drivers request,
each vector's taken at each turn of the loop and all waited for when it is
idle (`emul/lx_irq.c`, design-decisions 1585, 1587), or their legacy line
(INTx), `dev->irq` a number of the layer's own, each interrupt taken only
after its handler has run, as Linux unmasks a level interrupt
(design-decisions 1591); DMA from a window
beside the heap that the device's own memory backs, kmalloc serving from it
(`emul/lx_dma.c`, `kit/src/window.rs`, design-decisions 1586); a page
allocator, vmalloc and high-resolution timers (`emul/lx_mm.c`,
`emul/lx_hrtimer.c`); and for a network driver, Linux's own network core
below the sockets, the devices it registers opened and used by the layer's
own task (`emul/lx_nic.c`, design-decisions 1588); Linux's own firmware
loader, reading `firmware_class.path` and `/lib/firmware` through the
system's file calls with the process's own authority -- a `File`
capability to read with -- and Linux's own module parameters, set from the
process's arguments (`<module>.<parameter>=<value>`, each initcall level's
before that level runs; an unknown one, or a refused value, ending the
process), `emul/lx_fs.c`, `emul/lx_emul.c`, `kit/src/files.rs`,
design-decisions 1590. The firmware a driver loads comes from
linux-firmware at the release `firmware.pin` names, each file's sha256
pinned: `scripts/linuxfw.py fetch` takes them into `build/firmware/`, and
`scripts/pikvm.py stage` puts them on the boot medium as Limine modules
`firmware/<path>`, which the kernel places at `/.boot-firmware/<path>`,
read-only (design-decisions 1592). The driver's own
code is marked at build (`scripts/linux-tree.sh` renames its sections to
`*.lxdriver`, which `link.ld` brackets), so that only the driver's failing
initcall ends the process, a Linux subsystem's being carried past. `hello`
runs a kthread against a mutex held across a sleep, tries the scheduler's
edges (timeouts, kthread_stop() out of interruptible waits, kthreads freed
when they end, busy waits moving `jiffies`, RCU callbacks, rcuwait), arms
timers, schedules tasklets, queues work -- a sleeper and a CPU hog with work
queued behind each -- probes a device of its own through the driver core,
and overflows a stack into its guard on purpose; given the boot test's
spare 82574, it probes the card through Linux's PCI core, reads its MAC
address through BAR0, handles the interrupts it has the card raise --
on its legacy line, then by MSI -- and
has it read a descriptor and a frame from the DMA window, and routes three
of its causes to three MSI-X vectors. Given a firmware directory and
parameters of its own, it reads files through the layer at each of
Linux's edge cases, and firmware through Linux's loader every way a
driver asks for it; given no capability to read files, it reads none.

The first real driver, Linux's own `e1000e` (`linuxdrv/e1000e/`, roadmap.md
2.1), binds the spare 82574 in QEMU -- it registers `eth0` and brings its
link up on three MSI-X vectors -- and serves the card to SlateOS's own
network stack (design-decisions 1589): with `serve=<name>` the card, once
its link is up, is the service
`nic.<name>`; the netstack daemon connects to it and is given a ring in
shared memory (`netipc::ring`, its region shared by `SYS_SHM_GRANT`), into
whose posted slots the rx handler copies every frame received, and from
which the layer's task sends every frame the daemon submits
(`emul/lx_nic.c`; the ring's side in `kit/src/nicport.rs`, what the daemon
submits checked in `kit/src/frames.rs`; the loop waits for the connection
with the device's interrupts, `kit/src/wait.rs`). The daemon is told the
device's carrier as the card's link, and the multicast addresses it lists
become the device's own (`dev_mc_add`). The kernel's self-test runs it
beside the daemon's `nic-check`, which takes an address by DHCP through the
card, resolves the gateway and has 400 full-size pings answered, 16 at a
time -- frames sent and received through Linux's driver, its interrupts,
its DMA and its receive ring's page fragments -- then finds the card
passing a multicast group exactly while it is listed, and, in the boot
test, the link following the card's cable as QEMU's monitor pulls it and
plugs it back. A warning
from Linux code anywhere in that driver process fails the run
(`SLX_EXIT_WARNED`). On real hardware the same run is made on the card
`linuxdrv.card=BB:DD.F` names -- the test PC's I219-V at `00:1f.6`, staged
by `scripts/pikvm.py stage` -- `nic-check` told the network is a real one
(`lan`: the multicast check skipped, a few pings allowed lost).

The first WiFi driver, Linux's own `mt7925e` (`linuxdrv/mt7925e/`), drives
the test PC's MediaTek RZ717 on top of Linux's own 802.11 stack -- mac80211
and cfg80211, with nl80211 -- compiled into its process. The layer is
generic netlink's one user there (`emul/lx_netlink.c`, design-decisions
1593): it asks nl80211 as wpa_supplicant would, its requests parsed by each
command's policy and run between the family's `pre_doit` and `post_doit`,
and it is told what the groups it listens to are told. With "scan", the
layer's task (`emul/lx_wifi.c`) brings the card's interface up, has nl80211
scan passively and says each network found. The kernel's self-test runs it
on the card `linuxdrv.wifi=BB:DD.F` names (`06:00.0` on the test PC, its
firmware staged with it); in QEMU, which has no such card, it runs with none,
which must still start the 802.11 stack and exit 0. And in QEMU the stack
is tested end to end on Linux's own `mac80211_hwsim` (`linuxdrv/hwsim/`):
two simulated radios on one medium, the first made an open access point
through nl80211 (`SET_INTERFACE`, `START_AP`), the second's scan having to
find it -- its name, address and frequency.

## Licences

Linux's files, and anything generated from them -- prepared headers,
generated stops -- are GPL-2.0, and none is committed: they are made at
build time, outside the repository. The
layer's own C and Rust halves are MIT, which may be linked into a GPL
program. Each driver *executable* is therefore GPL-2.0 as a whole; the kernel
and the native services stay MIT and reach a driver only by IPC.
