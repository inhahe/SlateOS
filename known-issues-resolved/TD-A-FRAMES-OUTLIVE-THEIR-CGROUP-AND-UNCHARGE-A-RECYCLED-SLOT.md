### TD-A-FRAMES-OUTLIVE-THEIR-CGROUP-AND-UNCHARGE-A-RECYCLED-SLOT (lane A)

**Status:** FIXED 2026-09-02 — generation-tagged charge references, the proper
fix proposed below rather than either cheaper alternative. Rationale in
`design-decisions.md` §679.

**What landed.** The per-frame charge record in `kernel/src/mm/frame.rs` is now
a `u32` *charge tag* — cgroup id in the low 8 bits, the slot's generation in
the upper 24 — instead of a bare `u8` id. `CgroupNode` gained a `generation`
that `init()` bumps on every create, so a reissued slot never mints the tag its
previous occupant held. `cgroup::tag_for` mints a tag and
`cgroup::mem_uncharge_tag` redeems one, dropping the uncharge when the
generations disagree and counting it in `cgroup::stale_uncharges_dropped()`.

**It is no longer untriggerable.** The section below notes that nothing during
boot deletes a memory-charged cgroup, which is what kept this latent — and what
would have let a wrong fix pass unnoticed. The self-test added with the fix
(`mm::frame` self-test 14, "Cgroup slot reuse") therefore drives the real path:
it charges a frame to a group, deletes the group *while that frame is live*,
walks `next_id` around the ring with create/delete until `create` hands back
that exact slot, and only then frees the frame. It asserts the heir's usage is
untouched, the drop was counted, the per-frame record was still cleared, and —
guarding against the guard itself being inert — that the two tags differ.

The remaining exposure is aliasing after 2^24 reuses of a single slot (~4.3
billion cgroup creations); `u16` tags would have put that at ~65,000, which is
reachable, which is why the array is `u32`. See §679 for the memory cost.

**Original report follows.**

**Status when filed:** OPEN — latent; no known way to trigger it during boot
today.

**In short:** Memory pages remember which resource group paid for them, so the
right group gets credited when they are freed. But a group can be deleted
while pages it paid for are still in use, and the group's slot number is then
handed to a *different* group. When those pages are finally freed, the credit
goes to whichever group now holds that slot number — a group that never
allocated them. Its memory accounting drifts low, so its memory cap stops
being enforced correctly.

**Where:** `kernel/src/cgroup.rs` → `delete` / `create`, and
`kernel/src/mm/frame.rs` → `uncharge_cgroup_free`.

**The mechanism, precisely.**

1. `charge_cgroup_alloc` records the charging cgroup's id in the per-frame
   array `FRAME_CGROUP` at allocation time, so the right group is credited
   even if a different task frees the frame later.
2. `cgroup::delete` requires `nr_tasks == 0` and `nr_children == 0`. It does
   **not** require `mem_usage == 0`. A group can therefore be deleted while
   frames it paid for are still live and still name it in `FRAME_CGROUP`.
3. `cgroup::create` finds a free slot and calls `node.init(parent)`, which
   resets that slot's counters. Slot ids are small (`MAX_CGROUPS` is 256) and
   `next_id` wraps, so reuse is routine, not exotic.
4. When those older frames are eventually freed, `uncharge_cgroup_free` reads
   the stale id and calls `mem_uncharge(cg, 1)` — debiting the *new* group.

The result is under-accounting on the new group: `mem_usage` reads lower than
its real usage, so its `MemLimit` admits allocations it should reject. It
cannot panic or corrupt memory (`mem_uncharge` saturates at zero), which is
why this is debt and not a live bug.

**Not triggered today** because nothing deletes a memory-charged cgroup during
boot. The `cgroup-e2e` task frees its frames before deleting its group, and
the cgroupfs suite's group never allocates. It becomes reachable as soon as
containers are created and destroyed at runtime, which is the point of the
subsystem.

**The proper fix** is generation-tagged ids: widen the per-frame record to
carry a generation counter bumped by `init()`, and have `uncharge_cgroup_free`
drop a charge whose generation no longer matches. That makes a stale reference
detectable rather than silently misapplied. Two cheaper alternatives are worse
and should not be taken: refusing to delete a group with `mem_usage > 0` makes
deletion depend on unrelated tasks' page lifetimes (a group could become
undeletable indefinitely), and walking `FRAME_CGROUP` on delete to clear stale
entries is O(all RAM) per delete.
