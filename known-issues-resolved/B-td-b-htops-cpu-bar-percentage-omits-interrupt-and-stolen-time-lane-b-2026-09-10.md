## ~~TD-B-HTOPS-CPU-BAR-PERCENTAGE-OMITS-INTERRUPT-AND-STOLEN-TIME~~ (lane B, 2026-09-10) -- FIXED the same day

**In short:** a CPU doing nothing but servicing interrupts shows as 0% busy in
`htop`.

**Where.** `userspace/htop/src/main.rs`, `CpuBar::percent` -- the number beside
each CPU bar is the sum of the three segments actually drawn (`user`, `system`,
`nice`) and therefore omits `irq`, `softirq` and `steal`. `procinfo`'s own
`CpuTimes::busy` counts all of them, so the crate and its caller currently
disagree about what "busy" means.

**Why it is a percentage and not just a missing colour.** Under QEMU, `steal`
is the field that is reliably non-zero, and a network- or disk-heavy workload
puts real time in `softirq`. Both are the machine being unavailable to the user
and both read as idle.

**The fix is a UI change**, which is why it is separate: the bar needs a fourth
segment before the number can include a fourth category, or the number stops
matching the bar beside it -- and a percentage that disagrees with the picture
next to it is worse than one that under-reports consistently.

~~**Pinned by a test** (`interrupt_and_stolen_time_are_not_counted_yet`) that
asserts the current under-report *and* asserts `busy()` sees all of it, so
fixing this is a deliberate change with a failing test rather than a silent
one.~~

**Fixed 2026-09-10.** `CpuBar` gained a fourth field, `overhead` =
`irq + softirq + steal`, drawn as a fourth magenta segment -- which is the
colour real `htop` uses for interrupt time. Both halves moved together, which
was the whole reason this was a separate entry: the number beside the bar is
the sum of what is *drawn*, so it could not grow a fourth category until the
bar did.

**The pinning test did its job.** It failed on the first build after the
change, which is what it existed for, and is now its own inverse
(`interrupt_and_stolen_time_are_counted`). A CPU spending the whole interval on
interrupts and stolen time reads 100%.

**`percent()` still is not derived from `busy()`**, and that is deliberate.
They are two independent computations -- `percent()` sums the four drawn
segments, `busy()` is `total - idle - iowait` -- and
`percent_matches_the_crates_definition_of_busy` asserts they agree. Deriving
one from the other would make them agree *by construction* and hide a drift: if
`busy()` ever started counting `iowait`, a derived percentage would follow it
silently and the number would stop matching the picture beside it. The test is
what says so instead.

**Two more tests came out of it**, covering something that was never tested:
four independently-rounded fractions can each round up and ask for more cells
than the bar has, and a bar that overruns its width corrupts the memory gauge
drawn beside it. The clamp is a running total rather than four separate
`min(width)` calls -- which is the version that would let the second segment
reuse the first's room. `render_cpu_bar` became an associated function to make
that reachable from a test; it never read `self`.
