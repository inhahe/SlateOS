## §679 — a cgroup charge reference carries the slot's generation, and a stale uncharge is dropped rather than applied

**Date:** 2026-09-02. **Decided by:** Claude (autonomous). **Lane:** A.

**In short:** Memory pages remember which resource group paid for them, so the
right group gets credited when the page is freed. But a group can be deleted
while pages it paid for are still in use, and its slot number is then handed to
a *different* group — so the credit went to a group that never allocated the
page. That group's accounting then read lower than its real usage and its
memory cap quietly stopped being enforced. The fix pairs the slot number with a
counter that ticks every time the slot is reused, so a credit aimed at a group
that no longer exists is recognised and thrown away instead of landing on a
stranger. Closes the known-issue
`TD-A-FRAMES-OUTLIVE-THEIR-CGROUP-AND-UNCHARGE-A-RECYCLED-SLOT`.

### The decision

`mm::frame`'s per-frame charge record changes from a bare `u8` cgroup id to a
`u32` **charge tag**: the low 8 bits are the id, the upper 24 the slot's
generation, which `CgroupNode::init` bumps on every create.
`cgroup::mem_uncharge_tag` redeems a tag and drops the uncharge when the
generations disagree, counting the drop in `stale_uncharges_dropped()`.

### The charge and its tag come from one lock acquisition

The obvious API — charge with `mem_charge`, then ask `tag_for` what to record —
is wrong here, and the reason generalises past this fix. `mem_charge` uses
`try_lock` and returns `Ok(())` **without charging** when contended, because
frame allocation can run in interrupt context and blocking there would
deadlock. So "did the charge happen?" is not something the caller can infer,
and a second lock acquisition to fetch the tag can disagree with the first in
*both* directions: a charge that succeeded gets recorded with no tag and is
never released (a permanent over-count that tightens a live limit), and a
charge that silently no-opped gets recorded with a valid tag and is uncharged
anyway (an under-count that loosens one). The second is the very failure this
entry is about, reintroduced by the fix for it.

The fix is `mem_charge_tagged`, which performs the charge and mints the tag
under one acquisition and returns `TAG_NONE` for every path that did not
charge — so the answer to "was it charged?" and the answer to "what do I store
to release it?" are the same value and cannot drift apart. `mem_charge` is now
a thin wrapper over it, and `tag_for` survives only for callers that want to
*predict* or compare a tag with no charge attached, which in practice means the
self-tests. This is the general shape for any two-call API sitting behind a
`try_lock` that can succeed without doing the work.

### Why dropping is the right answer, and not a lesser one

Dropping looks like losing information, so it is worth being explicit that it
is not. The group that owed the charge is gone and its counters went with it,
so there is no correct account left to credit. The group holding the slot now
never allocated the frame. Debiting it is not "approximately right" — it is
strictly worse than doing nothing, because it drives that group's `mem_usage`
*below* its true usage, and an under-count is the one direction that makes a
`MemLimit` admit allocations it should reject. A limit that fails open is a
worse failure than a charge that goes unrecorded on a group that no longer
exists.

### Why 24 bits of generation, and why the array got four times bigger

The tag has to be wide enough that it cannot alias. With 24 bits, two tags
collide only if one slot is reused 2^24 = 16.7 million times — roughly 4.3
billion cgroup creations, given round-robin over 255 usable slots — while a
frame charged before the turn is still unfreed.

The cost is real and was weighed: the per-frame array goes from 1 byte to 4 per
16 KiB frame, i.e. from 1/16384th of RAM to 1/4096th. On 8 GiB that is 512 KiB
→ 2 MiB. A `u16` tag (8 bits of generation) would have cost only 1 MiB, and was
rejected: 256 reuses per slot is about 65,000 cgroup creations, which a
long-running container host reaches, and the failure it would reintroduce is
the same silent accounting drift — but now needing 65,000 creations to
reproduce, which is far harder to diagnose than the bug being fixed. Spending
1 MiB to retire the failure class outright, rather than to move it somewhere
harder to find, is the better trade.

### The two cheaper fixes that were rejected

Both were named in the known-issue and both are worse:

- **Refuse to delete a group while `mem_usage > 0`.** This makes deletion
  depend on unrelated tasks' page lifetimes: a group could stay undeletable
  indefinitely because some other process is sitting on a page it once
  charged. It converts an accounting bug into a liveness bug.
- **Walk the per-frame array on delete and clear stale entries.** Correct, but
  O(all RAM) per delete — on 8 GiB that is half a million entries scanned every
  time a container goes away.

### On testing a bug with no known trigger

The known-issue recorded that nothing during boot deletes a memory-charged
cgroup, which is why this was latent debt rather than a live fault. A fix for an
untriggerable bug is easy to get wrong and impossible to notice, so the
self-test drives the **real** reuse path rather than simulating it: it charges a
frame to a group, deletes the group while the frame is live, then walks
`next_id` all the way around the ring with create/delete until `create` hands
back that very slot, and only then frees the frame. It asserts three things —
the heir's usage is untouched, the drop was counted, and the per-frame record
was still cleared — plus that the two tags actually differ, which is what would
catch the guard being inert because the generation never advanced.

`stale_uncharges_dropped()` is deliberately a counter rather than a silent
correction. A non-zero value is the fix working, but a climbing one means
something upstream is holding frames past the lifetime of the group that paid
for them, and that is worth being able to see even though the accounting is now
correct either way.
