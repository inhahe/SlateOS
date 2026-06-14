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

**Status:** passive monitoring, clean streak reset to 0 by the 2026-06-12
recurrence. **Closure condition unchanged:** close this item (move to
Fixed/Closed as "likely cured incidentally," like F6/F7) once a fresh
combined dedicated-soak + routine-boot clean streak passes ~90 with no
recurrence. Re-open and bisect immediately on the next mid-self-test
truncation; given two recorded recurrences now, a finer-grained marker
pass around the `mm::oom::self_test()` / `sysctl::set` lock window
(per the F1/F4 method) is the priority diagnostic when next observed.

_(No other active bugs.  The two prior watchlist items — accounting
self-test hang and invariant self-test hang — went 90 consecutive
boot tests with zero recurrence after F4/F5 and have been closed as
"likely cured incidentally," and as of 2026-06-10 a further 38 clean
boots (128/128 total) keep them closed.  See F6 and F7 in Fixed Bugs.
The two items discovered 2026-06-10 — quota Test 5 and FS interceptor
deny — are now fixed; see F8 and F9.)_

---

## Fixed Bugs

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

### F6. Accounting self-test hang — LIKELY CURED INCIDENTALLY 2026-06-07

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

### TD22. File-backed `mmap` — Phase 1 (demand-paged `MAP_PRIVATE`) DONE; page cache + shared write-back still DEBT — PARTIAL 2026-06-14

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

