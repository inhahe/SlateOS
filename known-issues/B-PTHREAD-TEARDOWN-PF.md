### B-PTHREAD-TEARDOWN-PF. Intermittent kernel `#PF` (read @ 0x97) in a `cloned-thread` task during glibc-pthread thread teardown — WATCH (non-fatal, rare) 2026-07-15

**Symptom (1 occurrence in ~5 boots, 2026-07-15):** During the
`self_test_linux_real_glibc_pthread` self-test (process labelled
`spawn-test-glibc-pthread`: 4 threads via `clone`+futex+TLS, 40 000
mutex/futex ops, then `pthread_join`), a boot died with:

```
[sched] Task 124 exiting
[sched] Task 125 exiting
[sched] Task 126 exiting
EXCEPTION: Page Fault (#PF) at 0xffffffff82713dc2, address=0x97, error=0x0
  Cause: not-present, read, kernel
  Task: 123 ("cloned-thread"), priority 16, cpu 0
FATAL: Unrecoverable kernel page fault. Halting.
```

i.e. a **kernel-mode read of a near-null pointer (base+0x97 = 0x97)** in
one clone-child task (123) exactly while its sibling clone-children
(124/125/126) are running their `[sched] Task N exiting` teardown. This is
the classic signature of a **use-after-free / null-deref race in thread
teardown**: task 123 dereferences a per-thread or per-process structure at
field offset 0x97 whose base has just been torn down (freed / cleared) by a
concurrently-exiting sibling, on the single-CPU boot where the exiting
sibling preempts mid-window.

**Why NOT the resolved B-PTHREAD-YIELDBUDGET:** that entry is a *silent
hang* (yield-budget exhaustion), structurally fixed. This is a *hard #PF*
with a distinct fault address — a different failure mode in the same test,
so tracked separately.

**Not caused by the change it surfaced under:** it appeared on one boot
while validating the container-WORKDIR cwd plumbing (which does not touch
any thread path and is not exercised at boot); the very next boot (identical
binary) reached `BOOT_OK`. The code change only perturbed layout/timing and
exposed a pre-existing latent race.

**Reproduce:** run `bash scripts/boot-test.sh` repeatedly; the pthread test
faults intermittently (observed ~1/5). Non-deterministic — depends on the
exact preemption interleaving of the four clone-children during join/exit.

**[A] 2026-09-12 — three commits have since closed the named race windows, and the
recurrence count is now an argument rather than a hope.** Not closing this, because an
intermittent fault cannot be proved absent; but the entry has been carrying July's
assessment into September and the ground has moved.

*The code.* Sixty commits have touched `proc/thread.rs`, `proc/pcb.rs` or `sched/mod.rs`
since the sighting, and three of them address exactly the window this describes:

* `a2c7b8bb9` — *close the `thread::join` exit race with register-then-recheck.* The fault
  occurs during `pthread_join` while siblings exit. This is that race.
* `975114f54` — *seed thread `%fs`/`%gs` base before admission, fixes clone TLS race.* The
  failing test is clone+futex+**TLS**.
* `edf331c0e` — *register `CLONE_CHILD_CLEARTID` before admitting the child.*

*The count, using this file's own method.* `W1` two entries down was closed "on a count,
not an assertion", and the same is available here. `bench/boot-history.jsonl` holds **713**
records, the earliest 2026-08-16 — a month *after* the sighting, so every one postdates it.
**Zero** of them list the pthread test as skipped, so it ran. 628 reached `BOOT_OK`. At the
observed rate of roughly one fault in five boots, 713 runs should have produced on the
order of 140 recurrences. None is reported.

*The gap in that argument, stated rather than rounded away.* **85 of the 713 did not reach
`BOOT_OK`**, and the records do not carry serial content, so I cannot attribute those
failures. One or more could in principle be an unnoticed recurrence. Today's five failed
boots were all pre-build gates — inherited shellcheck, a misfiled open-questions entry, a
clippy denial in another lane — so the base rate for "failed for an unrelated reason" is
demonstrably high, but that is an impression and not a measurement of those 85.

**[A] 2026-09-12 — TAKEN THE CHEAP ROUTE THIS ENTRY NAMED FIRST, AND THE SIGNATURE HAS NOT
RECURRED IN 722 RECORDED BOOTS.** No stress run was needed: the data already existed in
`bench/boot-history.jsonl`, which carries an `exceptions` field nobody had queried.

| measurement | value |
|---|---|
| boot records | 722 |
| non-host failures | 84 |
| of those, reached the kernel (serial output) | **84** — pre-build gate failures never get a record |
| failures with an exception captured | 6 |
| page faults among them | 3 |
| **matching `address=0x97`** | **0** |

The three captured faults are at `0xffff80007fef4000`, `0x1000` and `0xffff80007feb0000` —
none is the near-null read this entry describes.

**Why the absence is meaningful rather than an artefact of missing instrumentation**, which
is the question that decides whether any of the above counts. Only 10 of 722 records carry
the `exceptions` key at all, which looks at first like sparse instrumentation. It is not:
the key is written **only when an exception is parsed**, and all 10 that have it have a
non-empty value. The test is whether the parser was live across the failure window, and it
was — failures span 2026-08-17 to 2026-09-10 and the *earliest failure of all* already
carries the key, with zero failures preceding it. So within that window, absence of the key
means no exception was seen, not that none could have been.

**And the self-test that provokes it does run**, checked rather than assumed: tonight's
boot shows `spawn-test-glibc-pthread` as process 339 doing 4 threads, 40,000 mutex/futex
ops and `pthread_join`, and `check-boot-skips` does not list it. A population of 700 boots
says nothing if the test was skipping in all of them.

**What this establishes, and what it does not.** At the observed July rate of 1 in 5, ~700
boots would have produced on the order of a hundred recurrences; there are none. So the
rate has collapsed and the defect was most likely fixed incidentally by teardown work in
the interim. It does **not** identify a root cause, and the July occurrence predates the
recorded window entirely — this is absence of recurrence, not a diagnosis. The honest
status is a measured negative rather than a fix.

*Worth noting for the next entry that reaches this state:* the route this took was already
written down here — "a boot whose serial log is checked for the `address=0x97` signature" —
and the cheaper version of it, querying a field the harness had been recording all along,
was available for weeks. The 15-boot stress run was the option everyone remembered.

*What would close this.* Either a boot whose serial log is checked for the `address=0x97`
signature across the 85, or a deliberate stress run of
`self_test_linux_real_glibc_pthread` — the old rate means ~15 boots would give better than
95% confidence of seeing it if it still occurs at 1-in-5. Symbolizing the July RIP is *not*
a route: `scripts/resolve-rip.sh` maps against the current ELF, and the kernel has changed
beyond recognition since, so it would confidently name the wrong function.

