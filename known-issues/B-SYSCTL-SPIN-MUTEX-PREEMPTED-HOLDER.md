### B-SYSCTL-SPIN-MUTEX-PREEMPTED-HOLDER. kswapd spun forever on `sysctl::REGISTRY`, a bare `spin::Mutex` whose holder had been descheduled — 2026-08-13 — MECHANISM FIXED, TRIGGER STILL OPEN

**Caught by:** the 250-boot B-KNULLJUMP soak, iteration 20 of 20
(`build/knulljump-soak.log`; artifacts
`build/hang-catches/soak-20260813-061906-iter20.{serial,regs,stdout}.txt`).
19 of 20 boots were clean. **This is not B-KNULLJUMP** — that signature is
`RIP=0x0` with `error=0x10` (a control-flow hijack). Here `RIP` is a perfectly
real kernel address and the CPU is *executing normally*, just never finishing.

**What the evidence says.** `resolve-rip.sh` on the wedged RIP and the
rbp-chain the liveness dump walked:

```
0xffffffff80f32d56 -> core::sync::atomic::spin_loop_hint   (a bare `pause`)
  # 0: 0xffffffff80f1ab87 -> kernel::sysctl::get
  # 1: 0xffffffff80777ffe -> kernel::mm::kswapd::watermark_low
  # 2: 0xffffffff80777a63 -> kernel::mm::kswapd::kswapd_entry
  # 3: 0xffffffff807e2e79 -> task_entry_trampoline
```

All 16 RIP samples are the same instruction. The last boot-sequence line on
serial is `[kswapd] Background page reclaimer started` — the *very next*
statement in `kswapd_entry` is the `low_wm=…` `serial_println!`, whose first
argument is `watermark_low()`, i.e. `sysctl::get(PARAM_MM_MIN_FREE_PAGES)`.
kswapd wedged on its first ever sysctl read and never printed a second line.

Disassembling `sysctl::get` shows the spin is an *inlined* `spin::Mutex::lock`:
a `compare_exchange_weak` on the flag byte at `0x822cff28` (= `sysctl::REGISTRY`)
falling into a `load` + `pause` loop. The task table confirms the other half:
tid=0 (`prctl-batch269`, prio 31 — *higher* than kswapd's 20) sat **Ready** for
5562 ticks and was never picked, while `ctx_switches` stayed frozen at 1573 with
the heartbeat still climbing. So the timer was firing and the scheduler was
refusing to switch.

**Mechanism.** `kernel/src/sysctl.rs` had `use spin::Mutex;` — the *bare*
`spin::Mutex`, not `crate::sync::Mutex`. Three consequences, all of them bad:

1. **No `preempt_disable()` on acquire.** `crate::sync::Mutex::lock` disables
   preemption for the whole hold precisely because "a spinlock must never be
   held across a context switch". The bare lock has no such guard, so a holder
   can be descheduled mid-update and every later caller spins on a lock whose
   owner is Ready-but-not-running.
2. **No stall detector.** `crate::sync::Mutex::lock_contended` prints a one-shot
   diagnostic after `STALL_SECONDS` and keeps spinning. The bare lock spun for
   **five minutes** and printed nothing, which is why the hang looked like a
   silent freeze instead of naming itself.
3. **No owner tracking.** `crate::sync::Mutex` records the holding tid so
   `report_stall` can say *who*. The bare lock cannot, so the task table left
   "which task holds REGISTRY?" unanswerable.

**Fixed.** `sysctl::REGISTRY` is now
`crate::sync::Mutex::named(Registry::new(), b"sysctl-reg")`. That closes the
preempted-holder window outright and makes any future contention self-report
with a name and an owner. `list_all()` was also allocating its `Vec` *inside*
the critical section; with preemption now disabled for the hold that nests the
heap allocator (and, under pressure, reclaim) under a spinlock, so the
reservation moved above the `lock()` and the guard is explicitly dropped before
the return. The ISR contract is unchanged: interrupt-context readers still must
use `try_get` (see B-SYSCTL-IRQ-DEADLOCK).

**Still open — what froze `ctx_switches`.** The fix removes the *mechanism* by
which a sysctl holder could be preempted, but it does not fully explain why
CPU 0 stopped switching at all. Two candidates remain, and the evidence to date
cannot separate them:

* a leaked `preempt_disable()` somewhere on CPU 0 (`PREEMPT_DISABLE_COUNT` is
  **per-CPU**, so a task that voluntarily blocks while holding a tracked
  `crate::sync::Mutex` would leave the count elevated with nobody on-CPU to
  decrement it — preemption is then off for that CPU forever); or
* the run queue genuinely not holding tid=0, consistent with
  `local_has_real_work=false` printed beside a Ready prio-31 task.

To tell them apart on the next catch, the liveness dump now prints
`preempt_disable_depth=` per CPU (`kernel/src/sched/mod.rs`, from the existing
`preempt_count(cpu)`). A non-zero depth means "the scheduler was not *allowed*
to switch"; zero means "the scheduler *chose* not to", which is a completely
different bug. Re-run the soak and read that field first.

**Context worth keeping.** The wedged boot was also in an IRQ 11 storm
(~600 000 IRQs/sec, four mask/cooldown cycles) and had just come through the
OOM self-test driving memory pressure to `critical`. Whether the storm is a
cause, a consequence, or a coincidence is unknown; it is recorded here so a
repeat catch can be compared.
