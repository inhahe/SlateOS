## A-TASK-IS-FREED-TWICE-WHEN-THE-ZOMBIE-TEST-REAPS (lane A, 2026-11-01) — OPEN

**Status:** OPEN -- seen in three full boot tests of two kernel binaries
(2c252e9a4 twice, a05eedbb4), never since; since b4f021af6 a live task's
slot refuses every free but its own and names the code that tried
(`mm::heap::guard_slot`, below), so the next time it strikes the report is
the stale free itself, and the task is left whole. It keeps lane A's
batches from 2c252e9a4 off main until it is understood.

**In short:** in two boot tests of one kernel binary the heap's double-free
detector fired at the same moment -- as the spawn self-test's zombie
`/proc` check reaped dead tasks -- and at the same slot: a 1024-byte heap
slot holding a scheduler task (`Box<Task>`) was freed when the detector
already took it for free. The detector cannot be
fooled by a slot's old contents (an allocation fills the whole slot,
`poison_alloc`), so a free reached that slot while a task lived in it:
something freed through a stale pointer -- any object of 513 to 1024 bytes
-- after the task took the slot, and the reaper's own, correct free of the
task is the one caught. The stale free itself goes unseen, since the slot
held a live task then. The boot test failed on it
(`syshealth`, run by the kernel shell's self-test, counts heap violations).

**Where.** `kernel/src/sched/mod.rs`, `reap_dead_tasks`, called by
`kernel/src/proc/spawn.rs`'s `self_test_zombie_keeps_proc_dir` after its
ring-3 process exits. Seen at 2c252e9a4 on lane-a-wip (boot of 2026-11-01
03:31 UTC; the serial log is kept at
`build/serial-failures/20261101T033121Z-2c252e9a4-rc1.txt`, line 7490):

```
[heap] DOUBLE-FREE detected! slot=0xffff80013756c400, class=1024
  kernel::mm::heap::poison_free
  kernel::mm::heap::HeapInner::slab_dealloc
  <KernelHeap as GlobalAlloc>::dealloc
  __rust_dealloc
  <Global as Allocator>::deallocate
  <Box<T,A> as Drop>::drop
  drop_in_place<Box<kernel::sched::task::Task>>
  kernel::sched::reap_dead_tasks
  kernel::proc::spawn::self_test_zombie_keeps_proc_dir
```

`reap_dead_tasks` removes each dead task from the scheduler's table under
its lock before dropping it, so two reapers cannot both drop one entry. A
real double free needs the same `Box<Task>` freed by another path first --
or held twice in the table under two ids. A false report needs a live task's
bytes at the detector's mark to read as its signature, which
`known-issues-resolved/A-PROC-VERSION-AND-LOADAVG-DOUBLE-FREE-A-256-BYTE-SLOT`
made address-dependent to rule out.

**Seen again, 2026-11-01 18:45 UTC**, at a05eedbb4 -- the next code
commit, which sealed WiFi drivers against inspection and moved the netstack
daemon off root -- with the same stack and another slot
(`0xffff8001374a8800`): two builds fail, so it is not one binary's
layout. The lead is 522cf381e..2c252e9a4, the third review follow-up of
the WiFi namespace work (mntns `Held`/`Sealed`, `enter_process`, `leave`,
`create`'s id order, the vfs detach self-test's hold), the only change
between the last build that passed and the first that failed.

**The guard, 2026-11-06.** `Task::boxed` guards every task's record as it
is boxed (`mm::heap::guard_slot`), and `Task`'s `Drop` unguards it: a free
of the slot meanwhile -- the stale free this issue is about -- is refused at
the start of `KernelHeap::dealloc`, before the per-CPU and global paths
part, counted with the double frees (so `syshealth` fails the boot), and
reported as "FREE OF A GUARDED SLOT refused" with the freeing code's
frames. The poison self-test's Test 6 checks it; its first version, on the
global path alone, failed that test (`(1, true, 2)`), the per-CPU cache
taking the free. The boot test of b4f021af6 passed with no task slot freed
early and no task double free -- this build did not strike it either.

**What is known of the cause.** It follows the binary's layout, not one
change: the boot tests of 522cf381e passed this check, both of 2c252e9a4
failed it (2026-11-01 03:31 and 03:55 UTC, the second log in
`build/serial-test-whole.txt` until the next boot), and 2c252e9a4 with a
ring of every 1024-byte free's stack added to `mm::heap::poison_free`, to
name the stale free, passed. 2c252e9a4 changed mount namespaces and
linuxdrv's driver start (design-decisions 1596) and frees no task; its
mount-namespace ring-3 tests run just before the zombie check, so their
teardown is the first place to look. Not the cause: `UserEntryInfo`, freed
by the entry trampoline and by an unstarted `SuspendedSpawn`'s drop, is 16
bytes, another class.

**The proper fix.** Find the stale free and fix it. Instrumenting the
allocator moves the layout and hides it, so the way in is one that leaves
the binary alone: a boot-time switch for what already exists -- KASAN
(`mm::kasan::enable`), which the boot runs only in its own self-test today,
or the first-free ring -- compiled into every build and read from the
command line, so that the binary that fails can be booted again with it on.
Then the report names the free that reached a live task's slot.
