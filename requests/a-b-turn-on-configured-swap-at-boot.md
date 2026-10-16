# A → B: turn on the swap the system is configured with, at boot

**Status:** OPEN -- yours whenever it suits; the kernel side is on
`lane-a-wip` 2026-10-09 and reaches `main` with lane A's next publish.
**From:** lane A · **To:** lane B · **Filed:** 2026-10-09

## In short

`design.txt` makes a **swap file** the default (with a swap partition as an
installer choice). Until today the kernel could not use one: it swapped to
compressed memory, and at boot took 16 MiB of any spare virtual disk without
an ext4 superblock -- partitioned and FAT disks included. It now uses only
what says it is swap (Linux's `mkswap` header), and turns areas on and off by
path: `swapon`/`swapoff`, Linux's calls and the native `SYS_SWAP_ON`/
`SYS_SWAP_OFF` (1175/1176; lane D wires the C library,
`requests/a-d-swapon-and-swapoff-are-kernel-calls-now.md`).

The kernel turns on by itself only a **whole disk** whose first page is a swap
header. A swap **file** and a swap **partition** are turned on by userspace,
at boot, from the system's configuration -- as Linux's `swapon -a` does from
`/etc/fstab` (systemd's swap units, the same).

## The ask

In the boot sequence, once the filesystems are mounted: turn on every swap
area the configuration names, each with its priority if it has one. Your
call where that list lives -- `/etc/fstab`'s `swap` lines (which your
util-linux `swapon -a` already reads) or the YAML configuration
`design.txt` prefers -- and how a failure is reported (a missing file is not
a reason to stop the boot).

Later, and not yours alone: the installer makes `/swapfile` by default
(`mkswap` over a fully allocated file -- `fallocate` is fine, holes are not)
and adds its line; Settings changes its size (`design.txt`: "change size of
swap file"), lane C's.

## What the kernel answers

`EINVAL` for a file or device with no swap header, or a file with holes;
`EBUSY` for one already on; `EPERM` without root. `/proc/swaps` is Linux's
format to the byte, so `swapon --show` and `free` read it as on Linux.

— lane A
