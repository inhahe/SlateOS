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

## Status

The tree is fetched, verified and prepared; no driver runs yet. The first,
`e1000e` (roadmap.md 2.1), measured on 2026-10-27 against 6.18.59: 11 objects
use 148 symbols they do not define. About 15 are Linux's generic library code
(`lib/` string, bitmap, CRC, `vsprintf`, hexdump; `timecounter`), to be
compiled in as they are; about 25 are the socket-buffer and Ethernet helpers
(`net/core/skbuff.c`, `net/ethernet/eth.c`, ethtool's), Linux's own where their
dependencies allow; the rest -- PCI (21), DMA (8), interrupts (3), timers,
`jiffies` and sleeping (about 11), workqueues (6), memory (about 10),
mutexes, `printk` and the `dev_*` messages, and `net/core/dev.c`'s NAPI and
`net_device` side (18), which is the bridge to the native network stack --
are the bottom layer to write.

## Licences

Linux's files, and anything generated from them -- prepared headers,
generated stops -- are GPL-2.0, and none is committed: they are made at
build time, outside the repository. The
layer's own C and Rust halves are MIT, which may be linked into a GPL
program. Each driver *executable* is therefore GPL-2.0 as a whole; the kernel
and the native services stay MIT and reach a driver only by IPC.
