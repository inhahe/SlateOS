## `A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH` (lane A, 2026-08-26) — **open**, 503 of 1928

**In short:** the kernel has ~430 small modules that each keep a table of numbers
and print it — how much swap is compressed, which signals a process has blocked,
how much memory a control group is using. **520 of the functions that change
those numbers cannot be called by anything**, because the only code that
mentions them is the module they live in. The visible effect is not a missing
feature. It is that a column which should be able to go *down* can only ever go
*up*, and a counter that cannot fall does not look like a gap — it looks like
data.

Measured by `scripts/check-unreachable-mutators.py` (reporting tool, always exits
0). **First measurement, 2026-08-26: 520 mutators with no caller outside their
own module, across 222 modules, out of 1940 mutators in 428 files** -- about 27%.

**Re-measured 2026-09-10: 504 across 219 modules, of 1928 in 430 files.**

**And 504 is not 503 because the tool was undercounting.** Its reachability
test was the substring `f"{module}::{name}("`, and `a11y` is a suffix of
`inputa11y` -- a real `inputa11y::set_filter_keys(` call in `kshell.rs`
contains the substring `a11y::set_filter_keys(`, so `a11y`'s own unreachable
mutator was read as reachable. There is no `a11y::set_filter_keys` caller
anywhere. 21 of the 429 module names are suffixes of another module's name
(42 ordered pairs: `ar` inside `sidebar`/`taskbar`/`tar`, `cache` inside
`pagecache`/`fscache`, plus `vfs`, `index`, `policy` and others), so the class
was live in 21 modules and had fired in one. **The August figure of 520 was
taken with the same bug and is an undercount too.**

It was found by making the tool 5.6x faster -- 70 s to 13 s, by indexing the
sources once instead of re-scanning every file per mutator -- and then
checking the answer had not changed. It had, by one, and the one was real. The
speed was the goal and the bug was the finding; had the count been taken on
trust because the refactor was "only" a speedup, neither would have surfaced.

### Who can actually feed these, measured 2026-09-10

504 is not one backlog, it is two, and only one of them is lane A's. Split by the
verb in the name, which says who observes the event:

| verb | count | natural writer |
|---|---|---|
| `set_*` | 273 | a userspace settings program -- another lane's |
| `record_*` | **105** | **the kernel itself, today** |
| `remove_*` / `add_*` / `register_*` / `unregister_*` / `clear_*` / `create_*` | 118 | mixed; mostly inventory a userspace enumerator owns |

**501 of the 504 sit in modules the shell reads**, so these are not dead internal
code -- they are 218 tables a user can display whose values can only move one
way. And **no module is wholly unreferenced** (checked: 0 of 429), so the gap is
always the specific operation, never the whole table.

The 105 `record_*` are where a lane A burn-down *could* be, because the kernel is
the only thing that knows when the event happened. But a count is not a work
list, and the first version of this section said it was:

**Corrected the same hour.** Of the 64 modules, **4 (10 functions) document their
own `record_*` as deliberately uncalled**, and two of them -- `netdev` and
`pagecache` -- were named as top targets in the table this replaces.
`pagecache`'s module header argues the case at length and is right: it
**projects** `mm::page_cache::stats()` into a synthetic row at read time, and
does *not* call `record_hit` on the fast path because that would take its spin
lock and do a per-device string compare inside a page-cache lookup -- an
operation whose entire purpose is to be faster than touching the disk. Its
`record_*` exist for a future per-device source and for the self-test. Wiring
them would have added a second copy of a number the kernel already keeps, which
is the two-sources-that-can-disagree defect this file is full of.

The four so documented: `cloudsync`, `mobilelink`, `netdev`, `pagecache`.

That leaves **95 functions in 60 modules whose headers say nothing** -- and
"undocumented" is not "a defect" either. Each still needs reading before it is
work. What the split buys is that the reading is bounded and ordered, not that
the answer is known:

| module | unreachable `record_*` |
|---|---|
| `diskio` | `record_read`, `record_read_error`, `record_write`, `record_write_error` |
| `dmastat` | `record_fault`, `record_map`, `record_transfer`, `record_unmap` |
| `numastat` | `record_access`, `record_local_alloc`, `record_migration`, `record_remote_alloc` |
| `tlbstat` | `record_flush`, `record_hit`, `record_miss`, `record_shootdown` |
| `cpucache` | `record_eviction`, `record_hit`, `record_miss` |
| `cpustat` | `record_context_switch`, `record_interrupt`, `record_time` |
| `pagestat` | `record_alloc`, `record_free`, `record_reclaim` |
| `schedclass` | `record_migration`, `record_slice`, `record_switch` |

