### BUG-CGROUP-1GIB. (RESOLVED 2026-07-22) per-frame cgroup array only covered the first 1 GiB → cgroup accounting leak above 1 GiB

**Where:** `kernel/src/mm/frame.rs` — was `const CGROUP_MAX_FRAMES = 65536;` +
`FRAME_CGROUP([u8; 65536])` static.

**What (was):** the per-frame cgroup-id array (used by `set_frame_cgroup`/
`get_frame_cgroup` to remember which cgroup a frame was charged to, so
`uncharge_cgroup_free` can uncharge the right group on free) was a fixed 65536
entries = 1 GiB window. A frame allocated above 1 GiB was charged
(`mem_charge` succeeded) but its per-frame record was dropped, so on free
`get_frame_cgroup` returned 0 and the group was **never uncharged** — a
monotonic cgroup memory-usage leak that eventually made a limited cgroup deny
allocations it should have allowed. Latent because every boot-test ran at
`-m 512M` (all frames < 1 GiB); surfaced when boot-test RAM was raised to 3 GiB
and the mm cgroup round-trip self-test panicked (`frame.rs:3035`, "charge must
record the cgroup id", `get_frame_cgroup` returned 0 for a high frame).

**Fix:** the cgroup array is now **dynamically sized to `total_frames`** and
carved from the frame-allocator metadata region in `frame::init` (right after
`page_info` + `refcount`); `FRAME_CGROUP_PTR`/`FRAME_CGROUP_LEN` publish it and
the accessors bounds-check against the live length. Covers all installed RAM.
Verified: boot GREEN at `-m 3072M`, `[mm] Cgroup per-frame tracking: OK` +
`Cgroup charge/uncharge round-trip (no double-charge): OK`.
