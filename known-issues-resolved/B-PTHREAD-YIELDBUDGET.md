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

**New variant 2026-07-15 — empty-capture, NOT a hang
(`build/hang-catches/soak-20260715-022705-iter18`).** Distinct manifestation of
the same real-glibc pthread self-test (`proc/spawn.rs`, `EXPECT_OUT =
"SLATE_GLIBC_PTHREAD_OK counter=40000 joinsum=10\n"`): the child **reached
Zombie and exited with the correct code** (so it passed the reap and exit-code
checks), but the captured stdout file read back **0 bytes** —
`[spawn]   FAIL: real glibc pthread — captured 0 bytes [], expected [83,76,…]`,
`WARNING: Path-Z real glibc pthread self-test failed: InternalError`. Since the
child fully exited (glibc's `atexit` stdio flush should have run and the
`write(1,…)` to the redirected capture file completed), an empty read-back points
at a **capture-file write/read visibility race** (a just-written file's contents
not yet visible to the harness's immediate `Vfs::read_file`), *not* the
yield-budget hang above — the two are different failure modes of the same test.
Intermittent: 1 of ~18 armed boots in that soak; every other pthread run in the
soak passed. Unlike the hang variant (classified `TimedOut` → WARNING), this
empty-capture path returns `InternalError` and boot-test.sh flags the boot
FAILED. ~~**Proper fix (deferred, needs its own investigation):** determine whether
the redirected-fd-1 file write is fully durable/visible at the moment the child
becomes Zombie — if not, either fsync/flush on the capture fd at process teardown
or have the harness retry the read-back a bounded number of times.~~

**ROOT CAUSE FOUND + RESOLVED 2026-07-22 — it was NOT a write/read visibility
race; it was a redirect *TOCTOU* race.** The empty-capture-with-correct-exit
symptom was reproduced on the hosted-cc `inline-asm` (Path-Z) rung and pinned to
a leaked bare `42` on the serial console — the child wrote to the *console*, not
the capture file. The capturing self-tests opened a capture file and then
installed the redirect into the child's kernel `linux_fd` table with
`linux_fd_take` + `linux_fd_install_at` **after `spawn_process` returned**.
`spawn_process` yields a *Ready* child, so a timer-tick preemption (or another CPU
under SMP) can schedule the child and let its very first `write(1, …)` run *before*
the parent installs the redirect — that write leaks to the console and the capture
file reads back empty (exit code still correct). The window is small, hence the
~1/18 intermittency, and it manifested across every output-capturing self-test
(pthread, stdio, full, signal, fault, sigqueue, forkexec, pipe, dash-script,
hosted-cc), not just pthread.

**Fix:** the redirect is now installed *atomically at spawn*, before the child is
runnable, via a new `spawn_process_with_redirects` /
`spawn_process_with_abi_and_redirects` (both thin wrappers over
`spawn_process_inner`, which applies the redirects into the child's `linux_fd`
table right after `linux_fd_install_stdio` — i.e. before the child's first
instruction). All ~11 capture self-test sites in `proc/spawn.rs` were converted
from the racy post-spawn pattern to the born-with-redirect wrappers, closing the
window entirely. While doing so, a **pre-existing latent handle leak** was also
fixed: the old post-spawn code installed the capture handle into `linux_fd`
*without* `register_ipc_handle`, so `exit_close_fds` / `destroy` never reclaimed
it (the `linux_fd` table has no teardown of its own — no `Drop`); each capture
test leaked one fs handle per boot. The new redirect path registers the moved-in
handle as an owned `File` ipc-handle, so it is reclaimed on both the normal
zombie transition and the force-kill/error path. Ownership is now unambiguous:
`spawn_process_with_redirects` owns the redirect handles on *every* path (moved +
registered into the child on success; closed on any error — directly for
pre-create validation failures, via `destroy`'s `cleanup_handles` for
post-create failures), and callers never close them.

**Container log redirect — converted 2026-07-22 (same class-fix).**
`kernel/src/container.rs`'s container-log stdout/stderr redirect used the same
post-spawn `linux_fd_take` + `linux_fd_install_at` shape (with a fd-2 dup2) and
had the *same two bugs*: the TOCTOU window (a preempted init could leak its
first `write` to the console before the redirect landed) and the missing
`register_ipc_handle` (so the capture handle leaked one fs handle per container
run). Both are now fixed by the same born-with-redirect mechanism: `run_with_abi`
opens the capture file *before* spawning (`open_capture_log`) and passes the
handle as **both** fd 1 and fd 2 to `spawn_process_with_redirects` /
`spawn_process_with_abi_and_redirects`, so the redirect is installed inside the
spawn before the child is runnable. To support the fd-1/fd-2 alias (dup2
semantics — one shared handle / append position), the spawn redirect applier was
generalised to **dedup by handle**: it installs every fd but registers (and, on
error/native paths, closes) each *distinct* handle exactly once, so an aliased
handle is reclaimed once rather than double-closed. Validated by the container
`logs` self-test (`run_with_abi(HELLO_ELF, …, AbiMode::Linux)` at
container.rs:~6557).

**Still not converted (separate, larger race — tracked):** `run_with_abi`'s
`add_process_task` (cgroup billing + pid/net/uts namespace binding) still runs
*after* `spawn_process` returns, i.e. after the child is Ready. In principle a
timer-tick preemption could run the child's first instructions before the
binding completes (the same "child runs before setup finishes" shape). This is a
distinct, pre-existing issue with different severity (accounting/namespace, not
lost output) and a much larger fix (threading container-binding info into the
spawn), so it is left for a dedicated follow-up rather than folded into the
capture-redirect fix.

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

**OCCURRENCE 2026-07-14 (netstack Phase 5 increment 5.6, persistent daemon + NIC
handoff).** Multiple `boot-test.sh --no-build` runs timed out at `BOOT_OK not
found within 480s` at **different, non-deterministic Path-Z points** run-to-run —
the definitive moving-hang tell of this race: (a) with the cutover switch forced
on, a run froze in the dash pipeline test `/bin/emit | /bin/countbytes > file`
(process 174 `countbytes` blocked reading the pipe); (b) with the switch off
(default), one run froze at the `ipv4` self-test after a one-off `Incorrect order
for descriptors` (virtio) on stderr, another froze at the tcc hosted-C build
(process 217). The `prctl-batch269` idle-task name recurred (as in earlier
occurrences and in an unrelated hrtimer-self-test panic this session). The 5.6
deliverables **all passed cleanly before every hang**: with the switch **on**, the
persistent netstack daemon spawned at boot, claimed the NIC, registered
`net.stack`, and DNS (`example.com`), TCP (HTTP over the daemon) and UDP (DNS
datagram over the daemon) parity all succeeded (serial lines 1800–1810); with the
switch **off**, the bounded netstack self-tests passed (raw-frame ARP round-trip,
DNS-over-IPC). The switch-off boot path is behaviourally unchanged by the 5.6
work (the persistent daemon only spawns when `net.userspace` is set), so these
hangs cannot be a 5.6 regression. Increment 5.6 committed on this basis. **STILL
OPEN — same container-exec / ring-3 spawn-dispatch race.**

**OCCURRENCE 2026-07-15 (EEVDF-PICK-ON O(log n) rewrite validation).** Two
consecutive `boot-test.sh` runs both timed out at `BOOT_OK not found within
480s` at **different, non-deterministic points**; an **immediate third run
passed cleanly in 95s** (`BOOT_OK`, all self-tests OK) — the definitive
non-determinism tell, not a regression. Run 1 froze in the **virtio-blk write
path** (repeated `[virtio-blk] Write sector NN timed out (IRQ mode)` on the
`vdb` ext4 rootfs) with QEMU's one-off stderr `Incorrect order for
descriptors`. Run 2 froze **earlier and at a ring-0 point** — mid-serial-output
inside `budstat::self_test` (printed `  [3/` of the buddyinfo self-test line and
then went dark mid-character). **Data point re. the two-wedge model:** run 2's
freeze is *pre-userspace* (a ring-0 boot self-test, long before any container
exec / ring-3 spawn) and froze *mid-serial-write*, which is the hard-CPU-wedge
signature (the UART poll loop spins because the CPU is otherwise stuck), NOT the
`[liveness] SYSTEM HANG … all CPUs idle-ticking` lost-wakeup signature of the
ring-3 spawn-dispatch race. `budstat::self_test` itself was **audited and is
provably deadlock-free** (straight-line assertions, no loops, `spin::Mutex`
fully released between calls, STATE never touched from interrupt context) — so
budstat is a red herring: it is merely where the CPU happened to be when the
wedge fired. This reinforces that at least one still-open wedge is a general
hard-CPU-wedge that can strike at *any* point (ring 0 or ring 3), distinct from
the ring-3-only container-exec lost-wakeup race. The EEVDF change under test is
an opt-in non-default scheduler backend whose self-tests **passed cleanly in all
three runs** (`eevdf: all tests passed`, serial line ~6842) and cannot affect
the ring-0 boot path (the default `PriorityRoundRobin` runs the boot); it was
committed on this basis. **STILL OPEN.**

**BREAKTHROUGH 2026-07-15 (armed wedge-soak caught the hang with a resolved RIP —
it is a deadlock on the GLOBAL KERNEL HEAP LOCK).** After the three runs above, an
armed hang-repro soak (`scripts/wedge-soak.sh`: `boot-test.sh
--hard-lockup-watchdog --no-build` in a loop, which enables the i6300esb NMI
watchdog + the HMP monitor so a wedge's frozen CPU state is captured directly from
QEMU) **reproduced and captured the wedge on iteration 2**. The captured guest
state (`build/hang-catches/soak-20260715-004449-iter02.{serial,regs}.txt`) is
decisive:
- **Wedged RIP = `0xffffffff80b25ce6` = `core::sync::atomic::spin_loop_hint`+6** —
  the CPU is spinning in a `spin::Mutex` acquire loop, not making progress.
- **RDI = `0xffffffff827a1378` = `kernel::mm::heap::HEAP`** (resolved via
  `llvm-nm`). RDI is the first-arg / lock pointer: **the lock being spun on is the
  global kernel heap allocator's `HEAP.inner` `spin::Mutex`.**
- **RFL = `0x202` → IF=1 (interrupts ENABLED).** So this is NOT the IF=0
  hard-CPU-wedge family — it is a *lost-release spinlock deadlock* with interrupts
  live (which is why `[liveness] SYSTEM HANG … all CPUs idle-ticking` DID fire and
  dump the task table this time).
- **RBX = `0xffffffff810c2e20` = `kernel::proc::spawn::userspace_entry_trampoline`**
  — the spinning task is on the process-spawn → ring-3 entry path (task
  `port-init`, `state=Running`, in the liveness dump), consistent with the
  long-suspected container-exec / spawn-dispatch locus.
- **`info cpus` shows a UNIPROCESSOR guest (only CPU#0).** So this is NOT a
  cross-CPU AB-BA lock-ordering deadlock (that needs ≥2 spinning CPUs). On UP, a
  permanent spin on a lock means the holder is **not running and never will be** —
  i.e. **a task acquired `HEAP.inner.lock()` and then exited / was torn down
  WITHOUT releasing it** (leaked `MutexGuard`), or was preempted while holding it
  in a context that then never reschedules it. The liveness dump shows several
  `state=Dead` tasks (`/tmp/restart-init.elf`, `logs-init`) alongside the live
  spinner — a dead holder fits.

**Ruled out this session:** (1) reentrancy through the frame allocator — the heap
slow path (`KernelHeap::alloc`/`dealloc` → `HeapInner::slab_alloc` → `refill`)
holds `HEAP.inner.lock()` across `frame::alloc_frame()` and
`memtype::charge()`, but **neither allocates from the heap**: `memtype::charge`
is pure atomics, and `frame::alloc_frame` + its sub-paths (`pcpu_refill`,
`alloc_order`, `charge_cgroup_alloc`) contain no `Vec`/`Box`/`BTreeMap`/`format!`
— only fixed per-CPU array pushes and atomics (audited `kernel/src/mm/frame.rs`).
So holding the heap lock across the frame call is not itself a reentrancy
deadlock. (2) Cross-CPU AB-BA — ruled out by the UP config.

**Remaining hypothesis to confirm:** a code path acquires the global heap lock
(directly, or transitively via any `alloc`/`dealloc`/`Vec`/`Box` on the
spawn/exec/teardown path) and then the owning task is destroyed or context-switched
away permanently before the guard drops, leaving the lock held forever. The
decisive next step is **heap-lock owner instrumentation**: record the owning
task-id + acquire-site RIP whenever `HEAP.inner` is locked, and dump it from the
liveness/NMI path, so the next caught wedge names the holder and the exact
acquire site. `scripts/wedge-soak.sh` reproduces reliably (~1 catch per 1–3 armed
boots) to validate any fix. **STILL OPEN — now localized to the global heap
spinlock; next: instrument the owner and identify the leaked-guard / dead-holder
site.**

**ROOT-CAUSED AND FIXED 2026-07-15 (it is a SERIAL-lock re-entrancy deadlock,
NOT the heap lock — the earlier RDI=&HEAP read was a coincidental register
leftover).** After adding the heap-lock owner instrumentation above, a fresh
armed soak caught the wedge again on iteration 6
(`build/hang-catches/soak-20260715-012819-iter06.*`), and this time the **NMI
hard-lockup watchdog produced a full `rbp`-chain backtrace** — vastly more
reliable than guessing the lock identity from a leftover register. The
backtrace is decisive:

```
core::sync::atomic::spin_loop_hint        <- spinning on a spin::Mutex
kernel::sched::liveness_boot_deadline_check   <- the frame taking the lock
kernel::sched::timer_tick
handle_timer_irq / dispatch_vector / run_on_irq_stack / irq_common_dispatch
isr_timer                                  <- TIMER IRQ context
kernel_main                                <- interrupted here (boot self-tests)
kmain
```

The heap-lock owner dump printed **`heap-lock: unlocked (no current holder)`** —
conclusively exonerating the heap. The only lock `liveness_boot_deadline_check`
acquires is the **global `SERIAL` `spin::Mutex`**, via its 30 s "boot-window
breadcrumb" `serial_println!`. Root cause: **`serial_print!` acquired
`SERIAL.lock()` without disabling interrupts.** A task doing boot self-test
output (`kernel_main`, serial-heavy) held the lock mid-write; a timer IRQ fired
on the **same CPU** (interrupts enabled); the ISR's `liveness_boot_deadline_check`
breadcrumb tried to re-acquire the already-held `SERIAL` lock and spun forever —
the interrupted task can never resume to release it. This is a textbook
ISR-vs-task non-reentrant-spinlock deadlock on the console lock.

It explains **every** prior symptom in this cluster: the "mid-serial-write" hard
freezes (the wedge *is* a serial write), the ring-0 hard-CPU-wedge signature
(`spin_loop_hint`, IF spinning), and the non-determinism (needs a timer tick to
land inside the narrow serial-lock window — and the breadcrumb path needs the
tick to also cross a 30 s bucket, hence ~1-in-several-boots). The earlier iter02
`RDI=0x…HEAP` was a register the interrupted code happened to leave behind, not
the lock identity — `spin_loop_hint` takes no arguments.

**Fix** (`kernel/src/serial.rs`): `serial_print!`/`serial_println!` now route
through a `serial::_print(fmt::Arguments)` function that takes the `SERIAL` lock
**inside `cpu::without_interrupts(...)`**, so same-CPU IRQ re-entry is
impossible (the standard console-lock discipline, cf. Linux `spin_lock_irqsave`).
Cross-CPU contention remains deadlock-free (the holder runs IRQ-off and releases
promptly). The `_print` function form (vs. inlining in the macro) preserves
`?`/`return` semantics for expressions used inside a `serial_println!(…)` format
argument. NMI/panic output is unaffected — it already uses the lock-free
`emergency_print!` path.

The heap-lock owner instrumentation (`HEAP_LOCK_OWNER`/`HEAP_LOCK_SITE` +
`lock_tracked()` + `mm::heap::dump_lock_owner()` wired into the liveness dump) is
**kept** — it is cheap, and it is what proved the heap was innocent; it will name
the holder immediately should a *heap*-lock deadlock ever occur.

**Validation — DONE (2026-07-15).** Rebuilt; boots green; re-ran the armed
`wedge-soak.sh`. **Four consecutive clean armed boots** (soak-20260715-020155
iters 01–04, 97/101/101/92 s to BOOT_OK) with **no `spin_loop_hint` wedge** — the
`spin_loop_hint ← liveness_boot_deadline_check ← timer_tick` deadlock no longer
reproduces. This spinlock-deadlock issue is considered **fixed**. As predicted,
a *distinct* wedge with a different signature then appeared (iter05, see next
entry) — that is a separate lost-wakeup/hang issue, not this deadlock.

**Recurrence 2026-07-18 (Q24 wedge-soak — pthread yield-budget flake, NOT a
lock regression).** During the final validation soak for the Q24 raw-`spin::Mutex`
holder-preemption conversion (`MONITOR_PORT=57321 MAX_ITERS=6`), **no wedge was
caught in 6 armed boots** (the holder-preemption race did not fire — Q24's
core goal validated), but iter 02 tripped the B-PTHREAD-YIELDBUDGET flake:
`[spawn] FAIL: real glibc pthread — process did not exit within 262144 yields
(state=Some(Running))` → `WARNING: Path-Z real glibc pthread self-test failed:
TimedOut`. Notable twist: this fired at a **normal** BOOT_OK time (133 s), not the
historically-heavy ~217–229 s runs — because the budget counts *scheduler yields*,
not wall-clock, and Q24's preemption-timing changes (leaf locks now
`PreemptSpinMutex`, i.e. preempt-disabling) legitimately shift how many yields the
driver burns while the child runs. This is **not** a Q24 correctness regression:
the futex primitive was already proven sound (2026-07-01 audit), the implicated
machinery is the ring-3 clone/CoW/thread-teardown path (untouched by Q24), and
5/6 pthread runs in this very soak passed. Second-order harness quirk reconfirmed:
the *non-fatal* `TimedOut` WARNING string literally contains "self-test failed",
so `boot-test.sh check_selftest_failures` flags the boot FAILED even though the
warning is meant to be non-fatal — extra motivation for the long-deferred harness
fix (wait on a real exit signal / adaptive budget rather than a fixed yield count,
and/or make the WARNING text not collide with the failure-grep phrase).

**Recurrence 2026-07-23 (B-KNULLJUMP corruption-hunt soak, iter 10 of 24 —
same flake, confirmed NOT a corruption).** During the armed corruption-hunt
soak (`HUNT=1`, KASAN shadow + free-quarantine armed around the Path-Z block;
`build/hang-catches/soak-20260723-083044-iter10.*`), iter 10 was the only
failing boot of the 13 that completed before this note, and it was the exact
B-PTHREAD-YIELDBUDGET signature: `[spawn] FAIL: real glibc pthread — process
did not exit within 262144 yields (state=Some(Running)); a thread likely
deadlocked on a futex or a worker faulted` → `WARNING: Path-Z real glibc
pthread self-test failed: TimedOut`. Diff vs. the adjacent passing boots
(iter 09/11) is diagnostic and benign: in a passing run the log shows workers
`Task 203..206 exiting`, then `Process 240 has no threads left — now zombie`,
then **`Task 202 exiting`** and `OK`; in iter 10 everything is identical *up
to* the zombie line, but the main thread (task 202) never reaches its exit
syscall before the driver's fixed 262144-yield budget expires — i.e. the main
thread was still burning userspace cycles in glibc's `pthread_join`
spin-then-futex path when the budget ran out. **Crucially, the corruption hunt
was CLEAN on this boot:** the Path-Z checkpoint reported `quarantine …
corruptions=0 (scan found 0); kasan violations=0 shadow_frames=298`, so this
was *not* a wild write into a parked slot. It is the yield-budget flake, not
B-KNULLJUMP. (B-KNULLJUMP manifests as a hard `#PF`/GPF in
`alloc::…::navigate::next_kv`, never as a soft `TimedOut`.) Reconfirms the
long-deferred harness fix (adaptive budget / wait on a real exit signal, and
de-collide the WARNING text from the failure grep).

**Tooling-gap note from the same hunt (for B-KNULLJUMP, recorded here for
cross-reference):** the free-quarantine only catches a wild write whose target
is a *freed, parked* slab slot. B-KNULLJUMP corrupts a *live* scheduler
`BTreeMap` node, so unless the stray store happens to land on a quarantined
(freed) slot, the quarantine cannot see it — which is why 13 clean
`corruptions=0` boots do **not** rule the race out. The definitive catch for a
wild store into a *live* allocation remains compiler-instrumented KASAN
(`-Zsanitizer=kernel-address`, feasibility confirmed) — see Q34.

**UPDATE 2026-07-23 (c) — KASAN shadow cover was too small; enlarged 4 GiB→64
GiB (fixes an intermittent self-test boot-halt AND a hunt blind spot; commit
`c6ff07ee3`).** A later armed soak (`HUNT=1`) caught a *wedge* on iter 01 that
turned out to be a **false** KASAN self-test panic (`assert!(check(a+40,1,…)
.is_err(), "kasan: redzone not caught")`), not a corruption. Root cause: the
shadow was sized for only **4 GiB** of heap (512 MiB shadow at 1:8), smaller
than the **5 GiB** physical RAM on the dev/QEMU config. Heap objects allocated
above the 4 GiB mark have no shadow byte — `shadow_of()` returns `None`, the
poison write is dropped, and `check_access` **fails open**, so the redzone
assertion trips. It was intermittent because hunt boots churn the heap into
higher frames, so whether the 40-byte test object lands above 4 GiB is
timing-dependent. This *also silently blinded the B-KNULLJUMP hunt above 4 GiB*
— an uncovered live allocation is unchecked, so any wild store up there would
have been missed entirely. Fix: `KASAN_SHADOW_SIZE` 512 MiB → **8 GiB** (covers
**64 GiB** heap, above physical RAM on any realistic dev box) in both
`mm/kasan.rs` and `kvspace.rs::KASAN_SHADOW`; the shadow is lazily mapped so the
reservation stays virtual-only until touched. Added a defensive self-test guard
that *skips with a warning* (instead of panic-halting the boot) if the test
object ever lands outside the covered window on a future RAM-larger-than-cover
box. Verified: boots green (BOOT_OK), KASAN self-test now **PASSED** (redzone,
UAF, partial-granule, out-of-range fail-open all OK), and the log confirms
`[kasan] shadow ready … covers heap [0xffff800000000000..0xffff801000000000)`
(a full 64 GiB span). The hunt tooling is now trustworthy for a long campaign.

**UPDATE 2026-07-23 (d) — soak wedge-detection was false-positiving on SLOW
boots; added a serial-stall discriminator.** The first re-launched hunt soak
(`soak-20260723-182551`) "caught a wedge" on iter 02 — but it was a **false
positive**: the archived serial log showed the kernel still actively running
self-tests (`[service] Register/connect/accept: OK`, near the end of the suite)
at the moment of the 480s timeout, having reached 23591 lines (*more* than the
23470 of a passing boot). The captured RIP `0xffffffff81ec4d2a` resolved to
`core::ptr::read_volatile+0x2a` with `RAX=ffff8000fee00020` (HHDM + local-APIC
base + 0x20 = the APIC ID register) — a completely benign MMIO read the HMP
capture happened to freeze on. Root cause: under TCG on a loaded host the full
self-test boot had slowed to ~460-470s (the harness comment still assumed
~305s), leaving the soak's 480s timeout almost no margin; and `boot-test.sh`
captured a RIP on *any* timeout-with-guest-alive, so `wedge-soak.sh`'s "rc≠0 +
captured RIP = wedge" rule fired on a merely-slow boot and **aborted the whole
campaign** on iter 02. Fix (commit pending): (1) `boot-test.sh` gained an opt-in
`--stall-secs=N` serial-stall detector — a *wedged* kernel stops emitting serial
output, so if the log goes silent for N s while QEMU is alive and BOOT_OK has
not appeared, that is a genuine hang (captures the RIP, exits **2**); a
slow-but-healthy boot keeps appending self-test lines and never trips it.
(2) `wedge-soak.sh` now only stops on a *genuine* anomaly — a corruption
checkpoint (`corruptions=[1-9]`), a stall-wedge (rc=2), a hard fault/panic/
`SYSTEM HANG` signature in serial, or a self-test regression — and treats a
plain slow-boot timeout (rc=1, none of those) as a wasted sample that it logs
and **continues past** instead of aborting. Per-boot timeout raised to 720s
(headroom over ~470s boots) with `STALL_SECS=150` (generous, so a legit long
*quiet* Path-Z tcc/make window is never mistaken for a hang). Net: the soak can
now run a long unattended campaign without a slow boot masquerading as a catch.

**UPDATE 2026-07-23 (e) — second false-positive class fixed: fault signatures on
a PASSED boot.** The re-launched soak (`soak-20260723-185331`) then stopped on
iter 01 with a bogus "HARD FAULT / HANG" — even though that boot **reached
BOOT_OK and PASSED in 324s** (also confirming the earlier ~466s was transient
host load, not a regression). The over-broad serial grep matched three
*expected* healthy-boot lines: a ring-3 `#GP` immediately followed by `SEH
handler resumes execution: OK` (an intentional SEH self-test), `instructions
would #UD` (the SMAP-absence diagnostic), and a **transient** `[liveness] SYSTEM
HANG` dump (a self-test paused >15s under load at serial line 4992, the detector
dumped the task table, then progress resumed and the boot completed). Fix
(same commit line): the fault/hang/self-test-regression catches are now **gated
behind `rc != 0`** — a boot that exits 0 is healthy by construction
(`boot-test` exits 0 only on BOOT_OK with no failures), so on a passed boot only
the hunt's own `corruptions=[1-9]` checkpoint counts; a genuinely fatal fault
always prevents BOOT_OK and thus surfaces as `rc != 0`, so nothing real is lost.
The rc≠0 fault grep was also narrowed to unambiguously-fatal tokens (`KERNEL
PANIC`/`FATAL`/`DOUBLE FAULT`/`SYSTEM HANG`/null-RIP) and no longer greps bare
`#GP`/`#UD`/`EXCEPTION`, which recur in expected ring-3 self-test output. (Note:
the quarantine *self-test* deliberately corrupts a parked slot and prints
`[quarantine] *** CORRUPTION ***` to prove the scanner works — that line is
`[quarantine]`-prefixed and never matches the hunt's `[hunt] … corruptions=[1-9]`
checkpoint pattern, so it does not false-trip the corruption catch.)

**UPDATE 2026-07-23 (f) — first full 100-iter armed hunt campaign ran CLEAN
(B-KNULLJUMP NOT reproduced; bug NOT cleared).** With the KASAN cover fix (c) and
both false-positive fixes (d),(e) in place, `soak-20260723-190300` ran to
completion: **100/100 boots reached BOOT_OK and PASSED**, boot wall-clock
~283–318s (297s on the final iter), `[hunt] corruptions=0` on every iteration,
zero serial-stall wedges (rc=2 never fired), zero fatal-fault catches. Final
harness verdict: `SOAK DONE: no wedge caught in 100 iters (race did not fire)` /
`WEDGE_SOAK_DONE rc_caught=0`. Archives: `build/hang-catches/soak-20260723-190300-iter*.{serial,stdout}.txt`.
**This does NOT clear B-KNULLJUMP.** At the observed ~1-in-120 base rate,
P(zero catches in 100 boots) ≈ e^(−100/120) ≈ **43%** — i.e. a clean 100-iter
run is the *more-likely-than-not* outcome even if the bug is fully present and
unchanged. So the campaign is inconclusive, not exonerating. What it *does*
establish: (1) the harness is now false-positive-free across 100 real boots
(the three fixed classes did not recur); (2) the passive KASAN-shadow +
slab-free-quarantine tooling (Path A / Q32→A), even with the 64 GiB cover, did
not catch the wild write in this window — consistent with the known structural
blind spot (B-KNULLJUMP stomps a *live* scheduler BTreeMap node, not a
parked/poisoned slot, so passive shadow only catches it on the rare occasions
the stray store lands in an already-poisoned granule). Path A is effectively
exhausted for reproduction/localization; the documented next escalation is
compiler-instrumented KASAN (Q34→B in open-questions.md), which is
operator-decision-worthy and NOT started unilaterally. B-KNULLJUMP does not
block other roadmap work.

**UPDATE 2026-08-12 (g) — Path B (compiler-instrumented KASAN) is BUILT and
survives boot, but is ~20× too slow to soak; hunt NOT yet run.** The operator
approved the Q34→B escalation (design-decisions.md §107) and the instrumented
profile now exists and works. Getting there took two structural fixes, both of
which are the same invariant with different roots — "no `__asan_*` call may be
reachable from here" — and both are now *build gates* rather than review items,
because source review demonstrably cannot see them (generic `core` functions
monomorphise into the kernel crate carrying the default, instrumented,
attribute, so a module-level `sanitize(address = "off")` does not cover them):

- **§118 — the pre-shadow window.** Kernel entry through
  `mm::kasan::install_zero_shadow` has no shadow to read and no IDT to catch the
  fault, so one instrumented access is a triple fault: QEMU resets and prints
  *nothing*, indistinguishable from any other boot failure. Two real ones were
  hit this way (`serial::init`'s spinlock CAS; a `for i in 0..512` lowering to
  `spec_next`). The window is now raw `asm!` loads/stores, and
  `scripts/kasan-check-preshadow.py` walks the disassembled call graph from the
  entry point to prove it.
- **§119 — the runtime check path.** Checks are now *outlined*
  (`-asan-instrumentation-with-call-threshold=0`) because the inline sequence
  dereferences `(addr >> 3) + offset` unconditionally, and for a *user* address
  that shadow is non-canonical — a `#GP`, not a report. Our kernel legitimately
  derefs user pointers (SEH context onto the user stack in `idt.rs`, every
  `mm/user.rs` helper), so inline checks made each such site a panic unrelated to
  the bug; one killed the boot at line 1298. Outlining creates the obligation
  that nothing under `shadow_allows` may perform an instrumented access, which a
  second walk from the `__asan_load*`/`__asan_store*` roots enforces — it caught
  `Option::<u8>::is_some` on its first run.

Current gate output: `14 function(s) reachable before the shadow is installed
and 21 on the check path, none instrumented`. A boot to `BOOT_OK` is in flight.
One genuine pre-existing test fragility surfaced en route and was fixed
(`fs/timerq.rs` self-test used a "far future" periodic deadline of 1000 s of
uptime, which only a *fast* boot outruns; now 2^62 ns).

**The blocker is cost, not correctness.** Measured: a plain debug boot is
~283–318 s; the instrumented one needs **5500–8500 s (~20×)**. At B-KNULLJUMP's
~1-in-120 rate an even-odds soak is ~80 boots = **over a week of wall-clock**,
versus ~7 h for the plain-build soak. So Path B is a validated tool that cannot
yet be *used* for the thing it was built for. How to make it affordable —
optimized instrumented build, trimmed workload, soak anyway, or shelve — is
**open-questions.md Q43**, deliberately left to the operator because the leading
option (`--release`) would be the first optimized kernel ever booted here and
perturbs the very timing the race depends on. `scripts/wedge-soak.sh` is already
armed for it: catch (0) greps `[kasan] CRITICAL:` on every boot regardless of
exit code (the profile uses `-asan-recover=1`, so a reported access still
completes and the boot can pass), and its header documents the raised
`SOAK_TIMEOUT`/`STALL_SECS` an instrumented soak requires.

**SEPARATE STILL-OPEN WEDGE — `gen_dmastat` / `restart-init.elf` spawn-dispatch
(first isolated 2026-07-15, `build/hang-catches/soak-20260715-020155-iter05.*`).**
On the very soak that validated the serial fix, iter05 caught a *different* wedge
that the NMI watchdog reported:
- **Wedged RIP = `0xffffffff80e6d896` = `kernel::fs::procfs::gen_dmastat+1270`
  (0x4f6)** — genuinely inside `gen_dmastat`, not a leftover symbol.
- **`RFL=0x202` (IF=1)** — interrupts *enabled*, so this is NOT the IRQ-off
  spinlock deadlock; the CPU is not spinning with IF=0.
- `RBX=0xffffffff80f8f7a0`, `CR2=0x60000ee200`.
- Serial tail: `[spawn] Process 225 running ("/tmp/restart-init.elf")` then
  `[liveness] SYSTEM HANG … all CPUs idle-ticking`, `heap-lock: unlocked`, and a
  2-task dump: **tid=0 `"prctl-batch269"` state=Ready**, **tid=189
  `"/tmp/restart-init.elf"` state=Running cpu=0**, `cpu0 last_rip=gen_dmastat`.

This is the recurring `restart-init.elf`(Running) / `prctl-batch269`(Ready)
spawn-dispatch signature from earlier in this cluster. Because `record_last_rip`
is called *unconditionally* from the timer ISR, `last_rip=gen_dmastat` means cpu0
was *actively executing* `gen_dmastat` at the last tick (not parked idle) — which
is in tension with a pure lost-wakeup model and hints at a busy path (a loop or
repeated re-entry) inside or above `gen_dmastat`. `gen_dmastat` itself
(`fs/procfs.rs:10081`) is straight-line with no obvious infinite loop, and
`dmastat::device_stats()`/`stats()` are bounded Vec clones — so a **single** RIP
frame is inconclusive (`last_rip` has been a red herring before: kernel_text,
budstat, now gen_dmastat).

**Proper next step (in progress):** make the liveness `SYSTEM HANG` dump
(`sched::dump_all_tasks_serial`) capture a full **rbp-chain backtrace of the
stuck CPU**, the way the NMI path does (`idt.rs::dump_kernel_backtrace` recovers
the interrupted RBP via `read_volatile(frame_ptr.sub(6))` from the ISR-stub save
area, then walks the chain with `backtrace::walk_from`). The liveness check runs
in the timer ISR, so to walk the *interrupted* (gen_dmastat) stack rather than
the ISR's own, the interrupt frame's saved RBP must be threaded into the dump.
Once done, the next catch of this wedge yields a conclusive call stack instead of
a lone RIP.

**UPDATE 2026-07-15 — rbp-chain backtrace implemented and it FIRED (iter19),
plus a backtrace-validator bug it exposed
(`build/hang-catches/soak-20260715-022705-iter19.*`).** The liveness `SYSTEM
HANG` dump now threads the interrupted RBP (sampled per-CPU in the timer ISR via
`rip_sample::record_last_rbp`, read from the ISR-stub save area at
`frame_ptr.sub(6)`) into `backtrace::print_from`. On iter19 it produced a
4-frame backtrace — but the frames were **garbage**, and diagnosing *why* found
a real bug in the walker:

- The sampled `cpu0 last_rbp = 0xffffffff824ca080` is in kernel **`.data`**, not
  a stack (per-task stacks are at `0xffffc100…`, the NMI capture confirms
  `RSP=RBP=0xffffc1000003bae0`). A frame pointer can never legitimately point
  into `.text`/`.data`.
- `backtrace::is_valid_frame_ptr` nonetheless **accepted any address ≥
  `0xFFFF_FFFF_8000_0000`** ("static stacks"), so the walker dereferenced static
  data as a frame chain and emitted four bogus frames (`#2/#3` resolved to
  `kshell::cmd_startmenu`, which is not on any live call path). **Fixed**: the
  validator now accepts, within the kernel image range, *only* the exact bounds
  of `KERNEL_BOOT_STACK` (new `crate::boot_stack_bounds()`), rejecting general
  `.text`/`.data`. A non-stack RBP now yields zero frames (honest "no backtrace")
  instead of a misleading one.
- **Why was `last_rbp` a `.data` pointer?** The timer sample (`last_rip =
  alloc::collections::btree::node::slice_shr+0x5d`, a `BTreeMap` node op) and the
  NMI capture (`RIP = 0xffffffff80a4f036 = kshell::cmd_queryable+0xd96`, DWARF →
  the `shell_println!`/`format!` macro; `RFL=0x202`, IF=1) are from **different
  instants** — cpu0 is executing *different* code at different times, i.e. this
  is a **livelock, not a hard freeze**, consistent with the `gen_dmastat` family
  above (batch task `tid=0 "prctl-batch269"` shown **Running**, liveness insists
  "all CPUs idle-ticking"). At the timer tick that recorded the sample, RBP
  happened to hold a data pointer (mid-prologue/epilogue or a leaf helper not
  using RBP as a frame pointer), so the walk-start was junk. The reliable signal
  remains the NMI RIP (`cmd_queryable`, in `shell_println!`/`format!`) — a busy
  alloc/format path — plus the livelock character. `last_rip`/`last_rbp` samples
  remain **inconclusive** for this wedge family (a lone sample has been a red
  herring three times now: kernel_text, budstat, gen_dmastat, and now a
  `.data` RBP). The next catch will at least no longer print a fabricated stack.

**UPDATE 2026-07-15 (second catch) — validator fix works; stale-stack limit
confirmed; per-CPU recent-RIP history added
(`build/hang-catches/soak-20260715-032821-iter19.*`, `RIP=0xffffffff81e7d35a`).**
A second soak caught the same wedge, and this time the backtrace-validator fix
paid off: `cpu0 last_rbp = 0xffffc10000063ae0` is a **valid task stack**, so
`backtrace::print_from` walked a clean **9-frame** chain instead of garbage — the
fix is confirmed good. But diagnosing the frames surfaced the *deeper* limitation:
- Frames `#0/#1/#2` land inside `procfs::self_test` (a 163 KiB symbol, verified
  with `llvm-nm --print-size` + Python bisect since debug-build monomorphizations
  have no exported symbols and defeat nearest-symbol lookup). Yet the serial log
  shows `[procfs] Running self-test...` **completed at line 3201**, long before
  the hang (line 6181). So those frames are **stale** — leftover return addresses
  on a reused stack, not the live call path.
- `last_rip` is inside `oomkiller::select_victim`'s iterator (a finite `Vec`
  fold — provably cannot infinite-loop), and the NMI-captured RIP lands in a
  symbol *gap* (discard). All three signals point to **transient** locations.