### Checked one: `tlbstat`, and it is `pagecache` again

Picked the first target off the table above and read it before touching it.
`fs/tlbstat.rs` exposes `record_flush`, `record_shootdown`, `record_hit` and
`record_miss`, none reachable, and its header documents an intended integration
rather than a deliberate absence — so by the classification above it is a gap.

It is not. `kernel/src/tlb.rs` already keeps `RANGE_FLUSH_COUNT`,
`FULL_FLUSH_COUNT`, `TOTAL_PAGES_FLUSHED` and `IPI_FLUSH_COUNT` as atomics, with a
`stats()` accessor. Wiring `record_flush` from the flush path would put a second
counter beside the kernel's own for one quantity — two sources that can disagree —
and would do it on a path that runs on every address-space change. That is
precisely the trade `fs/pagecache.rs` documents and declines, and its remedy
applies unchanged: **project `tlb::stats()` at read time, do not call `record_*`
on the hot path.**

Two of the four are not wireable at all, for a reason worth writing down rather
than discovering twice: `record_hit` and `record_miss` describe events only the
CPU's performance counters can observe. The kernel cannot know a TLB hit happened.
Those two are waiting on a PMU, not on wiring.

**And the obvious shortcut does not work.** Looking for "a non-fs module that
already counts this" by searching for the display module's *name* finds nothing
for any of the eight — because the counter for `tlbstat` lives in `tlb.rs`, for
`pagestat` it would be in the allocator, and so on. The names do not match by
construction. Each module needs reading; there is no grep that answers this, and a
sweep that appears to answer it will answer "no duplicate exists" for every entry,
which is the most expensive possible wrong answer here.

So the burn-down's real first question is not "is this unreachable" — the tool
answers that — but "does the subsystem already count it, and is the path hot".
Three of the four modules examined so far (`pagecache`, `netdev`, `tlbstat`) came
back *already counted*. That is not evidence the other 57 are, and it is strong
evidence against starting any of them by wiring.

**The lesson is the one this whole entry is about, turned on its author.** A tool
counted 504 things; I sorted them by verb, published eight modules as targets,
and two of the eight had headers explaining why they are correct -- one of them
in twenty lines with a performance argument. A mechanical measurement presented
as a work list is the same error as a counter that can only go up: it reads like
data.

Down 16 from August on the corrected basis, so the work is moving, and the paragraph below this one was still quoting the
August figure two weeks later. That is worth more than the seventeen: this entry
is about code a tool can see and nothing calls, and its own headline number had
drifted from the tool that produces it, because the number lives in prose and the
tool exits 0 whatever it finds. The heading was right and the body was not, which
is the harder direction to notice.

The remedy is the one applied to `check-linux-only-capabilities.py` earlier the
same day: make the count a **ratchet** -- pin it, let it fall, fail when it
rises -- so the number is asserted by something that runs rather than restated by
somebody who remembered. A reporting tool that always exits 0 records a fact
nobody is obliged to keep true. A function called only by its
own `self_test` counts as unreachable, and that is the important case: the test
proves the code works, which is exactly why the gap survives review.

**How it was found.** Not by looking for it. Four consecutive batches of the
`kshell` guessed-value burn-down each surfaced the same extra defect on the way
past:

| batch | module | unreachable operation | what that made monotonic |
|---|---|---|---|
| 30 | `rqstat` | `register_cpu` | every other subcommand returned `NotFound`, always |
| 31 | `zramstat` | `record_discard` | `mem_used` could only rise |
| 32 | `signalq` | `unblock` | `blocked_mask` could only gain bits |

Three in a row is what prompted measuring the population, and the measurement
says it is systemic rather than a property of those three.

**The cause is consistent, and it explains the shape of the gap.** These
modules' shell arms were written to *demonstrate* a feature, and demonstrating
means showing a counter go up. The operation that brings it back down — the
discard, the unblock, the release, the `remove_*` — has no demonstration value,
so it was never wired to anything. That is why the unreachable set is so heavily
weighted toward `set_*` and the un-doing verbs rather than being a random 27%.

