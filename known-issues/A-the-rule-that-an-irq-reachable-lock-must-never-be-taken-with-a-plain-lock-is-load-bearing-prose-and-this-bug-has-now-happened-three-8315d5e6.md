### [A] The rule that an IRQ-reachable lock must never be taken with a plain `lock()` is load-bearing prose, and this bug has now happened three times in five weeks -- 2026-09-17

**Status:** OPEN

**In short:** if a lock can be grabbed by an interrupt handler, then every
*other* place that grabs it has to switch interrupts off first. Otherwise an
interrupt can arrive while an ordinary task is holding the lock, and the
handler spins forever waiting for a task that cannot run until the handler
finishes. The machine stops dead with no message. Nothing in the build
checks this rule -- it is written in comments, and the comments have been
right; the code has drifted three times anyway.

The occurrences, in three unrelated subsystems:

| date | path | outcome |
|---|---|---|
| 2026-08-14 | keyboard ISR echoes through `CONSOLE.lock()` | fixed, `a18ea83a9` (B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK, now in `known-issues-resolved.md`) |
| 2026-09-15 | buffer-cache writeback softirq re-enters blkdev's registry lock | fixed with `try_with_device`; prompted the deferred gate in `todo.txt` |
| 2026-09-17 | `keyboard::scancode_to_ascii` reaching `keylayout::translate` from IRQ 1 | avoided at design time, by `translate_try`/`try_lock` (dd-946) |

The third is the one that matters for this entry. It was avoided only because
I happened to read `keyboard.rs`'s own docs and remember dd-940. That is not
a control; that is luck with a good memory. The 2026-09-15 deferral said a
comment would not hold the line and cited lane B's evidence for it -- a
header saying FIVE OUT OF FIVE RECENT ADDITIONS had a fault, read by someone
while committing the sixth. Its stated trigger was *a second deadlock of this
shape*. There have now been two more.

### The audit, and what it found

**`console` is the positive control, and it is clean.** Its module docs say
every acquisition uses `lock_irqsave`, *never* plain `lock()`, and call that
load-bearing. Verified rather than assumed: 45 acquisitions across `CONSOLE`,
`SCROLLBACK` and `COLOR_SCHEME`, all 45 `lock_irqsave`, zero plain `lock()`.
Any gate that flags `console` is a wrong gate.

**The interrupt-context marker covers 2 of 5 dispatch arms.**
`cputime::enter_irq`/`exit_irq` are called only from `handle_timer_irq`
(vector 32) and `handle_device_irq` (33-56). Vectors 251 (TLB shootdown), 252
(reschedule IPI) and 255 (spurious) never bump `irq_depth`, nor does the
`_ => {}` arm. Those three handlers are lock-free today -- atomics, `invlpg`,
EOI -- so this is latent rather than live.

**Corrected the same day, because the first version of this entry overstated
it.** I wrote that `irq_depth`'s gap also breaks nested-IRQ detection at
`apic.rs:1006`, where `irq_depth() > 1` caps timer-on-timer nesting to stop
the 16 KiB IRQ stack overflowing -- a real safety mechanism, not accounting.
It does not. A nest can only form inside a handler that re-enables
interrupts, and all three of these return with IF still clear: checked, not
assumed -- none contains an `sti`, a `without_interrupts`, or a
`softirq::process_pending`. The only handlers that do re-enable are the timer
and the device path, and both bump `irq_depth`. So the cap keeps its
coverage.

What is actually left is CPU-time *attribution*: cycles spent in those three
vectors are charged to whatever task they interrupted rather than to IRQ
time. That is worth fixing and is not urgent. The latent part is the one to
watch: the day any of the three grows a softirq tail or re-enables
interrupts, the nesting cap silently loses coverage, and nothing would say
so.

`dispatch_vector` already carries
the argument for fixing this, written for `count_vector` right above the same
`match`: *the five-call-site version is correct exactly as long as everyone
remembers it, which is the property that failed for 33-56 already.* The file
learned the lesson for counting and did not apply it to context.

**`PreemptSpinMutex`'s leaf premise is unenforced.** 489 instances across
~370 files, none visible to lockdep. That is deliberate and documented (dd-70,
`sync.rs:1079`): the type is for *true leaf* locks, ones that never nest
another lock, so ordering checks add nothing. The decision is sound; the
premise is asserted per call site and checked by no one. It is a dd-944
control -- it is what justifies skipping the check -- and controls have to be
executable.

**A dangling cross-reference.** `console.rs:51` points at `known-issues.md`
for an entry that moved to `known-issues-resolved.md` when it was fixed.

### Why the gate is smaller than the deferral assumed

The deferral proposed a static call-graph walker and said the baseline was the
hard part: 5954 blocking `lock()` sites, most of them legitimate. Checking the
*runtime* property instead removes the baseline problem entirely, because the
honest expected count is zero.

The invariant to check is not "no blocking lock in interrupt context" -- that
would flag `console`, which is correct code. It is:

> for any lock class ever acquired in hard-IRQ context, **every** acquisition
> of that class must have interrupts disabled.

Two per-class bits, set in `lockdep::lock_acquire`, which already runs on
every acquisition of `crate::sync::Mutex` and already has a re-entrancy guard
so that reporting cannot recurse through the serial lock. A class with both
bits is deadlock-prone by construction.