- The **reliable** signal is the serial *phase*: the hang struck during
  `[container] Running self-test...` right after `test-port-ct` (veth pair +
  published ports → NAT forward) spawns `port-init` (task 192, ring-3). This is
  the **container-exec / ring-3 spawn-dispatch race** family. IF toggling + RSP
  moving between NMI captures = **livelock**, consistent with the entries above.

**Fundamental limitation & the fix for it.** An asynchronous timer tick samples
the CPU at an arbitrary instant, which for a livelock is almost never a frame
boundary — so *both* the rbp-chain (stale return addresses) *and* the lone RIP
(whichever loop-body instruction ran) are unreliable. For a task cycling a loop
that never yields, the conclusive datum is the **set of recently-sampled RIPs**:
if the last N ticks cluster in a tight address range, that range *is* the spin
loop, revealed directly with no stack unwinding or symbol-gap guessing.
**Implemented this session:** a per-CPU recent-RIP ring buffer
(`rip_sample::RIP_HIST`, 16 samples/CPU, `record_rip_history` called every timer
tick alongside `record_last_rip`; reader `recent_rips`). The liveness
`SYSTEM HANG` dump (`sched::dump_all_tasks_serial`) now prints each CPU's last 16
sampled RIPs (newest-first, with `AddrClass`). The next catch should show whether
cpu0's RIPs cluster (→ names the livelock loop) or scatter (→ the wedge is a lock
holder / another CPU, not a spin here). Boot-tested clean; the profiler
self-test still passes.