**Why it matters more than "dead code".** Three separate consequences, in
increasing order of harm:

1. The operation is untested against a real caller. The self-test exercises it
   in isolation with fixtures it built itself.
2. The number the module reports is *wrong in a specific direction* — always
   the accumulating one — and nothing in the output says a whole class of
   transition is missing.
3. Where the module is meant to be fed by a real subsystem (`netdev::record_rx`,
   `numastat::record_local_alloc`, `pagecache::record_hit`), an unreachable
   recorder means the table is not merely incomplete but **empty of the thing it
   exists to count**, while still printing totals of zero as though zero were
   measured.

**Check the table is even open before wiring a recorder into it.** Most of these
modules' `self_test`s close their `STATE` when they finish and nothing reopens
it, so a newly-wired `record_*` returns `NotSupported` and is discarded, and the
number in `/proc` does not move — see
`A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT`, which lists the 18
modules nothing can open at all. That list and the high-value targets below are
nearly the same set, so for most of them it is the first step, not a footnote.

**The proper fix is per-module and is a burn-down, not a patch.** For each
module, decide which of the two it is: an accounting table a *subsystem* should
be feeding — in which case wire the call site in the subsystem, which is the
real fix and the valuable one — or a table only the shell drives, in which case
add the missing subcommand as batches 30–32 did. Do not "fix" it by deleting the
function: the function is usually right and the caller is what is missing, and
deleting it would remove the evidence that a number is unfed.

**Do not confuse this with the guessed-value ledger**
(`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-…`). That one is about commands that
invent an operand they could not read; this one is about operations no command
offers at all. They keep appearing together because both come from the same
habit — writing the arm that shows the feature working — but they are counted
separately and cleared separately.

**If you wire a registrar at boot, check it runs AFTER the self-test.** The
obvious wiring for a `register_*` mutator is to call it from the owning
subsystem's `init()`. That is right in principle and silently wrong in most
cases, because these `self_test`s begin *and end* by resetting their table —
which destroys rows a real subsystem registered earlier, not just the test's own
fixtures. `main.rs` line numbers decide it, and they mostly go the wrong way:

| registrar | self-test | result if wired naively |
|---|---|---|
| `net::init()` — main.rs:1311 | `fs::netdev::self_test()` — main.rs:4459 | row wiped, table empty for the boot |
| `numa::init()` — main.rs:6324 | `fs::numastat::self_test()` — main.rs:4185 | survives (registrar runs later) |

So before writing `fs::foo::register_x()` into a subsystem's `init`, grep both
line numbers in `main.rs`. If the registrar loses, the options are: move the
call to a later boot step; have the self-test restore rather than wipe (as
`fs::associations::self_test` already does — it snapshots `stats()` and does not
reset, which is why `register_defaults()` on the line above it survives); or
project at read time and register nothing, which is what `netdev` and
`pagecache` do and is usually best when the subsystem already keeps the numbers.
This is design-decisions.md §612's accepted liability showing up in the very
next task; it has not bitten anything yet because nothing has been wired the
naive way.

**Burn-down log.** Count at the head of this entry is
`scripts/check-unreachable-mutators.py`'s, not hand arithmetic.

