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