**UPDATE 2026-07-15 (THIRD catch — BREAKTHROUGH; recent-RIP instrumentation paid
off) (`build/hang-catches/soak-20260715-043312-iter06.*`, wedged
`RIP=0xffffffff80812316`).** The new recent-RIP history turned an inconclusive
single RIP into the clearest picture yet — and this catch is *coherent*, unlike
every prior one. All addresses below resolved against the exact booted binary
(`target/x86_64-unknown-none/debug/kernel`, built 04:32, soak started 04:33) via
`llvm-nm --print-size` + Python bisect (INSIDE-symbol verified, not gap guesses).

- **Serial phase:** hang struck during `[tmpwatch] Running self-test...`, right
  after `cleanup removes files: OK (removed 3)` (Test 5) — i.e. **Test 6's first
  `Vfs::write_file("<dir>/delete_me.tmp", b"delete")`** (a 6-byte write) never
  returned. A 6-byte write cannot legitimately take the full 240 s → genuine
  infinite loop, not poison-allocator slowness.
- **rbp-chain backtrace (20 frames, all INSIDE named symbols — the validator fix
  is now confirmed good across two catches):** a clean, sensible call chain
  `kmain → kernel_main → tmpwatch::self_test → Vfs::write_file →
  Vfs::write_file_resolved → MemFs::write_file → MemFs::resolve_write_path →
  MemFs::path_components → Iterator::collect → Vec::from_iter → RawVec allocate →
  Global::allocate → __rust_alloc → KernelHeap::alloc → HeapInner::slab_alloc →
  heap::check_poison`. So at the sampled instant cpu0 was allocating the
  `path_components` `Vec` through the poison allocator.
