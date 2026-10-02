### [A] The kernel now counts interrupt nesting twice, on two counters with different coverage, and I added the second one this session -- 2026-09-18
**Status:** RESOLVED 2026-09-18, boot-verified on ebb683642 -- NOT by consolidating. The two counters are not duplicates (see the correction below) and both remain. What landed is the attribution fix: vectors 251/252/255 are now charged to IRQ time via `charged_to_irq`, arm 32 untouched, and dispatch_vector still works

**In short:** the kernel tracks how deeply nested it is inside interrupt
handlers. It now does that in two separate places, which disagree: one
counts every kind of interrupt, the other counts two of five kinds. I added
the one that counts everything, today, *because* the existing one counted
two of five -- which is the same thing I criticised a filesystem module for
this morning, done by me, in the same session.

| counter | bumped at | coverage | read by |
|---|---|---|---|
| `cputime::irq_depth` (`cputime.rs:92`) | `apic.rs:989` (timer), `ioapic.rs:726` (device) | **2 of 5** dispatch arms | CPU-time accounting, and the nesting cap at `apic.rs:1006` |
| `idt::HARDIRQ_DEPTH` (`idt.rs:513`) | `dispatch_vector` | **5 of 5** | `in_hardirq()`, for the lockdep context check |

They describe the same physical fact -- hardirq nesting depth on this CPU.

**How it happened, which is the only part that generalises.** dd-948 said a
rule only an interrupt can break needs a check only an interrupt can trip,
so the lockdep marker had to see *every* vector. `cputime`'s counter saw two
of five. I sited a new counter at the one point every vector passes through
rather than extending the existing one to the other three arms -- the
expedient choice, and I did not record it as a choice at the time. That is
`fs/immutable.rs`'s shape exactly: a second store for a capability that
already had one, added because the first did not reach far enough.

**The consequence that already existed, and is now fixable in the same
change.** Vectors 251 (TLB shootdown), 252 (reschedule IPI) and 255
(spurious) never bump `cputime::irq_depth`, so cycles spent in them are
charged to whatever task they interrupted rather than to IRQ time. That was
recorded earlier as an attribution gap worth fixing and not urgent. It is
the *same* gap: the fix for the duplicate is the fix for the attribution.

**The consolidation, costed rather than asserted.** Move the
`enter_irq`/`exit_irq` bracket into `dispatch_vector`, drop the two existing
call sites so nothing is counted twice, and have `in_hardirq()` read
`cputime::irq_depth()`. One counter, 5-of-5 coverage, attribution closed.

`enter_irq` was read before proposing this rather than assumed cheap: it is
an `rdtsc`, a bounds-checked per-CPU lookup and four relaxed atomic
operations, with no lock, no allocation and no fallible path -- a missing
`CPU_TIME` slot returns early. `exit_irq` is the same shape with a
defensive `depth == 0` guard. So the added cost on the three uncovered
vectors is one `rdtsc` plus a few relaxed atomics per interrupt.

**The tradeoff, which is why this is a decision and not a cleanup.** Those
three vectors are the ones whose handlers are deliberately minimal --
atomics, `invlpg`, EOI. Adding an `rdtsc` to the reschedule IPI and to
*spurious* interrupts is a real cost on the hottest, least useful paths, and
spurious interrupts are exactly the ones you get a storm of when something
is wrong. Against that: two counters for one fact is a model that will drift
the first time someone changes one of them, and the accounting is wrong
today in a way nobody can see from `/proc`.

**Not applied yet**, and deliberately not applied in a hurry: it touches
`dispatch_vector`, which every interrupt in the system passes through, and
the correct order of `enter_irq` relative to EOI and `softirq::process_pending`
is the kind of thing that is obvious in review and wrong at runtime. It
needs its own boot, not a ride on one already in flight.

#### Corrected within the hour: consolidation is the WRONG fix, and the reason is a real one

Reading the two counters properly instead of comparing their shapes turns
this from "a duplicate to merge" into "two things that look alike". Both
points came out of the code, not from reconsidering:

**1. `enter_hardirq_for_test()` fabricates interrupt context on purpose.**
It exists so lockdep's negative control can fire -- a check that has never
fired is indistinguishable from one that cannot. If `in_hardirq()` read
`cputime::irq_depth()`, that helper would enter `enter_irq`, which does
`if prev_depth == 0 { irq_enter_tsc.store(now); irq_count.fetch_add(1) }`
and charges a cycle delta on the matching exit. **A self-test would inject
fake interrupts and fake IRQ cycles into `/proc`.** Merging the counters
would corrupt the accounting with the lockdep control's own fixtures --
dd-942's corpus problem, caused by the merge that was supposed to tidy up.

**2. The two have different correctness directions.** A lock-context marker
must be *conservative*: if unsure, say interrupt context, because a missed
report is a missed deadlock. Cycle accounting must be *exact*: over-count
and `/proc` lies. Those pull opposite ways, and one counter cannot serve
both once they ever disagree.

**And the merge had a live hazard I would have shipped.** The nesting cap at
`apic.rs:1006` reads `irq_depth() > 1`. Moving `enter_irq` into
`dispatch_vector` **without** removing `apic.rs:989` leaves depth at 2 for an
ordinary, non-nested timer tick -- so the cap would treat *every* timer
interrupt as nested and throttle it. That is not a subtle regression, and I
only saw it while writing down the ordering.

**Revised plan, which keeps both and makes the split deliberate:**

| counter | becomes | change |
|---|---|---|
| `idt::HARDIRQ_DEPTH` | the **interrupt-context marker** -- conservative, spans softirq, includes the test helper | none; document why it is not the accounting counter |
| `cputime::irq_depth` | **cycle accounting** only | extend the bracket to vectors 251/252/255 so attribution stops charging IRQ time to the interrupted task |

So the attribution gap is still worth fixing and the duplicate is not a
duplicate. What was genuinely wrong was that I added the second counter
without writing down why a second one was needed -- which is what made it
read as an accident an hour later, to me.

**The lesson is the one from this morning, pointed at myself twice.** I
compared the two counters by *shape* -- both per-CPU, both `AtomicU64`, both
counting interrupt nesting -- and concluded duplicate. The distinguishing
fact was in neither name nor type but in one caller and one doc sentence.
Same as `secmod` and `authbroker`: identical caller profiles, opposite
verdicts, and only line 1 separates them.
