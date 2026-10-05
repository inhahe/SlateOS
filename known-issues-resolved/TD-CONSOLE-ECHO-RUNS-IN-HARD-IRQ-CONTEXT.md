### [A] TD-CONSOLE-ECHO-RUNS-IN-HARD-IRQ-CONTEXT. Keyboard echo renders glyphs to the framebuffer from inside the IRQ 1 handler — 2026-08-14 — ✅ FIXED 2026-08-14 (`kernel/src/keyboard.rs`)

**Fix.** The ISR no longer renders. `push_char` now hands the byte to
`queue_echo`, which filters the non-echoing keys, pushes into a new 256-byte
SPSC echo ring, and submits a single `drain_echo` work item to the kernel
workqueue. `drain_echo` runs in the worker *task* context and does the
`console::putchar` calls there.

**Why a workqueue and not a softirq** — the mechanism that looks like the
obvious choice is the wrong one, so this is worth stating. A softirq runs on
the interrupted task's kernel stack with that task suspended mid-execution, so
it must never block on a lock the interrupted task might hold;
`softirq.rs`'s own contract requires handlers to use `try_lock`. Echo needs
the console lock unconditionally, so routing it through a softirq would have
converted the hard-IRQ deadlock into a softirq deadlock and looked like
progress. The workqueue is explicitly "deferred work in process context …
may sleep, take mutexes, allocate", which is what rendering needs. Linux
reaches the same conclusion: `tty_flip_buffer_push` defers to `flush_to_ldisc`
on a workqueue, not to a softirq.

**Details worth keeping.**

* *Submission is coalesced.* One work item per keystroke would exhaust the
  workqueue's 64-item capacity during a key-repeat storm and start dropping
  **other subsystems'** work — a much worse failure than slow echo. A
  `ECHO_DRAIN_SCHEDULED` flag means a burst submits once.
* *The flag is cleared before draining, not after.* A producer that pushes
  after the clear sees it clear and submits a fresh drain; the worst case is
  one redundant drain finding an empty ring. Clearing afterwards would leave
  a byte stranded until the next keystroke.
* *A failed submit un-latches the flag.* Otherwise a single full-queue moment
  would latch `SCHEDULED` true with nothing scheduled to clear it, and echo
  would be dead for the rest of the boot.
* *Early boot falls back to inline rendering.* `keyboard::init` runs ~700
  lines ahead of `workqueue::init`, so there is a real window with no worker
  to defer to. Inline rendering there is harmless — no userspace, no latency
  budget in force, no contention for the console lock — and `is_running()` is
  monotonic, so the two paths cannot interleave and reorder output.
* *Dropped bytes are counted, not ignored.* A silent drop shows up to the user
  as randomly missing characters while the input ring still holds the byte —
  i.e. the shell acts on input that was never displayed. `ECHO_DROPPED` makes
  that visible.

**Tested** by a new `echo_ring_self_test` (run from `keyboard::self_test`)
covering FIFO order, exact capacity (`SIZE-1`, one slot sacrificed to
distinguish full from empty), refusal past capacity, and drop accounting. It
masks interrupts for the duration because the producer under test *is* the
IRQ 1 handler — a keystroke arriving mid-test would push into the same ring
and make the assertions describe something other than what they name.

**Consequence for the earlier deadlock fix.** The console lock is now
task-context-only on this path, so the `lock_irqsave` from
B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK is belt-and-braces
rather than load-bearing. It stays: it is what makes the guarantee categorical
for any future IRQ-context printer, and re-introducing the bug should require
someone to actively remove a safeguard rather than merely forget one.

---

*Original entry:*

**The debt.** `handle_device_irq` → `keyboard::handle_scancode` →
`push_char` → `console::putchar` (chain tabulated in
B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK above) does the
full console pipeline — escape-sequence state machine, glyph blit, and on
the last column a whole-screen scroll (`memmove` of the framebuffer plus a
scrollback `Vec::push` that can hit the heap allocator) — with the CPU
inside a hard interrupt handler.

**Why it matters.** CLAUDE.md's interrupt-dispatch budget is "total ISR
latency < 10 µs, deferred work via softirq/tasklet equivalent". A
full-screen scroll at 1024×768×32bpp is ~3 MiB of `memmove`; it is not
within three orders of magnitude of 10 µs. Every keystroke that lands on
the bottom line therefore stalls the timer tick and every other device.

**Proper fix.** Split the echo out of the ISR the way Linux splits n_tty:
the handler decodes the scancode and pushes the resulting byte(s) into the
existing input ring, and a bottom half (the tty/console task) drains the
ring and does the rendering in task context. Once the console lock is
task-context-only, the `lock_irqsave` from the deadlock fix above becomes
belt-and-braces rather than load-bearing — keep it anyway, since it is what
makes the guarantee categorical for any *future* IRQ-context printer.

**Why not done now.** It changes echo latency and ordering
(character-visible-at-keystroke becomes character-visible-at-next-drain),
which is a user-visible interactivity change and wants its own boot test
and its own commit. The deadlock is the urgent half and is fixed
independently.
