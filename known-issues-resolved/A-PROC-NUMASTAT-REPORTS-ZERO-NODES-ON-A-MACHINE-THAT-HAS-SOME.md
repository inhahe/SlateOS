## `A-PROC-NUMASTAT-REPORTS-ZERO-NODES-ON-A-MACHINE-THAT-HAS-SOME` (lane A, 2026-08-26)

**Status:** ✅ FIXED 2026-08-26 (`266ba1ea5`) — `numastat::adopt_topology()` now
runs from `main.rs` right after `numa::init()`, exactly as the proper fix below
described, and `/proc/numastat` reports the real node count, per-node memory and
per-node CPU sets. Verified in QEMU: `[numastat] adopted 1 of 1 node(s) from
numa topology (UMA fallback), 1 CPU(s) placed`.

Three things worth carrying forward:

* **The distances are still empty, on purpose**, for the reason argued below —
  `numa::distance()` is a uniform-remote-cost model, not the SLIT, and
  `NodeDistance` cannot say which it is holding. The refusal is documented at
  all three places a future reader might try to "fix" it. design-decisions.md
  §618 records the general form of this argument.
* **A second self-test, `self_test_adoption()`, was added** rather than putting
  assertions in `adopt_topology()` — production code, where the project's
  `unwrap_used`/`panic` lints and `scan-unwrap.py` both apply. Its sharpest rung
  is the CPU tally: node counts and memory sizes are copied straight across and
  would have to be corrupted to differ, but the CPU sets are *derived*, so a CPU
  whose node reports itself absent vanishes silently while every other number
  stays right.
* **The CPU count is threaded from `adopt_topology` into `self_test_adoption`
  rather than re-read.** See the follow-up entry
  `A-NUMASTAT-CPU-SETS-ARE-A-BOOT-SNAPSHOT-NOT-A-HOTPLUG-VIEW` — a fresh
  `smp::cpu_count()` in the check would have been a genuine boot-failing flake.

**Original report.**

**In short:** NUMA is the fact that on a big machine, memory is divided into
banks ("nodes") and each CPU is nearer to some banks than others. The kernel
*does* work out that layout at boot — it reads it from a firmware table, and
falls back to "one node holding all the RAM" when there is no such table.
`/proc/numastat`, the file that is supposed to report that layout, says
`nodes: 0` on every machine anyway. Zero nodes is not a possible state: a
machine with memory has at least one. The file is not empty-because-unknown,
it is wrong.

**Where.** Two modules that never met:

| | `kernel/src/numa.rs` | `kernel/src/fs/numastat.rs` |
|---|---|---|
| What it is | the real topology | the reporting face |
| Fed by | `numa::init()` at `main.rs:6325` — parses the ACPI SRAT, else `init_uma()` | `register_node()` / `set_distance()` |
| Called by | boot | **nothing** |

`grep` for callers of `numastat::register_node`, `set_distance`,
`record_local_alloc`, `record_remote_alloc`, `record_access`,
`record_migration` outside the module itself returns nothing. The table is
therefore permanently empty, and `gen_numastat` (`procfs.rs:11387`) renders
that emptiness as `nodes: 0` followed by no rows.

**It is documented as wired.** `init_defaults`'s doc comment says the rows are
"populated from the ACPI SRAT at bring-up" and that "the memory subsystem is
expected to call `register_node`/`set_distance`". That expectation was never
met. A doc comment asserting a wiring that does not exist is worse than no
comment: it is the reason nobody looked.

**What is genuinely unknowable, and must stay zero.** Only the *topology* is
available today. The allocation, access-latency and migration counters have no
producer — the frame allocator does not record which node a page came from —
so they must keep reading zero. The fix must populate what is known without
inventing what is not.

**Distances must stay empty too, and this is the subtle part.** It is tempting
to fill the distance matrix from `numa::distance()`, which is right there. But
that function's own doc says "We don't parse SLIT yet, so we assume uniform
remote access cost" — it returns 10 for same-node and 20 for everything else,
a model, not a measurement. `numastat::NodeDistance` has no field
distinguishing modelled from measured, so writing those numbers into
`/proc/numastat` would launder a guess into a reading. That is the same
failure as the seed data removed from this module earlier, arriving by a
longer route.

**The proper fix.** A `numastat::adopt_topology()` called from `main.rs` right
after `numa::init()`, registering one row per present node with its real
memory size and the CPU set taken from `numa::cpu_node(cpu)` for each online
CPU — the same map the scheduler places threads by, so the two instruments
agree by construction rather than by coincidence. It must no-op when `numa`
has not initialised yet (detectable: no node reports `present`), so that
running `numastat test` from kshell can call it again to restore the real
topology the self-test wipes, instead of leaving the table empty for the rest
of the boot.

**How it was found.** Auditing the modules whose fabricated seed data had been
removed, to check whether removing the seed had left them reporting nothing
where something real was available. For `signalq` the emptiness was correct —
no signals had been sent. For `numastat` it was not: the data existed one
module away.