| date | module | 520 → | what changed |
|---|---|---|---|
| 2026-08-26 | `pagecache` | 516 (unchanged, and that is the point) | category 3, fixed *without* moving this count. `mm/page_cache.rs` carries the comment "Counters (monitoring; mirror what fs::pagecache exposes for stats)" above its `STAT_HITS`/`STAT_MISSES`/`STAT_EVICTIONS` — the two halves of one intent, never joined, so `/proc/pagecache` reported a 0.00% hit rate while the cache served VFS reads and demand paging. Joined at *read* time (a projected `kernel` row) rather than by calling `record_hit` on the cache's fast path, which would put this module's spin lock and a per-device string compare inside an operation whose purpose is to beat the disk. The four `record_*` stay unreachable and now legitimately so, reserved for a per-device source that does not exist. **Lesson for the metric: it counts functions, but the defect is an unfed table.** Where the subsystem already keeps free counters, projection is the fix and the count will not move. Do not read a flat count as no progress. |
| 2026-08-26 | `futexstat` | 516 | category 3, the sharpest instance found so far. `procfs.rs` publishes `futexstat::stats()` and `hotspots(10)` under `/proc`, and the `futexstat` shell command prints the same table — while all four of `record_wait` / `record_wake` / `record_timeout` / `record_contention` were unreachable. So `/proc`'s futex hotspot list was permanently empty and its wait/wake/timeout totals permanently zero, on a kernel whose futex implementation works. A reader takes that as *measured* zero contention. Fixed by wiring the four recorders into `kernel/src/ipc/futex.rs` at the real blocking and waking points. |
| 2026-08-26 | `irqstat` | 515 (and 222 → 220 modules, 1940 → 1937 mutators) | category 3, the third projection — and **the first one where deleting the mutators was right**, against this entry's own standing advice. `/proc/irqstat` and the `irqstat` command served per-IRQ-line counts, per-CPU interrupt totals and ISR latency while all six of `record`/`record_latency`/`mark_spurious`/`register_irq`/`register_cpu`/`init_defaults` were unreachable; before that the table was seeded with five fictional IRQ lines and four fictional per-CPU rows. Rewritten as a stateless projection of `idt::vector_counts()` — no `STATE`, no lock, nothing to seed or reset — which also makes `A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT` inapplicable to this module by construction rather than by care. **Why deletion, when the rule above says the function is usually right and the caller is what is missing:** here the functions were the wrong *shape*, not merely uncalled. `record(cpu, irq)` presumes a per-CPU per-line table the kernel's counter architecture cannot feed (`VECTOR_COUNTS` is one flat global array), and `IrqLine::spurious`/`affinity_mask` and the whole `CpuIrqState` latency pair had no source at all — they *were* the fabrication surface. Keeping them would not have preserved evidence of an unfed number; it would have preserved an invitation to the wrong fix, namely a second counter of the same event on the ISR path, which is two numbers that can disagree with nothing to say which is right. The `IrqType` enum went the same way: `Timer`/`Keyboard`/`Disk`/`Network`/`Usb`/`Gpu` is a guess about which device sits behind a line, and the kernel does not know that, so it is now `Timer`/`Device`/`Ipi`/`Spurious`/`Unassigned` — derived from the vector number, which the kernel *does* know. **Read the count change with care:** unlike the two rows below, this one moves the metric by removing functions rather than by wiring callers, so the drop is not five modules' worth of progress. That is the mirror of the `pagecache` lesson — the metric counts functions, and the defect is an unfed table. |
| 2026-08-26 | `netdev` | 516 (unchanged, and that is the point) | category 3, the second projection. `/proc/netdev` listed no interfaces and reported `total_rx_bytes: 0` on a kernel that had just completed a DHCP exchange, because all six of `record_rx`/`record_tx`/`record_error`/`record_drop`/`register_iface`/`set_link_state` were unreachable -- while `net::interface` counted every frame in six relaxed atomics that `netstat -i` was already printing. The two halves of one intent, never joined, exactly as with `pagecache`. Joined at *read* time (a projected `kernel` row) rather than by calling `record_rx` per frame, which would put this module's spin lock and a string compare per interface on the path of every packet. The six `record_*`/`register_*` stay unreachable and now legitimately so, reserved for a per-NIC source that does not exist. **New finding while naming the row:** `net::interface`'s counters are not one NIC's -- `net::veth::poll` records into the same atomics, so the total includes frames that never reached the wire. The projected row is therefore named `kernel` and not `eth0`, and the conflation is logged separately as `A-NET-INTERFACE-COUNTERS-CONFLATE-THE-NIC-WITH-EVERY-VETH-PAIR`. |
| 2026-08-30 | `perfmon` | 503 (and 220 → 219 modules, 1941 → 1928 mutators) | category 3, the fourth projection, and the largest single module cleared: **twelve** unreachable mutators, all of them. `/proc/perfmon` printed `CPU samples: 0 / Mem samples: 0 / Disk samples: 0 / Net samples: 0` on every boot for the life of the kernel, and `perfmon cpu` answered "No CPU samples" — while `kstat` had been sampling free frames, heap bytes, pressure, runnable/live task counts and per-CPU utilisation into a 60-entry ring *once a second since boot*. The `pagecache`/`netdev` shape exactly: two halves of one intent, never joined. Rewritten so every history is projected from `kstat::recent()` at read time and the module stores only the two alert thresholds, which are policy rather than measurement. **Both deletion rules from the `irqstat` row applied again, and more widely.** `CpuSample`'s `system_pct`/`user_pct` (the scheduler publishes only `(total, idle)` per CPU), `freq_mhz` and `temp_mc` (no frequency or thermal driver exists), and `process_count`/`thread_count` (no *historical* source) were deleted, as were `MemSample`'s `cached_bytes`/`swap_used_bytes`/`page_faults` and the whole of `DiskSample`/`NetSample` — the latter two because their counters live behind spin locks the timer softirq cannot take, so no sampler can ever fill them. `/proc/perfmon` now ends with a line pointing at `/proc/diskstat` and `/proc/netdev` for the cumulative figures, so the absence is stated rather than silent. **Also removed: two knobs that moved and changed nothing** — `perfmon interval <ms>` and `set_max_samples` stored a value that `get_config()` read back while nothing sampled any faster, because this module had no sampler; a knob that answers "did that work?" with a false yes is worse than a fixed value. Interval and depth are now reported from `kstat::sample_interval_ms()`/`history_depth()` and labelled not-settable. **And the alert model changed shape:** alerts are derived from the newest sample rather than appended per over-threshold sample, so `id`/`dismissed` and `perfmon dismiss` are gone — a CPU pinned at 100% used to accumulate identical rows, and dismissing them made a still-overloaded machine report itself healthy. Two new shell arms, `perfmon cpu-alert <pct>` and `perfmon mem-alert <pct>`, make the surviving setters reachable. Full rationale, including what was argued *against* deleting the fields: `design-decisions.md` §641. |

