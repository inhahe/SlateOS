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

### TD4. Monolithic `syscall::linux::self_test()` has an unbounded boot-stack frame — OPEN 2026-06-12

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
