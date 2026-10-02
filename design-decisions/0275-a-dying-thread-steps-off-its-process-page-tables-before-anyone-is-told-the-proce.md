## §275 — A dying thread steps off its process page tables before anyone is told the process can be reaped, rather than the reaper waiting for the thread

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When the last thread of a program finishes, the kernel frees the
"page tables" — the tables the CPU uses to translate the program's memory
addresses into real memory. It was freeing them while that very thread was
still running, because the announcement "this program is finished, its memory
can be reclaimed" was made several milliseconds before the thread actually
stopped. Another task could hear the announcement in that gap and hand the
tables back to the memory allocator; the still-running thread would then be
put back on tables that had been reused for something else, and the machine
would stop dead with no error message — there were no working tables left to
run the error handler with. The fix makes the dying thread move itself onto
the kernel's own page tables *before* the announcement, so by the time anyone
can reclaim the program's tables, nothing is using them. The alternative — a
usage counter that makes the reclaimer wait — was rejected as more machinery
for the same guarantee.

### What was wrong

`pcb::destroy` frees a process's PML4 page (the root page table) and states
its precondition in a `// SAFETY:` comment: *"no threads are running in this
address space, and no CPU has this PML4 loaded in CR3"*. Nothing established
it.

The zombie transition — the moment a process becomes reapable — happens inside
`proc::thread::on_thread_exit`, which runs **on the dying thread**. It wakes
the task parked in `wait4()` and posts `SIGCHLD`, and only *then* returns, so
the thread still has to run `sched::task_exit()`, the exit hooks, and a
`SCHED` acquisition before it switches away. Two `serial_println!` calls alone
cover ~8 ms at 115200 baud against a 10 ms timer tick, so a preemption inside
that window is ordinary. Preempted there, the thread is still `Running` and
its `Task` still records the process PML4, so the scheduler's switch-in path
reloads that (by then freed, possibly reused) frame into CR3.

### The options

**(a) Detach the dying thread from the address space before publishing the
zombie** — clear `Task::pml4_phys` and write CR3 to the kernel PML4.

*What changes:* nothing observable when things work; a class of silent,
unreportable machine stops disappears.

- **For:** it makes the precondition true by construction at the only place
  that can know it — the thread itself. It is one function, called from one
  place (plus an idempotent backstop), and it is testable synchronously: after
  the last thread exits, no task names the PML4. It is exactly what Linux does
  (`exit_mm()` → `switch_mm(&init_mm)`), so the shape is precedented rather
  than invented.
- **Against:** it is a *convention* — a future exit path that publishes a
  zombie without going through `on_thread_exit` would reopen the hole. It also
  costs a TLB flush per thread exit that a lazy scheme could sometimes avoid.

**(b) Reference-count the address space** (`mm_users`/`mm_count`), and make
`pcb::destroy` refuse or defer while the count is non-zero.

*What changes:* also nothing observable; the reclaim happens slightly later,
whenever the last user drops its reference.

- **For:** it enforces the invariant rather than arranging it, so it survives
  new exit paths and SMP without anyone remembering the rule. It is the more
  general answer, and it is also what Linux has (a and b are not exclusive
  there).
- **Against:** every acquire/release site is a place to get the count wrong,
  and getting it wrong yields a *leak* (silent, unbounded) or a *premature
  free* (the exact bug we are fixing) — with no natural test that fails
  loudly. It needs the counter threaded through fork, exec, clone, the
  reaper, and the kernel-thread borrow path, i.e. every place that touches a
  PML4, to buy a guarantee that (a) already provides for the one shape that
  actually occurs here (a dying thread and a reaper).

### Decision

**(a).** The bug is not "many holders, unclear lifetime" — it is "exactly one
holder, and it announced its own death too early." Refcounting is the right
tool for the former and overkill for the latter, and its failure modes are
quieter than the one it would replace. Choosing (a) is also not a bet against
(b): if the kernel later grows genuine multi-holder address-space sharing
(`CLONE_VM` across processes, a reaper that must touch a dead process's
memory), the counter becomes necessary and (a) remains correct underneath it.

The residual risk of (a) — a future exit path that forgets — is mitigated
rather than ignored: `sched::task_exit` calls the detach a second time as an
idempotent backstop, so any path that reaches the scheduler's exit at all is
covered even if it bypassed `on_thread_exit`.

### The generalisable lesson

A `// SAFETY:` comment that states a whole-system precondition — "no CPU has
this loaded in CR3" — and sits behind a public function anyone may call is not
a proof; it is an unenforced request addressed to nobody in particular. Write
such preconditions as something the *callee* establishes, or make the state
that publishes an object as reclaimable and the last instant that object is in
use the same step. Here they were two serial prints apart, which was enough.