**Proper fix (Phase 2):** introduce a unified page cache shared between the VFS
read path and mmap. Resolve `MAP_SHARED` faults to the shared cache page (mapped
writable for `PROT_WRITE`), add dirty-page tracking + `msync`/unmap write-back,
and switch `MAP_PRIVATE` to map the cache page CoW (copy only on write) instead
of read-copying into a fresh frame. This is a foundational architectural fork
(it touches the VFS, the frame allocator's ownership model, and writeback) and
is logged for operator input in `open-questions.md`.

---

### TD21. Minor Linux-ABI fidelity gaps — procfs fd visibility for native processes; sendfile pos write-back — APPROXIMATION 2026-06-13

**Where:** `kernel/src/fs/procfs` (`/proc/<pid>/fd[info]`, `linux_fd_list`) and
`kernel/src/syscall/linux.rs` (`sys_sendfile`). Both are documented in-code.

**What it is:** two small, deliberate Linux-ABI approximations:
- **`/proc/<pid>/fd/` and `/fdinfo/` are EMPTY for *native* processes.** Native
  processes keep their fd table in userspace (`posix/src/fdtable.rs`), which is
  not kernel-visible, so `linux_fd_list` returns `None` and the readdir yields
  zero entries rather than inventing fds. Only Linux-ABI processes (which use the
  kernel-side `KernelFdTable`) get a populated `fd/`. Same honesty stance as the
  fdinfo `mnt_id:`/`ino:` omission — printing fabricated fds would mislead
  introspection tools.
- **sendfile `put_user(pos, offset)` write-back EFAULT is not modelled.** In Linux
  this write-back runs unconditionally after the transfer and can override a
  success/EBADF result; we don't model it because the sendfile transfer itself is
  unimplemented (the call terminates EINVAL). The leading `validate_user_read`
  already rejects a wholly-unmapped offset pointer, matching the dominant failure
  mode.

**Impact:** low — native-process fd introspection via `/proc` is unavailable
(tools must use the native fd API); the sendfile gap only matters once sendfile
transfer is implemented.

**Proper fix:** procfs fd — expose a kernel-visible view of native fd tables (or a
read bridge into the userspace fd table) so `/proc/<pid>/fd` works uniformly.
sendfile — model the trailing `put_user(pos)` write-back when the sendfile data
path is actually implemented.

### TD20. Userspace crate verification & lint-cleanup gaps — DEBT 2026-06-13

**Where:** `userspace/coreutils/` (and any userspace crate whose *test* code uses
`std::os::unix`), and `gui/toolkit/` (guitk).

**What it is:** two low-priority verification/lint gaps in userspace crates:
- **coreutils host-test gap (2026-05-31):** coreutils unit tests cannot compile on
  the Windows dev host (`x86_64-pc-windows-gnu`) because bins like `test.rs`/
  `tar.rs` use `std::os::unix::fs::{PermissionsExt, MetadataExt}` (mode/uid/gid/
  mtime), which only exist on unix-family targets. The slateos target (os=linux)
  *has* them, so production compiles fine (`cargo build -p coreutils` for slateos
  is clean), but slateos test binaries can't be *run* on this Windows box — so
  coreutils' verification path is the slateos build, not host `cargo test`. Not a
  code bug; a cross-platform host-testing gap that applies to any userspace crate
  using `std::os::unix` in its test code.
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

**Proper fix:** coreutils — gate the `std::os::unix` test code behind `#[cfg(unix)]`
with inert host stubs (the pattern already used by `stat`/`stty`) so host
`cargo test` at least compiles, or stand up a slateos test runner. guitk — a
dedicated pedantic-cleanup sweep once the core ABI stabilizes, resolved together
with the TD19 lint-policy decision.

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
  *writes*. No net-config write syscall exists; only `NET_IF_INFO=842` /
  `ARP_TABLE=843` are present and read-only. Precise harm (traced 2026-06-01):
  with a Socket-WRITE cap the call silently binds+leaks a UDP socket on a low
  port and misleads the user that the config change applied; without the cap it
  fails. The intended operation never happens either way.
- **mount/umount**: no `MOUNT`/`UMOUNT` syscall (620/621 are
  `FS_TRASH_RESTORE`/`FS_TRASH_EMPTY`).
- **mkfs/fsck/diskutil**: no `FS_FORMAT`/`FS_VERIFY`/`FS_REPAIR`/`FS_TRIM`
  syscall (650–655 are `SEEK_DATA`/`SEEK_HOLE` + unassigned).
- **chroot**: no `CHROOT`/`CHDIR`/`SETUID`/`SETGID`/`SETGROUPS` syscall — needs a
  real process-credential + filesystem-root ABI.

**Impact:** these specific tools are non-functional (no-op at best). They are not
on any critical path, so nothing currently blocks on them.

**Proper fix:** this is an **operator design decision**, not a mechanical fix —
the kernel must first grow the missing ABI, and the *shape* of that ABI is a
fork: a native net-config syscall family vs. a network-manager IPC daemon for
the net tools; a real mount/umount + fs-admin (format/verify/repair) syscall set;
and a process-credential + fs-root ABI for chroot. A partial near-term win that
needs no decision: wire the net tools' **read** paths (`ifconfig` no-args, `ip
addr show`, `route -n`) to `NET_IF_INFO=842`. Trigger to revisit: when the
matching kernel syscalls land (track via roadmap net-config / mount / fs-admin
tasks). Related: `sys_clock_settime`/`sys_clock_adjtime` now enforce
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
accepted-but-ignored control bits. Linux FS create/delete syscalls
(`mkdir`/`mkdirat`/`rmdir`/`unlink`/`unlinkat`) now route through the native VFS
(`Vfs::mkdir`/`rmdir`/`remove`), so inotify events DO flow from those Linux-ABI
operations; the `rename`/`renameat`/`renameat2` family is still stubbed (ENOENT),
so renames via the Linux ABI do not yet emit `IN_MOVED_FROM`/`IN_MOVED_TO`.

**Impact:** low — the common "watch a dir for create/delete/modify/move/open/close"
file-manager/build-tool idiom is fully covered, now including the `IN_ISDIR`
dir-flag. Remaining gaps bite only apps that need inode-identity semantics across
delete+recreate (rare) or inotify events from Linux-ABI mutation syscalls.

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

**Remaining fix:** (a) route the Linux-ABI **rename** family
(`rename`/`renameat`/`renameat2`) through `Vfs::rename` so renames emit
`IN_MOVED_FROM`/`IN_MOVED_TO` (create/delete done 2026-06-14 via `resolve_at_path`
+ `require_fs_write` + `Vfs::mkdir`/`rmdir`/`remove`); (b) switch watch identity to
inode if/when stable inode numbers are available.

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

### TD9. Linux program interpreter (ld.so) loaded at a fixed base — no ASLR — DEBT 2026-06-12

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

**Related limitation (not debt, just unimplemented):** end-to-end
interpreter *execution* is untested because no real glibc/musl ld.so is
on the filesystem yet.  The load mechanism (base selection, biased
segment mapping via `load_segments_with_bias`, AT_BASE/AT_ENTRY auxv) is
unit-tested via `spawn::test_load_interpreter_fallbacks` (static-ELF and
absent-interpreter `Ok(None)` fallbacks).  See `todo.txt` "Linux
dynamic-linker (ld.so) load path".

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

**Residual divergence (documented, not debt):** Linux resets `membarrier_state`
to 0 on `execve` (`membarrier_exec_mmap`); we lack an exec-time PCB-reset hook
(the same gap already noted for `linux_dumpable`/`linux_keepcaps`), so a
registration currently survives exec. Tracked in `todo.txt`. The in-kernel
self-test caller (no owner mm) keeps the pre-TD8 "fence/0" behaviour by feeding
`u32::MAX` to the gating helper — there is no registration model for a kernel
thread with no sibling userspace threads.

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

### TD2. Clippy `clippy::all` deny-level errors not yet zeroed — RESOLVED 2026-06-10

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
