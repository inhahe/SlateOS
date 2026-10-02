### A-KILLED-THREAD-ON-ANOTHER-CPU-OUTLIVED-ITS-PAGE-TABLES -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** on a machine with more than one CPU, a thread killed while
another CPU was running it (a signal that ends its process, a crash in a
sibling) is only *marked* dead: that CPU runs it on until its next switch.
The process could be reaped -- its page tables freed -- in that window. And
the switch itself did not leave those tables: it compared the two tasks'
recorded address spaces, the dead thread's record had been cleared to
"kernel", and a switch to a kernel task looked like no change. The CPU then
ran kernel tasks on freed page tables until it next ran a user program. The
boot test runs one CPU, so it could not see this.

**Where:** `kernel/src/sched/mod.rs` (both switch paths), `kernel/src/proc/pcb.rs`
(the address-space teardown), `kernel/src/proc/thread.rs` (`on_thread_exit`).

**Fixed:** a switch compares against the live CR3 (`load_address_space`); a
thread killed while on a CPU is recorded (`Process::killed_on_cpu`), and its
process's address space is freed only once `sched::task_is_on_cpu` says it is
off -- deferred if need be, and drained by the boot thread's idle loop. Tests:
`sched`'s `test_task_is_on_cpu` and `test_load_address_space_uses_live_cr3`,
`pcb`'s `test_deferred_address_space`.
