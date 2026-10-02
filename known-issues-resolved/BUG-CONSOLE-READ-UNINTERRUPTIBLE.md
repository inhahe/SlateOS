## `BUG-CONSOLE-READ-UNINTERRUPTIBLE` — **FIXED** (lane A, stage 1: 2026-08-21, stage 2: 2026-09-08)

**Status: FIXED.** Both stages are now complete. A process blocked reading the
console is interruptible by signals (stage 1) and genuinely parks in the
scheduler rather than HLT-spinning (stage 2). The keyboard's `read_char_inner`
uses a lock-free CAS array of waiting task ids (`KEYBOARD_WAITERS`),
`park_interruptible`, and ISR-safe wakes from `push_char_raw` — matching the
pty backend's design. Deadline-based reads arm an hrtimer for precise wakeup.
Kernel-mode readers (kshell) keep the HLT loop, which is correct for their
use case.

**Stage 1, as landed (2026-08-21).** `kernel/src/keyboard.rs` grew
`pub enum ReadOutcome { Byte(u8), Interrupted, TimedOut }` and one private
`read_char_inner(deadline_ns: Option<u64>, pid: u64)` that all four public
entry points share. The loop is unchanged except for one added test per `HLT`
wake: `ipc::waiters::deliverable_signal_pending(pid)`. Because
`deliverable_signal_pending(0)` is unconditionally `false`, the pre-existing
`read_char()` / `read_char_timeout()` pass `pid = 0` and keep their exact old
semantics for kshell's six call sites — no duplicated loop, and no behaviour
change on the kernel-task path. The new `read_char_interruptible(pid)` and
`read_char_timeout_interruptible(deadline_ns, pid)` pass the real pid and are
what `tty::backend_read_char` / `backend_read_char_timeout` and
`sys_console_read_char` (`kernel/src/syscall/handlers.rs`) now call. As
predicted below, `canonical_read`/`raw_read` needed no edit: the four-state
`Input` enum already carried `Interrupted` through to `ConsoleRead::Interrupted`.

**The proper fix, corrected — and why the original one below is wrong.**
(Historical: step 1 of the two below has since been done — see the ✅ on it.
The paragraph is kept as written because it is the reasoning that identified
the ordering, and the ordering still holds.) The
text under "The proper fix" says to replace the `HLT` spin with
`park_interruptible` plus a wake from the IRQ 1 handler. **That would make a
USB keyboard dead.** USB HID keys are not interrupt-driven here at all: they
are noticed only by `keyboard::poll_usb_keyboard()` → `xhci::poll_keyboard()`,
and that function has exactly three call sites in the whole kernel — all of
them *inside* `try_read_char` and the blocking read loop. Nothing else in the
system drives the poll. A reader that parked indefinitely would stop polling,
and only a PS/2 keyboard would still work. IRQ 1 is the PS/2 line; it says
nothing about xHCI.

So stage 2 is two changes, in order:

1. ✅ **DONE 2026-08-21.** **Move USB HID polling out of the read path** into a
   driver-owned periodic task (a timer callback, or a workqueue item re-armed
   on the tick), so keystrokes are noticed whether or not anybody is blocked
   reading. This also fixes a *separate* latent bug that the current
   arrangement hides: **USB keystrokes are only ever polled while some task is
   inside a console read.** A USB key pressed while the system is busy
   elsewhere is not buffered, it is simply never fetched from the ring until
   the next read comes along.

   *Done as the timer-callback variant:* `keyboard::start_usb_hid_poller()`
   arms an 8 ms hrtimer (the endpoint's own advertised interval — QEMU reports
   `interval=7`, which xHCI encodes as 2^6 × 125 µs) whose callback drains the
   endpoint from the APIC timer ISR via the new `xhci::try_poll_keyboard`.
   Details, including why the ISR path must `try_lock` rather than `lock`, are
   in `A-USB-KEYSTROKES-ARE-ONLY-FETCHED-WHILE-SOMEBODY-IS-BLOCKED-READING`.

   **Read the paragraph below before treating this as unblocking step 2**: what
   was removed is the "a parked reader stops polling and the keyboard dies"
   objection. The poller still wakes nobody, so step 2's wake side is
   untouched — a parked reader would now sleep through a keystroke that *was*
   successfully captured, which is a better failure but still a hang.
