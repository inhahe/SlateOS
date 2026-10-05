### B-CGROUP-DBLCHARGE. Demand-fault paths double-charge cgroup memory (manual `try_charge_current_mem` + `alloc_frame`'s internal charge) — FIXED (2026-06-30)

**Where:** `kernel/src/proc/pcb.rs` — `try_resolve_fault` demand-paging
paths. The whole-frame anon/file fast path (and the subpage path) call
`try_charge_current_mem(1)` *and then* `frame::alloc_frame()`, but
`alloc_frame` already charges the current task's cgroup internally
(`charge_cgroup_alloc`, recording the per-frame cgroup id in the
`FRAME_CGROUP` array). At final free, `free_frame` performs exactly one
`uncharge_cgroup_free` using the recorded id.

**Effect:** when cgroup memory accounting is active (`CGROUP_MEM_ACTIVE`
true), each demand page fault charges the cgroup **twice** (manual +1
and alloc_frame's +1) but uncharges only **once** at the final frame
free → a net **+1 charge leak per faulted page**. Over a process's
lifetime this inflates the cgroup's accounted memory without bound,
which can spuriously trip the cgroup memory limit / OOM. When cgroup
accounting is inactive (the common boot path), both charge calls
fast-exit, so there is no visible effect — which is why this has gone
unnoticed.

**Proper fix:** remove the manual `try_charge_current_mem(1)` /
`uncharge` bookkeeping from the demand-fault paths and rely solely on
`alloc_frame`/`free_frame`'s internal per-frame cgroup charging (which
is already correct and balances at the final free). The only subtlety:
`try_charge_current_mem` is also the place that enforces the *limit*
(returns an error to fail the fault when over budget) — so the fix must
ensure `alloc_frame` itself honors the cgroup limit (fail allocation
when the charge would exceed the limit) before deleting the manual
pre-check, otherwise the limit stops being enforced on the fault path.
Verify against the cgroup memory-limit self-test after the change.

**Discovered:** 2026-06-30 while wiring the page cache into the
FileBacked fault path (the cached-hit branch correctly needs *no*
manual charge, which surfaced the existing double-charge on the miss
branch).

**Fixed:** 2026-06-30. Removed the manual `try_charge_current_mem(1)` /
`uncharge_current_mem(1)` bookkeeping from both demand-fault paths in
`kernel/src/proc/pcb.rs` (subpage and whole-frame); the frame allocator
now owns cgroup memory accounting end-to-end. `alloc_frame` /
`alloc_frame_zeroed` already charge the current task's cgroup and honor
its limit (returning `Err(OutOfMemory)`, which the fault paths now
propagate as a rejected fault), so the deleted manual pre-check did not
weaken limit enforcement. Also closed two latent charge holes on the
zero-pool path: `alloc_frame_zeroed`'s pool-pop fast path now charges
the consumer, and `refill_zero_pool` uncharges frames it parks in the
pool (pooled frames are uncharged free inventory; the charge lands when
a consumer pops one). Regression guard: `mm::frame` self-tests 12
("charge/uncharge round-trip — no double-charge") and 13 ("over-limit
charge leaves no record"), which drive the real `charge_cgroup_alloc_to`
/ `uncharge_cgroup_free` primitives against an explicit test cgroup
(kmain self-tests run with no scheduled task, so the ambient
current-task cgroup is always root). Both pass in QEMU; the existing
cgroup charge/uncharge and limit-enforcement self-tests (10/11) still
pass.
