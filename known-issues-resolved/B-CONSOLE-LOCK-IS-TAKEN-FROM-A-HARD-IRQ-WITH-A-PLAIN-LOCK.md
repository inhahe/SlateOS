### [A] B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK. The keyboard ISR echoes through `CONSOLE.lock()`, so any task interrupted while holding the console wedges the CPU forever, silently — 2026-08-14 — **FIXED** (`a18ea83a9`)

> **Resolution.** The fix landed in the *same commit* that added this entry,
> so everything below is written in the pre-fix voice ("Proper fix. Convert
> …") and describes work that is **already done**. Re-verified 2026-08-14:
> `kernel/src/console.rs` imports `crate::sync::Mutex` (line 71), all three
> statics are `Mutex::named` (`COLOR_SCHEME` 134, `SCROLLBACK` 512,
> `CONSOLE` 720), and the file now contains **45 `lock_irqsave()`
> acquisitions (34 + 7 + 4) and zero plain `.lock()` calls and zero
> `spin::` references**. The heading said `OPEN` until this correction —
> see the process note at the end of this entry.

**Symptom (predicted, not yet observed).** The machine stops dead with no
output, no panic, no stall report. Nothing is printed because the CPU is
spinning inside an interrupt handler on a lock whose holder is the very
frame the interrupt suspended.

**The chain.** All five links are `grep`-verifiable, no inference:

| # | Site | Call |
|---|---|---|
| 1 | `kernel/src/ioapic.rs:730` | `pub extern "C" fn handle_device_irq(irq: u32)` — **hard IRQ context** |
| 2 | `kernel/src/ioapic.rs:746` | → `keyboard::handle_scancode()` |
| 3 | `kernel/src/keyboard.rs:282` | → `handle_normal()` |
| 4 | `kernel/src/keyboard.rs:461` | → `push_char()` |
| 5 | `kernel/src/keyboard.rs:489,490,491,497` | → `crate::console::putchar(ch)` |

and `console::putchar` (`kernel/src/console.rs:850`) opens with
`let mut con = CONSOLE.lock();` where `CONSOLE`
(`kernel/src/console.rs:679`) is a **raw `spin::Mutex`** and `lock()` is
the plain, interrupts-enabled acquire.

So: task T calls any of the 45 `CONSOLE.lock()` sites. While T holds the
lock, IRQ 1 fires **on the same CPU**. The ISR runs on T's stack, reaches
`putchar`, and spins on a lock that only T can release — and T cannot run
again until the ISR returns. Permanent, silent, single-CPU deadlock.
`SCROLLBACK` (line 478) and `COLOR_SCHEME` (line 107) are raw the same
way; they are reachable from the same ISR because `putchar` → `scroll_up_locked`
→ `SCROLLBACK.lock()` (line 2271).

**Why it is silent.** This is the distinguishing feature, and the reason
it is worth writing down rather than just fixing. Both instrumented lock
types route contention through a 30-second stall detector that fires from
*inside* the spin loop (`kernel/src/sync.rs`, `STALL_SECONDS` at line 73).
A raw `spin::Mutex` has no such detector: it spins forever and reports
nothing. A wedge on this lock therefore produces exactly zero evidence.

**Why it has not been hit constantly.** Exposure is reduced — but not
eliminated — by two accidents:

* kshell drives the keyboard with `ECHO_ENABLED` off, so the shell's own
  key handling does not take the console lock from the ISR. The default
  canonical-TTY echo-on path does.
* The window is only as wide as a console critical section, and most of
  the 45 are a few instructions. `write_str` over a long line, and any
  call that scrolls (full-screen `memmove` plus a scrollback `Vec::push`
  that can realloc), are the wide ones.

Reduced exposure is not a defence. This is a "works until it doesn't"
bug: the odds scale with typing during output, which is precisely what an
interactive shell does.

**Contrast: `serial.rs` already defends this exact case, three ways.**
`serial::_print` (`kernel/src/serial.rs:204`) wraps the whole acquire in
`crate::cpu::without_interrupts(...)`, claims a per-CPU `IN_PRINT` flag
*before* taking the lock, and falls back to a lock-free
`SerialPort::emergency()` on re-entry. Its doc comment (lines 190–198)
describes the failure mode verbatim: "a garbled report can be read, a
deadlock cannot." `console.rs` has none of the three. The two files
diverged; only one of them was thought about.

**Proper fix.** Convert `CONSOLE`, `SCROLLBACK` and `COLOR_SCHEME` from
`spin::Mutex` to `crate::sync::Mutex` and change every acquisition to
`lock_irqsave()`. That is the structural fix, not a mitigation: masking
interrupts on the local CPU for the hold makes the reentrant arrival
*impossible* rather than merely unlikely, and it is categorical — it
protects against any future IRQ-context console user, not just the
keyboard. It also lands the locks in the instrumented type, so a future
wedge here reports itself instead of hanging mutely, and lockdep gets the
`CONSOLE → SCROLLBACK` edge.

This matches Q24's taxonomy (design-decisions.md §70): `CONSOLE` is a
**non-leaf** lock — `scroll_up_locked` (line ~2255) takes
`SCROLLBACK.lock()` at line 2271 while the caller holds `CONSOLE` — so it
takes `crate::sync::Mutex`, not `PreemptSpinMutex`.

**Invariant the fix relies on, recorded so it is not silently broken
later.** `lock_irqsave` closes the *interrupt* window; it does not close
an *exception* window, because `cli` does not mask faults. The fix is
therefore complete only while no exception handler prints to the console.
That holds today and was checked, not assumed: the only `console::` callers
outside `console.rs` are `keyboard.rs`, `kshell.rs` and `ipc/io_ring.rs`,
and the `#[panic_handler]` (`kernel/src/main.rs:5952`) executes `cli()` and
then uses `serial_println!` exclusively. If a fault handler is ever taught
to write to the console, it must first grow a per-CPU re-entrancy guard in
the shape of `serial.rs`'s `IN_PRINT`.

**Cost of the fix, stated honestly rather than glossed.** `lock_irqsave`
masks interrupts for the whole hold, so the widest console critical section
is now also the widest interrupts-off window on the system: `scroll_up_locked`
(`kernel/src/console.rs:~2296`) does a ~3 MiB framebuffer `ptr::copy` plus a
per-pixel clear of the bottom row while holding `CONSOLE`. That is hundreds
of microseconds to low milliseconds. This is accepted, for three reasons:

* It is task context, not ISR context, so it is not measured against the
  10 µs ISR budget.
* It cannot be narrowed while the lock is held — dropping `IF` mid-scroll
  to shorten the window is precisely the reentrant arrival the fix exists
  to prevent, so any "optimisation" here reintroduces the deadlock.
* The alternative (leave the lock raw and hope the ISR never lands inside
  a critical section) trades a bounded latency cost for an unbounded,
  silent, permanent hang. That is not a trade.

The console's other whole-screen loops were already written to drop the
lock first (`clear_screen` at ~868, `apply_scheme` at ~312 both capture the
geometry, `drop(con)`, then blit) — so the exposure really is limited to
the scroll path.

Usefully, the conversion also makes the cost *measurable*: `lock_irqsave`
feeds `crate::cpu::irqoff_tracker`, so `irqoff` in kshell now reports the
max interrupts-off duration this path actually produces, instead of it
being invisible. That number is the evidence that should drive
TD-CONSOLE-ECHO-RUNS-IN-HARD-IRQ-CONTEXT below.

**Not related to B-FORKEXEC-BOOT-HANG.** Tempting, but no: the
diagnostics that went missing in that hang were `serial_println!`, and
serial is a wholly separate lock with its own (working) defence. Recording
the non-link so a later session does not "solve" that hang by pointing at
this fix.

**Separate concern, deliberately not folded into this fix.** Rendering
glyphs into the framebuffer from inside a hard IRQ handler blows CLAUDE.md's
"total ISR latency < 10 µs" target by orders of magnitude, deadlock or no
deadlock. The right shape is Linux's: the ISR queues the character and a
bottom half does the echo, after which the console lock is task-context-only
again. That is a different change with a different risk profile, so it is
logged below as TD-CONSOLE-ECHO-RUNS-IN-HARD-IRQ-CONTEXT rather than
smuggled into a deadlock fix.

**Process note — why this entry said `OPEN` for a bug that was already
fixed.** Both this entry and the RTL8139 one below were written *while
investigating*, in the future tense ("**Proper fix.** Convert `CONSOLE` …"),
and then committed **together with the fix that carried them out**. The prose
was accurate when drafted and stale by the time it was committed, and the
`— OPEN` in the heading — the only part anyone skimming the file actually
reads — was never flipped. Both entries then sat mislabelled until a later
session went looking for "open Lane A bugs to fix", picked these two off the
`grep '— OPEN$'` list, and found the work already done.

That is an expensive failure mode in both directions: it invites duplicate
work, and worse, it corrodes trust in the file — if the `OPEN` list contains
fixed bugs, the natural correction is to stop believing the file, which is the
opposite of what a bug tracker is for. Note it is the same shape as the
benchmark failures recorded later in this file
(`B-BENCH-CANARY-CERTIFIES-CLEAN-RUNS…` and its three predecessors): **a
status that is never re-checked degrades into a status that is merely
asserted.** The heading is a claim about the world; committing it unchanged
alongside a fix makes it a claim about nothing.

The rule that prevents it: **when one commit both writes up a bug and fixes
it, the heading must be written in its post-fix state in that same commit.**
If the fix is not complete there, the write-up should say what remains rather
than inheriting a blanket `OPEN`. The cheap standing check is
`grep -n '— OPEN$' known-issues.md`, confirming each hit against the code
before trusting it — which is how these were caught.

**How widespread it was, measured rather than assumed.** Having found two, the
obvious question was whether the rest of the `OPEN` list could be trusted, so
all six Lane-A `— OPEN` headings were checked against the code. **Three of the
six were stale** — this one, the RTL8139 entry below, and
`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE`, the last of which had
even accumulated an internal `**Closed 2026-08-14 …**` paragraph while its
heading still said `OPEN`. So the file's headline status was wrong for **half**
the open list. That is the number worth remembering: this was not two
oversights, it was the normal outcome of the workflow that produced them, and
the only reason it looked like an exception is that nobody had counted.