- **Recent-RIP history (16 samples ≈ 160 ms, this is the decisive new datum):**
  NOT clustered in one tight loop but cycling through a *small working set* of
  heap + BTreeMap + iterator code: `heap::check_poison`, `heap::poison_alloc`,
  `heap::poison_free` (the O(size) byte-walk poison ops) and their inner
  `Range`/`usize::Step` loops (`spec_next`, `forward_unchecked`, `Iterator::next`);
  `BTreeMap::clone`, `BTreeMap::…insert_fit`, `slice::IterMut::next`;
  `AtomicBool::compare_exchange_weak` (a spinlock CAS); `ptr::write_volatile`
  (poison fill). Because these span *both* a BTreeMap *search/clone* phase and a
  BTreeMap *insert* phase, cpu0 is not wedged on one instruction — it is
  **repeatedly executing allocate→(BTreeMap op)→free**, i.e. a livelock, for the
  full 15 s+ that froze `useful_work` (315) and `ctx_switches` (1119).
- **`heap-lock: HELD by tid=0 acquired at heap.rs:1047:30`** (the `alloc` global
  path) — expected, caught mid-allocation; NOT a static self-deadlock (the recent
  RIPs prove forward motion, so it is not spinning 15 s on its own lock). The
  240 s NMI `RIP = spin_loop_hint+0x6` is a single sample of a transient CAS spin
  inside that busy loop, not the whole story.
