# Known Issues — OS kernel

Running list of unsolved bugs and technical debt.  Each entry should
have enough context to act on later: what the bug or debt is, where in
the code it lives, how to reproduce it (for bugs), and what the proper
fix looks like (for debt).

Per CLAUDE.md: "Ideally, bugs and tech debt are fixed immediately as
they're discovered — the tracking file is a fallback for when something
genuinely can't be addressed in the current task, not a place to defer
work that should be done now."

---

## Active Bugs

### D-SHM-MAP-NOCAP. `SYS_SHM_MAP`/`SYS_SHM_SIZE`/`SYS_SHM_CLOSE` do not verify the caller owns the handle — TECH DEBT (logged 2026-07-14)

`SYS_SHM_MAP` (kernel/src/syscall/handlers.rs `sys_shm_map`) maps a
shared-memory region into the caller's address space given only the
region's raw handle (which *is* the region ID — see `ShmHandle` in
kernel/src/ipc/shm.rs). It does **not** check that the calling process
created or was granted that region: any process that possesses (or
guesses — IDs are a small monotonic counter) a handle can map another
process's shared memory. `SYS_SHM_SIZE` and `SYS_SHM_CLOSE` have the same
gap (pre-existing). This is currently *by design* for the netstack Phase-4
bootstrap: the kernel creates the region and hands the handle to the
trusted `netstack` daemon over the `net.stack` control channel, so both
ends are trusted. **Proper fix:** gate SHM handles through the capability
system (unforgeable, per-process handle table with an explicit
grant/transfer op) before any *untrusted* process is allowed to use
`SYS_SHM_MAP` — i.e. before Phase 5 exposes socket data rings to arbitrary
apps. Until then, only kernel-mediated, trusted-daemon SHM sharing is
safe. Where it bites: any future userspace-to-userspace SHM use.

### B-FAULT-SERIALSTORM. Unconditional per-page-fault `serial_println!` saturated the (slow) serial port during demand-paging bursts, starving the hard-lockup kick and making boots crawl / appear hung — FIXED 2026-07-14

**Where:** `kernel/src/proc/pcb.rs` — `try_resolve_fault` (demand-paged
anonymous frame site, ~L5267) and `resolve_file_cached` (page-cache mapped
site, ~L5352).

**Symptom / how it was found:** while validating the i6300esb NMI
hard-lockup watchdog (Q20/§61, `boot-test.sh --hard-lockup-watchdog`), a
boot ran ~4915 ms/stage behind and the NMI fired on ~9.7 s of BSP
kick-starvation:
```
[hardlockup] armed (NMI on ~9.8s BSP silence)
[sched] Task [hardlockup] NMI WATCHDOG FIRED cpu=0 rip=0xffffffff8010f556 ...
        heartbeat=5365 kick_stale_ns=9738940603 — dumping backtrace + task table
```
The captured `rip`/rbp-chain, re-resolved with exact 64-bit integer
arithmetic (awk's double precision silently zeroed the high bits of
`0xffffffff8010f556`), walked through `spin_loop_hint` →
`liveness_boot_deadline_check` → `timer_tick` — i.e. the BSP was *not*
deadlocked, it was simply spending all its time emitting serial. Each
demand-paged frame and each page-cache mapping printed an unconditional
`serial_println!`; a process faulting in its whole address space emits
thousands of these, and the 115200-baud serial port (~11 KB/s) cannot
drain them fast enough. The write path back-pressures in kernel context,
delaying `hardlockup::kick()` from `timer_tick` past the watchdog's
~9.8 s threshold — the boot looked hung and, under host load, could
tip the documented B-DASH-STDIN-FLAKE reap race over its own edge.

**Fix:** route both hot-path fault logs through
`crate::klog!(Trace, "mm.fault", …)` instead of `serial_println!`. klog's
`serial_level` defaults to `Info`, so Trace entries stay in the dmesg ring
buffer (still available for debugging via `dmesg`) but are kept OFF serial
by default. No fault-path log is lost; only the serial storm is gone.

**Validation:** `boot-test.sh` after the fix reached `BOOT_OK` in 132 s
with `storm=0` (zero `Demand-paged`/`Page-cache mapped` serial lines vs.
thousands before) and the container multi-network self-test still passing.
Boot no longer crawls; the hard-lockup kick is no longer starved by
demand-paging bursts.

**Note (Q20 watchdog validated):** this capture also *confirms the
i6300esb NMI hard-lockup detector works end-to-end* — it armed over the
boot ring-3 window, detected real BSP kick-starvation, delivered an NMI on
the dedicated IST2 stack, and dumped a usable rbp-chain backtrace + task
table exactly as designed. The detector doing its job is what surfaced
B-FAULT-SERIALSTORM in the first place.

### B-PREEMPT-SPINLOCK. Involuntary preemption while holding a tracked spinlock → single-CPU priority-inversion deadlock — ROOT-CAUSED & FIXED 2026-07-01

**Where:** `kernel/src/sched/mod.rs` (`do_deferred_preempt`), `kernel/src/sync.rs`
(`Mutex::lock`/`try_lock`/`MutexGuard::drop`). Manifested as a hang in
`accounting::self_test` on the `ACCT` lock (`kernel/src/mm/accounting.rs`).

**This is the true root cause of the long-standing intermittent
spawn/kill/reap / accounting-self-test hang** previously filed as **F6**
("Accounting self-test hang — LIKELY CURED INCIDENTALLY", further below) and
related to the B-PTHREAD-YIELDBUDGET / TD31 "total silence, no dump"
fingerprint. F6 was never actually cured — it just didn't recur in the soak
because the trigger is timing-dependent (~5%). The spinlock stall detector
(commit `c8c1fa63`) finally caught it red-handed.

**Symptom / evidence:** boot hangs mid-`accounting` self-test. The stall
detector prints:
```
[sync] *** SPINLOCK STALL *** lock 'ACCT' ... (cpu 0, task 0, ... iters)
[lockdep]   cpu 0 holds 2 lock(s): [0] ACCT [1] ACCT
```
The "recursive" `[0] ACCT [1] ACCT` is NOT true recursion. lockdep's held
stack is **per-CPU** and is not cleared on context switch, so `[0]` is the
still-tracked entry of a task that was **preempted while holding `ACCT`**, and
`[1]` is a second, higher-priority task now spinning to acquire the same lock —
both accumulated on cpu 0's held stack.

**Root cause:** a kernel spinlock must never be held across a context switch.
`crate::sync::Mutex` did not disable preemption while held, so the timer ISR
could involuntarily preempt (`do_deferred_preempt` → `preempt`) a task
mid-critical-section. On a single CPU, if a higher-priority task (e.g. the
prio-31 boot self-test driver) then spins on that lock, the preempted holder
can never be rescheduled to release it → permanent deadlock. `do_deferred_preempt`
already had a *SCHED-only* guard (`SCHED.is_locked()`) for exactly this hazard —
it was a band-aid that covered one lock instead of the general invariant.

**Fix (the proper, general one):** a per-CPU preempt-disable count
(`PREEMPT_DISABLE_COUNT`, Linux `preempt_count` analogue). `Mutex::lock`/
`try_lock` call `sched::preempt_disable()` for the whole hold; `MutexGuard::drop`
calls `preempt_enable()` **after** the physical unlock (the inner spin guard is
now held in `ManuallyDrop` so the unlock is ordered before the enable — closing
the tiny window where a tick could switch away with the lock still physically
held). `do_deferred_preempt` refuses to involuntarily switch while
`preempt_count(cpu) > 0`, re-arming `NEED_RESCHED` so the preemption lands on a
later tick after the lock is released. Interrupts stay **enabled** (this is
preempt-disable, not IRQ-disable); locks also taken from a hardware ISR (e.g.
cgroup `TABLE` via `timer_tick`) already use `try_lock` on the ISR side, so
preempt-disable alone is sufficient.

**Verification:** 3× consecutive green boot tests (193–196s), accounting
self-test now passes the previously-deadlocking "Largest RSS" step; no
`SPINLOCK STALL` in the serial log; clippy clean on both changed files.

**Limitation / follow-up:** the guard covers *involuntary* preemption only.
Voluntarily yielding/blocking (`yield_now`/`block`) while holding a tracked
spinlock is still a caller bug and is not guarded (there is no such call site
today). **Done (2026-07-01):** added a one-shot warning in `schedule_inner`'s
voluntary-switch path when `preempt_count(cpu) > 0` (commit `49c92d346`);
it stayed silent across all boots, confirming no offending call site exists.
Also added (commit `ebd5c4b21`) a lockdep instant SELF-DEADLOCK diagnostic when
the *same* lock instance is re-acquired on one CPU — fires immediately instead
of waiting ~30s for the stall detector, now reliable because tracked mutexes no
longer carry stale per-CPU held-stack entries across a context switch.

**Raw `spin::Mutex` audit (2026-07-01):** the preempt-disable fix protects only
`crate::sync::Mutex`; a *raw* `spin::Mutex` (250+ call sites, mostly procfs/sysfs
leaf backends) held across a preemptible path and contended by a higher-priority
task is the same latent deadlock class — and is *invisible* to both lockdep and
the stall detector. Audited the only plausibly-dangerous category, the blocking
IPC primitives (`futex`, `pipe`, `stream_socket`, `semaphore`, `eventfd`,
`epoll`, `timerfd`, `signalfd`): **all clean** — every one follows the correct
enqueue-waiter → `drop(table)` → `block_current()` discipline (e.g.
`futex.rs:340-379` scopes the table lock in a block that closes before the park).
The remaining raw-`spin::Mutex` uses are short snapshot copies where the
held-across-preempt window is a handful of instructions and cross-priority
contention is implausible. **Proper systemic fix (deferred tech-debt):** migrate
kernel-internal raw `spin::Mutex` to `crate::sync::Mutex` so *all* kernel
spinlocks disable preemption and get lockdep coverage — gated on first checking
the lockdep class-table capacity (a 250-lock bulk migration could overflow it),
so it needs a capacity bump or a per-class opt-in rather than a blind sweep.

### B-ACCT-LARGEST. `accounting` self-test "Largest RSS" assumed test-only isolation, panicking when a live process held >50 RSS frames — FIXED 2026-06-30

**Where:** `kernel/src/mm/accounting.rs`, self-test "Largest RSS"
section (was ~line 507). The test charged two fake PML4s (a=20, b=50)
then asserted `largest_rss().pml4_phys == pml4_b`. But `largest_rss()`
scans the **global** accounting table, which during a live boot also
contains *real* process address spaces. Whenever a concurrent real
process happened to hold >50 frames at that instant, `largest` was that
real PML4 (e.g. `0x1DFE0000`, not the fake `0xBEEF0000`), so the
`assert_eq!` panicked and **hard-halted the whole boot**, masking every
self-test after it. A load-dependent flake: it passed on light boots
and failed under heavier ones.

**Fix:** the assertion was false-isolation; replaced with invariants
that hold deterministically even with real entries present:
(1) among the test's own entries, `query` confirms b (50) outranks
a (20); (2) `largest_rss().rss_frames >= 50` — i.e. it returns a true
global upper bound — instead of asserting it equals a specific fake
PML4. Verified: clean build + green boot self-test.

### B-CONTAINER-JAIL-TESTRACE. `container` self-tests 18/19 (rootfs jail + volume mounts) flaked non-deterministically: spawned a real init process, then inspected its per-PID namespace state, which the process cleared by exiting mid-test — FIXED 2026-06-30

**Where:** `kernel/src/container.rs`, self-tests "Rootfs jail (chroot) for
init process" (Test 18) and "Volume (bind) mounts for init process"
(Test 19). Both originally did `let pid = run(ct, HELLO_ELF, &opts)` to
spawn a *real, schedulable* init process, then called
`namespace::resolve_path_for(pid, …)` several times to assert the chroot/
volume wiring. The race: `HELLO_ELF` prints one line and **exits
immediately**; on another CPU it could run and exit *between* two of the
test's resolves. Thread teardown on exit calls `namespace::detach(pid)`,
which drops `PROCESS_ROOT[pid]`/`PROCESS_MOUNTS[pid]`, so a later
`resolve_path_for(pid, …)` returned the **unjailed input verbatim** and
the `assert_eq!` panicked → hard-halted the boot. Observed as Test 18's
`..`-escape assert failing on a heavy boot while an identical-binary
re-run passed (load-dependent flake). Production code is correct: a live
process resolves its *own* paths inside its own syscall handler, so the
jail always exists for the duration; only a third-party test reading
another process's namespace after it may have exited hits this.

**Fix:** Tests 18/19 no longer spawn a schedulable process. They register
a *synthetic, never-scheduled* PID through `add_process(ct, FAKE_PID)` —
the exact same container-layer wiring path `run()` uses
(`add_process_task` → `set_root`/`add_volume`) — and then run the
resolution asserts deterministically (the PID has no thread, so it cannot
exit and clear its state). The concerns that genuinely need a live
process are still covered without the race: the end-to-end
`run()`→cgroup-billing path by the "Run init process + cgroup billing"
test (Test 17), and the resolution *semantics* (`..` clamp, longest-
prefix volume match) by `namespace::test_process_root` /
`test_volume_mounts` (which already use synthetic PIDs 88888/88889). The
`state != Created` config-rejection guard is now exercised via `stop()`
rather than a live process, so it too is deterministic. Verified: clean
build + green boot self-test ("Self-test PASSED (19 tests)").

**Update (2026-06-30) — latent flake OBSERVED as a boot hang, now FIXED:**
The Test 17 liveness risk noted above stopped being theoretical. On a
heavy boot run the serial log froze mid-test right after the `run()` log
line (`[container] run id=8 'test-run-ct': init pid=219 …`) and never
reached `BOOT_OK` (480s timeout → boot gate FAILED). An identical-binary
re-run passed (`BOOT_OK after 187s`), confirming a load-dependent race,
not a logic bug — a timer ISR preempted the boot self-test thread into
the freshly-spawned init task, which executed `hello`; the exiting
thread's teardown then raced the test's explicit teardown, deadlocking
(a hang, not an assert panic — no `[PANIC]` was printed). This was worse
than the predicted assertion flake because a hang fails the *entire* boot
gate. **Fix:** Test 17's spawn→teardown window is now bracketed in
`cpu::without_interrupts(...)`, so the init task is still *registered*
(cgroup billing is verified end-to-end exactly as before) but can never
be *scheduled* before `destroy()` removes it — deterministic, with no
loss of real-`run()` coverage. Verified: clean build + green boot
self-test. Production code is unaffected (a live process only ever
resolves its *own* state inside its own syscall handler).

### B-PTHREAD-YIELDBUDGET. Intermittent "BSP-dead total-silence hang" during boot ring-3 self-tests — RESOLVED 2026-07-02 (structural: interrupts now enabled before the battery; see the "STRUCTURAL ROOT FIX" note at the end of this entry). Original title: `/bin/pthread` self-test can exceed the 262 144-yield exit budget under heavy boot load — WATCH (non-fatal)

**Where:** boot integration self-test that spawns `/bin/pthread`. The
harness waits for the child to exit within a fixed yield budget
(262 144 yields). On a heavy boot (observed once at ~229 s wall vs. the
normal 161–192 s), the child was still `state=Running` when the budget
expired and the harness logged "process did not exit within 262144
yields (state=Running)". This is a **non-fatal warning** — it does not
panic or fail the boot, and the same test passed on the immediately
preceding and following boots.

**Assessment:** a timing flake, not a correctness bug. The mutex/futex
hot loop was not touched by the surrounding container/VFS work, and the
failure is purely budget-vs-wall-clock under contention. **Proper fix
(deferred):** make the harness wait on an actual exit signal / longer
adaptive budget rather than a fixed yield count, so a slow-but-correct
run isn't misreported. Tracked here until the harness is reworked.

**Recurrence 2026-06-30:** observed again on a ~217 s BOOT_OK run (heavy
boot); the harness logged the same "did not exit within 262144 yields
(state=Running)" for the real-glibc pthread variant. Non-fatal that time —
BOOT_OK was reached and the container self-test (40 tests) passed on the
same boot.

**Severity escalation 2026-06-30 — a *full* boot hang was observed, not
just the non-fatal warning.** On a subsequent run the boot never reached
BOOT_OK within the 480 s timeout; the serial log's last activity was in the
real-glibc clone/COW region (pid 170/171: `[cow] Cloned address space`,
page-cache faults for the glibc text inode, a freshly spawned thread in the
child) with no further progress — consistent with the pthread `clone`+futex
worker deadlocking *permanently* rather than merely running slow. The very
next boot (identical binary) reached BOOT_OK at 222 s with the pthread test
passing (`captured 48 bytes == expected: OK`), confirming the hang is
intermittent. This means the futex/clone path has a **real, low-probability
deadlock**, not purely a yield-budget timing artifact — the fixed-budget
harness masks it as a warning on slow-but-live runs but the underlying hang
can be total. **Proper fix (still deferred, now higher priority):** root-cause
the futex wait/wake race in the glibc `clone`+TLS worker path (candidate: a
lost wakeup when a waker runs before the waiter parks, or a missed requeue),
in addition to reworking the harness to wait on a real exit signal. No code
change made this session (the observation came from unrelated container-CLI
boot tests); logged here so the intermittent total hang isn't forgotten.

**Search narrowed 2026-07-01 (negative result):** audited the core futex
wait/wake primitive for the "lost wakeup when a waker runs before the waiter
parks" hypothesis and found it **sound** — not the bug. `futex_wait_bitset`
enqueues the `Waiter` under `FUTEX_TABLE`, drops that lock, then calls
`sched::block_current()`; the classic window between "dropped the futex lock"
and "parked" is closed by the scheduler's `pending_wake` flag: `sched::wake`
(mod.rs ~L1388) and `sched::try_wake` (ISR path, ~L1436) both set
`task.pending_wake = true` when the target is *not yet* `Blocked`, and
`block_current` (~L1373) consumes that flag and returns **without** parking. So
a `futex_wake` (or timer/ISR wake) that races ahead of the park cannot be lost.
The `register-then-recheck` signal-waiter dance likewise closes the
signal-vs-enqueue window for user tasks. **Conclusion:** stop looking at the
futex primitive; the intermittent total hang is in the surrounding ring-3
`clone`/CoW-fault/thread-teardown-reap machinery (the last serial activity on
the total-hang run was in the glibc `clone` CoW region — `[cow] Cloned address
space`, page-cache faults for the glibc text inode — not inside a futex wait).
Next candidates to instrument: (a) the CoW page-fault handler taking a lock the
reaper/`clone` path also takes (frame-alloc vs. address-space vs. page-table
lock ordering), and (b) `on_thread_exit`/`reap_dead_tasks` racing a thread that
is mid-`clone`. A lock-order tracer around the address-space + frame-alloc +
SCHED locks during a `clone`-heavy boot is the tool to build next.

**Tooling reconnaissance 2026-07-01 (negative — narrows the fix, no code
change).** Two findings that reshape what "instrument this" requires:
1. *A lockdep validator already exists and is enabled at boot* (`kernel/src/lockdep.rs`,
   `lockdep::init()` at `main.rs:3678`; `crate::sync::Mutex` auto-reports
   acquire/release; `lockstats` kshell cmd). It flags an AB-BA cycle on **any**
   boot where both orderings are ever observed — but **only for locks that use
   the tracked `crate::sync::Mutex`.** The two prime suspects are **untracked raw
   `spin::Mutex`**: the buddy frame allocator (`mm/frame.rs:813`
   `static ALLOCATOR: Once<Mutex<BuddyAllocator>>`, `use spin::{Mutex, Once}`)
   and the rmap table (`mm/rmap.rs:174` `static TABLE: Mutex<RmapTable>`,
   `use spin::Mutex`). **That is exactly why the hanging runs produced no lockdep
   report.** Migrating them to `crate::sync::Mutex` would let lockdep catch a
   latent inversion deterministically — but the frame allocator is a
   <1 µs-target hot path and lockdep adds ~50–200 ns/acquire, a >20% regression
   on every `alloc_frame`/`free_frame`, so this can't just be left on in normal
   builds. A `cfg(feature = "lockdep_mm")` gated migration is the proper form if
   this route is taken.
2. *Give-up-path instrumentation would not catch the TOTAL hang.* The yield-budget
   "did not exit within N yields" give-up messages in `proc/spawn.rs` (~20 sites)
   only fire when the driver task keeps running and merely the *child* is slow.
   In the total-hang variant the serial log stops mid-clone with **no further
   output at all** — the give-up line never prints, meaning the driver (or the
   whole CPU) also stalled, consistent with a lock held forever by a stuck task.
   So a state-dump *at the give-up* is useless here; catching this needs a
   **timer-interrupt watchdog** that, on N seconds of no forward progress, dumps
   every task's `(id, name, state, cpu, wait-reason)` from IRQ context (and must
   itself take **no** contended lock — use `try_lock`/lock-free reads only). That
   watchdog is the real next build; it's larger than a one-liner, hence deferred
   rather than bolted on mid-turn. Until then the bug stays WATCH: it is rare,
   does not affect the common boot (BOOT_OK is reached ~95%+ of runs), and is
   fully documented here.

**Root-cause narrowing 2026-07-01 (audit line concluded — I/O paths cleared,
instrument built).** A systematic pass eliminated every lock-order and I/O
lost-wakeup hypothesis, leaving two structural suspects, and the hung-task
watchdog called for above is now **implemented and boot-validated**.
- *Hypotheses eliminated (all proven sound by inspection):*
  1. Futex primitive — sound; `pending_wake` closes the register/block race.
  2. Ready-starved task lost from the run queue — RULED OUT: `check_starvation`
     (`sched/mod.rs`) re-enqueues any Ready non-throttled task within ~2 s.
  3. `page_cache::get_or_fill` (`mm/page_cache.rs:214`) — optimistic
     fill-then-insert with race resolution; **no fill-in-progress wait queue**,
     so no lost-wakeup there.
  4. PAGE_CACHE ↔ frame ALLOCATOR lock order — consistent (PAGE_CACHE is always
     the outer lock via `ref_inc`; `alloc_order` releases ALLOCATOR *before*
     reclaim/compact/OOM), so no AB-BA.
  5. Page-cache fill closure (`fs/handle.rs:584` `read_at_uncached` →
     `Vfs::read_at_uncached_resolved` → `fs.lock().read_at`) holds **no**
     page-cache/frame lock across the read, and `write_at`/`truncate` invalidate
     the cache only *after* dropping `fs.lock()` — no fs.lock↔PAGE_CACHE nesting.
  6. **Block-device read (the serial trace stops exactly here) — ELIMINATED.**
     `virtio/blk.rs::wait_completion` in IRQ mode is a **HLT-poll loop bounded by
     a 500-attempt (~5 s) timeout** (`if attempts > 500 { … "timed out (IRQ
     mode)" … return Err(TimedOut) }`), *not* a wait-queue block. The 100 Hz
     timer wakes every `hlt()`, so even a fully lost device IRQ cannot hang it
     silently — it would print `[virtio-blk] … timed out` and return an error.
     The hang trace shows no such line, so the disk read is not the stall. The
     RAM-disk path is a plain synchronous memcpy (no wait queue either).
- *Remaining suspects (cannot be pinned by static reading — need a runtime
  dump at the moment of hang):* (a) a `clone`/CoW thread whose wakeup is lost on
  some primitive *other* than the futex/page-cache/frame paths above; (b)
  `on_thread_exit`/`reap_dead_tasks` racing a thread that is mid-`clone`.
- *Instrument built (this is the "real next build" the reconnaissance note asked
  for):* a **system-wide liveness watchdog** in `sched/mod.rs`
  (`liveness_arm`/`liveness_disarm`/`liveness_check`/`dump_all_tasks_serial`,
  driven by the BSP every `WATCHDOG_CHECK_INTERVAL` = 5 s alongside the existing
  soft-lockup watchdog). It watches one global counter, `USEFUL_WORK_TICKS`,
  bumped by `timer_tick` whenever a tick preempts a **non-idle** context
  (`from_user || local_has_real_work`). At the total-hang every CPU is parked in
  the idle task with an empty run queue, so this counter **freezes** even though
  per-CPU heartbeats keep climbing (which is precisely why the soft-lockup
  watchdog can't see it). If it fails to advance for `LIVENESS_ALERT_COUNT` = 3
  consecutive intervals (~15 s) while armed, the BSP dumps every task's
  `(tid, state, cpu, prio, pending_wake, ready_since, waited, blocked_on_pi,
  name)` plus each CPU's `(heartbeat, ctx_switches, local_has_real_work)`
  straight to serial from IRQ context using **try_lock only** — and if it can't
  get `SCHED`, it reports *that* (a task wedged holding `SCHED` is itself the
  deadlock). It then disarms so the report prints exactly once. Scoping solves
  the idle false-positive problem the reconnaissance note flagged: it is armed
  only for the boot ring-3 window (`main.rs`, right before the ring-3 fork/CoW/
  reap self-tests) and disarmed at BOOT_OK, before the system may legitimately
  idle at an interactive prompt. Validated: a healthy boot reaches BOOT_OK with
  **zero** `[liveness]` output (silent when healthy). Next time the hang
  reproduces in a boot test, the serial log will name the lost thread and its
  state — turning this heisenbug into a directly-diagnosable one.
- *On-demand dump added:* the same task-table dump is now reachable
  interactively via the kshell `taskdump` command (aliases `hungcheck`/
  `dumptasks`; `sched::dump_task_table()`), for capturing state when a system
  feels wedged at a prompt — the window where the boot-scoped watchdog is
  disarmed. try_lock-only, safe on a partially-hung system, output to serial.
- *Reproduction attempt 2026-07-01 (negative):* ran `scripts/hang-repro-loop.sh`
  for 16 consecutive boots (15-boot batch + 1 validation) with the instrument
  armed — **all reached BOOT_OK, zero `[liveness]` fires, no catch.** Consistent
  with the ~5% rate (P(0 catches in 16 boots) ≈ 44%), so this neither reproduces
  nor disproves the bug; it just confirms the instrument is silent on healthy
  boots and does not itself destabilise boot. The watchdog stays permanently
  armed for the boot window, so any future reproduction (in CI or ad-hoc boots)
  will be captured automatically. Not running further blind repro batches — they
  produce no artifact — until the bug surfaces on its own.
- **Reproduced 2026-07-01 (the bug surfaced on its own) — BUT THE WATCHDOG DID
  NOT FIRE, exposing a structural blind spot in the instrument.** A boot test
  during the tee(2) session hung: no BOOT_OK within the 480 s timeout, ~470 s of
  total serial silence. The hang point matches the family signature exactly — the
  "REAL make-drives-tcc build (ring 3, Path Z)" stage: `/bin/tcc -c /cap-a.c -o
  /cap-a.o` triggered `[cow] Cloned address space: parent=0x1bb83000 ->
  child=0x119000`, task 176 / process 210 exec'd a PIE ELF (ld-linux
  interpreter), then the last two lines were `[thread] Process 210 has no threads
  left — now zombie` / `[sched] Task 176 exiting`, followed by dead silence. Log
  preserved at `build/hang-catches/CAUGHT-2026-07-01-tee-session-nobootok.txt`
  (5773 lines). The very next boot (`--no-build`, identical binary) reached
  BOOT_OK in 206 s — confirming intermittency, as always. **The critical new
  signal: no `[liveness] SYSTEM HANG` dump, no `[watchdog]` soft-lockup line —
  nothing at all.** The watchdog *was* armed (armed at `main.rs:1341`, well before
  this Path-Z stage; disarmed only at BOOT_OK, which was never reached), so
  arming is not the gap. That leaves two structural blind spots, and the total
  silence points hard at the second:
  1. *Livelock (watchdog resets every interval):* if some non-idle task keeps
     getting ticked (a busy-spin / lost-wakeup retry loop in ring-0 or ring-3),
     `timer_tick` charges the tick to a non-idle context (`from_user ||
     local_has_real_work`) and bumps `USEFUL_WORK_TICKS`, so `liveness_check`
     (`sched/mod.rs:1738`) sees `current != previous`, resets `LIVENESS_STALL_COUNT`
     to 0, and never reaches the 3-interval alert. The watchdog only catches an
     *idle* hang (all CPUs parked in the idle task), not a *busy* one.
  2. *BSP stopped ticking (watchdog never runs at all) — most likely here.* The
     ENTIRE watchdog stack (`watchdog_check` + `liveness_check`) is driven from
     `timer_tick` on **cpu == 0 only** (`sched/mod.rs:1955`, `:1972-1976`). If the
     BSP itself wedges with interrupts disabled — a spin holding a raw `spin::Mutex`
     with IF=0, or the LAPIC timer not re-armed — the BSP timer ISR never runs, so
     neither watchdog ever executes and no diagnostic can print. The observed
     **total** silence (not even the soft-lockup detector, which watches per-CPU
     heartbeats and would fire within 15 s if the BSP were still ticking while an
     AP froze) is the fingerprint of a dead BSP tick, i.e. blind spot (2).
  **Proper fix (the real next build, deferred — larger than a one-liner):** make
  the hung-system detector independent of the BSP timer tick.
  - *Cross-CPU liveness (cheap partial fix):* also call `liveness_check()` from an
    **AP's** `timer_tick`, not just cpu 0, so a wedged BSP doesn't take the whole
    watchdog down with it. Guard the shared stall counters for concurrent access
    (they're already atomics; the one-shot disarm makes double-fire harmless).
    Does not help if *all* CPUs stop ticking, and — critically — **our boot test
    runs single-CPU**, so there is no AP to run this. Useful only once boot tests
    exercise SMP.
  - *NMI-based hard-lockup detector — FEASIBILITY BLOCKER FOUND 2026-07-01.* The
    Linux `watchdog_hld.c` model arms a **PMC counter overflow → LAPIC LVT
    PerfMon → NMI**, which fires even with IF=0. **But this cannot work in our
    validation environment:** `scripts/boot-test.sh` launches QEMU with **no
    `-accel` and no `-cpu` flag** → default **TCG** + `qemu64`, which does **not
    emulate the PMU overflow→NMI path** at all. A PMC-based detector would never
    fire under our only test harness, so it is untestable and effectively dead on
    arrival here. (On real hardware / KVM it would work, but we have no such test
    path.) Combined with single-CPU (no AP to send a watching NMI-IPI), the PMC
    approach is the wrong build for this project as currently tested. **Do NOT
    build the PMC detector against the current harness.**
  - *Revised approach that DOES work under TCG (the actual next build): QEMU
    `i6300esb` PCI watchdog → inject-NMI.* Add `-device i6300esb` +
    `-action watchdog=inject-nmi` to `boot-test.sh`, write a small kernel driver
    that maps the device BAR and **kicks** the watchdog from the timer tick (or a
    dedicated periodic point). If the BSP wedges with IF=0 the kicks stop, the
    watchdog expires, and QEMU injects a real NMI regardless of IF — caught by
    `handle_nmi` (idt.rs:1422), which would then dump the task table (try_lock
    only) via `sched::dump_task_table`. Requires: the driver, a **dedicated IST**
    for the NMI vector (currently `ist=0`), arming scoped to the boot ring-3
    window, and the harness flag change. **Blast-radius caveat:** this touches the
    *shared* boot harness — a mis-tuned kick period would make every future boot
    test spuriously NMI-dump or let QEMU reset the guest. Because it changes shared
    test infra, it is queued for an operator steer in `open-questions.md` rather
    than landed unilaterally. Validating it against the actual ~5% heisenbug is
    also hard (needs ~20 boots to reproduce once).
  **Blind spot (1) livelock guard — IMPLEMENTED 2026-07-01** (`sched/mod.rs`
  `liveness_check`, `total_ctx_switches`, statics `LIVENESS_LAST_CTX` /
  `LIVENESS_CTX_STALL_COUNT`). On the healthy branch (useful-work advanced), the
  watchdog now also samples the **system-wide context-switch total** (sum of the
  per-CPU `CTX_SWITCHES`). The busy-livelock signature is *useful-work advancing
  while the aggregate ctx-switch count is frozen*: a task monopolizing a CPU
  without ever yielding gets its own timer ticks charged as "useful work" yet
  produces no context switch, whereas a healthy boot self-test phase
  context-switches continuously (thread spawn/reap/futex hand-off/yield). After
  `LIVENESS_ALERT_COUNT` (3 = 15 s) such intervals it prints a `SUSPECTED
  LIVELOCK` line + task dump. Deliberately chosen discriminator over
  "sample-the-running-tid": the long-lived boot self-test *driver* task keeps the
  same tid across the whole armed window, so same-tid-for-K-intervals would
  false-positive; ctx-switch-frozen does not. Because a rare legit long
  single-task compute in a stress self-test could in principle also freeze ctx
  switches while charging useful work, the livelock report is a **soft warning**:
  it does NOT disarm the watchdog (so a false positive cannot disable hang
  detection for the rest of boot) and re-fires at most once per 3 intervals.
  Covered by an extended `test_liveness_watchdog` self-test (drives the guard to
  threshold under IF=0, asserts it warns without disarming and resets on
  ctx-switch progress). This closes the *busy*-livelock variant; the **BSP-dead
  blind spot (2)** (total silence, IF=0 spin — the fingerprint of the 2026-07-01
  catches) still requires the NMI-based detector above and remains deferred.
  **Blind spot (2) software mitigation — IMPLEMENTED 2026-07-01** (`sync.rs`
  `Mutex::lock_contended` / `report_stall`, `lockdep::dump_held_locks`). Rather
  than wait on the operator-gated i6300esb/NMI hardware path (Q20), the contended
  path of `crate::sync::Mutex` now runs a **bounded-spin stall detector** in pure
  software: it spins on `try_lock` (behaviourally identical to the old
  `spin::Mutex::lock()`), and if a single acquisition spins longer than
  `STALL_SECONDS` (30 s) of PIT-calibrated TSC wall time it emits a **one-shot,
  non-fatal** `*** SPINLOCK STALL ***` diagnostic naming the lock, the wedged
  cpu/task, and — via the new `lockdep::dump_held_locks` — the locks that cpu
  already holds (the key AB-BA/convoy clue), then keeps spinning. Because it fires
  from *inside* the spin loop it works even with IF=0, which is exactly the
  BSP-dead fingerprint the timer-driven watchdog misses. The threshold is far
  beyond any legitimate kernel hold (ms-scale), so it never false-fires under
  normal contention (verified: BOOT_OK 182 s, zero `SPINLOCK STALL` lines).
  Globally rate-limited to `MAX_STALL_REPORTS` (8) so a multi-CPU convoy can't
  flood serial; falls back to a raw iteration count if the TSC isn't yet
  calibrated. **Coverage caveat:** this only catches deadlocks on locks that go
  through `crate::sync::Mutex`; a hang on a *raw* `spin::Mutex` (or a
  non-lock IF=0 spin) is still invisible to it — those remain the domain of the
  Q20 hardware NMI detector. The new `dump_held_locks` helper is exercised by a
  lockdep self-test (Test 6). This meaningfully narrows blind spot (2) without
  touching the shared boot harness or waiting on the operator.
  **CGROUP TABLE lock brought under observability — 2026-07-01** (`cgroup.rs`).
  The cgroup `TABLE` lock — the single lock most implicated in the hang (TD31:
  adding attach/detach TABLE traffic to spawn/reap made the ~5% hang
  near-deterministic) — was a **raw `spin::Mutex`**, so it was invisible to both
  lockdep and the stall detector. Converted it to a tracked
  `crate::sync::Mutex::named(…, b"CGROUP")`. Zero behavioural change (only
  `lock()`/`try_lock()` were used, both drop-in), but now: (a) a TABLE-side
  deadlock produces a `SPINLOCK STALL` dump instead of silence, and (b) lockdep
  tracks TABLE for order validation and contention stats. Cost is negligible —
  cgroup mutations are rare and off every hot path. Verified BOOT_OK 185 s, no
  new lockdep violation, cgroup self-test still green.
  **NOT yet converted: `SCHED` (sched/mod.rs:255) is also a raw `spin::Mutex`.**
  For lockdep to detect the *suspected* SCHED↔CGROUP AB-BA it needs **both** locks
  tracked, so the edge is still not recorded. But converting SCHED is a separate,
  **benchmark-gated** decision: SCHED is the hottest lock in the kernel (acquired
  on every context switch / timer tick / spawn / reap), and `crate::sync::Mutex`
  adds a lockdep held-stack push + edge scan on every acquire — a real
  context-switch-latency risk against the <5 µs target. On a **single-CPU** boot a
  classic two-CPU AB-BA is impossible anyway; the realizable single-CPU deadlock
  is an ISR acquiring a lock held by the interrupted code (the timer-tick cgroup
  path already uses `TABLE.try_lock` precisely to avoid this) or a recursive
  self-acquire — neither of which needs the AB-BA edge to be caught, only the
  stall detector, which now covers TABLE. So the pragmatic call is: **let the
  TABLE stall detector probe the next recurrence** before paying to instrument
  SCHED. If a recurrence stays silent (SCHED-side spin), revisit converting SCHED
  behind a benchmark and possibly a debug-only lockdep-on-SCHED build.

**Recurrence 2026-07-01 (embedded-DNS work, same signature).** During the
boot test for the container embedded-DNS increment, one run hung with no
BOOT_OK in 480 s; serial stopped mid-line at `[thread] Spawned thread (t`
immediately after `[cow] Cloned address space: parent=… -> child=…` and
`[sched] Spawned task 144` for a ring-3 clone (pid 177), with a burst of
page-cache faults for a glibc text inode just before — the exact
clone/CoW/thread-spawn signature documented above, and **no** watchdog dump
(BSP-stuck blind spot). The **immediately following** boot of the identical
binary reached BOOT_OK at 177 s with every self-test passing (including the
new `[cnetwork]   embedded DNS resolve: OK`). Confirms again the hang is in
the ring-3 `clone`/CoW-fault/thread-spawn path and is independent of the
touched code (this session changed only `cnetwork.rs`/`kshell.rs`, neither
on the boot spawn path). No new fix this session; datapoint logged.

**Recurrence 2026-07-01 (livelock-guard work, same signature).** While
boot-testing the new blind-spot-(1) livelock guard, one confirmation run hung
with no BOOT_OK in 480 s; serial stopped at `[spawn] Process 220 running
(thread 184, entry=0x4000000000, user_rsp=0x7fffffff0000)` — the container
`exec` self-test spawning ring-3 `/bin/hello` (task 184 in process 220),
immediately after `[thread] Spawned thread (task 184)`. Same
clone/thread-spawn fingerprint, and **no watchdog dump at all** (BSP-dead
blind spot 2). This is the *variant the new guard does NOT catch* — the guard
targets busy-livelock (blind spot 1); this is the IF=0 BSP-dead case that
still needs the NMI detector. Confirmed unrelated to the change: the new
`test_liveness_watchdog` self-test logged `[sched]   liveness watchdog: OK`
long before the hang, and the immediately-prior boot of near-identical code
reached BOOT_OK in 191 s. Datapoint logged; underscores that blind spot 2
(NMI detector) is the remaining high-value work on this bug.

**Clean datapoints 2026-07-02 (TD31 symmetric-accounting landed — *added*
CGROUP TABLE traffic to spawn/reap).** Landing the TD31 attach-on-spawn change
(commit `51c4033ef`) adds one `cgroup::attach_task` (TABLE lock) per task spawn,
on top of the detach-per-reap already present — i.e. it re-introduces exactly the
kind of extra TABLE traffic that, in the *original* TD31 attempt (pre
B-PREEMPT-SPINLOCK fix), made this hang near-deterministic and hung the boot
twice. With B-PREEMPT-SPINLOCK now fixed and CGROUP TABLE now a *tracked*
`crate::sync::Mutex`, the change booted **green 4× consecutively** (190/182/181/
185 s), **zero** `[liveness]`/`SPINLOCK STALL`/self-test-failure lines and no
`dash`/`pthread` flake. This is strong evidence the preempt-disable fix cured (or
at least drastically reduced) the TABLE-traffic-sensitive variant of this hang —
the added traffic that used to make it ~deterministic no longer reproduces it. A
follow-up 15-boot `hang-repro-loop.sh` soak on the TD31 binary is running to
gather more evidence (the now-tracked CGROUP lock means a TABLE-side deadlock
would finally produce a `SPINLOCK STALL` dump rather than silence). The genuinely
*total-silence* BSP-dead variant (blind spot 2) still needs the operator-gated
i6300esb/NMI detector (Q20) to be caught if it recurs.

**Blind spot (2) NMI hard-lockup detector — IMPLEMENTED 2026-07-02 (the
operator authorized option D — root-cause this hang — which unblocked the
i6300esb build previously gated behind Q20).** New `kernel/src/hardlockup.rs`
drives the QEMU i6300esb watchdog (PCI `0x8086:0x25ab`): maps BAR0 NO_CACHE,
programs a two-stage ~9.8 s countdown (1 kHz mode, `STAGE_PRELOAD`=5000 ≈
4915 ms/stage) with the reboot action left enabled (QEMU's inverted
`ESB_WDT_REBOOT` logic — bit clear = action armed), which `-action
watchdog=inject-nmi` routes to an injected NMI. `arm`/`kick`/`disarm`/`is_armed`
API. The **BSP** `timer_tick` (`sched/mod.rs`, `cpu==0`) kicks it every tick, so
while the BSP takes timer interrupts it never expires; if the BSP wedges with
IF=0 the kicks stop and QEMU broadcasts an NMI to every CPU — the wedged BSP
takes it *despite* IF=0. `handle_nmi` (`idt.rs`), when `hardlockup::is_armed()`
and the NMI has no port-0x61 hardware-error bits, prints `[hardlockup] NMI
WATCHDOG FIRED cpu=… rip=… cs=… rflags=…` for every CPU (the BSP's line is the
prize — the wedge RIP we could never observe) and the first arriver dumps the
full task table (one-shot latch). Armed at `main.rs` right after `liveness_arm`
(before the ring-3 container self-tests), disarmed at BOOT_OK. The device is
**opt-in** via `boot-test.sh --hard-lockup-watchdog` (already present, off by
default), so a normal boot finds no device and every entry point is a cheap
no-op — **zero blast radius on ordinary boots** (this resolves the shared-harness
blast-radius caveat that gated the build). Uses `ist=0` for v1 (the wedge is an
ISR spin with the stack intact). Verified: a clean `--hard-lockup-watchdog` boot
arms (~4915 ms/stage), disarms at BOOT_OK, reaches BOOT_OK in 172 s with **no
false-fire**. `hang-repro-loop.sh` now boots with the watchdog and treats
`[hardlockup] NMI WATCHDOG FIRED` as a catch. A soak with the instrument is
running to capture the wedge RIP; once captured, the RIP + task-table dump turn
this heisenbug into a directly-diagnosable one. **This is the tool that finally
makes blind spot 2 observable.**

**Fire path validated & width-bug fixed 2026-07-02.** An early deliberate-fire
self-test (`hardlockup::self_test_fire`: arm, then spin `IF=0` without kicking
for ~15 s) initially FAILED — the counter never started, so no NMI. Root cause:
QEMU's `i6300esb_config_write` decodes the *access width* — it only handles the
CONFIG register (0x60) on a 2-byte write and the LOCK register (0x68) on a
1-byte write — but `pci::config_write16` always emits a 32-bit `outl`
(read-modify-write, len==4). Both the CONFIG program and the ENABLE bit fell
through to default config storage, so `i6300esb_restart_timer` never ran. Fixed
by adding true-width `pci::config_write8` (byte access to data-port lane
`0xCFC + (offset&3)`) and `pci::config_write16_native` (`outw` to
`0xCFC + (offset&2)`), used for LOCK and CONFIG respectively (commit
`d0b6e648c`). Re-validated: with the fix the self-test PASSES — QEMU injects an
NMI ~10 s into the `IF=0` spin, `handle_nmi` catches it despite `IF=0`, resolves
`rip=kernel::cpu::delay_us` (exactly the spin), and dumps the task table. The
instrument is now proven end-to-end; the temp self-test call was reverted before
committing.

**Wedge window narrowed from the newest catch (2026-07-01 tee-session).** That
total-silence hang's last two serial lines were `[thread] Process 210 has no
threads left — now zombie` (`proc/thread.rs:445`) then `[sched] Task 176 exiting`
(`sched/mod.rs:1213`), then nothing. So the BSP wedges in the *tail of
`task_exit`*, after that print: `notify_exit_hooks(current_id)` (exit hooks run
lock-free) → `SCHED.lock()` to set `Dead` → `schedule_inner(false, Uncounted)`
(the context switch, which runs with IF=0). The dead-BSP/IF=0 fingerprint points
at the switch itself or a lock taken in an exit hook. The armed NMI soak will
resolve *which* by giving the exact wedge RIP; no further static speculation
until the catch lands.

**ROOT-CAUSED & FIXED 2026-07-02 — it was a false-positive watchdog trip on a
multi-second IF=0 SHA-256, NOT a deadlock.** The armed NMI soak caught the wedge
on the first iteration, and the new RBP-chain backtrace in `handle_nmi`
(`idt.rs::dump_kernel_backtrace`) resolved the exact call chain:
```
kmain → kernel_main → proc::spawn::self_test_linux_real_glibc_full
  → fs::vfs::Vfs::write_file → write_file_resolved
    → fs::history::try_auto_record → record_version
      → fs::cas::put → crypto::sha256 → crypto::Sha256::update  (rip in rotate_right)
```
The NMI fired at `rflags=0x10002` (**IF=0**) right as the glibc-full self-test
began staging its files, and — decisively — the serial log **continues past the
NMI dump to `BOOT_OK`**, so the machine was never actually deadlocked. What
happened: file-history auto-versioning was **on by default** (`fs::history`
static `HISTORY` had `auto_version: true`), so every boot-time overwrite of an
OS system file (the glibc tree, staged for the Path Z self-tests) made
`record_version` read the *old* content and SHA-256-hash it via `cas::put`.
Crucially, the entire Path Z self-test block runs **before** "Step 21: Enable
hardware interrupts" (`main.rs` `cpu::sti()`), i.e. with **IF=0**. In a debug
(unoptimised) build, hashing a multi-megabyte glibc file takes several seconds;
with IF=0 the BSP takes no timer ticks, so the timer-driven hard-lockup watchdog
kick (`sched::timer_tick` → `hardlockup::kick`, BSP-only) is starved. Under
host-scheduling jitter the ~9.8 s watchdog occasionally expired mid-hash,
producing the intermittent "BSP-dead total-silence" fingerprint. It presented as
a ~5% *hang* rather than 100% because the hash time sits near the watchdog
threshold / the soak-harness boot timeout, and only the jitter tail crosses it.

**Fix (proper, targeted):** file-history auto-versioning now starts **disabled**
and is enabled only at `BOOT_OK` (`main.rs`, right after `hardlockup::disarm()`,
via `fs::history::set_auto_version(true)`). Rationale: versioning OS files as
they are staged during boot is pointless (nobody rolls them back) *and* running
a seconds-long SHA-256 with IF=0 is the "long operation under IRQs-disabled"
anti-pattern regardless of the watchdog. Past BOOT_OK the BSP is preemptible
(IF=1) and OS staging is done, so auto-versioning real user-data writes is safe.
The history self-test is unaffected — it calls `record_version()` explicitly on
`/tmp` paths (which `should_auto_version` skips), independent of the flag.
Follow-up perf note logged separately: auto-versioning being globally on means
every user-data overwrite pays a read+rehash tax; capping by size or making it
truly opt-in per-path (per the module's own "opt-in" design statement) is a
worthwhile future optimisation, but it no longer gates boot liveness.

**STRUCTURAL ROOT FIX 2026-07-02 — enable interrupts BEFORE the ring-3 self-test
battery (RESOLVED).** Deferring auto-versioning (offender #1) did *not* stop the
watchdog fires: the armed NMI soak caught a second, independent offender on the
first iteration — a ring-0 (`cs=0x8`) IF=0 page fault resolved through
`try_resolve_fault → resolve_subpaged_fault → fs::handle::read_at → drop(Vec) →
slab_dealloc → mm::heap::poison_free`, RIP in the debug per-byte overflow
precondition-check inside the poison loop (`rflags=0x10002`, IF=0, task tid≈133
"dash-redir"), and — like offender #1 — the log **continued past the NMI dump to
BOOT_OK**, i.e. another false-positive on slow-but-live IF=0 work. Two
independent offenders in the same window meant fixing them one at a time was
band-aid accumulation (CLAUDE.md: "if you find yourself patching around the same
issue in multiple places, stop; redesign the underlying system").

The underlying system: `main.rs` deferred `cpu::sti()` until *after* the entire
ring-3 integration self-test battery (dozens of real Linux-ABI processes — glibc,
dash, gcc/make — that fork, CoW-clone, exec, demand-page file-backed mappings),
so the whole battery ran with **IF=0**. That is the "long operation under
IRQs-disabled" anti-pattern: no timer ticks → no preemption, the timer-driven
liveness/hung-task watchdogs are blind, and the BSP-only hard-lockup kick
(`sched::timer_tick → hardlockup::kick`) is starved. In a debug build (heap
poisoning on) the battery's O(n)-over-large-data ops are seconds-long, so
host-scheduling jitter occasionally pushed a slow-but-live boot across the ~9.8 s
watchdog / harness-timeout threshold → the intermittent "BSP-dead total-silence"
fingerprint (~5%).

**Fix (commit `c596b2fcc`):** move the Step-21 interrupt enable
(`idt::init_irq_stack(0)` + `cpu::sti()` + APIC-timer verification) from *after*
the battery to the init/test seam, immediately **before** the first ring-3 spawn
self-test (`main.rs`, right after the fs/blkdev self-tests, before
`self_test_linux_dynamic_interp`). The battery now runs the way userspace
actually runs — interrupts on, preemption live. The two validations that must
follow interrupt-enable but need not precede the battery (`sleep_ns`, `softirq`)
stay at the tail of boot. Results: a clean boot reaches **BOOT_OK in 91 s** (vs
the historical 161–229 s — ~2× faster, because ring-3 children now get
timer-driven CPU + interrupt-driven I/O completion instead of cooperative
`yield_now`-only slices), and the seconds-long IF=0 offenders are gone by
construction (they run with IF=1, so the timer keeps kicking the watchdog).

**Bonus:** the timer-driven liveness / hung-task watchdogs are now **live during
the battery**, so if a *genuine* clone/CoW/reap deadlock (the still-unproven
phenomenon #2 — the 480 s no-BOOT_OK total hang seen historically) ever recurs,
it will now produce a `[liveness] SYSTEM HANG` task-table dump instead of silence,
rather than being masked by the non-preemptive cooperative driver. If that dump
ever lands, root-cause the named lost thread's wait state. Until then this bug is
downgraded from the ~5% intermittent hang to RESOLVED for the false-positive
class; a 20-boot watchdog-armed soak is validating no NMI false-fire recurs.

**FOLLOW-UP STRUCTURAL FIX 2026-07-02 — make page-fault resolution preemptible
(the residual single IF=0 window).** The battery-wide reorder above eliminated
the *seconds-long* IF=0 offenders, but a fresh 20-boot armed soak still caught
one NMI false-fire on iteration 1 (still recovered → BOOT_OK, `ctx_switches=688`
`heartbeat=1011`, so preemption was confirmed live). The NMI RIP resolved to
`resolve_subpaged_fault::closure` behind an `isr_page_fault` asm boundary —
i.e. the residual IF=0 window is a *single* page fault, not the battery. Root
cause: **#PF is an interrupt gate (IDT type 0xE), so `handle_page_fault` ran
with IF=0 for its entire duration.** A single fault can be long — demand-paging
a subpaged file frame reads up to 16 KiB through the VFS, CoW/large copies touch
many pages, and debug heap poisoning makes every alloc/free O(size) per-byte —
so one slow fault could still hold IF=0 past the ~9.8 s threshold even with the
rest of the battery preemptible. Holding IF=0 across that I/O-bound work is the
same "long operation under IRQs-disabled" anti-pattern, just narrowed to one
handler invocation.

**Fix (`kernel/src/idt.rs` `handle_page_fault`):** mirror Linux `do_page_fault`
— capture CR2 first (so a nested fault can't clobber it), then `cpu::sti()`
*only when the faulting context's saved `RFLAGS.IF` was set*. Faults from an
already-IF=0 context (ISR, scheduler, cli/raw-spin critical section) keep
interrupts disabled, so we never widen interruptibility beyond what the
interrupted code allowed. Now the timer keeps ticking (preemption + watchdog
kick + liveness heartbeat) across even a long demand-paging/CoW fault, closing
the residual IF=0 window by construction. A 20-boot watchdog-armed soak is
validating no NMI false-fire recurs.

**REPRODUCED AS A FULL HANG 2026-07-02 — ping-pong livelock in the dash-redir
ring-3 test (a liveness-watchdog blind spot).** The post-§56 armed soak caught a
*total* boot hang on iteration 1: serial froze mid-`spawn_process` for the
`spawn-test-dash-redir` child (`echo > file` redirection test) — last line
`[thread] Spawned thread (task 133) in process …`, process 167 — and stayed
silent for 6+ min with **no** NMI dump and **no** `[liveness] SYSTEM HANG` dump,
until the 480 s harness timeout. Diagnosis:
- **Not caused by §56.** The #PF `sti()` cannot cause a lock-held context switch:
  timer-driven preemption defers when `preempt_count > 0` and refuses to re-enter
  `SCHED` (`sched/mod.rs` ~L2342/2348), and tracked-lock ISRs use `try_lock`
  (`sched/mod.rs` L355). So enabling interrupts mid-fault is within the existing
  concurrency contract. This is the pre-existing dash-redir / ring-3 reap-futex
  race (same family as B-DASH-STDIN-FLAKE), now manifesting as a *hang* instead
  of a fast `InternalError` because live preemption (§55) changed the timing.
- **Why neither watchdog fired.** The hard-lockup NMI needs IF=0 on cpu0 — but the
  driver's `yield_now` loop re-enables IF between yields, so cpu0's `timer_tick`
  keeps running (kicks hardlockup → no NMI). `liveness_check` runs *directly* from
  `timer_tick` (L2165), so it *did* run every 5 s — but its two detectors are
  blind here: the total-hang path needs `useful_work` frozen and the busy-livelock
  path needs `ctx_switches` frozen, yet in a ping-pong livelock (driver re-schedules
  the deadlocked child, child runs briefly and blocks, repeat) **both** counters
  keep advancing, so neither trips.

**Instrumentation fix (`sched/mod.rs` `liveness_check`):** added a purely
time-based **boot-deadline backstop** — `LIVENESS_BOOT_DEADLINE_INTERVALS = 60`
(× 5 s = 300 s from arming). A healthy boot disarms at BOOT_OK ~91 s after arming
(>3× headroom, no false-fire risk), so if the watchdog is still armed 300 s after
arming it dumps the full task table once (`[liveness] BOOT DEADLINE EXCEEDED`).
This catches *any* hang mode — total, busy-livelock, or ping-pong livelock — that
the progress-based detectors miss, giving the task-state breadcrumb needed to
root-cause the dash-redir reap deadlock. Next armed soak should capture the dump;
root-cause the named stuck task's wait state (child `blocked_on` / driver state)
from it.

**HARD-LOCKUP WATCHDOG NMIs ARE TCG FALSE POSITIVES — ROOT-CAUSED 2026-07-02.**
_(Correcting an earlier note in this file that wrongly attributed the watchdog
trips to `task_list()`-on-exit. The `task_list` change below is kept as a real
optimization, but it was **not** the cause of the NMIs.)_ Two consecutive armed
`--hard-lockup-watchdog` catches (offender #3: `rip=0xffffffff814decc9`; offender
#4 / `build/hang-catches/CAUGHT-iter-1-hardlockup.txt`: `rip=0xffffffff80fc4248`,
in `Vec<u8>::drop` during the glibc-staging self-test) both fired the NMI with
`rflags` showing **IF=1**, in heavy debug-build compute, holding no
interrupt-disabling lock, and **both recovered to BOOT_OK**. That is decisive:
`hardlockup::kick()` sits at the *top* of `timer_tick` on cpu0, *before* any lock
acquisition, so a live-and-ticking BSP always kicks — an NMI that fires while the
BSP is demonstrably still executing `timer_tick`-eligible code and then recovers
cannot be a genuine `IF=0` wedge. These are **spurious NMIs from QEMU/TCG
virtual-clock-vs-APIC-timer divergence** during heavy debug-build compute bursts
(the poison allocator makes `O(size)` drops multi-second, and the i6300esb counts
in QEMU_CLOCK_VIRTUAL): the APIC timer that should keep kicking gets starved of
TCG translation-block boundaries relative to the watchdog's virtual clock, so the
countdown expires even though the BSP is fine. The genuine bug (offender #2) is a
*permanent* wedge (480 s, never reaches BOOT_OK); it was never one of these
catches — the spurious NMIs kept ending the soak before it could reproduce.

**Proper structural fix (this commit) — heartbeat-progress NMI discriminator.**
Per CLAUDE.md's anti-band-aid rule, rather than keep chasing individual "offender"
RIPs (each a red herring), the NMI handler now *distinguishes* a real wedge from a
spurious NMI instead of treating every watchdog NMI as a catch:
- `sched::bsp_heartbeat()` reads `WATCHDOG_HEARTBEAT[0]`, bumped every BSP
  `timer_tick` (NMI-safe: one relaxed atomic load).
- `hardlockup::classify_nmi(hb)` swaps `hb` into a `PREV_NMI_HEARTBEAT` baseline
  (reset to a sentinel in `arm()`). First NMI since arming → benefit of the doubt
  (spurious). Subsequent NMI whose heartbeat advanced `< ALIVE_TICKS` (=4) since
  the previous NMI → **real wedge** (a spin with `IF=0` freezes `timer_tick`, so
  the delta is exactly 0); advance ≥ 4 → spurious (live-but-busy BSP advances the
  heartbeat by hundreds per ~9.8 s window).
- `idt::handle_nmi` (armed branch): only **cpu0** classifies/acts (the watchdog is
  driven solely by the cpu0 kick, and `classify_nmi`'s swap must run exactly once
  per event); APs print a non-greppable info line and return. On a **real** verdict
  cpu0 emits the greppable `NMI WATCHDOG FIRED` marker + one-shot backtrace/task
  dump. On a **spurious** verdict it prints a distinct `spurious NMI … re-kicking`
  line, re-kicks, and resumes — no latch, no false catch.
This catches a genuine BSP-dead wedge on the *second* NMI (~20 s) instead of the
480 s liveness timeout, and — crucially — lets the soak run *past* the spurious
NMIs so offender #2 can finally reproduce. Builds clean, 0 new clippy warnings.

**Kept optimization (commits `acf9da4f9`, `d2da77e5c`):** `pacct::on_task_exit`
and `procfs::task_exists` no longer call `sched::task_list()` (which builds a heap
`Vec` of *all* tasks and volatile-scans every stack under SCHED just to find/test
one task). Added:
- `sched::task_info(task_id) -> Option<TaskInfo>` — one `tasks.get(&id)`, skips the
  stack scan (`stack_used`/`stack_pct` = `None`). Used by `pacct::on_task_exit`.
- `sched::task_exists(task_id) -> bool` — a `tasks.contains_key(&id)`. Used by
  `procfs::task_exists` (~14 pid-validation sites).
These are genuinely wasteful patterns worth removing on their own merits (a map
lookup holds SCHED for microseconds), but they were **not** the watchdog cause.
The genuine *never-recovers* dash-redir ping-pong livelock (offender #2, the 480 s
no-BOOT_OK case) is still open; the discriminator above plus the boot-deadline
backstop will capture its task dump on the next reproduction.

**CORRECTION 2026-07-03 (later the same day): the IRQ-stack fix below is REAL but
is NOT the (only) cause of this intermittent hang — there are (at least) TWO
distinct wedges, and the DOMINANT one is still open.** A 30-boot armed soak run
*after* the IRQ-stack fix reproduced a hang on **boot 1**, but with a completely
different signature: `[liveness] SYSTEM HANG: no task-level forward progress …
all CPUs idle-ticking`, **heartbeat still advancing** (so cpu0 is NOT wedged with
IF=0 — this is not the IRQ-stack overflow). The task table showed a **container
exec of `/bin/hello` (pid 220, task 184, inode 72)** marked `state=Running` on
cpu0 while the CPU idle-ticks, having **never executed a single instruction** (zero
page faults for its entry `0x4000000000`, no output). Saved:
`build/hang-catches/CAUGHT-iter-1-liveness.txt` /
`CAUGHT-iter14-liveness-lostwakeup.txt`. The prior session's `healthy-serial.txt`
froze on the **same inode 72** (`/bin/hello`) mid page-cache-map — so this
container-exec dispatch/wakeup hang is the recurring dominant failure and it
**predates** the IRQ-stack fix (my fix did not introduce it, nor cure it). This is
the genuine lost-wakeup / failed-dispatch race (B-PTHREAD-YIELDBUDGET /
B-DASH-STDIN-FLAKE family): a container-exec'd task is left `Running`/current on an
idle CPU. **STILL OPEN — root-cause the container exec dispatch path next.** The
IRQ-stack fix remains committed on its own merits (unbounded nesting *will*
overflow under a slow-enough handler; it was one genuine wedge — the
`CAUGHT-iter-2-nobootok` IF=0 guard-page `#PF`).

**OCCURRENCE 2026-07-14 (two back-to-back boots during Q18/§59 virtio-gpu work).**
Two consecutive `boot-test.sh` runs both timed out at `BOOT_OK not found within
480s`, but at **different, non-deterministic points**: run 1 froze at **process
211** (a `/lib64/ld-linux-x86-64.so.2` interpreter exec), run 2 froze at
**process 226** — the last serial line cut off mid-write `[spawn] Process 226
running (thread 190, e`, immediately after the container-exec sub-tests passed
(`[container] exec + wait (exec_path/wait_process): OK`), on a plain
`entry=0x4000000000` `/bin/hello`-style spawn. Total silence after, heartbeat
family (same lost-wakeup / failed-dispatch signature above). The **moving hang
location run-to-run** is the definitive tell that this is the timing-dependent
race, not a code regression: the Q18 change under test (virtio-gpu GETPARAM
render ioctl) runs *far earlier* at process 146 and **passed cleanly in both
runs** (`renderD128 GETPARAM(3D_FEATURES)==0, honest no-3D reporting: OK`), with
boot progressing hundreds of processes past it each time. Q18 committed on this
basis. **STILL OPEN — root-cause the container-exec / ring-3 spawn-dispatch race.**

**OCCURRENCE 2026-07-14 (netstack Phase 4 increment 5, UDP-exchange-over-IPC).**
One `boot-test.sh --no-build` run timed out at `BOOT_OK not found within 480s`
with the same signature: `[liveness] SYSTEM HANG: no task-level forward progress
for 15+ seconds (useful_work=13, all CPUs idle-ticking)`, cpu0 heartbeat still
advancing (2501), the current task `tid=0 name="prctl-batch269"` `state=Running`
`last_rip` in `kernel_text`. QEMU also printed a one-off `Incorrect order for
descriptors` (virtio) on stderr. Boot had progressed to ~line 4147/4175 (~99%),
well past the netstack self-tests, which **all passed cleanly** (A resolve, PTR
`dns.google`, TCP `HTTP/1.1 200 OK`, and the new UDP-exchange DNS datagram — all
OK at serial lines 1822–1831). An **immediate re-run passed in 88s** with every
netstack op OK and no hang/virtio error — the definitive tell of the timing race,
not a regression from the UDP-exchange change. Increment 5 committed on this
basis. **STILL OPEN.**

**IRQ-stack overflow wedge (one of the two) — ROOT-CAUSED AND FIXED 2026-07-03.**
The
first-NMI one-shot backtrace (added to `idt.rs::handle_nmi` this session so a
genuine wedge dumps its stack regardless of the spurious/real classification)
finally caught the real wedge: `build/hang-catches/CAUGHT-iter-2-nobootok.txt`.
Decisive evidence:
- First NMI at `rip=0xffffffff80083956`, **`cs=0x8` (ring 0), `rflags=0x10002`
  (IF=0)**, `rsp=0xffffffff…27a80` — i.e. cpu0 wedged in the kernel with
  interrupts off, in Task 0 `"prctl-batch269"`.
- The rbp chain + stack scan showed the LAPIC timer handler recursively nested on
  the per-CPU IRQ stack: the cycle `isr_timer → irq_common_dispatch →
  run_on_irq_stack → dispatch_vector → handle_timer_irq → timer_tick →
  liveness_boot_deadline_check → clock_monotonic → tsc_freq` repeats many times,
  under a task doing `spawn_process → load_interpreter → read_file → …read_through
  → get_or_fill → fill_file_page → MemFs::read_at → touch_accessed_relatime →
  metadata_now_ns → clock_realtime`.
- It ended with `[fault] Guard page hit at 0xffffc10000028000 — stack overflow`,
  `EXCEPTION: Page Fault (#PF) … address=0xffffc10000028000, error=0x0`, Task 0
  `"prctl-batch269"`, `FATAL: Unrecoverable kernel page fault. Halting.` The IRQ
  stack is exactly `0xffffc10000024000..0xffffc10000028000` (16 KiB, guard at
  `0x28000`).

**Mechanism.** `handle_timer_irq` (apic.rs) re-enables interrupts *while still
running on the IRQ stack* — once inside `softirq::process_pending` (its internal
`STI`) and once via an explicit `sti` before the deferred-preempt check. The
softirq layer's `IN_SOFTIRQ` re-entry guard bounds softirq *work*, but NOT the raw
interrupt re-enable. So whenever a timer handler takes longer than the ~10 ms tick
period — trivially true in the **poison-debug build**, where the poison allocator
makes `O(size)` heap ops multi-second and every file-page read does a
`relatime → clock_monotonic → tsc_freq` clock call — the next timer IRQ fires while
the previous handler is still on the IRQ stack, nests (grows *down* the same stack
via `irq_common_dispatch`'s nested-IRQ branch), re-enables interrupts again, and so
on. Depth grows without bound until the 16 KiB IRQ stack overflows its guard page →
fatal kernel `#PF`. This is a *uniprocessor* bug (QEMU boots 1 CPU here), which is
why "SMP timing race" framings never panned out. It is the same B-DF1 IRQ-stack
design (Q7 option A) whose own note (below) warned *"A correct IRQ-stack
implementation must therefore support nesting (or …)"* — nesting was supported but
never *bounded*.

**Structural fix (commit this session; `apic.rs` + `cputime.rs`).** Only the
**outermost** timer handler may re-enable interrupts. `cputime` already keeps a
per-CPU hardirq nesting depth (`irq_depth`, bumped in `enter_irq`); a new
`cputime::irq_depth()` accessor exposes it, and `handle_timer_irq` computes
`let nested = cputime::irq_depth() > 1;` right after `enter_irq()`. When `nested`,
it **skips `process_pending`** and **skips the explicit pre-preempt `sti`**, so the
nested handler runs its entire body with IF=0. Because the timer IDT entry is an
**interrupt gate** (type `0x0E` → IF auto-cleared on entry) and the nested handler
never sets IF back, *no further timer can fire until the nested frame returns* —
hard-capping timer-on-timer nesting at **depth 2** regardless of how slow any
single handler is. Softirq bits raised by a nested tick are drained by the outer
frame's own `process_pending` loop (identical to the `IN_SOFTIRQ` short-circuit,
but without ever toggling IF); preemption is unaffected (nested IRQs never run
`do_deferred_preempt` anyway — the outermost frame owns it). Builds clean, 0 new
clippy warnings. NOTE: the post-fix soak did NOT reproduce the IRQ-stack overflow
again, but it DID reproduce the *other* (dominant) wedge — the container-exec
lost-wakeup described in the CORRECTION note above — so this fix cannot be
soak-"verified" in isolation until that second wedge is also fixed. It stands on
its analytical merits (bounded nesting by construction) plus the absence of any
further IF=0 guard-page `#PF`.

**NMI WATCHDOG BLIND-SPOT — ROOT-CAUSED AND FIXED 2026-07-03 (why the dominant
wedge escaped with *zero* catchable NMIs).** After the IRQ-stack fix, three more
armed soaks (`CAUGHT-iter-2-nobootok` make-cc pid 210 inode 126; a tcc-hosted
catch pid 214; `soak5` `CAUGHT-iter-1-nobootok` pid 176 **inode 72** — a
*different* binary again) all reproduced the dominant wedge as a `nobootok` with
**no watchdog dump at all**, running silently to the 480 s harness kill. Decisive
observations from those catches:
- **The wedge is an IF=0 total-silence spin on cpu0** — the last serial line is
  always a page-fault the handler *completes* (`[fault] … mapped/Demand-paged …`)
  right as a freshly `exec`'d ld.so-linked Linux binary is demand-paging its early
  pages, then nothing. `liveness_boot_deadline_check` emits a 30 s breadcrumb every
  BSP tick while armed, and **zero breadcrumbs** appear after the wedge → the timer
  IRQ stopped → cpu0 is spinning with IF=0 (only an NMI can preempt it).
- **It is NOT make+tcc-specific.** Catches span inode 126 (tcc), inode 72
  (`/bin/hello`-class), and make grandchildren — i.e. the common factor is
  *spawning/exec'ing an ld.so-linked Linux binary and demand-paging it*, not any
  one test. (Consequently the per-reap-loop `dump_task_table` instrumentation added
  earlier this session **cannot** observe this wedge: the reap loop runs on the same
  wedged cpu0 and is starved too. Only the NMI path can catch it.)
- **Why the NMI watchdog stayed silent.** Two compounding defects in the *diagnostic
  instrument* (not the bug itself): (1) the old `classify_nmi` compared the BSP
  heartbeat between *consecutive* NMIs against a `PREV_NMI_HEARTBEAT` baseline. A
  **mid-boot spurious TCG NMI** (seen in `CAUGHT-iter-2` at `heartbeat=997` during
  the dash-test compute burst) set that baseline to 997; minutes later the wedge
  froze the heartbeat at a large value H, so the wedge's first NMI saw `H − 997`
  (huge) and was dismissed as spurious. Catching then depended on a *second* wedge
  NMI (delta 0), which the QEMU i6300esb did not reliably re-inject after the first
  fire → no catch, ever. (2) That same mid-boot spurious NMI consumed the *one-shot*
  `HARDLOCKUP_DUMPED` latch, so even if the wedge had been classified real, the
  backtrace/task-table dump was already spent.

**Fix (commit this session; `hardlockup.rs` + `idt.rs`).** Replace the fragile
across-NMI heartbeat-delta classifier with a **self-contained monotonic
kick-staleness** check that fires on the wedge's *first* NMI, immune to any stale
baseline:
- `hardlockup::kick()` (called at the top of the BSP `timer_tick`) now stamps
  `LAST_KICK_NS = clock_monotonic()` — a direct "when did the BSP timer last tick?"
  clock. `clock_monotonic` is a pure `rdtsc` + relaxed loads, so it advances even
  with IF=0 and is NMI-safe.
- `classify_nmi()` (no args) returns real iff `clock_monotonic() − LAST_KICK_NS ≥
  WEDGE_STALE_NS` (2 s). A live BSP kicks every ~10 ms → staleness ≪ 1 s → spurious;
  a real wedge stopped kicking → by the ~9.8 s hardware fire the stamp is ~9.8 s
  stale → real, on the *first* NMI. The old `PREV_NMI_HEARTBEAT`/`ALIVE_TICKS`
  baseline machinery is removed.
- `idt::handle_nmi` now, on a **real** verdict, dumps the backtrace + task table
  **unconditionally** (ignoring the one-shot latch) so a prior spurious NMI can no
  longer rob the real wedge of its stack trace; it logs `kick_stale_ns` for
  confirmation. Spurious NMIs still take a one-shot early dump, re-kick, and resume.
Builds clean, 0 new clippy warnings. This makes the dominant wedge **observable**:
the next armed soak should finally print `NMI WATCHDOG FIRED … rip=…` + backtrace
pinpointing where the freshly-exec'd binary's demand-paging path spins with IF=0.
**Still OPEN** (the underlying wedge) — but no longer a blind heisenbug.

**REGISTER-VS-RUNNABLE RACE — ROOT-CAUSED AND FIXED 2026-07-03 (the yield-budget
PANIC variant; the silent IF=0 wedge is a SEPARATE bug, still open).** The reset
experiment (`scripts/wdog-reset-experiment.sh`, `WATCHDOG_ACTION=reset`) caught a
*non-silent* member of this family: `build/hang-catches/RESET-CAUGHT-iter-2.txt`
— a fatal kernel PANIC at `container.rs:5370` `assert!(zombified, "exec'd hello
did not exit within the yield budget")`. Decisive serial evidence (lines
9119–9123): `[sched] Task 184 exiting` printed **before** `[sched] Spawned task
184 …` and `[thread] Spawned thread (task 184) in process 220`, and **no**
`[thread] Process 220 … now zombie` line ever appeared. I.e. the exec'd
`/bin/hello` child ran to completion *before* its owning process/thread were
registered, so the process was never zombified and the container self-test spun
its 100 000-yield budget and fired the assert.

*Mechanism (a classic register-vs-runnable race):* `thread::spawn`
(`proc/thread.rs`) created the scheduler task via `sched::spawn`, which enqueues
it **Ready and runnable and re-enables interrupts** (`without_interrupts` ends)
*before* `thread::spawn` did `pcb::add_thread` + the `THREAD_OWNERS.insert`. On
the uniprocessor a timer preemption in that window switches to the short-lived
child, which prints and `exit()`s; `on_thread_exit` (`thread.rs:396`) then does
`owners.remove(&task_id)?` → `None`, bails, and **skips the process's zombie
transition entirely**. (The out-of-order serial — child exit logged before its
own spawn/registration logs — is the exact fingerprint of this window.)

*Proper structural fix (commit this session; `sched/mod.rs` + `proc/thread.rs`),
SMP-correct — not a widened `without_interrupts` window:*
- `sched::spawn_suspended()` creates the task **Blocked and NOT enqueued** (and
  does not signal a CPU), sharing a new `spawn_inner(…, admit: bool)` with the
  normal immediate-admit `spawn`/`spawn_with_affinity`.
- `sched::admit()` (built on `wake()`) performs the Blocked→Ready transition and
  enqueue once the caller is ready.
- `thread::spawn` now: create the task **suspended**, complete **all** ownership
  registration (`add_thread` + `THREAD_OWNERS` insert + `Creating→Running`)
  *before* calling `admit()`. The child therefore cannot run until
  `on_thread_exit` is guaranteed to find its owning process. Includes an
  unwinding path (detach + kill) if `admit` ever fails.
Builds clean, 0 new clippy warnings in the changed files.

*Scope / what this does and does NOT fix.* This eliminates the **yield-budget
PANIC variant** (a task that *ran and exited* but left an un-zombified process).
It is analytically the same ordering hazard behind the "task `state=Running`,
never executed" liveness catch (`CAUGHT-iter-1-liveness.txt`), which the fix also
closes by construction (a task is registered before it is ever runnable). It does
**NOT** fix the **dominant silent IF=0 wedge**: a 40-boot `reset`-action soak of
the fixed kernel reproduced on **iteration 1** with a *different* signature —
`build/hang-catches/RESET-CAUGHT-iter-1.txt`: pid 188 heavily demand-paging inode
72 (`/bin/hello`) page-cache maps, then **total silence** at 47 s (no panic, no
assert, no yield-budget line), i.e. cpu0 spun with IF=0, the BSP stopped kicking,
and the i6300esb reset fired. That silent wedge is a separate mechanism (a
freshly-exec'd binary's demand-paging path spins with IF=0) and remains **OPEN** —
next step is the dedicated-NMI-IST work below so an `inject-nmi` soak can finally
dump its backtrace.

**ORPHANED-`Running` LOST-DISPATCH WEDGE — ROOT-CAUSED AND FIXED 2026-07-03 (the
BSP-*alive* lost-dispatch variant; distinct from the BSP-dead IF=0 spin above).**
The dedicated-NMI-IST + monotonic-kick-staleness instrument finally caught the
**BSP-alive** member of this family cleanly:
`build/hang-catches/NMI-NOBOOTOK-iter-2.txt`. Decisive evidence: right after
`[spawn] Process 220 running (thread 184 …)`, the box goes
`[liveness] SYSTEM HANG … all CPUs idle-ticking` with **cpu0's heartbeat still
advancing** (4251→4501 — the BSP is alive and idle-ticking, NOT wedged with IF=0,
so this is a *different* wedge from the silent-spin one), and the task dump shows
exactly three tasks:
- `tid=184 /bin/hello state=Running` — a **phantom**: never executed a single
  instruction (zero page faults for its entry `0x4000000000`, no output),
- `tid=183 hello-init state=Dead`,
- `tid=0 name="prctl-batch269" state=Ready` — the **idle/boot task, stranded Ready**.
Critically there is **no** `[sched] BUG: context switch failed` line → the orphaning
happened via the *silent* idle-fallback path, not the main dispatch path.

**Mechanism (the dispatch invariant was violable).** In `schedule_inner`,
`pick_next_local` **dequeues** the picked task, and the old code then marked it
`state=Running` **before** confirming *both* context-switch pointers
(`old_data` = outgoing/current task's saved-context slot, `new_data` = incoming
task's) were successfully extracted from the task table. When extraction failed —
`old_data` is `None` because the *current* task isn't found in `tasks` — the picked
task (184) was left **orphaned**: `state=Running`, **not** current on any CPU, and
**no longer in any run queue** (the dequeue already removed it). Nothing ever
re-enqueues a `Running` task: `check_starvation` only rescues `Ready` tasks (and
additionally skips `priority >= IDLE_PRIORITY`), so the run queue drains to empty →
every CPU HLTs forever. Because the idle/boot task (task 0) is itself only `Ready`
and stranded, it can never resume its yield loop → total hang. (This is the
BSP-alive twin of the RESET-CAUGHT yield-budget PANIC: there the driver *could*
resume and hit the `assert!(zombified)`; here it cannot resume at all.)

**Structural fix (commit this session; `sched/mod.rs`, both dispatch sites) —
restore the invariant "a task is marked `Running` only once its context switch is
committed":**
- **Idle-fallback path:** extract `old_data`/`new_data` **first**; if either is
  `None`, re-enqueue the picked task iff it is still `Ready` (`PER_CPU_SCHED.enqueue`
  with its effective priority), print `[sched] BUG: idle-fallback switch aborted …
  re-enqueued ready task N`, `drop(s)` and `continue` the fallback loop. Only when
  **both** are present is the picked task's `record_dispatch` + `state=Running` +
  `last_cpu` committed. The old trailing "context extraction failed" block is now
  unreachable (kept as a defensive no-op for the borrow checker).
- **Main path:** the pre-extraction `Running` mark was **removed**; the
  `record_dispatch`/`state=Running`/`last_cpu` write now lives **inside** the
  `if let (Some(old_data), Some(new_data)) = …` success branch. The `else` branch
  re-enqueues the picked task iff still `Ready` before returning, logging
  `[sched] BUG: context switch failed — task C or N not in table (re-enqueued ready
  task N)`.
Re-borrowing `tasks.get_mut(&picked)` to set `Running` after taking `old_data`'s
raw `&raw mut` context pointer is sound: raw pointers are not live borrows and no
map insert/remove occurs in between, so the pointer stays valid. Builds clean
(`cargo build -p kernel`, 50.6 s), 0 new clippy warnings in the edited range.

**What this fixes / what remains.** This eliminates the *total hang* from the
BSP-alive lost-dispatch: even when extraction fails, the picked task returns to the
run queue instead of vanishing, and the new `BUG:` logs will pinpoint **why**
`old_data`/the current task becomes `None` (the deeper trigger — how the *current*
task drops out of `tasks` mid-dispatch — is not yet definitively identified; static
analysis says the current task is reap-protected via `active_ids`, so the logs are
the next lead). **Also noted (not the forward-progress blocker):** the idle task
(task 0) being renamed to `"prctl-batch269"` by a userspace `PR_SET_NAME` implies
`current_task_id()` returned 0 while a userspace task ran (or the boot self-test
genuinely runs in task-0 context) — a cosmetic/desync concern flagged for later.
The **BSP-dead IF=0 silent-spin** variant (freshly-exec'd binary demand-paging with
IF=0) remains **OPEN** and is the next target once a soak captures its NMI backtrace.

**POST-ACCT-FIX SOAK OBSERVATIONS 2026-07-03 (this silent wedge recurs — NOT
caused by the ACCT `lock_irqsave` fix; two fresh data points).** After the
B-ACCT-SPINLOCK-STALL fix (`lock_irqsave`, commit `b267b5e6f`) landed and was
independently verified (a standalone boot reached BOOT_OK in ~80 s with **zero**
`ACCT` stall signatures), a `scripts/hang-repro-loop.sh` soak (`--no-build
--hard-lockup-watchdog`) reproduced this *pre-existing* silent total-hang on
iteration 1 in two consecutive runs, freezing at different points in the ring-3
glibc spawn/exec/reap battery: **soak-1 froze at pid 210, soak-2 at pid 155**
(catch preserved: `build/hang-catches/SPAWN-SLOW-soak2-pid155.txt`). Both are the
now-familiar **BSP-dead IF=0 silent-spin** fingerprint: cpu0 wedged with
interrupts disabled, the BSP timer stopped ticking, **no** `[liveness] SYSTEM
HANG` dump, **no** `[watchdog]`/`SPINLOCK STALL` line, and — critically — the
i6300esb NMI hard-lockup watchdog **did not fire** either, so no backtrace was
captured. Explicitly attributed to the pre-existing spawn-hang class above, **not**
to the ACCT fix: the ACCT fix *prevents* the recursion (IF=0 during the short leaf
hold blocks the re-entering interrupt) rather than silencing any symptom, and a
clean standalone boot passed after it, so it is not a regression source. The open
blocker is unchanged and now doubly-confirmed: **observability** — the NMI
watchdog does not fire on this particular IF=0 BSP wedge, so the next step is to
determine *why* the i6300esb → inject-NMI path fails to catch it (candidate: the
NMI IST/vector setup, or the kick stops but the injected NMI is masked/lost under
TCG in this specific spin state) before the actual spawn/exec/reap or
demand-paging spin can be root-caused.

**NMI DELIVERY VALIDATED + NEW HANG LOCUS FOUND 2026-07-03 (the observability
blocker above is narrower than thought).** Two decisive results this session:
1. *The injected-NMI → dump chain WORKS end-to-end under our exact TCG harness.*
   Temporarily wiring `hardlockup::self_test_fire()` (a deliberate ~15 s IF=0
   no-kick spin, reproducing the BSP-dead condition) into `main.rs` right after
   `hardlockup::arm()` and booting `--hard-lockup-watchdog` produced:
   `[hardlockup] NMI WATCHDOG FIRED cpu=0 rip=0xffffffff814dbbe1 … kick_stale_ns=9899867054`
   then `self-test-fire: PASS — NMI observed (fired 0 -> 1)`. So the i6300esb
   inject-nmi fires under TCG, the NMI IST2 is good, and the current
   monotonic-kick-staleness `classify_nmi` correctly returns REAL on the *first*
   NMI of a 9.9 s-stale wedge. This means the *older* silent catches (pid 210/155)
   were almost certainly on a kernel with the **pre-rewrite heartbeat-delta
   classifier** that misclassified the wedge NMI as spurious — not a delivery
   failure. (Probe reverted; kernel rebuilt clean.)
2. *A fresh silent catch on the CURRENT kernel froze in KERNEL space, not the
   ring-3 battery.* A bounded `--hard-lockup-watchdog` soak (`scripts/soak-nmi-check.sh`,
   150 s timeout) caught on iteration 1 (`build/hang-catches/SNMI-CAUGHT-1-silent.txt`,
   9340 lines): the last line is OCI self-test **Test 14** (`[oci]   metadata
   instructions (VOLUME/STOPSIGNAL/SHELL/ONBUILD): OK`, `oci.rs:4079`), i.e. it
   wedged in **Test 15 "multi-stage builds"** (`oci.rs:4082`), which does heavy
   VFS + block-I/O (`build_image`/`load_image`/`extract_layer`/`read_file`/`rmdir`).
   A single `[liveness] boot-window breadcrumb: 30s armed (…heartbeat=2398)` fired
   but **no 60 s breadcrumb, no NMI, no SYSTEM HANG** — the BSP tick went dark
   ~30 s into the armed window. This is a *different* locus from the ring-3
   spawn/exec/reap hangs, suggesting the hang family is a **shared lower-level
   primitive** (VFS path / block-device wait / a lock taken on both the OCI-build
   and ring-3-spawn paths), not something specific to `clone`/CoW.
   **Caveat / open:** the 150 s timeout may itself produce false "silent" catches
   (a slow-but-live boot cut off early). A 300 s-timeout re-soak is running to
   disambiguate: a real BSP-dead wedge will now fire the NMI (delivery proven), and
   a slow-but-live boot will either reach BOOT_OK or trip the 200 s-armed
   `[liveness] BOOT DEADLINE EXCEEDED` task dump. Result pending.

**ROOT-CAUSED + FIXED 2026-07-03: `TSC_FREQ` spinlock re-entry deadlock — this
was the silent BSP-dead wedge.** The HMP-monitor RIP capture (new tooling, see
below) caught a live wedge and, walking the frozen `RBP` chain, resolved it
exactly:
- **Frozen state:** `RIP=ffffffff800e1d46` = `spin_loop_hint+0x6` (spinning),
  `RFL` with `IF=0`, `CPL=0` (kernel), `CR2=0x60000c7800`.
- **Stack (RBP chain, innermost → outermost):**
  `tsc_freq ← clock_monotonic ← kick_staleness_ns ← handle_nmi`.
- **Mechanism (same class as B-ACCT-SPINLOCK-STALL below):** `bench::tsc_freq()`
  read the write-once calibrated TSC frequency through a `spin::Mutex<u64>`
  (`static TSC_FREQ: Mutex<u64>`). But `timekeeping::clock_monotonic()` calls
  `tsc_freq()`, and `clock_monotonic()` runs on the normal hot path **and** from
  timer-IRQ context (scheduler `bsp_heartbeat`) **and** from NMI context
  (`hardlockup::classify_nmi`/`kick_staleness_ns`). On the uniprocessor, if a
  timer IRQ or watchdog NMI fires while normal code is *inside*
  `TSC_FREQ.lock()`, the handler re-enters `clock_monotonic → tsc_freq →
  TSC_FREQ.lock()` and spins forever at `IF=0`. Silent BSP death, no ticks, all
  timer-driven watchdogs blind.
- **Why the NMI never dumped:** the watchdog NMI *was* delivered (`handle_nmi`
  is on the frozen stack — this **inverts** the earlier "NMI never taken"
  hypothesis above), but `classify_nmi`'s very first act is a
  `kick_staleness_ns()` → `clock_monotonic()` → `tsc_freq()` → the same
  `TSC_FREQ.lock()` that is *already held* by the interrupted normal-context
  code. The NMI self-deadlocks in the identical lock before it can print. That
  is why every catch was **silent** with zero watchdog output.
- **Fix (commit 5f658336c):** `TSC_FREQ: Mutex<u64>` → `AtomicU64`. The value is
  write-once at calibration and read-only forever after — it never needed a lock
  at all. `calibrate_tsc()` does `TSC_FREQ.store(freq, Relaxed)`; `tsc_freq()`
  does `TSC_FREQ.load(Relaxed)`. `clock_monotonic()` is now fully lock-free and
  genuinely IRQ/NMI-safe (its doc comment's "no locks" claim is finally true),
  and it's also faster on the hot clock path. This is the *proper* structural fix
  (lock-free for a write-once value), not a band-aid.
- **New tooling that caught it — HMP-monitor RIP capture.** No
  addr2line/llvm-symbolizer exists in any toolchain, and the in-guest NMI dump
  was itself deadlocked, so neither in-guest mechanism could see the wedged RIP.
  `scripts/boot-test.sh` now attaches a QEMU HMP monitor
  (`-monitor tcp:127.0.0.1:55123,server,nowait`, only under
  `--hard-lockup-watchdog`) and, on timeout with no wait-marker, queries it over
  bash `/dev/tcp` (`info registers` / `info cpus` / `info registers -a`) to read
  the frozen CPU's registers straight from the emulator — bypassing in-guest NMI
  delivery entirely. `resolve_kernel_symbol()` resolves RIP to the nearest
  preceding symbol via `llvm-nm -nC --defined-only`, comparing **zero-padded
  16-hex-digit strings** (NOT awk `strtonum`, whose doubles lose precision above
  2^53 for higher-half ~1.8e19 addresses) and computing the offset in bash
  64-bit arithmetic. `scripts/soak-nmi-check.sh` preserves the register dump
  (`SNMI-CAUGHT-*-regs.txt`) alongside each serial catch. This RIP-capture path
  is reusable for any future silent IF=0 wedge.
- **Confirmation (DONE):** 12-iteration `--hard-lockup-watchdog` soak (300 s
  timeout) post-fix returned **12/12 clean BOOT_OK, zero catches** — no silent
  wedge, no NMI dump, no liveness dump. Wedge no longer reproduces. Before the
  fix, this soak caught a silent wedge within the first few iterations.

### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock self-deadlock — ROOT-CAUSED + FIXED 2026-07-03

**STATUS: FIXED** (commit this session). Root cause confirmed by the
owner-tracking instrumentation: a **recursive self-deadlock** — the same task
that holds `ACCT` re-enters it from interrupt context. Fix: acquire `ACCT` via
the new `Mutex::lock_irqsave()` (interrupts masked for the hold), the standard
`spin_lock_irqsave` discipline for a lock shared with interrupt context. See
"Root cause + fix" below. Re-soak to confirm no recurrence.

**Root cause + fix (2026-07-03):** The instrumented soak reproduced on
**iteration 1** and the owner stamp printed the verdict verbatim:
`[sync]   lock 'ACCT' holder: task 138 == spinner — RECURSIVE self-deadlock
(same task re-entered the lock)` (task 138 = "countbytes", the ring-3
`/bin/emit | /bin/countbytes > file` pipeline; catch:
`build/hang-catches/ACCT-OWNER-recursive-task138.txt`).

Mechanism (uniprocessor — no cross-CPU AB-BA needed):
1. `Mutex::lock()` disables *preemption* but **not interrupts** — it leaves IF
   as-is. `ACCT` was acquired this way.
2. `ACCT` is reachable from **interrupt/softirq context**: the frame allocator
   calls `compact::try_compact()` for any `order > 0` allocation
   (`mm/frame.rs:2033`), and compaction's `estimate_movable_pages()` calls
   `accounting::tracked_count()` (`mm/compact.rs:266`) → acquires `ACCT`. So a
   device IRQ / softirq that allocates a multi-order buffer re-enters accounting.
3. Critically, the **page-fault handler re-enables interrupts** (`idt.rs:2048`,
   `cpu::sti()` when the faulting context had IF=1) *before* calling
   `mm::fault::resolve` → `map_frame`/CoW → `charge`/`uncharge`. So a
   `charge`/`uncharge` on the fault path runs and holds `ACCT` **with interrupts
   enabled**.
4. An interrupt lands while `ACCT` is held → its handler allocates an
   order>0 frame → compaction → `tracked_count()` → tries to re-acquire `ACCT`
   → spins forever (holder can never resume to release it). On UP the spinner
   *is* the same task's IRQ frame, so `owner == spinner` → the recursive verdict.

Why the earlier static analysis missed it: I looked only for a *direct*
IRQ-context accounting caller and found none; the real path is indirect
(IRQ → frame alloc → compaction → `tracked_count`) and is only opened by the
page-fault handler's `sti`. The accounting functions themselves remain correct
leaf scans; the bug was the *locking discipline*, not the functions.

**Fix:** added `Mutex::lock_irqsave()` + `MutexIrqGuard` to `kernel/src/sync.rs`
(save IF, `cli`, acquire; guard restores IF after releasing the lock and
re-enabling preemption — reverse of acquire order; nests correctly, only the
disabling edge restores). Switched all 12 `ACCOUNTING.lock()` sites in
`kernel/src/mm/accounting.rs` to `lock_irqsave()`. This masks interrupts for the
short leaf-only hold, closing the re-entry window for *any* interrupt (not just
the compaction path). A nested #PF cannot occur during the hold (the functions
only touch a static `.bss` array + trivial stack), so masking maskable
interrupts is both necessary and sufficient. Builds clean, no new clippy
warnings. Module doc in `accounting.rs` updated to document the IRQ-safety
requirement.

**Follow-up (separate, low priority):** `all_stats()` still `.collect()`s a
`Vec` under the lock (now under `lock_irqsave`, so interrupts are masked across
a heap alloc — worse for IRQ latency, though it has no live callers). Should be
count-then-release or a fixed stack buffer regardless.

---

<details><summary>Original investigation notes (pre-fix, kept for history)</summary>

#### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock stuck at end of ring-3 battery — REPRODUCED 2026-07-03

**Where:** `kernel/src/mm/accounting.rs` (the `ACCOUNTING` spinlock, named `b"ACCT"`,
line 102) / `kernel/src/sync.rs` (the `Mutex` wrapper). Caught by the armed
hang-repro soak on **iteration 7/24** with the orphaned-Running-fixed kernel:
`build/hang-catches/ACCT-STALL-iter7-*.txt`.

**This is a DISTINCT bug from the orphaned-Running dispatch wedge** (which was
committed just before this soak). Decisive discriminator: the catch shows **no
`[sched] BUG:` line**, so the fixed dispatch path is not involved.

**Observed signature (`ACCT-STALL-iter7`):**
- `[liveness] SYSTEM HANG: no task-level forward progress for 15+ seconds
  (useful_work=140, all CPUs idle-ticking)` — cpu0 heartbeat=3501 **still
  advancing** (BSP alive, not an IF=0 spin), `local_has_real_work=false`,
  `last_rip=0xffffffff81107fb9 (kernel_text)`.
- Task dump: **91 tasks, 90 `state=Dead`, only `tid=0` (the boot/self-test
  driver, name overwritten to "prctl-batch269") is `state=Running`** on cpu0 at
  prio=31. This is the very end of the ~34-test ring-3 battery — everything ran
  and exited, leaving only the driver.
- Then: `[sync] *** SPINLOCK STALL *** lock 'ACCT' still not acquired after ~30s
  of spinning (cpu 0, task 0, 66805760 iters). Likely self-deadlock or lock
  convoy` followed by `[lockdep]   cpu 0 holds 0 lock(s):`. So task 0 spins
  ~66M iters trying to acquire `ACCT`, which the timer-driven liveness watchdog
  cannot rescue (the spin holds the CPU with preemption disabled).

**Analysis so far (static; not yet definitive):** The `ACCT` lock is
`mm/accounting.rs`'s `Mutex` (a `spin::Mutex` that does **not** disable
interrupts — `lock()` only `preempt_disable()`s). All *live* callers of the
accounting functions (`charge`/`uncharge` on the map/unmap/CoW page-fault path;
`query`/`tracked_count`/`largest_rss`/`memory_info` from procfs/kshell/
diagnostics/invariant checks) run in **task context** — I could not find any
IRQ/softirq/timer-context caller, which argues *against* a simple
interrupt-reentrancy self-deadlock. The accounting functions themselves are all
leaf scans that never yield/fault/allocate under the lock, so a single call
cannot leak the guard. The one structurally-unsafe function, `all_stats()`
(collects a `Vec` *under* the lock — violates the module's documented "ACCT is a
leaf lock, never held across other lock acquisitions" invariant), has **no live
callers**, so it is not the trigger here (but should be fixed on its own merits:
count-then-release or use a fixed stack buffer). `lockdep cpu 0 holds 0 locks`
is ambiguous — lockdep may only mark a lock *held* after successful acquire, so
a spinner shows 0, and the true holder (if since-dead) leaves no lockdep trace.

**Instrumentation added (commit this session; `sync.rs`) to make it definitive
on the next repro:** every `Mutex` now records the acquiring task id in a new
`owner: AtomicU64` (set in `make_guard`, cleared to `OWNER_NONE`=`u64::MAX` in
`MutexGuard::drop` — one relaxed per-CPU read+store, negligible next to the CAS
and lockdep call already present). `report_stall` now prints the holder and
classifies the stall:
- `owner == spinner tid` → **recursive self-deadlock** (same task re-entered).
- `owner == some other task` → **guard held by another task** (leaked if that
  task is Dead in the dump).
- `owner == OWNER_NONE` → **lost-unlock / flag desync** (spinlock flag set with
  no recorded holder).
This single datum discriminates all three hypotheses. Builds clean. **STILL OPEN
— re-run the armed soak with the instrumented kernel; the next `ACCT` stall will
name its holder and pin the exact leak/recursion path.**

</details>

### B-DASH-STDIN-FLAKE. `dash script-from-stdin` ring-3 self-test intermittently returns `InternalError` — WATCH 2026-07-01

**Where:** the boot self-test that runs the REAL `dash` shell over a script fed
on fd 0 (`kernel/src/proc/spawn.rs` ring-3 dash integration test; serial marker
"REAL dash shell script-from-stdin …"). Normally logs `… captured 55 bytes ==
expected, EOF→exit 0): OK`.

**Observed:** on one boot (2026-07-01, `BOOT_OK after 181s`) the harness logged
`WARNING: Path-Z real dash shell script-from-stdin self-test failed:
InternalError` (serial line 3589) instead of the OK line, while the *immediately
preceding* boots (identical dash test) passed. Load-dependent — same family as
B-CONTAINER-JAIL-TESTRACE / B-PTHREAD-YIELDBUDGET (intermittent races in the
ring-3 `clone`/`fork`/`exec`/reap + futex machinery). Non-fatal on this run:
BOOT_OK was still reached; only the one sub-test flaked.

**Assessment:** almost certainly the same underlying low-probability
spawn/exec/reap or futex race already tracked for pthread/container tests, not a
dash-specific logic bug. **Proper fix:** shares the root-cause work with the
pthread `clone`+futex deadlock (B-PTHREAD-YIELDBUDGET) — instrument the ring-3
spawn/reap path (lock-order tracer + futex wait/wake ordering) and fix the race;
also make the dash harness distinguish a transient spawn failure from a real
shell error. Logged so the intermittent dash failure isn't forgotten.

**Diagnostic classification DONE (2026-07-01):** all ~34 ring-3 real-binary
self-tests (`proc/spawn.rs`: glibc hello/stdio/full/pthread/signal/fault/
sigqueue/forkexec/pipe/redir/redirin, all 16 real-dash tests, make/cc/hosted-cc/
make+tcc) previously collapsed *both* a **hang** ("did not exit within N yields")
and a genuine **wrong-result** (mismatched output/exit code) into the same
`KernelError::InternalError`, so a captured flake report ("InternalError") could
not be told apart from a real shell logic bug. Now the two are distinct: a
never-reached-Zombie timeout returns `KernelError::TimedOut` (the transient
spawn/reap/futex flake class — B-DASH-STDIN-FLAKE / B-PTHREAD-YIELDBUDGET), while
a completed-but-wrong result keeps `InternalError`; fd-redirect infrastructure
failures now propagate the real fd-install error. So the non-fatal WARNING line
in `main.rs` is self-classifying: `TimedOut` == flake/hang, `InternalError` ==
real logic bug, other == infra. This does not *fix* the underlying race (root-
cause work still pending), but future flakes are now unambiguously attributed.
The B-PREEMPT-SPINLOCK preempt-disable fix (top of file) may also have reduced
this race's incidence; watching future boots for recurrence.

**Boot-data points (post B-PREEMPT-SPINLOCK fix):** 2026-07-01 (BOOT_OK 177s):
dash script-from-stdin passed (`captured 55 bytes == expected, EOF→exit 0: OK`);
no recurrence. No unexpected WARNING/failed lines this boot (the only
`[lockdep] WARNING`s are the lockdep self-test's intentional AB/BA + transitive-
cycle detections, each followed by `OK`). **2026-07-02 (TD31 landed):** 4 further
consecutive green boots (190/182/181/185 s) with zero self-test-failure lines —
the dash script-from-stdin test passed on every one; no `InternalError`/`TimedOut`
recurrence even with the added spawn/reap CGROUP traffic. **2026-07-14 (bad
flake streak under host load): 3 consecutive `--no-build` boots HUNG** (no BOOT_OK
within 480 s) at three *different* spawn/reap points — run 1 mid ring-3 `dash`
dirstat test, run 2 after the `test-restart-ct` container-init spawn (line 9289;
same wedge the TD "symmetric cgroup accounting" entry documents), run 3 at a
glibc-dynamic-exec page-cache fault for pid 165 (before any container code) — then
**run 4 reached BOOT_OK in 136 s clean**. The three hangs were the pre-existing
spawn/force-kill/reap SMP race, *not* a code regression: run 3 wedged before the
container subsystem even ran, and the runs were competing for host CPU with
concurrent cargo builds + overlapping QEMU instances (the race is host-timing
sensitive, so heavy host load raises its incidence). The Q19/§60 multi-network
self-test (`Multi-network membership (attach/detach): OK`) passed on every run
that reached it (runs 2 and 4). Takeaway: **run boot tests one at a time on an
unloaded host** — overlapping QEMUs materially worsen this flake.

### TD-EDITOR-UTF8. Text editors reject non-UTF-8 files (`fs::read_to_string`) — LOW PRIORITY / graceful failure 2026-07-02

**Where:** `apps/editor/src/main.rs` and `apps/markdowneditor/src/main.rs`
(`Document::from_file`), which load file content via
`std::fs::read_to_string(path)`.

**The limitation.** `read_to_string` returns `Err` for any file whose bytes are
not valid UTF-8, so both editors *refuse to open* a non-UTF-8 (e.g. Latin-1,
UTF-16, or binary) file. This is a **graceful failure** (the open is rejected
cleanly — there is no silent `from_utf8_lossy` corruption, which CLAUDE.md rule 7
forbids), so it is a limitation rather than a correctness bug. Discovered while
implementing the external-change merge feature (todo2.txt line 1), which reuses
the same String/`Vec<String>` line model.

**Why deferred, not fixed now.** Both editors store the document as
`lines: Vec<String>` and operate on `&str` throughout (rendering, cursor math,
find/replace, the diff/merge engine `diffcore` which is `String`-based). Truly
handling arbitrary bytes would require converting the whole editor + `diffcore`
to a byte-oriented buffer model with an explicit encoding/decoding layer — a
large refactor. For a *plain-text* editor, UTF-8 is a defensible domain
assumption and clean rejection of non-text files is acceptable behavior, so the
refactor is disproportionate to the value. CLAUDE.md rule 7's core concern
(OS-boundary metadata: paths, env, pipe data handled as bytes; no
`from_utf8_lossy`) is already honored — this is document *content*, and the
failure mode is safe.

**Proper fix (if a concrete need appears — e.g. editing config files in a legacy
encoding):** give the editor a byte buffer + a detected/selectable encoding
(UTF-8 default, with a lossless round-trip for at least Latin-1/UTF-16), decode
for display, re-encode on save, and thread the same through `diffcore`
(byte-slice diff). Trigger: a user report of a real file that won't open, or an
explicit requirement to edit non-UTF-8 documents.

### B-PAGECACHE-COHERENCE. Read-only page cache invalidation on FS mutations — FIXED 2026-06-30 (de-double-cache vs. buffer cache still pending)

**Resolution (2026-06-30):** the two correctness gaps below are now
closed. `mm::page_cache::invalidate_identity(fs_id, ino)` is wired into
the VFS mutation paths — `Vfs::write_at`, `Vfs::write_file`,
`Vfs::truncate`, `Vfs::remove`, and replacing same-mount `Vfs::rename`
— via the `cache_identity()` helper, which captures the file's
`(fs_id, ino)` under the held VFS lock (gated on a single relaxed
`is_populated()` atomic so the write path pays ~nothing when nothing is
cached). `remove` and the replacing-rename capture identity *before* the
inode is freed, closing the inode-reuse hole; the others capture after
the content change. Verified by boot self-test check 8 (is_populated +
invalidate_identity) and a green BOOT_OK.

**Shrinker (sub-task 4 eviction) landed 2026-06-30.**
`mm::page_cache::shrink(PressureLevel)` evicts *idle* cached pages
(refcount ≤ 1, i.e. no live mapper) proportional to the pressure level
(Low 25% / Medium 50% / Critical 90%), registered with `mm::pressure`
by `mm::page_cache::init()` (called from `kernel_main`). Verified by
boot self-test check 9 (shrink spares live, evicts idle) *and* by the
shrinker actually firing under real critical pressure during boot —
serial shows `[pressure] page_cache freed 49 objects (level=critical)`
then `freed 5 objects`, with BOOT_OK reached cleanly. Freeing 54 frames
under live pressure with no fault is a strong exercise of the
freed-while-mapped hypothesis: a mapped cache page always has
refcount ≥ 2 (cache entry + each PTE; `map_frame` does not bump
refcount, so the `get_or_fill` caller ref *becomes* the PTE ref), so
the shrinker's `refcount <= 1` gate never selects a mapped frame.

**Still pending (performance, not correctness — §36 sub-task 4 tail):**
de-double-cache the page cache against the block buffer cache
(`fs/cache.rs`) so a page does not live in both. Tracked as a follow-up;
not a bug.

The original write-up (now resolved for the correctness parts):



**Where:** `kernel/src/mm/page_cache.rs` (the cache) + the VFS/handle
write/truncate/unlink/rename paths (`kernel/src/fs/handle.rs`,
`kernel/src/fs/vfs.rs`, and the relevant syscall translators in
`kernel/src/syscall/linux.rs`).

**What it is:** sub-task 3 (commit wiring the FileBacked fault path to
`page_cache::get_or_fill`) populates the cache from mmap faults but does
**not** yet invalidate cached pages when the backing file changes. Two
correctness gaps result:

1. **Stale data after write/truncate.** If process A `mmap`s a file
   (pages enter the cache) and process B `write(2)`s or `ftruncate(2)`s
   that same file, A keeps seeing the *old* bytes through its mapping
   (and any later mmap of the file gets the cached stale page). The
   cache is read-only by design (writable MAP_SHARED writeback stays
   ENOSYS, §23), but read-side coherence with `write(2)` is still
   required and is missing.

2. **Inode-number reuse.** The cache key is `FileId { fs_id, ino }`.
   `fs_id` is monotonic per-mount (never reused), but `ino` **can** be
   reused within a mount after `unlink`. If file X (ino 53) is cached,
   unlinked, and a new file Y reuses ino 53, a fault on Y would be
   served X's stale pages. (`fs_id` prevents *cross-mount* collisions
   only.)

**Effect:** wrong file contents observed through a file mapping after a
concurrent write/truncate, or after unlink+recreate reuses an inode.
Not hit on the boot path (programs mmap read-only shared objects they
don't concurrently rewrite), which is why boot is green — but it is a
real correctness bug for general workloads.

**Proper fix (sub-task 4):** wire cache invalidation to FS mutations:
`page_cache::invalidate_file(file_id)` (or a page-range invalidate) on
`write`/`pwrite` that extends/overwrites a regular file, on `truncate`/
`ftruncate`, and on `unlink`/`rename` that drops/replaces an inode.
Resolve the `FileId` cheaply at the mutation site (the handle/path is
already known). Keep it cheap when nothing is cached (the
BTreeMap-range invalidate already returns 0 fast for an absent file).
Also de-double-cache against the block buffer cache (`fs/cache.rs`) per
§36 sub-task 4. Until this lands, the page cache is only safe for the
read-mostly mmap workloads the boot path exercises.

**Discovered/created:** 2026-06-30 (completing sub-task 3 without
sub-task 4's coherence wiring).

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

### D-CGROUP-TASK-UNASSIGNED. Cgroup memory controller now reachable for real workloads — RESOLVED (2026-07-01)

**Original problem:** every `Task` was constructed with
`cgroup_id: ROOT_CGROUP` and no path ever set it to anything else, so
`current_task_cgroup()` always returned root, `charge_cgroup_alloc`
fast-exited, and the per-cgroup memory limit / accounting was never
exercised by real workloads — only by self-tests charging an explicit
cgroup. Container memory limits did not actually constrain memory.

**Resolution (Q14, operator option A):**
1. **Assignment path** — `sched::set_task_cgroup(task_id, cgroup)`
   (`kernel/src/sched/mod.rs:1287`) is the single authoritative
   process→cgroup assignment: it swaps `task.cgroup_id` under the SCHED
   lock and keeps the cgroup `nr_tasks` counts consistent (detach old,
   attach new) with a strict SCHED→cgroup-TABLE lock order.
   `container.rs` `add_process_task` (line ~1543) calls it to move a
   container's task into the container's cgroup, and `remove` (line
   ~1640) moves it back to root.
2. **Inheritance path** — `sched::spawn` (`mod.rs:1031/1046`) captures
   `current_task_cgroup()` before the task-creation critical section and
   copies it onto the new task, so `fork` (routes through
   `thread::spawn`→`sched::spawn`), `thread_clone`, and `spawn_user`
   (also `→sched::spawn`) all inherit the creating task's cgroup — Linux
   fork/clone semantics. Recorded in design-decisions §39.
3. **End-to-end test** — `cgroup_e2e_test_task` in `kernel/src/main.rs`
   runs as a live scheduler task (so `current_task_cgroup()` resolves to
   a real task, unlike the no-task kmain self-tests): it creates a
   memory-limited child cgroup, joins it via `set_task_cgroup`, allocates
   N=32 frames through the ordinary `alloc_frame` path (into a stack
   array — no heap growth to perturb the count), and asserts the group's
   `mem_usage` rose by exactly N; then frees them and asserts usage
   returns to baseline (uncharge follows the per-frame `FRAME_CGROUP`
   record, so it debits the right group even after the task rejoins root).
   Prints `[cgroup-e2e] PASS`/`FAIL` on the boot serial log.

**Discovered:** 2026-06-30 while fixing B-CGROUP-DBLCHARGE. **Resolved:**
2026-07-01 once Q14 settled which layer owns process→cgroup assignment
(`kernel/src/cgroup.rs` enforces + owns assignment via `set_task_cgroup`;
`fs::cgroupfs` remains the config frontend).

### D-PTHREAD-DETACH-LEAK. Detached pthread stacks are never freed (64 KiB leaked per detached thread) — RESOLVED (2026-07-01)

**Resolved 2026-07-01.** Implemented the userspace self-unmap fix exactly
as prescribed below:

- `THREAD_TABLE` is now a **lock-free array of atomic `ThreadSlot`s**
  (`task_id: AtomicU64` doubling as occupancy flag with `SLOT_EMPTY`/
  `SLOT_RESERVED` sentinels; `stack_base`/`stack_size: AtomicUsize`;
  `state: AtomicU8`). The old `static mut` + "single-creator convention"
  data race is gone.
- Added the `__pthread_exit_unmap(stack_base, stack_size, retval)`
  `global_asm!` primitive (`target_os="none"` only): it does
  `SYS_MUNMAP` then `SYS_THREAD_EXIT`, carrying `retval` in **R12** (a
  callee-saved reg the kernel's SYSCALL entry stub preserves — verified
  against `kernel/src/syscall/entry.rs`, which pushes/pops rbx/rbp/r12-r15
  around the handler). No memory is touched between the two syscalls.
- The per-slot `state` arbitrates the detach-vs-exit race via
  `compare_exchange`: `JOINABLE --detach--> DETACHED` (thread self-unmaps
  on exit) vs `JOINABLE --exit--> EXITED` (a joiner, or a `pthread_detach`
  that observes `EXITED`, frees the stack after `SYS_THREAD_JOIN` confirms
  the thread is off it). **Exactly one party frees** — no use-after-free.
  `pthread_join` rejects a detached thread (`EINVAL`); double-detach
  returns `EINVAL`; detach-after-joinable-exit reaps.
- Covered by 5 host unit tests in `pthread::tests`
  (`test_thread_slot_store_find_release`, `test_detach_marks_state_detached`,
  `test_double_detach_is_einval`, `test_join_rejects_detached_thread`,
  `test_detach_after_joinable_exit_reaps`) exercising the arbitration
  state machine directly.

**Residual (smaller) follow-up — D-PTHREAD-DETACH-KERNEL-EXITVAL — RESOLVED (2026-07-01):**
the kernel previously retained a small `THREAD_EXIT_VALUES: BTreeMap<TaskId,i64>`
entry (~tens of bytes) for a never-joined *detached* thread, because only
`join` removed it. **Fixed** by threading a "detached" flag through
`SYS_THREAD_EXIT` (arg1): `sys_thread_exit` (`kernel/src/syscall/handlers.rs`)
now decodes `let detached = args.arg1 != 0;` and passes it to
`thread_exit_with_value(exit_value, detached)`. A new `record_exit_value`
helper in `kernel/src/proc/thread.rs` skips the map insert entirely when
`detached` is set (task IDs are not reused while a task is live, so there
is no stale entry to clear). The userspace `__pthread_exit_unmap` self-unmap
asm sets `esi = 1` (detached) before `SYS_THREAD_EXIT`; the joinable
`pthread_exit` path uses `syscall2(SYS_THREAD_EXIT, retval, 0)` so arg1 is a
*defined* 0 (a bare `syscall1` would leave RSI holding stale/undefined bits,
which the kernel could misread as detached). In-kernel self-test
`test_detached_exit_not_retained` (verified at boot: `[thread]   Detached
exit value not retained: OK`) confirms joinable exits are recorded and
detached exits are not. Combined with the userspace stack self-unmap fix,
a detached thread now leaks neither its 64 KiB stack, its table slot, nor a
kernel map entry.

*Note:* a native SlateOS-ABI userspace test harness that links `posix`
does not currently exist (the boot path's thread tests use real glibc via
`clone`, not our `SYS_THREAD_CREATE`), so the "boot self-test spawning N
detached threads" originally envisioned below is deferred until such a
harness exists; the host unit tests cover the (bug-prone) arbitration
logic in the meantime.

---

**Original entry (for reference):**

**Where:** `posix/src/pthread.rs`. `pthread_create` mmaps a
`DEFAULT_THREAD_STACK_SIZE` (64 KiB) user stack and records it in
`THREAD_TABLE`. `pthread_join` frees the stack after `SYS_THREAD_JOIN`
returns. But a **detached** thread is never joined, so nothing ever
munmaps its stack — the `ThreadInfo` slot and the 64 KiB mapping leak for
the life of the process. `pthread_detach` only flips `info.detached`.

**Effect:** A long-running process that repeatedly spawns detached
worker threads leaks 64 KiB per thread plus a `THREAD_TABLE` slot (only
64 slots), eventually exhausting the table and address space. Most
current userspace tools don't spawn many detached threads, so it's
low-frequency, but it is a genuine unbounded leak.

**Proper fix (userspace-only, no kernel change):** the exiting thread
must free its *own* stack, glibc-`__unmapself`-style. Add a small
bare-metal asm primitive `__pthread_exit_unmap(stack_base, stack_size,
retval)` that issues `SYS_MUNMAP(stack_base, stack_size)` then
`SYS_THREAD_EXIT(retval)` **without touching the stack between the two
syscalls** (stash `retval` in a callee-saved reg that `SYSCALL` doesn't
clobber — not the stack). In `pthread_exit`, after running TSD
destructors, look up the calling thread's `ThreadInfo`: if `detached`,
`take_thread_info()` and tail-call `__pthread_exit_unmap`; if joinable,
fall through to the normal `SYS_THREAD_EXIT` (the joiner frees the
stack). Threads created detached, or detached before exit, both work.

**Concurrency caveat that must be handled:** `pthread_detach` (called
from another thread) races the exiting thread's read of `info.detached`
+ `take_thread_info`. The `THREAD_TABLE` currently relies on a
"single-creator convention" with NO lock — that is unsafe for the
detach-vs-exit window. The proper fix must add a real lock (or an atomic
detached flag per slot, CAS'd by whichever of detach/exit gets there
first) so exactly one of {joiner, self-unmap} frees the stack and there
is no use-after-free. Model on glibc's `joinid`/`cancelhandling` atomic.

**Why deferred:** the asm self-unmap path is `target_os="none"`-only and
cannot be unit-tested on the host; combined with the detach/exit data
race it is too risky to land without a QEMU multithread stress test.
Landing it blind risks a use-after-free crash, which is far worse than a
slow leak. Do it as its own focused task with a boot self-test that
spawns N detached threads in a loop and asserts address-space / table
usage stays bounded.

**Discovered/documented:** 2026-06-30 (already noted as a `// Known
limitation` in `pthread_detach`'s doc comment; promoted to tracked tech
debt while implementing per-thread TSD).

### D-CRT-INIT-ARRAY. `.init_array`/`.preinit_array` constructor + `.fini_array` destructor support — MECHANISM LANDED (end-to-end C/C++ validation pending a consumer)

**Status (2026-07-01):** The constructor/destructor machinery is now
**implemented and host-tested**. What remains is purely *validating it
against a real C/C++ program that emits constructors* — no such program
exists in-tree yet, so the mechanism has only been proven to be a correct
no-op for the (all-Rust) programs that currently run.

**What landed:**
- *crt (`posix/src/crt.rs`):* host-testable walkers `run_init_array(start,
  end)` (ascending, skips nulls) and `run_fini_array(start, end)`
  (descending), gated `#[cfg(any(target_os = "none", test))]`. Weak
  boundary externs (`__preinit_array_start/end`, `__init_array_start/end`,
  `__fini_array_start/end`) declared weak via a `.weak` **assembly
  directive** (`global_asm!`) rather than the nightly-only
  `#[linkage = "extern_weak"]` attribute — the kernel/boot build uses the
  **stable** toolchain, so a nightly `feature(...)` gate in posix breaks
  `bash scripts/boot-test.sh` (`E0554: #![feature] may not be used on the
  stable release channel`). A weak *undefined* symbol resolves to null at
  link time, so pure-Rust programs (no `.init_array`, no boundary symbols
  synthesised by lld's default layout) link cleanly and the startup walk
  sees null bounds → no-op. `run_constructors()` (preinit then init) is
  called from `__libc_start_main` after environ/signal init and before
  `main`; `run_destructors()` is registered via `atexit` so it fires
  LIFO-correct at normal exit. All `#[cfg(target_os = "none")]`, so the
  slateos userspace target (os=linux, uses Rust std's own startup) is
  untouched. Four host unit tests cover forward/skip-null, reverse order,
  null-bounds no-op, and empty-array no-op.
- *Linker scripts:* `.preinit_array`/`.init_array`/`.fini_array` output
  sections with `PROVIDE_HIDDEN` boundary symbols added to
  `services/{hello,init,ticker}/linker.ld` and
  `userspace/{coreutils,sha256sum,shell}/linker.ld` (`:load` so they land
  in the mapped PT_LOAD; `KEEP` so `--gc-sections` keeps them). NOTE: these
  6 scripts are currently **vestigial** — `kernel/build.rs` no longer
  passes `-T` for them and no per-crate build.rs applies them — so they're
  updated for correctness/future-proofing but do not yet feed a live link.

**Validated:** `cargo build -p posix` (stable, unknown-none) clean; all 6
programs whose linker scripts changed build+link clean; posix host tests
19992 passed (incl. the 4 init/fini tests); `bash scripts/boot-test.sh`
→ BOOT_OK (zero regression — the walk is a no-op for everything currently
running).

**Still pending (why not fully closed):** no in-tree C/C++ program emits
constructors, so the *non-null* path has never executed on real hardware/
QEMU. When the first such consumer lands it additionally needs either
(a) a C crt0 + `__libc_start_main` exported on slateos, or (b) a
posix-linking Rust program that actually emits `.init_array` — at which
point the boundary symbols become non-null and the walk should be
end-to-end boot-tested against that program. Until then this entry stays
open to flag that the constructor path is *implemented but unproven under
load*.

**Discovered/documented:** 2026-06-30; mechanism implemented + host-tested
2026-07-01.

### D-CNET-L2BRIDGE. User-defined container networks now provide a shared layer-2 bridge (same-network peers reach each other directly) — RESOLVED 2026-07-01

**Resolution (2026-07-01):** each named network now stands up one
`net::bridge` instance and switches frames at L2 between its members'
veth host-ends, so two containers on the same named network reach each
other directly by their allocated IPs. The prior IPAM-only behaviour is
now backed by real inter-container reachability.

**What landed:**
1. **veth bridged flag** (`kernel/src/net/veth.rs`): `VethEnd` gained a
   `bridged: bool`; `poll_all()` skips bridged ends (the bridge owns their
   frames, not the global host stack). New `set_bridged`/`is_bridged`.
2. **Bridge veth ports** (`kernel/src/net/bridge.rs`): `BridgePort` gained
   `veth_pair: Option<usize>`; `MAX_BRIDGES` raised to 16.
   `attach_veth`/`detach_veth` register a veth pair's host-end (end A) as a
   bridge port (idempotent; port id = slot index), toggling the veth
   bridged flag outside the BRIDGES lock. `forward(bridge_idx)` drains each
   ingress port's `veth::recv(pair, A)`, learns src→port + resolves dst in
   one BRIDGES-locked step (MACs parsed via `get`+`try_from`, no slicing),
   then delivers: known unicast → `veth::send(out_pair, A, frame)`;
   broadcast/multicast/unknown → flood-clone to all other members **and**
   `ethernet::process_frame(&frame)` into the host stack (this preserves
   the pre-existing external-NAT path — no regression). `forward_all()`
   snapshots active bridges and forwards each.
3. **net::poll wiring** (`kernel/src/net/mod.rs`): `bridge::forward_all()`
   runs immediately before `veth::poll_all()`, so bridged host-ends are
   consumed by the bridge rather than the generic drain.
4. **Lazy per-network bridge lifecycle** (`kernel/src/cnetwork.rs`):
   `Network` gained `bridge_idx: Option<usize>`; `Allocation` gained
   `veth_pair: Option<usize>`. `attach_container_veth(name, cid, pair)`
   creates the bridge lazily on first attach, attaches the veth, and
   records the pair on the owning lease. `release`/`release_container`
   detach their veth pairs; `detach_and_maybe_teardown` deletes the bridge
   when its last port leaves (`veth_port_count == 0`).
5. **run-path wiring** (`kernel/src/kshell.rs`): the `oci run --network
   NAME` path calls `attach_container_veth` after taking the IPAM lease,
   printing `L2 bridge: NAME (N members)` (non-fatal warning if the veth
   is missing).

**Lock ordering:** `TABLE (cnetwork) → BRIDGES (bridge) → veth`; no reverse
edge, and `bridge::forward` never holds BRIDGES across veth I/O.

**Boot self-test:** `cnetwork::self_test()` builds a two-member network,
asserts the bridge is created lazily, exercises broadcast-flood and
learned-unicast forwarding, then verifies teardown on last detach —
serial `[cnetwork]   L2 bridge forward/learn: OK` and
`[cnetwork]   L2 bridge lifecycle: OK`.

**Follow-up (unchanged from before):** `poll_all` still dispatches
non-bridged veth frames into the *global* `ethernet::process_frame`;
per-namespace RX dispatch remains a separate TODO independent of this L2
switching work.

**Discovered/documented:** 2026-07-01 (while landing the `docker network`
IPAM feature, increments 60–61). **Resolved:** 2026-07-01.

### D-CNET-NSRX. Per-namespace veth RX + TX dispatch — RESOLVED (RX threading + container veth TX egress both landed)

**Status (2026-07-01): both halves landed and boot-validated.** The whole
ingress chain carries the arrival namespace (a container server socket bound
in its own netns is matched by the per-ns socket lookup), **and** the egress
path now routes container traffic (IPv4 data, fragments, ARP requests, ARP
replies) onto the container's veth instead of the physical NIC. Container
inbound/outbound over a user-defined network is functional end-to-end. The
one residual limitation is the **shared (non-namespaced) ARP cache** — see
the note at the end.

**What landed (RX threading).** `ns_id` is threaded as a parameter through
the entire RX chain:
- `ethernet::process_frame(data, ns_id)` — `is_for_us` now compares against
  the receiving namespace's interface MAC via the new
  `interface::ns_mac(ns_id)` (physical NIC MAC in root; veth-endpoint MAC in a
  container ns) instead of always the host NIC MAC.
- `ipv4::process_ipv4(payload, ns_id)` — `is_for_us` uses `ns_info(ns_id)`;
  inbound firewall uses `check_inbound_ns(ns_id, …)`; dispatches to
  tcp/udp/icmp with `ns_id`; `dispatch_reassembled` threads it too.
- `ipv6::process_ipv6(payload, ns_id)` — same shape (transport socket lookup
  is ns-scoped; NDP/SLAAC stay physical-NIC based, IPv6 container addressing
  is future work).
- `arp::process_arp(payload, ns_id)` — the "request for our IP?" check and
  reply source use `ns_ip`/`ns_mac`.
- `tcp::process_tcp/_v6(pkt, ns_id)` — pass `ns_id` to `process_tcp_common`
  instead of the old hardcoded `ROOT_NS`.
- `udp::process_udp/_v6(pkt, ns_id)` — the delivery loop now filters by
  `sock.ns_id` (root permissive, mirroring the TCP listener rule).
- `icmp::process_icmp(pkt, ns_id)` — echo replies are sent from the arrival
  namespace via `ipv4::send_ns`.
- Call sites: `net::mod::poll` and `bridge::forward_all`'s host-stack flood
  pass `ROOT_NS`; `veth::poll_all` passes each drained endpoint's own
  `ns_id`.

Boot-validated: `[udp]   Namespace isolation: OK` (extended to cover
delivery-level scoping — a datagram arriving in ns1 reaches only the ns1
socket, root arrival is permissive), full `[net] Network self-test PASSED`,
and the ARP ns tests — no physical-NIC regression.

**What landed (container veth TX egress).** The ns-aware send path now has a
veth egress branch keyed on the namespace:
- `net::send_frame_ns(ns_id, frame)` (`net/mod.rs`) — the single egress
  chokepoint: for `ns_id != ROOT_NS` with a veth endpoint
  (`veth::find_endpoint_for_ns`), it captures TX, `veth::send(pair, end,
  frame)` (→ enqueues on the peer host end A's RX → `bridge::forward_all`
  switches to the peer / floods to the host NAT stack), and records TX;
  otherwise it falls through to `send_frame` (physical NIC). Root traffic is
  unchanged.
- `ipv4::send_ns_ecn` and `send_fragmentable_ns` (both single-frame and the
  fragmentation while-loop) now source MAC/IP from `interface::ns_mac(ns_id)`
  / `ns_ip(ns_id)` / `ns_info(ns_id)`, resolve the next hop via
  `arp::resolve_ns(ns_id, …)`, and egress via `send_frame_ns(ns_id, …)`.
- `arp::send_request_ns` / `resolve_ns` — ARP requests are sourced from the
  ns interface and egress the ns link; `resolve_ns`'s poll loop drives
  `net::poll` (drains veth+bridge), so a peer reply returning through the
  bridge is learned into the cache. `resolve`/`send_request` delegate to the
  `_ns` forms with `ROOT_NS`.
- `arp::send_reply` — **no longer drops** non-root replies; it egresses via
  `send_frame_ns(ns_id, …)`, so a container answers ARP for its own IP on its
  user-defined network.

The container-creation path already assigns the ns interface IP/mask/gw/dns
(`netns::configure_interface`) and sets up the veth pair
(`setup_container_veth`), and `resolve_next_hop` for non-root uses
`netns::route_lookup` → ns gateway → direct-to-dst fallback, so
`resolve_next_hop`/`is_for_us` line up.

Boot-validated: new `[veth]   test 11 (send_frame_ns veth egress): OK`
(`[veth] Self-test PASSED (11 tests)`) — asserts a non-root
`send_frame_ns` lands on the peer host end's RX and a root-ns frame does NOT
leak into the veth — plus the RX-side `[udp]   Namespace isolation: OK` and
full `[net] Network self-test PASSED`, no physical-NIC regression.

**Per-namespace ARP cache — RESOLVED (was: shared ARP cache).** The former
residual (a single global ARP cache shared across all namespaces, so two
container networks reusing a subnet/IP could collide) is now closed. The
per-namespace ARP cache infrastructure that already existed (`NS_ARP`,
`ns_init`/`ns_destroy`/`ns_lookup`/`ns_insert`/`ns_flush`) is now wired into
the real paths:
- `container::setup_container_veth` calls `arp::ns_init(net_ns)` (and
  container removal calls `arp::ns_destroy(net_ns)`), so every networked
  container gets its own active ARP cache.
- `arp::process_arp` learns the sender's MAC into the *arrival* namespace's
  cache via `ns_insert(ns_id, …)` (delegates to global for ROOT_NS) instead
  of always `cache_insert` (global).
- `arp::resolve_ns` reads/waits on `ns_lookup(ns_id, …)` instead of the
  global `lookup`.
Boot-validated by a new `[arp]   ns process_arp learns into ns cache: OK`
(`[arp-ns] Per-namespace ARP self-test PASSED (4 tests)`), which asserts a
reply arriving in a namespace is learned into that ns's cache and does NOT
leak into the global cache. Root-namespace behavior is unchanged (still uses
the global `ARP_CACHE`).

**Discovered/analyzed:** 2026-07-01 (embedded-DNS work). **RX threading
landed:** 2026-07-01. **TX egress landed:** 2026-07-01. **Per-ns ARP cache
wired:** 2026-07-01.

### D-CONTAINER-EXEC-WAIT. Real in-container `docker exec` + synchronous wait — RESOLVED (all four steps landed)

**Status (2026-07-01): steps 1–4 done and boot-validated.** `container
exec` is no longer a net_ns-switch facade — it launches a genuine process
inside the container and (foreground) blocks until it exits, printing the
exit status. Step 4 (healthchecks) now landed too: the OCI `Healthcheck`
config is parsed, stored on the container, and driven by a periodic
non-blocking supervisor that surfaces health in `inspect`/`ps`.

**What landed:**
1. `container::wait_process(pid) -> KernelResult<i32>`
   (`kernel/src/container.rs`): the generalised block-on-exit primitive.
   Parks the caller on an arbitrary spawned global pid via
   `pcb::set_wait_task` + `sched::block_current`, woken by the
   zombie-transition path (`remove_thread` hands back the registered
   wait-task). Lost-wakeup-safe (re-check after register + scheduler
   `pending_wake`). On zombie it reads `pcb::exit_code(pid)` and reaps via
   `pcb::try_reap`, so an exec'd non-init child never lingers unreaped.
2. `container::exec_path(id, guest_cmd, argv) -> KernelResult<ExecSpawn>`:
   resolves `guest_cmd` under the container rootfs (`resolve_in_rootfs`,
   `..` cannot escape), reads the ELF, `spawn_process`es it, and
   `add_process_task`s it into the container's cgroup + PID/user/network
   namespaces + rootfs jail (the `run` wiring, minus flipping state /
   recording `init_pid`). Rolls the spawn back on bind failure. Stdio is
   left at the console default (foreground output appears live).
3. Shell `container exec [-d] <id> <cmd> [args...]`
   (`kernel/src/kshell.rs`, cmd_container "exec" arm): builds argv from the
   tokens, calls `exec_path`; foreground → `wait_process` + print exit
   status + `remove_process_task` cleanup; `-d` → print pid and return.

**Root-cause fix bundled in:** cgroup task-count accounting was previously
decremented **only** by an explicit `set_task_cgroup`/`remove_process_task`
while the task was still alive; a task that simply *exited* while assigned
to a non-root cgroup left a stale `nr_tasks` count forever (the task is
gone from the scheduler table before anyone can move it back to root).
`sched::reap_dead_tasks` now auto-detaches a reaped task from its cgroup
(skipping the root group; `detach_task` is saturating so a
detach-then-die can't underflow). This makes teardown accounting robust
for *any* exiting task, not just exec'd ones.

**Validation:** boot self-test `[container]   exec + wait
(exec_path/wait_process): OK` — creates a Running container with a real
rootfs, stages `/bin/hello`, execs it, yields until it zombifies, and
asserts: exit code 0 captured, process reaped (`pcb::state` is `None`),
cgroup billed +1 while alive then 0 after reap, plus the error paths
(exec on a non-Running container → InvalidArgument, missing binary →
NotFound, `wait_process(bogus)` → NoSuchProcess). BOOT_OK, hello's stdout
observed once in the serial log.

**Step 4 (healthchecks) — landed:** `oci::HealthcheckConfig`
(`kernel/src/oci.rs`) parses the OCI `Healthcheck` (test-token +
interval/timeout/retries/start_period, CMD vs CMD-SHELL). Each container
stores the probe plus its live health state
(`health_status`/`health_fail_streak`/`health_started_ns` and the
in-flight probe pid/task/deadline). The pure state machine
`container::apply_probe_result` implements the Docker semantics
(start-period grace does not count failures while `Starting`; a
`retries`-long failure streak → `Unhealthy`; any pass → `Healthy` + reset
streak) and is unit-covered by boot self-test `19k2h`.

The probes are driven by a **non-blocking** supervisor: a persistent
repeating `hrtimer` (250 ms tick, `start_health_monitor`, armed just
before `BOOT_OK` so it can't perturb the hrtimer self-test's exact
`pending_count` assertion) fires in ISR context, submits `health_tick_job`
to the shared `workqueue`, and `health_tick` polls every container.
Critically it **never blocks the single workqueue worker**: each probe is
launched via `exec_path`, then *polled* for its zombie transition on
subsequent ticks (never `wait_process`-blocked), reaped via the
`wait_process` fast path once dead, scored via `apply_probe_result`, and a
probe that overruns its timeout is `kill_process_threads`'d and scored as
a failure. The tick uses snapshot-under-lock → act-outside-lock (exec /
reap / kill / remove all take the table lock internally) → write-back.
Health is surfaced in `inspect` (JSON `health` field + human Health line
with failing streak) and `ps` (a `(healthy)`/`(unhealthy)`/`(health:
starting)` sub-state on the status column). Boot self-test `19k2s` drives
a real `/bin/hello` CMD probe deterministically to `Healthy`.

**Discovered/documented:** 2026-07-01 (while surveying the next container
increment after `docker network`). All four steps landed same day.

### W-KERNEL-COW-WRITE. Kernel-mode write fault on a user COW page is not routed to the resolver — WATCH (not currently reproducible)

**Where:** `kernel/src/idt.rs` page-fault handler (~line 1787). After
`mm::fault::resolve()` (kernel-VMA demand paging) declines a user
address, the user-fault resolver chain (swap-in →
`proc::pcb::try_resolve_fault`/CoW → stack growth) is entered **only**
when `error & 4` (CPL3, ring-3 access). A *kernel-mode* (ring-0) write
to a **present, read-only** user page (`error == 0x3`) therefore skips
CoW resolution and falls straight through to "FATAL: Unrecoverable
kernel page fault. Halting."

**Why it matters now:** the read-only page cache (§36) maps writable
`MAP_PRIVATE` file pages **RO + COW** on first fault (so writes copy out
of the shared frame), whereas the old private path mapped them
**writable** directly. Any kernel path that writes through a user
pointer into such a page *without first calling*
`mm::user::validate_user_write` (which breaks CoW eagerly at a safe
point, mirroring Linux `get_user_pages(FOLL_WRITE)`) would now trip a
ring-0 write fault on a COW page and halt. With pre-validation, the
correct kernel paths never hit this, which is why two full boots
(BOOT_OK, shrinker exercised under critical pressure) are clean.

**Status:** a prior-session boot showed a one-off
`EXCEPTION: Page Fault (#PF) ... error=0x3` at a USER_MMAP address
(`0x6000213450`) consistent with this scenario, but it has **not**
reproduced against the current source (two deterministic green boots).
Most likely it was a transient intermediate-edit state, not the
committed code. Left as a WATCH rather than a fix because the obvious
"route ring-0 user-address faults to `try_resolve_fault`" hardening
risks lock re-entrancy/deadlock (the faulting kernel code may already
hold the VMA/process locks the resolver takes) — exactly what
pre-validation exists to avoid.

**Proper fix if it recurs:** identify the specific kernel write path
that reaches a user COW page without pre-validating and make it call
`validate_user_write` (the architecturally-correct point to break CoW),
rather than weakening the fault handler. Only if a path genuinely cannot
pre-validate should the handler route ring-0 user-address faults to the
resolver, and then only with a fault-fixup/exception-table mechanism so
an unresolvable access returns `-EFAULT` instead of halting.

**Discovered:** 2026-06-30 (page-cache §36 sub-task 4 review).

### B-COMPACT1. Memory-compaction self-test (`collect_private_frames`) panicked non-deterministically across boots — FIXED 2026-06-16

**Where:** `kernel/src/mm/compact.rs` — `self_test()` Test 5; the API under test is
`kernel/src/mm/rmap.rs::collect_private_frames`.

**What it was:** the self-test added one fake private rmap entry, then called
`collect_private_frames(&mut [0u64; 4], 0)` once and asserted the fake frame was
among the (up to 4) results. `collect_private_frames` fills its `out` buffer with
the first `out.len()` private frames in table-index order, starting from the cursor
and wrapping once around the whole 16384-slot table. By the time the compaction
self-test runs, the rmap table already holds entries from other subsystems (a
failing boot showed ~16). With a 4-slot buffer, only the four lowest-indexed
private frames are returned; whether the fake entry (hashed to slot
`0x0F00_0000 % 16384`) is among them depends on what else occupies lower slots —
so the assertion passed or panicked depending on unrelated boot state. The panic
(`"collect_private_frames should find our fake entry"`) aborted the kernel mid-boot,
failing the Path-Z boot-test.

**Fix (2026-06-16):** the test now pages through the table with a 32-slot buffer
(larger than the live entry count, so a single full sweep already finds every
private frame including the fake one) and a bounded loop that advances the
continuation cursor each page, breaking as soon as the fake frame is seen, the
table is exhausted (`found == 0`), the cursor stops advancing, or a 64-page hard
cap is hit (guaranteed termination). This makes the test deterministic regardless
of how many unrelated rmap entries exist. Verified: BOOT_OK with
`[compact]   collect_private_frames: OK (saw_fake=true)` and
`[compact] Self-test PASSED`, 0 self-test failures.

**Related debt (not fixed):** `collect_private_frames`'s continuation/pagination
is mildly broken as a "visit every unique private frame exactly once" iterator —
each call performs a *full* `0..RMAP_TABLE_SIZE` sweep from `start_idx`, so when
more than `out.len()` private frames exist the continuation re-encounters frames
below the cursor on the next page (it never returns the `(found, 0)` "scan
complete" sentinel). The production consumer
(`compact.rs::try_compact`, 4 batches × 32) tolerates this — it re-checks each
candidate via `try_migrate_one` and only wastes a little work re-examining
duplicates — so it is a performance/clarity wart, not a correctness bug. A proper
fix would have the continuation scan only the *remaining* `[next, original_start)`
window rather than re-sweeping the whole table. Tracked here; low priority.

### B-EXT4-DIR. ext4 directory entries past the first block became invisible, and every directory insert grew the directory by a full block — FIXED 2026-06-16

**Symptom:** The ring-3 `link()`/`linkat()` hard-link self-test
(`self_test_linux_link`, kernel/src/proc/spawn.rs) intermittently failed
with exit 193 (link failed). Tracing showed `Vfs::write_file("/mnt/lnk-src",
b"L")` returned `Ok` but the file was then unresolvable, and later
`link()` reported `AlreadyExists` for a name the VFS layer's `exists()`
could not see. The persistent `/mnt` ext4 fixture (rootfs.ext4) also grew
without bound across boots as the self-tests created and deleted files.

**Root cause (two independent ext4 directory bugs):**

1. **`parse_dir_entries` abandoned the whole directory at the first
   `rec_len == 0`** (kernel/src/fs/ext4/driver.rs). ext4 directory data is
   a sequence of independent `block_size` chunks; a chunk can legitimately
   end with zero-padding (rec_len 0) while *later* blocks still hold live
   entries. The old loop `if hdr.rec_len == 0 { break; }` broke out of the
   entire directory, so every entry living in a block after the first
   zero-padded block was invisible to `read_dir_entries` → `dir_lookup` →
   path resolution. A file whose dirent landed in a later block "didn't
   exist" to `Vfs::exists`/`open`, yet `add_dir_entry`'s own physical scan
   still saw it (→ spurious `AlreadyExists`). It also meant `remove` could
   not find/unlink such entries, so they accumulated as orphans.

2. **`add_dir_entry`'s in-place-reuse path was dead code** (off-by-one).
   It computed the last directory block as `(dir_len / block_size) *
   block_size`, which for a block-aligned directory equals `dir_len`
   itself, so the guard `last_block_start < dir_len` was never true. Every
   insert fell through to the grow path, appending a fresh block per entry:
   unbounded directory bloat and fragmentation, which in turn fed bug (1)
   (more blocks → more chances for an entry to hide past a zero-padded
   block).

**Fix (proper):**

- Rewrote `parse_dir_entries` to parse block-by-block: an outer loop over
  `block_size` chunks and an inner loop over entries within
  `[block_start, block_end)`. `rec_len == 0` now terminates only the
  *current* block and advances to the next, never the whole directory.
  Name bounds use `block_end`, not `data.len()`. Added a regression test
  with a two-block buffer where block 0 ends in a zero-padded entry and
  block 1 holds a live entry, asserting both entries are found.
- Fixed `add_dir_entry` to compute the real last-block start as
  `dir_len.saturating_sub(block_size)` (guarded by `dir_len > 0 &&
  block_size > 0`), so free space in the final block is actually reused
  instead of growing the directory every time.
- Refactored `insert_dir_entry` to take an explicit `block_start`
  parameter (removing a buggy `(offset / remaining).max(1) * ...`
  reconstruction) and scan forward from it to find the previous entry to
  shrink.

**Verified:** With the fixes plus a freshly regenerated rootfs.ext4
(`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`), the ring-3
link()/linkat() self-test passes and the full boot reaches BOOT_OK with
zero self-test failures.

**Fixture note:** The pre-existing rootfs.ext4 had accumulated duplicate /
orphaned `lnk-dst` directory entries from prior buggy boots that the fixed
code could now see but a single `remove()` could not fully clear. The
fixture was regenerated clean. `self_test_linux_link` also gained a bounded
`drain()` loop that removes any stale src/dst names before staging, so the
test is robust to a dirty persistent fixture going forward.

### B-CWD1. Linux-ABI relative path resolution ignored the per-process cwd (relative `open`/`*at` resolved against `/`) — FIXED 2026-06-16

**Symptom:** After a process did `chdir("/dir")`, a relative `open("file")`
(or `openat(AT_FDCWD, "file")`, and the relative-path branches of stat,
access, mkdir, unlink, rename, readlink, chmod, chown, etc.) resolved the
path against the filesystem **root** rather than `/dir`. e.g. `cd /reltest &&
echo x > rel.txt` created `/rel.txt`, not `/reltest/rel.txt`. This broke
standard Unix semantics for essentially every program that uses relative
paths after changing directory.

**Root cause:** The Linux ABI's `open_common` forwarded the raw userspace
path pointer straight to `sys_fs_open` → `fs::handle::open` →
`Vfs::resolve_path`, and `resolve_at_path` (the `*at` family helper) returned
the path verbatim for the `AT_FDCWD`/relative case. None of those layers
take a PID, and `Vfs::normalize_path` treats `rel` identically to `/rel`
(it splits on `/` and always re-emits a leading slash), so the per-process
cwd stored in the PCB by `chdir` (`pcb::set_cwd`) was never consulted on the
open side. The limitation was even documented in `resolve_at_path`'s doc
comment ("there is no per-process cwd in the native path resolver").

**Fix (proper):** Resolve relative paths against the caller's cwd at the
Linux ABI boundary, reusing the existing `canonicalize_path(cwd, path)`
helper (already used by the chroot gate and `fstatat`). `open_common`
(kernel/src/syscall/linux.rs) now canonicalises the path against
`pcb::get_cwd(caller)` and opens via a new `handlers::fs_open_kernel_path`
(a kernel-string variant of `sys_fs_open` that does the File-READ cap check
+ handle registration without reading userspace), and `resolve_at_path`
canonicalises its `AT_FDCWD`/absolute result the same way. Kernel context
(no caller PID) falls back to cwd `"/"`, preserving the prior behaviour for
in-kernel callers and the native ABI (`sys_fs_open` is untouched). Absolute
paths are normalised but otherwise unchanged. Regression test: Path Z
Part 23 (`self_test_linux_real_glibc_shell_relpath`) runs `cd /reltest &&
echo RELOK > relfile.txt` in ring 3 and asserts the file landed at
`/reltest/relfile.txt` and **not** at `/relfile.txt`.

### B-ACCESS1. Linux-ABI `access`/`faccessat`/`faccessat2` always returned ENOENT (no-file skeleton-FS stub) — FIXED 2026-06-16

**Symptom:** Every `access`/`faccessat`/`faccessat2` call returned `-ENOENT`
unconditionally, even for files that exist in the VFS. The headline casualty
was unmodified GNU `make`: make issues `access("/bin/sh", X_OK)` **before**
spawning a recipe and, on failure, prints `"/bin/sh: No such file or directory"`
+ `Error 127` and never spawns the recipe shell — so no Makefile recipe could
run. (Confirmed via `strace` on real Linux: `access(shell, X_OK) = 0` precedes
the `clone3`.) Same class of stale stub as B-STAT1, but for the accessibility
probes rather than `stat`.

**Root cause:** `sys_access` / `sys_faccessat` / `sys_faccessat2` validated the
mode/flag bits and the path pointer, then hard-coded `linux_err(errno::ENOENT)`
with a comment that "without a backing filesystem there is no path that exists."
True when written; a silent lie once the VFS gained a backing store.

**Fix (proper):** The three syscalls now share a new `access_path_common`
back-end (kernel/src/syscall/linux.rs) that canonicalises the path against the
caller's cwd (`pcb::get_cwd`) and looks it up via `Vfs::metadata` (follow) /
`Vfs::lmetadata` (`AT_SYMLINK_NOFOLLOW`). Under the no-DAC capability model
(design-decisions §31) `F_OK`/`R_OK`/`X_OK` succeed for any existing file/dir —
consistent with `execve`, which ignores on-disk x-bits. Kernel context (no
caller PID) preserves the ENOENT no-file contract the fidelity self-tests
assert. Regression test: Path Z Part 34 (`self_test_linux_real_glibc_make`) runs
real GNU make end-to-end, whose recipe dispatch depends on `access(shell, X_OK)`.

**Known limitation (W_OK):** `W_OK` is granted for any existing file; it does
not yet consult per-mount read-only state (not tracked at this layer). A
read-only mount should return `EROFS` for `W_OK`. Low priority — no read-only
mounts are exposed to ring-3 writers today.

### B-ABI1. A *bare* static Linux ELF (no OSABI/PT_INTERP/PT_GNU_PROPERTY) is misclassified as Native-ABI on `exec` — KNOWN LIMITATION (escalated as open-questions.md Q9)

**Symptom:** A Linux binary with none of the markers `elf::detect_linux_abi`
keys off — e.g. the output of `tcc -nostdlib -static`, or a hand-rolled static
musl/asm program (OSABI=`SYSV`/0, no `PT_INTERP`, no `PT_GNU_PROPERTY`; the only
GNU-ish artifact is a `PT_GNU_RELRO` segment, deliberately rejected as a signal)
— is classified as a **Native-ABI** process. Its raw `syscall`s are then routed
through the native dispatch table instead of `kernel::syscall::linux`, so e.g.
`write(1, …)` produces 0 observable bytes and `exit(n)` loses its status. This
bites the **`exec` path** (a shell or `make` exec'ing a freshly-built bare tool),
which re-detects the ABI from the ELF with no way for the caller to override.

**Root cause:** A bare SYSV static ELF carrying only generic GNU-toolchain
artifacts is genuinely ambiguous between "Linux binary" and "SlateOS-native
binary built with a GNU/LLVM toolchain." No automatic heuristic separates them
reliably; disambiguation needs an explicit marker on one side.

**Worked around (spawn only):** `spawn::spawn_process_with_abi(elf, options,
AbiMode::Linux)` lets an in-kernel caller that *knows* the binary's ABI state it
explicitly (used by `self_test_linux_real_glibc_cc`, which just compiled the
binary as a Linux program). This does **not** cover the general `exec` path.

**Proper fix (deferred — needs operator decision):** open-questions.md **Q9** —
recommendation is to default unmarked bare ELFs to the Linux ABI and stamp
SlateOS-native binaries with an explicit OSABI value / `.note.slateos`, plus add
`NT_GNU_ABI_TAG` note-walking as a positive Linux signal. Where it bites:
`kernel/src/proc/elf.rs::detect_linux_abi`, `spawn.rs::spawn_process_inner`, and
the `exec` `new_abi_mode` path.

### B-SPAWN1. `posix_spawn`/`vfork` child loses the exec-failure errno under CoW-fork degradation — KNOWN LIMITATION (acceptable)

**Symptom:** When a glibc `posix_spawn(3)` (or `vfork`) child fails its
`execve` (e.g. the target is missing), the parent observes a child that exited
with status 127 rather than receiving the precise `errno` glibc's posix_spawn
normally reports.

**Root cause:** glibc's posix_spawn does `clone3({CLONE_VM|CLONE_VFORK|
CLONE_CLEAR_SIGHAND, ...})` expecting a **shared** address space: on exec
failure the child writes `errno` to a stack location the parent then reads. Our
processes are address-space isolated, so `linux_clone_inner`'s VFORK_SPAWN
branch degenerates the shared-VM vfork to a copy-on-write fork. The child runs
on its own copied stack, so a post-fork write to the (formerly shared) errno
slot is invisible to the parent — only the child's exit status survives. The
common success case is unaffected (the child execve's and never writes back).

**Proper fix (deferred):** Genuine `CLONE_VM` shared-address-space semantics for
the vfork window, or a kernel-mediated errno relay from the failing child's
exec path back to the parent's clone return. Deferred until a workload depends
on the precise errno; status-127 is the universally-understood "exec failed"
signal and is what shells display anyway.

### B-STAT1. Path-based `stat`/`lstat`/`newfstatat`/`statx` always returned ENOENT (no-file skeleton-FS stub) — FIXED 2026-06-16

**Symptom:** Every path-based stat syscall returned `-ENOENT` unconditionally,
even for files that exist in the VFS. Any program that stats a path before
opening it saw the file as missing: dash's `[ -f FILE ]` / `[ -e FILE ]` /
`[ -d DIR ]` test predicates were always false, `ls FILE`, `stat FILE`, and
`configure`-style existence probes all failed. `fstat` (fd-based) worked, so
this only bit the path-based variants. (Distinct from B-CWD1, which was about
*relative* path resolution — B-STAT1 returned ENOENT even for valid
*absolute* paths.)

**Root cause:** The handlers carried a stale "no files exist on our skeleton
FS" assumption from before the VFS held real files. `stat_path_impl`
(shared by `stat`/`lstat`) and the non-empty-path branches of
`sys_newfstatat` / `sys_statx` validated the path pointer and then hard-coded
`linux_err(errno::ENOENT)` with comments explaining that `filename_lookup`
"always fails on our no-file FS". That was true when written but became a
silent lie once the VFS gained a backing store.

**Fix (proper):** Do a real VFS lookup for ring-3 callers.
`stat_path_impl` was rewritten into `stat_path_common(path_ptr, statbuf_ptr,
follow)` which canonicalises the path against the caller's cwd
(`canonicalize_path` + `pcb::get_cwd`) and resolves it via new helpers
`stat_meta_for_path` (calls `Vfs::metadata` when `follow`, else
`Vfs::lmetadata`; maps `NotFound`→ENOENT) + `fill_stat_from_meta` /
`fill_statx_from_meta` (map `EntryType`→`S_IF*` bits, synthesise default
perms `0o755`/`0o777`/`0o644` when the FS reports `permissions == 0`, and
backfill 0 timestamps with `clock_realtime()`). `sys_newfstatat` / `sys_statx`
non-empty branches resolve via `resolve_at_path(dirfd, path)` (dirfd + cwd
rules) then real-stat-and-fill, with `follow = (flags & AT_SYMLINK_NOFOLLOW)
== 0`. Statbuf pointer gates are deferred until *after* a successful lookup,
matching Linux's `getname`/`filename_lookup`-before-`cp_new_stat` ordering
(so `stat("missing", NULL)` returns ENOENT, not EFAULT). Kernel context
(`caller_pid().is_none()`) still returns ENOENT, preserving the batch-488
syscall-fidelity self-tests (which pass a fake pointer in kernel context).
Regression test: Path Z Part 24 (`self_test_linux_real_glibc_shell_statpath`)
runs `[ -f /bin/dash ] && echo HASFILE > /stat-out.txt` in ring 3 and asserts
the redirect fired (8 bytes, exit 0), proving the `-f` predicate's path stat
now succeeds.

### B-SYM1. Linux-ABI `symlink`/`symlinkat` returned EROFS and `readlink`/`readlinkat` returned EINVAL unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could create or resolve a symbolic link. Every
`symlink(2)`/`symlinkat(2)` returned `-EROFS` ("read-only file system") and
every `readlink(2)`/`readlinkat(2)` returned `-EINVAL` ("not a symlink"),
regardless of whether the path existed or was actually a symlink, even though
the VFS is fully writable and natively supports symlinks (`Vfs::symlink`,
`Vfs::readlink`). This breaks any toolchain that relies on symlinks (build
systems, `ld` SONAME links, package layouts).

**Root cause:** The four handlers were placeholder stubs left over from before
the VFS gained symlink support: `sys_symlink`/`sys_symlinkat` validated their
path arguments and then hard-coded `linux_err(errno::EROFS)`, and
`sys_readlink`/`sys_readlinkat` hard-coded `linux_err(errno::EINVAL)` after
the argument gates. The stubs' errno terminals were also (correctly) asserted
by the batch-478/487 syscall-fidelity self-tests, which call the handlers in
kernel context with fake pointers — so the fix had to keep those terminals for
kernel callers while doing real work for ring-3 callers.

**Fix (proper):** Wire all four to the VFS for ring-3 callers.
`sys_symlink`/`sys_symlinkat` share a new `symlink_common(target_ptr,
newdirfd, linkpath_ptr)` that stores the `target` *verbatim* (a symlink may
dangle and may be relative — it is NOT resolved or canonicalised), resolves
the `linkpath` against the caller's cwd / `newdirfd` via `resolve_at_path`,
requires a File-WRITE capability (`require_fs_write`), and calls
`Vfs::symlink`. `sys_readlink`/`sys_readlinkat` share a new
`do_readlink_copy(path, buf_ptr, bufsiz)` that calls `Vfs::readlink` and
copies `min(target_len, bufsiz)` bytes with **no** trailing NUL, returning the
byte count (the Linux `do_readlinkat` contract); the user buffer is validated
and written only *after* the dentry is confirmed to be a symlink
(`NotFound`→ENOENT, `InvalidArgument`→EINVAL/"not a symlink"). Kernel context
(`caller_pid().is_none()`) preserves the prior EROFS/EINVAL terminals;
`sys_readlink` canonicalises the path against `pcb::get_cwd` first, the `*at`
variants use `resolve_at_path`. Regression test: Path Z Part 27
(`self_test_linux_symlink_readlink`) — a hand-built raw-syscall Linux-ABI ELF
(`build_linux_symlink_readlink_test_elf`) calls `symlink("Z", "/sl-rl-link")`
then `readlink("/sl-rl-link", buf, 64)` from ring 3 and asserts the call
returned exactly 1 byte == `'Z'` (self-diagnosing exit sentinels
`0xB1`/`0xB3`/`0xB4`). The harness pre-removes the link path and, after the
process exits 0, independently confirms kernel-side via `Vfs::readlink` that
the created link resolves to `"Z"`. (Raw ELF rather than dash because dash has
no `ln` builtin and cannot invoke `symlink(2)`/`readlink(2)` directly.)
**Follow-up — `link`/`linkat` now wired (2026-06-16):** `sys_link`/`sys_linkat`
share a new `link_common` that resolves both names via `resolve_at_path`,
requires a File-WRITE capability, and calls `Vfs::link` (kernel context still
EROFS). Regression test: Path Z Part 28 (`self_test_linux_link`) hard-links
`/mnt/lnk-dst` to a pre-staged `/mnt/lnk-src` from ring 3 and reads the byte
back through it.

**memfs does not support hard links (deferred):** the test runs on the **ext4**
mount at `/mnt`, not the in-memory root (`/`, `/tmp`). memfs stores file data
inline in by-value tree nodes (`MemFsNodeKind::File(Vec<u8>)` owned by the
parent's `BTreeMap`), so two directory entries cannot share one inode — which
is exactly what a hard link requires. memfs therefore correctly returns
"unsupported" (Linux returns **EPERM** for filesystems without hard-link
support). Proper fix: refactor memfs to an inode-table model (`MemFs` owns
`BTreeMap<ino, Inode>`; file/symlink directory entries hold an `ino` instead of
the body, so multiple names can reference one inode with a shared `nlink`).
This is a sizeable refactor of a core subsystem with many passing self-tests,
and ext4 (the design's real root FS) already implements hard links, so it is
deferred rather than done speculatively. **Fidelity gap (minor):** `Vfs::link`
always follows a symlink `oldpath`, whereas plain `link(2)` should not follow
and `linkat` should follow only with `AT_SYMLINK_FOLLOW`; the common
regular-file case is correct, only the rare hard-link-a-symlink case differs
(would need a `Vfs::link` no-follow variant to fix properly).

**Follow-up — `utimensat`/`utimes`/`utime` now wired (2026-06-16):** see
B-UTIME1 below.

### B-UTIME1. Linux-ABI `utimensat`/`utimes`/`utime` returned EROFS unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could update a file's access/modification
timestamps. Every `utimensat(2)`/`utimes(2)`/`utime(2)` returned `-EROFS`
after the (Linux-faithful) input-shape gate ladder, even though the VFS is
writable and `Vfs::set_times` is implemented by memfs, ext4 and fat. `touch`,
`make` (which stamps targets), `tar -x` (restores mtimes) and configure
scripts all depend on this.

**Root cause:** The three handlers were placeholder stubs that validated their
arguments (and faithfully reproduced Linux's `EINVAL`/`ENOENT`/(OMIT,OMIT)→0
input-shape diagnostics — batch 489) and then hard-coded
`linux_err(errno::EROFS)`. The EROFS terminal is also asserted by the
batch-489 fidelity self-tests, which call the handlers in kernel context, so
the fix had to keep that terminal for kernel callers.

**Fix (proper):** For ring-3 callers (`caller_pid().is_some()`) each handler
now resolves the target path (`resolve_at_path` against the caller's cwd /
dirfd; `utimensat` with a NULL pathname resolves the open file behind `dirfd`
via `handle_path`), translates the parsed `timespec`/`timeval`/`utimbuf` into
ns-since-epoch (`UTIME_NOW`→`clock_realtime`, `UTIME_OMIT`/NULL-field→leave
unchanged, otherwise `sec*1e9 [+ sub-second]`), requires a File-WRITE
capability, and calls `Vfs::set_times`. Kernel context preserves the EROFS
terminal. Regression test: Path Z Part 29 (`self_test_linux_utimensat`) — a
hand-built raw-syscall ELF (`build_linux_utimensat_test_elf`) calls
`utimensat(AT_FDCWD, "/utimensat-test", {atime=1.6e9 s, mtime=1.5e9 s}, 0)`
from ring 3 (self-diagnosing exit sentinel `0xD1`); the harness stages the file
on the memfs root (memfs implements `set_times`) and, after exit 0,
independently asserts the kernel-side `Vfs::metadata` reports
`accessed_ns == 1.6e18` and `modified_ns == 1.5e18` exactly.

**Fidelity gaps (minor, documented in the linux.rs module comment):**
1. `Vfs::set_times` always follows symlinks, so `utimensat`'s
   `AT_SYMLINK_NOFOLLOW` is a no-op (the target is touched, not the link).
   Proper fix needs a `Vfs`/`Filesystem` no-follow `lset_times` variant.
2. The `Timestamp = u64` VFS API overloads `0` ("ns since epoch") as the
   "leave this field unchanged" sentinel, so a request to set a field to
   exactly the Unix epoch (or any pre-epoch / negative instant) is silently
   treated as "leave unchanged". Proper fix needs an `Option<u64>` (or
   explicit "omit" flag) plumbed through `Filesystem::set_times` for every FS.

### B-CHOWN1. Linux-ABI `chmod`/`fchmod`/`fchmodat`/`fchmodat2`/`chown`/`lchown`/`fchown`/`fchownat` returned EROFS unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could change a file's permission bits or
ownership. The whole chmod/chown family returned `-EROFS` after input
validation, even though the VFS tracks Unix mode bits and uid/gid and
implements `Vfs::set_permissions` / `Vfs::set_owner` across memfs/ext4/fat.
`install -m`, `chmod +x`, `tar -x` (restores perms/owner), and package
managers all depend on this.

**Root cause:** Placeholder stubs that validated arguments (faithfully
reproducing Linux's `ENOENT`/`EBADF`/`EINVAL` input-shape diagnostics —
batches 483/484) and then hard-coded `linux_err(errno::EROFS)`. The EROFS
terminal is asserted by those fidelity self-tests in kernel context.

**Fix (proper):** For ring-3 callers each handler resolves the target
(`resolve_at_path` for the path variants against the caller's cwd/dirfd;
`handle_path` on the open file for the `fchmod`/`fchown` fd variants and the
`fchownat(AT_EMPTY_PATH)` form), requires a File-WRITE capability, and calls
`Vfs::set_permissions` (mode masked to `0o7777`) or `Vfs::set_owner` (uid/gid
narrowed to 32 bits; the `(uid_t)-1`/`(gid_t)-1` "leave unchanged" sentinels
are honoured by `Vfs::set_owner`). Kernel context keeps the EROFS terminal.
`fchmod`/`fchown` on a non-file fd (pipe/console, no backing inode) return
EINVAL. Regression test: Path Z Part 30 (`self_test_linux_chmod_chown`) — a
hand-built raw-syscall ELF (`build_linux_chmod_chown_test_elf`) calls
`chmod("/chmod-chown-test", 0o640)` then `chown(path, 1234, 5678)` from ring 3
(sentinels `0xE1`/`0xE2`); the harness stages the file on the memfs root and,
after exit 0, independently asserts `Vfs::metadata` reports
`permissions == 0o640`, `uid == 1234`, `gid == 5678`.

**Follow-up (`fchmodat2`, syscall #452):** the 4-arg flags-aware chmod
(`fchmodat2`) was a separate EROFS stub missed by the first pass; it was
wired in the same idiom as `sys_fchownat` during the truncate-line cleanup.
`AT_EMPTY_PATH` resolves `dirfd` to its backing path (AT_FDCWD → cwd, else an
open File fd via `handle_path`); the non-`AT_EMPTY_PATH` branch keeps the
empty-path → ENOENT discrimination then `resolve_at_path` + `chmod_apply`.
Kernel context keeps the EROFS terminal (batch-485 self-test still green).
Regression test: Path Z Part 32 (`self_test_linux_fchmodat2`) —
`build_linux_fchmodat2_emptypath_test_elf` `open(O_RDWR)`s `/fchmodat2-test`
and calls `fchmodat2(fd, "", 0o600, AT_EMPTY_PATH)` (sentinels `0xE5`/`0xE6`);
the harness confirms `Vfs::metadata` reports `permissions == 0o600`.

**Fidelity gaps (minor):**
1. `lchown` and `fchownat(AT_SYMLINK_NOFOLLOW)` must operate on the symlink
   itself, but `Vfs::set_owner` always follows the final symlink (same
   no-follow gap as B-SYM1 for `link`). The common non-symlink case is
   correct; a proper fix needs a no-follow `lset_owner` VFS variant.
2. We gate chmod/chown on the generic File-WRITE capability rather than a
   dedicated `CAP_CHOWN`/`CAP_FOWNER`; any process holding File-WRITE can
   change mode/owner. This matches the OS's capability model (no per-syscall
   POSIX capability bits yet) but is laxer than Linux's privilege checks.

### B-TRUNC1. Linux-ABI `truncate`/`ftruncate` returned EROFS unconditionally for confirmed regular files (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could resize a file. Both `truncate(2)` and
`ftruncate(2)` ran their full input-shape gate ladders (EINVAL on negative
length, EFAULT/ENOENT on bad/empty/missing paths, EISDIR on directories,
EINVAL on non-regular inodes / read-only-fd via the FMODE_WRITE check) and
then hard-coded `linux_err(errno::EROFS)` for a confirmed regular file —
even though the VFS exposes `Vfs::truncate` implemented across
memfs/ext4/fat. Every database that pre-sizes its file (sqlite, lmdb),
log rotators, `dd`/`truncate(1)`, `fallocate`-fallback paths, and `./configure`'s
`AC_FUNC_FTRUNCATE` probe depend on this.

**Root cause:** Placeholder terminals from the universal-read-only era.
`sys_truncate` already did a real `Vfs::stat` triage (so the EISDIR/EINVAL/
ENOENT diagnostics were live), and `sys_ftruncate` already enforced the
`FMODE_WRITE` gate; only the final File-arm answer was a hard-coded EROFS.
Both terminals are asserted in kernel context by the batch-447/448 fidelity
self-tests, which short-circuit on `caller_pid().is_none()`.

**Fix (proper):** For ring-3 callers `sys_truncate` (after the stat triage
confirms a regular file) and `sys_ftruncate`'s File arm now enforce
`RLIMIT_FSIZE` (EFBIG on a grow past the soft limit — the check returns to
its proper Linux position now that mounts are writable, between the
`FMODE_WRITE`/`mnt_want_write` gate and `do_truncate`), require a File-WRITE
capability, resolve the target (`canonicalize_path` for the path variant,
`handle_path` on the open fd for `ftruncate`), and call `Vfs::truncate`,
which grows-with-zeros or shrinks the file. Kernel context keeps the EROFS
terminal (gated on `caller_pid().is_none()`). Regression test: Path Z Part 31
(`self_test_linux_truncate`) — a hand-built raw-syscall ELF
(`build_linux_truncate_test_elf`) shrinks `/truncate-test` to 4 bytes via the
path syscall, then `open(O_RDWR)` + `ftruncate(fd, 10)` grows it; sentinels
`0xF1`/`0xF2`/`0xF3`. The harness stages a 16-byte file on the memfs root and,
after exit 0, independently asserts the readback is exactly 10 bytes with the
leading 4 preserved (`'A'`) and the grown tail zero-filled.

**Fidelity gaps (minor):**
1. `Vfs::truncate` follows the final symlink (`resolve_follow`), matching
   Linux's `truncate(2)` (which also follows). `ftruncate` operates on the
   fd's already-resolved inode, also correct. No no-follow truncate exists
   in Linux, so there is no gap here — noted only for symmetry with B-CHOWN1.
2. We gate the resize on the generic File-WRITE capability; Linux additionally
   honours `IS_APPEND`/`IS_IMMUTABLE` inode flags (EPERM) and the per-fd
   `O_APPEND`-doesn't-block-truncate nuance. The append/immutable-flag EPERM
   path is not yet plumbed (same capability-model gap as B-CHOWN1).

### B-FALLOC1. Linux-ABI `fallocate` COLLAPSE_RANGE/INSERT_RANGE now shift contents; only UNSHARE_RANGE still EOPNOTSUPP — PARTIALLY RESOLVED 2026-06-18, COLLAPSE/INSERT ADDED 2026-06-20

**Status:** `sys_fallocate` (syscall #285) was wired 2026-06-16 (Path Z Part 33)
from a blanket EOPNOTSUPP terminal to the real VFS for the two *allocate* modes:
`mode == 0` (posix_fallocate grow → `Vfs::file_size`/`Vfs::truncate`, never
shrinking) and `FALLOC_FL_KEEP_SIZE` (block reservation → `Vfs::fallocate`).
MemFd fds grow via `ipc::memfd::truncate`. Both enforce `RLIMIT_FSIZE` (EFBIG)
and the File-WRITE capability.

**Update 2026-06-18 — PUNCH_HOLE / ZERO_RANGE implemented.** The two most
commonly used range modes now do real work instead of returning EOPNOTSUPP. New
helpers `fallocate_zero_vfs` / `fallocate_zero_memfd` (kernel/src/syscall/linux.rs)
zero `[offset, offset+len)` in 16 KiB chunks via the backend's efficient
`write_at` (ext4/fat/memfs all override it). `i_size` is preserved for PUNCH_HOLE
(always KEEP_SIZE) and ZERO_RANGE+KEEP_SIZE — the zeroed region is clamped to the
current size and a range entirely past EOF is a no-op; ZERO_RANGE *without*
KEEP_SIZE grows the file to `offset+len` if the range crosses EOF, zero-filling
the gap. This is correct **read-as-zero** behaviour; the only thing not provided
vs. a real hole-punch is **disk-space reclamation** (an optimisation, not a
correctness property — our backends are non-sparse). Covered by
`self_test_fallocate_range` (registered in kernel/src/main.rs as a late, post-/tmp
self-test) which exercises ZERO_RANGE+KEEP_SIZE, PUNCH_HOLE, a past-EOF KEEP_SIZE
no-op, a ZERO_RANGE grow, and a MemFd ZERO_RANGE — all green at boot.

**Update 2026-06-20 — COLLAPSE_RANGE / INSERT_RANGE implemented.** Both
content-shifting modes now do real work for regular files (`HandleKind::File`)
instead of returning EOPNOTSUPP. The dispatch (kernel/src/syscall/linux.rs
`sys_fallocate`) enforces the full Linux contract: it queries the backing fs
block size via `Vfs::statvfs` and rejects a non-block-aligned `offset`/`len`
with EINVAL; COLLAPSE at/past EOF is EINVAL (Linux says use ftruncate); INSERT
at/past EOF (`offset >= size`) is EINVAL; INSERT also re-checks RLIMIT_FSIZE
against the *grown* size (`size + len`). The shifts themselves are chunked
(16 KiB) memmoves over `Vfs::read_at`/`write_at`: `fallocate_collapse_vfs`
slides the tail down (ascending copy, dst < src) then truncates by `len`;
`fallocate_insert_vfs` grows the file, slides the tail up (descending copy to
avoid clobber) then zeroes the inserted `[offset, offset+len)` hole. Our
backends are non-sparse, so this is a true content collapse/insert (not an
extent splice) — byte-for-byte identical from a reader's view; the only thing
not provided vs. a native ext4 extent op is the in-place efficiency, an
optimisation, not a correctness property. Covered by `self_test_fallocate_range`
cases (6)-(8): COLLAPSE_RANGE, INSERT_RANGE, and an INSERT+COLLAPSE round-trip
identity, all green at boot. A backend whose `statvfs` reports `block_size == 0`
(can't validate the alignment contract) keeps the EOPNOTSUPP fallback.

**Remaining limitation:** `UNSHARE_RANGE` still returns EOPNOTSUPP — it is a
reflink/CoW unshare concept our backends don't implement (there are no shared
extents to unshare). Well-behaved callers treat EOPNOTSUPP as "operation
unsupported" and skip it or fall back, so nothing breaks.

**Proper fix (deferred) for UNSHARE:** once a backend grows reflink/CoW extents
(none do today), dispatch UNSHARE_RANGE to a preallocate-and-unshare path; on a
non-reflink fs it is correctly a no-op (nothing is shared), so the EOPNOTSUPP
terminal is the conservative choice until reflinks exist. Kernel context
(caller_pid None) keeps the EOPNOTSUPP terminal for every mode, asserted by the
batch-536 FMODE_WRITE + vfs_fallocate gate-order self-tests.

### B-SIG1. dash's `wait` builtin (background-job reap) livelocked: no SIGCHLD on child exit + `rt_sigsuspend` was a stub — FIXED 2026-06-16

**RESOLVED 2026-06-16.** A real glibc `dash` running `/bin/emit > file &
wait` (Path-Z self-test `self_test_linux_real_glibc_shell_bgjob`) hung the
boot thread to a timeout. dash's `wait` builtin uses
`dowait(DOWAIT_BLOCK|DOWAIT_WAITCMD)`, whose `waitproc` computes
`flags = WNOHANG` (because `DOWAIT_WAITCMD` makes `block != DOWAIT_BLOCK`),
then loops `while (!gotsigchld && !pending_sig) sigsuspend(&oldmask)` —
relying on SIGCHLD delivery (its handler sets `gotsigchld`). The
synchronous pipe/loop/cmdsub waits use blocking `waitpid` (flags 0) and
never needed SIGCHLD, which is why those parts passed.

Two kernel gaps caused the livelock, both fixed properly:

1. **SIGCHLD was never posted to the parent on child exit.**
   `kernel/src/proc/thread.rs::on_thread_exit` now posts SIGCHLD to the
   parent when a child becomes a zombie — via the Linux-ABI disposition
   path (`signal::set_pending_info`, delivered by
   `deliver_linux_signal` → `linux_disposition`) for Linux parents, and
   `classify_post_info` for native parents (SIGCHLD's default action is
   ignore, so a no-handler parent correctly drops it). This is distinct
   from the existing `wait4()` waiter wakeups, which target a thread parked
   in `wait4()`, not the signal path.

2. **`sys_rt_sigsuspend` was a stub returning EINTR immediately.** This
   made dash busy-spin (`sigsuspend` → EINTR → re-loop → …), starving the
   boot thread. It is now a real park loop modeled on `sys_pause`
   (`kernel/src/syscall/linux.rs`): it installs the temporary mask, parks
   on the signalfd wait-queue until a signal deliverable under that mask
   arrives, and restores the original mask correctly via a Linux
   `saved_sigmask`/`TIF_RESTORE_SIGMASK` mechanism — `emit_linux_rt_frame`
   writes the saved pre-suspend mask into the handler frame's `uc_sigmask`
   (so `rt_sigreturn` restores it), and the no-handler tail of
   `deliver_linux_signal` restores it directly. The contextless
   (in-kernel, `caller_pid()==None`) case still returns EINTR immediately
   so the existing rt_sigsuspend self-test is unaffected.

**Verify:** boot test reaches `BOOT_OK`; the bgjob self-test logs "read
back 16 bytes == expected, exit 0: OK".

### B-HEAP1. Kernel heap redzone "overflow" reports during init file-install were FALSE POSITIVES from a pre-poison allocation window — FIXED 2026-06-16

**Symptom (as originally observed):** During boot (init step 24, after all
self-tests), the debug heap allocator's dealloc-time redzone scanner reported
several `[heap] BUFFER OVERFLOW detected! slot=…, alloc=N, class=C, offset=N`
lines, e.g. `alloc=10, class=16, offset=10` (right before
`[init] Installed /bin/hello`) and two `alloc=18, class=32, offset=18`. Boot
still reached `BOOT_OK` and all self-tests passed.

**Root cause (NOT a real overflow):** The redzone check relies on the invariant
"every byte in `[alloc_size, class_size)` is `ALLOC_POISON` (0xCD)". That holds
only if the slot was `poison_alloc`'d *at the time it was handed out*. But
`enable_poison()` was called very late in boot (`kernel/src/main.rs` step 22f-3,
old line ~3518) while the heap is initialized far earlier (`mm::heap::init`,
~line 455). **Every allocation made in that window was never poison-filled.**
When such a slot was later freed *after* poisoning came online, `check_redzone`
scanned whatever bytes the pre-poison occupant had left there — zeroed
fresh-frame bytes, or stale content from an earlier reuse — and reported them as
overflow. Captured byte dumps confirmed this: a slot freed with `alloc_size=18`
held the intact 31-char string `/tmp/tmpwatch_test/delete_me.tmp` filling the
whole 32-byte class (a former occupant), and `"/bin/hello"+'e'+zeros` showed
unpoisoned (zero) redzone bytes — neither is possible if the slot had actually
been alloc-poisoned. So the reports were detector false positives, not memory
corruption.

**Fix:** Move `mm::heap::enable_poison()` to immediately after `mm::heap::init()`
(`kernel/src/main.rs`, step 6), *before the first heap allocation*. With no
pre-poison allocation window, every slab slot is poison-filled at its first
alloc and the redzone invariant always holds. The redundant late
`enable_poison()` at step 22f-3 was removed (the `poison_self_test()` call
stays). Poison is still toggled OFF only for the duration of the heap
benchmarks (`deferred_bench_task`), which free their own allocations within that
window. Note this only affects slab classes (≤ 8192 B); large allocations (the
actual MB-sized binaries) go through the buddy path and are never poisoned or
redzone-checked, so the early-enable adds negligible boot cost.

### B-DP1. `validate_user_range` rejected committed-but-not-yet-faulted-in demand-paged user buffers (EFAULT on large fresh output buffers) — FIXED 2026-06-16

**RESOLVED 2026-06-16.** `kernel/src/mm/user.rs::validate_user_range`
(the core of `validate_user_read`/`validate_user_write`) walked every
4 KiB page of a user buffer and returned `InvalidAddress` the moment
`page_table::translate()` reported a page *not present*. That is wrong
for **demand-paged** memory: a freshly-`malloc`/`mmap`'d buffer is
committed (covered by a VMA) but its pages are not populated until first
touched. A syscall handed such a buffer as an *output* target would
EFAULT on every page past the first, because the process had not yet
written to those pages itself.

**Reproduce:** run `dash -c 'echo /globdir/* > out'` (Path-Z real-glibc
self-test `self_test_linux_real_glibc_shell_glob`). glibc's `opendir`
allocates a 32 KiB dirent buffer and calls `getdents64` into it before
touching it; the buffer's later pages were not present, so
`validate_user_write(dirp, 32768)` returned EFAULT, `readdir` returned
NULL, and dash's glob matched nothing — emitting the literal `/globdir/*`
instead of the three filenames. (The directory open, VFS readdir, and
getdents64 encoding were all proven correct via tracing; the validation
pre-walk was the sole culprit.)

**Fix:** when the pre-walk finds a not-present page, call the new
`try_fault_in_user_page(addr, need_writable)`, which synthesizes an x86
page-fault error code (not-present + user + write-iff-needed) and routes
it through `crate::proc::pcb::try_resolve_fault` — the same demand-paging
resolver the hardware #PF handler uses — then re-checks `translate()`.
This mirrors Linux's `get_user_pages()` faulting pages in before a
kernel-side access. A genuinely unmapped or permission-violating address
still fails (the resolver returns `false`), so invalid pointers are still
rejected. **Validated:** the dash glob self-test now reads back the
expected 45 bytes (`/globdir/a.txt /globdir/b.txt /globdir/c.txt\n`),
exit 0; full boot test passes with no self-test failures.

### B-DF1. Kernel-stack overflow → double fault when an IRQ frame pushes onto a near-full kernel task stack (deferred benchmark suite) — FIXED 2026-06-15 (Q7 option A)

**RESOLVED 2026-06-15.** Fixed via `open-questions.md` Q7 → **option A**
(operator-chosen): a dedicated per-CPU guard-page IRQ stack with a manual
nesting-aware switch in `idt::irq_common_dispatch` (so hardware IRQ frames/
handlers never consume the interrupted task's stack), plus **deferred
preemption** (timer ISR sets `NEED_RESCHED`; the outermost IRQ frame runs the
context switch on the task stack via `sched::do_deferred_preempt`). The
restructuring also exposed an **unbounded re-entrant preemption recursion**
(nested timer tick during `schedule_inner`, with interrupts enabled on the task
stack, misclassified as a fresh outermost IRQ → recursion until guard-page
overflow); fixed by disabling interrupts across the involuntary switch in
`do_deferred_preempt`. See `design-decisions.md` §26. **Validated:**
`http_gzip_8KiB` — which previously double-faulted entering the dashboard benches
on a near-full task stack — now runs to completion.

**Follow-up 2026-06-15 — `BENCH_OK` now reached end-to-end.** After the Q7
landing, two further blockers were chased to ground:

1. **The previously-documented `bench_isr_latency` null-pointer crash no longer
   reproduces.** It was an artifact of the *old* timer-ISR path that called
   `preempt()` inline during the hard-IRQ handler; the Q7 deferred-preempt
   restructuring (timer ISR only sets `NEED_RESCHED`; the switch runs later on
   the task stack) removed it. Verified by running `bench_isr_latency()` both
   early and in its normal end-of-suite slot — it completes cleanly (≈54 µs
   hard-IRQ phase under TCG, above the 10 µs target but that is emulation
   noise, not a fault). The stale `todo.txt` "Cross-Zone Bug Reports" entry is
   superseded.

2. **The actual last `BENCH_OK` blocker was a scheduler self-deadlock, now
   fixed.** `bench_dashboard_api_status` calls `dashboard::api_status()` →
   `sched::task_list()`, which holds `SCHED` (a plain `spin::Mutex`) across a
   heap `Vec` collect over *all* tasks. Run 1000× in a tight loop, a timer tick
   reliably lands while the task holds `SCHED`; the Q7 deferred-preempt then ran
   `preempt() → schedule_inner() → SCHED.lock()` on the *same* CPU and spun
   forever (the `cli` in `do_deferred_preempt` made the hang unrecoverable). The
   fix: `do_deferred_preempt` now checks `SCHED.is_locked()` and, if held,
   re-arms `NEED_RESCHED` and defers to the next tick instead of blocking — the
   same try/skip discipline `unthrottle_expired()` already uses from ISR
   context. This closes the *entire* "involuntary preempt while the interrupted
   context holds SCHED" deadlock class (including the tiny analogous window
   during voluntary `yield_now`/`block`), at the single involuntary-preempt
   site. **Validated: the full `--bench` suite now reaches `BENCH_OK` ("Boot
   test PASSED").** See `design-decisions.md` §27.

The original analysis is retained below for history.

**Root cause (CONFIRMED): kernel task stack overflow into the guard page.**
The deferred benchmark suite runs heavy, *debug-built* code paths in kernel
context (gzip/deflate, `format!`-heavy JSON, crypto) on a kernel task with a
fixed **64 KiB** stack (`TASK_STACK_SIZE = 4 * 16 KiB`). The kstack allocator
(`kernel/src/mm/kstack.rs`) lays out each task stack as `[guard 16 KiB][stack
64 KiB]`, slot stride `SLOT_SIZE = 0x14000`, region base `0xFFFF_C100_0000_0000`.
The reported fault `RSP = 0xffffc1000003ffb8` decodes to slot 3, within-slot
offset `0x3FB8`, which is **< GUARD_SIZE (0x4000)** — i.e. RSP is **inside the
guard page**, ~72 bytes below `stack_bottom`. So the stack overflowed; the
faulting `atomic_load` (and the IRQ frame that the CPU was pushing) landed on
the unmapped guard page → the fault could not be delivered → #DF.
(Correction to an earlier note: RSP is **not** "near the top of the stack" — I
had mis-decoded the slot stride. It is firmly in the guard page. The two
backtrace frames are the #DF handler's own IST stack — `handle_double_fault` /
`isr_double_fault` — and are uninformative.)

**Why an IRQ tips it over.** Hardware IRQs (timer vector 32; device IRQs 33–56,
incl. mouse IRQ12) are installed in the IDT with **IST index 0** (see
`idt.rs::init`, `IdtEntry::new(..., 0, 0)`) — they run on the *current* kernel
task stack, not a dedicated stack. When a benchmark has driven the task stack
near `stack_bottom`, the CPU pushing the interrupt frame (and the handler's own
frames) crosses into the guard page → #DF. Only the double fault itself uses an
IST (IST1). This makes *any* near-full kernel stack a double-fault risk on the
next interrupt — a real, production-relevant bug for any in-kernel code that
uses a lot of stack, not merely a benchmark artifact.

**FIXED part — the 16 KiB gzip hash table (`kernel/src/fs/compress.rs`).**
`lz77_tokenize()` allocated `let mut head = [0u32; HASH_SIZE]` with
`HASH_SIZE = 4096` = **16 KiB on the stack** (a quarter of the whole 64 KiB
stack), while its sibling `prev` was already heap-allocated. Moved `head` to a
`Vec` (heap) and changed `insert_hash`/`find_best_match` to take `&[u32]`/`&mut
[u32]` slices (call sites unchanged — `&mut Vec<u32>` coerces). Verified: with
this fix the `http_gzip_1KiB` and `http_gzip_8KiB` benchmarks now **complete**
(8192B → 4507B), where before they double-faulted. This was the dominant
single stack frame and removing it is correct regardless (gzip should never use
16 KiB of stack).

**OPEN part — RESOLVED 2026-06-15 by the Q7 option-A per-CPU IRQ stack;
empirically confirmed 2026-06-20.** The systemic interrupt-on-near-full-stack
overflow was fixed by moving interrupt handling off the interrupted task's stack
onto a dedicated per-CPU guard-page IRQ stack (`idt.rs::init_irq_stack` /
`run_on_irq_stack` / `IRQ_STACK_TOP`/`IRQ_STACK_BOTTOM`, with nesting-aware
manual RSP switch + `sched::do_deferred_preempt` after RSP is back on the task
stack — see open-questions.md Q7 / design-decisions.md §26). Once IRQ frames no
longer land on a near-full task stack, the 64 KiB task stack is sufficient for
the debug-built `core::fmt`-heavy dashboard path. **Validated 2026-06-20:**
`scripts/boot-test.sh --bench` runs the *entire* deferred suite to completion —
`dashboard_api_status`/`_health`/`_metrics`, `isr_latency`, the 62-entry
scorecard, and a clean `BENCH_OK` — with no double fault (serial-test.txt lines
9843–9913). The stale "still double-faults entering dashboard_api_status"
description below is retained for history only and no longer reproduces.

_Historical (pre-fix) description:_ After the
gzip fix the suite advances one stage further and double-faults again at the
**identical** guard-page `RSP=0xffffc1000003ffb8`, now in `Task 114` during
`bench_dashboard_api_status` (`crate::net::dashboard::bench_api_status`). The
dashboard path has no single large array — it is `format!`-heavy, and debug
builds give `core::fmt` very deep, un-inlined, stack-hungry call chains. So this
is the *general* problem: 64 KiB is marginal for debug-built in-kernel heavy
code + an IRQ frame on top. Fixing it benchmark-by-benchmark is whack-a-mole.

**Proper fix is an architectural decision — see `open-questions.md`.** The
textbook fix is a dedicated per-CPU IRQ stack (x86 IST), like Linux's IRQ
stacks, so interrupt handlers never consume the interrupted task's stack.
**Complication:** the timer handler deliberately re-enables interrupts
(`apic.rs:1162`, `sti` after EOI, for preemption), so IRQs *can* nest — a naive
single shared IRQ IST would be clobbered by a nested IRQ resetting RSP to the
IST top. A correct IRQ-stack implementation must therefore support nesting (or
the hard-IRQ phase must not re-enable IF). This is a careful change to the
hottest, most safety-critical path; alternatives (bump kernel-task stack size;
keep heavy code out of the kernel; release-build) each have tradeoffs. Deferred
to the operator as an open question rather than changing the IRQ path
autonomously.

**Reproduce:** `bash scripts/boot-test.sh --bench --timeout=600`; the suite now
runs through `compress`, `context_switch`, `pick_next`, `ipc`, `vfs`, all
`http_*` incl. both `http_gzip_*`, then #DFs entering `dashboard_api_status`.

**Large-stack-array audit (2026-06-14).** I scanned the kernel for fixed-size
stack arrays ≥ 8 KiB that could contribute to the same overflow class. Findings:
`bench.rs::bench_vfs_throughput_16k` held a `[u8; 16384]` (16 KiB) in the bench
task — moved to a heap `Vec` (committed). Remaining latent (lower-risk, not the
immediate trigger, left as tech-debt): `audio_notify.rs::self_test` `[u8; 8192]`
(boot self-test path), `syscall/linux.rs` ~line 53451 `drain [u8; 8192]`, plus
several `[u8; 4096]` buffers in `rng`/`smp`/`virtio/sound`/`linux.rs` self-tests.
Note these arrays are **not** the immediate dashboard double fault: the
`dashboard_api_status` overflow has **no** large array — it is pure debug-built
`core::fmt` call-chain depth — so reducing stack arrays will not by itself make
`BENCH_OK` appear; only the Q7 IRQ-stack / stack-size decision will.

**Impact (historical):** Before the Q7 IRQ-stack fix, `BENCH_OK` and the last
benchmarks (dashboard API, ISR latency, scorecard) did not complete. As of the
fix (and re-confirmed 2026-06-20) the full deferred suite completes and
`BENCH_OK` prints. Normal operation was never affected: the default `BOOT_OK`
boot test always passed (the deferred bench suite runs only after BOOT_OK).

### W1. Intermittent boot-test hang recurred once at the OOM self-test — WATCHLIST 2026-06-10

**Where:** boot self-test sequence; serial output (`build/serial-test.txt`)
truncated mid-line at `[sysctl] mm.oom_pol…` during `mm::oom::self_test()`
Test 3 (the `register_kill_callback` / `handle_oom(10)` step).

**Symptom:** One boot-test run did not reach `BOOT_OK` within 300s; serial
stopped mid-line inside the OOM self-test.  The very next run (identical
binary) reached `BOOT_OK` in 26s with the full OOM test passing.

**Assessment:** Same class as F1/F6/F7 — an intermittent hang that
truncates serial mid-line at whatever self-test happens to be running,
historically traced to spin::Mutex / interrupt-window / RCU timing rather
than the self-test's own logic.  `mm::oom::self_test()` and `handle_oom()`
are fully synchronous (no spawning, no blocking, fake kill callback), so
the OOM code is almost certainly the *victim*, not the *cause*.  This is
the first recorded recurrence since the F6/F7 "likely cured incidentally"
closure (128/128 prior clean boots), so it is logged here rather than
re-opening F6/F7.

**Next step if it recurs:** soak `scripts/boot-test.sh` ~20× to get a
recurrence rate and bisect the hang window the way F1/F4 were diagnosed
(finer-grained pre/post serial markers around the suspected lock).

**Soak 2026-06-12:** ran the diagnostic soak in two batches —
12× then a further 10× back-to-back `boot-test.sh` runs
(`build/oom-soak-*.log`, `build/oom-soak2-*.log`) targeting this hang
window. **22/22 clean, every run BOOT_OK at 25s, zero recurrence, no
truncated serial, no failure serials to bisect.** This **meets the
~20× diagnostic bar** the entry set, with an observed recurrence rate
of **0/22**. Consistent with the "OOM self-test is the victim, not the
cause" assessment: the single recorded truncation has not reproduced.

**Recurrence 2026-06-12 (second recorded):** while boot-testing the F10
boot-stack fix (`build/boottest-536-fixed.log`, run `brqckyayz`), one run
again truncated mid-line at exactly `[sysctl] mm.oom_pol…` during
`mm::oom::self_test()` and never reached `BOOT_OK` within 300s. The
immediate identical-binary re-run (`bx59ud6x2`) reached `BOOT_OK` in 26s
with the full OOM test passing (`[oom]   Callback registration and
invocation: OK`) and the shell prompt. This is the **same fingerprint** as
the original truncation (same self-test, same mid-line cut point), and it
is **not** caused by the F10 boot-stack change — the fix only enlarges the
boot stack / adds a redzone canary and is unrelated to the OOM self-test
path, and the canary did not trip. This recurrence **resets the clean
streak** that the 22/22 soak had been accumulating toward the ~90 closure
bar.

**Soak 2026-06-14:** 7 consecutive clean runs back-to-back (1× full
build+boot + 6× `--no-build`, `build/w1-soak-*.log`), every run BOOT_OK in
26–32s with the OOM self-test passing (`[oom]` step clean, no mid-line
`[sysctl] mm.oom_pol…` truncation). 0/7 recurrence. Clean streak now **7**
toward the ~90 closure bar.

**Status:** passive monitoring, clean streak **7** (after the 2026-06-14
soak; was reset to 0 by the 2026-06-12 recurrence). **Closure condition
unchanged:** close this item (move to Fixed/Closed as "likely cured
incidentally," like F6/F7) once a fresh combined dedicated-soak +
routine-boot clean streak passes ~90 with no recurrence. Re-open and bisect immediately on the next mid-self-test
truncation; given two recorded recurrences now, a finer-grained marker
pass around the `mm::oom::self_test()` / `sysctl::set` lock window
(per the F1/F4 method) is the priority diagnostic when next observed.

### W2. Deferred benchmark suite livelocks in `bench_pick_next` after `context_switch` → `BENCH_OK` never prints — ROOT-CAUSED & FIXED 2026-06-14

**RESOLUTION 2026-06-14 — root cause was the mouse cursor task busy-yielding,
NOT a benchmark or backend bug.** The livelock was never about the nop helpers
or `bench_pick_next` per se; it was a **system-wide priority-starvation bug**
that the long bench suite merely exposed first. `cursor_task_entry`
(`kernel/src/mouse.rs`, spawned at priority **16**) polled a lock-free mouse
event ring and, when the buffer was empty, called `crate::sched::yield_now()`
in a tight loop "to avoid spinning." But `yield_now()` re-enqueues the current
task at *its own* priority and then picks the highest-priority Ready task — and
the cursor task, at p16, was *still the highest-priority Ready task*, so it was
immediately re-picked. The "yield" loop therefore **never relinquished the CPU
to any task of priority > 16** (it only ever ceded to something strictly
higher-priority, of which there usually was none). This pinned a core, so every
p≥17 task — the p18 `deferred_bench_task` driver, the p18 workqueue worker,
background daemons — could make progress *only* via the ~1 s anti-starvation
booster (one or two tasks nudged to priority 0 each pass, hence the perpetual
`[sched] Anti-starvation: boosted N tasks` spam). `bench_pick_next` "stalled"
because its driver only got a sliver of CPU per second.

**Diagnosis chain:** markers proved `run()` never returned even though the nop
helpers *did* exit → so the lone driver itself was starving, not the helpers →
boost-ID logging (`cur=<current task> boosted <ids>`) showed the boosted/starved
tasks were tids 115 (bench driver) + 103 (workqueue worker), and that the task
hogging the CPU (`cur=`) was the **mouse cursor task** → reading
`cursor_task_entry` revealed the idle `yield_now()` busy-loop.

**Fix:** in the idle branch the cursor task now `sleep_ms(8)` (~125 Hz) instead
of `yield_now()`. `sleep_ms` (≤100 ms ⇒ hrtimer path) *removes* the task from
the run queue entirely until an hrtimer wakes it, so lower-priority work runs
freely while the cursor is idle; active mouse movement still drains events
tightly (the sleep only triggers once the ring empties). Verified: with this
fix the `--bench` suite runs from `page_alloc` all the way through `compress`,
`context_switch`, `pick_next`, `syscall_dispatch`, `ipc`, `vfs`, and into the
`http_gzip` benchmarks — vastly further than ever before (previously it never
passed `context_switch`). The default `BOOT_OK` boot test still passes
(BOOT_OK after 29 s), confirming no regression to normal operation. (Fixing W2
unmasked a separate latent double fault in a late bench stage — see B-DF1
below.)

**General lesson:** `yield_now()` is NOT a valid "idle until work arrives"
primitive for any task that is not the lowest priority on its core. A task that
yields at its own priority and is the highest-priority Ready task will be
re-picked immediately and spin. Idle waiting must *block* (sleep, or wait on a
waitqueue/futex), removing the task from the run queue. Audit other drivers for
the same `yield_now()`-when-idle antipattern.

---

**Original investigation notes (retained for history):**

**Where:** `kernel/src/bench.rs` `bench_pick_next()` (the
`run("sched_pick_next_4tasks", 500, || sched::yield_now())` loop, run after
the four `bench_nop_task` helpers at priorities 8/12/16/20 are spawned);
interacts with the scheduler's yield/pick path and anti-starvation boost in
`kernel/src/sched/mod.rs`.  Driven from the background `deferred_bench_task`
spawned at the end of `kernel/src/main.rs` boot.

**Symptom:** With `scripts/boot-test.sh --bench --timeout=600`, the deferred
benchmark suite runs cleanly through `page_alloc`, `heap`, `compress`,
`rdtsc`, `hpet`, and `context_switch_rt`, then **stalls** at/after
`bench_pick_next`: no `[bench] sched_pick_next_4tasks: …` line is ever
printed, `BENCH_OK` never arrives, and the serial log fills with continuous
`[sched] Anti-starvation: boosted N tasks to priority 0` (N = 1–2).  The
default `BOOT_OK` boot test is unaffected (it stops at `BOOT_OK`, long before
the benchmarks run).

**CORRECTION 2026-06-14 — the four nop helpers DO exit (original
"never exit" claim falsified).** A 600 s-timeout run captured all four
`bench-pn` nop helper tasks (tids **119, 120, 121, 122**) printing
`[sched] Task N exiting` *after* `context_switch_rt`'s result line — i.e. they
spawn AND drain to `task_exit` successfully.  So the nop helpers are **not**
the livelocking tasks, and `bench_pick_next`'s task-draining works.  The hang
is therefore **after** the helpers exit: either `run("sched_pick_next_4tasks",
500, yield_now)` not returning on the lone driver task (tid 114) once the
helpers are gone, a *later* benchmark stage that the driver enters silently, or
genuine starvation of 1–2 **other** Ready tasks (background daemons / the
workqueue worker tid 104 at prio 18) behind the busy prio-18 driver — those
are what the perpetual "boosted 1–2 tasks" lines refer to, NOT the nop
helpers.  Next diagnosis must localize where tid 114 actually gets stuck after
the helpers drain (add a marker after `run()` returns in `bench_pick_next` and
at the start of `bench_syscall_dispatch`), rather than assuming the nop helpers
are the culprit.

**Assessment:** Independent of the F15 sleep-queue leak — it reproduced
identically *before* the F15 fix (when it could have been blamed on
kswapd/workqueue spin-starvation) and *after* it (0 `sleep queue full`
warnings).  `run()` is a plain non-blocking loop and the task-exit path
(`task_finished` → `task_exit` → `schedule_inner(false, Uncounted)`) is clean,
so the hang is a scheduler-level livelock among several equal-/mixed-priority
tasks that only `yield_now()` (no sleeping, no I/O).  The persistent
anti-starvation boosting suggests the scheduler is thrashing — repeatedly
boosting starved tasks to priority 0 without the nop helpers ever being
scheduled through to completion.  Not yet root-caused.

**Impact:** The deferred micro-benchmark suite cannot complete past
`context_switch`, so `BENCH_OK` and the later benchmarks (pick_next, syscall
dispatch, IPC, VFS, net, crypto, HTTP, ISR latency, scorecard) never run in
normal operation.  Early-benchmark perf tracking still works:
`boot-test.sh --bench` prints the captured numbers up to the hang even on
timeout.

**Update 2026-06-14 (anti-starvation duplicate-enqueue fix — ruled OUT as the
root cause):** While investigating, I found and fixed a genuine
scheduler-correctness bug in the anti-starvation booster
(`check_starvation()` in `kernel/src/sched/mod.rs`): it boosted a starved
Ready task by `PER_CPU_SCHED.dequeue(id, effective_priority(), cpu)` followed
by `enqueue(id, 0, cpu)`.  Because `effective_priority()` returns the task's
*base* priority while an already-boosted task physically sits in priority
queue 0, the level-targeted dequeue scanned the wrong queue, removed nothing,
and the enqueue created a **duplicate** run-queue entry — the same task id
present twice in queue 0.  Re-boosting on every ~1 s pass (the booster never
reset `ready_since_tick`) multiplied the duplicates without bound.  Fix:
(a) added `dequeue_any(id)` to `PriorityRoundRobin`/EEVDF/Deadline +
`SchedulerBackend`/`PerCpuScheduler`, which removes *all* copies of a task at
*any* level and clears the bitmap bit when a level empties; the booster now
`dequeue_any` then single-`enqueue` at 0, leaving exactly one entry; and
(b) the booster now resets each boosted task's `ready_since_tick` so it is not
re-boosted before being dispatched.  This is a real, system-wide fix (the
corruption could happen to any starved task, not just benchmark tasks).
**However, it did NOT resolve W2:** with the fix in place the suite still
stalls entering `bench_pick_next` (no `sched_pick_next_4tasks` line, `BENCH_OK`
never arrives), boot remains clean (0 self-test failures, 0 sleep-queue spin
warnings), and the booster still fires (now without duplicating entries).  So
the duplicate enqueue was an *amplifier* of the thrash, not the trigger: the
benchmark nop helpers still genuinely fail to run to `task_exit`.

**Timeout calibration (corrected — the original stall-point stands).** A first
post-fix run with the default 300 s timeout appeared to stall right after
`heap_raw_alloc_free_4096`, suggesting the hang had moved earlier.  That was a
**timeout artifact, not a regression**: a 600 s re-run showed the suite *does*
still progress cleanly through `compress`, `rdtsc`, `hpet`, and
`context_switch_rt`, then stalls entering `bench_pick_next` — exactly the
original symptom.  The 300 s budget simply expired *inside* the
`compress_repeating` benchmark, which is savagely slow under QEMU/TCG:
mean ≈ 1.01 s per iteration × 200 iters ≈ **~202 s for that one benchmark
alone** (max single iter ≈ 22 s).  Because `bench_pick_next`'s own work is
trivial (~110 ms for all 500 yields at the measured ~220 µs/round-trip), its
failure to complete within the remaining multi-hundred-second budget confirms a
**genuine stall**, not mere slowness.  Practical note: reproduce W2 with
`scripts/boot-test.sh --bench --timeout=600` (the default 300 s no longer
reaches the stall point because the compress benchmarks eat the budget first).

The deeper root trigger (why four `yield_now()`-only tasks at priorities
8/12/16/20 never drain past `bench_pick_next`) is still uncharacterised.

**Next step:** Add finer-grained serial markers inside `bench_pick_next`
(before/after spawn, before/after the `run()` loop, per-iteration sampling)
and instrument the scheduler's pick/yield path to capture *which* task is
selected each switch during the stall.  Determine whether the nop helpers are
never picked, or are picked but never run to their `task_exit`.  Likely a
priority/round-robin or anti-starvation interaction; treat as a real
scheduler-correctness bug, not merely a benchmark quirk.  Risky to change the
scheduler blindly, so diagnose before patching.

_(The two prior watchlist items — accounting
self-test hang and invariant self-test hang — went 90 consecutive
boot tests with zero recurrence after F4/F5 and have been closed as
"likely cured incidentally," and as of 2026-06-10 a further 38 clean
boots (128/128 total) keep them closed.  See F6 and F7 in Fixed Bugs.
The two items discovered 2026-06-10 — quota Test 5 and FS interceptor
deny — are now fixed; see F8 and F9.)_

---

## Fixed Bugs

### F19. rmap self-test used low fake frame addresses that collided with real CoW frames → flaky `assertion failed: is_private(frame2)` panic — FIXED 2026-06-30

**Where:** `kernel/src/mm/rmap.rs` (`self_test()`), invoked from
`kernel/src/main.rs:3288`.

**Symptom:** Intermittent boot panic `panicked at kernel\src\mm\rmap.rs:445:
assertion failed: is_private(frame2)` (also reproducible at the Test-1
`add(frame1,...)`/`count==1` assertion). The rmap self-test ran to completion on
most boots but panicked on others — pure timing/allocation flakiness, not a
deterministic failure. Surfaced while validating the container read-only-volume
work (increment 15); that change is functionally invisible to this MM path —
it merely perturbed frame-allocation timing enough to expose the latent test
bug. (A separate, also-flaky CoW-pipeline hang in the same boot run is the known
F18-family fragility of the `dash | … > file` ring-3 test and is unrelated.)

**Root cause:** The rmap is a **global** hash table keyed by physical frame
address, and `self_test()` runs *late* in boot — after the Path-Z ring-3
toolchain tests (dash pipelines, tcc, make) have done heavy CoW/fork activity
that registers thousands of **real** user frames in that global table. The test
used fixed low fake addresses (`frame1 = 0x10_0000` = 1 MiB, `frame2 = 0x20_0000`
= 2 MiB, untracked-frame probe `0xDEAD_0000`). When a real user frame happened to
sit at exactly one of those physical addresses, it already had a mapper in the
table, so the test's `add(frame2, pml4_a, virt2)` appended a *second* mapper and
`is_private(frame2)` returned false → assertion panic. Whether a real frame
landed on 0x20_0000 depended on allocation order, making it flaky.

**Fix:** Move the test frames far above any installed physical RAM (machines here
have at most a few GiB) so the global table can never hold a pre-existing entry
for them: `frame1 = 0x0F00_0000_0000` (~15 TiB), `frame2 = frame1 + 16 KiB`, and
the untracked-frame probe to `0x0F00_0001_0000`. These remain valid u64 hash keys
(the rmap does not validate physical-address width) and are impossible as real
frames, so the test is now collision-proof regardless of allocation timing. A
detailed comment records the invariant. (A fuller fix — refactoring the rmap API
to operate on an injectable test-local table instead of the global static — was
rejected as disproportionate: it would add a `&mut table` parameter to every
production rmap entry point purely for testability. Impossible-address selection
is the minimal correct fix.) The self-test still cleans up all its entries
(`frame1`/`frame2` removed before exit), so no fake entries leak into the live
table.

### F18. CoW refcount granularity mismatch (per-16 KiB-frame refcount vs per-4 KiB-PTE resolution) double-freed a still-shared frame → parent `dash` #GP in a pipeline — FIXED 2026-06-16

**Where:** `kernel/src/mm/cow.rs` (`resolve_cow_fault`, `clone_frame_group`)
and `kernel/src/mm/page_table.rs` (`clear_user_address_space`).

**Symptom:** A real `dash -c '/bin/emit | /bin/countbytes > /dash-pipe-out.txt'`
(Path Z Part 12) crashed the *parent* `dash` with a #GP at glibc
`wait4`'s errno store (`mov %eax,%fs:(%rdx)`, libc+0x110839) — but only
on the `wait4` *error* path (e.g. `-ECHILD`), which is why the
single-fork Part 11 never hit it. The faulting `%rdx` was garbage loaded
from a libc `.got` slot (the errno `R_X86_64_TPOFF64` negative TLS
offset), so `%fs:(%rdx)` was non-canonical. The `.got` 4 KiB page lived
at virt `0x6000203000`, sub-page 3 of the 16 KiB frame group based at
`0x6000200000`.

**Root cause:** CoW refcounting is **per-16 KiB frame** (the buddy
allocator's unit), but CoW *sharing/resolution* is tracked **per-4 KiB
PTE** (each 16 KiB frame maps as 4 consecutive PTEs). The ELF loader
packs a read-only segment tail and a writable segment head into one
16 KiB frame, so a group can hold a read-only *shared* sub-PTE (no COW
bit) next to a writable *CoW* sub-PTE — both pointing into the same
frame. Three operations used **inconsistent** rules for "the group's
reference to the frame":
- `clone_frame_group` incremented the refcount once, keyed on the *first
  present* sibling.
- `resolve_cow_fault` decremented once per resolve event whenever *any*
  CoW sibling was copied out — **even though a read-only shared sibling
  still referenced the old frame**.
- `clear_user_address_space` freed once per group, keyed on *only the
  base (sub-page 0)* PTE.

So a forked child that wrote the writable sub-PTE resolved it to a
private copy and decremented the old frame, *while still mapping the old
frame via the read-only sub-PTE*. At teardown the child's base PTE still
pointed at the old frame → it freed it **again** (double-decrement). Two
such children drove the parent-shared frame's refcount to 0; the freed
frame was reused (filled with a child's exec image), corrupting the
parent's `.got` errno slot → garbage `%rdx` → #GP.

**Fix:** Make all three operations agree on one invariant — *each address
space holds exactly one refcount on each **distinct** 16 KiB frame its
group's sub-PTEs reference*:
- `resolve_cow_fault` now drops the old frame's reference (ref_dec + rmap
  remove) **only if, after the copy loop, no sub-PTE of the group still
  points into the old frame**. A read-only shared sibling keeps the
  reference alive; the new private frame is registered in rmap
  unconditionally.
- `clone_frame_group` increments the refcount (and adds rmap) once **per
  distinct frame** found among the group's present siblings (handles a
  parent that had already partially resolved a group before forking
  again).
- `clear_user_address_space` inspects **all four** sub-PTEs of each group
  and frees each **distinct** frame exactly once (was: only the base
  PTE), so copied-out private frames are no longer leaked and refcounts
  stay symmetric with resolve/clone. (The refcount-aware `free_frame`
  already only returns a frame to the allocator at its last reference.)

**Verification:** Part 12 boot self-test
`proc::spawn::self_test_linux_real_glibc_shell_pipe` now passes (parent
`dash` exits 0, `/dash-pipe-out.txt` == `n=16\n`).

### F17. fd-bearing resources were closed at *reap* (`destroy`) instead of at *exit* (zombie) → `cmd1 | cmd2` pipeline deadlock — FIXED 2026-06-16

**Where:** `kernel/src/proc/pcb.rs` — new `exit_close_fds(pid)` + extracted
`close_initial_fds()`; `kernel/src/proc/thread.rs` — `on_thread_exit` calls
`pcb::exit_close_fds(pid)` at the zombie transition;
`kernel/src/proc/pcb.rs::destroy_process_resources` now just calls
`cleanup_handles` + `close_initial_fds` for the force-kill / never-zombied
path (the slices are already empty on the normal exit path).

**Symptom:** A real glibc `cmd1 | cmd2` pipeline (`/bin/pipe`: `pipe`→`fork`;
child `dup2`s the write end onto fd 1 and `execl`s `/bin/emit`; parent closes
the write end, `read`s the pipe to EOF, then `waitpid`s the child) **hung
forever** — `self_test_linux_real_glibc_pipe` reported "process did not exit
within N yields (state=Running)" regardless of the yield budget (a 4×
budget bump changed nothing — the tell that it was a deadlock, not
under-budgeting).

**Root cause:** A blocked pipe reader only gets EOF (`read()`→0) when the
*last* write end closes. The child's exec'd image inherited a copy of the
pipe write end; that fd's kernel resource was only released by
`destroy_process_resources`, which ran when the **parent reaped** the child
via `wait4`. But the parent could not reach `waitpid()` until its `read()`
returned EOF. EOF ⟸ child's write end closed ⟸ child reaped ⟸ parent past
`read()` ⟸ EOF. Circular wait → deadlock.

**Fix:** Close every fd-bearing kernel resource (all `ipc_handles` + any
unclaimed initial fds) the moment a process **exits** (becomes a zombie),
not when its parent reaps it — matching Linux's `exit_files()` in `do_exit`.
`exit_close_fds` `core::mem::take`s the two lists out of the PCB under the
table lock, drops the lock, then dispatches `cleanup_handles` +
`close_initial_fds`. Idempotent: the reap-time teardown finds the lists
already drained, so no double-close and no leak; the force-kill path (where
a process is destroyed without ever zombying) still closes everything.

**Validation:** `self_test_linux_real_glibc_pipe` now passes — the parent
wakes from `read()` the instant the child zombies, prints
`SLATE_GLIBC_PIPE_OK n=16 body=SLATE_PIPE_BODY\n` (46 bytes captured ==
expected) and `exit(29)`; boot test PASSED. This is a general correctness
fix: it affects every pipe/socket EOF-on-last-writer-exit, not just the
test. It is also the standing semantics any real shell relies on.

### F16. `on_thread_exit_hook` dereferenced user pointers unconditionally → kernel page-fault panic when thread cleanup ran cross-address-space — FIXED 2026-06-16

**Where:** `kernel/src/proc/thread_clone.rs` — `on_thread_exit_hook(task_id)`.

**Symptom:** `PANIC` — page fault in `read_user` reached via
`fetch_robust_entry ← exit_robust_list ← on_thread_exit_hook`, with CR2 in a
glibc-mmap user range, when a boot self-test reaped a real glibc process
(e.g. the Part 7 pipe test) by calling `thread::on_thread_exit(task_id)` from
**task 0's (boot) address space** rather than the dying process's.

**Root cause:** The exit hook walked PI-owned futexes, the glibc robust
list, and zeroed `clear_child_tid` — all of which dereference *user* virtual
addresses valid only in the dying process's address space. When the hook
runs from a different active CR3 (cross-AS reap), those addresses point into
the wrong (or unmapped) address space → faulting kernel read → panic.

**Fix:** AS-active guard. The hook computes
`as_active = page_table::active_pml4_phys() == pcb::get_pml4(owner_process(task_id))`
and runs the user-memory operations (PI-futex walk, robust-list walk,
`clear_child_tid` zero-write + `futex_wake`) **only when `as_active`**. The
in-kernel bookkeeping removals (`ROBUST_LIST` / `RSEQ` / `CLEAR_CHILD_TID`
map entries) always run regardless. When not AS-active the hook skips the
user dereferences and returns after the in-kernel cleanup — correct, because
the futex-wake/ctid-clear only matter to a live address space, and a process
being reaped from outside its own AS has no threads left to wake.

**Validation:** the Part 7 pipe boot test no longer panics in the robust-list
walk; boot test PASSED.

### F15. Sleep-queue slot leak: an expired entry was only freed when `try_wake` returned `true`, so tasks woken early / destroyed before their deadline leaked a slot permanently — daemons then busy-spun and starved low-priority work — FIXED 2026-06-14

**Where:** `kernel/src/sched/mod.rs` — `process_sleep_wakeups()` and the new
`wake_expired_sleeper()` helper; the fixed-size `SLEEP_QUEUE` (`MAX_SLEEPERS`
= 256) and the `sleep_until_tick()` busy-spin fallback.

**Symptom:** Surfaced while adding a `--bench` mode to `scripts/boot-test.sh`
(which waits for the deferred `BENCH_OK` instead of stopping at `BOOT_OK`).
During the post-boot benchmark phase the serial log filled with **688**
`[sched] WARNING: sleep queue full, task <N> falling back to spin` lines —
tasks 103 (kswapd) and 104 (the workqueue worker), both long-lived daemons
that sleep between work, could no longer register a sleep, so they fell back
to the `yield_now()` busy-spin loop in `sleep_until_tick()`. That pinned a CPU
and starved the low-priority deferred-benchmark task. The default boot test
never saw this because it kills QEMU at `BOOT_OK`, before the daemons have
looped enough to exhaust the queue.

**Root cause:** `process_sleep_wakeups()` (timer-ISR tick handler) cleared an
expired slot only when `try_wake(task_id)` returned `true`. But `try_wake`
returns `false` in two fundamentally different situations:
1. **Lock contended** (`SCHED.try_lock()` failed) — transient; retrying next
   tick is correct.
2. **Task not `Blocked` / no longer in the table** — terminal. A task that
   slept and was then woken early through another path (channel/futex/eventfd
   wake), or that was destroyed before its deadline, is no longer `Blocked`,
   so `try_wake` can *never* succeed for that slot again.
The code conflated the two and kept the slot in both cases. In the terminal
case the slot was retained forever — a permanent leak. As short-lived
boot/self-test/benchmark tasks slept-then-exited, slots leaked one by one
until all 256 were gone, after which every subsequent sleeper busy-spun.

**Fix:** Split the two failure modes with a dedicated `wake_expired_sleeper()`
that returns `SleeperWake::{Release, Retry}`. It acquires the scheduler lock
itself: on `try_lock` failure it returns `Retry` (keep the slot — genuine
contention); otherwise it inspects the task and returns `Release` in **all**
non-contention cases — task still `Blocked` (wake it, as before), task present
but already awake (record `pending_wake`, release), or task gone (release).
`process_sleep_wakeups()` now clears the slot whenever it gets `Release`, so an
expired slot is reclaimed at its deadline at the latest, bounding occupancy to
"tasks with un-expired deadlines" instead of leaking permanently. Verified by
re-running `scripts/boot-test.sh --bench --no-build`: the
`sleep queue full` warning count dropped from **688 to 0**, with the benchmark
numbers up to `context_switch_rt` captured cleanly.

**Residual (separate, pre-existing):** `BENCH_OK` is still not reached — the
deferred benchmark suite livelocks later, in `bench_pick_next` (logged
separately under Active Bugs as the "deferred benchmark suite hangs after
`context_switch`" item). That hang reproduced identically *before* this fix
(when it was masked by the spin-starvation) and *after* it (0 spin warnings),
confirming it is independent of the slot leak.

### F14. `arch_prctl(ARCH_SET_GS)` wrote `KERNEL_GS_BASE` (Linux convention) but Slate's entry stub uses the inverted GS convention → first syscall after SET_GS faulted on per-CPU access — FIXED 2026-06-14

**Where:** `kernel/src/syscall/linux.rs` `sys_arch_prctl` (ARCH_SET_GS /
ARCH_GET_GS arms); the userspace `%gs`-base context-switch restore in
`kernel/src/sched/mod.rs` (both switch sites); the `execve` `%gs` reset in
`kernel/src/proc/spawn.rs`.

**Symptom:** Latent until exercised. The new two-process `%gs`-base
context-switch regression test (`self_test_linux_gs_tls_switch`) reliably
triggered it: a ring-3 process that issued `arch_prctl(ARCH_SET_GS, sentinel)`
and then made *any* further syscall took an unrecoverable kernel `#PF` writing
to `sentinel + 8` — i.e. the syscall entry stub's `mov gs:[8], rsp` was
dereferencing the user's `%gs` sentinel as if it were the per-CPU base. With
no real ring-3 caller ever issuing ARCH_SET_GS before this test, the bug had
shipped undetected.

**Root cause — two self-consistent GS conventions, mixed:**
- **Linux convention:** syscall handlers run with the per-CPU pointer *active*
  in `GS_BASE` (one `SWAPGS` at entry, one at exit) and the userspace value
  parked in `KERNEL_GS_BASE`. So Linux's `ARCH_SET_GS` writes `KERNEL_GS_BASE`.
- **Slate's actual entry stub** (`kernel/src/syscall/entry.rs`) does a *second*
  `SWAPGS` back before calling the Rust handler, so a handler runs with the
  userspace `%gs` base *active* in `IA32_GS_BASE` and the per-CPU pointer
  resting in `KERNEL_GS_BASE`. Phase 4 swaps again for per-CPU stack access on
  the way out. Interrupts never `SWAPGS` at all. The invariant is therefore
  "**`KERNEL_GS_BASE` always holds the per-CPU pointer while in the kernel**,"
  and the userspace `%gs` base is simply the active `IA32_GS_BASE` — fully
  symmetric to `%fs`/`IA32_FS_BASE`.

  The pre-existing `ARCH_SET_GS` was copied from the *Linux* convention
  (writing `KERNEL_GS_BASE`), which under Slate's stub clobbers the per-CPU
  pointer mid-handler; phase 4's `mov gs:[8], …` (after its `SWAPGS` brings the
  now-corrupted `KERNEL_GS_BASE` into the active slot) then faults.

  A first attempt at the context-switch restore made the same wrong assumption
  in the other direction — it tried to fall back to a "live per-CPU base" read
  from `IA32_GS_BASE` when a task had no custom `%gs`. But inside a syscall
  handler `IA32_GS_BASE` holds the *user's* base (0 for a never-set task), so
  that read yielded 0 and the next `SWAPGS` loaded `GS_BASE = 0`, faulting per-CPU
  access on the *first* ring-3 process spawned.

**Fix:** Treat the userspace `%gs` base exactly like `%fs` — it is the active
`IA32_GS_BASE`. `ARCH_SET_GS`/`ARCH_GET_GS` now write/read `IA32_GS_BASE`
(0xC000_0101), not `KERNEL_GS_BASE`; the scheduler restores
`wrmsr(IA32_GS_BASE, task.gs_base)` on switch-in for user tasks (0 = no custom
`%gs`, the default — correct to restore directly); `execve` resets
`IA32_GS_BASE = 0`. `KERNEL_GS_BASE` is now written in exactly one place
(`syscall::entry::init`, the per-CPU pointer) and never touched again, making
the invariant trivially true. The TD4 `arch_prctl` GS validation self-test was
updated to bracket `IA32_GS_BASE` instead of `KERNEL_GS_BASE`. Verified: build
+ clippy (0 errors) + boot-test green; both the `%fs` and `%gs` two-process
context-switch regression tests print OK and there are no panics.

**Lesson:** When two layers each encode a CPU-state convention (the asm entry
stub vs. the syscall handler), they must agree explicitly. The FS/GS-base
handling is the canonical example; both are now documented as "active-register,
symmetric to %fs" on `cpu::IA32_GS_BASE`, `Task::gs_base`, and the
`sys_arch_prctl` const doc.

### F13. Userspace `%fs` (TLS) base and `%gs` base were not saved/restored per task across context switches — FIXED 2026-06-14

**Where:** `kernel/src/sched/mod.rs` context-switch path (both switch sites);
`kernel/src/sched/task.rs` (`fs_base`/`gs_base` fields);
`kernel/src/syscall/linux.rs` `sys_arch_prctl`; `kernel/src/proc/{fork,
thread_clone,spawn}.rs`.

**Symptom:** Latent for single-process workloads; fatal for any multi-process
glibc workload (a real toolchain: gcc/ld/make/bash). `IA32_FS_BASE` is glibc's
thread-local-storage pointer (`%fs` base) and is a global CPU register *not*
part of the saved GP `Context`. With two concurrent glibc processes, a context
switch left the incoming process running on the outgoing process's TLS pointer
— silently corrupting `errno`, the stack-protector canary, and every `__thread`
variable. The `%gs` base (see F14) is the sibling register with the same flaw.

**Root cause:** The scheduler swapped CR3, FPU state, and the GP register
`Context` on a switch, but never the per-thread segment-base MSRs. `CR4.FSGSBASE`
is off, so userspace can only change these via `arch_prctl`/`CLONE_SETTLS`,
making a kernel-stored per-task field authoritative.

**Fix:** Added authoritative per-`Task` `fs_base`/`gs_base` fields, restored on
switch-in for user tasks (`pml4_phys != 0`), kept in sync at
`arch_prctl(ARCH_SET_FS/SET_GS)`, inherited across `fork`/`clone`, and reset on
`execve`. Two two-process ring-3 regression tests
(`self_test_linux_fs_tls_switch`, `self_test_linux_gs_tls_switch`) install
distinct sentinel bases in concurrent processes and assert each survives
cooperative yields; both print OK at boot. (See F14 for the `%gs`-specific
convention subtlety that the GS half of this work uncovered.)

### F12. ALSA PCM `hw_params` leaked a mixer slot under concurrent calls on a shared fd — FIXED 2026-06-13

**Where:** `kernel/src/ipc/alsa_pcm.rs` `hw_params` (the slot-reservation
re-acquire path, ~lines 376-410).

**Symptom:** None observed yet (latent). Two concurrent `SNDRV_PCM_IOCTL_HW_PARAMS`
ioctls on the *same* PCM fd — reachable when a fd is shared across threads or
inherited across `fork()` — could permanently leak one `audio_mixer` stream
slot. Mixer slots are a finite resource, so repeated occurrences would
eventually exhaust them and make `open_stream` fail with `WouldBlock` for all
clients.

**Root cause:** A TOCTOU window in the leaf-lock dance. `hw_params` read
`need_stream = pcm.mixer_stream.is_none()` under the table lock, dropped the
lock to call `audio_mixer::open_stream()` (which must not run under the table
lock), then re-acquired the lock and did `pcm.mixer_stream = Some(sid)`
**unconditionally**. Two racing calls both observed `mixer_stream == None`, both
opened a slot, and the one that re-acquired the lock second overwrote the
first's stored `StreamId` — orphaning it (it was never `close_stream`d; the
instance's eventual `close` frees only the surviving slot).

**Fix:** On re-acquire, only store the freshly-opened slot if `mixer_stream` is
still `None`; otherwise treat it as redundant, keep the existing slot, and free
the redundant one with `audio_mixer::close_stream` *after* dropping the table
lock (preserving the documented leaf-lock invariant — no mixer call under the
table lock). Added a single-threaded idempotency assertion to the self-test
(a repeat `hw_params` stays `SETUP` with unchanged params, exercising the
`need_stream == false` reuse branch).

### F11. hrtimer self-test Test 2 raced the APIC timer ISR → intermittent boot panic — FIXED 2026-06-12

**Where:** `kernel/src/hrtimer.rs` self-test Test 2 (~lines 475-496).

**Symptom:** Intermittent boot panic at `hrtimer.rs:488`
`"Timer with 0 delay didn't fire on process_expired()"`. The panic blocked
the boot gate for any batch whose validation run happened to lose the race,
even though the code under test was correct.

**Root cause:** The self-test runs with interrupts ENABLED. It scheduled a
0-delay timer and then called `process_expired()` manually, expecting to
drain it. But the periodic APIC timer ISR also calls `process_expired()`;
when the ISR fired in the window between `schedule_ns` and the manual
`process_expired()`, the ISR drained the 0-delay timer first, so the manual
call returned `n == 0` and the `assert!(n >= 1, ...)` panicked.

**Fix:** Wrap the `schedule_ns(0, ...)` + `process_expired()` pair in
`crate::cpu::without_interrupts(|| { ... })` so the manual drain is
deterministic — the ISR cannot steal the timer in between. Test-only
correctness fix; the hrtimer subsystem itself was already correct.

### F10. Boot-stack overflow from monolithic translation self-test silently corrupted `.bss` (FPU_STRATEGY) → futex-test `#UD` — FIXED 2026-06-12

**Where:** `kernel/src/main.rs` boot stack (`KERNEL_BOOT_STACK`, was 512 KiB)
vs. `kernel/src/syscall/linux.rs::self_test()` (a single ~1.4 MB monolithic
function). Crash surfaced in `kernel/src/sched/context.rs::switch_context`
reading `sched::context::FPU_STRATEGY`.

**Symptom:** Boot reached `[syscall/linux] Translation self-test PASSED`,
then the very next subsystem — `ipc::futex::self_test()` — spawned task 36
("futex-test") and the first context switch faulted:
`EXCEPTION: Invalid Opcode (#UD) at 0xffffffff81133b0e`, instruction bytes
`49 0f ae 20` (= `xsave64 [r8]`), then `FATAL: Unrecoverable kernel #UD`.
The kernel never reached `BOOT_OK`, so boot-test could not pass. Appeared
only after the batch-536 ABI change (a translator-only `sys_fallocate`
gate not even exercised by the futex test) — a classic layout-shift
heisenbug. Reproduced deterministically with batch 536 applied; passed
deterministically with it stashed.

**Root cause:** Boot-stack overflow. `switch_context` dispatches the FPU
save on the global `FPU_STRATEGY` byte (0=FXSAVE, 1=XSAVE, 2=XSAVEOPT).
Boot init selected **FXSAVE** (QEMU CPU reports no XSAVE; serial line 84:
`strategy=FXSAVE`), yet the crashing switch executed the **XSAVE64**
branch → `FPU_STRATEGY` had been corrupted 0→1. The corruptor: the
monolithic `syscall::linux::self_test()` runs directly on the boot stack
and, in the unoptimized debug build (`opt-level=0`, no stack-slot
coloring), its frame is the *sum* of every per-batch block's locals —
disassembly of the prologue showed a ~480 KiB frame (`sub r11, 0x75000` +
probe loop + `sub rsp, 0x900`). With the 512 KiB boot stack (no guard
page) minus `kernel_main`'s own frame, batch 536's extra locals tipped the
frame past the stack bottom; the prologue's page-probe / frame writes
scribbled the adjacent `.bss`, flipping the `FPU_STRATEGY` byte to 1. The
self-test still completed (it never re-reads that byte), printed PASSED,
and returned — the poison only bit later when the futex context switch
trusted the corrupted strategy and ran `xsave64` on a CPU without
`CR4.OSXSAVE` → `#UD`. The boot stack having **no guard page** is what made
the overflow silent instead of a clean fault (same silent-`.bss`/page-table
class noted in the `KERNEL_BOOT_STACK` doc comment for the original Limine
stack).

**Fix (`kernel/src/main.rs`):**
1. Enlarged `KERNEL_BOOT_STACK_SIZE` 512 KiB → **2 MiB** so the boot-time
   self-tests fit with generous headroom (~1000+ ABI batches of runway).
2. Added a **64 KiB bottom redzone canary** (`BOOT_STACK_REDZONE`,
   `BOOT_STACK_CANARY = 0xC7`): `init_boot_stack_canary()` fills it early in
   `kernel_main` (RSP near top), `check_boot_stack_canary()` (called right
   after `syscall::linux::self_test()`) volatile-scans it and FATAL-halts
   with a clear "boot stack overflow detected" message if clobbered. The
   unoptimized stack-probe prologue writes a zero to every 4 KiB page it
   descends through, so any frame that reaches the redzone is guaranteed to
   trip the canary — converting future silent overflows into clear
   diagnostics before they can corrupt the `.bss` below the stack.

**Proper long-term fix (tracked as TD4):** the real smell is the monolithic
~1.4 MB `self_test()` with an unbounded per-batch frame. It should be split
into many small `#[inline(never)]` sub-functions so no single frame is
large. Deferred because the function is one giant 4-space enclosing block
(~39 k lines, opens early / closes at line 75298) and a hand-split risks
silently mis-scoping shared locals; the 2 MiB stack + canary make the
system correct and self-diagnosing in the meantime.

**Verification:** boot-test with batch 536 applied now reaches `BOOT_OK`
in 26s (was deterministic `#UD` FATAL before `BOOT_OK`), with serial
running through to the `user>` shell prompt; the redzone canary scan runs
clean (no "boot stack overflow detected"), `[syscall/linux] Translation
self-test PASSED`, and the futex self-test that previously faulted now
completes normally. (One of the validation runs hit the pre-existing
intermittent OOM-self-test truncation tracked as W1 — unrelated to this
fix; the immediate re-run was clean.)

### F8. quota self-test Test 5: wrong inode expectation (test bug, not production) — FIXED 2026-06-10

**Where:** `kernel/src/fs/quota.rs` — `self_test()` Test 5.

**Symptom:** Boot serial printed a non-fatal ERROR "expected Allowed at
limit, got SoftWarning" from Test 5.

**Root cause:** A *test* bug, not a production-code bug. Test 2 sets the
test user's limits to `soft_inodes = 100, hard_inodes = 200`. Test 5
then set usage to 199 inodes and expected `check_create()` to return
`Allowed`, with a comment reasoning only about the hard limit ("→ 200,
equals hard, should be allowed"). It ignored that 199 inodes is already
far over the soft limit of 100, so `check_inodes()` correctly returns
`SoftWarning` (199+1 = 200 > soft 100; grace not yet enforced). The
production check path is correct and symmetric with `check_bytes()`
(both use `new_total > limit`): there is no inode-vs-byte off-by-one.

(Initially mis-logged as Active bug A1 — a supposed production off-by-one
in the inode soft-limit boundary. That was wrong; corrected on the same
day after reading the limit setup.)

**Fix:** Rewrote Test 5 to exercise all three quota bands the way Tests
2-4 do for bytes — under-soft (50 inodes → Allowed), over-soft within
grace (150 → SoftWarning), and at the hard limit (200 → Denied) — so it
validates real inode-quota semantics instead of asserting a value the
code never produces.

**Verification:** boot-test — quota self-test reaches "[quota]   inode
limit OK" with no ERROR.

### F9. FS interceptor deny handlers fail open for trailing-slash prefixes — FIXED 2026-06-10

**Where:** `kernel/src/fs/intercept.rs` — `pre_check()` interceptor
match filter.

**Symptom:** Boot serial printed non-fatal "[intercept]   ERROR: deny
handler allowed". A `Deny` interceptor registered for `/protected/` did
not block a write to `/protected/secret.txt` — it failed *open*.

**Root cause:** The match filter used
`path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')`,
but interceptors are registered with a **trailing-slash** prefix
(`/protected/`). With the slash included, `get(prefix.len())` looks at
the byte *after* the slash, so the check only matched double-slash paths
(`/protected//x`). Real children like `/protected/secret.txt` never
matched, so the deny handler was never invoked and the operation was
allowed. (Same idiom bug as F-class integrity.rs fix in commit
`22a8098f`; see TD3 for the broader audit.)

**Fix:** Extracted `path_matches_prefix(path, prefix)` which normalises
away a single trailing slash (`strip_suffix('/')`) before applying the
canonical component-boundary check, so it is correct whether or not the
registrant supplied a trailing slash, and also matches the protected
directory node itself (`/protected`). Added boundary regression
assertions to Test 3: `/protectedX/file.txt` must NOT match (no prefix-
string leak) and `/protected` (the dir itself) must match.

**Verification:** boot-test — "[intercept]   deny handler with path
prefix OK" and "[intercept] Self-test passed (10 tests)" with serial
showing DENIED on both `/protected/secret.txt` and `/protected` and no
denial of `/protectedX/...`.

### F1. RCU self-test occasionally hangs at boot (intermittent) — FIXED 2026-06-07

**Where:** `kernel/src/rcu.rs` — `call()`, `process_callbacks()`,
`stats()` and (defense-in-depth) `synchronize()`.

**Root cause:** The `CALLBACKS` spinlock was acquired both from
direct callers (boot path → `rcu::call`, `rcu::stats`,
`rcu::synchronize` → `process_callbacks`) AND from `rcu::tick()`
running in softirq context.  Softirqs dispatch with interrupts
re-enabled on the same CPU.  If a timer ISR fired while a direct
caller held the lock, the softirq's `process_callbacks()` re-entered
the same critical section on the same CPU and deadlocked the
spin::Mutex.  The hang manifested between
`[rcu]   Quiescent state: OK` and `[rcu]   Callback registration: OK`
(i.e. inside `rcu::call`) because that's the first lock acquisition
after the periodic softirq starts running.

**Diagnosed by:** Running boot-test.sh 10× — observed 2 hangs, both
with the serial log truncated at exactly the same point (after
"Quiescent state" probe, before "Callback registration").  This
showed the hang was in `call()`, not `synchronize()` as the original
hypothesis suggested.

**Fix:** Wrap every `CALLBACKS.lock()` site in
`crate::cpu::without_interrupts(...)` so the lock cannot be acquired
from a path that is interruptible.  Additionally, in `synchronize()`,
explicitly bump the calling CPU's own QS counter after snapshotting
(the caller cannot itself be in a read-side critical section by RCU
invariant), and add a million-iteration safety cap with diagnostic
print so any future grace-period failure surfaces a warning instead
of a silent hang.  Added finer-grained "[rcu]   Synchronize: pre/post"
self-test probes to localize any future regression.

**Verification:** 20/20 consecutive boot tests pass after the fix
(previously 2/10 hung).

### F2. Watchdog self-test heartbeat-increment assertion race — FIXED 2026-06-07

**Where:** `kernel/src/watchdog.rs` — `self_test()` test 1.

**Root cause:** The test does
`before = HEARTBEATS[cpu].load(); heartbeat(); after = HEARTBEATS[cpu].load();`
and asserts `after == before + 1`.  But the APIC timer ISR also calls
`watchdog::heartbeat()` on every tick (via `apic.rs`), so a timer
interrupt landing inside the before→after window can cause the
counter to advance twice, tripping the assertion.  Observed once on
2026-06-07: panic with `left: 368, right: 367`.

**Fix:** Wrap test 1's load/heartbeat/load sequence in
`crate::cpu::without_interrupts(...)`.

**Verification:** 20/20 consecutive boot tests pass after the fix.

### F3. Softirq self-test races APIC timer ISR — FIXED 2026-06-07

**Where:** `kernel/src/softirq.rs` — `self_test()` tests 2, 3, and 4.

**Root cause:** The self-test runs after `[boot] Interrupts enabled —
preemptive scheduling active`, so the APIC timer ISR fires
asynchronously throughout the test.  The ISR's path calls
`process_pending()` on the same CPU, which mutates `TOTAL_RUNS`,
`TOTAL_HANDLERS`, `IN_SOFTIRQ`, and `PENDING`.  Three races:

  * Test 2 (no-op fast path): an ISR firing between
    `process_pending()` returning and `TOTAL_RUNS.load()` bumps the
    counter and trips `runs_after != runs_before`.
  * Test 3 (dispatch + clear): an ISR firing between `raise()` and
    the test's own `process_pending()` drains TIMER_SOFTIRQ first;
    the test's call then runs no handler and trips
    `handlers_after <= handlers_before`.
  * Test 4 (re-entry guard): after the test clears
    `IN_SOFTIRQ[cpu] = false`, an ISR firing before the
    `still_pending` load runs a real `process_pending()`, consumes
    TIMER_SOFTIRQ, and trips "bits were consumed despite re-entry
    guard".  Observed once on 2026-06-07 during the post-RCU-fix
    soak (build/serial-test.txt at 11:44).

**Fix:** Wrap each of tests 2, 3, and 4 in
`crate::cpu::without_interrupts(...)`.  In test 4, also sample
`PENDING` *before* clearing `IN_SOFTIRQ` so the semantic ordering
("did the guarded call consume bits?") is preserved.  `process_pending`
internally toggles IF (STI→handlers→CLI); `without_interrupts` saves
and restores the outer IF state, so the boot path's interrupt state
post-test is unchanged.  Test 1 already had its own CLI/STI window
and didn't need changes.

**Verification:** Boot test passes cleanly with `softirq` self-test
showing all four sub-tests OK and `Self-test PASSED`.  Post-fix
30-run soak: 29/30 pass with zero softirq self-test failures (the
single failure was in `frag_history` test 6 — see F4 below).

### F7. Invariant self-test hang — LIKELY CURED INCIDENTALLY 2026-06-07

**Where:** `kernel/src/invariant.rs` — `self_test()`, between the
test 1 `check_all()` call and the test 2 `all_ok()` call.

**Original symptoms:** Single observation 2026-06-07 during the
post-RCU-fix soak (`build/soak-hang-run2.txt`).  Serial output stopped
cleanly after the 8th `[PASS]` detail line, before the test 2
`Quick check: OK` line.

**Why closed:** Did NOT recur in 90 consecutive boot tests across
three 30-run soaks after F4 (and was already not recurring before
F5).  The `invariant` checks include `frame_accounting`, which
calls `frame::stats()` — exactly the path F4 made IRQ-safe.  That
is the most plausible incidental cure: test 2's `check_all()`
re-entry triggered `frame::stats()` in a window when an APIC timer
ISR landed inside the held `ALLOCATOR` lock, and F4 closed that
window.  Cannot prove this was the sole cause from a single
observation, but the empirical bar (90/90 post-fix) is met.

**Watch:** If this ever recurs, reopen — most likely culprit would
be a different invariant closure (heap balance, scheduler balance,
IPC counters, cap audit) hitting an analogous lock-class race.

**Re-verified 2026-06-10:** 38 additional consecutive clean boots
(8-run + 30-run batches, `build/stability/batch30.log`) on the
post-procfs-restructure binary, all reaching BOOT_OK in 24–27s with
no hang at the invariant self-test.  Running total of clean boots
since the F1–F5 sweep is now 128/128.

### F6. Accounting self-test hang — LIKELY CURED INCIDENTALLY 2026-06-07 (SUPERSEDED: true root cause found 2026-07-01, see B-PREEMPT-SPINLOCK)

**2026-07-01 update:** This was NOT actually cured by the F1–F5 IRQ-safety
sweep. The real bug was involuntary preemption while holding the `ACCT`
spinlock (a single-CPU priority-inversion deadlock), now root-caused and fixed
under **B-PREEMPT-SPINLOCK** (top of this file). It "stopped recurring" only
because the trigger is timing-dependent (~5%). The IRQ-safety hypothesis below
was plausible but wrong for this specific hang.


**Where:** `kernel/src/mm/accounting.rs` — self-test path, after
"[accounting]   Destroy: OK".

**Original symptoms:** Single observation 2026-06-07 during batch 473
boot test (`build/serial-test.txt`, truncated at line 3073).  Serial
output stopped mid-accounting self-test before the expected
"Tracked count: 0 (after cleanup)" line; anti-starvation logs
floods every tick afterward, suggesting scheduler alive but the
accounting test thread blocked.

**Why closed:** Did NOT recur in 90 consecutive boot tests across
three 30-run soaks after the F1–F5 IRQ-safety sweep.  The
hypothesis at the time of observation was the same shape as F1
(same-CPU spinlock + softirq re-entry).  F1 fixed RCU, F3 fixed
softirq self-test, F4 fixed `frame::stats()`, and F5 finished the
ALLOCATOR sweep — closing every IRQ-vs-softirq lock-class race
known to be reachable from the timer ISR.  The accounting hang is
most plausibly an incidental casualty of one of those fixes (the
accounting subsystem's tracker uses a mutex that's touched in
allocation paths that F5 made IRQ-safe).

**Watch:** If this ever recurs, reopen — at that point a finer
probe between `Destroy: OK` and `Tracked count` would localize the
new hang window.

**Re-verified 2026-06-10:** 38 additional consecutive clean boots
(8-run + 30-run batches, `build/stability/batch30.log`) on the
post-procfs-restructure binary, all reaching BOOT_OK in 24–27s with
no hang at the accounting self-test.  Running total of clean boots
since the F1–F5 sweep is now 128/128.

### F5. `frame::ALLOCATOR` lock uniformly IRQ-safe — FIXED 2026-06-07

**Where:** `kernel/src/mm/frame.rs` — all 13 remaining `allocator.lock()`
acquisition sites outside `pcpu_refill`/`pcpu_drain` (which are
already called with IRQs off) and `try_stats()` (panic-only).

**Why this was technical debt (was TD1):** F4 made `stats()`
IRQ-safe but left `alloc_*`, `free_*`, `is_allocator_owned`,
`refcount`, `ref_inc`, `ref_dec`, and `validate_free_lists` taking
the lock without wrapping in `without_interrupts`.  No
currently-registered softirq path took the allocator lock (audited
2026-06-07), so there was no exploitable deadlock — but the next
softirq subsystem that touched the allocator (kswapd periodic
reclaim, RCU-deferred page free, memory-pressure tick) would have
silently re-opened the same race that F4 closed.

**Fix:** Wrap each acquisition site in
`crate::cpu::without_interrupts(...)` at the call site, matching
the F1/F3/F4/workqueue pattern.  The multi-attempt `alloc_order_inner`
and `alloc_order_constrained_inner` paths use a per-attempt
without_interrupts so IRQs are re-enabled between attempts (so
reclaim/compact/OOM can run normally and wake other tasks).  Did
NOT wrap `pcpu_refill` / `pcpu_drain` — their callers already run
with IRQs disabled and the function-level comments document this
invariant.  Used inline wraps rather than a helper because the
sites have varied shape (KernelResult returns, multi-attempt retry
loops, value vs Option returns) — a `with_allocator` helper would
have required `FnOnce(&mut BuddyAllocator) -> R` plumbing at every
site, which is more code churn than the wraps themselves.

**Verification:** Post-fix 30/30 boot tests pass.  Zero allocator-lock
hangs observed across this soak.

### F4. frag_history self-test test 6 hangs in sample() loop — FIXED 2026-06-07

**Where:** `kernel/src/mm/frag_history.rs` — `self_test()` test 6
("Ring buffer wraps correctly"), inside the
`for _ in 0..HISTORY_SIZE + 5 { sample(); }` loop.

**Root cause (hypothesis, verified by soak):** `sample()` calls
`mm::frame::stats()` on every iteration, which acquires
`frame::ALLOCATOR.lock()`.  The boot path runs with interrupts
enabled, so an APIC timer ISR could fire on the same CPU while the
lock was held.  Per a softirq-handler audit, no currently-registered
softirq path takes `ALLOCATOR.lock`, so a clean dead-lock chain
wasn't conclusively proven — but the empirical data (hang exactly
in this 37-iteration tight loop over a lock-acquiring call) plus
the cure (see Fix) make this the most likely explanation.  A
plausible alternate path: any future softirq subsystem (kswapd
periodic reclaim, RCU-deferred page free, memory-pressure tick)
that touched the allocator would have re-introduced the race.

**Diagnosed by:** Post-F3 30-run soak showed `[frag_history]
Trend: OK (Stable)` as the last serial line of one failure
(`build/soak-hang-run18.txt`).  Bisected the hang window to the
test 6 sample-loop.

**Fix:** Made `frame::stats()` itself IRQ-safe by wrapping the
`ALLOCATOR.lock()` acquisition in `crate::cpu::without_interrupts(...)`.
The companion `try_stats()` (panic-handler variant) already used
`try_lock()` for the same family of reasons; this brings the
regular `stats()` to parity.  Hardening — eliminates an entire
class of same-CPU IRQ-vs-main deadlocks on the buddy allocator
lock without measurable performance cost (CLI/STI on a stats read
that already serializes on a spinlock is negligible).

**Verification:** Post-fix 30/30 boot tests pass; zero recurrence
of the frag_history hang AND zero recurrence of Active Bugs #1
(accounting) and #2 (invariant) over those same 30 runs.

---

## Technical Debt

### D-NETSTACK-TCP-MINIMAL. Userspace `netstack` TCP client is minimal (slirp-only correctness) — DEBT 2026-07-14

**Where:** `services/netstack/src/main.rs` — `tcp_fetch` / `send_tcp` /
`recv_tcp_seg` (the `OP_TCP_FETCH` control op). Kernel exercises it via
`kernel/src/proc/spawn.rs::netstack_tcp_fetch_roundtrip`.

**What it is:** the Phase-4 one-shot TCP client implements just enough of
RFC 793 to be correct on the loss-free QEMU-slirp path: SYN/SYN-ACK/ACK
handshake, in-order data reception with cumulative ACKs, SYN + request-payload
retransmission (bounded), and a graceful FIN close. Deliberately **omitted**:

- **No out-of-order reassembly.** Out-of-order data segments are dropped and
  dup-ACKed to prompt a retransmit; a genuinely reordering path would stall.
- **No congestion / flow control.** Fixed advertised window (`TCP_WINDOW`),
  no cwnd/ssthresh, no RTT estimation — retransmit timers are fixed poll counts.
- **No outbound segmentation.** The request `payload` must fit a single segment
  (one MSS); larger requests are not split. Fine for the HTTP HEAD self-test.
- **Single fixed ephemeral port + fixed ISN** (`EPHEMERAL_PORT` / `isn`): only
  one connection at a time, and no ISN randomization (no security concern in the
  bounded self-test, but not production-grade).
- **Response capped at the control-path `MSG_CAP` (512 B).** Bodies beyond the
  cap are ACKed (to keep the peer moving) but discarded; only the first ~511
  bytes reach the caller.

**Proper fix:** these all go away with the **Phase-5 shared-memory data ring**
(io_uring-style zero-copy) and a real per-connection TCP state machine (proper
RTO, windowing, reassembly, multiple concurrent sockets, ISN randomization).
Tracked as part of the net-userspace migration; this control-path client is
intentionally the bounded-self-test stand-in until then. See
`net-userspace-migration.md` Phase 4/5 and `design-decisions.md` §64.

### BENCH-COMPOSITOR-SLOW. Compositor over its 4K frame budget (~10.6ms/frame vs 2ms) — PERF BUG 2026-07-01, IMPROVED 4.6x 2026-07-02

**UPDATE 2026-07-02 (4) — parallel background clear landed, 11.9ms → 10.6ms/frame
min (cumulative 48.6ms → 10.6ms = 4.6x).** Both `Framebuffer::clear` and
`Framebuffer::clear_except` now split the framebuffer into horizontal row-bands
and fill them concurrently via `std::thread::scope` over disjoint
`chunks_mut` slices — no `unsafe`, no shared mutable aliasing (each worker owns a
distinct slice). Worker count comes from the new `fill_worker_count`, which caps
at 8 and gracefully falls back to a single thread when the buffer is below 1M
pixels or when `std::thread::available_parallelism()` can't be reported, so it
never pessimizes small buffers or single-core targets. The per-scanline
span-merging logic (formerly inline in `clear_except`) was extracted into the
static `fill_uncovered_band(buf, y0, band_rows, width, color, covered, fb_height)`
helper, shared by the single-threaded and parallel paths, using absolute-y
overlap tests against the covered rects and band-local writes. New unit test
`test_clear_except_parallel_band_boundaries` (2048×1024 = 2M px, covered rects
straddling band boundaries; asserts the parallel result is byte-identical to the
single-threaded reference, plus covered-kept / uncovered-cleared spot checks). 64
compositor tests total, clippy clean. baselines.toml `measured_ns` updated to
10572000. NOTE: this only parallelizes the *background clear*; the per-window
opaque content draws are still single-threaded, so the remaining gap needs a
persistent thread-pool (to amortize the per-frame `thread::scope` spawn cost) + a
RenderEngine band-view refactor to parallelize the window-render tiles too.

**UPDATE 2026-07-02 (3) — desktop-clear occlusion cull landed, 15.8ms → 11.9ms/frame
min (cumulative 48.6ms → 11.9ms = 4.1x).** The full-desktop background clear no
longer memsets the pixels hidden behind opaque windows. `full_recomposite_into_back`
now computes `Compositor::opaque_cover_rects()` — the screen-space rectangles
provably overwritten with opaque content this frame (buffer-less windows whose
first command opaquely covers the client area at full opacity, and windows
carrying an opaque `is_opaque()` shared buffer at full opacity, over the covered
sub-rect) — and calls the new `Framebuffer::clear_except(color, &covered)`, which
fills only the complementary (uncovered) spans per scanline. Decorations
(title bar, border, translucent shadow) are deliberately excluded from the cover
rects since they lie outside the client rect, so the background under them is
still cleared (conservative → only ever costs a little correct overdraw, never
correctness). New unit tests: `test_clear_except_*` (4: empty/single/overlapping-
merge/offscreen-clip), `test_opaque_cover_rects_*` (3: opaque-command window
reported, translucent/minimized/rounded excluded, buffer sub-rect + Argb
excluded), and `test_full_recomposite_cull_matches_uncovered_background` (visual
equivalence). 63 compositor tests total, clippy clean. baselines.toml
`measured_ns` updated to 11929000.

**UPDATE 2026-07-02 (2) — occlusion cull landed, 21.4ms → 15.8ms/frame min
(cumulative 48.6ms → 15.8ms = 3.1x).** `render_window` now skips the
compositor's default white client-background fill when the client's first render
command is an opaque, square-cornered `FillRect` that fully covers the client
area on a fully-opaque window (`Compositor::first_command_covers_client`). That
first fill was 100% overdraw in the common "client paints its own background"
case (~29% of the 4K benchmark's opaque stores). Guarded to be correct: rejects
translucent windows (opacity < 1.0), non-opaque colors (alpha < 255), rounded
corners (corner pixels would show the bg), and partial-cover rects. New unit
test `test_first_command_covers_client` (55 tests total). baselines.toml
`measured_ns` updated to 15831000.

**UPDATE 2026-07-02 (1) — fill_rect row-wise rewrite landed, 48.6ms → 21.4ms/frame
min (2.3x).** `RenderEngine::fill_rect` no longer calls `blend_pixel`
per pixel. Two new `Framebuffer` fast paths were added next to `copy_row`:
`fill_row_solid` (opaque color → single `[u32]::fill`/memset per row, skips the
per-pixel float-alpha math and bounds check) and `blend_row` (translucent color
→ hoists the alpha computation and branch out of the inner loop, integer blend
only). `fill_rect` resolves the effective alpha once (color-alpha × opacity) and
dispatches to the solid, blend, or skip (alpha 0) path per row.

**Why it's still over 2ms (and why the remaining gap is *not* another naive-code
bug):** after culling the wasted white bg fill, the benchmark still issues ~31M
opaque u32 stores/frame — an 8.3M-pixel clear plus 16 windows painting opaque
client content — i.e. ~124 MB written per frame. At ~16ms that's ~8 GB/s
effective, near the ceiling for scalar cache-polluting stores on this host. The
per-pixel-work bug is fixed; what's left is memory bandwidth on a *full*
recomposite. Getting a full 16-window 4K
recomposite under 2ms would need SIMD non-temporal (streaming) stores +
multithreaded tiles, and/or occlusion culling to skip the fully-covered white
client-bg fill (that first fill is 100% overdraw when the client paints an opaque
full-window rect). **Crucially, steady-state rendering does NOT full-recomposite
every frame** — the compositor uses damage-rect partial updates (only changed
regions repaint), which is the actual 144Hz-vsync mechanism; this benchmark
deliberately stresses the worst-case full-recomposite path (wallpaper change,
resize, many simultaneously-moving windows). Remaining optimization directions
below are now *lower priority* — the dominant per-pixel bug is resolved.

**Where:** `gui/compositor/src/main.rs` — the software composite path
`Compositor::full_recomposite_into_back` → `render_all_windows` (~2807) →
`render_window` (~2832, shadows + decorations + per-command draw) over the
`Framebuffer` per-pixel ops (`clear`/`clear_rect`/`set_pixel`/`blend_pixel`,
~503-600).

**Measured (2026-07-01, via the new `bench_compose_frame_4k`):** a 4K
(3840×2160) full recomposite with 16 decorated windows carrying toolkit client
content takes **~48.6ms/frame (min), ~50ms mean, RELEASE build on the dev
host** — roughly **25x the 2ms target** in CLAUDE.md's perf-critical table,
i.e. ~20fps, missing even a 60Hz (16.7ms) vsync budget, nowhere near 144Hz
(6.9ms). This is the classic "correct-but-naive" hot-path code the
benchmark-everything mandate exists to catch. Recorded in
`bench/baselines.toml` `[compositor_frame_4k]` (`measured_ns = 48570000`).

**Likely culprits (profile before optimizing):** (1) ~~per-pixel scalar fills in
`fill_rect` with bounds-checks per pixel~~ — **FIXED 2026-07-02** (row-wise
`fill_row_solid`/`blend_row`); (2) ~~per-pixel float alpha in `blend_pixel` for
solid fills~~ — **FIXED for fills** (alpha resolved once per fill; `blend_pixel`
still used by the per-pixel `blit_buffer` slow path and font glyphs); (3)
full-screen clear + full redraw of every window every frame even when
`bench_full_composite` forces it — the real `compose_frame` has a partial-damage
path, but the fully-damaged case (wallpaper change, resize, many moving windows)
hits this — STILL the structural cost (bandwidth-bound overdraw); (4)
`render_window` clones `render_tree.commands` and the z-stack each frame
(`render_all_windows`/`render_window`, allocations on the hot path) — small for
the benchmark's 4-command windows, but worth eliminating for large trees.

**Remaining optimization directions (lower priority — per-pixel bug resolved):**
SIMD non-temporal/streaming stores for solid rects (avoid cache pollution on
huge fills) + multithreaded tile compositing to break the single-core bandwidth
ceiling; ~~occlusion culling so a window's default opaque client-bg fill is
skipped when the first command fully covers it~~ — **DONE 2026-07-02** (first-command
cull, plus desktop-clear cull under fully-opaque covering windows and opaque
shared buffers — DONE 2026-07-02 (2) & (3)); precompute/caches for window
decorations and shadows (they rarely change frame-to-frame); avoid per-frame
`Vec` clones in `render_window` (borrow or reuse scratch buffers); ensure the
damage-tracking fast path is actually taken for the common "one window changed"
case. Target: < 2ms/4K (for a full recomposite; likely needs SIMD+threads). NB:
this is the CPU-software fallback; the eventual GPU/DRM-KMS accelerated path is
separate.

**Status:** per-pixel-cost bug FIXED + redundant-bg-fill occlusion cull DONE +
desktop-clear occlusion cull DONE + parallel background clear DONE (cumulative
4.6x, 48.6ms → 10.6ms, 2026-07-02); the remaining gap to 2ms on a *full*
recomposite is memory-bandwidth-bound (~124 MB/frame worst case at ~12 GB/s
scalar stores) and needs a SIMD-streaming-store + multithreaded-window-tile
initiative (its own focused session: persistent thread-pool to avoid per-frame
`thread::scope` spawn cost + a RenderEngine band-view refactor). All the cheap
algorithmic overdraw wins have now been taken; the remaining work is a
bandwidth/parallelism problem, not a naive-code problem. Unblocked (no Linux
binaries / operator input needed).

### BENCH-COMPOSITOR. Compositor frame benchmark — RESOLVED 2026-07-01 (benchmark added; revealed BENCH-COMPOSITOR-SLOW)

**Resolution:** added `bench_compose_frame_4k` (an `#[ignore]`d measurement test
in `gui/compositor/src/main.rs`) plus the `Compositor::bench_full_composite`
hook (which shares `full_recomposite_into_back` with the real `compose_frame`
so they can't drift) and the `[compositor_frame_4k]` baseline in
`bench/baselines.toml`. The compositor is host-runnable (`cargo test -p
compositor --target x86_64-pc-windows-gnu --release -- --ignored --nocapture
bench_compose_frame_4k`), so a real number is measurable. Running it immediately
surfaced the ~25x-over-target result now tracked as BENCH-COMPOSITOR-SLOW above.
Original gap description retained below for context.

**Where (original gap):** `gui/compositor/src/main.rs` — the composite path is
`Compositor::compose_frame` (line ~2746) → `render_all_windows` (~2807) →
`blit_buffer` (~2949). There is frame-budget *tracking* at runtime
(`end_frame`, line ~849, returns whether the frame was within budget) but no
benchmark that measured the actual composite cost against a target.

**What:** CLAUDE.md's performance-critical-subsystems table lists "Compositor
frame — Must composite a full desktop in < 2ms at 4K to not miss 144Hz vsync"
as a hard benchmark target, and mandates "benchmark everything critical." Every
other critical subsystem (syscall dispatch, IPC, context switch, page fault,
page/heap alloc, scheduler pick_next, futex, io_uring, IOCP, ISR latency, VFS,
FS r/w) has a benchmark in `kernel/src/bench.rs` scored against a
`bench/baselines.toml` target. The compositor has none. `bench/` currently
contains only `baselines.toml` (no per-subsystem benchmark crates yet), and
`grep` finds no `criterion`/`#[bench]`/`fn bench` anywhere under `gui/`.

**Why not done in the discovering session:** identified during a benchmark-gap
audit at the tail of a long, context-heavy autonomous session. Doing it right
(build a host- or target-runnable harness that constructs a 4K in-memory
framebuffer + a representative multi-window damaged scene, drives
`compose_frame`/`render_all_windows`, and records ms/frame against the 2ms
target) is real work that deserves a fresh context rather than a rushed pass.

**Proper fix:** add a compositor composite-frame benchmark. Options:
(a) a `criterion` bench under `gui/compositor/benches/` if the compositor crate
(deps: `guitk`, `guiremote`) builds and composites on the host with an
in-memory framebuffer (verify `compose_frame`/`render_all_windows`/`blit_buffer`
don't require real DRM/KMS hardware handles — construct the `Compositor` with a
plain `Vec`-backed 3840×2160 framebuffer); or (b) if the composite path is too
coupled to the target, add an in-kernel/target self-test bench analogous to
`bench_pick_next_scaling`, driving a synthetic scene and using `rdtsc`. Scene
should scale window count / damage area to expose O(n)-in-pixels or
O(n)-in-windows behaviour. Record a `[qemu.compositor_frame_4k]` (and/or a
host baseline) in `bench/baselines.toml` with `target_ns = 2_000_000` (2 ms).
Note the compositor is userspace, so the on-hardware number (not the TCG figure)
is the meaningful one; document the measurement environment.

**Trigger:** next time the compositor's render path is touched, or as the next
benchmark-infrastructure task — it is unblocked (does not need Linux binaries or
operator input), just deferred for context reasons.

### EEVDF-PICK-ON. EEVDF backend `pick_next` is O(n) worst-case (non-default backend) — DEBT 2026-07-01

**Where:** `kernel/src/sched/eevdf.rs`, `EevdfScheduler::pick_next` (Phase 1
eligibility scan, ~lines 320-337). The same shape appears in the
work-stealing `steal` path (`self.tree.iter().rev()`).

**What:** The run queue is a `BTreeMap<(virtual_deadline, TaskId), EevdfEntry>`
ordered by *deadline*, but a task is *eligible* only when
`vruntime <= min_vruntime`. `pick_next` walks the tree from the front
(earliest deadline) until it finds the first eligible task. Because the
earliest-*deadline* tasks can be ineligible (higher vruntime — e.g. a
just-preempted task re-enqueued with its accumulated vruntime but an early
deadline), that scan can walk past many entries: **O(n) worst-case**, not the
O(log n) the docs used to claim. This violates CLAUDE.md's hard rule that the
scheduler's `pick_next` "must be O(1) or O(log n) — never O(n) over all tasks."

**Why tolerated for now:** EEVDF is a **non-default, opt-in** backend
(`SchedulerBackend::from_id(BACKEND_EEVDF)`); the default `PriorityRoundRobin`
is strictly O(1). In the common case (most waiting tasks eligible) the scan
stops almost immediately, so the O(n) only bites under an adversarial
early-deadline/high-vruntime mix. The docs (module header, `pick_next`, and
`backend.rs` variant doc) were corrected 2026-07-01 to state the real O(n)
worst-case so nobody trusts a false O(log n) guarantee. It was NOT rewritten
because a correct fix is a subtle, fairness-critical redesign of a component
the operator won't line-review, tested only by a boot self-test — the risk of
introducing a subtle starvation/unfairness regression outweighs the benefit
for a non-default backend.

**Secondary defect found while analysing this:** `update_min_vruntime`
(~line 291) computes its candidate from `self.tree.values().next()` — the
*earliest-deadline* task's vruntime, NOT the true minimum vruntime across the
queue (the tree is keyed by deadline, not vruntime). So the `min_vruntime`
reference point (hence the eligibility boundary itself) is approximate. A
proper fix must correct this too. It is monotonic (only ever advances,
line ~300), which is the one property a correct redesign can rely on.

**Proper fix (do before EEVDF is ever made default or heavily used):** Linux
solves this with an **augmented rb-tree** — each node caches its subtree's
min vruntime, letting `__pick_eevdf` (`kernel/sched/fair.c`, v6.6+) find the
earliest-deadline *eligible* task in O(log n). Rust's
`alloc::collections::BTreeMap` is not augmentable, so this needs either
(a) a custom intrusive augmented tree (unsafe, substantial), or (b) a
redesign into split **eligible** (keyed by deadline) / **ineligible** (keyed
by vruntime) structures with corrected, true-minimum `min_vruntime`
bookkeeping — promoting ineligible→eligible as `min_vruntime` advances
(correct because a waiting task's vruntime is fixed and `min_vruntime` is
monotonic, so each task promotes at most once per residency), amortised
O(log n). Option (b) stays in safe std collections but the Phase-2
"no eligible → earliest deadline overall" fallback still needs a deadline
ordering over the ineligible set, so it is not a trivial two-map swap.

**Trigger to do it properly:** any move to make EEVDF a default backend, ship
it as the recommended "interactive desktop" scheduler, or run it under
workloads with many runnable tasks per CPU. Flag to operator so they can
decide whether to prioritise the rewrite vs. keep EEVDF opt-in.

### TD32. Container rootfs jail uses the extracted `lower` dir directly (no overlay CoW) and only jails absolute paths

**Where:** `kernel/src/kshell.rs` (`oci run`, `cmd_oci`) sets the container's
`root_path` to the extracted `/tmp/oci-<name>/lower` tree;
`kernel/src/ipc/namespace.rs` (`apply_root`). The `fs::overlay` module exists and
`oci run` *creates* an overlay (lower+upper) but the overlay is ID-addressed, not
mounted into the VFS path tree, so the per-process root jail (which prepends a
host path prefix and routes through the normal VFS) cannot resolve through it.

**The debt.**
1. **No copy-on-write isolation.** Because the jail points at `lower`, writes the
   container makes land in the shared extracted image tree, not the per-container
   `upper`. Two containers from the same image would see each other's writes, and
   `overlay reset`/`commit` semantics don't apply to the running container.
2. **Relative paths are not jailed.** `apply_root` only re-anchors absolute
   paths; relative paths pass through for a per-process cwd layer to resolve. That
   cwd layer does not yet jail cwd, so a container process using relative paths
   from an unjailed cwd could currently resolve outside its root. The image
   entrypoint and its libraries use absolute paths, so this doesn't bite the
   common launch path, but it is a real containment gap.

**Why it didn't block increments 3–4 (§42):** the entrypoint binary and its
libraries are read via absolute paths under the rootfs, which `apply_root` jails
correctly (`..` clamped), so launching a statically-linked image entrypoint
works and is isolated for reads. The gaps are CoW write-isolation and
relative-path containment.

**Proper fix.** (a) VFS-mount the overlay at the container's rootfs mountpoint so
the jail routes through copy-on-write (writes → `upper`, reads → merged), i.e.
give `fs::overlay` a real VFS mount adapter and point `root_path` at the merged
mountpoint instead of `lower`. (b) Jail cwd end-to-end: make the per-process cwd
itself a jailed (absolute, within-root) path so relative resolution is contained,
then have `apply_root` (or the cwd-join layer) treat relative paths as
rooted-after-join. Track alongside the mount-namespace/`pivot_root` work deferred
in §42.

**Update 2026-06-30 (increment 5):** Part (a)'s blocker is removed. The
`fs::overlay::OverlayFs` VFS mount adapter now exists and works — but only after
fixing a foundational VFS issue: the global VFS lock was held across every
filesystem method call, so mounting an overlay (whose methods re-enter the VFS to
read their backing layers) deadlocked on boot. The VFS now uses a **per-mount
lock** (`Arc<Mutex<Box<dyn FileSystem>>>` + `resolve_mount`; design-decisions
§43), so stacked filesystems mount cleanly (overlay self-test 13 passes). **Still
open for TD32:** wiring `oci run`/`container create` to actually mount an
`OverlayFs` at the container rootfs and point `root_path` at that mountpoint
instead of `lower` (increment 6), plus part (b) cwd jailing.

**Update 2026-06-30 (increment 6): part (a) DONE.** `oci run` now VFS-mounts the
per-container `OverlayFs` adapter at `/containers/<name>/rootfs` and jails the
container at that merged mountpoint (not the read-only `lower`), so container
writes are copy-on-write isolated — reads see the merged view, writes land in the
per-container `upper` layer. The overlay creation (`fs::overlay::create`) now
flows its `OverlayId` into the mount step; if the overlay can't be created or
mounted, the launch gracefully falls back to jailing at the read-only `lower`.
The mountpoint is recorded on the `Container` (`rootfs_mount` field +
`set_rootfs_mount` setter, Created-only) and `container::delete` unmounts it on
teardown (outside the table lock; the VFS has its own per-mount locking). Both the
entrypoint-ELF read and the jail now route through `jail_root`.
**Still open for TD32:** part (b) — cwd jailing (relative-path containment). The
absolute-path read isolation and now CoW write isolation are both in place; the
remaining gap is jailing a container process's *cwd* so relative resolution is
contained, alongside the mount-namespace/`pivot_root` work deferred in §42.

**Update 2026-06-30 (increment 7): double-jail bug in fd-backed I/O — FIXED.**
While preparing part (b) we discovered that *all* fd-backed file I/O was broken
for jailed (container) processes — a regression that increment 6's CoW mount
would have exposed the moment a container actually opened a file. Root cause:
`namespace::apply_root` is intentionally **non-idempotent** (it blindly prefixes
the jail root, assuming a *guest* path), but `handle::open()` stored the
*already-resolved host path* in the file handle (`file.path`), and every
subsequent handle op (`Vfs::read_at(&file.path)`, `write_at`, `truncate`,
`metadata`, `readdir_at`, `file_identity`, `flock`/`funlock`, …) called
`resolve_follow` *again* → re-applied the jail prefix → double-jailed to a path
that doesn't exist. For a jailed process even `open()` failed (its internal
`stat`/`truncate`/`write_file` re-jailed). Non-jailed processes were unaffected
only because `resolve_follow` is idempotent on already-resolved non-jailed paths.
**Fix (design-decisions §44):** every path-based `Vfs` method is split into a
thin wrapper (`resolve_follow` → call worker) plus a `*_resolved` worker that
operates on an already-resolved host path *without* re-translating. Handle-backed
ops call the `*_resolved` worker directly (an open fd holds a resolved reference —
Unix semantics, immune to later chroot/rename/symlink changes). Split methods:
`read_at`, `read_file`, `stat`, `write_file`, `write_at`, `truncate`, `metadata`,
`read_at_uncached`, `readdir_at`, `file_identity`, `flock`, `funlock`,
`lock_query`. A non-idempotency guard was added to
`namespace::test_process_root` (re-resolving an already-jailed path must
double-jail) to pin the invariant so a future refactor that makes handle ops
re-resolve is caught at boot. Build clean, clippy delta zero, boot-test green.

**Update 2026-06-30 (increment 8): part (b) cwd / relative-path containment —
DONE.** TD32 part (b) is closed. Relative paths are canonicalized against the
per-process cwd in the syscall layer *before* the VFS jails them, so containment
hinges entirely on cwd (and dirfd base paths) being stored as **guest** paths.
`chdir` already stored a guest cwd, but three sites stored/used the *resolved
host* path and so leaked the jail location (`getcwd`) and double-jailed relative
resolution: (1) `fchdir` stored `handle_path` (host) as cwd; (2) `sys_openat`
with a real dirfd built `host_dir + rel` then re-jailed it (and its directory
type-check `stat(&host)` re-jailed → ENOENT for every relative `*at` from a
jailed process); (3) `resolve_at_path` (the shared `*at` resolver:
fstatat/unlinkat/fchownat/…) had the identical defect. **Fix
(design-decisions §45):** added `namespace::unjail_path_for(pid, host) → guest`
(exact inverse of `apply_root`: strips the jail-root prefix; no-op when
unjailed). `fchdir` now stores the un-jailed guest cwd. A new shared helper
`dirfd_to_guest_dir(dirfd)` resolves a real dirfd to its *guest* directory path,
doing the directory-type check with `stat_resolved` (no re-jail); both
`sys_openat` and `resolve_at_path` use it, so the combined path is jailed
exactly once. Round-trip regression assertions
(`unjail(resolve(guest)) == normalized guest`, unjailed no-op, out-of-jail
defensive passthrough) added to `namespace::test_process_root`. **Limitation:**
`unjail_path_for` reverses only the chroot layer, not namespace Bind/Hide
remapping — the container runtime never combines Bind rules with a chroot jail,
so the reversal is exact for the container case (documented on the function and
in §45). With parts (a) [CoW, inc 6] and (b) [this] done, TD32's remaining scope
is the broader mount-namespace/`pivot_root` work deferred in §42 (a separate,
larger feature, not a containment gap).

**Update 2026-06-30 (increment 9): volume (bind) mounts — DONE.** The first
concrete slice of TD32's remaining mount-namespace scope landed. A per-process
volume table (`PROCESS_MOUNTS` in `namespace.rs`) layers Docker `-v`-style bind
mounts *over* the chroot: a guest path under a volume prefix resolves to an
arbitrary host target (escaping the rootfs), while everything else still jails
under the rootfs. Volume matching runs *after* `..`-normalization, so a guest
cannot climb out of a volume into the host (security-critical ordering).
`unjail_path_for` reverses volumes too (longest host-target match), so `fchdir`
into a volume reports the guest path and stays single-jailed. Container plumbing:
`Container.volumes` + `add_volume_mount()` (Created-only, `-v` order), installed
on the init process in `add_process_task`, cleared in `remove_process_task`/
`delete`/`detach`. Covered by `namespace::test_volume_mounts` and container
self-test 19; build/clippy clean, boot-test green. Design rationale in §46.
Still deferred (TD32 remainder): a true longest-prefix mount-tree that subsumes
the rootfs as the `/` mount (the `pivot_root` target), read-only volumes
(`-v …:ro`), and tmpfs/named-volume types — all straightforward extensions on
the same table.

**Update 2026-06-30 (increment 10): `-v` CLI flag — DONE.** The volume
mechanism now reaches end-to-end from the shell: `oci run <dir> -v
/srv/data:/data` (also `--volume`, repeatable) parses each spec on the first
`:` (Docker order), validates both sides are absolute, and installs the bind
mount via `add_volume_mount` while the container is still in Created state —
before the init process launches. Usage/help strings updated. Container
self-tests 18/19 were also made deterministic this session (synthetic
never-scheduled PID instead of a real init process that could exit mid-test and
clear its namespace — see B-CONTAINER-JAIL-TESTRACE). Build clean, boot-test
green ("Self-test PASSED (19 tests)"). The TD32 remainder above (read-only
volumes, mount-tree/`pivot_root`, tmpfs) is unchanged.

**Update 2026-06-30 (increment 11): port publishing (`-p`) — DONE.** Docker
`-p host:container[/proto]` port publishing landed, reusing the existing
`net::nat` port-forward table. `Container` gained `container_ip` (captured from
the configured network IP) and `published_ports`; `add_port_publish` records
publish intents (Created-only, requires a network IP, rejects port 0, last-
writer-wins, capped at `MAX_PUBLISHED_PORTS`); `run()` installs them as NAT
rules forwarding host traffic to the container IP inside its netns; `stop()`
flushes them and `delete()` clears the intents. CLI: `oci run -p
8080:80[/udp]` (repeatable). Container self-test 20 covers the lifecycle
deterministically (forwards are per-netns, not per-PID). This is orthogonal to
the rootfs/volume mount-namespace scope; the TD32 mount remainder (read-only
volumes, mount-tree/`pivot_root`, tmpfs) is still open.

**Update 2026-06-30 (increment 12): env injection (`-e`) — DONE.** Docker
`-e KEY=value`/`--env` environment injection landed entirely in the CLI launch
path (`kshell::cmd_oci` `run`/`create`); the container/kernel layer needed no
change because env already passes through `SpawnOptions::envp`. The parser
requires `KEY=value` (a bare `-e KEY` is rejected — a container has no host
environment to inherit) and rejects an empty key. At launch the CLI `-e` entries
are merged over the image's declared ENV with Docker override semantics: each
`-e` entry wins over an image ENV entry with the same key, and the merged set has
no duplicate keys (CLI entries added first, then image ENV entries whose key is
not already overridden). Usage/help strings updated to include `[-e KEY=value
...]`. The TD32 mount remainder (read-only volumes, mount-tree/`pivot_root`,
tmpfs) is still open.

**Update 2026-06-30 (increment 13): `docker`/`dk` CLI-compat shim — DONE.** A
thin Docker-CLI front-end (`docker`, alias `dk`) translates familiar verbs to
the native `oci` (image) and `container` (lifecycle) handlers: `run`/`create`
→ `oci run`/`create`; `ps [-a]` → `container list` (all states; `-a` accepted +
ignored since there is no running-only index); `start`/`stop`/`rm` →
`container start`/`stop`/`delete`; `inspect` → `container info`; `exec` →
`container exec`; `images <dir>` → `oci inspect` (SlateOS has no name-keyed
image registry — images are on-disk OCI layout dirs). Argument spacing is
preserved verbatim when delegating. Registered in dispatch, `is_builtin`, and
the tab-completion list.

**Update 2026-06-30 (increment 14): resource limits (`--memory`/`--cpus`) —
DONE.** `oci run`/`create` now accept Docker `--memory`/`-m <SIZE>` (bytes with
optional binary k/m/g[b] suffix, rounded up to whole 16 KiB frames → cgroup
`mem_limit`) and `--cpus <N[.M]>` (fractional cores → percent of one core, e.g.
`1.5` → 150 → `CpuLimit::from_percent` via cgroup `cpu_quota`). Parsing is pure
and float-free (kernel has no FPU state in this path); two helpers
(`parse_mem_size_to_frames`, `parse_cpus_to_percent`) are covered by
`kshell::cli_resource_parser_self_test()`, wired into the boot self-test run in
`main.rs`. The TD32 mount remainder (read-only volumes, mount-tree/`pivot_root`,
tmpfs) is still open.

**Update 2026-06-30 (increment 15): read-only volumes (`-v …:ro`) — DONE.**
Docker `-v host:guest[:ro|:rw]` now carries an access mode end-to-end. The
volume table entry (`VolumeMount` in `namespace.rs`, `VolumeSpec = (guest,
host, read_only)` in `container.rs`) gained a `read_only` flag; `add_volume`
and `add_volume_mount` take it (last-writer-wins, so re-mounting the same guest
prefix `:rw` clears a prior `:ro`). Enforcement is a new
`namespace::check_writable(path)` / `check_writable_for(pid, path)` that mirrors
the exact resolution pipeline used by `resolve_path_for` — step-1 namespace
translation, `..`-clamping `normalize_jailed`, then longest-prefix volume match —
and returns `KernelError::ReadOnlyFilesystem` (EROFS) when the matched volume is
read-only. It is a cheap `Ok(())` no-op for any process without volumes or
without a chroot root (all non-container processes, and containers with only
read-write volumes), making the wide enforcement surface zero-risk to existing
behavior. Two chokepoints gate writes: (1) fd-based writes via
`fs::handle::open()` reject up front when the open flags request write/create/
truncate/append; (2) ~17 path-based mutating `Vfs` methods (`write_file`,
`write_at`, `truncate`, `remove`, `remove_recursive`, `mkdir`, `mkdir_all`,
`rmdir`, `rename`/`rename_noreplace` via `rename_inner`, `rename_exchange`,
`set_permissions`, `set_times`, `set_xattr`, `remove_xattr`, `symlink`, `link`,
`atomic_write`) call the namespace check on the caller's (guest) path before
host-path resolution. The `_resolved` variants are intentionally *not* gated
(they take already-translated host paths). CLI: `oci run -v /srv/data:/data:ro`
parses an optional third `:mode` segment (`ro`/`rw`, default `rw`); unknown
modes are rejected. Covered by `namespace::test_volume_mounts` (read-only volume
write-denied / read-allowed assertions) and container self-test 19
(`check_writable_for` on `/logs` ro vs `/data` rw vs `/bin/sh` rootfs).
The TD32 mount remainder (a true longest-prefix mount-tree subsuming the rootfs
as the `/` mount / `pivot_root` target, `--read-only` root, and tmpfs/named-
volume types) is still open.

**Update 2026-06-30 (increment 16): read-only root (`--read-only`) — DONE.**
Docker `--read-only` now makes the whole container rootfs non-writable while
writable (`:rw`) volumes still punch writable holes through it. A per-process
flag set `PROCESS_ROOT_RO` in `namespace.rs` (set via `set_root_read_only(pid,
ro)` / queried via `is_root_read_only`, cleared on `detach`/`clear_root` for
PID-reuse safety) feeds the same `check_writable_for` decision used for `:ro`
volumes: longest-prefix volume match first (a `:ro` volume → EROFS, a `:rw`
volume → allowed), and when *no* volume matches the path lives in the rootfs, so
it is denied iff the root is read-only. The fast-path `Ok(())` no-op now also
requires a writable root, so non-container processes and writable containers are
still zero-cost. `ContainerConfig` gained a `read_only_root` field + `.read_only(bool)`
builder; the flag rides through `create` → `add_process_task`, which calls
`set_root_read_only(pid, true)` after installing volumes (only when a chroot root
exists). Post-create `container::set_read_only_root(id, ro)` (Created-state-gated,
like `set_root_path`) mirrors the volume setter; `ContainerInfo` reports it. CLI:
`oci run … --read-only` (a bare flag) prints `Root FS: read-only`. Covered by
`namespace::test_volume_mounts` (read-only-root block: rootfs denied, `:rw`
volume still writable, flag-clear restores writability) and container self-test
19b (now 21 tests total). The TD32 mount remainder is now just the true
longest-prefix mount-tree subsuming the rootfs as the `/` mount (`pivot_root`
target) and tmpfs/named-volume types.

**Update 2026-07-01 (increment 17): tmpfs mounts (`--tmpfs`) — DONE.** Docker
`--tmpfs /guest` now mounts an ephemeral in-memory filesystem at a guest path.
Modeled as a bind mount whose host target is a per-container `fs::memfs` mount:
`add_tmpfs_mount(id, guest)` (Created-only) validates the guest path (absolute,
not `/`, no duplicate against existing volumes/tmpfs), then — outside the table
lock — `Vfs::mkdir_all` + `memfs::mount` a fresh in-memory fs at a unique host
mountpoint `/var/lib/slate/tmpfs/<id>-<index>`, and records it as a **writable**
`VolumeSpec` at the guest prefix so all the existing volume resolution/write
machinery (`resolve_path_for`, `check_writable_for`, `..`-clamping) applies
unchanged. The `Container` gained a `tmpfs_mounts: Vec<String>` of owned
mountpoints; `delete()` unmounts and `remove_recursive`-removes each so nothing
leaks. CLI: `oci run … --tmpfs /tmp` (repeatable) — mount **options** (`--tmpfs
/tmp:size=64m`) are explicitly rejected with a warning rather than silently
ignored (an unbounded tmpfs is a containment/DoS gap until per-mount quota
enforcement lands; honest failure until then). Covered by container self-test 46
(two mounts, bad-spec/duplicate rejection, writable-memfs write+read-back,
non-Created rejection, delete-unmount verification — now 60 tests total). Build/
clippy clean, boot-test green. With this, the volume *types* are all covered —
host bind mounts (`-v /host:/guest`), read-only volumes (`:ro`), named volumes
(`-v NAME:/guest` via `volume::ensure`), and now tmpfs (`--tmpfs`). The TD32
mount remainder is therefore now just the true longest-prefix mount-tree
subsuming the rootfs as the `/` mount (the `pivot_root` target) — the last
structural piece, not a volume-type gap.

**Update 2026-07-01 (increment 18): container-aware `/proc/<pid>/mountinfo` —
DONE.** A container (jailed) process now sees *its own* mount view in
`/proc/<pid>/mountinfo` instead of the host's global mount table. Previously
`gen_pid_mountinfo` rendered `Vfs::mounts_full()` for every PID, so a process
inside a container observed the entire host mount topology (an info leak) and
none of its own rootfs/volumes/tmpfs (a correctness gap). Fix:
`namespace::mount_view_for(pid)` returns `None` for an unjailed process (keep
the global table) or the container's ordered view — the rootfs at guest `/`
(read-only iff `--read-only`), then each volume/tmpfs at its guest prefix with
its own `:ro`/`:rw` flag. `procfs::render_container_mountinfo` renders it,
resolving each entry's *fstype* from the real host mount backing its
`host_target` (`fstype_for_host_path` longest-prefix match: overlay for the
rootfs, tmpfs/memfs for `--tmpfs`, the host fs for binds) while reporting the
`source` field as `none` so host backing paths are **not** leaked into the
container. The same container-aware rendering was applied to the `/proc/mounts`
line format (`render_container_mounts`): the global `/proc/mounts` now resolves
the *caller's* view (`current_task_id`), and a new per-PID `/proc/<pid>/mounts`
(hence `/proc/self/mounts`) file mirrors Linux's mount-namespace-local table.
Covered by procfs self-tests (container view for both `mountinfo` and `mounts`:
RO rootfs→overlay, RO bind→ext4, RW tmpfs→tmpfs; plus `mount_path_covers`
boundary safety so `/data` doesn't cover `/database`). Build/clippy clean,
boot-test green. Note this is *introspection* only — real in-container
`mount`/`umount`/`pivot_root` syscalls mutating a per-container mount table
remain the deferred mount-namespace piece.

### TD33. Container `logs` capture works only for Linux-ABI container inits — ACCEPTED LIMITATION 2026-06-30

**Where:** `kernel/src/container.rs` (`redirect_output_to_capture`, called from
`run_with_abi` right after `spawn_process`). The capture works by rewriting the
init process's **Linux fd table** — `pcb::linux_fd_take(pid, 1)` then
`linux_fd_install_at(pid, 1, FdEntry::file(capture_handle, O_WRONLY))` and
`linux_fd_dup2(pid, 1, 2)` — during the window after spawn but before the init is
scheduled.

**The limitation.** The `linux_fd_table` is only installed for **Linux-ABI**
binaries (`spawn.rs`: `if is_linux_abi { … linux_fd_install_stdio(pid) }`).
Native SlateOS binaries have no `linux_fd_table`, so `linux_fd_install_at` fails,
`redirect_output_to_capture` returns `None`, the container's `log_path` stays
empty, and `container logs ID` returns `NotFound`. A native-ABI container init's
stdout/stderr therefore goes to the console and is **not** captured to
`/var/log/containers/<id>.log`.

**Why it's accepted, not blocking.** Real Docker/OCI container entrypoints are
Linux-ABI glibc ELFs, which is exactly the path the capture supports. The
native-ABI container init is a SlateOS-specific corner case (no real image ships
one), so the Docker-compatible `logs` feature is correct and sufficient for its
intended use. The self-test (19t) deliberately forces `AbiMode::Linux` via
`run_with_abi` so it exercises the real capture path deterministically.

**Proper fix (deferred).** Also wire capture through the **native** fd-inheritance
channel (`initial_fds` / `SpawnOptions.fd_map`, consumed via
`SYS_PROCESS_GET_INITIAL_FDS`): install the capture handle as fd 1/2 in the
native init's `initial_fds` when the ABI is Native. Deferred because it needs
verification that native binaries honour `initial_fds` for stdout and that the
file-offset-sharing (single append position for interleaved 1+2) semantics match
the Linux-fd path — unverified today, and shipping it unverified would violate
the no-band-aid rule. Trigger to do it: a real native-ABI container init appears,
or `initial_fds` stdout semantics are confirmed.

### TD31. Cgroup `nr_tasks` accounting is attach/detach-symmetric only, not membership-accurate — RESOLVED 2026-07-02

**RESOLUTION (2026-07-02).** Made membership counting symmetric with task
lifetime. The **detach half** had already landed in `reap_dead_tasks` (commit
`d7b926037`, 2026-07-01): a reaped task in a non-root cgroup calls
`cgroup::detach_task(task_cgroup)` after `drop(state)` (SCHED released → TABLE,
preserving lock order). This 2026-07-02 change adds the matching **attach half**
in `sched::spawn_with_affinity`: after the `without_interrupts`/SCHED critical
section ends and SCHED is dropped, a task that inherited a non-root cgroup calls
`cgroup::attach_task(inherit_cgroup)` (ROOT skipped, matching the reap-side skip;
TABLE taken strictly after SCHED). Because *all* task creation (kernel and user)
funnels through `spawn_with_affinity` (`proc::thread::spawn_user` →
`thread::spawn` → `sched::spawn` → `spawn_with_affinity`), this single site makes
every fork/clone/spawn counted and every reap decremented — a true membership
count. Tasks bound via `set_task_cgroup` (e.g. a container init, which inherits
ROOT at spawn so the spawn-attach is skipped, then is explicitly bound) stay
balanced: attach at bind, detach at reap.

**Why it's now safe (was BLOCKED on a boot hang).** The earlier attempt hung the
boot twice because the extra `TABLE` lock traffic aggravated
**B-PREEMPT-SPINLOCK** — a `crate::sync::Mutex` held across an involuntary
preemption could deadlock against a higher-priority spinner on a single CPU. That
root cause was fixed 2026-07-01 (per-CPU `PREEMPT_DISABLE_COUNT`: a tracked mutex
now disables preemption while held). With that fix, re-applying the attach edit
booted **green 4× consecutively** (baseline 190s + 182s/181s/185s), zero hangs,
zero `SPINLOCK STALL`, zero self-test failures, and no `dash`/`pthread` flakes —
exactly the retry trigger this entry documented. `cgroup::delete`'s
`nr_tasks > 0 ⇒ NotEmpty` guard is now a true "container still has live
processes" check.

---

**Original entry (for context):**

**Where:** `kernel/src/cgroup.rs` (`attach_task`/`detach_task`/`stats.nr_tasks`),
`kernel/src/sched/mod.rs` (`sched::spawn` ~L1046 sets `new_task.cgroup_id` on
creation but does **not** call `cgroup::attach_task`; `reap_dead_tasks` ~L2789
removes a dead task without `cgroup::detach_task`). The single authoritative
mover `set_task_cgroup` *does* keep the counts balanced (detach old, attach new).

**The debt.** `nr_tasks` only counts tasks that were *explicitly moved* via
`set_task_cgroup`. Two asymmetries:
1. **Creation:** a task that simply *inherits* its creator's `cgroup_id`
   (the common case — every fork/clone/spawn) bumps no counter, so a busy
   cgroup can report `nr_tasks == 0` while hosting many tasks.
2. **Death:** when a task is reaped, its cgroup's `nr_tasks` is never
   decremented (and `set_task_cgroup`-style moves to ROOT on container
   `remove_process` leave ROOT's count permanently inflated, since the task is
   then killed without a matching detach).

`detach_task` saturates at 0 so neither asymmetry can panic/underflow, but the
counter is unreliable for anything that needs a true membership count (e.g. a
cgroup "no new forks past a task limit" controller, or `cgroup.procs`-style
introspection).

**Why it didn't block container increment 1 (§41):** `container::run` binds the
init task via `set_task_cgroup`, which *does* increment the container cgroup, so
the end-to-end "process billed to container cgroup" assertion (`nr_tasks == 1`)
holds. The self-test cleanup calls `remove_process_task` (a `set_task_cgroup` to
ROOT) *before* killing the task, so the container cgroup returns to 0 and
`delete()` (which requires `nr_tasks == 0`) succeeds.

**Proper fix.** Make membership counting symmetric with task lifetime, not with
explicit moves: call `cgroup::attach_task(inherit_cgroup)` in `sched::spawn` when
a new task adopts a cgroup, and `cgroup::detach_task(task.cgroup_id)` in
`reap_dead_tasks` (after dropping the SCHED lock, honoring the SCHED → cgroup
lock order). Audit ROOT_CGROUP bootstrapping so the idle/boot tasks are counted
consistently. Once symmetric, `cgroup::delete`'s `nr_tasks > 0 ⇒ NotEmpty` guard
becomes a true "container still has live processes" check.

**ATTEMPTED 2026-07-01 — BLOCKED on a boot hang the change triggers/exposes.**
Implemented exactly the proper fix above: `attach_task(inherit_cgroup)` in
`spawn_with_affinity` (after the `without_interrupts`/SCHED critical section, so
the cgroup `TABLE` lock is taken strictly after SCHED, mirroring
`set_task_cgroup`'s order) and `detach_task(task.cgroup_id)` in `reap_dead_tasks`
(capture `task.cgroup_id` under SCHED, `drop(state)`, then detach — TABLE after
SCHED). It builds clean, clippy-0, and the *normal* container lifecycle self-test
(nr_tasks 0→1→0) still passes. **But two consecutive boot tests hung** (BOOT_OK
never printed within 480 s), each time immediately after a **userspace container
init process** was spawned and marked "running" — run #1 hung in the
`container restart` self-test (after `test-restart-ct` task 185), run #2 in the
`container port` self-test (after `test-port-ct` task 187). Reverting *only* the
two sched edits → BOOT_OK reached in 181 s. So the change is the trigger; the
varying hang location within a boot points to a **near-deterministic SMP timing
race** in the process spawn/force-kill/reap path that the *extra cgroup-`TABLE`
lock traffic* (one attach per spawn, one detach per reap) aggravates rather than
a plain AB-BA deadlock (SCHED and `TABLE` are never held nested; charging holds
frame-lock→`TABLE` while reap does `TABLE`→frame-lock but with `TABLE` released
in between, so no static inversion was found by inspection). Note the boot is
*already* mildly flaky independent of this change: the reverted-sched boot run
saw an unrelated `dash script-from-stdin` self-test `InternalError` (see the
dash-flake entry) — consistent with a pre-existing timing fragility in the
ring-3 spawn/reap machinery that this change amplifies.

**Decision (Claude, autonomous):** do NOT land the symmetric-accounting change
until the underlying spawn/kill/reap race is root-caused, because it regresses
boot stability, and the debt it fixes is cosmetic (stale `nr_tasks` for
force-killed-unreaped tasks; `container::delete` ignores the `cgroup::delete`
NotEmpty error with `let _ =`, so accounting drift never blocks teardown). The
`nr_tasks==1` container-billing assertion and the D-CGROUP-TASK-UNASSIGNED
end-to-end memory-charging test both pass without it. **Trigger to retry:** after
the ring-3 spawn/reap SMP race is instrumented (per-lock acquire/spin counters or
a lock-order tracer) and fixed; then re-apply the two sched edits and run the
boot test ≥3× to confirm stability. The exact patch is small and is captured
above so it can be reconstructed.

### TD30. Console TTY line discipline: `^C`/`^\`/`^Z` signal the fg pgrp (canonical + raw), `VMIN`/`VTIME` + `NOFLSH` honoured, orphan-pgrp `SIGHUP`/`SIGCONT` — RESOLVED 2026-06-20

**Where:** `kernel/src/tty.rs` — `feed()` (canonical line editor) and
`raw_read()` (non-canonical reader); driven by `dispatch_console_read` /
`deliver_console_signal` / `console_terminal_ioctl` in
`kernel/src/syscall/linux.rs`.

**RESOLVED — gap (1) `ISIG` signal generation (`^C`/`^\`):** the console
now has a foreground process group and delivers terminal signals to it.
`tty.rs` gained a `FOREGROUND_PGID` atomic with
`foreground_pgid()`/`set_foreground_pgid()`, the `TIOCGPGRP` (0x540F) /
`TIOCSPGRP` (0x5410) ioctls (`tcgetpgrp`/`tcsetpgrp`), and a
`ConsoleRead{Data(n)|Signal(sig)}` return from `console_read`. On a
`^C`/`^\` in canonical mode (`feed` → `LineStep::Signal`),
`deliver_console_signal()` resolves the foreground pgrp via
`pcb::pids_in_group` and posts `SIGINT`/`SIGQUIT` (with `SI_KERNEL`
siginfo) to every member, then returns `ERESTARTSYS` so the blocked
reader's signal checkpoint runs — a transparent restart when the reader
isn't in the fg group (or the handler has `SA_RESTART`), otherwise the
default action / `-EINTR`. With no foreground group installed
(`pgid == 0`) no signal is generated and the read simply restarts.

**RESOLVED — Ctrl-Z (`VSUSP`) → `SIGTSTP`:** `feed()` now recognises
`VSUSP` under `ISIG` (default `^Z`) and returns `LineStep::Signal(20)`,
flushing the in-progress line like `^C`/`^\`. `deliver_console_signal`
routes `SIGTSTP` to the foreground pgrp, whose `DefaultAction::Stop`
(already implemented in `proc::signal`) suspends the job; a later
`SIGCONT` (shell `fg`/`bg`) resumes it. `NOFLSH` is not yet honoured.

**RESOLVED — `VTIME`:** `raw_read()` now honours all four `(VMIN, VTIME)`
combinations per POSIX. A new `keyboard::read_char_timeout(deadline_ns)`
(HLT-yield loop bounded by an `hrtimer::now_ns()` deadline) backs the two
timed cases: `VMIN=0,VTIME>0` (bounded read timeout on the first byte) and
`VMIN>0,VTIME>0` (inter-byte timer restarted after each byte, first byte
blocking). `VMIN=0,VTIME=0` (poll) and `VMIN>0,VTIME=0` (count) are
unchanged. VTIME is interpreted in deciseconds.

**RESOLVED — raw-mode `ISIG`:** `raw_read()` now classifies each byte
against `VINTR`/`VQUIT`/`VSUSP` when `ISIG` is set (in all four
`(VMIN,VTIME)` arms) and returns `ConsoleRead::Signal`, discarding any
bytes collected so far in the call (input flush — see the `NOFLSH` note
below for why this is unconditional in raw mode).  Apps that clear `ISIG`
(most full-screen programs) still get the characters as literal data.

**RESOLVED — orphaned-process-group `SIGHUP`/`SIGCONT`:** POSIX requires
that when a process exit orphans a process group that still contains a
*stopped* member, that group be sent `SIGHUP` then `SIGCONT` so wedged
jobs are not stuck forever with no shell able to continue them. Now
implemented in the process-exit path rather than tied to a
controlling-terminal model: `pcb::guarded_child_pgrps(pid)` captures the
distinct groups `pid` *guards* (children in a different group but the same
session) **before** `remove_thread` reparents them to init;
`thread::on_thread_exit` re-checks each captured group after the process
zombifies via `pcb::pgrp_orphaned_with_stopped(pgid)` — true only when no
live member has a guardian (a live parent in a different group of the same
session; zombies count as neither member nor guardian) *and* some member
is stopped — and calls `handlers::kill_orphaned_pgrp(pgid)`, which sends
`SIGHUP` then `SIGCONT` to every member via the authority-free
`handlers::deliver_kernel_signal` (classify → default action). Covered by
the `pcb::test_orphaned_pgrp` boot self-test (guarded-vs-orphaned and the
no-stopped-member negative case).

**RESOLVED — `NOFLSH`:** `feed()` now honours the `NOFLSH` (0x80) lflag in
canonical mode: a signal character (`^C`/`^\`/`^Z`) flushes the in-progress
line by default, but with `NOFLSH` set the buffered input is preserved and
only the signal is generated (the line then completes normally on the next
newline). Raw mode keeps no kernel-side input queue across `read(2)` calls
(each call reads straight from the keyboard), so there is no buffered input
for `NOFLSH` to preserve there — documented on `raw_read`. Covered by the
`tty` boot self-test (NOFLSH-preserves-line) and a `#[cfg(test)]` unit test.

**Severity:** none remaining — interactive `^C`/`^\`/`^Z` (canonical and
raw), `VMIN`/`VTIME` raw reads, orphaned-process-group hangup, and `NOFLSH`
all work (once a shell installs a foreground pgrp via `tcsetpgrp`).

### TD29. Linux signal `siginfo` sender-class (`si_code`/`si_pid`/`si_uid`) — RESOLVED 2026-06-15

**Resolution:** Implemented sender-faithful `siginfo`. `SignalState`
(`kernel/src/proc/signal.rs`) now carries a per-signal `Option<SigInfo>` array co-located
under the same lock as the pending bitmap, recorded on the clear→set transition
(coalescing first-wins, matching Linux's standard-signal `struct sigqueue` behaviour) and
taken at delivery. `SigInfo { code, sender_pid, sender_uid, value }` is threaded through the
post funnel: `kill(2)` → `SI_USER` + sender pid/uid; `tkill`/`tgkill` (`raise`/`pthread_kill`)
→ `SI_TKILL` + sender pid; timer expiry (`setitimer`/`alarm` SIGALRM, `kernel/src/proc/itimer.rs`)
→ `SI_KERNEL`. `build_linux_rt_frame` dequeues the matching record to fill the
`LinuxSiginfo` handed to an `SA_SIGINFO` handler. Verified by the `siginfo
record/deliver/coalesce` unit self-test (13 tests pass) and the `/bin/signal` ring-3 glibc
test, which now asserts `si_code == SI_TKILL (-6)` and `si_pid == getpid()` for `raise()`
(`SLATE_GLIBC_SIGNAL_OK signo=10 code=-6 self=1`).

**Synchronous fault `si_code`/`si_addr` — RESOLVED 2026-06-16 (follow-on to TD29).**
CPU faults on an `AbiMode::Linux` process with an installed handler are now delivered as
real Linux signals with a faithful, fault-specific `siginfo`. A shared emitter
`emit_linux_rt_frame(pid, sig, act, regs: &LinuxTrapRegs, siginfo) -> Option<RtFrameEntry>`
(`kernel/src/syscall/linux.rs`) builds the `rt_sigframe` from a neutral register snapshot, so
it is reused by both the async syscall-return path (`build_linux_rt_frame`, snapshot from the
`SyscallFrame`) and the synchronous fault path (`try_deliver_linux_fault_signal`,
`kernel/src/idt.rs`, snapshot read out of the `InterruptStackFrame` + `SavedRegisters` via
`read_volatile`). `linux_fault_mapping` classifies the trap vector → `(signo, si_code)`:
`#DE`→`SIGFPE`/`FPE_INTDIV`, `#OF`→`SIGFPE`/`FPE_INTOVF`, `#UD`→`SIGILL`/`ILL_ILLOPN`,
`#MF`/`#XM`→`SIGFPE`/`FPE_FLTINV`, `#AC`→`SIGBUS`/`BUS_ADRALN`,
`#BR`/`#NP`/`#SS`/`#GP`→`SIGSEGV`/`SI_KERNEL`; `#PF` is handled in `handle_page_fault`, which
sets `si_addr = CR2` and `si_code = SEGV_ACCERR` (protection, present bit set) or
`SEGV_MAPERR` (not mapped). For non-`#PF` faults `si_addr =` faulting RIP. The emitter does
**not** re-arm on a frame-build failure — the fault caller terminates instead, since resuming
would immediately re-fault. Native processes keep the SEH-style `SignalContext` trampoline
(design-decision #4). Verified by the `/bin/fault` ring-3 glibc self-test
(`self_test_linux_real_glibc_fault`, `kernel/src/proc/spawn.rs`): a real `#PF` store to an
unmapped `0xDEAD000` enters an unmodified glibc `SA_SIGINFO` `SIGSEGV` handler that reads
`si_signo==11`/`si_code==SEGV_MAPERR(1)`/`si_addr==0xdead000` and `siglongjmp`s out, printing
`SLATE_GLIBC_FAULT_OK signo=11 code=1 addr=0xdead000` (boot test PASSED).

**`SI_QUEUE` `si_value`/`si_ptr` payload — RESOLVED 2026-06-16 (follow-on to TD29).**
`rt_sigqueueinfo(2)`, `rt_tgsigqueueinfo(2)` and `pidfd_send_signal(2)` now read the
user-supplied `siginfo`, copy out `si_code` and the 8-byte `si_value` union
(`read_user_siginfo_payload`, SMAP-safe via `copy_from_user`), record the value on the
pending signal, and stamp it into the delivered `siginfo_t` at the correct ABI offset
(struct +24) via the new `LinuxSiginfo::queue(...)` builder; `build_linux_rt_frame`
branches to it when `si_code == SI_QUEUE`. The shared kill funnel was refactored into
`kill_common_value` / `tgkill_common_value` / `sys_signal_send_with_info(args, si_code,
value)` so all gate ordering (EFAULT → forging-EPERM → ESRCH-before-EINVAL → authority)
is shared and only the final post stamps the payload. Linux's `do_rt_sigqueueinfo`
forging gate (`(si_code >= 0 || si_code == SI_TKILL) && caller != target → EPERM`) is now
enforced on all three queued-signal entry points; the recorded `si_pid`/`si_uid` is the
*real caller* (faithful + unforgeable), only `si_value`/`si_code` come from the user.
Verified ring-3 by `/bin/sigqueue` (`sigqueue(getpid(), SIGUSR1, {.sival_int=0x12345678})`
→ handler reads `si_code==SI_QUEUE(-1)`, `si_value.sival_int==0x12345678`,
`si_pid==getpid()`, printing `SLATE_GLIBC_SIGQUEUE_OK signo=10 code=-1 value=0x12345678
self=1`, boot test PASSED) plus in-kernel forging-gate (EPERM) and SI_QUEUE-bypass
(ESRCH-before-EINVAL) assertions.

### TD28. Linux `munmap` is 16 KiB-frame-granular (delegates to native handler), not 4 KiB-page-granular — FIXED 2026-06-16

**Where:** `kernel/src/syscall/linux.rs` — `sys_munmap` delegates to the native
`kernel/src/syscall/handlers.rs::sys_munmap`.

**What it is:** the native `munmap` requires a **16 KiB-frame-aligned** start
(`vaddr.is_multiple_of(FRAME_SIZE)`, else `BadAlignment` → `EINVAL`), rounds the
length **up** to a whole 16 KiB frame, unmaps at whole-frame granularity, and
removes only a VMA that *starts exactly* at `vaddr` (`pcb::remove_vma`, not the
`remove_vma_range` surgery). Linux `munmap(2)` on x86-64 accepts any **4 KiB
(page)**-aligned start and unmaps an arbitrary page-granular sub-range, splitting
VMAs at 4 KiB boundaries. So three behaviours diverge from Linux:
1. A 4 KiB-aligned-but-not-16-KiB-aligned start returns `EINVAL` where Linux
   succeeds.
2. A length that is a multiple of 4 KiB but not 16 KiB is rounded **up**, so the
   unmap can spill 4 KiB sub-pages into an adjacent mapping that shares the
   straddling 16 KiB frame.
3. A partial unmap that does not start on a VMA boundary drops no VMA record
   (leaves a stale `[start,end)` VMA), where Linux would split it.

**Why it is not currently biting:** every base address our `mmap` hands back is
16 KiB-aligned (we allocate whole frames), and glibc only `munmap`s regions it
received from `mmap`, so in practice the start is always 16 KiB-aligned and
adjacent glibc mappings are themselves 16 KiB-aligned — the round-up does not
cross into a live neighbour. The Path-Z real-glibc tests (hello/stdio/full/
pthread) all pass with the current handler.

**Proper fix:** give the Linux `sys_munmap` its own 4 KiB-granular path, parallel
to the 4 KiB-granular `sys_mmap`/`sys_mprotect` work: validate `HW_PAGE_SIZE`
(4 KiB) alignment, unmap each 4 KiB sub-page PTE via an `unmap_4k` primitive
(refcount-aware `frame::free_frame` only when the last sub-page of a 16 KiB frame
is unmapped), and call `pcb::remove_vma_range(pid, start, end)` (already 4 KiB-
capable — it splits at arbitrary boundaries) for the VMA surgery, refunding
`RLIMIT_AS` for the actual span. Blocked only by the per-sub-page frame-refcount
bookkeeping (deciding when a shared 16 KiB frame's last 4 KiB tenant leaves).

**Fix (2026-06-16):** `sys_munmap` (`kernel/src/syscall/linux.rs`) now has its own
4 KiB-granular path and no longer delegates to the native handler. It (1) gates
exactly like Linux `do_vmi_munmap` — unaligned (to 4 KiB) start → `EINVAL`; a
length that rounds to zero (incl. `len == 0`) → `EINVAL`; address-arithmetic
overflow or a range leaving user space → `EINVAL` (Linux surfaces all of these as
`EINVAL`, **not** `ENOMEM`); (2) tears down each 4 KiB sub-page PTE via the
existing refcount-aware [`unmap_user_range`] primitive (frees the backing 16 KiB
frame only once its last sub-page tenant is gone, so a partial unmap sharing a
straddling frame with a live neighbour leaves the neighbour intact); (3) performs
4 KiB-boundary VMA surgery via `pcb::remove_vma_range` (splits the covering
VMA(s), retaining/releasing file-backing references for the surviving/removed
pieces); and (4) refunds `RLIMIT_AS` for the bytes of VMAs that *actually*
overlapped `[addr, end)` (computed before the surgery via `linux_vma_overlap_bytes`,
so a never-mapped or VMA-less range refunds 0 — matching that eagerly-mapped PIE
segments were never charged to `linux_as_bytes`). The per-sub-page refcount
bookkeeping that "blocked" this was already solved by `unmap_user_range` (written
for the `MAP_FIXED` overlay path), so no new frame-accounting code was needed.
Verified by an in-kernel gate self-test (`linux.rs` batch 533b: 4 KiB-unaligned
start → EINVAL; 4 KiB-aligned-but-not-16-KiB start no longer EINVAL — reaches pid
resolution → ESRCH from the boot task, proving the alignment is now accepted with
no side effect; out-of-range → EINVAL) plus a clean Path-Z boot-test (BOOT_OK,
0 self-test failures).

**Related fix (2026-06-15):** `remove_vma_range`'s **right** remainder
`[end, vma.end)` previously kept the original `FileBacked.file_offset` while its
`start` moved forward from `vma.start` to `end`, so the surviving high-side piece
of a split file-backed VMA mapped the wrong bytes. Now built via `vma_subrange`
(which advances `file_offset` by `end - vma.start`), matching the `protect_vma_range`
surgery. The left remainder was already correct (its start is unchanged).

### TD27. `mprotect` updates PTE permissions but not VMA flags — a reclaimed-then-refaulted RELRO page restores the old (writable) permission — FIXED 2026-06-15

**Where:** `kernel/src/syscall/linux.rs` — `sys_mprotect`; the VMA surgery lives in
`proc::pcb::protect_vma_range` (with `vma_subrange` for boundary splitting and
`vma_coverage_gaps` for the hole/ENOMEM check). The demand-fault resolver that
reconstructs a PTE from the covering VMA's `flags` is `pcb::try_resolve_fault` /
`pcb::resolve_subpaged_fault`.

**What it was:** `mprotect(2)` changed the live page-table entries for the range
but did **not** split/adjust the underlying `Vma.flags`. As long as the page stayed
resident this was invisible, but if a page in the range was later reclaimed under
memory pressure (`madvise(MADV_DONTNEED)`, or a future swap/anon reclaim path) and
re-faulted, the fault resolver rebuilt the PTE from the *VMA's* stale `flags` — so a
page glibc made read-only for RELRO would come back **writable**, silently weakening
the hardening. There was also a *correctness* bug for demand-paged mappings: glibc's
pthread thread-stack path `mmap(PROT_NONE)` then `mprotect(…, RW)` *before first
touch*, so a PTE-only mprotect left the not-yet-faulted region with its stale
PROT_NONE protection and the worker thread's stack writes faulted — surfacing as
`pthread_create` → EINVAL.

**Fix (2026-06-15):** `sys_mprotect` now calls `pcb::protect_vma_range`, which
performs per-subpage VMA surgery — it splits the covering VMA(s) at the (4 KiB-
aligned) range boundaries via `vma_subrange` (adjusting `FileBacked.file_offset`
and dup'ing backing references for the extra pieces) and recomputes
`WRITABLE`/`NO_EXECUTE` on `Vma.flags` for the affected sub-range, so the fault
resolver reconstructs the correct permissions after reclaim *and* freshly-mmapped
demand-paged regions fault in with the post-mprotect protection. Coverage (Linux's
"ENOMEM on a genuine hole") is checked before any mutation via `vma_coverage_gaps`
combined with a present-PTE check, so the eagerly-mapped (VMA-less but PTE-present)
PIE main-executable segments that glibc RELRO-protects are accepted while true holes
still return ENOMEM. Verified by the Path-Z real-glibc pthread self-test
(`proc::spawn::self_test_linux_real_glibc_pthread`: 4 threads via clone+TLS, 40000
mutex/futex ops, pthread_join) reaching `SLATE_GLIBC_PTHREAD_OK` and exit 13.

### TD26. User-mode CET shadow-stack state (`IA32_PL3_SSP`, `IA32_U_CET`) will be the next instance of the F13/F14 bug class when user CET is enabled — FORWARD-LOOKING HAZARD 2026-06-14

**Where:** `kernel/src/cet.rs` — `set_user_cet(enable_shstk, enable_ibt, user_ssp)`
and `read_user_ssp()`, both currently `#[allow(dead_code)]`. The per-task
context-switch save/restore lives in `kernel/src/sched/mod.rs` (the two
switch sites near lines 3779/3795 and 3974/3985 that already restore
`IA32_FS_BASE` and `IA32_GS_BASE`).

**What it is:** a forward-looking hazard, not a live bug. User-mode CET
(shadow stacks / IBT) is **not currently wired up for user tasks** — the
shadow-stack MSRs `IA32_PL3_SSP` (per-thread user SSP) and `IA32_U_CET`
(per-thread user CET config) are written only by the dead-code
`set_user_cet`, which nothing calls. So today there is no per-thread CET
state to clobber. The doc comment on `set_user_cet` already *claims* it is
"Called during context switch to restore per-task CET state" — that wiring
does not yet exist.

**Why it matters:** `IA32_PL3_SSP` and `IA32_U_CET` are exactly the same
**bug class** as F13/F14 (FS/GS base): they are userspace-settable
*per-thread* CPU register state that lives in MSRs, **not** in the saved GP
`Context` and **not** in the XSAVE area unless XSAVES + the CET_U state
component (bit 11) is enabled. The moment user shadow stacks are turned on,
each thread gets its own shadow stack and its own SSP; if the SSP (and the
U_CET enables) are not saved on switch-out and restored on switch-in, the
first context switch will leave a thread running on another thread's shadow
stack → spurious `#CP` faults or a security hole (shadow-stack reuse). This
audit (the same sweep that found F13/F14) flagged it proactively so it is
not re-discovered the hard way.

**Proper fix (when user CET is enabled):**
1. Add `pub user_ssp: u64` and `pub user_cet: u64` fields to `Task`
   (`kernel/src/sched/task.rs`), symmetric to `fs_base`/`gs_base`; `0` =
   no user CET (the default).
2. In both `sched::mod.rs` switch sites, after the FS/GS restore, restore
   `IA32_PL3_SSP` and `IA32_U_CET` for user tasks (gated on the task
   actually having CET enabled, to avoid a `#GP` writing an SSP MSR when
   CET is off in CR4/U_CET).
3. Sync the fields wherever the SSP/U_CET change: thread creation (allocate
   the shadow stack), `clone`/`fork` (new thread gets a fresh shadow stack;
   `fork` child inherits the parent's SSP value but its own COW shadow-stack
   page), and `exec` (reset to a fresh shadow stack or `0`).
4. Alternatively, if XSAVES is adopted, enabling the CET_U state component
   (XCR0/IA32_XSS bit 11) folds SSP/U_CET into the existing
   `xsave64`/`xrstor64` context-switch path — preferable because it reuses
   the FPU save machinery instead of hand-rolled MSR save/restore. Decide
   between explicit MSR save and XSAVES-CET_U at the time user CET lands.

**Trigger:** do this in the same change that first calls `set_user_cet`
from a live path (i.e. when user-mode shadow stacks / IBT are enabled for
user processes). Until then this is inert dead code and there is nothing to
fix.

### TD24. `link`/`linkat` return a blanket `EROFS` regardless of mount/filesystem — RESOLVED 2026-06-16 (Path Z Part 28)

**Resolution (2026-06-16, commit 5c8ae3e77 "Wire link/linkat to the VFS"):**
this is no longer accurate. `link`/`linkat` now do real VFS work for ring-3
callers: `link_common` (`kernel/src/syscall/linux.rs`) resolves oldpath/newpath
against the caller's cwd/dirfds via `resolve_at_path`, requires a File-WRITE
capability, and calls `Vfs::link`. ext4 implements real hard links (the Part 28
self-test creates one on the `/mnt` ext4 mount and reads it back); memfs cannot
share an inode between two names, so it correctly reports unsupported (mapped to
the filesystem-appropriate errno, matching Linux's `EPERM` for an FS without a
`->link` op — not the misleading `EROFS` this entry was filed against). Only the
kernel-context path (`caller_pid().is_none()`, no fd table) still returns the
`EROFS` terminal, which is required to keep the batch-481 syscall-fidelity
self-test green. The two residual fidelity gaps — `Vfs::link` always follows a
symlink oldpath (so plain `link(2)`'s no-follow contract and `linkat` without
`AT_SYMLINK_FOLLOW` are not honoured for the rare symlink-oldpath case) and
memfs lacking hard-link support (an inode-table refactor) — are tracked under
**B-SYM1**, not here. The historical analysis below is retained for context.

**Where (historical):** `sys_link` / `sys_linkat` in `kernel/src/syscall/linux.rs` (both
return `errno::EROFS` after validating their path/flags arguments).

**What it is:** no filesystem in the OS implements hard links, so both syscalls
fail unconditionally with `EROFS` ("read-only file system"). Linux instead
returns errno by case, in `do_linkat`/`vfs_link` order: oldpath missing →
`ENOENT`; newpath already exists → `EEXIST`; the two paths are on different
mounts → `EXDEV`; the destination mount is read-only → `EROFS`; and a writable
filesystem that simply lacks a `->link` op → `EPERM`. The common real case —
`link("/tmp/a", "/tmp/b")` on our *writable* `/tmp` memfs — should be `EPERM`
(unsupported), not `EROFS` (which misleadingly claims the mount is read-only).

**Related sub-fix landed 2026-06-14 (directory `st_nlink`):** memfs previously
hardcoded every node's `st_nlink` to `1`, including directories. A Unix
directory's link count is `2` (its name in the parent + its own `.`) plus one
per immediate subdirectory (each subdir's `..`); files/symlinks do not bump it.
`find(1)`'s leaf optimisation keys off `nlink == 2` (no subdirs ⇒ skip stat'ing
entries), so the hardcoded `1` both defeated that optimisation and reported a
count no real filesystem produces. memfs now computes directory link counts
honestly via `MemFsNode::nlink_count()` (files/symlinks still report `1` because
file hard links remain unimplemented — the main debt below). This does NOT
resolve TD24: `link`/`linkat` still return blanket `EROFS`.

**Why it's not a live bug today:** programs that use `link(2)` for speed
(git's `link_or_copy`, rsync `--link-dest`, `cp -l`, `ln`) fall back to copying
or report the error; none branch on `EROFS`-vs-`EPERM` in a way that corrupts
data. The only observable effect is a misleading error *message* on an
operation that cannot succeed regardless.

**Proper fix:** the real fix is hard-link support in the backing filesystems
(a substantial FS feature — memfs/ext4/FAT inode link-count + dirent aliasing).
Until then, an interim accuracy improvement would resolve oldpath/newpath, emit
`ENOENT`/`EEXIST`/`EXDEV` (the `KernelError::CrossDevice` variant added 2026-06-14
already maps to `EXDEV`) / `EROFS` / `EPERM` in Linux's order. That interim step
was deliberately NOT taken: faithfully reproducing `do_linkat`'s lookup ordering
(`AT_SYMLINK_FOLLOW` oldpath resolution, `AT_EMPTY_PATH`, dirfd resolution,
parent `ENOTDIR`/trailing-slash handling) for a syscall that always fails risks
introducing *new* divergences that are worse than the current honest-but-coarse
`EROFS`. Revisit when hard links are actually implemented.

### TD23. No `/sys/devices/system/cpu/cpuN/cache/` tree — lscpu/hwloc cannot read real cache geometry — RESOLVED 2026-06-13

**Resolution (2026-06-13):** Built the per-CPU `cache/indexI/` sysfs subtree in
`kernel/src/fs/sysfs.rs`, sourced from `cpu::cache_topology()`. Each detected
cache level/type exposes `level`, `type`, `size`, `coherency_line_size`,
`ways_of_associativity`, `number_of_sets` (all directly CPUID-derived, honest)
plus `shared_cpu_map`/`shared_cpu_list` derived from the real topology via
`cache_shared_cpus()` — which matches `max_sharing` against the known per-core
(thread-sibling) and whole-package scopes and never overclaims (an unplaceable
clustered cache falls back to the known-true per-core subset). The tree is
present only when geometry was detected (`cache_index_count() > 0`); when CPUID
reports nothing (e.g. QEMU's default model) the `cache/` dir is absent rather
than fabricated. lscpu's existing reader (fixed under the previous commit)
lights up automatically with real data on hardware that exposes caches.
Self-test step 13 covers both the populated and absent paths. The original
debt write-up follows for history.

---

**Original debt (now resolved):**

**Where:** kernel `kernel/src/fs/sysfs.rs` (would add a `cache/indexN/` subtree
under each `cpuN`), data source `kernel/src/cpu.rs::cache_topology()` (returns
real CPUID-derived `CacheInfo { level, cache_type, size, line_size, ways, sets,
shared, max_sharing }`). Consumer `userspace/lscpu/src/main.rs` reads
`/sys/devices/system/cpu/cpu0/cache/indexN/{level,type,size,ways_of_associativity}`.

**What:** The sysfs per-CPU `cache/` subtree does not exist yet, so lscpu has no
honest source for L1/L2/L3 cache sizes. As of this entry lscpu correctly
*omits* cache lines it cannot source (the previous behaviour — printing
fabricated `32K`/`256K`/`8192K` defaults and hardcoded `8`/`16` associativity —
was removed because it showed invented numbers as if real). The result is
correct but less informative: `lscpu` and `lscpu -C` show no cache rows.

**Proper fix:** Build the kernel `cache/indexN/` tree from
`cpu::cache_topology()`, exposing the Linux files: `level`, `type`
(`Data`/`Instruction`/`Unified`), `size` (e.g. `32K`/`8192K`),
`coherency_line_size`, `ways_of_associativity`, `number_of_sets`, and
`shared_cpu_map`/`shared_cpu_list`. The geometry fields are all directly
honest (CPUID-derived). `shared_cpu_list` can be derived from `max_sharing`
under our contiguous CPU-numbering model (cache instance for cpuN groups the
`max_sharing` contiguous CPUs containing N) — verify this matches the topology
before relying on it; if `max_sharing` cannot be mapped to a specific CPU set
honestly, omit the share-map files rather than guess. Once the tree exists,
lscpu's existing reader lights up automatically with real data.

**Severity:** low — cosmetic/informational; no correctness impact on CPU
*enumeration* (count/topology come from the already-correct `online`/`present`
range files and `topology/` subtree). Tracked as the follow-up to the
CPU-enumeration sysfs work.

### TD22. File-backed `mmap` — Phase 1 (demand-paged `MAP_PRIVATE`) DONE; Phase 2 read-only unified cache PLANNED-DEFERRED (C-lite, §23); writable `MAP_SHARED` WON'T-FIX — UPDATED 2026-06-14

**Where:** `kernel/src/mm/vma.rs` (`VmaKind::FileBacked`), `kernel/src/proc/pcb.rs`
(`try_resolve_fault` FileBacked arm, `vma_release_backing`, `remove_vma`,
`remove_vma_range`, `reset_vmas_for_exec`, `fork_create`, `destroy`),
`kernel/src/syscall/linux.rs` — `linux_file_mmap` (the file-backed arm of
`sys_mmap`), plus `unmap_user_range` / `linux_file_mmap_rollback` helpers.

**Phase 1 — DONE (2026-06-14): demand-paged `MAP_PRIVATE` for regular files.**
A private, non-fixed `mmap` of a regular file now registers a
`VmaKind::FileBacked { handle, file_offset }` VMA and allocates **no frames**
up front. The page-fault handler (`pcb::try_resolve_fault`) resolves each page
lazily: allocate a zeroed frame, `read_at(handle, file_offset + (page - start))`
into it (tail stays zero past EOF — Linux page zero-fill), then map. Because
the mapping is private, a write faults onto its own per-process frame and never
reaches the file (correct `MAP_PRIVATE` semantics); once populated the frame is
swap-reclaimable and CoW-shareable across `fork` like any anonymous page.
- **Backing-handle lifetime:** the VMA owns an independent reference on the open
  file description (`dup_shared` at mmap, again per-VMA on `fork`, net
  retain/release on `remove_vma_range` splits), released via `close` on
  `munmap` (`remove_vma`), `execve` (`reset_vmas_for_exec`), and process exit
  (`destroy`). This decouples the mapping's lifetime from the caller's fd:
  `munmap`-after-`close` still reads the right bytes.
- **Bonus fix:** `execve` previously never cleared the per-process VMA list when
  it tore down the old address space (`clear_user_address_space`), leaving stale
  records in `/proc/<pid>/maps` and stale ranges the fault resolver could
  "resolve". `reset_vmas_for_exec` now drops them all (and releases their
  backings) so a fresh image starts with an empty VMA list, matching spawn.

**Still eager (unchanged):** memfd-backed maps, read-only `MAP_SHARED`, and
`MAP_FIXED` overlays (the `ld.so` per-segment loader) keep the eager-copy path —
`VmaKind::Fixed`, frames allocated and `read_at`-filled at map time. memfd has a
separate handle layer; FIXED ranges are typically faulted in immediately by the
loader, so demand paging buys little there.

**Still DEBT — Phase 2: unified page cache + writable `MAP_SHARED`.**
- Writable `MAP_SHARED` is still rejected with `ENOSYS` — we never write
  modified pages back to the file, so the shared-write contract is impossible.
  Any Linux program using shared mmap'd files for IPC or in-place editing (some
  databases, `mmap`-based logging) gets `ENOSYS`.
- Two processes mapping the same file do **not** share physical pages — each
  demand-faults its own private copy. There is no unified page cache shared
  between the VFS read path and mmap, so file pages can be resident twice.
- The fault handler reads the file **synchronously via the VFS** inside the
  page-fault path; a page cache would serve hits without re-reading.

**Phase 2 — split decision (operator, 2026-06-14; supersedes the earlier blanket
won't-fix).** The operator reopened Q5 and chose **C-lite**: a unified
*read-only* page cache. See `design-decisions.md` §23 (which narrows §22).
- **Read-only unified cache — PLANNED, DEFERRED.** Cross-process read-only page
  sharing (shared-library `.text` dedup + de-double-caching against
  `fs/cache.rs`) is adopted in principle but **not built yet**. Trigger to
  implement: the first concrete consumer of read-only page sharing — in practice
  the dynamic linker wanting shared-library text dedup. Precursor: stable VFS
  file-identity (`FileMeta.ino` is 0 for memfs/FAT today). Full deferral
  rationale + trigger logged in `todo.txt`.
- **Writable `MAP_SHARED` writeback — WON'T-FIX (unchanged).** Dirty-tracking,
  `msync`/unmap write-back, and cross-process write coherence remain declined.
  **Writable `MAP_SHARED` of a regular file stays `ENOSYS` indefinitely** — a
  deliberate, accepted limitation, not outstanding debt. C-lite (read-only) needs
  none of this machinery.

When C-lite is built, the proper fix is: a unified page cache shared between the
VFS read path and mmap, with file pages cached once and shared read-only
(refcounted frames). The Phase-1 `VmaKind::FileBacked` fault-path shape is already
the right foundation — C-lite only changes each page's *source* (shared cache
frame vs per-mapping `read_at`). It needs the stable VFS file-identity precursor
above and a double-cache-vs-unify call against `fs/cache.rs`. See
`design-decisions.md` §23.

---

### TD21. Minor Linux-ABI fidelity gap — procfs fd visibility for native processes — APPROXIMATION 2026-06-13; sendfile + copy_file_range + splice + tee + vmsplice transfer IMPLEMENTED 2026-06-14

**Where:** `kernel/src/fs/procfs` (`/proc/<pid>/fd[info]`, `linux_fd_list`) and
`kernel/src/syscall/linux.rs` (`sys_sendfile`, `sys_copy_file_range`,
`sys_splice`, `sys_tee`, `sys_vmsplice`). All are documented in-code.

**What it is:** one remaining deliberate Linux-ABI approximation:
- **`/proc/<pid>/fd/` and `/fdinfo/` are EMPTY for *native* processes.** Native
  processes keep their fd table in userspace (`posix/src/fdtable.rs`), which is
  not kernel-visible, so `linux_fd_list` returns `None` and the readdir yields
  zero entries rather than inventing fds. Only Linux-ABI processes (which use the
  kernel-side `KernelFdTable`) get a populated `fd/`. Same honesty stance as the
  fdinfo `mnt_id:`/`ino:` omission — printing fabricated fds would mislead
  introspection tools.

**Progress (2026-06-14) — sendfile data transfer implemented.** `sys_sendfile`
previously validated its front gates and then terminated `EINVAL` (no transfer).
It now performs a real in-kernel copy via `sendfile_core`: a 64 KiB bounce-buffer
loop reads from the source fd at an absolute byte offset (`fs::handle::read_at` /
`memfd::read_at`, never advancing the open-file cursor) and writes to the
destination fd (`fs::handle::write` / `memfd::write` / `pipe::write|try_write` /
console). Source must be a seekable byte container (File/MemFd — the kinds that
back Linux's sendfile splice_read); destination may be File/MemFd/Pipe/Console.
Position semantics match Linux: with a NULL `offset` the source's file position is
read-from and advanced; with a non-NULL `offset` the read starts at `*offset`, the
file position is untouched, and the post-transfer position is written back via
`put_user` (which — as on Linux — can override a successful copy with EFAULT if the
offset slot became unwritable). `count` is clamped to `MAX_RW_COUNT` (0x7fff_f000);
the transfer stops at source EOF or a short destination write; a pipe whose reader
has gone yields EPIPE. Error semantics follow `do_sendfile`: a first-byte error
propagates, a later error returns the partial count. Tested end-to-end by the
post-`/tmp` boot self-test `self_test_sendfile` (File→File whole-file, offset+count
slice, count-clamp-to-remaining, EOF→0, File→MemFd and MemFd→File cross-kind). The
gate-only `self_test_sendfile_splice_aio` batch-538 checks still pass (kernel-context
callers have no fd table, so the syscall still terminates `EINVAL` before the
transfer). The `put_user(pos)` write-back EFAULT noted previously is now modelled.

**Progress (2026-06-14) — copy_file_range data transfer implemented.**
`sys_copy_file_range` was likewise a validate-front-gates-then-`EINVAL` stub. It
now performs a real positional in-kernel copy via `copy_file_range_core`: a
64 KiB bounce-buffer loop that reads from the source at an absolute byte offset
(`read_at`) and writes to the destination at an absolute byte offset (`write_at`,
which extends the file) — neither cursor is touched by the core, so distinct
source/dest offsets are honoured. Both source and destination must be regular-file
kinds (File/MemFd), matching `vfs_copy_file_range`'s `S_ISREG` requirement; pipes
and consoles are rejected `EINVAL`. The full Linux gate order is now enforced:
fds (`EBADF`) → offset readability (`EFAULT`) → `flags != 0` (`EINVAL`) →
regular-file (`EINVAL`) → access-mode/`O_APPEND` (`EBADF`). Position semantics
mirror sendfile: a NULL `off_in`/`off_out` reads-from and advances the file's own
cursor; a non-NULL pointer supplies an explicit position, leaves the cursor, and
writes the post-transfer position back via `put_user` (EFAULT-after-copy modelled).
`len` is clamped to `MAX_RW_COUNT`; the same first-byte-propagate / later-error-
returns-partial semantics apply. Linux's same-file-overlap `EINVAL` is enforced by
`copy_file_range_overlaps`. **LIMITATION:** "same object" is detected by open-path
equality (File) / raw-handle identity (MemFd); two *hardlinks* to one inode have
distinct paths and are not detected as overlapping — Linux compares inodes, which
our fd layer does not expose here (same approximation the rest of the path layer
makes). Tested end-to-end by the post-`/tmp` boot self-test
`self_test_copy_file_range` (File→File positional whole-file, positional read
offset, positional *write* offset, File→MemFd / MemFd→File cross-kind, and
overlap-detect true/false/cross-kind). The batch-537 gate-only checks still pass
(kernel-context callers terminate `EINVAL` before the transfer).

**Progress (2026-06-14) — splice data transfer implemented.** `sys_splice` was
also a validate-front-gates-then-`EINVAL` stub. It now moves data via
`splice_core`, a 64 KiB bounce-buffer loop where File/MemFd ends are read/written
positionally (`read_at`/`write_at`) and pipe ends use their own cursors
(`pipe::read|try_read` / `pipe::write|try_write`). The full Linux gate order is
preserved (len==0→0; flags mask→EINVAL; fds→EBADF; pipe-end-with-offset→ESPIPE;
offset readability→EFAULT; FMODE_READ/WRITE→EBADF) and the do_splice prologue
gates are added: at least one end must be a pipe (else EINVAL — sendfile territory),
a non-pipe end must be a splice-capable regular file (File/MemFd, else EINVAL), and
splicing a pipe to its own other end (ipipe==opipe) is EINVAL. `SPLICE_F_NONBLOCK`
selects the non-blocking pipe path; a broken pipe yields EPIPE, non-blocking
exhaustion with nothing moved yields EAGAIN. Position semantics match the siblings
(NULL offset advances the file cursor; explicit offset is read/written-back via
`put_user`). Tested by `self_test_splice` (File↔Pipe, Pipe→Pipe, positional
read+write offsets, empty-source→EAGAIN, and the no-loss bound).
**LIMITATION (pipe→pipe data-loss race):** our model copies bytes (read source →
write dest) rather than moving pipe buffers by reference like Linux. To avoid
discarding already-consumed source bytes on a partial destination-pipe write, the
non-blocking read is bounded to the destination pipe's current free space
(`readable_bytes` on the write end). A *concurrent* writer racing to fill the
destination pipe between the space probe and the write is the only residual loss
window; it is bounded to one 64 KiB chunk and is no worse than any non-atomic
splice. The blocking path has no such window (its inner write loop drains the full
chunk). Proper fix would require reference-counted pipe buffer pages (Linux's
`pipe_buffer` model) so splice transfers ownership instead of copying.

**Progress (2026-06-14) — tee data transfer implemented.** `sys_tee` was the
last validate-then-`EINVAL` stub in this family. It now duplicates data from one
pipe to another *non-destructively* via `tee_core`, built on two new pipe
primitives (`kernel/src/ipc/pipe.rs`): `peek_at(handle, offset, buf)` copies
buffered bytes at a logical offset without consuming them, and `wait_readable`
blocks for input without consuming (the blocking-tee path). Gates match `do_tee`:
flags/len front gates (unchanged batch-540), then FMODE_READ/WRITE→EBADF and the
"two distinct pipes" requirement (both ends must be pipes with different ids,
else EINVAL). `SPLICE_F_NONBLOCK` selects non-blocking; a broken destination
pipe→EPIPE, non-blocking empty source→EAGAIN, EOF source→0. Because tee never
consumes the source, the splice pipe→pipe data-loss concern does **not** apply
here — a partial destination write simply copies fewer bytes and leaves the
source intact. Tested by `self_test_tee` (duplicate + verify the source is
unchanged, empty→EAGAIN, EOF→0, len-clamp). The whole splice/tee/vmsplice
gate-only batch checks (539/540/541) still pass.

**Progress (2026-06-14) — vmsplice data transfer implemented.** `sys_vmsplice`
was the last validate-then-`EINVAL` stub in the zero-copy family. It now moves
data between a process's user iovecs and a pipe via `vmsplice_core`. Direction is
chosen by the pipe-fd's access mode (Linux's `vmsplice_type`): a write-end
(FMODE_WRITE) **gathers** user buffers into the pipe (ITER_SOURCE); a read-end
(FMODE_READ) **scatters** pipe bytes out to the user buffers (ITER_DEST). The
Linux gate order is preserved: flags `& !SPLICE_F_ALL`→EINVAL; fd validity→EBADF;
`nr_segs==0`→0; `nr_segs>1024` (UIO_MAXIOV)→EINVAL; iov pointer NULL→EFAULT;
`validate_user_read` of the iovec array→EFAULT; then the fd must resolve to a
**pipe** (non-pipe→EBADF, matching `get_pipe_info`). Each 16-byte iovec is parsed
(`iov_base`,`iov_len`), zero-length segs skipped, and `iov_base==0` /
`base+len > USER_SPACE_END` rejected EFAULT; the running total is capped at
`MAX_RW_COUNT`→EINVAL. A broken pipe yields EPIPE, non-blocking exhaustion with
nothing moved yields EAGAIN, and the first-byte-propagate / later-error-returns-
partial convention matches the siblings.
  The novel piece is **cross-address-space user access.** The existing
`copy_from_user`/`copy_to_user` (`mm/user.rs`) target the *current* CR3 via
STAC/CLAC, which is the kernel's own address space at boot (no user mappings), so
they can't be exercised by a boot self-test. Two new pml4-parameterized primitives
— `copy_from_user_as(pml4, src, dst)` and `copy_to_user_as(pml4, dst, src)` —
walk an *explicit* page table and reach each user page through the HHDM
(physical→kernel direct map), sidestepping SMAP entirely. In production
`sys_vmsplice` passes the caller's own pml4 (`cr3_to_pml4(read_cr3())`); the self-
test passes a throwaway process's pml4. These are also the reusable primitive a
future `process_vm_readv`/`writev` will need. Tested by the post-process-init boot
self-test `self_test_vmsplice`, which spins up a throwaway PCB, maps two adjacent
writable user frames, and checks: cross-page `copy_from_user_as`, cross-page
`copy_to_user_as` plus rejection of an unmapped VA, ITER_SOURCE (user→pipe), and
ITER_DEST (pipe→user). The batch-541 gate-only checks still pass (kernel-context
callers have no caller-pid/fd table and terminate before the transfer).

**Bug fixed in passing (2026-06-14) — rmap/swap-reclaimable leak on process
teardown.** While boot-testing vmsplice the `mm/compact.rs` Test 5 assertion
("collect_private_frames should find our fake entry") began panicking. Root cause
was a genuine pre-existing correctness bug, *not* the test: `clear_user_address_
space` (`mm/page_table.rs`) freed a process's user frames (`frame::free_frame`)
without ever calling `rmap::remove` or `swap::unregister_reclaimable` for them. So
every process destroy/exec leaked stale reverse-mappings and reclaimable entries
pointing at frames that were freed and could be reused — a real hazard (memory
compaction or the swap reclaimer could act on a freed-and-reused frame) that also
let leaked rmap entries accumulate until they crowded out compact's fragile
4-slot probe. **Fix:** in the frame-freeing loop, before `free_frame`, the page-
table indices are reassembled into the frame's virtual base
(`(pml4_idx<<39)|(pdpt_idx<<30)|(pd_idx<<21)|(pt_idx<<12)`) and used to call
`rmap::remove(frame_phys, pml4_phys, virt_base)` and
`swap::unregister_reclaimable(pml4_phys, virt_base)` (both no-ops for untracked
frames). After the fix the rmap table is empty at compact time and Test 5 passes
(`found=1, saw_fake=true`).

**Impact:** low — native-process fd introspection via `/proc` is unavailable
(tools must use the native fd API).

**Proper fix:** procfs fd — expose a kernel-visible view of native fd tables (or a
read bridge into the userspace fd table) so `/proc/<pid>/fd` works uniformly.

### TD20. Userspace crate verification & lint-cleanup gaps — coreutils RESOLVED 2026-06-14; guitk pedantic still DEBT 2026-06-13

**Where:** `gui/toolkit/` (guitk). (The coreutils half is resolved — see below.)

**What it is:** two low-priority verification/lint gaps in userspace crates:
- **coreutils host-test gap (2026-05-31) — RESOLVED 2026-06-14.** The affected
  bins (`stat`, `du`, `chown`, `chmod`, `tar`, `test`, `ln`) now follow the
  `stat.rs` pattern: every `std::os::unix` import and the unix-only logic sit
  behind `#[cfg(unix)]`, a `#[cfg(not(unix))]` stub `main` keeps the non-unix
  host compile-clean, and the pure formatting/parsing helpers live outside the
  gate with host-runnable unit tests. Verified 2026-06-14:
  `cargo test -p coreutils --target x86_64-pc-windows-gnu` compiles and runs
  green on the Windows dev host (20 test binaries, ~480 tests, 0 failures), so
  the host `cargo test` path now works alongside the slateos build. (Originally:
  coreutils unit tests couldn't compile on the Windows dev host because bins
  used `std::os::unix::fs::{PermissionsExt, MetadataExt}`, which only exist on
  unix-family targets.)
- **guitk pedantic deferral (2026-06-03):** guitk does not yet enable
  `#![deny(clippy::pedantic)]`; a pedantic run emits ~1,232 warnings,
  overwhelmingly doc-style (`missing_panics_doc`, `missing_errors_doc`,
  `must_use_candidate`, `return_self_not_must_use`, `needless_pass_by_value`,
  `similar_names`, `items_after_statements`). The crate is ~50k LOC; cleanup is a
  multi-session sweep, deferred until core subsystems (kernel/mm/sched/ipc, fs,
  drivers) reach a stable baseline — little value in extensive doc lints on
  toolkit code while the syscall ABI is still in flux. (Related to TD19's
  lint-policy conflict.)

**Impact:** low — neither blocks feature work; both crates build for slateos.

**Proper fix:** coreutils — DONE (the `#[cfg(unix)]` gating + `not(unix)` stub
`main` pattern is now applied across the affected bins; host `cargo test`
compiles and passes). guitk — a dedicated pedantic-cleanup sweep once the core
ABI stabilizes, resolved together with the TD19 lint-policy decision.

### TD19. Crate-root `#![deny(clippy::pedantic)]` overrides the workspace lint allow-list — DEBT 2026-06-13 (needs operator policy call)

**Where:** every crate carrying a crate-root `#![deny(clippy::all,
clippy::pedantic)]` (e.g. `posix/src/lib.rs`) vs. the root `Cargo.toml`
`[workspace.lints.clippy]` block. Reproduce: `cargo clippy -p posix --target
x86_64-pc-windows-gnu` reports ~3038 errors + 260 warnings.

**What it is:** rustc applies crate-root attributes *after* and at higher
precedence than the command-line lint flags Cargo derives from
`[workspace.lints]`. So a crate-root `#![deny(clippy::pedantic)]` re-denies the
whole pedantic group and overrides every per-lint `= "allow"` in
`[workspace.lints.clippy]`. The only allows that survive are ones *also* listed
in that crate's own `#![allow(...)]`. Result: workspace-allowed lints
(`unreadable_literal` ~1943, `must_use_candidate` ~761, `manual_let_else`, …)
fire as hard errors anyway. The 260 warnings (`indexing_slicing` 171,
`arithmetic_side_effects` 89) are correct warn-level per the workspace config.
This is a design conflict between (a) CLAUDE.md's mandate of
`#![deny(clippy::all, clippy::pedantic)]` in every crate and (b) the newer
`[workspace.lints.clippy]` block (`pedantic = "warn"` + centralized allow-list)
documented as the intended suppression mechanism — mutually exclusive while both
are in force. Note: 15 userspace tools have already adopted `[lints] workspace =
true` and dropped their crate-root deny, so the conflict is being resolved
piecemeal in that direction.

**Impact:** low — bare-metal build and all host tests are green; this only
affects `clippy -p <crate>` noise. Not blocking feature work.

**Proper fix:** an **operator policy call**, because CLAUDE.md is operator-owned
and OPT 1 relaxes its "deny in every crate" rule:
- **OPT 1 (recommended):** remove the redundant crate-root deny from each crate
  and rely on `[lints] workspace = true`; the workspace config becomes
  authoritative (`clippy::all` deny, pedantic warn, allow-list effective).
  Residual non-allowed lints then surface as warnings to fix or add to the
  allow-list. Downside: pedantic becomes warn-level workspace-wide.
- **OPT 2:** keep the crate-root deny and copy the full workspace allow-list into
  every crate's `#![allow(...)]` — the per-crate duplication the workspace block
  was created to avoid. Already done in source: `decimal_bitwise_operands` was
  relaxed at both the workspace level and in `posix/src/lib.rs` (our `linux_*`
  ABI constant tables mirror upstream kernel headers verbatim, so hex literals
  would obscure the correspondence). Trigger: dedicated lint-policy pass once the
  operator picks an option.

### TD18. A group of userspace net/disk/admin tools target syscalls that don't exist in the native ABI — DEBT 2026-06-13

**Where:** `userspace/` tools — net-config (`dhcpcd`, `fw`, `ifconfig`, `ip`,
`nft`, `route`), mount (`mount`, `umount`), disk-admin (`mkfs`, `fsck`,
`diskutil`), and `chroot`. Authoritative syscall list:
`kernel/src/syscall/number.rs`.

**What it is:** a 2026-05-30/31 audit of ~55 userspace tools that hand-roll
inline-asm syscalls found most tools were either already correct or fixable by
migrating to `std` / posix `extern "C"` symbols (those fixes shipped — see git
log for jq/zip/ssh-keygen/curl/dig/whois/screen/telnet/stty/df/chown/chmod/
monctl/date/at/nmap/ntpd/hwclock). The residual group below calls syscalls that
**genuinely do not exist** in the native ABI, so they cannot be fixed by a
client-side number correction:

- **net-config** (`dhcpcd`, `fw`, `ifconfig`, `ip`, `nft`, `route`): all issue
  `SYS_NET_IOCTL=810` (which aliases `UDP_BIND`) for interface/route/DNS/firewall
  *writes*. **Interface-address writes: kernel syscall LANDED 2026-07-02** —
  `SYS_NET_IF_CONFIG=856` (`kernel/src/syscall/number.rs`, dispatched in
  `dispatch.rs`, handled by `sys_net_if_config` in `handlers.rs`) is the native
  write side of `NET_IF_INFO=842`: root-gated (`require_netadmin_authority`), it
  applies IPv4 address/mask/gateway/DNS and/or the up/down flag to the physical
  NIC via `net::interface::configure`/`set_up` (new `set_up` helper), using an
  18-byte record with a per-field mask (bit0..4 = ip/mask/gateway/dns/up) so a
  tool changes only the fields it means to (read-modify-write). Boot self-test
  `net::interface::test_write_primitives` (snapshot→configure→toggle up/down→
  restore) verified in serial. **Tool rewiring (a): DONE 2026-07-02** —
  `ifconfig`, `ip`, `route`, and `dhcpcd` now issue `SYS_NET_IF_CONFIG=856`
  instead of the neutered `net_ioctl` stub, via a shared `build_config_record`/
  `net_if_config` (host-unit-tested per tool). Mapping: `ifconfig eth0 <ip>` →
  IP bit; `ifconfig ... netmask` → MASK bit; `ifconfig up/down` → UP bit;
  `ip addr add <ip>/<prefix>` → IP|MASK (prefix→mask); `ip addr del` → IP=0;
  `ip link set up/down` → UP; `ip route add/del default via <gw>` and
  `route add/del default gw <gw>` → GATEWAY bit (clearing to 0 on del);
  `dhcpcd` applies a whole lease (IP|MASK|GATEWAY|UP) in one call. Fields the
  kernel model can't represent are now honest hard errors instead of silent
  fake-success: `ifconfig` MTU/explicit-broadcast. Host tests green: `ifconfig`
  38, `ip` 23, `route` 15, `dhcpcd` 110.
  **Route-table follow-up (b) — DONE 2026-07-02.** Three native route syscalls
  now exist (`kernel/src/syscall/number.rs`): `SYS_NET_ROUTE_ADD=857`
  (root-gated, 16-byte record `[dest(4), mask(4), gateway(4), metric(4 LE)]`,
  rejects 0.0.0.0/0), `SYS_NET_ROUTE_DEL=858` (root-gated, 8-byte
  `[dest(4), mask(4)]`), `SYS_NET_ROUTE_LIST=859` (read-only, fills a buffer
  with 16-byte records, returns count). All operate on the caller's netns via
  `crate::sched::current_task_net_ns()` and the pre-existing per-namespace
  `netns` route table (`add_route`/`remove_route`/`routes`). The *default*
  route (0.0.0.0/0) still lives in the interface gateway (SYS_NET_IF_CONFIG),
  not the table — see design-decisions §52; `resolve_next_hop` for the root
  namespace now consults `route_lookup(ROOT_NS, dst)` before the interface
  gateway fallback. Boot self-test: `ipv4::root_route_next_hop_self_test()`
  (runs after `netns::init()`; adds a TEST-NET-3 route, checks the next hop,
  removes it). The `ip` tool (`ip route add/del <prefix> via <gw> [metric]`)
  and `route` tool (`route add/del -net/-host …`, and `route flush`) now issue
  these syscalls for non-default routes and list the table via
  `SYS_NET_ROUTE_LIST`; the default-route path still uses the interface
  gateway. **Firewall write path DONE 2026-07-02:** `SYS_NET_FW_ENABLE`/
  `_SET_POLICY`/`_ADD_RULE`/`_DEL_RULE`/`_FLUSH` (860–864, root-gated, per-netns
  with root ns == global firewall) expose `net::firewall`'s write path.
  `ADD_RULE` takes a 12-byte binary record mirroring `Rule` 1:1
  (`[direction, action, protocol, src_prefix, dst_port:u16le, priority:u16le,
  src_ip:4]`); reads stay on `/proc/net/firewall`. The `fw` tool now issues
  these syscalls (`fw enable/disable/allow/deny/policy/delete/reset/load` apply
  to the kernel; `apply_to_kernel` does flush+re-add so kernel state matches the
  in-memory set). Rules the kernel model cannot represent (a `src_port` or
  `dst_ip` constraint) are **skipped with a warning** rather than pushed as a
  broader rule — see design-decisions §53. `fw delete N` maps the list position
  to the correct kernel index (counting only representable rules). See
  design-decisions §53 for the ABI + fail-safe rationale. **Still TODO:** the
  `nft` tool (3.6k-line nftables front-end) is not yet wired to these syscalls,
  and IPv6 firewall rules (`Rule6`) have no write syscall yet — both are
  separate follow-ups. Original harm analysis (traced
  2026-06-01): with a Socket-WRITE cap the old call silently binds+leaks a UDP
  socket on a low port and misleads the user that the config change applied;
  without the cap it fails. **Write-path harm neutered 2026-06-14** for all six
  net-config tools (`ifconfig`/`ip`/`route` then `dhcpcd`/`fw`/`nft`) — see the
  dedicated bullets below.
- **mount/umount**: **RESOLVED 2026-06-20.** Real native syscalls now exist —
  `SYS_FS_MOUNT=652` and `SYS_FS_UMOUNT=653` (`kernel/src/syscall/number.rs`),
  dispatched in `dispatch.rs`, handled in `handlers.rs`
  (`sys_fs_mount`/`sys_fs_umount`, root-gated via `require_mount_authority`).
  `SYS_FS_MOUNT` takes three ptr+len string pairs (source/target/fstype —
  consuming all six arg slots, so mount *flags* are deferred to a future
  versioned extension) and dispatches on the fstype string to the existing
  in-kernel backends (ext4/tmpfs(memfs)/iso9660/devfs/proc/sysfs/vfat).
  `SYS_FS_UMOUNT` takes target ptr+len and refuses `/` and busy mounts. Kernel
  boot self-test: `fs::vfs::mount_self_test()` (mounts a scratch tmpfs at
  `/_mount_selftest`, write/read roundtrip, confirms `/` is unmountable-refused,
  unmounts) — runs unconditionally on any root. The `userspace/mount` tool now
  issues these real syscalls (via a `syscall6` inline-asm helper) instead of
  returning ENOSYS; `canonical_fstype` maps user fstype names to kernel
  fstypes, bind/remount are rejected (unsupported by the ABI), and mount
  options emit a "not yet honoured" warning. Host unit tests: `cargo test -p
  mount --target x86_64-pc-windows-gnu` (6 pass). The redundant
  `userspace/mount-cli` demo tool (which printed *fabricated* mount listings and
  fake-succeeded without a syscall) was **removed 2026-06-20** — all three of
  its personalities are already covered by real, non-fabricating tools:
  `mount`/`umount` (the tool above) and the standalone `userspace/findmnt`
  (reads `/proc/mounts`). Nothing referenced `mount-cli`. (Judgment call —
  removal is reversible via git; see todo.txt.) The analogous
  `userspace/mkfs-cli` and `userspace/fsck-cli` demo shims (which *fabricated*
  mkfs/fsck success — fake UUIDs, "done", "clean, NNN/NNN files" — without
  issuing any syscall, telling the user a format/check succeeded when nothing
  happened) were **removed 2026-06-20** for the same reason: all their
  personalities are already covered by the real, syscall-backed `userspace/mkfs`
  (argv0 `mkfs.<type>` detection → `SYS_FS_FORMAT`) and `userspace/fsck` (argv0
  `fsck.<type>` detection → `SYS_FS_CHECK`). The shims' extra aliases
  (`e2fsck`/`xfs_repair`/`mkswap`) were pure fabrication for filesystems we don't
  support; reintroducing any as a real alias is a future task with real backing.
  Nothing referenced either crate. (Judgment call — removal is reversible via
  git; see todo.txt.)
- **mkfs/diskutil format: RESOLVED 2026-06-20** — added a real
  `SYS_FS_FORMAT=654` (`kernel/src/syscall/number.rs`), dispatched in
  `dispatch.rs`, handled in `handlers.rs` (`sys_fs_format`, root-gated via
  `require_format_authority`). ABI: arg0/arg1 = device-name ptr+len (the
  block-device registry name, e.g. "vda"/"sda" — **not** a `/dev/` path),
  arg2/arg3 = fstype ptr+len, arg4/arg5 = optional label ptr+len (0/0 = none).
  The handler dispatches on the fstype string to the existing in-kernel
  `fs::fat::mkfs_fat(device, label)` for the FAT family (vfat/fat/fat32/fat16/
  msdos); all other fstypes return `NotSupported` (ext4 mkfs not yet ported;
  tmpfs has no device to format). Kernel boot self-test:
  `fs::fat::format_self_test()` — registers a 4 MiB `RamBlockDevice` ("fmttest0",
  added to `blkdev.rs` alongside `blkdev::unregister`), runs `mkfs_fat`, mounts
  the formatted volume via `FatFs::mount` + `Vfs::mount` at `/_fmt_selftest`,
  write/read roundtrips a file, then tears everything down — runs unconditionally
  on any root (verified "[fat] mkfs/format self-test PASSED" in serial). Both
  `userspace/mkfs` and `userspace/diskutil format` now issue the real syscall
  via a `syscall6` inline-asm helper (FAT family only; unsupported fstypes report
  an honest "kernel cannot format X yet" error instead of ENOSYS). mkfs warns
  that `-F`/`-s`/`-S` are advisory (the kernel backend auto-selects FAT type and
  cluster geometry from device size). Host tests: `cargo test -p mkfs --target
  x86_64-pc-windows-gnu` (35 pass).
- **fsck/diskutil verify+repair: RESOLVED 2026-06-20** — added a real
  `SYS_FS_CHECK=655` (`kernel/src/syscall/number.rs`), dispatched in
  `dispatch.rs`, handled in `handlers.rs` (`sys_fs_check`, root-gated via
  `require_fsck_authority`). ABI: arg0/arg1 = device-name ptr+len (the registry
  name, e.g. `vda`/`sda`, NOT the `/dev/` path), arg2 = flags (bit0 = repair).
  Returns the count of *outstanding* errors (after repair if requested) or a
  negative `KernelError`. FAT only — delegates to the existing in-kernel
  `fs::fat::fsck_fat(device, repair)`. Kernel boot self-test:
  `fs::fat::fsck_self_test()` — registers a 4 MiB `RamBlockDevice` ("fscktest0"),
  `mkfs_fat`, then `fsck_fat(dev, false)` (expects 0 errors) and
  `fsck_fat(dev, true)` (expects 0 outstanding after repair), teardown via
  `cache::invalidate` + `blkdev::unregister`; runs unconditionally on any root
  (verified "[fat] fsck self-test PASSED" in serial). Both `userspace/fsck`
  (rewired from the **colliding** `652`/`653` — which I had just reassigned to
  `SYS_FS_MOUNT`/`SYS_FS_UMOUNT`, so `fsck` was invoking mount/umount with garbage
  args; now uses `655` + `FS_CHECK_REPAIR=1<<0`) and `userspace/diskutil`
  (`verify` = `fs_check(false)`, `repair` = `fs_check(true)`) now issue the real
  syscall. Host tests: `fsck` 39 pass, `mkfs` 35 pass, `diskutil` 0.
- **diskutil usage/statfs: RESOLVED 2026-06-20** — diskutil's `usage` was an
  ENOSYS stub falling back to a sysfs size estimate, but a real native
  `SYS_FS_STATVFS=608` syscall already existed (`sys_fs_statvfs` in handlers.rs,
  backed by the fully-implemented `Vfs::statvfs(path) -> FsInfo` across
  FAT/ext4/memfs/devfs/iso9660/procfs/sysfs). `cmd_usage` now calls it
  (`fs_statvfs(path)`: path ptr+len + 64-byte buffer → block_size/total/free
  blocks + inodes), printing exact Total/Used/Free/Available/inode figures; it
  only falls back to the sysfs estimate if the syscall genuinely fails. Host
  tests: `diskutil` 5 pass (`read_u64_le` LE-parse + bounds, `syscall_error_msg`,
  `format_size`). The kernel exposes a single free count (no separate
  "available-to-unprivileged"), so diskutil reports available == free.
- **Linux-ABI `statfs`/`fstatfs` returned fixed synthetic data — RESOLVED
  2026-06-20** — `sys_statfs`/`sys_fstatfs` (`kernel/src/syscall/linux.rs`) never
  resolved the path/fd; they always `fill_statfs_default()`'d a hardcoded block
  (TMPFS_MAGIC, 16 GiB total / 8 GiB free, 64K inodes) regardless of the real
  filesystem. So Linux programs calling `statfs("/")` or `df`-style tools got
  bogus capacity. Now `sys_statfs` canonicalises the path against the caller's
  cwd and routes through `Vfs::statvfs`, and `sys_fstatfs` resolves the fd's VFS
  handle to a path (`fs::handle::handle_path`) and does the same; a new
  `fill_statfs_from_info` maps `FsInfo` → the 15-`u64` `struct statfs` layout
  with a real `f_type` super-magic (`statfs_magic_for`: ext4 0xEF53, FAT 0x4d44,
  iso9660 0x9660, procfs 0x9fa0, sysfs, else TMPFS_MAGIC). NotFound → ENOENT;
  non-VFS fds (pipes/eventfd/…) and virtual filesystems still get neutral
  defaults (honest — they have no on-disk capacity). The field-packing loop was
  refactored to `chunks_exact_mut` (no index arithmetic). Validated by a new
  post-mount boot self-test `self_test_statfs_root()` (called from main.rs after
  the root is mounted, since the in-`self_test()` checks run pre-mount) asserting
  `statfs("/")` returns 0 with a non-zero `f_type` + `f_namelen`; the pre-mount
  self-test keeps the NULL→EFAULT checks. Boot PASSED.
- **diskutil trim** — **RESOLVED 2026-06-20.** Built the full fstrim stack:
  (1) a block-layer discard primitive — `BlockDevice::supports_discard()`/
  `discard(start_lba, count)` (default not-supported) with a real
  `RamBlockDevice` impl (zeroes the range, fully bounds/overflow-checked) and
  registry helpers `blkdev::supports_discard()/discard()`; (2) `FileSystem::trim()`
  (default no-op `Ok(0)`) + `FatFs::trim()` which walks the FAT, coalesces
  contiguous free clusters into runs and issues `blkdev::discard` for each
  (after `cache::invalidate_range` drops cached copies so stale free-space data
  can't resurface) — **non-destructive**, only free blocks are touched;
  (3) `FileSystem::device_name()` + `Vfs::trim_device(dev)` for device→mount
  resolution; (4) `SYS_FS_TRIM` (656, root-only) wired to `Vfs::trim_device`,
  returning bytes discarded; (5) `diskutil trim` issues the syscall and reports
  the byte count. Three boot self-tests (block-layer discard, FAT fstrim via
  `Vfs::trim_device`, unknown-device rejection) + 5 diskutil host tests. Boot
  PASSED (fstrim discarded 4,160,512 bytes on a 4 MiB scratch volume).
  **Follow-ups (TD18 residual):** virtio-blk does not yet negotiate
  `VIRTIO_BLK_F_DISCARD`, so on real/virtio devices `supports_discard()` is
  false and fstrim is a successful no-op (0 bytes) — discard only actually
  fires on `RamBlockDevice` today; and ext4 still uses the default `trim()`
  no-op (no free-block-bitmap enumeration yet). See todo.txt.
- **chroot**: no `CHROOT`/`CHDIR`/`SETUID`/`SETGID`/`SETGROUPS` syscall — needs a
  real process-credential + filesystem-root ABI. **Already neutered** — `chroot`
  carries ENOSYS stubs and a comment about the earlier fake syscall numbers.

**Impact:** these specific tools are non-functional (no-op at best). They are not
on any critical path, so nothing currently blocks on them.

**Read-path wiring — DONE 2026-06-14.** The decision-free near-term win below
has been applied to all three read-path tools (`ifconfig`, `ip`, `route`):
- **`ifconfig` (no-args / `-a` / `-s` / `ifconfig <iface>`) — DONE 2026-06-14.**
  Display mode previously read `/sys/class/net/` and `/proc/net/dev`, neither of
  which the kernel populates (sysfs only serves `kernel`/`params`/`devices`;
  `/proc/net` is a flat file with no `dev`/`if_inet` subfiles), so the tool
  reported "No network interfaces found". It now falls back to the existing
  read-only `SYS_NET_IF_INFO=842` syscall, decoding the 24-byte record
  (ip/mask/gw/dns/mac/up) into a synthesized `eth0` interface (counters left at
  0 — the syscall carries none — rather than fabricating traffic stats). Pure
  decode/format helpers (`parse_net_if_info`, `fmt_ipv4`, `fmt_mac`,
  `compute_broadcast`) are host-unit-tested (8 new tests; `cargo test -p
  ifconfig` 32 pass). The **write** paths (`up`/`down`/`set ip`/…) no longer
  issue the bogus `SYS_NET_IOCTL` — see the write-path safety fix below.
- **`ip` (`ip addr show`, `ip link`, `ip route`, `ip stats`) — DONE 2026-06-14.**
  Same dead read paths (`/sys/class/net/`, `/proc/net/dev`, `/proc/net/route`).
  `read_interfaces` now falls back to `SYS_NET_IF_INFO` to synthesize the `eth0`
  interface, and `read_routes` synthesizes the default route from the record's
  gateway field. `ip neigh` previously read the unpopulated `/proc/net/arp`; it
  now falls back to the read-only `SYS_ARP_TABLE=843` syscall (12-byte records:
  ip/mac/ttl), reusing the `arp` tool's count-bounded parse + zero-MAC =
  INCOMPLETE convention. 14 host tests total (`cargo test -p ip`: 14 pass; +4 for
  ARP). Write paths (`ip link set`, `ip addr add/del`, `ip route add/del`)
  no longer issue the bogus `SYS_NET_IOCTL` — see the write-path safety fix below.
- **`route` (`route`, `route -n`, `route -v`) — DONE 2026-06-14.** Its
  `/proc/net/route`, `/sys/net/routes`, and `/proc/net/if_inet` sources are all
  unpopulated; `read_routes` now synthesizes the connected network route and the
  default route from `SYS_NET_IF_INFO`. 4 new host tests (`cargo test -p route`:
  10 pass). Write paths (`route add/del/flush`) — see the write-path safety fix
  below.
- **`netstat` (`-t`/`-l`/`-r`/`-i` connection, route, and iface views) — DONE
  2026-06-14.** Its `/proc/net/{tcp,udp,route,dev}` and `/sys/class/net` sources
  are all unpopulated. It now falls back to the read-only diagnostic syscalls:
  connection list ← `SYS_TCP_LIST=840` (20-byte records) + listeners ←
  `SYS_TCP_LISTENER_LIST=841` (4-byte records, mapping the kernel
  `net::tcp::TcpState` discriminant to netstat's state enum); route view ←
  `SYS_NET_IF_INFO=842` (connected + default route, same synthesis as `route`);
  iface view ← `SYS_NET_IF_INFO` (name/MTU) + `SYS_NET_STAT=825` (48-byte
  counters; rx_errors/tx_dropped reported as 0 since the kernel exposes
  neither). UDP has no kernel socket-table syscall, so the UDP connection view
  stays empty. 9 new host tests (`cargo test -p netstat`: 31 pass). netstat is
  read-only (no write paths).
- **`ss` / `sockstat` (TCP socket view) — DONE 2026-06-14.** Reads
  `/proc/net/{tcp,tcp6,udp,udp6,raw,raw6,unix}`, all unpopulated. The TCP view
  now falls back to `SYS_TCP_LIST=840` + `SYS_TCP_LISTENER_LIST=841` (IPv4 only,
  so the fallback is skipped under `-6`), mapping the kernel `net::tcp::TcpState`
  discriminant to ss's `SocketState`. UDP/raw/unix have no kernel enumeration
  syscall yet and stay empty. NOTE: unlike the other net tools, ss's existing
  `run_ss`/`run_sockstat` unit tests exercise `gather_sockets`, which reaches the
  query functions — so the real `syscall` asm is gated
  `#[cfg(all(target_arch="x86_64", not(test)))]` with an ENOSYS stub under
  `test` to avoid executing a raw syscall on the host; the pure record decoders
  are unit-tested directly. 5 new host tests (`cargo test -p ss`: 37 pass). ss is
  read-only (no write paths).

**Write-path safety fix — DONE 2026-06-14** (`ifconfig`, `ip`, `route`). The
write paths in these three tools were worse than the "harmless no-op" originally
documented. Each defined `const SYS_NET_IOCTL: u64 = 810` and called
`syscall(810, cmd, …)` where `cmd` ∈ {1,2,3,10,11,12} (up/down/set-ip/route
add/del/flush) was passed as **arg0**. But `810` is `SYS_UDP_BIND` and its arg0
is a **port number** — so every config command actually bound a UDP socket to
port 1/2/3/10/11/12, leaked the returned handle, and — because the handle is a
non-negative return value — reported **false success** to the user. (`route`
additionally carried a dead `net_ioctl6`/`syscall6` path, and a dead
`/sys/net/routes/*` sysfs write fallback the kernel never serves.) Fix: removed
the fabricated `SYS_NET_IOCTL` constant from all three; `net_ioctl` now returns
`-38` (ENOSYS) **without issuing any syscall**, with a doc comment explaining the
`810` aliasing; removed `route`'s dead `net_ioctl6`/`syscall6`; added honest
`-38 → "Function not implemented (... not yet supported on Slate OS)"` arms to
`route`'s add/del error matches. Result: false-success-with-socket-leak becomes
an honest failure + non-zero exit until the net-config ABI lands. The read-only
`SYS_NET_IF_INFO`/`SYS_ARP_TABLE` query wrappers (`syscall3`/`syscall4`) are
retained and still used. All three still cross-compile for `x86_64-slateos` and
pass clippy + host tests (ifconfig 32, ip 14, route 10).

**Write-path safety fix extended to `dhcpcd`/`fw`/`nft` — DONE 2026-06-14.** The
same `SYS_NET_IOCTL=810` misuse lived in the remaining net-config tools:
- **`dhcpcd`** issued `net_ioctl(NET_IF_{SET_IP,SET_MASK,UP,SET_GW}, …)` after
  acquiring a lease — each binding+leaking a UDP socket on port 3/4/1/5 and
  reporting a non-negative "success". `net_ioctl` now returns `-38` without any
  syscall (its only `syscall4` user, so `syscall4` was removed); DHCP transport
  itself is unaffected (it uses `std::net::UdpSocket`). 107 host tests pass.
- **`fw`** was the worst case: besides the write commands, its *read* path
  `fw_ioctl(FW_GET_STATUS)` decoded the leaked UDP socket **handle as firewall
  status bits**, fabricating bogus enabled/logging/policy state. Both `fw_ioctl`
  and `fw_ioctl_buf` now return `-38` (no syscall); the dead direct-`syscall4`
  `load_rules_from_kernel` path and the now-dead kernel-status branch in `load()`
  were removed, so status reads fall back to `/proc/net/firewall` → saved rules
  file → defaults. 40 host tests pass.
- **`nft`** never actually called its `nft_ioctl_buf`/`syscall4` (all dead code
  behind `#[allow(dead_code)]`), so there was no live bug — but the dangerous
  `SYS_NET_IOCTL=810` plumbing was removed outright; the `NFT_*` sub-command
  numbers are kept as documentation of the future control ABI. 102 host tests
  pass.

  **`nft`/`iptables` are stateless and non-functional as configurators — BUG,
  open 2026-07-02.** Separate from the (fixed) syscall-misuse issue: `run_nft`
  and `run_iptables` (`userspace/nft/src/main.rs` ~lines 2264, 2278) each build a
  fresh `Ruleset::new()` per invocation, apply the single command, print, and
  **discard all state on exit**. The tool never persists to a file, never reads
  `/proc/net/nftables`, and never touches the kernel — so `nft add rule …` /
  `iptables -A …` are no-ops that only echo syntax. The module doc's claim that
  "Rules are persisted through `/proc/net/nftables`" is a **doc/reality
  mismatch** (nothing reads or writes that path). The kernel firewall write
  syscalls now exist (860–864, used by `fw`), so wiring is *possible*, but doing
  it well needs (1) a persistence-format decision and (2) a heavily-lossy mapping
  from nftables' model (tables/chains/hooks/sets/maps/NAT) onto our narrow kernel
  `Rule`. That is a genuine design fork, tracked as **open-questions Q21** (A full
  wiring / B minimal wiring / C make it honestly parser-only + steer to `fw`;
  Claude recommends C). Until resolved, `fw` is the one working firewall
  front-end. **Proper fix:** resolve Q21, then either implement the chosen wiring
  or (option C) correct the module doc and print a "not applied — use `fw`"
  notice on mutating `nft`/`iptables` commands.

All three cross-compile for `x86_64-slateos` and pass clippy. With this, **no
remaining userspace tool defines or issues `SYS_NET_IOCTL`/`810` for net-config**
(verified by grep). Only the legitimate `SYS_UDP_BIND=810` users (`dig`, `nc`,
`inetd`, …) reference the number now.

**Disk-admin format-path safety fix — DONE 2026-06-14** (`mkfs`, `diskutil`).
**SUPERSEDED 2026-06-20** — `format` is now wired to the real `SYS_FS_FORMAT=654`
(see the "mkfs/diskutil format: RESOLVED 2026-06-20" bullet above); the honest
ENOSYS stub described here was the interim state. Historical record follows.
The same fabricated-syscall pattern lived in the disk-admin tools: both defined
`SYS_FS_FORMAT=651` and issued `syscall(651, path_ptr, …)`. But `651` is
`SYS_FS_SEEK_HOLE` — a real syscall whose arg0 is a *file descriptor*, not a
path pointer — so a `mkfs`/`diskutil format` actually invoked `seek_hole` with a
userspace pointer reinterpreted as an fd, returning a misleading `EBADF`/`EINVAL`
while formatting nothing. Fix:
- **`mkfs`** — removed `SYS_FS_FORMAT`/`syscall3`; `do_format` now returns the
  honest `ENOSYS` message without issuing a syscall. 35 host tests pass.
- **`diskutil`** — removed `SYS_FS_{IOCTL,FORMAT,VERIFY,REPAIR,TRIM,STATFS}` +
  the `syscall5`/`syscall3`/`syscall2`/`c_str` plumbing; format/verify/repair/
  trim now fail honestly with `ENOSYS`, and `usage` (statfs) skips the kernel
  round-trip and goes straight to its existing sysfs-based estimate. The
  exact-usage formatting is retained, ready to wire once a real statfs ABI lands.
  Builds + clippy clean.
- **`fsck`** left as-is at the time: its `652`/`653` were *unassigned*, so the
  kernel returned a clean `ENOSYS` (no real-syscall aliasing) — benign **then**.
  **SUPERSEDED 2026-06-20** — when `SYS_FS_MOUNT`/`SYS_FS_UMOUNT` were assigned to
  `652`/`653`, `fsck`'s stale numbers started aliasing the mount handlers (a real
  collision I introduced), and the fs-admin ABI now exists. `fsck` was rewired to
  the real `SYS_FS_CHECK=655` — see the "fsck/diskutil verify+repair: RESOLVED
  2026-06-20" bullet above.

**Proper fix:** this is an **operator design decision**, not a mechanical fix —
the kernel must first grow the missing ABI, and the *shape* of that ABI is a
fork: a native net-config syscall family vs. a network-manager IPC daemon for
the net tools; a real mount/umount + fs-admin (format/verify/repair) syscall set;
and a process-credential + fs-root ABI for chroot. The partial near-term win
that needed no decision — wiring the net tools' **read** paths (`ifconfig`, `ip`,
`route`) to `NET_IF_INFO=842` — is now DONE (see above); only the **write** paths
remain blocked on the ABI fork. Trigger to revisit: when the matching kernel
syscalls land (track via roadmap net-config / mount / fs-admin tasks). Related: `sys_clock_settime`/`sys_clock_adjtime` now enforce
`require_clock_authority()` keyed on `uid==0`; revisit to key off a real
per-process `CAP_SYS_TIME` bit when the PCB gains a POSIX capability set (today
`ProcessCredentials` is only uid/gid/groups).

### TD17. inotify event coverage is limited to native-derived events — PARTIAL 2026-06-14 (IN_ISDIR added; was DEBT 2026-06-12)

**Where:** `kernel/src/ipc/inotify.rs` (Linux-ABI adapter) backed 1:1 by
`kernel/src/fs/notify.rs` native watches.

**What it is:** inotify watches are backed 1:1 by native `fs::notify` watches, so
the reportable event set is exactly what the native layer produces:
`IN_CREATE`/`IN_DELETE`/`IN_MODIFY`/`IN_ATTRIB`/`IN_MOVED_FROM`/`IN_MOVED_TO`
(Renamed→pair)/`IN_DELETE_SELF`/`IN_MOVE_SELF`/`IN_ACCESS`/`IN_OPEN`/
`IN_CLOSE_WRITE`/`IN_CLOSE_NOWRITE`, plus synthetic `IN_Q_OVERFLOW` and
`IN_IGNORED`. `IN_ISDIR` is now OR'd into the reported mask whenever the event
subject is a directory (mkdir/rmdir, directory-handle close, a renamed
subdirectory) — `FsEvent` carries an `is_dir` flag threaded through both the
kernel inotify adapter and the native fs_watch ABI (byte 524) into the posix
inotify shim. Watches are NON-RECURSIVE and keyed by
NORMALIZED PATH STRING, not inode — re-adding the same path returns the same wd
(mask replaced, or OR-combined under `IN_MASK_ADD`); a watched path deleted and
recreated keeps the same wd. `IN_ONESHOT`/`IN_DONT_FOLLOW`/`IN_EXCL_UNLINK` are
accepted-but-ignored control bits. Linux FS mutation syscalls
(`mkdir`/`mkdirat`/`rmdir`/`unlink`/`unlinkat`/`rename`/`renameat`/`renameat2`)
now route through the native VFS (`Vfs::mkdir`/`rmdir`/`remove`/`rename`), so
inotify events DO flow from Linux-ABI filesystem operations — including
`IN_MOVED_FROM`/`IN_MOVED_TO` for renames. `renameat2` honours `RENAME_NOREPLACE`
(atomic for the common same-mount case — see below) and `RENAME_EXCHANGE`
(atomic same-mount swap on filesystems that implement it — memfs does; ext4/FAT
return `EINVAL`). `RENAME_WHITEOUT` is rejected with `EINVAL` (overlayfs whiteout
device nodes are unsupported).

**Impact:** low — the common "watch a dir for create/delete/modify/move/open/close"
file-manager/build-tool idiom is fully covered, now including the `IN_ISDIR`
dir-flag and Linux-ABI-driven mutations. Remaining gaps bite only apps that need
inode-identity semantics across delete+recreate (rare), an atomic
`RENAME_NOREPLACE`, or `RENAME_EXCHANGE`/`RENAME_WHITEOUT`.

**Progress (2026-06-14): IN_ACCESS, then IN_OPEN / IN_CLOSE_WRITE /
IN_CLOSE_NOWRITE now implemented.** All three are gated by the lock-free
per-event-bit interest counter (`fs::notify::INTEREST_COUNTS` /
`interest_includes`): watch create/close adjust the counts, and `emit()` plus the
hooks early-out with a few relaxed atomic loads before touching the `WATCHES` lock
unless a live watch actually requests that bit, so they cost nothing when unused
and stay excluded from `ALL_CHANGES`.
- `IN_ACCESS`: `Vfs::read_file` / `Vfs::read_at` emit `FsEventType::Accessed` after
  dropping the VFS lock.
- `IN_OPEN`: `fs::handle::open` emits `FsEventType::Opened` after the handle is
  installed (so a failed allocation never produces a spurious open).
- `IN_CLOSE_*`: `fs::handle::close` emits `FsEventType::ClosedWrite` /
  `ClosedNoWrite` on the final (refcount→0) close, discriminated by the handle's
  write-mode, after dropping the `OPEN_FILES` lock (keeps the
  `OPEN_FILES → WATCHES` lock order one-directional). Directory handles now emit
  their close too, tagged `is_dir` so the adapter ORs in `IN_ISDIR`.
- `IN_ISDIR` (2026-06-14): `FsEvent` gained an `is_dir` flag. `emit_dir` /
  `emit_created_dir` / `emit_deleted_dir` set it; `Vfs::mkdir`/`rmdir` and the
  directory-handle close use them. The kernel inotify adapter ORs `IN_ISDIR` into
  every directory-subject record (create/delete/close/renamed-subdir, never the
  synthetic `IN_IGNORED`/`IN_Q_OVERFLOW`). The native fs_watch syscall ABI carries
  it in record byte 524 (reserved padding before), and the posix inotify shim
  (`epoll.rs::translate_kernel_event`) ORs `IN_ISDIR` the same way.
Covered by `fs::notify::self_test` (interest-gate create/close, synthetic emit,
mask-filtering, end-to-end `Vfs::read_file` ACCESS hook, and an end-to-end
open/close through the handle layer asserting Opened + ClosedNoWrite for read-only
and Opened + ClosedWrite for writable), the inotify boot self-test (a dir-create
event asserting `IN_CREATE | IN_ISDIR`), and the posix `test_translate_isdir_or_in`
host unit test (dir vs file subject, and IN_IGNORED never tagged).

**Progress (2026-06-14): atomic `RENAME_NOREPLACE`.** Gap (b) is resolved for the
common same-mount case. New `Vfs::rename_noreplace` (kernel/src/fs/vfs.rs) shares
a private `rename_inner(from, to, noreplace)` with `Vfs::rename`; in the same-mount
branch the destination-existence check (`mp_to.fs.stat(rel_to)` → EEXIST if
present) executes under the **same held `VFS.lock()`** as the underlying
`mp_to.fs.rename`, so there is no TOCTOU window — no concurrent creator can slip a
file into the destination between the check and the rename. The Linux-ABI
`rename_common` (kernel/src/syscall/linux.rs) now calls `Vfs::rename_noreplace`
when the `RENAME_NOREPLACE` flag is set instead of doing a separate
`Vfs::stat`-then-`Vfs::rename` pre-check. The cross-mount copy+delete convenience
path (which Linux rejects outright with EXDEV) keeps a documented best-effort
destination pre-check, since multiple lock acquisitions make it inherently
non-atomic. Covered by the existing `syscall::linux::self_test` rename round-trip
(EEXIST onto an existing destination through `renameat2`) plus a new VFS-level
assertion that `rename_noreplace` onto a *free* destination succeeds and moves
src→dst.

**Progress (2026-06-14): `RENAME_EXCHANGE`.** Gap (c)'s exchange half is resolved
for filesystems that implement it. New `FileSystem::rename_exchange` trait method
(default `NotSupported`) with a real memfs implementation (atomically detaches
both entries and re-attaches them swapped, all-or-nothing with rollback if the
second operand is missing; self-exchange is a no-op; both operands must exist or
`NotFound`). `Vfs::rename_exchange` (kernel/src/fs/vfs.rs) resolves both paths,
checks tags/writability/intercept, and delegates the swap to the FS under the held
`VFS.lock()` — atomic w.r.t. the FS — requiring the **same mount** (cross-mount
exchange → `NotSupported`, since no atomic cross-FS swap exists). The Linux-ABI
`sys_renameat2` now routes `RENAME_EXCHANGE` to a new `rename_exchange_common`
(kernel/src/syscall/linux.rs) instead of the old blanket gate-4 `EINVAL`. The
mutual-exclusion gates (EXCHANGE+NOREPLACE/WHITEOUT → EINVAL) and the WHITEOUT
CAP/unsupported gates are unchanged. Covered by the post-`/tmp`
`self_test_rename_noreplace` (now also asserts an EXCHANGE swap of two existing
files' contents and a missing-operand `ENOENT` that leaves the survivor intact),
verified at boot.

**Progress (2026-06-14): cross-mount `RENAME_EXCHANGE` now returns `EXDEV`, not
`EINVAL`.** Previously a filesystem lacking exchange support *and* a cross-mount
request both surfaced as `EINVAL`, where Linux uses `EXDEV` specifically for the
cross-mount case. Added a `KernelError::CrossDevice` variant (code `-512`, in the
FS range) mapping to `EXDEV` in `linux_errno_for`/`kernel_error_from_code`;
`Vfs::rename_exchange`'s cross-mount branch now returns `CrossDevice` (FS-lacking-
support still returns `NotSupported` → `EINVAL`), so `rename_exchange_common`'s
generic `Err(e) => linux_errno_for(e)` arm yields `EXDEV` for cross-mount. The
`self_test_rename_noreplace` boot test gained case (6): with the boot-test's
writable memfs root + memfs `/tmp` (two distinct mounts), it asserts
`Vfs::rename_exchange` across them returns `CrossDevice` and that `renameat2`
maps it to `-EXDEV` (skips cleanly if the root is read-only in another config).

**Remaining fix:** the items left are: (a) switch watch identity to inode if/when
stable inode numbers are available; and (c-whiteout) `RENAME_WHITEOUT` support
(currently `EINVAL`).

### TD16. epoll fd readiness not reported when an epoll is nested in poll/select/epoll — RESOLVED 2026-06-14

**Where:** `kernel/src/ipc/epoll.rs` + the `HandleKind::Epoll` arm of
`poll_revents_from_entry` in `kernel/src/syscall/linux.rs`.

**What it was:** an epoll fd is itself pollable on Linux (it reports `EPOLLIN`
when any monitored fd is ready), allowing epoll fds to be nested inside another
epoll/poll/select. The `HandleKind::Epoll` arm of `poll_revents_from_entry`
returned 0 (never-ready), so nested-epoll readiness was NOT reported. `epoll_wait`
over directly-monitored fds always worked fully; only the nested case was wrong.

**Resolved (2026-06-14):** added `epoll_instance_ready(pid, handle, depth)` next
to `poll_revents_from_entry`. The Epoll arm now, given the threaded `owner_pid`,
resolves the epoll's `interest_list` against that process's fd table and reports
`POLLIN|POLLRDNORM` if any member is ready. Non-epoll members are evaluated by
`poll_revents_from_entry` (which never recurses back, as only the epoll arm calls
the helper); nested-epoll members recurse into `epoll_instance_ready` with
`depth + 1`, bounded by `EP_MAX_NESTS = 4` (mirrors `fs/eventpoll.c`) so a cyclic
or pathologically-deep nest can never blow the kernel stack. Without an
`owner_pid` (kernel/self-test context) the arm still reports not-ready rather
than consult an unrelated process's fd table. Boot self-test added in
`syscall::linux::self_test` ("nested-epoll readiness (TD16) OK"): a throwaway
process with a pipe → inner epoll E1 (watches pipe read) → outer epoll E0
(watches E1), asserting both E1 and the nested E0 are not-ready on an empty pipe,
both ready after a write, and not-ready when evaluated with `owner_pid = None`.

### TD15. timerfd `TFD_TIMER_CANCEL_ON_SET` is a silent no-op — RESOLVED 2026-06-14

**Where:** `kernel/src/timekeeping.rs` (generation counter), `kernel/src/ipc/timerfd.rs`
(stamp/check/wake), `kernel/src/syscall/linux.rs` (`sys_timerfd_settime`,
`dispatch_timerfd_read`), `kernel/src/syscall/handlers.rs` (`sys_clock_settime`,
`sys_clock_adjtime`).

**What it was:** `timerfd_settime` accepted the `TFD_TIMER_CANCEL_ON_SET` flag
(bit 1) without error, but the cancel-on-clock-step behavior was NOT implemented.
On Linux, a `CLOCK_REALTIME` timerfd armed with an absolute expiry and this flag is
"cancelled" (read returns `ECANCELED`, poll reports `POLLIN` readiness — *not*
`POLLERR`, contrary to the original note here) if the system realtime clock is
discontinuously changed (settimeofday/clock_settime/NTP step).

**Fix (implemented):** `timekeeping` now keeps a `REALTIME_GENERATION` counter,
bumped on every discontinuous realtime-clock step (`set_realtime`,
`adjust_realtime`); a smooth TSC advance does NOT bump it. `sys_timerfd_settime`
honours `TFD_TIMER_CANCEL_ON_SET` only for an absolute `CLOCK_REALTIME` timer,
snapshotting the generation into the timerfd at arm time (`armed_gen`). On read,
`take_cancellation` / `BlockingRead::Cancelled` return `ECANCELED` once per step
(resyncing `armed_gen`); on poll, `is_readable` reports readiness while the
generation is stale (level-triggered, no explicit poll wake needed). A blocked
reader is woken promptly by `clock_was_set()`, called from the `clock_settime` /
`clock_adjtime` handlers after the step. Boot self-test added to
`timerfd::self_test` ("TFD_TIMER_CANCEL_ON_SET (TD15): OK"): arms an absolute
`CLOCK_REALTIME` cancel-on-set timer far in the future, steps the clock via
`adjust_realtime(0)` (bumps the generation without moving the clock value),
asserts the timer becomes readable / `take_cancellation` returns true exactly
once, and that a re-armed timer *without* the flag is unaffected by a step.

### TD14. Per-process CPU-time / fault / ctxsw accounting — RESOLVED 2026-06-13 (time + page-fault + context-switch counters all done)

**Where:** `kernel/src/syscall/linux.rs` `sys_getrusage` and `sys_times`;
`kernel/src/sched/task.rs` (`Task::user_ticks`/`sys_ticks`, `tick_burst(from_user)`);
`kernel/src/sched/mod.rs` (`timer_tick(from_user)`, `cpu_ticks(tid)`, `TaskInfo`);
`kernel/src/proc/thread.rs` (`process_cpu_ticks(pid)`, `process_fault_counts(pid)`,
`on_thread_exit`); `kernel/src/proc/pcb.rs` (`Process::{acct_,child_}{user,sys}_ticks`
and `{acct_,child_}{min,maj}_flt`, `ThreadExitAccounting`, `remove_thread`,
`try_reap`/`try_reap_any`, `process_acct_ticks`/`process_child_ticks`,
`process_acct_faults`/`process_child_faults`); `kernel/src/sched/mod.rs`
(`account_fault`/`fault_counts`, `ctxsw_counts`, `SwitchKind` threaded through
`schedule_inner`); `kernel/src/idt.rs` (`account_fault` calls in
`handle_page_fault`); `kernel/src/apic.rs` (CPL sampling in `handle_timer_irq`);
`kernel/src/fs/procfs.rs` (`build_pid_stat`, `build_pid_status` ctxsw lines).

**Resolved — base (2026-06-13):** Linux-style tick-sampling CPU-time
accounting. On every timer IRQ, `handle_timer_irq` reads the interrupted frame's
CPL (`(frame.cs & 0x3) == 0x3` ⇒ ring-3) and passes `from_user` down through
`sched::timer_tick` → `Task::tick_burst`, which charges the whole tick to
`user_ticks` or `sys_ticks` (O(1), zero syscall-fastpath cost — Linux's default
non-NO_HZ_FULL model). `sched::cpu_ticks(tid)` exposes the per-thread split.

**Resolved — exited-thread fold + children-time (2026-06-13):** added a
per-process CPU-time accumulator to the PCB. When a thread exits,
`on_thread_exit` captures its `(user, sys)` ticks (while the Task is still
alive in the scheduler) and `remove_thread` folds them into
`Process::acct_user_ticks`/`acct_sys_ticks`. `process_cpu_ticks` now returns
`accumulator + Σ(live thread ticks)`, so it is exact for multi-threaded
processes that have already reaped worker threads — not just single-threaded
ones. For children time, `try_reap`/`try_reap_any` credit the parent's
`child_user_ticks`/`child_sys_ticks` with the reaped child's total CPU time
plus the child's own children-time (POSIX cutime/cstime carry-up, mirroring
Linux `wait_task_zombie` → `signal->cutime`/`cstime`). Both reset to 0 on fork.

Wired into:
- `getrusage(RUSAGE_SELF)` → process roll-up (live + exited threads);
  `getrusage(RUSAGE_THREAD)` → current thread; `getrusage(RUSAGE_CHILDREN)` →
  children accumulator. `ru_utime`/`ru_stime` populated (ticks×10ms → timeval).
- `times(2)` `tms_utime`/`tms_stime` and `tms_cutime`/`tms_cstime`
  (USER_HZ==TICK_RATE_HZ==100, so tick counts map directly to clock_t).
- `/proc/<pid>/stat` fields 14/15 (utime/stime) and 16/17 (cutime/cstime).

Self-test: `pcb::test_cpu_time_accounting` exercises the exited-thread fold,
`process_cpu_ticks` after all threads exit, and the parent←child←grandchild
children-time carry-up (asserts parent sees `(5+2, 3+1)`). Boot-test PASSED.

**Resolved — page-fault counters (2026-06-13):** added per-task `min_flt`/`maj_flt`
to `Task` (sched/task.rs) charged by `sched::account_fault(tid, major)` from the
three user-fault resolution points in `idt.rs::handle_page_fault` — swap-in ⇒
major (required I/O); demand-page (CoW/demand-zero) and stack growth ⇒ minor.
Mirroring the CPU-time path, the PCB gained `acct_min_flt`/`acct_maj_flt`
(exited-thread fold) and `child_min_flt`/`child_maj_flt` (reaped-children
carry-up). `remove_thread`'s signature was refactored from positional tick args
to a `ThreadExitAccounting { user_ticks, sys_ticks, min_flt, maj_flt }` struct
(the proper fix vs. a 6-arg signature). `proc::thread::process_fault_counts(pid)`
sums live + exited; `pcb::process_child_faults(pid)` reports the children
accumulator. Wired into `getrusage` `ru_minflt`(off 64)/`ru_majflt`(off 72) for
SELF/THREAD/CHILDREN, and `/proc/<pid>/stat` fields 10/11/12/13
(minflt/cminflt/majflt/cmajflt). `test_cpu_time_accounting` extended to assert
the fault fold (grandchild `(3,1)`), child children-faults `(3,1)`, and parent
children-faults `(4+3, 2+1) = (7,3)`. Boot-test PASSED.

**Resolved — context-switch counters (2026-06-13):** added per-task
`nvcsw`/`nivcsw` to `Task`, charged at the scheduler switch point. A
`SwitchKind` enum (`Voluntary`/`Involuntary`/`Uncounted`) is threaded into
`schedule_inner` from its five call sites (`yield_now`/`block_current`/
self-`suspend` ⇒ voluntary; `preempt` ⇒ involuntary; `task_exit` ⇒ uncounted)
and the outgoing task's counter is bumped under the SCHED lock at the actual
switch (where `next_id != current_id`). The PCB gained
`acct_nvcsw`/`acct_nivcsw` (exited-thread fold) and `child_nvcsw`/`child_nivcsw`
(reaped-children carry-up); `ThreadExitAccounting` carries the two fields too.
`proc::thread::process_ctxsw_counts(pid)` sums live + exited;
`pcb::process_child_ctxsw(pid)` reports the children accumulator. Wired into
`getrusage` `ru_nvcsw`(off 128)/`ru_nivcsw`(off 136) for SELF/THREAD/CHILDREN,
and `/proc/<pid>/status` `voluntary_ctxt_switches`/`nonvoluntary_ctxt_switches`
(previously stubbed as `0`/`schedule_count`). `test_cpu_time_accounting`
extended to assert the ctxsw fold (grandchild `(6,4)`), child children-ctxsw
`(6,4)`, and parent children-ctxsw `(7+6, 5+4) = (13,9)`. Boot-test PASSED.

**TD14 is now fully resolved** — all `getrusage` time/fault/ctxsw fields, `times`,
and the `/proc/<pid>/stat` + `/proc/<pid>/status` accounting surfaces are sourced
from real per-task counters rolled up per process with children carry-up. The
only rusage fields left at 0 are ones Linux also commonly leaves 0 (`ru_ixrss`,
`ru_idrss`, `ru_isrss`, `ru_nswap`, `ru_msgsnd`/`msgrcv`, `ru_nsignals`,
`ru_inblock`/`oublock`), which would require swap-RSS integral / signal-IPC
accounting not yet modelled.

### TD13. A few Linux-compat-flavored fields live in the native PCB — WATCH 2026-06-13

**Where:** `kernel/src/proc/pcb.rs` — job-control stop state
(`ProcessState::Stopped`/stop-signal tracking) and the `PR_SET_PDEATHSIG`
parent-death-signal storage (`get`/`set` around lines 2282–2290; field noted
"not wired because we don't yet have user-signal infrastructure").

**What it is:** the native process control block carries a small amount of
state whose *origin* is Linux/POSIX semantics (job-control stop/continue and
`prctl(PR_SET_PDEATHSIG)`). Per design-decisions.md §4 and §12, Linux-ABI
constructs should stay confined to the compat layer / Linux-ABI PCB state and
not accrete in the native PCB.

**Why it's not a live bug:** stop/continue is arguably a general
process-lifecycle notion (not strictly Linux), and `PR_SET_PDEATHSIG` storage
is inert (delivery is unwired). Nothing native consumes these as signals;
native process control remains IPC-based and faults remain SEH-style
exceptions. So the native ABI is not actually leaking *behavior* today.

**Proper fix (when the boundary is next touched):** move the pdeathsig value
(and any other purely-Linux fields) into the Linux-ABI PCB side-state (next to
`KernelFdTable`/the saved auxv), keyed by pid, so the native PCB carries only
constructs that would exist if Linux had never existed. Keep `ProcessState`
lifecycle states that are genuinely ABI-neutral. The trigger to do this is the
Linux compat ELF loader / signal-infrastructure work landing — co-locate all
Linux-ABI per-process state there in one pass rather than piecemeal.

### TD12. DRM event `read(2)` returns EAGAIN instead of blocking when empty — DEBT 2026-06-13

**Where:** `dispatch_drm_card_read` in `kernel/src/syscall/linux.rs`.

**What it is:** `read(2)` on a `/dev/dri/cardN` fd drains queued KMS events
(flip-complete records from `PAGE_FLIP` with `DRM_MODE_PAGE_FLIP_EVENT`).
When the event queue is empty it returns `EAGAIN` unconditionally — it does
not honour a *blocking* fd by parking the caller until an event arrives
(unlike, e.g., the signalfd read path, which has a real wait queue).

**Why it's not a live bug today:** our DRM backends retire page flips
**synchronously** inside `DrmDevice::page_flip`, so a flip-complete event is
queued *before* the `PAGE_FLIP` ioctl returns. A client following the normal
pattern (submit flip with the EVENT flag, `poll(2)` the fd, then `read(2)`)
always finds the event already queued; `poll` reports `POLLIN` immediately
and the read succeeds. The empty-read path is only reachable by a client
that reads without having submitted a flip — a client bug — and returning
EAGAIN there prevents a kernel hang rather than causing one.

**Proper fix (deferred until a backend retires flips asynchronously):** add a
per-client DRM event wait queue (mirroring the signalfd waiter pattern:
`register` + re-check + `block_current`, woken by `queue_event`), and have a
blocking read park on it instead of returning EAGAIN. Only worth doing once
a real vblank/async-flip source exists; under synchronous retirement it is
dead code.

### TD11. DRM dumb-buffer mmap not ref-tracked across `fork()` — DEBT 2026-06-13

**Where:** `drm_mmap_dumb` in `kernel/src/syscall/linux.rs` (the
`HandleKind::DrmCard` mmap interception in `sys_mmap`), in concert with
the refcounted `mm/frame.rs::free_frame` and the process-exit teardown
in `mm/page_table.rs::clear_user_address_space`.

**What it is:** The DRM Linux-ABI shim's MAP_DUMB path maps a dumb
buffer's GEM frames into the calling process by `frame::ref_inc`-ing
each frame before `map_frame`, so process-exit teardown's refcounted
`free_frame` merely balances the extra ref rather than double-freeing
the buffer (the GEM object retains its own ref until `gem_destroy`).
This is correct for a single process. It is NOT correct under a future
deep-copying `fork()`: a child that inherits the user PTEs for a dumb
mmap does not get a second `ref_inc`, so if fork ever gains general
per-page CoW of arbitrary user VMAs, a dumb mmap inherited by a child
and torn down on both sides could mis-count the frame refcount.

**Why it's not a live bug today:** our `fork()` does not deep-copy
arbitrary user mappings (see todo.txt Judgment Calls, fork(), 2026-05-31),
and graphics clients are single-process and do not fork while holding a
live framebuffer mmap. The gap is unreachable in practice.

**Proper fix (deferred until fork does general user-VMA copying):**
teach the fork path to recognise DRM-dumb-backed VMAs (or, more
generally, externally-refcounted frames) and `ref_inc` each frame per
child mapping, so every address space that maps a frame holds exactly
one ref and teardown stays balanced. Also recorded in todo.txt under
Judgment Calls.

### TD10. ALSA PCM shim does not implement the STATUS ioctl — DEBT 2026-06-13 (narrowed 2026-06-13)

**Update (commit 4b):** SYNC_PTR and READI_FRAMES are now implemented.
`alsa_pcm_ioctl` (`kernel/src/syscall/linux.rs`) stores `boundary` /
`avail_min` from SW_PARAMS, computes `appl_ptr` (= frames submitted) and
`hw_ptr` (= `appl_ptr − mixer-buffered frames`) reduced modulo the
boundary, and answers `SNDRV_PCM_IOCTL_SYNC_PTR` with a byte-exact
`snd_pcm_sync_ptr` (the status/control pages sit in 64-byte unions, so the
payload size is independent of the timestamp ABI). `READI_FRAMES` returns
zeroed capture frames. Both are covered by the
`ipc::alsa_pcm::self_test()` boot self-test (SYNC_PTR snapshot appl=2/hw=0,
appl_ptr/avail_min push-adopt, capture silence read).

**What still remains:** `alsa_pcm_ioctl` returns **ENOTTY** for
`SNDRV_PCM_IOCTL_STATUS` / `STATUS_EXT`.

**Why STATUS is still deferred:** unlike SYNC_PTR, the `snd_pcm_status`
payload embeds bare `struct timespec`s directly (not inside a padded
union), so its `sizeof` — and therefore the ioctl request number — depends
on the time64-vs-legacy-timespec ABI (the ambiguity flagged in the
commit-2 note at the top of `todo.txt`). Pinning that layout down is a
self-contained follow-up. STATUS is also only a convenience overlay: a
conforming ALSA-lib client learns `hw_ptr`/`appl_ptr` from SYNC_PTR (now
handled), so STATUS-on-ENOTTY does not block the playback hot path.

**Empirical confirmation of the fork (2026-06-14):** the upstream
`struct snd_pcm_status` declares its trailing pad as
`unsigned char reserved[64 - 5*sizeof(struct timespec) - 5*sizeof(int)]`
(older kernels: `reserved[52 - 4*sizeof(struct timespec)]`). With a
**16-byte** 64-bit `struct timespec` that pad size goes **negative**,
which cannot compile — proof that the mainline kernel never uses a single
struct with a 64-bit timespec here. Instead it maintains **two distinct
ABI structs**: a legacy `snd_pcm_status` built on a 32-bit
`old_timespec32` (used by the `SNDRV_PCM_IOCTL_STATUS`/`STATUS_EXT`
request numbers compiled for a 32-bit timespec) and a separate
`snd_pcm_status` / time64 path (`__SNDRV_PCM_IOCTL_STATUS_EXT64` etc.)
built on `__kernel_timespec`. The two carry **different `_IOR` request
numbers** because their `sizeof` differs. Consequently we cannot just
"pin the timespec layout" — implementing STATUS means deciding *which*
alsa-lib variant our userspace targets and answering the matching request
number(s). Until that target is fixed, emitting one guessed number risks
silently mismatching the client's other variant. This is the concrete
reason STATUS stays deferred rather than being a quick add.

**Impact:** low. SYNC_PTR (the per-period pointer exchange ALSA-lib's
kernel plugin actually relies on) works; only the `snd_pcm_status()`
convenience query falls back to ENOTTY.

**Proper fix:** add byte-exact `snd_pcm_status` (resolving the timespec
layout against our 64-bit `struct timespec`), define
`SNDRV_PCM_IOCTL_STATUS` / `STATUS_EXT` from its `size_of`, fill it from
the same `sync_position` snapshot plus the trigger/reference timestamps
once a monotonic audio clock exists, and replace the ENOTTY arm.

**Related limitations (not debt, intentional first-cut scope):** the shim
advertises only `RW_INTERLEAVED` access (mmap-based clients unsupported)
and only the mixer's native 48 kHz / S16_LE / stereo format (non-native
configs are rejected by HW_PARAMS rather than resampled/converted).
Resampling + format conversion + an mmap transfer path are future work.

### TD9. Linux program interpreter (ld.so) + PIE executable loaded at a fixed base — no ASLR — RESOLVED 2026-06-14

**Resolution (PIE-executable base, 2026-06-14):** the main `ET_DYN`/PIE
executable base is now randomised too. A new `choose_exec_load_bias(is_pie)`
helper (`kernel/src/proc/spawn.rs`) returns `0` for `ET_EXEC` and, for PIE,
an ASLR base ≥ `LINUX_PIE_BASE` drawn via `apply_aslr_base(LINUX_PIE_BASE,
rng::next_bounded(PIE_ASLR_SPAN_PAGES))` (28 bits of entropy, 16 KiB-page
units, falling back to the fixed floor before the CSPRNG is seeded). It is
computed once per spawn/exec at the two `exec_load_bias` sites
(`spawn_process` + `exec_process`) and already threads uniformly through
`load_segments_with_bias`, the biased entry point, and the SysV stack
builder's `AT_ENTRY`/`AT_PHDR`, so the whole image relocates consistently.
The highest PIE base (`≈0x5955_5555_0000`) leaves ~22 TiB below the
interpreter floor (`0x7000_0000_0000`) for the image + brk growth, and the
PIE floor sits far above the mmap window (`0x60_0000_0000`), so no
collision is possible. `sys_brk` is now a real demand-paged heap (see the
"Linux brk(2) heap" resolution below): a PIE image's heap grows from its
page-aligned image end up to a ceiling of `LINUX_INTERP_BASE`, i.e. into
that 22 TiB headroom, and the grow path's VMA-overlap check is a second
guard against colliding with the interpreter or mmap window. Covered by
`spawn::self_test`'s
`test_pie_aslr_window` (alignment + ≥1 TiB interpreter-floor headroom).
Both halves of TD9 are now done; entropy/always-on policy is in
design-decisions.md #20.

**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base; verified loading at e.g. 0x701e77808000, not the fixed
0x700000000000). The entropy-bits choice is recorded in
design-decisions.md.

---



**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base). The entropy-bits choice is recorded in
design-decisions.md.

**What remains (PIE-executable base — still DEBT):** the position-independent
*main* executable is still loaded at the fixed `LINUX_PIE_BASE =
0x5555_5555_4000`. Randomising it is more delicate than the interpreter
because the brk heap grows immediately above the PIE image, so the PIE
ASLR window must be chosen to leave room for brk growth without colliding
with the mmap window below or the interpreter window above. Deferred as a
separate follow-up. Original debt write-up follows.

---



**What:** The Linux dynamic-linker load path (`load_interpreter` in
`kernel/src/proc/spawn.rs`) maps the program interpreter (ld.so) at a
**fixed** virtual base, `LINUX_INTERP_BASE = 0x0000_7000_0000_0000`,
every time.  Real Linux randomises the interpreter base (and the mmap
region generally) via ASLR.  The executable itself is also loaded at its
fixed link-time vaddr (PIE executables are not yet re-based either).

**Where:** `kernel/src/proc/spawn.rs` — the `LINUX_INTERP_BASE` constant
and `load_interpreter()`.  AT_BASE is reported correctly from whatever
base is chosen, so making this random is a localised change.

**Why it's debt, not a bug:** ASLR is a security hardening measure, not a
correctness requirement — ld.so relocates itself to wherever it is placed
using the base it is told (AT_BASE) and its own dynamic relocations.  A
fixed base is fully functional; it just removes the address-space
randomisation defence against exploitation.

**Proper fix:** Once a userspace mmap-region allocator / ASLR policy
exists, draw the interpreter base (and PIE executable base) from it with
per-exec randomisation instead of the fixed constant.  Keep the AT_BASE
plumbing as-is — it already carries whatever base is chosen.

**Update 2026-06-14:** the dependency is now in place — a per-process
VMA-aware mmap gap allocator (`pcb::reserve_unmapped_area` →
`mm::vma::find_gap`, fronted by `handlers::alloc_user_mmap_reserve`) now
serves the general user mmap window with freed-gap reuse and atomic
find+insert.  ld.so's general-region maps already flow through it; what
remains for TD9 is purely the *randomisation policy*: pick a randomised
base for the interpreter/PIE load instead of the fixed `LINUX_INTERP_BASE`
constant.  Note the interpreter is loaded at `0x7000_…`, disjoint from the
mmap window `0x0060_…`, so ASLR for it will need its own randomised
placement (or be folded into the mmap region) rather than just calling the
new allocator.

**Related limitation (not debt, just unimplemented):** end-to-end
interpreter *execution* is untested because no real glibc/musl ld.so is
on the filesystem yet.  The load mechanism (base selection, biased
segment mapping via `load_segments_with_bias`, AT_BASE/AT_ENTRY auxv) is
unit-tested via `spawn::test_load_interpreter_fallbacks` (static-ELF and
absent-interpreter `Ok(None)` fallbacks).  See `todo.txt` "Linux
dynamic-linker (ld.so) load path".

### TD25. `sys_brk` was a no-op stub (claimed grow succeeded but mapped nothing → latent SIGSEGV) — RESOLVED 2026-06-14

**What it was:** `sys_brk` (`kernel/src/syscall/linux.rs`) simply echoed
`args.arg0` back to the caller — claiming the requested program break was
granted while mapping **no** memory.  Any real glibc/musl program whose
`malloc` used the brk fast path (it does for small allocations until the
main arena is exhausted) would write into the "granted" heap and take an
immediate page fault on unmapped memory → ring-3 SIGSEGV.  The stub only
happened to be harmless because no glibc binary runs end-to-end yet; it
was a live trap waiting for the first one.

**Resolution (2026-06-14):** Implemented a real demand-paged brk heap.

- **PCB state:** added `brk_start` (heap floor) and `brk_current` (program
  break) to `Process` (`kernel/src/proc/pcb.rs`), inherited verbatim across
  `fork` (CoW heap clone) and reset on `exec` — recomputed from the new
  image's page-aligned end for Linux images (`elf::image_end`), cleared to
  `0` for native images (no Linux brk heap).  Accessors `set_brk_region` /
  `get_brk` / `set_brk_current`.
- **VMA:** new `VmaKind::Brk` (`kernel/src/mm/vma.rs`) — faults exactly like
  `Anonymous` (demand-paged, zero-filled); exists so `/proc/<pid>/maps`
  labels it `[heap]` and `sys_brk` can find/resize its own VMA.  The heap is
  a single `[brk_start, round_up(brk_current))` VMA.
- **sys_brk semantics (Linux-faithful):** `brk(0)` / `addr < brk_start`
  query (return unchanged break); grow maps the new span by replacing the
  heap VMA (demand-paged) and charges `RLIMIT_AS` for the full added virtual
  span up-front (committed-by-default — no overcommit); shrink unmaps+frees
  faulted frames via `unmap_user_range` and refunds the charge; same-top-
  frame moves touch nothing.  On **any** failure (RLIMIT_DATA, RLIMIT_AS,
  VMA collision, OOM, overflow) it returns the *unchanged* break — exactly
  what glibc's `__sbrk` expects so it falls back to mmap and reports ENOMEM
  itself.
- **Heap ceiling (image-dependent):** `brk_ceiling(brk_start)` returns
  `USER_MMAP_BASE` for a low-loaded ET_EXEC (`brk_start < USER_MMAP_BASE`)
  and `LINUX_INTERP_BASE` for a high-loaded PIE (`brk_start >=
  USER_MMAP_BASE`), so the heap can never grow into the mmap window, the
  interpreter window, or the stack.  The VMA-overlap check is a second
  guard.

**Tests:** `syscall::linux::self_test_brk_logic` (pure: `brk_round_up`
boundary/overflow cases + `brk_ceiling` ET_EXEC/PIE/ordering) and the
ring-3 end-to-end `proc::spawn::self_test_linux_brk` (a real Linux-ABI
process queries its break, grows 32 KiB, writes a sentinel into the
*second* heap frame, reads it back, exits with that byte — exit `0x6D`
proves the grow + demand-paging of multiple frames works; both verified in
the boot-test serial log).

**Update (2026-06-14): `arch_randomize_brk` gap now implemented.** The heap
floor is the page-aligned image end shifted up by a random gap
(`spawn::choose_brk_start`), mirroring Linux x86_64's `arch_randomize_brk`
with 13 bits of entropy (matching Linux's position count per the
entropy-is-the-metric policy of design-decisions #20; 128 MiB max gap at our
16 KiB pages). Always-on when the CSPRNG is seeded, no-gap fallback before
seeding, "no heap" (`image_end == 0`) preserved. Covered by
`test_brk_aslr_gap` and exercised end-to-end by `self_test_linux_brk`. No
remaining gaps on the brk heap.

### TD8. `membarrier` PRIVATE_EXPEDITED issue without prior REGISTER returns 0 where Linux returns `-EPERM` — RESOLVED 2026-06-14

**What it was:** `sys_membarrier()` (`kernel/src/syscall/linux.rs`) accepted every
issue command (`MEMBARRIER_CMD_PRIVATE_EXPEDITED`,
`…_PRIVATE_EXPEDITED_SYNC_CORE`, `…_PRIVATE_EXPEDITED_RSEQ`) and returned 0
unconditionally. Linux v6.6's `membarrier_private_expedited()` first checks the
issuing mm's `membarrier_state` and returns **`-EPERM`** when the matching
`MEMBARRIER_STATE_*_READY` bit is not set — i.e. when the process never issued
the corresponding `…_REGISTER_*` command. That EPERM check runs **before** the
single-CPU `return 0` shortcut, so even on our uniprocessor an unregistered
`PRIVATE_EXPEDITED` issue should be `-EPERM`, not 0. Symmetrically, our
`…_REGISTER_*` commands were no-ops and `MEMBARRIER_CMD_GET_REGISTRATIONS`
always reported 0. (Note: `GLOBAL_EXPEDITED` *issue* is NOT gated on Linux —
only the three `PRIVATE_EXPEDITED*` issues are; the original note overstated
this.)

**Fix (implemented):** added a per-mm `membarrier_state: u32` READY bitmask to
`Process` (`kernel/src/proc/pcb.rs`), shared across the process's threads (so a
thread may register and a sibling issue), inherited verbatim across `fork`
(Linux's `dup_mm` memcpy) via `pcb::membarrier_register` / `membarrier_state`
accessors. `sys_membarrier` now resolves the issuing mm's state and routes
through the pure, unit-tested `membarrier_decide(cmd, state)`: `REGISTER_*` OR
in their READY bit; the three `PRIVATE_EXPEDITED*` issues return `-EPERM`
unless their bit is set; `GET_REGISTRATIONS` reports the registered-command
bitmask via `membarrier_registrations_mask`; `GLOBAL`/`GLOBAL_EXPEDITED` issue
need no registration. The boot self-test (`self_test_membarrier_registration`,
"membarrier per-mm registration gating (TD8): OK") exercises `membarrier_decide`
exhaustively and drives the per-mm READY-bit store (register/idempotency/
cross-command isolation/GET mask) through a throwaway `pcb::create` process —
solving the original "no owner mm at boot" testability blocker by testing the
pure helper and the pcb layer directly rather than through the syscall caller's
(absent) mm.

**Residual divergence — RESOLVED 2026-06-14:** Linux resets `membarrier_state`
to 0 on `execve` (`membarrier_exec_mmap`); we previously lacked an exec-time
PCB-reset hook (the same gap noted for `linux_dumpable`/`linux_keepcaps`/
`linux_thp_disable`), so a registration survived exec. Now fixed: added
`pcb::reset_linux_state_for_exec(pid)`, called from `spawn::exec_process` after
`reset_vmas_for_exec`, which clears (under one `PROCESS_TABLE` lock) exactly the
fields Linux unconditionally resets on every exec — `membarrier_state` → 0
(`exec_mmap`→`membarrier_exec_mmap`), `linux_dumpable` → 1 (`SUID_DUMP_USER`;
explicit `set_dumpable` in `begin_new_exec`), and the `linux_securebits`
`SECBIT_KEEP_CAPS` bit (bit 4 only — `cap_bprm_creds_from_file` clears it on
every exec, preserving the lock bit and every other securebit). That bit 4 is
now the **single source of truth** for `prctl(PR_SET_KEEPCAPS)` (see the
follow-up note below), so clearing it on exec resets keepcaps too. Fields Linux
preserves across a normal (non-privileged)
exec are left untouched: `linux_thp_disable` and `linux_memory_merge` (both
`MMF_INIT_MASK` mm-flags that the new mm inherits via
`mm->flags = current->mm->flags & MMF_INIT_MASK` — `begin_new_exec` has no
explicit THP/KSM override, so they survive exec), `linux_pdeathsig` (cleared
only on set-uid/caps exec, otherwise preserved per prctl(2)),
`linux_personality` (x86_64 `set_personality_64bit` only clears the unmodelled
`READ_IMPLIES_EXEC`; `ADDR_NO_RANDOMIZE` survives), `linux_no_new_privs`
(sticky), `linux_child_subreaper`, timer-slack. (An initial version of the hook
wrongly reset `linux_thp_disable`, repeating entry 98's mistaken "cleared on
execve" claim; corrected same session.) Self-test
`pcb::test_reset_linux_state_for_exec` asserts the cleared state (membarrier,
dumpable, keepcaps, securebits KEEP_CAPS bit with lock+other bits kept) and the
five preserved fields ("[proc]   exec Linux-state reset: OK"). The in-kernel
`membarrier` self-test
caller (no owner mm) keeps the "fence/0" behaviour by feeding `u32::MAX` to the
gating helper — there is no registration model for a kernel thread with no
sibling userspace threads.

**Follow-up — keepcaps/securebits single source of truth (2026-06-14):** the
exec-reset audit surfaced a real ABI incoherence: `prctl(PR_SET_KEEPCAPS)` was
backed by a standalone `linux_keepcaps` field while `SECBIT_KEEP_CAPS` lived in
`linux_securebits`, even though Linux stores both in the *same*
`cred->securebits` bit 4. `PR_SET_KEEPCAPS`/`PR_SET_SECUREBITS` wrote different
storage, so `PR_GET_KEEPCAPS` and `PR_GET_SECUREBITS` could disagree where Linux
keeps them identical. Fixed by removing the `linux_keepcaps` field and making
`pcb::get_keepcaps`/`set_keepcaps` thin views over `linux_securebits` bit 4
(set/clear only bit 4, leaving every other securebit intact). Also added the
missing Linux gate to the `PR_SET_KEEPCAPS` handler: once
`SECBIT_KEEP_CAPS_LOCKED` (bit 5) is engaged the flag is frozen and the call
returns `-EPERM` (`cap_task_prctl`, verified against torvalds/linux v6.6
`security/commoncap.c`). The gate is the pure helper
`keepcaps_change_allowed(securebits)` so it is unit-testable without a caller
PCB. Tests: `self_test_prctl_dispatch`'s keepcaps block now asserts get/set
coherence in both directions (keepcaps↔securebits bit 4) and the lock-gate
truth table; `pcb::test_reset_linux_state_for_exec` proves `set_keepcaps`
coherently drives bit 4 and the exec reset clears only it.

**Companion fix — PR_SET_SECUREBITS lock enforcement now unit-tested
(2026-06-14):** the same audit found the `PR_SET_SECUREBITS` lock-bit
enforcement (a set lock can't be cleared; a locked flag can't flip) was
inline in the handler and so its `-EPERM` path was unreachable from the
kernel-context boot self-test (no `caller_pid` PCB to seed locked bits) —
the test only covered value validation. Extracted the decision into the pure
`securebits_change_allowed(cur, new_val)` (mirrors `cap_task_prctl`) and added
a truth-table test to `self_test_prctl_dispatch` covering: no-locks→allowed,
new-lock→allowed, clear-set-lock→denied, flip-locked-flag (both
set→clear and clear→set)→denied, and locked-flag-kept-while-flipping-an-
unlocked-flag→allowed ("PR_SET_SECUREBITS lock-bit enforcement … : OK").

### TD7. `set_mempolicy_home_node` returns 0 where Linux returns `-ENOENT`/`-EOPNOTSUPP` — APPROXIMATION 2026-06-12

**What:** `sys_set_mempolicy_home_node()` (`kernel/src/syscall/linux.rs`)
returns 0 for any valid non-empty range. Linux v6.6 instead walks the VMAs
in `[start, end)` with `err` initialized to `-ENOENT`: it returns `-ENOENT`
when no VMA in the range carries an explicit `MPOL_BIND`/`MPOL_PREFERRED_MANY`
policy, and `-EOPNOTSUPP` for a VMA whose policy is some other mode. Only a
range that already has a bind/preferred-many policy yields 0.

**Why we diverge:** our `mbind` is a UMA no-op that does **not** store
per-VMA mempolicy, so the kernel cannot tell whether the caller previously
established a policy on the range. We pick 0 (the "policy was set, home node
applied" success outcome — the common real-world sequence where
`set_mempolicy_home_node` follows a successful `mbind(MPOL_BIND)`) over
`-ENOENT`. Returning `-ENOENT` would instead break that common path.

**Proper fix:** implement real per-VMA mempolicy storage so the VMA walk can
distinguish "no policy" (`-ENOENT`), "wrong policy" (`-EOPNOTSUPP`), and
"bind policy → apply home node" (0). Tracked as an open question
(`open-questions.md`) because the 0-vs-`-ENOENT` choice is a genuine
tradeoff. **Note:** batch 551 *did* fix the unambiguous part — the
`home_node` online check now runs before the len/end gates, matching v6.6.

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

### TD6. `move_pages` per-page node error stores `-EINVAL` where Linux stores `-ENODEV`/`-EACCES` — RESOLVED 2026-06-12 (batch 549)

**Resolution:** `sys_move_pages` now stores `-ENODEV` for any non-zero
target node, matching `do_pages_move`'s `err = -ENODEV` path (out-of-range
or `!node_state(node, N_MEMORY)`). On a single-node box every node but 0
lacks `N_MEMORY`, so `-ENODEV` is correct for all of them; the `-EACCES`
"valid node not in `task_nodes`" branch is unreachable when only node 0 has
memory. Batch-105 self-test Case 4 updated to expect `[0, -ENODEV, 0]`.
Original analysis retained below for reference.

---

**What:** `sys_move_pages` (`kernel/src/syscall/linux.rs`), in move mode
(`nodes != NULL`), writes `status[i] = -EINVAL` for any requested target
node other than 0 (we only have node 0). Linux's `do_pages_move`
(`mm/migrate.c`) instead validates each target node and stores a per-page
error via `store_status`: `-ENODEV` when the node is out of range or has no
memory (`!node_state(node, N_MEMORY)`), or `-EACCES` when the node is valid
but not in `task_nodes` (`!node_isset(node, task_nodes)`). On a single-node
box, target node 1 would be `-ENODEV` (node 1 has no `N_MEMORY`), not
`-EINVAL`.

**Divergence:** observable only in `status[i]` for a deliberately-bogus
target node; the syscall return code (0) is unaffected. Batch-105 self-test
Case 4 currently asserts `status == [0, -EINVAL, 0]`.

**Why deferred (not fixed in batch 548):** batch 548 fixed two
independently-verified divergences (missing pid→ESRCH lookup; invented
E2BIG cap) and intentionally did **not** guess at the per-page errno. The
exact `do_pages_move` node-validity path (range check → `N_MEMORY` →
`node_isset(task_nodes)` → `store_status`) needs its own verbatim v6.6
verification before changing the stored errno.

**Proper fix:** verify `do_pages_move`/`add_page_for_migration`/`store_status`
against v6.6, then store `-ENODEV` for out-of-range / no-memory nodes and
`-EACCES` for valid-but-disallowed nodes, and update Case 4's expectation.

### TD4. Monolithic `syscall::linux::self_test()` has an unbounded boot-stack frame — RESOLVED 2026-06-14

**Resolution (2026-06-14):** The split is complete. Every self-contained
validation block in `self_test()` is now wrapped in its own
`#[inline(never)]` nested helper (`fn self_test_NAME() -> KernelResult<()>`,
called via `?`), so each sub-frame is allocated and freed transiently around
its call and no single frame is the sum of all batches. The body went from one
monolithic ~1.4 MB frame to ~80 small per-block helpers. Three earlier helpers
that had grown to wrap multiple sibling blocks (`getrusage_sysinfo_times` = 5
blocks, `capget_capset` = 2, `sched_affinity` = 2) were peeled apart so each
block gets its own frame. A structural scan confirms **zero** bare top-level
blocks remain. The technique used throughout (Technique B): insert a 5-line
header — `self_test_NAME()?;` + `#[inline(never)] fn self_test_NAME() -> …
{ use crate::serial_println;` — immediately before the block's leading
comment, and a 2-line footer — `Ok(())` + `}` — immediately after the block's
closing brace; the block body is never reproduced or re-indented, so the wrap
is safe for arbitrarily large blocks. A non-inlined nested fn cannot capture
enclosing locals, which acts as a compile-time safety net against
mis-scoping. Every wrap was individually boot-tested (BOOT_OK) and committed.
This removes the F10 (`.bss`/`FPU_STRATEGY` silent-corruption) failure class
at its root rather than merely deferring it behind the boot-stack canary.

**Progress (2026-06-13):** Began the incremental `#[inline(never)]` split. The
two leading self-contained check groups were extracted into standalone
functions — `self_test_errno_mapping()` (errno round-trips + the `check_errno!`
macro, used nowhere else) and `self_test_native_translation()` (the
`linux_from_native` round-trips). Both are guaranteed behaviour-preserving:
their locals never escape the extracted region. `self_test()` now calls them
via `?`. This establishes the repeatable extraction pattern (cut a contiguous
region whose locals don't cross the boundary, lift to an `#[inline(never)] fn
… -> KernelResult<()>`, replace inline with a `?` call, build+boot-test).
Continue opportunistically: the safe cut points are regions that don't share a
reused local (e.g. the early checks share `args`/`r`, so a larger contiguous
run ending at the last use of those must be lifted as one unit). Remaining
work is the bulk of the ~40 k-line body.


**What:** `kernel/src/syscall/linux.rs::self_test()` is a single ~1.4 MB
function (~39 k lines, opens near line 35858, closes near line 75298) whose
body is one giant 4-space enclosing block. Each ABI-fidelity batch (536 and
counting) appends its own locals inside that block. In the unoptimized
debug build (`opt-level=0`, no LLVM stack-slot coloring), the compiler does
**not** reuse stack slots across the lexically-disjoint per-batch sub-blocks,
so the function's single frame is the *sum* of every batch's locals
(~480 KiB as of batch 536 and growing monotonically). It runs directly on
the guardless boot stack — this is exactly what caused F10 (silent
`.bss`/`FPU_STRATEGY` corruption when the frame overran the old 512 KiB
stack).

**Why it's debt, not a bug now:** F10's fix (2 MiB boot stack + 64 KiB
redzone canary) gives ~1000+ batches of runway and converts any future
overrun into a clean `FATAL: boot stack overflow detected` halt instead of
silent corruption. So the system is correct and self-diagnosing. But the
frame grows ~1 KiB/batch, so this only defers the wall; it does not remove
it.

**Proper fix:** split `self_test()` into many small `#[inline(never)]`
sub-functions (e.g. one per batch or per logical group, `fn self_test_b536()
-> Result<…>` …) called in sequence from a thin driver, so each sub-frame is
allocated and freed around its call and no single frame is large. This caps
the boot-stack frame regardless of batch count and is the real removal of
the F10 failure class.

**Why deferred:** the function is one giant 4-space block; a hand-split risks
silently mis-scoping locals shared across batch boundaries (a local defined
in an early batch and read in a later one would stop compiling, or worse,
shadow). Doing it safely means iterating in small chunks with a build after
each (~50 s/cycle), and the canary makes it non-urgent. **Trigger to do it
properly:** before the boot-stack usage (reported by the canary scan / a
future high-water mark print) crosses ~50 % of the 2 MiB stack, or
opportunistically when next touching the self-test scaffolding.

### TD3. Prefix-boundary subtree checks: audit every site for trailing-slash correctness — RESOLVED 2026-06-10

**What:** The "is `path` inside directory subtree `prefix`" check was
written inline at ~30 sites as
`path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')`
(sometimes with a leading `path == prefix ||`).  This idiom is **only
correct when `prefix` has no trailing slash**.  When `prefix` already
ends in `/` (e.g. a registration like `"/protected/"`), the
`get(prefix.len()) == Some(&b'/')` boundary check looks one byte past
the slash and therefore only matches *double-slash* paths
(`/protected//x`), so real children never match — the check silently
fails (open for deny handlers, or simply never fires for "missing file"
/ exclusion logic).

**RESOLUTION (2026-06-10):** Created a single canonical helper module
`kernel/src/fs/pathutil.rs` exposing `path_in_subtree(path, dir)` and
`path_strictly_under(path, dir)`.  Both normalise away an optional
trailing slash (`dir.strip_suffix('/')`) before the component-boundary
check, so they are correct whether or not the caller's prefix carries a
trailing slash.  Five `#[cfg(test)]` unit tests pin the contract
(basic boundary, trailing-slash equivalence, empty/root-matches-all,
strictly-under-excludes-self, strictly-under-root).  Every real subtree
check now routes through this helper; the footgun idiom is gone from the
fs subsystem.

**Confirmed-buggy (silent failures), now fixed via the helper:**
- `integrity.rs` baseline-paths filter (earlier commit `22a8098f`) —
  prefix carried a trailing slash; `verify_dir` never reported missing
  files.  Now also routed through `path_in_subtree` (removed the
  per-iteration `format!("{excl}/")` allocation in the exclude-dir scan).
- `intercept.rs` `pre_check` interceptor filter — prefixes registered
  with trailing slashes (`/protected/`) so every deny handler failed
  open.  `path_matches_prefix()` is now a thin `#[inline]` wrapper over
  `path_in_subtree` (kept for the descriptive call-site name + bug note).
- `findex.rs:304` `columns_for_dir` — built `prefix` *with* a trailing
  slash, so the old boundary check matched nothing and column discovery
  always returned empty.  Now routed through `path_strictly_under`.

**Routed through the helper for robustness (prefix-source could carry a
trailing slash; uniform now):** `undelete.rs` (scan filter), `search.rs`
(exclude prefixes), `queryable.rs` (root filter), `dedup.rs` (exclude
prefixes), `directio.rs` (`is_dio_path`), `index.rs` (exclude/remove/
is_watched ×3), `fswalk.rs` (`is_excluded`, both default + opts),
`fcomment.rs` (search/list/remove_under ×3), `changetrack.rs` (path +
old_path prefix filter), `fileversion.rs` (policy + max-size lookups).

**Verified correct, left as-is (slash-free prefixes by construction):**
`vfs.rs` (mount paths), `freeze.rs:264` (mountpoint), `atime.rs:163`
(mount_path), `overlay.rs:169` (already-normalised `is_under`),
`notify.rs` `path_matches` (distinct `strip_prefix` impl with
recursive/non-recursive semantics the helper does not model),
`apps/defrag/src/main.rs:659` (`/*` glob with the slash already stripped;
separate crate, cannot reach `fs::pathutil`).

Build clean; QEMU boot test green.

**Kernel-wide sweep (2026-06-10):** grepped all of `kernel/src` for the
`get(X.len()) == Some(&b'/')` idiom — the only matches are the six
`fs/` files already accounted for above (plus `pathutil.rs`, the helper
itself).  No sibling instances exist in `net`, `proc`, `ipc`, `mm`, or
any other subsystem, so the footgun is fully contained and closed.

### TD2. Clippy `clippy::all` deny-level errors not yet zeroed — RESOLVED 2026-06-10 (regressed + re-fixed 2026-06-14)

**REGRESSION RE-FIX (2026-06-14):** 7 deny-level `clippy::all` errors had crept
back in since the original resolution — `byte_char_slices` (`drm/edid.rs:604`,
`fs/compress.rs:1956`), `question_mark` (`fs/fswalk.rs:229`+`:354`,
`fs/hotkeys.rs:134`), and `for_kv_map` (`fs/history.rs:435`, `proc/pcb.rs:1618`).
All fixed mechanically (byte-string literals, `?` operator, `.values()` map
iteration); `cargo clippy -p kernel` is back to **0 deny-level errors**, build +
QEMU boot test green. Lesson: the deny-level gate is only green between sweeps —
it needs to actually run in CI to stay zeroed (no CI exists yet).

**Sweep extended to all default-members (2026-06-14):** since the kernel was
not the only crate that could regress, `cargo clippy` was run on the other two
default-member crates. `posix` had **1** deny-level regression —
`too_many_arguments` (8/7) at `epoll.rs:2150`, `translate_kernel_event`,
introduced when the `is_dir` param was added for IN_ISDIR (TD17). Fixed with a
justified `#[allow(clippy::too_many_arguments)]` (commit `8acddca0c`); the 8
params are the distinct fields of one kernel watch event and a struct would only
add indirection to a pure host-tested translator. `toolchain/stubs`
(`slateos-stubs`) was already clean. All three default-members now report **0
deny-level errors**; posix's 19972 host tests still pass.

**RESOLUTION (2026-06-10):** `cargo clippy -p kernel` now reports
**0 deny-level errors** (down from 451) and ~17,297 warn-level warnings.
The deny-level `clippy::all` gate is green and can be used as CI.  The
warn-level lints remain by design (see below).  Landed across several
reviewable batches: the 158 doc-formatting lints, the 167 machine-
applicable idiom fixes, the 181 doc-comment lints, and a final hand-
fixed batch of 77 (commit `15dc0168`) covering `manual_memcpy`,
`ptr_arg`, `inherent_to_string`→`Display`, `wrong_self_convention`,
`upper_case_acronyms`, `enum_variant_names`, `type_complexity`,
`if_same_then_else` (inspected — no real copy-paste bugs), and a tail of
singletons (`fn_to_numeric_cast`, `forget_non_drop`, `never_loop`,
`only_used_in_recursion`, `pointers_in_nomem_asm_block`,
`large_enum_variant`, etc.).  `cargo build` and the QEMU boot test pass.

The two warn-tier correctness audits (step 3 below) are also complete:

* **`cast_ptr_alignment` (107) — audited, safe, left as warn.**  Every
  site is in MMIO / DMA-ring / on-disk-format / wire-protocol code
  (virtio, xhci, hda, e1000, ahci, ext4 `ondisk`, smp, `mm/frame`,
  syscall device-register reads).  Alignment is guaranteed by the
  page-aligned DMA frame allocation or by naturally-aligned hardware
  registers; the lint fires only because it sees a bare `*mut u8`/`*const
  u8` base.  Representative samples verified (e.g. `virtio/queue.rs:168`
  casts a page-aligned frame + 16-byte descriptor stride to
  `*mut VirtqDesc`).  One outlier — `ext4/ondisk.rs:1017` — casts an
  align-1 stack `[u8; 1024]` to a struct pointer; technically UB but
  benign on x86_64 and confined to a boot self-test.  No production
  under-alignment.  Eventual cleanup is a per-site `// SAFETY:` +
  `#[allow]`, but the casts are correct as-is.

* **`large_stack_arrays` (7) — audited; 1 genuine fixed, rest are false
  positives.**  Five (`cgroup.rs`, `fs/vfs.rs`, `klog.rs`, `mm/rmap.rs`,
  `sched/priority_rr.rs`) are `const fn` constructors whose arrays are
  const-evaluated directly into `static`/rodata storage — never on the
  stack; the lint is conservative.  `ktrace.rs:461` was a genuine 512-
  entry self-test window on the stack → now heap-allocated via
  `alloc::vec!`.  `scfilter.rs` built a ~19 KiB `FilterTable` on the
  stack before `Box::new` (the prior comment's "heap" claim was defeated
  by the by-value temporary) → `new()` is now `const fn` materialized via
  a `const EMPTY` binding so the box copies from rodata.  (Fixes + doc in
  the follow-up commit.)  The 6 remaining warnings are all const-context
  arrays in static storage and carry no stack-overflow risk.

---

**Original report (for history):**

**Where:** kernel-wide.  Snapshot `cargo clippy -p kernel` (rust 1.95.0,
2026-06-10): **451 deny-level errors** and **17,320 warn-level
warnings**.

**What this is — and why the two tiers are treated differently.**
The workspace lint config (`Cargo.toml [workspace.lints.clippy]`) sets
`clippy::all = deny (priority -1)`, `clippy::pedantic = warn`, and the
five correctness-pressure lints (`unwrap_used`, `expect_used`, `panic`,
`indexing_slicing`, `arithmetic_side_effects`) = `warn`.  So:

* **Warn-level (17,320) — intentional by design, NOT a blocker.**
  Dominated by:
  - `arithmetic_side_effects` 7,511
  - `indexing_slicing` 5,711
  - `expect_used` 2,689
  - `unwrap_used` 1,034
  - `unnecessary_wraps` 156, `cast_ptr_alignment` 107, others < 25 each.

  These are the defensive-pressure lints CLAUDE.md deliberately set to
  `warn` rather than `deny` because they are pervasive in low-level
  kernel code (every `a + b`, every `slice[i]`, every page-table index)
  and forcing `checked_*`/`.get()` everywhere would bury real signal
  under mechanical noise.  They are advisory: the rule is "prefer `?`,
  `.get()`, `.checked_*` in new code and surgically harden hot/attacker-
  reachable paths," not "drive the count to zero."  **These are accepted
  by design and should NOT be mass-rewritten.**  Two sub-categories DO
  deserve a real audit pass and should be tracked as their own work:
  `cast_ptr_alignment` (107 — genuine UB risk if any cast actually
  under-aligns; most are MMIO/identity-mapped and provably fine but each
  should carry a `// SAFETY:`/`#[allow]` with justification) and
  `large_stack_arrays` (7 — kernel stacks are bounded; verify none blow
  the stack).

* **Deny-level (451 `clippy::all` errors) — these SHOULD be fixed**, per
  the project's own `all = deny` gate.  The good news: they are almost
  entirely **mechanical, machine-applicable idiom lints**, not logic
  bugs.  Top categories:
  - `doc_overindented_list_items` 137, `doc_lazy_continuation` 21
    (158 = doc-comment formatting — auto-fixable)
  - `unwrap_or_default` 21, `manual_strip` 15, `manual_slice_fill` 14,
    `vec_init_then_push` 13, `manual_memcpy` 10, `manual_clamp` 8,
    `assign_op_pattern` 8, `manual_div_ceil` 8, `slow_vector_
    initialization` 7, `while_let_loop` 6, `explicit_counter_loop` 5,
    `single_char_add_str` 5, `single_match` 5 … (all auto-fixable)
  - A small tail needs human judgment, not blind `--fix`:
    `type_complexity` 10 (extract type aliases), `duplicated_attributes`
    9 (a module-level `#![allow(dead_code)]` duplicating the parent
    `#[allow]` in `fs/mod.rs` — remove the inner one),
    `upper_case_acronyms` 9 and `enum_variant_names` 7 (renames — verify
    no public-API churn), `if_same_then_else` 7 (could be a real copy-
    paste bug — inspect each), `comparison_to_empty` 7.

**File distribution of the 451 errors** (primary span):
`syscall/linux.rs` 200, `kshell.rs` 39, `fs/bzip2.rs` 8,
`syscall/handlers.rs` 8, `sched/mod.rs` 6, `fs/contextmenu.rs` 5,
`fs/procfs.rs` 5, `fs/monitors.rs`/`fs/tags.rs`/`fs/taskbar.rs`/
`net/http.rs` 4 each, then a long tail of 1–3 across ~40 more files.
`linux.rs` alone is 44% of the total (it is the single largest source
file, ~28k lines, and accretes idiom lints fast).

**Why it's open rather than fixed-on-sight:** the count is large and
spread across ~50 files; the bulk is `cargo clippy --fix` territory but
that produces a sweeping multi-file diff that materially changes the
shape of hot syscall code (`linux.rs`), so it warrants being landed as
its own reviewable change(s) rather than smuggled into a feature commit.
Two deny-errors that were authored as part of the /proc work
(2026-06-10) were fixed immediately at their source:
`procfs.rs` `gen_pid_statm` doc list (`doc_overindented_list_items`) and
`pcb.rs` `set_exe_path` (`manual_contains` → `slice.contains`).

**Tooling caveat (verified 2026-06-10):** `cargo clippy --fix` does
**not work** in this environment — it recompiles, reports the count of
machine-applicable suggestions (e.g. "to apply 176 suggestions"), but
writes **zero** changes to disk.  Tried four ways:
`cargo clippy -p kernel --fix --allow-dirty`;
`… --bin kernel … --no-deps`;
`… -- --force-warn clippy::all` (to defeat the deny-as-error so the
verify-recompile would pass); and with the workspace
`clippy::all` level temporarily flipped to `warn` in `Cargo.toml`.
All four no-op'd (0 `.rs` files modified, ~4 min each).  The kernel
targets the built-in `x86_64-unknown-none` with no build-std, so this
is not a custom-target issue; it looks like `cargo fix`'s
write-back/verify phase failing silently on this Windows toolchain.
**Do not burn build cycles retrying `--fix` — remediation must be by
hand (or with a non-cargo rewrite tool).**

**Proper fix / remediation plan:**
1. Hand-fix the machine-applicable bulk in reviewable batches, grouped
   by lint family so each diff is easy to verify: start with the ~158
   doc-formatting lints (`doc_overindented_list_items`,
   `doc_lazy_continuation` — pure comment edits, zero risk), then the
   manual-idiom families (`unwrap_or_default`, `manual_strip`,
   `manual_slice_fill`, `vec_init_then_push`, `manual_memcpy`,
   `manual_clamp`, `assign_op_pattern`, `manual_div_ceil`, …).  These
   rewrites are semantics-preserving (`manual_memcpy` →
   `copy_from_slice`, `vec_init_then_push` → `vec![…]`, etc.).  Land
   `linux.rs` (200 of the 451) as its own commit(s) since it is the
   hottest file and the largest single chunk.  Boot-test after each
   batch.
2. Hand-fix the judgment tail: dedupe the `#![allow(dead_code)]`
   attributes, extract `type_complexity` aliases, inspect every
   `if_same_then_else` for an actual logic bug before collapsing it,
   and do the acronym/enum renames with a grep for external callers.
3. Separately audit `cast_ptr_alignment` (107) and `large_stack_arrays`
   (7) from the warn tier — these are the only warn-level lints with a
   real correctness dimension; annotate or fix each.
4. Leave the remaining warn-level lints as-is (by design); revisit only
   the policy, not the individual sites.

Until step 1–2 land, `cargo clippy -p kernel` exits non-zero, so it
cannot be used as a CI gate yet.  `cargo build` / boot-test are clean.

---

### (closed) TD1 — `frame::ALLOCATOR` IRQ-safety — closed as F5 on 2026-06-07.