**A blind spot in the metric, found while writing that row and worth knowing
before the next module.** `scripts/check-unreachable-mutators.py` decides
reachability by searching other files for the literal text `module::name(` — a
*call*. Passing a mutator as a **function item** to a shared helper
(`set_threshold(&parts, "cpu", perfmon::set_cpu_alert)`) is a real caller that
the scanner cannot see, and it reported both `perfmon` setters as callerless
after they had been wired up. Wrapping in a closure makes the text match but
trips `clippy::redundant_closure`, so the resolution was to split the helper
into a parse half and a report half, leaving each shell arm to call its setter
directly. Two consequences: a future refactor that tidies those two arms back
into one callback will silently re-break the measurement without changing any
behaviour, and — more importantly — **the 503 figure may undercount reachability
elsewhere for the same reason**, so treat it as an upper bound on the problem
rather than an exact census.

The futexstat fix is worth reading before doing the next category-3 module,
because it ran into the constraint that shapes all of them: **you usually cannot
put the accounting call beside the existing counter.** `ipc::stats::futex_*` are
plain relaxed atomics and every one of their call sites bumps them while
`FUTEX_TABLE` is held; `fs::futexstat` keeps a `Vec` behind a spin lock, so the
same placement would take a second lock inside the futex table's critical
section and invert a lock order (`scripts/check-recursive-locks.py` is the live
gate for that). The wiring therefore went in as four named `note_*` helpers with
a module note saying they must only be called from outside the lock, placed at
the points that are provably outside it: after the enqueue block in
`futex_wait_bitset` and `futex_wait_bitset_timeout`, after the wake loop in
`futex_wake_bitset` and `requeue_inner`.

Two smaller judgment calls recorded there, in case a later reader disagrees:
`requeue_inner` attributes its wake to `addr1` only, because the requeued tasks
are still blocked (now on `addr2`) and so are not a wake of anything; and the
`timeout_ns == 0` non-blocking "try" return is not counted as a wait *or* a
timeout, because it never enqueued and never blocked, so counting it would
report contention that did not happen.

**Still unwired in that file, deliberately:** `futex_wait_multiple` and the
priority-inheritance paths (`lock_pi_inner`, `futex_wait_requeue_pi`,
`futex_cmp_requeue_pi`), whose `ipc::stats::futex_*` sites sit at roughly lines
461, 630, 2363 and 2666. They are not instrumented because their enclosing lock
scope was not read closely enough to prove a call would be outside
`FUTEX_TABLE`, and a wrong answer there is a lock-order inversion rather than a
wrong number. The consequence while they stay unwired is bounded and known:
`/proc`'s futex table under-counts PI and multi-wait contention, rather than
reporting zero for everything.

**Not a regression.** True of each module since it was written.
