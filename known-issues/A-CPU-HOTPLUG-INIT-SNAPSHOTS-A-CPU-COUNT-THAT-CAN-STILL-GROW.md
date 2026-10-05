## A-CPU-HOTPLUG-INIT-SNAPSHOTS-A-CPU-COUNT-THAT-CAN-STILL-GROW

**Lane:** A **Date:** 2026-08-27 **Status:** **FIXED 2026-08-27** — APs now
register themselves via `cpu_hotplug::mark_online_self`, called from
`ap_entry`, and the bounded wait says when it expires. See "How it was fixed".

**In short:** if a CPU finished starting up slightly too late, the kernel's
CPU-hotplug bookkeeping never learned it existed. That CPU ran and scheduled
work normally, but the hotplug framework thought it was absent forever: it
could not be taken offline, it was missing from the online count, and
`/sys/devices/system/cpu` under-reported the machine.

**And it was worse than "under-reports", which is how this entry originally
read.** Three subsystems ask `cpu_hotplug::is_online` before deciding what to
do with a CPU, and each would have drawn the wrong conclusion about one that
was demonstrably running:

| Consumer | Behaviour on a live CPU it believes is offline |
|---|---|
| `kernel/src/rcu.rs:319` — grace-period wait | **Skips it.** `synchronize_rcu` could return while that CPU sat inside an RCU read-side critical section, after which the caller frees memory the CPU is still reading. A use-after-free, not a reporting gap. |
| `kernel/src/sched/mod.rs:4363` — load balancing | Never migrates or steals work to it. A whole core stays idle under load. |
| `kernel/src/irqbalance.rs:348,411` | Never routes an interrupt to it. |

The RCU consequence is what took this off the "cosmetic, fix when convenient"
pile. Nothing was observed to crash — the straggler case is rare and the window
narrow — but the failure mode was silent memory corruption, not a wrong number
in a file.

**Where.** `kernel/src/cpu_hotplug.rs:160` (`init`) and `kernel/src/smp.rs:1230`
(the bounded wait in `init`).

**The mechanism.** `smp::init` starts the APs and then waits for them to report
in — but on a *bounded* spin:

```rust
let expected_cpus = (booted_count + 1) as u32; // +1 for BSP
let wait_limit: u64 = 50_000_000; // ~50 ms
for _ in 0..wait_limit {
    if NUM_CPUS_ONLINE.load(Ordering::Acquire) >= expected_cpus { break; }
    core::hint::spin_loop();
}
```

The loop exits either because every AP arrived *or* because the budget ran
out, and the two cases are indistinguishable afterwards — nothing checks which
happened. `cpu_hotplug::init()` then does:

```rust
let cpus = smp::cpu_count();
for i in 0..cpus { CPU_STATES[i] = Online; }
ONLINE_COUNT.store(cpus as u64, ...);
```

An AP that bumps `NUM_CPUS_ONLINE` after that store gets no `CPU_STATES` slot
and is never counted. `ONLINE_COUNT` has no other writer than `init`,
`offline` and `online`, so the omission is permanent for the boot.

**Why this is the numastat bug again.** `A-NUMASTAT-CPU-SETS-ARE-A-BOOT-SNAPSHOT-NOT-A-HOTPLUG-VIEW`
is the same sentence with a different subject: a boot-time reading of a live
counter stored as though it were final. The governing principle is the one
that came out of that fix — **a bounded wait is a race window, not a barrier.**
A 50 ms budget is generous on real hardware and not obviously generous under
QEMU TCG on a host that is simultaneously running a Rust build, which is
exactly the configuration this project's boot test uses.

**How it was found.** Not by observing it — by `scripts/check-live-counter-reads.py`,
written after the `irqstat` panic, flagging `cpu_hotplug::self_test` for
comparing two readings of `cpu_count`. Chasing why the two readings could
differ is what surfaced the bounded wait. The test was asserting a property
that the code does not actually guarantee, and the assertion was the only
thing pointing at the gap.

**What was changed first (2026-08-27, earlier).** Only the self-test, which
used to walk `0..smp::cpu_count()` asserting each CPU was `is_online` — an
assertion that would *fail* on the straggler, blaming the test rather than
naming the defect. That stopped the flake without touching the gap.

**How it was fixed (2026-08-27, later).** Exactly the shape this entry
predicted: `cpu_hotplug` is now *told*, from the same event that moves the
counter.

1. **`cpu_hotplug::mark_online_self(cpu)`**, called from `smp.rs`'s `ap_entry`
   immediately after `NUM_CPUS_ONLINE.fetch_add(1)`. It transitions the CPU
   `NotPresent → Online` with a `compare_exchange`, bumps `ONLINE_COUNT` once,
   and fires the `PostOnline` notifier chain.
2. **`init()` is now idempotent** rather than authoritative. It
   `compare_exchange`s from `NotPresent` instead of storing blindly, and
   recomputes `ONLINE_COUNT` by *scanning the states* instead of storing
   `cpu_count()` over them. That is what makes the two paths commute: an AP
   that self-registered before `init()` is not counted twice, and one that
   registers after is not erased.
3. Both directions are safe because the transition is only ever
   `NotPresent → Online`. An AP racing a deliberate `offline()` cannot revert
   it — `Parked` and `GoingOffline` are the BSP's decisions.
4. The event is `PostOnline`, not `PreOnline`: the CPU is already executing by
   the time it reaches this call, so there is nothing left to veto and a
   notifier returning `false` could not un-start it. The return value is
   ignored for that reason.
5. `mark_online_self` logs `"registered after init (late AP)"` when it runs
   after `init()`, so the case this whole entry is about is visible in the
   serial log the first time it actually happens.

**Also done:** the bounded wait now says which way it exited. `smp::init`
tracks whether the loop broke on the count or ran out of budget and logs
`"[smp] WARN: AP wait expired with N of M CPU(s) reporting in"` in the second
case. That warning appearing in a boot log means `wait_limit` is too small for
the host and the late-AP paths are carrying weight they were only meant to
insure against.

**Tests.** `cpu_hotplug::self_test` gained the invariant the old version could
not state — `ONLINE_COUNT` equals the number of CPUs *in state* `Online`,
counted by scanning, rather than "indices `0..online_count()` are all online",
which is only equivalent while the online set is a contiguous prefix. Plus
idempotence (re-announcing the BSP must be refused and must not move the count)
and range-checking (`mark_online_self(MAX_CPUS)` refused). The offline/online
cycle test now picks the highest index actually in state `Online` rather than
`recorded - 1`, for the same non-contiguity reason.