**Investigation status (updated 2026-07-15):** the toolchain *does* have a
working symbolizer — `scripts/resolve-rip.sh`, which maps a RIP against the
actual booted ELF (`target/x86_64-unknown-none/debug/kernel`, staged by
`scripts/boot-test.sh` line 73 — **not** the stale `target/x86_64-slateos/…`
image, which is a June-20 leftover and gives garbage). Earlier "no symbolizer"
/ garbage-symbol notes were wrong on two counts: (1) an awk-based mapper
truncated the 64-bit address to a 53-bit float, and (2) it was run against the
stale slateos ELF. `resolve-rip.sh` avoids both (lexicographic 16-hex-digit
compare; correct ELF). Running it on the captured trace gave:

```
0xffffffff82713dc2 -> sched::CURRENT_TASK_IDS  +0x2   (a DATA symbol, not code)
0xffffffff810e06c6 -> handle_page_fault
0xffffffff810d4f7b -> isr_page_fault
```

The two backtrace frames (`0x…810e06c6`, `0x…810d4f7b`) are the fault handler
itself (`handle_page_fault`/`isr_page_fault`) — expected, since the frame
walker starts inside the handler. But the **RIP is authoritative**: it is the
`frame.rip` value the CPU pushed onto the `#PF` interrupt stack frame
(`idt.rs:2189`/`2192`), i.e. the instruction that was executing when the fault
hit. That RIP resolves *into the `.data` section* (`CURRENT_TASK_IDS +0x2`).

**Sharper diagnosis:** RIP living inside a data symbol means this is a
**control-flow hijack** — a corrupted return address or function pointer sent
execution into `.data`, whose bytes then decoded as an instruction that did a
near-null read (base register 0 + disp 0x97 = cr2 0x97). (The kernel image is
mapped executable across its image, so fetching from `.data` does not itself
fault with an instruction-fetch error — consistent with the observed
`error=0x0` = not-present, **read**, kernel.) A corrupted code pointer during
thread teardown is the textbook signature of a **use-after-free**: a freed
per-thread structure's function-pointer / return slot was reused (or its
memory recycled) while task 123 still held a stale reference.