- **tid=0 `"prctl-batch269"` prio=31 Running** — the boot task itself, running the
  self-test inline at max priority with no preemption point, so the loop starves
  every other task → SYSTEM HANG.

**Root-cause hypothesis (strong, not yet proven): slab/heap corruption →
cyclic `BTreeMap` → non-terminating traversal.** `resolve_write_path`'s own loop
is bounded (`MAX_SYMLINK_DEPTH = 40`) and the non-existent-file write takes the
immediate `None → return` branch, so the loop is **not** a logic bug in memfs.
The recent RIPs put the livelock inside `BTreeMap` node ops fed by poison-allocator
churn. MemFs stores directory children in a `BTreeMap<name, node>`; if a slab
free-list develops a **cycle** (the poison allocator's own `slab_dealloc` comment,
heap.rs:632, explicitly calls this out as a corruption hazard it guards against
*only* for detected double-frees — a use-after-free that overwrites a freed slot's
`->next` from an unrelated allocation would NOT be caught), two live allocations
alias the same memory, a `BTreeMap` node's child pointer becomes cyclic, and
`children.get()` / insert traverses forever. This unifies every symptom: the
BTreeMap ops in the RIP set, the poison-allocator involvement, the **moving catch
location** (corruption is timing-dependent — prior catches landed in container
self-test and `gen_dmastat`, this one in tmpwatch), and why bounded logic loops
can't explain a 240 s hang. It is the same **B-PTHREAD-YIELDBUDGET / container-exec
spawn-dispatch** intermittent family, now with a concrete mechanism to chase.

**CONFIRMING INSTRUMENT — IMPLEMENTED 2026-07-15 (free-list link validation).**
Rather than an O(n) full free-list walk, added an **O(1)-per-pop** intrusive-link
validator (`heap::free_link_valid`, guarded by `POISON_ENABLED`) wired into both
slab pop sites (`HeapInner::slab_alloc` and `pcpu_slab_alloc`). Rationale: a freed
slot's `next` pointer lives in bytes 0..8, but the poison magic/fill only covers
bytes 8..`class_size` — so an 8-byte use-after-free write to a freed slot's first
word corrupts `next` **without** tripping `check_poison`. That is precisely how an
*undetected* free-list cycle/alias forms. On each pop the validator checks the
about-to-be-installed head link is null, or (a) higher-half HHDM, (b) `class_size`-
aligned, (c) not a self-cycle; on failure it logs
`[heap] FREE-LIST CORRUPTION! class=… slot=… bad next=…` and **severs** the list
(hands out the current slot, leaks the corrupted tail) instead of following a wild/
aliasing link. This converts the silent, location-moving wedge into a precise,
located fault at the moment of damage — and, critically, *stops* the corruption
from reaching the `BTreeMap` that would otherwise livelock. Boot-tested clean (no
false positives — valid slots always pass by construction). It does not catch a
perfect cycle between two *valid* same-class slots; if the wedge recurs without a
`FREE-LIST CORRUPTION` line, the corruption source is elsewhere (buddy allocator
`mm/frame.rs` / rmap `mm/rmap.rs`) and the next tool is a bounded full-list Floyd
cycle check on `refill`. (The `lockdep_mm` migration from todo.txt targets *lock
inversion*, a deadlock — the wrong tool for this **livelock**; deprioritized.)
Instrumentation + validator committed this session; this catch is the first that
gives an actionable code-level mechanism rather than a lone red-herring RIP.

**UPDATE 2026-07-15 (FOURTH catch — DEFINITIVE ROOT CAUSE, distinct from the
free-list hypothesis; FIXED) (`build/hang-catches/soak-20260715-051830-iter22.*`,
wedged `RIP=0xffffffff80726d96`).** A 25-iteration soak *with the free-list
validator in place* caught a wedge on iter22 — and the validator did **not** fire
(`grep 'FREE-LIST CORRUPTION'` → nothing), so the free-list-cycle hypothesis is
**not** what wedged here. But the recent-RIP + heap-lock-owner instrumentation
produced a *conclusive, different* picture — a textbook single-CPU
**holder-preemption spinlock deadlock** on the global heap lock:
- **Wedged RIP = `spin_loop_hint+0x6`** (`core::sync::atomic`) — the Running CPU
  is busy-waiting in a spinlock acquire loop.
- **`heap-lock: HELD by tid=133 acquired at heap.rs:1212:30`** (the `dealloc`
  global path) — but the task table shows **tid=133 `"emit"` state=Ready**, i.e.
  the lock holder is **not running**. It was involuntarily preempted mid-critical-
  section.
- **tid=132 `"spawn-test-glibc-pipe"` state=Running** is the spinner. Its
  rbp-chain (all INSIDE named symbols): `Vfs::read_at_uncached_resolved →
  MemFs::read_at → MemFs::resolve → drop(String) → RawVec::deallocate →
  __rust_dealloc → KernelHeap::dealloc → slab_dealloc` — freeing a path-resolution
  temporary, which takes the heap lock and spins on it forever.
- **tid=0 `"prctl-batch269"` prio=31 Ready, ready_since=2168 (waited 41 ticks)** —
  a *higher-priority* task is Ready but starved by the prio-16 Running spinner.
  That is the smoking gun: **the timer is not preempting the spinner** (a prio-31
  Ready task could never be starved by a prio-16 Running one if preemption were
  live). So cpu0 is effectively pinned in the spin with no context switch — the
  Ready holder (tid=133) can never be scheduled to release the lock. Deadlock.

**Root cause (proven).** `mm/heap.rs` locks its `inner` with a **raw
`spin::Mutex`** (`use spin::{Mutex, MutexGuard}`, line 43) rather than the
preempt-aware `crate::sync::Mutex`. The raw lock does **not** call
`sched::preempt_disable()` on acquire, so a heap critical section can be
involuntarily preempted by the timer tick. This is exactly the general
**B-PREEMPT-SPINLOCK** class (see that entry, 2026-07-01: "a spinlock must never
be held across a context switch") — which was fixed for *tracked*
`crate::sync::Mutex` via the per-CPU `PREEMPT_DISABLE_COUNT`, but the heap lock
was deliberately left a raw `spin::Mutex` (to keep the global allocator out of
lockdep — it is a leaf lock taken under nearly every other lock, and dragging it
into lockdep risks re-entrant allocation) and thus **never received the
preempt-disable protection**. The `dealloc`→`slab_dealloc` critical section got
preempted; a second task then spun on the same lock; single CPU → permanent wedge.

**Fix (committed this session).** Give the heap lock the preemption protection
directly, *without* pulling it into lockdep: `KernelHeap::lock_tracked()` now calls
`sched::preempt_disable()` before `self.inner.lock()` (disabled *before* the spin,
mirroring `crate::sync::Mutex`), and `TrackedGuard::drop` calls
`sched::preempt_enable()` — after releasing the physical spinlock. To order the
physical release strictly *before* the preempt re-enable (otherwise a window
exists where the lock is held but preemptible again — reopening the bug), the
guard wraps its `MutexGuard` in `ManuallyDrop` and explicitly drops it, then
re-enables. This defers the timer's context switch until the heap lock is
released, so the lock is never held across a switch. No lockdep, no re-entrant
allocation, no extra overhead on the hot path beyond two relaxed atomics. This is
a *distinct* bug from the free-list-cycle hypothesis above (which the validator
did not confirm here); the free-list validator stays in as cheap defence-in-depth.
Re-soak after this fix to confirm the `spin_loop_hint` wedge no longer reproduces.

**UPDATE 2026-07-15 (heap fix VALIDATED by re-soak; a NEW, distinct blocker
surfaced) (`build/hang-catches/soak-20260715-061420-iter12.*`).** Re-ran the soak
after the holder-preemption fix above. **The `spin_loop_hint` / holder-preemption
signature is gone** — and boot now progresses *hundreds* of self-tests further
(from the tmpwatch phase all the way to the eventfd-timeout self-test phase)
before wedging. So the heap deadlock fix is confirmed working. The new iter12
wedge is a *different* bug, with two parts:

1. **PRIMARY (NOT REPRODUCIBLE after the container fix — downgraded from blocker
   to latent) — nested device-IRQ dispatch hang.** cpu0 was stuck with `IF=0`,
   heartbeat frozen ~9.8 s, inside a *nested* device-IRQ handler (`isr_irq11`)
   reached from the **outermost timer's IF=1 window** (the timer re-enables
   interrupts on the IRQ stack for softirq/preempt; a level-triggered device IRQ
   then fired and nested). `RSP=0x…27be8` is only ~0x418 below the IRQ stack top
   `0x…28000` — i.e. only ~1 KiB into the 16 KiB IRQ stack, so this is **not** a
   deep-nesting stack overflow (unlike the 2026-07-03 catch below). Suspected: a
   level-triggered IRQ on vector 11 re-firing / storming (not ACKed/masked), or an
   ISR-reentrancy lock spin.
   **STATUS 2026-07-15:** after the container `TABLE` holder-preemption fix (which
   was the iter03 catch), a **40-iteration soak came back 40/40 clean**
   (`build/soak-postfix2.log`, `WEDGE_SOAK_DONE rc_caught=0`) — this IF=0
   device-IRQ signature did **not** reproduce even once. Given the earlier soaks
   caught wedges by iter03/12/22 and this one caught none in 40, the intermittent
   boot wedge is resolved for practical purposes. This IF=0 catch was seen exactly
   once and is not currently reproducible; it may have been a downstream symptom of
   the same task-starvation the holder-preemption deadlocks caused (a spinning,
   never-preempted CPU can leave a device IRQ un-serviced), or a genuinely
   ultra-rare separate race. **Not claiming it fixed** (never root-caused), but it
   is no longer an active blocker. Re-open if a future soak reproduces it — the
   crash-dump guard-page fix means the next catch will have a full task-table dump
   to work from.

