## 1571. Compressed swap is `/dev/zram0`, half of memory at most 4 GiB

**Date:** 2026-10-15 · **Lane:** A · **Decided by:** Claude (autonomous) -- the spec asks for compressed swap ("highly recommended for desktop", design.txt's swap parameters); its size and its name are lane A's call, and easy to change.

**In short:** the part of swap that compresses pages and keeps them in memory
was 4 MiB -- too small to matter -- and was listed in `/proc/swaps` as `zram`,
a name `swapoff -a` cannot act on, so a shutdown script that runs it reported
a failure every time. It is now as big as half the machine's memory, at most
4 GiB, and appears as the device `/dev/zram0`, which `swapoff` turns off
(bringing its pages back into memory) and `swapon` turns back on.

**What was built.** `mm::swap::init` sizes the tier from memory and registers
`/dev/zram0` (`MemoryTierDevice`): read-only, its first page a `mkswap`
header for the tier's size and the rest zeros, so tools that list block
devices find it and util-linux's `swapon` reads a swap signature on it, as on
Linux. `mm::swapctl` routes `swapon`/`swapoff` of `/dev/zram0` to the tier
itself (`swap::enable_memory_tier`, `AreaTarget::Memory`). `/proc/swaps` lists
it as `/dev/zram0`, `partition`, priority 100 unless `swapon -p` says
otherwise. Closes known-issues
`A-PROC-SWAPS-LISTS-THE-COMPRESSED-TIER-AS-ZRAM-WHICH-SWAPOFF-CANNOT-NAME`.

**The size.** `min(memory / 2, 4 GiB)` is the default of Fedora's
zram-generator, which every Fedora desktop has run since 2020; it is a cap on
what may be stored, not memory taken: a slot costs its 32-byte table entry
until a page is stored in it, and a stored page costs its compressed size
rounded up to the heap's size class. No commit accounting in this kernel
counts swap capacity, so a larger tier lets no program commit more than
memory holds.

**What a stored page costs (amended 2026-10-15, after review).** As first
built, a stored page kept the 16 KiB buffer it was compressed into, and an
incompressible page was kept as a full copy, so at this size reclaim could
"free" gigabytes without freeing a frame. Now an all-zero page costs nothing;
a page is kept only if it compresses to half a frame or less, in an
allocation of exactly its size -- the heap serves anything larger as whole
frames, so it would save nothing -- and one that does not is refused (as
Linux's zswap refuses it), staying in memory or going to a disk area. That
bounds the tier at its largest to a quarter of usable memory, in the range
of zswap's 20% pool cap. Compression uses scratch the tier allocated when it
was made, so storing a page allocates only the page's own bytes.

| Option | Against it |
|---|---|
| Keep 4 MiB | Compressed swap does nothing; the spec recommends it |
| `memory / 2`, at most 4 GiB (taken) | A machine with much memory caps at 4 GiB, as Fedora does |
| All of memory | Under pressure the tier's own memory crowds what it saves |

**The device.** Not a block device of compressed sectors, as Linux's zram is:
swap's own compressed tier (`MemBackend`) stores pages, and a device under the
disk path would compress them twice. The device exists so that the tier has a
name the tools know; reading anything but its header gives zeros.
