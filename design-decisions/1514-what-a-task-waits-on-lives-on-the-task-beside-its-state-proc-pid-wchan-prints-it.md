## 1514. What a task waits on lives on the task, beside its state; `/proc/<pid>/wchan` prints it with its holder

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** `ps`, `top` and the process explorer can show what a stuck
program is waiting for -- a lock, a pipe, a child, another program -- in a
column called WCHAN. The kernel had a table meant for this, but nothing ever
wrote to it, so every program read as "not waiting" and there was no
`/proc/<pid>/wchan` at all. Now every place in the kernel that puts a thread
to sleep says what for, the scheduler keeps that next to the thread's
"blocked" state, and `/proc/<pid>/wchan` prints it in one line -- with *who
holds* the thing waited on wherever the kernel knows, which is what lane E's
explorer needs to find deadlocks
(`requests/e-adf-what-the-process-explorer-still-cannot-ask.md`, part 1).

**What changed:**
- **Blocking says what for.** `sched::block_current_on(wait)` beside
  `block_current()`; `ipc::waiters::park_interruptible` takes a `Wait`. The 31
  direct blocking sites and the 22 that park through `park_interruptible`
  describe their wait: a futex by its address, a channel, pipe, eventfd or
  socket by its handle, a child by its pid, a join by the thread. Waits built
  from timed slices (socket reads) say "socket", not "timer"
  (`sleep_ms_interruptible_as`).
- **The record lives on the task.** `Task::wait`, written in the scheduler
  critical section that sets `Blocked`; `sched::wait_of` reads it and answers
  "not waiting" for a task in any other state, `stopped` for a suspended one,
  and `wait` for a blocked one whose blocking code said nothing.
- **What is published:** `/proc/<pid>/wchan` and
  `/proc/<pid>/task/<tid>/wchan` (format below); `/proc/<pid>/stat` field 35
  is 1 while the task waits; the kernel shell's `wchan` lists the same lines;
  the scheduler's hang dumps print each task's last wait.

The line, with no trailing newline, as Linux's:

```text
0                                       not waiting
poll                                    a kind with no argument
futex 0x7f001000                        a kind and its argument
channel 12 holder 34                    ... and the process holding the other end
join 57 holder 34 thread 57             ... and the holding thread, when known
```

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. On the task, under the scheduler lock (chosen)** | the wait is a field of the task, written with `Blocked` | exact: one record per task, and it can never disagree with the state; costs one 16-byte store inside a critical section the park already takes | a reader takes the scheduler lock -- as procfs already does to read the state |
| B. Fill the existing lock-free table at every park | readers take no lock | | two tasks whose ids differ by 1024 share a slot, so one reads as "unknown"; the record and the state are written apart, so a woken task could read as waiting; two more atomic stores on every park and wake |

**Smaller decisions:**

| decision | alternative | why this one |
|---|---|---|
| The holder goes on the same line: `holder <pid>`, then `thread <tid>` | a second line, as the request suggested | Linux's `wchan` is one line with no newline and `ps -o wchan` prints the file as it is -- a newline would break its column. The first token is still the single word Linux tools expect |
| An argument of 0 is left out | print it | for most kinds 0 means "no argument"; for `child`, plain `child` is "any child" |
| Holders only where the kernel knows: a channel's recorded peer, a named child, a joined thread, a priority-inheritance futex's owner | guess (the last process to touch a pipe, a plain futex's last locker) | a deadlock analyzer that draws a wrong edge finds a cycle that is not there; a missing edge only hides one |
| Kernel locks and wait queues report `mutex` with no argument | their address | a kernel address in a file any process reads would defeat address randomisation |
| A reader who may not inspect the process reads `0` (§1516, added the same day) | refuse, or show everyone | Linux's behaviour: `ps -o wchan` must not fail on other users' processes, and a wait can name a handle or an address in the process |

**Revisit** if the explorer needs holders for pipes and sockets: that means
recording which processes hold each end, which the kernel does not track
today.
