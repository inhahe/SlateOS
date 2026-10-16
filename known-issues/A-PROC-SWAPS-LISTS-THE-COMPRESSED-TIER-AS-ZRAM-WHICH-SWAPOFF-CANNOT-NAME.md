### [A] `A-PROC-SWAPS-LISTS-THE-COMPRESSED-TIER-AS-ZRAM-WHICH-SWAPOFF-CANNOT-NAME` — `swapoff -a` fails on the compressed tier's line in `/proc/swaps` -- 2026-10-14
**Status:** OPEN (lane A). Next on lane A's swap list.

**In short:** `/proc/swaps` lists the compressed-memory swap tier as `zram`
with type `partition`. A program that acts on that list -- util-linux's
`swapoff -a`, which an init script runs before powering off -- calls
`swapoff("zram")`, which is not a path to anything: it fails with "No such
file or directory" and exits 1 every time, though nothing is wrong.

**Where.** `kernel/src/fs/procfs.rs` `gen_swaps` prints each area's `path`;
the tier's is `zram` (`mm::swap::init`). `mm::swapctl::swap_off` takes a
`/dev` node or a file; and `swap::deactivate` refuses `AreaKind::Memory`
outright, so even a name it could resolve would not turn the tier off.

Found by the max-tier review of 2026-10-14 (Fable at `xhigh`), after
`/proc/swaps` took Linux's exact format: tools now read it, so they act on
its names.

**The fix.** The tier as Linux shows it, a block device: `/dev/zram0`, in
`/proc/swaps` under that name; `swapoff /dev/zram0` drains it into memory as
any area is drained (`swap::drain` already walks every address space) and
takes it away; `swapon /dev/zram0` puts it back. Its size from the machine's
memory rather than the fixed 256 slots (the open roadmap item on zram
sizing) belongs with it.

**Until then:** harmless but loud -- `swapoff -a` turns off every real area
and reports failure for the tier.