The hard-IRQ bit genuinely needs the `dispatch_vector` marker above and cannot
be derived from the interrupt flag: an IDT interrupt gate clears IF on entry,
so *inside* an ISR `cpu::interrupts_enabled()` is false -- indistinguishable
from a `lock_irqsave` caller in task context. That is a reason the marker is
necessary, not merely tidier.

### First real run, 2026-09-17: 4 violations, 1 suspect, 8 classes -- and 3 of the 4 were one bug in the check

Boot `b1ebbda65`, release, BOOT_OK after 581s. The control fired correctly
(`fires once on a real overlap, silent on the irqsave pattern: OK`), and the
banner read:

```
[bench]   lock-context check: 4 violation(s), 1 suspect(s), over 8 class(es) seen in interrupt context
```

**I had predicted zero violations, and written the prediction down first.**
It was wrong, and the way it was wrong is the argument for the whole
approach. I reasoned from `grep lock_irqsave` (3 files) plus dd-70's
ISR-reachable list, concluded the corpus was console + sysctl + accounting +
rtl8139, and checked that each was correct. The runtime check found **8**
classes acquired in interrupt context, including locks I had no idea were
reachable from one. Static reasoning over the call sites I could think to
grep for is exactly what dd-947 calls a coverage failure.

### The false positive, three independent instances of it

`sysctl-reg`, `SWAP` and `CGROUP` were all reported as violations, and none
is one. `lockdep::lock_acquire` is called for a **successful `try_lock`**
too, with `Acquire::Try`, and `note_lock_context` ignored the kind. A
try_lock in interrupt context cannot be the hazard: the caller walks away if
the lock is held, so it can never spin on a holder it preempted.

What makes this worse than an ordinary bug is *which* locks it hit. In every
one of the three, the try_lock path exists **specifically because** a
blocking acquire from interrupt context was a known hazard, and somebody
wrote the non-blocking path and documented it:

| lock | the documented ISR path |
|---|---|
| `sysctl-reg` | `sysctl::try_get` -- added after B-SYSCTL-IRQ-DEADLOCK, a real boot wedge: the timer IRQ's `sched::check_starvation` blocked on `REGISTRY` behind a task in `sysctl::set()` holding it across a slow `serial_println!` |
| `SWAP` | `swap.rs:1153`, "Non-blocking variant of `summary()` for interrupt/softirq context" |
| `CGROUP` | `cgroup.rs:598`, "try_lock: called from scheduler timer tick (interrupt context)"; also 1037 for the I/O scheduler |

So the check's first act was to report three deliberate fixes as the defect
they fix. My own commit message had said "a check that reports the fix as the
defect is worse than no check" -- written about `sysctl` while the same bug
was live for the other two, because I found `sysctl` by reading and never
asked whether anything else had the same shape.

Fixed: the interrupt-side buckets now require `matches!(how,
Acquire::Blocking)`. The task-side bucket deliberately still records both
kinds -- the hazard there is *holding* the lock with interrupts enabled, and
how the holder acquired it makes no difference to an interrupt landing
mid-hold. A third control covers exactly this shape, which is the bit pattern
of a real violation minus the blocking acquire.

### Still open after the fix

Two reports had no name to give: class 47 (`0xffffffff81720b38`, the
suspect) and class 50 (`0xffffffff81727538`, a violation).

**My first explanation for that was wrong, and checking took one minute.** I
wrote that `class_name` had refused a slot reserved but not yet published.
It had not. `sync::Mutex::new` sets `name: b"?"` as its *default* --
`Mutex::named` is the one that takes a diagnostic name -- and there are
**563** `Mutex::new(` instances in `kernel/src`. So those two locks are not
unnameable; they were simply never named, along with 561 others.

Worth keeping because it is a diagnostic ambiguity, not just my error: `?`
in a lockdep report has **two** independent causes -- an unnamed lock, and
`class_name` declining a half-written slot -- and nothing in the output
distinguishes them. A reader who assumes either one is right half the time.

It also raises the value of printing the acquisition site from what I
thought it was. I added it as a convenience for a rare unnameable class. It
is in fact the *only* way to identify an unnamed lock at all, because 563 of
them answer to the same name.

Both reports now print the acquisition site from `CLASS_SITE`, which
`lock_acquire` already records via `Location::caller()`, so an unnamed class
still yields a file and a line.

### Resolved on the next boot (`e8a2c179c`): all five were the one bug

With `Acquire::Try` excluded from the interrupt side, the real tree reports
**nothing**. `sysctl-reg`, `SWAP`, `CGROUP`, class 47 and class 50 are all
silent; the only surviving report is the self-test's own deliberate
`ctx-control`, and it now carries its site
(`First acquired at kernel/src/lockdep.rs:1493`). So every one of the four
violations and the single suspect was the same omission -- counting a
successful `try_lock` as the interrupt-side hazard.

That is the good outcome and it is also the uncomfortable one. A check whose
first run produces five findings, all false, is a check that would have been
believed: three of the five named locks whose try_lock path is the
*documented fix* for this exact hazard. The thing that caught it was reading
`sysctl.rs`'s prose before acting on the report, not the report itself.