2. **SECONDARY (FIXED this session) — crash-dump stack-scan over-read.** The wedge
   was caught, but the hard-lockup crash dump then took a **fatal `#PF` at
   `0xffffc10000028000`** (the IRQ-stack guard page), which *destroyed* the
   task-table dump needed to root-cause the primary. Cause: `dump_kernel_backtrace`
   (idt.rs) scans/chases words **upward** from the wedged `rsp` (a 256-word / 2 KiB
   stack scan, plus the rbp-chain walk) with **no bound against the IRQ-stack
   top** — with `rsp` only 0x418 below the top, the scan ran straight into the
   guard page. Fix: added `irq_stack_top_for(addr)` (returns the current CPU's
   IRQ-stack top iff `addr` is on that stack, else 0) and applied it to **both**
   walkers — the stack scan caps at `irq_top` before each read; the rbp-walk checks
   per-iteration (the chain legitimately crosses off the IRQ stack onto the boot
   stack, where the helper returns 0 → no cap). Now a wedge near the IRQ-stack top
   dumps cleanly instead of double-faulting, unblocking diagnosis of the primary.
   Built clean, BOOT_OK, committed.

**UPDATE 2026-07-15 (SIXTH catch — crash-dump fix VALIDATED; SECOND holder-
preemption instance, on the container `TABLE` lock; FIXED)
(`build/hang-catches/soak-20260715-070017-iter03.*`, wedged
`RIP=0xffffffff81056446`).** With the crash-dump over-read fixed, a 30-iter soak
caught a wedge on iter03 and this time **the crash dump survived cleanly** (no
guard-page `#PF`) — confirming the idt.rs fix. The dump gives a conclusive
picture, and it is a **third-party of the same holder-preemption class** as the
heap deadlock, but on a *different* raw `spin::Mutex`:
- **`RFL=0x202` → IF=1** (interrupts *enabled*) — so this is NOT the iter12 IF=0
  device-IRQ hang; that remains a separate open item above.
- **Wedged RIP = `spin_loop_hint+0x6`** with one sample in `AtomicBool::load` — a
  busy-wait in a spinlock acquire loop.
- **rbp backtrace:** `syscall_entry → syscall_handler_inner → sys_exit →
  on_thread_exit → remove_thread → notify_init_exit+0xbf` — i.e. an exiting
  thread's `sys_exit` path, spinning on `container::TABLE.lock()` (container.rs:1438).
- **Task table (survived!):** `tid=189 "/tmp/restart-init.elf" state=Running
  prio=16` is the spinner; **`tid=0 "prctl-batch269" state=Ready prio=31,
  waited=1735 ticks`** is a *higher*-priority task that is Ready but never
  scheduled — the smoking gun that the spinner's context is non-preemptible and
  the Ready TABLE holder can't run to release the lock. `heap-lock: unlocked`
  (so the heap fix held; this is a *different* lock).

**Root cause (proven).** `container::TABLE` (and `EVENT_LOG`) used a **raw
`spin::Mutex`** (`use spin::Mutex`, container.rs:47), which — like the heap lock
before its fix — does **not** disable preemption on acquire. A container operation
(tid=0) held TABLE and was involuntarily preempted mid-critical-section; a process
exiting via `remove_thread → notify_init_exit` then spun on `TABLE.lock()` forever
while the Ready holder (tid=0) could never be scheduled on the single CPU. Note
`remove_thread` correctly **drops `PROCESS_TABLE` before** calling
`notify_init_exit` (pcb.rs:2039), so this is *not* a lock-ordering/nesting bug —
it is purely the missing preempt-disable on the raw spinlock.

**Fix (committed).** Converted `container::TABLE` and `EVENT_LOG` from raw
`spin::Mutex` to the preempt-aware `crate::sync::Mutex` (named for lockdep:
`container-tbl` / `container-evt`). The tracked mutex calls `preempt_disable()` on
acquire, so a holder can never be preempted mid-critical-section → the hold is
bounded and short → the exit-path spinner finds TABLE free almost immediately. As
a bonus this adds lockdep coverage (which would have *caught* this) and owner
tracking. Safe because container locks are only taken in task context (never ISR:
`restart_backoff_fire` runs in the hrtimer ISR but only submits to the workqueue),
and `EVENT_LOG` is a leaf (never acquires TABLE), so no ordering inversion.

**SYSTEMIC NOTE — raw `spin::Mutex` holder-preemption is a latent class, not a
one-off.** Confirmed instances: heap, container `TABLE` (holder-preemption);
`sysctl::REGISTRY`, completion-timer→`SCHED` (interrupt-reentrancy). The kernel
has ~476 files importing `spin::` — any raw `spin::Mutex` whose critical section
can be involuntarily preempted *and* is contended across tasks is a latent
holder-preemption deadlock on a single CPU. Most are safe (true leaf locks,
trivially short sections, or never contended under preemption).

**STRATEGY UPDATE 2026-07-18 — Q24 RESOLVED: operator chose the PROACTIVE
audit/conversion (option B), see `design-decisions.md` §70.** The prior
reactive-only strategy (below) is superseded. We are now deliberately eliminating
the class rather than waiting for the soak to surface each instance. Execution is
per-lock triage (NOT a blind sed):
- **Hot / cold leaf locks** (the ~230 `fs/*.rs` procfs config & stat stores, and
  other true leaves) → the new **`PreemptSpinMutex`** in `kernel/src/sync.rs`
  (commit 03cccdd5f): `preempt_disable` around a raw `spin::Mutex`, no lockdep,
  shares the stall detector. Closes holder-preemption without dragging cold locks
  into lockdep. Conversion is a per-file import swap
  (`use spin::Mutex;` → `use crate::sync::PreemptSpinMutex as Mutex;`); the
  compiler validates API compat, and converting is strictly safe (it can only
  *introduce* a problem if the section already sleeps while holding the lock,
  which would already be a latent deadlock under raw spin).
- **Contended, non-leaf / ordering-sensitive locks** (core FS: `vfs`, `handle`,
  `fdtable`, `pipe`, `overlay`, `ext4/*`, `memfs`, `cache`, mount/notify families;
  and cross-subsystem locks) → `crate::sync::Mutex` (full lockdep + owner
  tracking) so ordering bugs are caught.
