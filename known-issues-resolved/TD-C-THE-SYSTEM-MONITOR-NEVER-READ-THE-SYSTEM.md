## TD-C-THE-SYSTEM-MONITOR-NEVER-READ-THE-SYSTEM -- FIXED 2026-09-15

**In short:** the system monitor showed fifteen processes, a memory total, a
CPU percentage and load averages. All of it was written into the program by
`load_demo_data` at startup. `refresh()` — the method whose name says it
re-reads the machine — sorted the list it already had and read nothing at all.
It now reads the real `/proc`, including the machine's hostname and kernel
release, and says plainly when it cannot.

**Date:** 2026-09-15. **Lane:** C. The other half of
`TD-C-TWO-PROCESS-MANAGERS-REPORTED-KILLING-PROCESSES-THEY-NEVER-TOUCHED`,
whose fix repaired this app's *claims* and left its *data*.

**The comment was a promise, and this is it being kept.** `main` read:

```rust
// Until a real process source exists this is what there is to show. It is
// loaded here rather than in `new` so that the moment a source arrives,
// this is the one line that changes.
monitor.load_demo_data();
```

That was honest and well-judged when written. `procinfo` — the shared `/proc`
reader lane B built at lane C's request — had existed for days, so the source
had arrived and nobody came back. **The value of a comment like that is
entirely in someone returning to it**, and what makes that likely is not the
comment; it is a check that notices. `apps/procexplorer` had the identical
comment, also stale, found the same afternoon.

**What is read now:** every process `/proc` admits to (pid, name, state,
resident memory, thread count), plus memory totals, load averages, uptime, CPU
model, hostname and kernel release. Two numbers are deliberately *not*
produced:

* `cpu_percent` stays 0.0. A percentage needs two samples of a counter and a
  refresh has one. Inventing it is the mistake this whole change undoes.
* A process's age is `system uptime − its start time`, because `/proc` does not
  publish an age. That arithmetic is pinned by a test, and the sabotage run
  confirmed that returning the start time instead goes red.

**A fixture caught a wrong path, which is the point of asserting values.** The
first version of the system test wrote `/proc/hostname`; `procinfo` reads
`sys/kernel/hostname`. The test failed with `left: "" right: "slate-test"`
rather than passing over a field that was never read.

**One constant, one derivation.** `apps/procexplorer` defined its own
`TICKS_PER_SECOND: u64 = 100` beside `procinfo::TICKS_PER_SEC`, which is the
same fact about the same files written twice. It now uses `procinfo`'s. Two
copies of a kernel constant is how one program keeps working across a change
and the other quietly stops.

**Verified by sabotage**, five claims, each broken with an edit that still
compiles: the process list not read, a process's age reported as its start
time, the system figures not read, the kernel release taken as the whole
`/proc/version` line, and an unreadable `/proc` passed over in silence. All
five went red.