2. ✅ **DONE 2026-09-08.** **Convert the loop to a real park**: a lock-free
   CAS array of waiting task ids (`KEYBOARD_WAITERS`, 4 slots),
   `park_interruptible`, and a wake from `push_char_raw` (reached from both
   the PS/2 IRQ 1 handler and the USB HID poll timer). The wake uses the
   ISR-safe idiom — `sched::try_wake(tid)` and, on failure,
   `sched::defer_wake(tid)` — as prescribed. The full-array fallback is the
   old `HLT` poll, so the degenerate case degrades to current behaviour rather
   than to a deadlock. Deadline-based reads (VTIME) arm a one-shot hrtimer
   that wakes the parked task at the deadline, so they no longer rely on the
   timer tick for wakeup. Kernel-mode readers (`pid == 0` — kshell, boot
   console) keep the HLT loop: they have no signal context, and `park_interruptible`
   degrades to `block_current` for pid 0 anyway.

**Original report follows, unedited apart from the heading, because the
reasoning it records is what stage 1 acted on.**

**Found:** 2026-08-21, while generalising `kernel/src/tty` from one console into
N terminal devices (`requests/b-a-pty-devices-need-the-line-discipline-that-the-console-already-has.md`).

**What it is.** A process blocked reading the *console* cannot be interrupted by
a signal. It parks inside `keyboard::read_char()`, which is a `HLT`-spin waiting
for a scancode and has no signal check in it, so `SIGINT`, `SIGTERM` — anything
short of `SIGKILL`'s scheduler-level teardown — does not wake it. The process
resumes only when somebody presses a key.

**Where.** `kernel/src/keyboard.rs` `read_char()`, reached from
`kernel/src/tty/mod.rs` `backend_read_char()`'s `Backend::Console` arm. The doc
comment there points at this entry.

**Why it did not matter before, and does now.** With one terminal it was
invisible: the only process that could be blocked on the console was the
foreground job, and a `^C` typed at the console *is* a keypress, so the same
event that generated the signal also woke the read. The generalisation makes the
gap visible by contrast — the pty backend
(`pty::slave_read_input_blocking`) parks through
`ipc::waiters::park_interruptible` and *is* interruptible, so the same
`tty::read` call is interruptible on one device and not on another. A process
signalled from elsewhere (another terminal, a `kill` from a script) while
blocked on the console still hangs until a key is pressed.

**Reproduce.** Have a process block on `SYS_TTY_READ` against the console; from
a second context send it `SIGTERM`. It stays blocked. Press any key: it wakes
and the signal is then delivered at the syscall-return checkpoint.

**The proper fix.** Give the keyboard the same waiter/park structure the pty
backend has: a `WaiterSet` of tasks blocked on the scancode queue, `park_interruptible`
instead of the `HLT` spin, and a wake from the IRQ 1 handler. Then
`backend_read_char` can return `Input::Interrupted` for the console exactly as it
already does for a pty, and `canonical_read`/`raw_read` need no change at all —
the four-state `Input` return was widened for precisely this. The keyboard is
lane A's, so no cross-lane request is needed.

**Why it is not fixed in the same change.** The pty work is already a large
refactor of the shared discipline, and the keyboard rework touches the IRQ path,
which wants its own boot test rather than being folded into one that is already
proving the discipline. Scoped as the immediate follow-up.

**Severity.** Real but narrow: today's only console reader is the interactive
shell, whose signals arrive from keypresses. It becomes a genuine hang the
moment a second terminal exists and something on it kills a job on the console —
which is the state the pty work creates.