- **Deliberately-raw** (global heap lock — lockdep can't allocate under it; SCHED)
  → keep raw + manual `preempt_disable`/`enable`.
- IRQ-context acquirers stay on `try_lock`/`without_interrupts` (don't regress the
  already-clean interrupt-reentrancy surface).

**Rollout progress (running checklist):**
- [x] `PreemptSpinMutex` primitive + self-tests (Test 5/6), boot-green (03cccdd5f).
- [x] Convert `fs/*.rs` cold leaf config/stat stores → `PreemptSpinMutex`.
  **DONE 2026-08-17** — 35 lock sites across 21 files; `kernel/src/fs` now holds
  zero raw `spin::Mutex`. See the [A] note below.
- [ ] Route core-FS contended locks → `crate::sync::Mutex` (lockdep), triage the
  lock-ordering reports that surface.
- [-] Sweep non-fs subsystem raw locks (`net/`, `ipc/`, drivers) per the same
  triage. **Partially triaged 2026-08-17** (see [A]). **Audio sub-batch DONE
  2026-08-17** — `ac97`, `audio_history`, `audio_mixer`, `hda`, `virtio/sound`
  (6 lock declarations across 5 files), see the [C] note below; **37 files still
  hold a raw `spin::Mutex`**, excluding the four that are deliberately raw
  (`sync.rs`, `mm/heap.rs`, `serial.rs`, `bench.rs`). Unlike the `fs/` batch,
  every audio file needed its critical section restructured, not just its lock
  type swapped — budget for that in the driver files that remain.
- [ ] Final full wedge-soak (switch-off + switch-on) clean.

**[A] 2026-08-17 — the `fs/` batch is finished, and auditing it first found
three real bugs that a blind import swap would have preserved.**

*Scale, measured rather than estimated.* The checklist above feared "~230
files". A counted inventory found **88 raw-lock sites across 59 files**
kernel-wide, of which `fs/` held **35 across 21 files**. Those 35 are now
`PreemptSpinMutex` and `kernel/src/fs` contains no raw `spin::Mutex` at all.
The old estimate counted files *touched by* the fs conversion, not files still
holding a raw lock.

*A caution about that inventory, because the first version of it was wrong.*
Counting `static NAME: Mutex<…>` alone **undercounts**, and it undercounted
here by 13 sites and 9 whole files. A raw lock is not always a static:
`mm/heap.rs` holds its as a struct *field* (`inner: Mutex<HeapInner>`), and
others are `Mutex::new` bindings. `ahci`, `nvme`, `audio_mixer`, `hrtimer`,
`sched/kchannel`, `sched/priority_rr`, `sched/waitqueue`, `virtio/gpu` and
`virtio/sound` are invisible to a static-only scan and appear only when struct
fields and constructor calls are counted too. Any future batch must scan for
all three shapes, and must resolve `use spin::Mutex` vs `use crate::sync::Mutex`
per file — a bare `Mutex<…>` means different things in different files.

*`named`, not `new`.* Every converted site uses `PreemptSpinMutex::named(v,
b"NAME")`. `new` leaves the diagnostic name as `?`, and these are precisely the
locks a silent wedge would otherwise hide in — converting a lock for its
self-reporting and then leaving it anonymous would throw away half the benefit
this sweep is being done for (see B-FORKEXEC-BOOT-HANG's static audit).

*Three genuine defects, found by auditing critical sections before converting.*
The conversion is advertised as a mechanical import swap, and it is — but
"is this lock actually a leaf?" has to be answered per lock, and answering it
turned up three places calling the VFS while holding a raw spinlock:

| Site | Was |
|---|---|
| `fs/bookmarks.rs::validate` | `Vfs::metadata` once **per bookmark** under `BOOKMARKS` |
| `fs/thumbcache.rs::get` | `Vfs::metadata` under `CACHE` |
| `fs/fileops.rs::create` | `Vfs::metadata` once **per source** under `OPERATIONS` |

`Vfs::metadata` walks the mount table, takes filesystem locks of its own and can
block on the backing device, so each of these held a leaf lock across an
unbounded I/O path *and* put its acquisition order ahead of the VFS's. All three
now snapshot under the lock and call out unlocked — the shape `freeze::freeze`
and `fileops::undo` already used in the same directory, which is what made it
obvious these three were oversights rather than deliberate.

Two follow-on fixes fell out of the restructure:

- **`thumbcache::get` never invalidated a stale entry.** The comment said
  "Source changed — invalidate" and the code only returned `None`. The entry
  stayed cached, so every later lookup of that path re-failed validation and
  re-paid the metadata call, with its bytes still counted in `MEMORY_USED`,
  until LRU happened to evict it. It is now actually removed and its bytes
  returned.
- **`fileops::create`'s admission check had to be re-tested under the lock.**
  With the metadata walk hoisted out, the early `MAX_OPERATIONS` check no longer
  runs under the lock the operation is pushed under, so another caller could
  take the last slot in between. The early check is now advisory and a binding
  re-check guards the push; the id is allocated only after admission is certain,
  so a rejected create burns no id.

*The bug hid behind an empty test.* `thumbcache`'s `test_store_and_get` never
called `get()` — it asserted `count() == 1`, explained in a comment that
validation would fail, then asserted `count() == 1` again. The validation path
had no coverage at all, which is why a stale entry could be left behind
indefinitely without a test noticing. `test_get_validation` now covers all three
outcomes against a real file under `/tmp`: source unreadable (trust it — an
unreadable source must not be mistaken for a changed one), source changed (miss
**and** evict **and** `memory_used()` returns to its prior value), source
unchanged (hit). The third case is not redundant: the restructure introduced a
window in which the entry can be evicted between the unlocked metadata call and
the re-acquire, so "a matching stamp still hits" is a claim the old single-lock
code did not have to make and the new code does.

*And the new test did not run either — the first boot proved nothing.* The
boot after this conversion passed, and the pass was **worthless as evidence**:
grepping the serial log for the expected `[thumbcache]` line found nothing,
because `fs::thumbcache::self_test` had no caller anywhere in the tree. Checking
the rest of the batch found the same thing for **all 21** converted modules —
every one had a `self_test()` already written, and not one was reachable from
the boot path. The conversion would have shipped with literally zero automated
coverage while appearing green. All 21 are now called from `main.rs`, next to
the `locale`/`timezone` pair that a previous session wired up for exactly this
reason, with the same note attached: *a test that never runs is not a test.*
This is the third time this trap has been hit here (`bytestr::self_test`,
lockdep Test 11, now these), so the rule is worth stating flatly: **a green boot
only certifies what the serial log can be shown to contain.** Verify the
expected output is present; never infer that a test passed from the absence of
a failure.

Wiring them in then exposed a second gap of the same kind: of the three
restructured critical sections, `fileops::create` was covered (its "create
operation" case) and `thumbcache::get` now is, but **`bookmarks::validate` had
no test whatsoever** — the very function that was doing a `Vfs::metadata` walk
per bookmark under the lock. `test_validate` now covers both outcomes (a
resolvable and an unresolvable path), matched **by name rather than by
position**, and asserts the snapshot neither drops nor duplicates entries —
which is the specific way a snapshot-then-call-unlocked rewrite can go wrong.

It is not confined to this batch, and the wider problem is logged separately
as [B] below: ~60 more `fs/` self-tests have no caller at all, and a further
~230 are reachable only by typing a `kshell` subcommand, i.e. never in CI.

*Partial triage of the remainder.* This was a read-only pass over the **statics**
only, so it is a starting point for the next batch, not a complete map — the
struct-field and `Mutex::new` sites called out above are **not** yet triaged.

- **Clean leaves, safe to convert:** `ac97`, `audio_history`, `cap/request`,
  `hda`, `iommu`, `iommu_remap`, `klog`, `xhci`, `proc/signal`, and `net/httpd`'s
  `LISTENER`/`TLS_LISTENER`.
- **Need individual design work, not a batch swap:** `tlb.rs::SHOOTDOWN_LOCK`
  (held across an IPI broadcast — preempt-disabling it is desirable but it is a
  cross-CPU protocol, not a leaf); `workqueue.rs::worker_entry` (the only
  sleep-under-lock hit in the whole sweep, and it wraps its lock in
  `without_interrupts`); `rcu.rs::CALLBACKS` (reachable from too many contexts).
- **Staying raw, correctly:** `mm/heap.rs` (lockdep cannot allocate under the
  allocator's own lock), `serial.rs` (the printer lockdep prints through),
  `bench.rs`'s `RAW`, which is deliberately the uninstrumented control that
  `lock_tracked_nested` measures against, and the two sites inside `sync.rs`
  itself, which are the primitives `Mutex`/`PreemptSpinMutex` are built out of.

*A note on the audit method.* The scope approximation was deliberately
conservative — guard binding to end of enclosing function — so it over-reported:
`fileops::undo`, `freeze::freeze`, `fileops::move_item` and `httpd::tick` all
flagged and all turned out to drop the guard before calling out (`let listener =
*LISTENER.lock();` drops its temporary at the end of the statement). That noise
is the price of the same over-reporting that surfaced the three real defects; a
tighter heuristic would have produced a cleaner report and missed them.

*Verification.* `cargo build -p kernel` clean (zero warnings). `cargo clippy -p
kernel` exit 0, with no new warnings attributable to the conversion or the
hoists (`fileops` 0 hits, `bookmarks`' 7 all predate this change). Boot test **PASSED** (clean streak 5) — and, this time, verifiably: the serial log contains `[thumbcache]   get_validation: ok`, `[bookmarks]   validate: ok` and self-test output from all 21 converted modules, with zero `self-test failed` warnings.

**[B] 2026-08-17 — most `fs/` self-tests are never executed. TECH DEBT, open.**

Found while checking why [A]'s new test produced no serial output. `fs/` has
**433** modules defining a `pub fn self_test()` (excluding `mod.rs`/`tests.rs`).
Counted before [A] wired its 21:

| Category | Before [A] | After [A] | Runs unattended? |
|---|---|---|---|
| Called from `main.rs` | 144 | 165 | yes, every boot |
| Reachable only via a `kshell` subcommand | 230 | 230 | **no** — needs a human to type it |
| No caller anywhere in the tree | 59 | 38 | **no** — dead code |

So **two thirds of `fs/`'s self-tests never run** unless someone sits at the
kernel shell and invokes them by hand, which no CI path does.

The 38 with no caller are the serious ones: they are dead code that the
compiler keeps alive only because they are `pub`, so they rot silently and
nothing detects it. `thumbcache` was in this set, and its `test_store_and_get`
had degenerated into a test that asserts the same thing twice and never calls
the function it names — which is precisely how the stale-entry leak in [A]
survived. Expect more of the same in the rest of the set.

The remaining 38, in full, so the next session need not re-derive them:
`archive`, `backup`, `batch`, `changetrack`, `contextmenu`, `cpufreq`,
`cputopo`, `dedup`, `deskicons`, `dirsync`, `diskio`, `encrypt`, `fcompress`,
`fileselect`, `filetype`, `fswalk`, `health`, `ioprio`, `iso9660`, `linkcheck`,
`openwith`, `policy`, `powerwake`, `properties`, `readdir_plus`, `reclaim`,
`search`, `sidebar`, `snapshot`, `splice`, `statusbar`, `sysctlfs`, `sysuptime`,
`tags`, `thermal`, `transaction`, `undelete`, `usage`. (The 21 from [A] were in
this set too and are now wired.)

*The proper fix is not 60 more hand-written call sites.* `main.rs` already
carries several hundred lines of copy-pasted `if let Err(e) = fs::x::self_test()`,
which is how the gap opened in the first place: adding a module and forgetting
the call site is silent, and nothing anywhere asserts the two lists agree. What
is wanted is a registry — a `linkme`/`inventory`-style distributed slice, or a
declarative macro that emits both the call and the registry entry — so that
defining a self-test *is* registering it and the failure mode becomes impossible
rather than merely unlikely. `selftest.rs` already has a `TestSuite` table with
the right shape (`name`, `description`, `run`, `category`); it is populated by
hand and has drifted from `main.rs` in the same way. Unifying the two behind one
registry is the actual task.

*Interim guard, cheaper than the registry and worth doing first:* a build-time
or boot-time check that every `pub fn self_test` in `fs/` is reachable from the
registry, failing loudly when it is not. Without it this list will regrow.

Not fixed here because it is a much larger change than the lock sweep it was
found inside, and mixing them would make both unreviewable.

**[C] 2026-08-17 — the audio batch: 6 lock declarations across 5 files, and
every one of them needed a restructure rather than an import swap.**

The `fs/` batch in [A] was overwhelmingly a mechanical conversion with three
exceptions. The audio batch is the inverse: **all five files** needed the
critical section changed, not just the lock type, and one of them
(`hda`) is best served by removing the lock from the caller entirely. This is
worth recording because it says something about where the remaining 37 files
are likely to sit: the closer a lock is to hardware, the less likely the swap
is mechanical.

| File | Lock(s) | What was actually wrong |
|---|---|---|
| `ac97.rs` | `DEVICE` | Held across `busy_wait_us(duration_ms × 1000)` — the **entire tone** |
| `virtio/sound.rs` | `DEVICE` | Held across the whole tone, incl. a 10 M-iteration used-ring poll *per chunk* |
| `audio_history.rs` | `HISTORY` | `String`/`Vec` allocation under the lock, up to 64 times per call; plus UTF-8 UB |
| `audio_mixer.rs` | `StreamSlot::{name, ring}` | `Vec::push` with a ring guard alive in argument position; plus UTF-8 UB |
| `hda.rs` | `DEVICE` | Taken from `handle_irq` — interrupt reentrancy |

### Why a blind swap would have been actively harmful here

`PreemptSpinMutex` disables preemption for the length of the hold. For a leaf
lock held for a few hundred instructions that is exactly right. For
`ac97::play_test_tone`, which held its lock across a multi-millisecond busy
wait, converting *without* restructuring would have **made things worse** —
turning a lock that merely blocked other callers into one that also stopped the
scheduler on that CPU for the whole tone. The conversion is only "strictly
safe" (checklist wording above) for sections that are already short. Deciding
whether a section is short is per-lock work and cannot be delegated to the
compiler.

Both tone paths were restructured to the same shape: **claim under the lock,
release, do the long thing, re-acquire to tear down.** The claim
(`ac97: dev.playing`, `virtio-snd: dev.active_stream`) is what provides mutual
exclusion over the tone; the lock now only covers individual transactions.

### The ABA hole that shape opens, and how it is closed

Releasing the lock mid-operation means the claim can be taken by the one caller
entitled to take it — `stop()`, which exists precisely to end a tone early. A
naive re-check (`is my claim still set?`) is **not** sufficient:

```
caller 1: claim ──────────── wait ─────────────── "still claimed, tear down"
caller 2:        stop()  play_test_tone() claim ────────► destroyed by caller 1
```

`playing` is `true` again and `active_stream` is `Some(0)` again — playback
always uses stream 0 — so caller 1 cannot tell caller 2's claim from its own.
Both drivers therefore carry a monotonic `play_gen: u64`, bumped under the lock
at claim time; a caller compares the generation it recorded, and on a mismatch
ends quietly without a second teardown (a second `RELEASE` on a released
virtio-snd stream is a protocol error, and a second `stop_playback_inner` would
cut off the new tone).

This is the part that would not have been found by looking at lock *types*. It
only appears once you ask what the lock was doing for you that it no longer
does.

### `hda`: the handler gets atomics, not `lock_irqsave`

`handle_irq()` took `DEVICE`, which self-deadlocks against a task-context
holder on the same CPU the moment the handler is wired to the IOAPIC — latent
today only because nothing calls it yet. The obvious fix, `lock_irqsave` at all
nine sites, was implemented and then **rejected**: `configure_output()` holds
`DEVICE` across two 10 ms stream-reset polls and six `send_verb` calls that each
poll the RIRB for up to `CODEC_RESPONSE_TIMEOUT_US` (50 ms) — ~320 ms with
interrupts off, reached exactly when a codec has stopped answering.

The handler needs three values (`mmio_base`, `iss`, `out_stream_idx`), all
written once in `init()` and never mutated. They are now published in
`IRQ_MMIO_BASE`/`IRQ_STREAM_IDX` (index stored first, base stored last with
`Release`; base read first with `Acquire`, so a non-zero base implies a valid
index), and `handle_irq` takes no lock at all. `DEVICE` stays a plain
task-context-only `PreemptSpinMutex`. Rationale and the register-overlap check
that makes concurrent handler/task execution safe: `design-decisions.md` §221.

`configure_output` still holds `DEVICE` across all six verbs, and that is
correct rather than an oversight: `send_verb` advances `corb_wp`/`rirb_rp`, the
shared CORB/RIRB ring pointers, so two interleaved sequences would desync the
response ring and read each other's replies. Those verbs are one indivisible
configuration. The 320 ms bound is unchanged but is now paid in preemption
rather than in interrupts.

### Two memory-safety bugs found on the way

Both drivers copy a caller-supplied name into a fixed byte array and truncate:

```rust
let copy_len = name.len().min(NAME_LEN - 1);
dst[..copy_len].copy_from_slice(&name.as_bytes()[..copy_len]);
```

Truncating at a **byte** offset splits any multi-byte UTF-8 character that
straddles the limit, leaving an invalid sequence in the buffer.
`audio_history::HistoryEntry::name_str` then handed that buffer to
`core::str::from_utf8_unchecked` — undefined behaviour, justified in a comment
by "we only store valid UTF-8 names (from kernel code)", which is true of the
*input* and not of what was stored. Both sites now walk down to a character
boundary with `is_char_boundary`, and `name_str` is checked
(`from_utf8(..).unwrap_or("")`) so a malformed buffer from any other source
degrades to an empty name instead of UB.

### Allocation under a spinlock, including one that hides in plain sight

`audio_history::recent` built its `Vec<(String, …)>` **inside** the `HISTORY`
critical section — up to 64 `String` allocations, nesting the kernel heap lock
under a leaf spinlock and making the section's length depend on allocator state
(including an OOM reclaim). It now `Copy`-snapshots the fixed-size entries into
a stack array under the lock and allocates after releasing it; `HistoryEntry`
gained `Copy` for exactly that.

`audio_mixer::list_streams` had the subtler form:

```rust
result.push((i as StreamId, …, slot.ring.lock().len()));   // guard lives to `;`
```

A temporary guard in an argument position lives until the end of the enclosing
**statement**, not the sub-expression — so the ring stayed locked across a
`Vec::push` that can allocate. Bound to a local first. Worth internalising as a
pattern: `container.push(… lock().x …)` is always this bug.

### Inventory after this batch

| | Files | Note |
|---|---|---|
| Before | 42 | excluding the four deliberately-raw |
| Converted here | 5 | `ac97`, `audio_history`, `audio_mixer`, `hda`, `virtio/sound` |
| **Remaining** | **37** | plus `sync.rs`, `mm/heap.rs`, `serial.rs`, `bench.rs` (deliberately raw) |

Still-flagged allocation-under-spinlock, unconverted, for whoever takes the next
batch: `cap/request.rs:148` (`String::from`) and `sched/mod.rs` lines 1258,
1303, 1553 (`Box::new` into `state.tasks`).

**Superseded reactive strategy (kept for context):** the armed hang soak
(`scripts/wedge-soak.sh`) reliably reproduces these under stress; each catch names
the exact lock via the backtrace; convert *that* lock to `crate::sync::Mutex`
(or, for true leaf/allocation locks like the heap, keep raw + manual
preempt_disable/enable). This remains the fallback if the proactive sweep is
paused.

**END-TO-END VALIDATION 2026-07-15 — ring-3 socket capstone passes on the
switch-on spawn-heavy path.** With both holder-preemption fixes in, the deferred
netstack Phase-5.6 ring-3 HTTP capstone (`services/httpget`, a Linux-ABI ring-3
ELF doing raw `socket()`/`connect()`/`write()`/`read()`/`close()` over the
daemon-backed fd path, spawned by `run_persistent_netstack`) now boots and runs
green switch-on: `Created process 228 ("httpget")` -> `[httpget] connected` ->
`[httpget] OK: HTTP response` -> `ring3 HTTP capstone: OK ... (exit 0)`. This is
the spawn-heavy container-exec/ring-3 dispatch path that previously wedged; a
15/15-clean switch-on wedge soak plus this live spawn confirm the container-exec /
ring-3 spawn-dispatch race is resolved for practical purposes. (The self-test is
bounded by a 15 s Zombie-poll deadline so a stuck fetch can never wedge the boot.)

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
- **Bug-class audit (2026-08-13, done — do not redo).** Since `ACCT` and
  `TSC_FREQ` were two instances of one class (*a lock reachable from interrupt
  context*), the ISR-reachable surface was swept for further instances. Result:
  **clean — TSC_FREQ was the sole remaining outlier.** Verified:
  - `kernel/src/timekeeping.rs` and `kernel/src/apic.rs` now contain **zero**
    `.lock()` calls — the whole clock read path is lock-free and NMI-safe.
  - `sched::timer_tick` (the 100 Hz path, `sched/mod.rs:2822-2996`) takes only
    `try_lock` + atomics: `SCHED.try_lock`, `PER_CPU_SCHED.tick` (try_lock, and
    degrades safely if normal code holds the per-CPU queue lock),
    `cgroup::cpu_charge` (try_lock), `rcu::quiescent_state` (atomic).
  - `handle_nmi` (`idt.rs:1526`) helpers are all lock-free: `count_vector`
    (atomic), `sched::bsp_heartbeat` (atomic), `smp::current_cpu_index`
    (rdtscp/atomic), `hardlockup::kick_staleness_ns` (now lock-free).
  - `sched::account_fault` (page-fault path) is `try_lock`-guarded.
  - The other scalar/getter-shaped statics (`HPET_TABLE_PHYS`, `FADT_TABLE_PHYS`,
    the `fs/*` `INITIALIZED` flags, `net` `LISTENER`/`NEXT_PORT`) are **not**
    ISR-reachable — e.g. `acpi::hpet_table_phys()` is called only from
    `hpet::init()`.
  - **Signature to watch for in future:** the danger is not an obvious
    `FOO.lock()` inside an ISR — those are already disciplined — but a *hidden*
    lock inside an innocent-looking **getter** (`tsc_freq()` looked like a pure
    read). When adding a lock to any leaf accessor, check whether an ISR can
    reach it.
- **Bearing on the intermittent-hang family (hypothesis, NOT yet closed).** This
  wedge is a strong candidate root cause for the long-running "intermittent
  silent boot hang, serial truncated at whatever self-test happened to be
  running" family — `B-FORKEXEC-BOOT-HANG`, `W1` (OOM self-test), and the OCI
  multi-stage catch above. It fits every observed property: a *shared low-level
  primitive* (`clock_monotonic` is called from nearly everywhere) rather than any
  one test's logic; non-deterministic because it needs an IRQ to land in a narrow
  lock-hold window; a different freeze locus each occurrence; total silence with
  no `#PF`/PANIC (a spin, not a fault); and IF=0, which blinds the timer-driven
  watchdogs. `W1` had already independently concluded "traced to spin::Mutex /
  interrupt-window timing … the OOM code is the *victim*, not the cause."
  **Those entries are deliberately left OPEN** — 12 clean boots does not
  statistically discriminate bugs whose historical rate was ~1-in-20+. If any of
  them recurs, the HMP RIP-capture will now name the wedged instruction directly.