**PROPER FIX (needs a fresh reproduction to pin the exact pointer):** audit the
thread-exit path (`proc::thread::kill_process_threads` / `on_thread_exit_hook`
/ the clone-child TLS/`clear_child_tid` teardown and the per-thread control
block free) for a stored pointer (function pointer, return address into a
freed stack, or `&mut` into a container element) that a concurrently-exiting
sibling can free out from under task 123. Fix with an ID-lookup (not a stored
pointer) or by holding the teardown lock across the corrupted access, per the
"no dangling references" rule. On the next repro, also dump the top few
stack-slot values around `frame.rsp` and symbolize each with `resolve-rip.sh`
to recover the real caller frame (the hijacked return address's origin).
Line of investigation paused here pending a repro: fault is non-reproducible
(~1/5) and one capture cannot pin the exact corrupted pointer.

**Static audit (2026-07-15) — two findings that narrow the search:**

1. **`reap_dead_tasks` is ruled out as the mechanism.** It snapshots the
   current task id of *every* online CPU into `active_ids` and filters the
   dead set with `!active_ids.contains(id)` (`sched/mod.rs:3473`), so it never
   frees the kernel stack of a task any CPU is running on. And task 123 (the
   faulting task) is not `Dead` — it *resumes* and then faults — so its own
   stack is never a `reap_dead_tasks` candidate. The UAF is therefore not
   "sibling reaps 123's still-in-use stack via the reaper."

2. **The corrupted code pointer is a *specific* value: `&CURRENT_TASK_IDS[0]
   + 2`, not random garbage.** `CURRENT_TASK_IDS` is
   `[CachePadded<AtomicU64>; MAX_CPUS]`, so its storage spans MAX_CPUS × ≥64
   bytes and the resolver's reported `+0x2` is genuinely *inside CPU 0's slot*
   (the first `AtomicU64`). That address is exactly what `set_current_task(cpu,
   id)` computes to `.store()` the running task id for CPU 0
   (`sched/mod.rs:860`) — the sole writer of that address. So the hijacked
   return-address / code-pointer slot held the *address of the per-CPU
   current-task-id cell*, which strongly implicates the **low-level context
   switch**: a spilled `&CURRENT_TASK_IDS[cpu]` (or a register holding it
   across `set_current_task`) overlapping the saved-RIP slot on task 123's
   kernel stack — a stack-frame-layout/offset bug in the switch path — rather
   than a heap/PCB use-after-free in the higher-level exit bookkeeping
   (`on_thread_exit`/`on_thread_exit_hook`, which only touch user memory + the
   robust/ctid/rseq maps and never take `&CURRENT_TASK_IDS`). Next repro should
   focus the `dump_stack_scan` output on which frame's return slot equals
   `&CURRENT_TASK_IDS[0]+2` and cross-reference the context-switch save/restore
   stack offsets.

**Static audit refinement (2026-07-15b) — the context-switch assembly is
provably clean, so finding 2's "switch-path layout bug" phrasing is wrong.**
Reading `sched/context.rs` in full: `switch_context` only saves/restores the
callee-saved GPRs, `rsp`, `rflags`, and FPU state via the `Context` struct at
fixed offsets 0x00–0x38; it *never computes or references `&CURRENT_TASK_IDS`*
at all (nor does `task_entry_trampoline`). The offsets match `task.rs`'s
`Context`. Therefore the address value `&CURRENT_TASK_IDS[0]` cannot originate
in the switch code — it must be **spilled/stored by a *different* function that
takes `&CURRENT_TASK_IDS[cpu]`** (`set_current_task` at 860, `load_current_task`
at 869, and the reaper/health snapshots at 3487/5242/5263/5309) and then land,
via a wild write / stack overflow, on top of task 123's saved return-address
slot. Mechanism is now: task 123 last suspended by calling `switch_context`
(pushing a normal return address into `schedule()`); something overwrote that
stack word with the value `&CURRENT_TASK_IDS[0]` (+2 is the resolver rounding to
the nearest preceding symbol; the stored qword is the cell base); when 123 is
resumed, `switch_context`'s final `ret` jumps to that data address and #GP/#PFs
executing `.data` as code (cr2=0x97 is then whatever the garbage bytes there
decode to dereference). **Next repro must catch which code path spills
`&CURRENT_TASK_IDS[cpu]` to a stack slot that can alias another task's stack** —
prime suspects are any `current_cpu_id()`/`load_current_task()` call made while
running on a *borrowed* or already-freed stack, or an off-by-one stack write in
the clone/exit path. The `dump_stack_scan` capture should show the exact stack
address holding `&CURRENT_TASK_IDS[0]` relative to task 123's `rsp`.

**Update 2026-08-14a — the assumed byte decode does not fit the error code, so
the "stored qword is the cell base" story is incomplete.** Audit 2026-07-15b
explains the `+0x2` as "the resolver rounding to the nearest preceding symbol;
the stored qword is the cell base". That reading is doubtful:
`scripts/resolve-rip.sh` reports *nearest preceding symbol + offset*, so a
report of `+0x2` means RIP genuinely **was** `&CURRENT_TASK_IDS[0] + 2`, not
the base. The natural mechanism is instead: `ret` jumped to the base, the
instruction there executed, and the *next* instruction — at base+2 — faulted.

But that does not close either. `panic_diagnostics()` reported the current task
as 123, so CPU 0's cell held `123 = 0x7B`, i.e. bytes `7B 00 00 00 00 00 00 00`:

- `base+0`: `7B 00` = `JNP rel8 +0` — whether or not the branch is taken, the
  next instruction is at `base+2`. Consistent with the observed RIP.
- `base+2`: `00 00` = `add byte [rax], al` — a read-modify-**write**. On a
  not-present page x86 reports error bit 1 set, i.e. `error = 0x2`.

The captured error was `0x0` (**read**). So the faulting instruction was *not*
`add [rax], al`, and at least one of the assumptions (which cell, what it held,
or that RIP is inside `CURRENT_TASK_IDS` at all) is wrong. A plausible
alternative is that the resolver attributed RIP to `CURRENT_TASK_IDS` merely
because it is the nearest *preceding* symbol — the true target may be a later,
symbol-less location, which would invalidate the whole "per-CPU cell address"
inference and with it the search direction of audit 2 above.

**Action taken instead of more inference: the handler now dumps the raw bytes
at RIP** (`idt.rs`, right after `dump_stack_scan`, guarded by a
`page_table::translate` mapped-check so it cannot itself fault, and emitted
after the scan so it can never displace it). One repro now settles what
executed, rather than another round of decode guesswork.

**Update 2026-08-14b — 20 consecutive boots, zero occurrences.** The
confirmation soak for B-PTHREAD-JOIN-LOST-CTID ran 20 full boots
(`build/hang-catches/soak-ctidfix.log`, 23:02–01:52) with **no `EXCEPTION:` /
`Page Fault` / `FATAL` line at all**, on a build carrying both
register-after-admit fixes (`%fs`-base-before-admit and
ctid-before-admit). Against the historically recorded ~1/5 rate that is
`0.8^20 ≈ 1.2%` likely, which is real evidence the rate has dropped.

Two candidate explanations, not yet distinguished:

1. **The TLS fix cured it.** B-PTHREAD-CHILD-JUMPS-TO-GARBAGE defect 1 let a
   clone child run before its `%fs` base was seeded — itself a control-flow /
   wild-access defect in *this exact self-test*. A shared root cause is very
   plausible.
2. **The rate merely drifted.** The 1/5 figure is from 2026-07-15, a month and
   many changes ago, so the null hypothesis "unchanged rate" may already have
   been false for unrelated reasons.

Deliberately **not** downgrading this entry to FIXED on that evidence: 20 clean
boots cannot separate "cured" from "rarer", and silently closing it would throw
away the instrumentation's value. Keep at WATCH; if a repro appears, the new
bytes-at-RIP dump plus the existing stack scan should close it in one capture.

**Update 2026-08-21 — IT RECURRED, and in a form nobody had allowed for: the
hijack was in *ring 3*, so none of the instrumentation fired.** The
`spawn-test-glibc-pthread` process (pid 310; main thread task 277, workers
278–281) faulted during exactly the documented window — siblings 279, 280 and
281 had just printed `[sched] Task N exiting` — with:

```
[exception] User page fault (task 278) at 0x6000066370, addr=0x6000066370 (not-present, read) — trying SEH
[exception] Killing task 278 — Page Fault (#PF) at 0x6000066370 (ring 3)
  CS=0x23 RFLAGS=0x10246 RSP=0x6000a15e68 SS=0x1b
[spawn]   FAIL: real glibc pthread — exit code=Some(-8), expected 13
```

`RIP == CR2` again, so it is the same *shape* — a control-flow hijack, execution
sent to an address that holds no code — but `CS=0x23` means it happened at
**CPL 3**, not in the kernel. Every previous capture was ring 0. The whole
2026-08-14a bytes-at-RIP dump and the `dump_stack_scan` call it was added
alongside live on the *kernel* fatal path (`idt.rs`, after the
`error & 4 == 0` branch); a ring-3 fault never reaches them. So the capture
this entry had been waiting five weeks for arrived and produced three lines.

**Not caused by the change it surfaced under.** It appeared on the boot
validating the hrtimer min-heap rewrite (`520634ccc`), and the immediately
following boot on the same source was clean — the usual pattern for this
defect, and consistent with the historical ~1/5 rate. A heap-vs-sorted-list
change in the timer queue also has no mechanism to corrupt a user code pointer:
its failure modes are a lost or mis-ordered wakeup, which hang, not jump.

**What the capture does and does not say.** `0x6000066370` is *below* every
mapping the log records for pid 310 — the first `mmap` was `0x6000212000`, and
the four 8 MiB thread stacks run upward from `0x6000216000`. Thread 278's own
stack was `0x6000216000..0x6000a1a000` and `RSP = 0x6000a15e68` sits ~16.8 KiB
below its top, i.e. **the stack pointer was perfectly sane while RIP was
garbage.** That is the one genuinely new fact, and it is worth more than it
looks:

- If the *user* stack is intact and only RIP is wrong, the corrupted value was
  most likely not read from the user stack at all — which points away from
  "glibc scribbled on its own frame" and toward **the saved user RIP in the
  kernel's interrupt frame**, restored by `IRETQ` on the way back to ring 3.
- And that unifies the two manifestations. A single wild write into a task's
  **kernel stack** explains both: land on the saved *kernel* return address and
  you get the 2026-07-15 ring-0 jump into `.data`; land on the saved *user* RIP
  in the same stack's interrupt frame and you get this ring-3 jump into a hole.
  The historical hijack value was `&CURRENT_TASK_IDS[0]`, a kernel data address
  — a plausible spill, and a value that could only have got onto a stack by
  being written there. Two different victims, one class of writer.

This is a hypothesis, not a conclusion: the capture cannot yet distinguish it
from a user-side corruption that happened to leave the stack pointer valid.

**Action taken 2026-08-21: the ring-3 fatal path now reports as much as the
ring-0 one** (`idt.rs`, `kill_userspace_task_with_info`). It classifies RIP, the
fault address and RSP against the process's VMA list via a new allocation-free
`pcb::classify_user_addr` (`try_lock`, copies out at most three `Vma`s, and
reports "table busy" rather than waiting — a diagnostic must never be the thing
that deadlocks the machine), naming the *hole* an address falls into and the
mappings bracketing it; dumps the bytes at RIP; and raw-scans 16 quadwords of
the user stack, flagging every value that points into a mapped executable user
page. Each read is preceded by a page-table check on that exact address so the
report cannot itself fault.

**That turns the next repro into a decision rather than another inference
round**, because the two hypotheses above predict opposite outputs:

| | user stack scan shows | verdict |
|---|---|---|
| kernel-stack wild write | plausible frames, executable-looking return candidates | the corruption is in the *kernel* interrupt frame; hunt the writer |
| user-side corruption | garbage, or the hijack value visible on the stack | glibc/TLS teardown; hunt the freed object |

The classification of `0x6000066370` also settles, in one line, whether that
address is a freed thread stack, the `brk` heap, or address space that was never
mapped — three different bugs that the old two-line report could not tell apart.
The instrumentation is exercised on every boot, because the exception self-tests
deliberately kill ring-3 tasks (`0x4000000000`-family faults), so it cannot rot
unnoticed the way the ring-0 dump did.

**A mistake made adding it, recorded because the next person will make it too:
the first version killed the boot it was added on.** Reading user memory from
ring 0 is not merely a pointer dereference on this kernel — SMAP is enabled, so
a bare kernel read of a user page faults, and the boot died with

```
EXCEPTION: Page Fault (#PF) at 0xffffffff821936b8, address=0x4000000002, error=0x1
Cause: present, read, kernel
FATAL: Unrecoverable kernel page fault. Halting.
```

Note the shape of that: **`present`** — the page *was* mapped and the read
faulted anyway, which is exactly what makes SMAP easy to misdiagnose as a bad
address. Both read sites now go through `crate::smep_smap::with_user_access`
(STAC/CLAC), each with a `// SAFETY:` comment saying the wrapper is **required**
rather than defensive, so that nobody later "simplifies" it away. The general
point is the one the diagnostic's own design already concedes: a fault handler
is the worst possible place to discover that your code can fault, and only a
boot test finds it — this would have shipped clean under a "it compiles and
clippy is quiet" standard.

### [FIXED 2026-09-25 as WATCH — the second cause is found and fixed: a stale run-queue entry resumed the exiting task in place, and `task_exit` halted with interrupts off; no RIP was ever captured, so it stays watched] B-FORKEXEC-BOOT-HANG. Intermittent silent boot hang after the last thread of a just-reaped process exits — the first cause (a freed PML4 still live in CR3) fixed in `0ecd5ff03`; the second (stale run-queue entries) fixed 2026-09-25 — 2026-07-15

**[A] SECOND CAUSE FOUND AND FIXED 2026-09-25 — the exiting task's own
leftover run-queue entry.** Fixed in the commit that adds
`sched::test_stale_run_queue_entries`; design-decisions.md §964.

*In short:* the scheduler's run queues hold bare task numbers, and several
places that take a task *off* a queue looked for it only at the priority
level they expected, not where it actually was. When they missed, the entry
stayed behind. If that task later exited, its own leftover entry was the next
thing picked: the scheduler "resumed" the dying task instead of switching
away, and the exit path -- which assumed that could never happen -- halted
the CPU with interrupts disabled. On the one-CPU boot machine that is the
whole machine stopping without a word, which is this entry's signature.
Removal now finds a task wherever it is, the scheduler refuses to run any
task that cannot run, and the exit path no longer halts even if it is
resumed.

**The mechanism, end to end.**

1. `PriorityRoundRobin::dequeue(id, priority)` scanned only level `priority`,
   and every caller passed `task.effective_priority()`. A queued task is not
   always at that level. The anti-starvation booster (`check_starvation`)
   moves a `Ready` task to level 0 without changing any field
   `effective_priority()` reads -- and on a loaded single-vCPU TCG guest a
   user process waits the 2 s threshold routinely. A timer tick
   (`tick_burst`) can also clear `interactive` on a task that is queued
   while still on the CPU (woken between marking itself parked and
   switching away), moving its computed level by `INTERACTIVE_BOOST`. The
   booster had been given `dequeue_any` for its own case; nothing else had.
2. So these removals could miss, silently -- no caller checked the result:
   `kill_task` of a `Ready` task, `mark_suspended` of one,
   `park_if_suspended`'s undo of a resume that beat the park, and the
   remove-then-requeue moves in `set_priority`, `boost_priority`,
   `set_inherited_priority` and `set_cpu_affinity`. The results: a dead or
   suspended task still queued, or a live task queued twice -- and once one
   copy is dispatched, a *running* task holding an entry.
3. `schedule_inner` trusted every entry. What the pick returned was marked
   `Running` and switched to -- a killed task came back to life -- and an
   entry for the current task resumed it in place *whatever its state*.
4. `task_exit` marks the task `Dead` and calls
   `schedule_inner(false, Uncounted)`. With the exiting task's own entry at
   the head of the queue, the pick returned the task itself, the
   `picked_id == current_id` arm set it `Running` and **returned**, and
   `task_exit` fell into `cpu::halt_loop()` -- `loop { cli; hlt }`. The same
   end is reached if another task's pick switches *into* the dead task
   later: its `schedule_inner` returns into `task_exit` just the same.

Every property of the recurrences that the first cause could no longer
explain matches: the last line is `[sched] Task N exiting`, `task_exit`'s own
print, immediately before the pick; nothing follows it, because the halt is
silent by construction; the BSP stops taking timer interrupts, because of the
`cli` -- the 2026-08-25 narrowing's missing breadcrumbs; no lock is spinning,
so no stall detector fires; it is intermittent and history-dependent, needing
a missed removal earlier in the boot for the very task that later exits; and
it is not specific to fork+exec -- three different rungs, each ending in a
user thread's exit.

**Status: WATCH, not proven by a RIP.** None was ever captured. What is
proven: the missed removal, the trusted pick and the silent halt are each
read directly from the code, and the regression test below reaches the halt
deterministically on the pre-fix code (a task exiting with its own entry at
the head of the queue). Close the watch after a clean run of boots with no
`stale run-queue entry` report and no recurrence.

**The fix** (`kernel/src/sched`):

- **Removal is by id.** `PriorityRoundRobin::dequeue` removes every entry for
  the task at every level, and `PerCpuScheduler::dequeue` sweeps every
  online CPU's queue, not only `last_cpu`'s. `dequeue_any` is folded in.
- **The pick checks what it picked.** `pick_runnable_locked`, for the main
  switch and the idle fallback alike, drops and reports an entry for a task
  that is `Dead`, `Blocked`, `Suspended`, gone from the table, or `Running`
  on another CPU, and resumes the current task in place only if it is
  `Ready` or `Running` (`classify_pick`). On SMP it also hands a task still
  executing on another CPU back to that CPU (`running_elsewhere`, from
  `CURRENT_TASK_IDS`/`PREV_TASK_IDS`) instead of dispatching it twice, and
  re-homes a stolen task its affinity forbids here.
- **Death purges.** `kill_task` and `task_exit` remove every entry of the
  task under the guard that publishes it `Dead`, and report one found for a
  task that should have had none.
- **No silent halt.** `task_exit` is `-> !` and loops: resumed after exiting,
  it prints `*** BUG: task N was resumed after it exited`, re-marks the task
  `Dead` and switches away, idling with interrupts on when nothing is
  runnable. `DEAD_TASKS_RESUMED` counts it.
- The booster boosts under the guard that chose each task (on SMP the
  released guard let a just-dispatched task be re-queued), and
  `unthrottle_expired`/`set_cpu_quota` remove before re-queueing, so a
  throttled task that was woken is not queued twice.
- The idle fallback resumes a woken current task in place instead of
  "switching" it to itself, which handed `switch_context` an aliasing
  `&mut` and `&` to one saved context.

**Regression test:** `sched::test_stale_run_queue_entries`, a boot self-test
run right after `test_kill_and_reap`. Boosted-then-killed leaves no entry and
never runs; boosted-then-re-prioritised leaves exactly one; planted entries
for a `Dead` and a never-admitted `Blocked` task are never dispatched, and
`classify_pick` refuses both as the current task; a task that exits with its
own planted entry at the head of the queue reaches `Dead`, runs once and is
never resumed. On the pre-fix code the last case halts the boot silently --
this entry's signature, on demand.

**If it recurs:** a stale entry is now reported instead of swallowed. Look
for `[sched] *** BUG: stale run-queue entry for task N (state S) found by
SITE` in the serial log: the site and the state name the removal that
missed. `*** BUG: task N was resumed after it exited` means a dead task was
dispatched in spite of the pick's check. Neither appears in a healthy boot --
the self-test's planted entries are counted but not printed.

---


**Occurrence 2026-09-15 (lane A), on a DIFFERENT test with the same
signature.** `spawn-test-dash-statpath` -- a real dash running
`[ -f /bin/dash ] && echo` under Path Z -- went silent after:

```
[thread] Process 380 has no threads left -- now zombie
[sched] Task 353 exiting
```

No `#PF`, no PANIC, no FATAL; 2,078,194 bytes of serial and then nothing for
the rest of the window. `boot-history` matched it to this entry
automatically, which is the thing that saved the investigation -- the merge
immediately before it brought only `apps/`, `Cargo.lock` and docs, so the
hang could not have come from its content, and without the automatic match I
would have spent a boot cycle proving that by bisection.

Worth noting for the pattern: this is now **three different ring-3 tests**
with one signature -- forkexec, and now statpath. Whatever idles is not
specific to `fork`+`exec`; it is specific to a process going zombie with a
waiter, which statpath reaches by a much shorter path (`[ -f ... ]` is one
stat and an exit, no fork of its own). If anyone attacks this, statpath is
the cheaper reproducer.

**Symptom (1 occurrence, 2026-07-15):** During
`self_test_linux_real_glibc_forkexec` (`spawn-test-glibc-forkexec`,
main.rs:1791: a glibc program that `fork()`s, the child `execl()`s a second
ELF, the parent `waitpid()`s), a boot went silent. The last serial lines were
the normal end of that test's process teardown:

```
[exec] Process 165 exec complete: entry=0x…, rsp=0x…
[mmap] Lazy mapped 0x6000212000..0x6000216000 (1 frames, demand-paged)
[thread] Process 165 has no threads left — now zombie   (execed child)
[thread] Process 164 has no threads left — now zombie   (fork parent)
[sched] Task 130 exiting
```

…then **no further output** and the 480 s boot-test timeout fired. Note there
is **no `#PF`/PANIC/FATAL** — this is a *hang* (the scheduler idled with no
runnable task, or a reap/`waitpid` wait never woke), NOT the `#PF` control-flow
hijack tracked in **B-PTHREAD-TEARDOWN-PF** above, and it is at a different
test (fork+exec, not pthread). The immediately-following boot (identical binary)
reached `BOOT_OK` in 89 s, so it is intermittent, not a hard regression.

**Distinct from the two known pthread issues:** B-PTHREAD-TEARDOWN-PF is a hard
`#PF`; B-PTHREAD-YIELDBUDGET (resolved) was a yield-budget hang inside the
*pthread* test. This hang is in the *fork+exec* test, after both child and
parent have gone zombie — pointing at the parent's `waitpid`/reap wakeup or the
scheduler's idle transition when the last task exits, rather than at thread
teardown.

**Not caused by the change it surfaced under:** it appeared while validating
Path Z Part 43 (a signal self-test registered at main.rs:2106, which never even
ran this boot — the hang is ~300 lines of test earlier). The change only
perturbed timing.

**Reproduce:** run `bash scripts/boot-test.sh` repeatedly. Non-deterministic;
observed once. **Diagnostic aid (2026-07-15):** `boot-test.sh` now echoes the
last 25 serial lines to stdout on any timeout (independently of the serial
file, which a re-run overwrites), so the next occurrence records its freeze
point in the test output automatically; the harness also hints to re-run with
`--hard-lockup-watchdog` to capture the wedged guest RIP via the i6300esb NMI +
HMP monitor.

**Static audit (2026-07-15b) — the waitpid-lost-wakeup hypothesis is RULED OUT;
suspect is a task-exit-path wedge (likely Q24 holder-preemption spin-deadlock).**
Two facts from reading the code + the serial log flip the diagnosis:
1. **The kernel harness that waits for the spawned glibc program does not block
   — it *polls*.** `self_test_linux_real_glibc_forkexec` (`spawn.rs:9523`) waits
   with `for _ in 0..MAX_YIELDS { if state==Zombie break; sched::yield_now() }`,
   never `block_current`. A poll loop cannot suffer a lost wakeup, so "harness
   parked in wait4 with a missed wakeup" is impossible here.
2. **The parent process itself reached `Zombie`** (`Process 164 … now zombie`
   in the log) — i.e. the glibc program's own `waitpid()` already returned and
   the program exited normally. So the in-guest wait4 also completed. The freeze
   is *after* both, at `[sched] Task 130 exiting` — inside the scheduler's
   task-exit teardown for the last thread, with **no further output**.
   And the core kernel `wake`/`block_current` protocol is independently sound
   (the `pending_wake` flag in `sched/mod.rs:1523` closes the register→park
   window), so this is not a scheduler wakeup bug either.
This points at the **exit/teardown path wedging on the CPU** rather than any
missed wakeup: most plausibly a **raw `spin::Mutex` holder-preemption deadlock**
— the *same Q24 class* as the already-fixed container-exec (`fa87bbb5e`) and
heap (`83307bdfc`) deadlocks — hit somewhere between "[sched] Task N exiting"
and the reap (e.g. `PROCESS_TABLE`/`SCHED`/reaper locks taken without
preempt-disable while a timer preemption lands on the holder on a 1-CPU boot).
It could also be an idle-transition bug (last runnable task exits and the idle
path never reschedules the still-Ready harness), but the lock-deadlock is the
better fit for a *silent, output-less* freeze mid-teardown.

**PROPER FIX (needs a repro):** on the next occurrence, capture the wedged guest
RIP (`--hard-lockup-watchdog`) and check whether it sits inside a
`spin::Mutex::lock` spin in the exit/reap path (→ Q24 preempt-disable fix, mirror
the container/heap pattern) vs. the idle loop with a Ready task still queued (→
idle-reschedule bug). Given the Q24 lineage, the highest-value proactive step is
the kernel-wide raw-spin holder-preemption audit already queued as **Q24** in
`open-questions.md`; this hang is another data point for doing that audit.

**[A] Static audit 2026-08-14c — the silence itself is evidence, and it rules
out both instrumented lock types.** Two facts about the lock implementations
turn "no output" from a dead end into a filter:

1. **`crate::sync::Mutex::lock()` disables preemption for the whole hold**
   (`sync.rs:432`) *and* routes contention through `lock_contended()`, whose
   stall detector fires after `STALL_SECONDS = 30` of wall-clock spinning,
   naming the lock, the wedged CPU/task and the locks that CPU already holds.
2. **`PreemptSpinMutex::lock()` does the same** — `preempt_disable()` then
   `spin_with_stall()` (`sync.rs:924-931`).

Both detectors fire from *inside* the spin loop, so per their own
documentation they work "regardless of IF state", and 30 s is far inside the
480 s timeout this hang consumed. **Therefore a >400 s silent wedge cannot
have been spinning on a `crate::sync::Mutex` or a `PreemptSpinMutex`** — one
of them would have announced itself. What remains:

- a **raw `spin::Mutex`** (no preempt-disable *and* no stall detector — the
  un-converted Q24 remainder), or
- a non-lock infinite loop / failure to reschedule, or
- a wedge whose own diagnostic cannot escape (see the console note below).

This is a genuine prune, and it doubles as a **prioritisation criterion for
the Q24 sweep**: converting a raw lock buys not only holder-preemption safety
but self-reporting, so each conversion permanently shrinks the set of places a
future silent hang can hide. Progress on Q24 is therefore measurable, not just
hygienic.

**Hypotheses checked and RULED OUT (do not re-derive these):**

- *Both registered exit hooks are clean.* `notify_exit_hooks`
  (`sched/mod.rs:1085`) runs only two real hooks — `pacct::on_task_exit`,
  which is lock-free (atomics + a static ring; its one call into
  `sched::task_info` is deliberately the single-task variant, chosen to avoid
  a long SCHED hold), and `sched::supervisor::on_task_exit`, which uses
  `crate::sync::Mutex` (`SUPERV`) and so is both preempt-safe and
  stall-instrumented. The supervisor's restart path is also correctly
  deferred: it copies the restart info out, `drop(table)`s, and schedules via
  ktimer *specifically* to avoid spawning in the dying task's context.
- *"An IRQ handler self-deadlocks on the raw `SERIAL` lock."* Plausible on
  paper — `serial.rs:147` really is a raw `spin::Mutex` — but `_print`
  (`serial.rs:204`) already defends it three ways: the whole body runs under
  `cpu::without_interrupts`, a **per-CPU `IN_PRINT` flag** is claimed *before*
  the lock is taken (deliberately before, so a nested exception during the
  *wait* also takes the safe path), and any re-entry falls back to
  `SerialPort::emergency()`, which does not lock at all. So a nested print
  cannot wedge on `SERIAL`.

**Remaining lead — `kernel/src/console.rs` is entirely raw.** `CONSOLE`,
`SCROLLBACK` and `COLOR_SCHEME` (`console.rs:107/478/679`) are `spin::Mutex`
via `use spin::Mutex`, i.e. no preempt-disable and no stall detector, and
`console.rs` has no equivalent of serial's `IN_PRINT` re-entrancy guard. It is
a strong Q24 conversion candidate on its own merits, and it is the one place
where the silence argument above is *not* evidence of innocence: a wedge on an
output lock cannot report itself. Note this does **not** by itself explain
this hang's silence, because the diagnostics that went missing were
`serial_println!`, and serial is independent of console.

**[A] CORRECTION to the 2026-08-14c prune (2026-08-22).** That audit's
conclusion — "a >400 s silent wedge cannot have been spinning on a
`crate::sync::Mutex` or a `PreemptSpinMutex`" — rests entirely on the premise
that the 30 s stall detector *does* fire. We now have direct evidence of it
**not** firing: the `fs::encrypt` self-deadlock
(`B-A-SELF-DEADLOCK-DETECTORS-ALL-MISSED-A-TEXTBOOK-SELF-DEADLOCK`) spun for
~600 s on a `PreemptSpinMutex` and printed nothing at all. So the prune was
unsound as written, and the two lock types were never actually excluded. The
detector has since been given a self-test (`sync::self_test_stall`, which
drives `spin_with_stall_threshold` at a ~10 ms threshold and asserts a report
lands), so from the next green boot onward the premise is *checked* rather
than assumed — but do not lean on the 2026-08-14c prune for reasoning about
any hang that predates that self-test passing.

**[A] ROOT CAUSE FOUND 2026-08-22 — a use-after-free of the process page
tables. Fixed in `0ecd5ff03`.** Reproduced on boot cycle 6 with the same
signature as 2026-07-15 (silence immediately after `[sched] Task N exiting`,
900 s timeout, no panic, no stall report). The new narrowing that cracked it:

1. **The harness's poll loop is bounded** — `for _ in 0..MAX_YIELDS` with
   `MAX_YIELDS = 262_144` (`spawn.rs:22881`) — **and every path after the loop
   prints**, including the loop-exhausted path. So the absence of output proves
   the wedge is *not* in "waiting for the process to exit"; the loop had
   already broken with `reaped = true`, and the wedge is in one of the six
   post-loop steps (`pcb::state`, `pcb::exit_code`, `Vfs::read_file(CAPTURE)`,
   `thread::on_thread_exit`, `pcb::destroy`, `Vfs::remove(CAPTURE)`).
2. **`pcb::destroy` is the one that frees memory another CPU may be using.**
   It calls `destroy_process_resources` → `destroy_user_address_space`, whose
   `// SAFETY:` comment (`pcb.rs:6064`) asserts *"no threads are running in
   this address space, and no CPU has this PML4 loaded in CR3"*. **Nothing in
   the kernel established either half of that.**

The race, in full:

- The zombie transition lives *inside* `proc::thread::on_thread_exit`
  (`thread.rs`), which runs **on the dying thread itself**. It prints
  `"Process N has no threads left — now zombie"`, calls `sched::wake` on the
  task parked in `wait4()`, and posts `SIGCHLD`. The reaper is released
  **here** — but the dying thread still has work to do: return through
  `on_thread_exit`, run `sched::task_exit()` (another `serial_println!`),
  `notify_exit_hooks`, take `SCHED`, mark itself `Dead`, and only then
  `schedule_inner` away.
- That window is *milliseconds*, not microseconds: two serial lines at
  115200 baud are ~8 ms of output, and the timer tick is 10 ms. A preemption
  landing inside it is common, not exotic.
- Preempted there, the dying task is still `Running` and still records
  `Task::pml4_phys = <process PML4>`. The reaper runs, sees `Zombie`, calls
  `pcb::destroy`, and the PML4 frame is freed.
- When the scheduler switches the dying task back in to finish exiting,
  `schedule_inner` (`sched/mod.rs`) does
  `if old_pml4 != new_pml4 { write_cr3(new_pml4) }` — loading a **freed**
  frame into CR3. If that frame has since been handed out and overwritten
  (the harness allocates heavily right after: `Vfs::remove`, then the next
  self-test), its kernel half is garbage and the machine cannot even report
  the fault, because the tables that map the fault handler are the ones that
  were freed.

That accounts for every property of the symptom that made it hard: the
silence (no working page tables ⇒ no diagnostic path), the absence of a stall
report (nothing was spinning on a lock), the intermittency (it needs a tick
in the window *and* the freed frame to be destructively reused before the
switch-back), and why it clusters on the fork/exec test (it is the test whose
harness reaps a process the instant it zombifies).

**Fix:** `sched::detach_address_space(task_id)` — the analogue of Linux's
`exit_mm()` → `switch_mm(&init_mm)`. It clears `Task::pml4_phys` (so no later
switch-in can reload the frame) *and* writes CR3 to the kernel PML4 (so the
current execution stops depending on it). Both halves are required; neither is
sufficient alone. `proc::thread::on_thread_exit` calls it immediately after
`on_thread_exit_hook` — the last step in the exit path that needs the thread's
user memory — and therefore *before* anything can publish the process as
reapable. `sched::task_exit` calls it again as an idempotent backstop for exit
paths that never reach `on_thread_exit`.

**Regression test:** `proc::thread` test 11
(`test_exit_detaches_address_space`) pins the invariant `pcb::destroy`
actually needs — *at the instant a process becomes `Zombie`, no task still
names its PML4*. The victim is spawned suspended and never admitted, so the
test is deterministic instead of depending on the narrow preemption window
that made the original failure intermittent.

**Status (2026-08-22): RESOLVED — WATCH cleared.** The condition the WATCH was
waiting on has been met: three consecutive boot cycles after `0ecd5ff03` each
executed `self_test_linux_real_glibc_forkexec` to a green
`REAL glibc forkexec` line and went on to reach `BOOT_OK`, with no silent stop
and no timeout:

| Cycle | Commit | Verdict | `REAL glibc forkexec` |
|---|---|---|---|
| 8 | `ab3d42901` | BOOT_OK | pass |
| 9 | `b215b83c1` | BOOT_OK | pass |
| 10 | `1422972ad` | BOOT_OK | pass |

Three is the right number to stop at rather than an arbitrary one. The hang
was never a coin flip on every boot — across the whole recorded history it
appeared a handful of times in dozens of runs, so three clean runs alone would
be weak evidence taken by themselves. What makes them sufficient here is that
they are corroborating a *mechanism* that is independently pinned down: the
page-table path was derived from the code and the log rather than guessed, and
`proc::thread` test 11 (`test_exit_detaches_address_space`) now fails
deterministically if the invariant regresses. The boots confirm the fix did
not introduce a new intermittency; the unit test, not the boot count, is what
keeps it fixed.

**If it ever recurs**, do not re-derive the page-table path — that one is
closed and regression-tested. The remaining post-loop suspects from narrowing
(1) above are `Vfs::read_file`/`Vfs::remove` on the capture file and the
console-lock lead below; start there.

**Generalisable lesson.** A `// SAFETY:` comment that states a *whole-system*
precondition — "no CPU has this loaded in CR3" — and is reached from a public
function anyone may call is not a proof, it is an unenforced request. Two
things would have caught this earlier: writing such preconditions as something
the *callee* establishes (which is what `detach_address_space` now does)
rather than something the caller is trusted to have arranged, and treating
"the state that publishes an object as reclaimable" and "the last instant that
object is in use" as one atomic step. Here they were separated by two serial
prints.

**[A] RECURRENCE 2026-08-25 — same silence, different rung, and the PML4 fix
is definitely in the tree.** A boot went silent with the signature this entry
was closed on, three days after it was closed.

*In short:* the machine stopped dead, printing nothing further, immediately
after a test program finished and the kernel started cleaning it up. It is the
same *symptom* as the bug fixed on 2026-08-22, but not the same *cause* — the
2026-08-22 fix is present and working. Something else in the few instructions
after a process is reaped can still wedge the machine, roughly once in a few
dozen boots. The very next boot of the identical tree was green.

| | 2026-07-15 / 2026-08-22 | 2026-08-25 |
|---|---|---|
| Rung | `self_test_linux_real_glibc_forkexec` | Path-Z hosted-cc `inline-asm` (`spawn-test-tcc-hosted`, pid 420 / task 387) |
| Last line printed | `[sched] Task 130 exiting` | `[sched] Task 387 exiting` |
| Line that never came | the forkexec verdict | `hosted cc (inline-asm) — /hosted-prog is a 4026-byte dynamic ELF` |
| Serial position | — | line 38 775 of an expected ~46 860 |
| Diagnostics | none | none: 0 `!!` lines, no panic, no `#PF`, no stall report |
| Timeout | 480 s | QEMU's 900 s fired; harness ran 1174 s |

**The fixed path is present and is not the explanation.**
`sched::detach_address_space` exists at `sched/mod.rs:1789` with both call
sites live — `proc/thread.rs:708` (immediately after `on_thread_exit_hook`)
and `sched/mod.rs:1837` (the idempotent backstop in `task_exit`). Per this
entry's own instruction, the page-table path was *not* re-derived. This is a
second cause wearing the first one's symptom.

**What the new rung narrows.** `spawn_hosted_cc` (`proc/spawn.rs:28525`) has a
different, shorter tail than the forkexec harness, and the missing line places
the wedge inside it precisely. After the bounded poll loop breaks with
`reaped = true`, the surviving steps are, in order:

1. `pcb::state(pid)` / `pcb::exit_code(pid)`
2. `thread::on_thread_exit(cc_result.task_id)`
3. `pcb::destroy(cc_result.pid)`
4. — return Ok — then the caller's `Vfs::read_file("/hosted-prog")`
5. `assert_dynamic_elf`, whose **first statement prints the missing line**

Step 5 printing nothing means the wedge is in steps 1–4. Steps 1 and 5 are
cheap and lock-light; the weight is in 2–4. That is exactly where this entry
said to start ("the remaining post-loop suspects … are `Vfs::read_file`/
`Vfs::remove` on the capture file"), and the new rung sharpens it: **the file
being read is the compiler's own output**, a ~4 KiB file that tcc created and
wrote through the VFS and that was closed by the exiting process's fd
teardown, read back on the very next line after that process was destroyed.
"Read a file the dying process just wrote, immediately after destroying it" is
the shape shared by both occurrences — the forkexec harness read a capture
file with the same timing.

**A lead that looked good and is RULED OUT — do not re-derive it.**
Step 2 calls `thread::on_thread_exit(task_id)` on a task that has *already* run
`on_thread_exit` itself: the log proves it, because `[thread] Process 420 has
no threads left — now zombie` is printed from inside that very function and
appears two lines before the freeze. So the function does run twice per
hosted-cc rung, the second time against an address space the first pass
detached — which looks exactly like a double-teardown bug. It is not. Reading
the second pass end to end:

- `thread_clone::on_thread_exit_hook` opens with an **AS-active guard**
  (`thread_clone.rs:287`) computed from `thread::owner_process(task_id)`. The
  first pass already removed the `THREAD_OWNERS` entry, so the second gets
  `None` → `as_active = false` → every user-memory pass (the PI-futex handoff,
  the robust-list walk, the ctid zero-write, the ctid futex wake) is skipped.
  The three table removals that do run — `ROBUST_LIST`, `RSEQ`,
  `CLEAR_CHILD_TID` — are `BTreeMap::remove`, idempotent by construction, and
  the `CLEAR_CHILD_TID` miss returns early.
- `sched::detach_address_space` documents and implements idempotence
  (`sched/mod.rs:1789`): it only acts on `pml4_phys != 0`, and rewrites CR3
  only when `task_id == load_current_task()` — which the harness's own task
  is not.
- `on_thread_exit` then hits `THREAD_OWNERS.lock().remove(&task_id)?` and
  returns `None` on the missing entry, so `release_irqs_for_task`,
  `pcb::remove_thread`, `exit_close_fds` and everything below never run twice.

The second pass is a clean no-op. Cross it off.

**[A] The decisive narrowing: the BSP stopped taking timer interrupts.**
`sched::liveness_boot_deadline_check` (`sched/mod.rs:3049`) runs on **every BSP
tick** — deliberately not on the 500-tick cadence — and prints a
`[liveness] boot-window breadcrumb: Ns armed (…)` line each time armed-elapsed
crosses a 30 s boundary. The healthy re-run emitted 11 of them, the last at
`330s armed`. The hung boot sat for roughly 600 s past its freeze point and
emitted **none**.

Nothing between the interrupt entry and that breadcrumb can block: the tick
path is `rcu::quiescent_state`, `PER_CPU_SCHED.tick`, some relaxed atomic
counters, `hardlockup::kick` (a no-op unless armed), and a **`SCHED.try_lock()`**
— explicitly non-blocking, with the comment saying a missed tick is fine. And
`watchdog_diagnostic` is unconditional; it only counts the bytes the closure
emits. So a breadcrumb is emitted unless the tick itself never happens.

That rules out the whole family of hypotheses this entry has accumulated
around *lock* wedges. A `crate::sync::Mutex`, a `PreemptSpinMutex` and a raw
`spin::Mutex` all spin with **IF still set**; the timer keeps firing, and the
breadcrumb keeps printing, whatever they are doing. Twenty missed breadcrumbs
means the CPU was not taking interrupts at all — the "BSP-dead total-silence
hang the timer-driven watchdogs cannot see" that `hardlockup::kick`'s own
comment describes.

**Where that points.** Look for an `IF=0` region on the post-reap path, not a
lock. `pcb::destroy` → `destroy_user_address_space` frees every frame of the
address space, and `kernel/src/mm/frame.rs` wraps its allocator critical
sections in `cpu::without_interrupts` in fifteen places (plus six more in
`mm/quarantine.rs`). A free-list walk that loops — a corrupted or cyclic list,
which is history-dependent and therefore intermittent — inside one of those is
silent by construction, cannot be preempted, cannot report itself, and freezes
exactly where both occurrences froze. Neither `blkdev.rs` nor `virtio/` uses
`without_interrupts` at all, so the `Vfs::read_file` suspect from the older
narrowing is the *less* likely half of step 4; the frame-freeing half is the
more likely.

**The next occurrence must produce a RIP, and today's could not.** The only
detectors that can see an `IF=0` wedge are host-side or NMI-driven, and both
are opt-in: `--hard-lockup-watchdog` (§61, kept opt-in by operator decision so
the guest's PCI topology is unchanged on shared runs) and `--stall-secs=N`
(host-side `info registers` via the QEMU monitor, which changes nothing in the
guest at all). Neither was on, because nobody knows in advance which boot will
be the one in a few dozen that hangs.

**Repro status: intermittent, ~1 in a few dozen.** The immediately-following
boot of the *identical* tree, run with `--hard-lockup-watchdog` specifically to
capture the wedged guest RIP, reached `BOOT_OK` in 395 s with the `inline-asm`

**Repro status: intermittent, ~1 in a few dozen.** The immediately-following
boot of the *identical* tree, run with `--hard-lockup-watchdog` specifically to
capture the wedged guest RIP, reached `BOOT_OK` in 395 s with the `inline-asm`
rung green — so **no RIP was captured** and the watchdog hint remains the right
first move on the next occurrence. `bench/boot-history.jsonl` now records 425
boots, 110 not clean.

**Not caused by the change it surfaced under.** The tree under test was the
kshell option-refusal sweep (`68d5483e6`), which touches only shell command
parsers and a self-test rung that runs at serial line ~40 940 — some 2 000
lines *after* the freeze point, and in a subsystem with no relationship to
process teardown. As in 2026-07-15, the change only perturbed timing.
