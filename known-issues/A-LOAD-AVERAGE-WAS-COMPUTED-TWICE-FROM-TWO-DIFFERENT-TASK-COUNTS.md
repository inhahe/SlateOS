### A-LOAD-AVERAGE-WAS-COMPUTED-TWICE-FROM-TWO-DIFFERENT-TASK-COUNTS — **Status: FIXED** 2026-08-30

**In short:** the number that tells you how busy the machine is was being
worked out twice, in two places, from two different ideas of "busy" — and both
were wrong. Ask the machine one way (`cat /proc/loadavg`) and an idle box said
0.00; ask it another way (`uptime`, the `loadavg` command, `/proc/health`) and
the same idle box said about 40.00. On top of that the health monitor was
reading its copy on the wrong scale, inflating it another 20.48×, which parked
`/proc/health` at **Critical** permanently on a machine doing nothing. All of
it silent: no warning, no failed check, just numbers that could not both be
true.

**Where it lived.** `kernel/src/loadavg.rs` (now deleted) and
`kernel/src/sched/mod.rs` `update_load_average` / `nr_runnable`;
`kernel/src/syshealth.rs` `check_load`.

**Three independent defects, which is why the numbers were so far apart.**

| # | Where | What it counted | Effect |
|---|---|---|---|
| 1 | `crate::loadavg::sample` | `total_tasks_spawned - total_tasks_exited` — tasks that *exist* | Load ≈ the number of live tasks (~40), never decays to 0 |
| 2 | `sched::update_load_average` | `queue_length` per CPU — includes each CPU's parked idle task, excludes the task actually running (`pick_next` pops it) | Idle machine floors at 1.00 per CPU; fully-busy machine with empty queues reads 0.00 |
| 3 | `syshealth::check_load` | read a ×2048 fixed-point value as ×100 | Every printed load and every threshold comparison 20.48× too large |

Defect 1's own source comment said *"Count runnable tasks (Ready + Running
states)"*. It did not; it counted blocked ones too. **A comment describing the
count you meant to write reads exactly like one describing the count you
wrote** — the same shape recorded in `numastat.rs`, and the reason this
survived review for the life of the module.

**Why two implementations coexisted undetected.** They had *identical*
constants — `FSHIFT = 11`, `EXP_1/5/15 = 1884/2014/2037`, a 5-second
`LOAD_FREQ` — and identical EWMA arithmetic. A reader comparing the two modules
would compare the maths, find it matching, and conclude they were the same
computation. The entire difference was the one line that fetched the input.

**Why it mattered most at `/proc/health`.** Defects 1 and 3 compose:
`per_cpu_x100` came out around 40 × 2048 × 100 / 4 ≈ 2 048 000 against
`DEFAULT_LOAD_CRIT_PER_CPU` of 8.00 (stored as 800). So the health check
crossed Critical on the first sample after boot and stayed there, re-evaluated
once a second from `initproc::tick()` for the life of the machine. **An alarm
that is always on is an alarm nobody reads**, and it was also the loudest
consumer of the wrong number.

**The fix.**

- `sched::nr_runnable()` counts what Linux's `nr_running` counts: queued
  non-idle tasks (new `real_tasks()` on all three backends — the counting twin
  of the existing `has_real_work()` predicate) **plus** the non-idle task on
  each CPU. The running half needs to know whether a CPU's current task is its
  idle task without taking the SCHED lock, because `update_load_average` runs
  in the timer softirq where a blocking acquire of a lock the interrupted code
  holds can never succeed — hence `IDLE_TASK_IDS`, a lock-free per-CPU record
  written once at registration, with `u64::MAX` as the sentinel because 0 is
  the BSP's real idle task id.
- `crate::loadavg` **deleted**, not corrected. Making it agree would have left
  a second EWMA tracking the first, i.e. two numbers that can disagree with
  nothing to say which is right — the same call, for the same reason, as the
  irqstat mutators. Its five callers now read `sched::load_averages_fixed()`.
- `syshealth` converts once at the boundary through `load_x100`, so no raw
  fixed-point value reaches a comparison or a format string.

**Pinned by** `sched::test_load_average` (a scratch queue with one idle and two
real tasks must answer 3 to `total_tasks` and 2 to `real_tasks`, and an
idle-only queue 0 — asserted for *each* of the three backends, or
`sched.backend=eevdf` would keep the floor and only its users would see it) and
by four exact conversions in `syshealth::self_test` (0→0, 2048→100, 1024→50,
6144→300). The exact cases are the load-bearing ones: the live load on an idle
test machine is 0, the single input that is identical under both scales and
therefore proves nothing.

**Two display bugs fixed with them,** because their sources were in hand:
`uptime`'s `runnable/total` field had *both* halves computed as
spawned-minus-exited, so it read `N/N` for every N; and the `loadavg` command
printed the literal word `total` where Linux's `/proc/loadavg` field 4 has the
denominator — a line shaped like Linux's that could not be read like it.

**Not `crate::fs::loadavg`.** That is a third, unrelated module — the
`/proc/fs_loadavg` accounting table, reached by the `lavg` command — and is
untouched. The name collision is itself part of why this was easy to miss.
