### TD5. NUMA nodemask `{0, extra-node}` is rejected where Linux accepts it — APPROXIMATION 2026-06-12

**What:** `get_nodes_uma()` (`kernel/src/syscall/linux.rs`, used by
`sys_mbind` and `sys_set_mempolicy`) collapses Linux's full nodemask down
to two booleans — `mask_empty` and `mask_has_extra_bits` (any node other
than node 0 set) — and the callers reject `mask_has_extra_bits` with
`-EINVAL`. Linux instead **intersects** the user mask with
`current->mems_allowed` (= `{0}` on our single-node system) and checks the
*intersected* mask for emptiness in `mpol_ops[mode].create`.

**Divergence:** a mask of `{0, N}` (node 0 **plus** a non-existent node N)
is rejected by us (`-EINVAL`) but **accepted** by Linux for
`MPOL_PREFERRED` / `MPOL_BIND` / `MPOL_INTERLEAVE` / `MPOL_PREFERRED_MANY`,
because the intersection `{0,N} ∩ {0} = {0}` is non-empty. A mask of `{N}`
alone (no node 0) is `-EINVAL` in both (intersection empty), so only the
"node 0 present *and* an extra bogus node" case differs.

**Why it's an approximation, not a bug now:** real programs on a
single-node box pass either an empty mask or `{0}`; `{0, N>0}` is not a
shape `numactl`/libnuma/jemalloc/tcmalloc produce when only node 0 exists.
The result is also strictly *more* conservative (we reject something Linux
accepts; we never accept something Linux rejects).

**Proper fix:** have `get_nodes_uma` report the effective mask after
intersecting with `mems_allowed = {0}` (i.e. "is bit 0 set?") separately
from "are there bits we must hard-reject" (only bits above `MAX_NUMNODES`
are hard-rejected by Linux's `get_nodes` itself), and apply the per-mode
emptiness check to the *intersected* mask in `mpol_new_check`'s spirit.
This is only worth doing if/when we support more than one NUMA node.
