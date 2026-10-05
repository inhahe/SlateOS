### [A] Why that corpus is near-empty, and what it makes the check worth -- 2026-09-17

**Status:** OPEN

Having found that the lock-context check's population was two synthetic
classes, the next question is whether the *real* number can be anything but
zero. Mostly it cannot, and the reason is that the hazard has been designed
out rather than guarded. That changes what the check is for, so it is worth
writing down instead of leaving the zero to look like a defect.

**My first explanation was wrong.** I assumed an automated QEMU boot presses
no keys, so `console`'s interrupt path never runs. The log says otherwise --
`Live IRQ lines: 1 seen via the ISR path [1]` -- IRQ 1 is the keyboard and it
fired.

The actual reason is better. `push_char` no longer calls `putchar`; it calls
`queue_echo`, whose doc says *"Called from hard-IRQ context, so it must not
render: see `drain_echo` for why the rendering moved to a worker task."* The
chain `console.rs`'s module doc still describes -- IRQ 1 ->
`handle_device_irq` -> `handle_scancode` -> `push_char` -> `putchar` -- is
now taken only in a pre-`workqueue::init` window, and `queue_echo`'s comment
explains why rendering from an ISR is harmless in that one stretch: no
userspace, no scheduler-visible latency budget, nothing else contending for
the console lock. That window is ~700 lines of boot wide and needs a
keystroke to land inside it.

So the two reasons the real corpus is ~0 are both deliberate engineering:

| reason | whose decision |
|---|---|
| interrupt-context locks are raw `spin::Mutex` or `PreemptSpinMutex`, and the check hooks `crate::sync::Mutex` | dd-70's conversion sweep, which kept the ISR-reached locks raw on purpose |
| the one `sync::Mutex` that was IRQ-reachable had its hot path moved to a worker | whoever wrote `queue_echo`/`drain_echo` |

**What that makes the check worth.** It is a **tripwire for regressions, not
an audit of the present.** It cannot tell me the kernel is currently correct,
because it can barely see the kernel; it can tell me the day somebody makes
an ISR take a `crate::sync::Mutex` with a blocking acquire. That is a real
thing to want -- `console` and `sysctl` were both *converted* from raw to
`sync::Mutex`, which is exactly the migration that would trip it -- but it is
a much narrower claim than `clean` sounds.

This is the whole reason the line prints `VACUOUS` rather than `clean` when
the population is zero. Without that word the output would read as a clean
bill of health for a kernel the instrument never examined.

**What would give it a real corpus.** Extend it to `PreemptSpinMutex`, which
is where interrupt-context locking actually lives now. The obstacle is that
the type deliberately has no lockdep class (dd-70), so there is no class
index to key the two bits on -- but they do not need a class table: two
`AtomicBool`s in the lock struct itself is 489 x 2 bits, needs no
registration, and is already the pattern the leaf check uses for its name.
Not done yet; recorded so the zero is not mistaken for completeness.
