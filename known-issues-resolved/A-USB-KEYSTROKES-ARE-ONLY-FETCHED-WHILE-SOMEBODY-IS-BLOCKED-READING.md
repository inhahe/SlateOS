## A-USB-KEYSTROKES-ARE-ONLY-FETCHED-WHILE-SOMEBODY-IS-BLOCKED-READING — 2026-08-21 — lane A — FIXED 2026-08-21

**Fixed** the same day it was filed, by items 1 and 2 of "The proper fix" below.
`keyboard::start_usb_hid_poller()` arms an 8 ms hrtimer whose callback drains
the HID endpoint from the APIC timer ISR, so a USB keystroke now reaches the
input ring whether or not anybody is reading. The read paths poll inline only
as a fallback, gated on `USB_HID_POLLER_ARMED` — which records whether the
timer *runs*, not whether it was requested, because `hrtimer` refuses to
schedule past a per-CPU ceiling and hands back a handle that never fires.

Confirmed on the boot rig, which attaches `-device qemu-xhci -device usb-kbd`:

```
[xhci] Configured keyboard on slot 1 (EP1 IN, 8 bytes, interval=7)
[keyboard] USB HID poller armed (8 ms period)
[keyboard]   USB HID poller: armed (xhci present, keyboard present): OK
```

`interval=7` is the device's own answer and it matches: xHCI encodes the
interval as 2^(n-1) × 125 µs, so 7 is exactly the 8 ms the poller uses. The
period is not a guess.

The ISR calls `xhci::try_poll_keyboard`, a new `try_lock` variant. That is a
correctness requirement, not a tuning choice: `XHCI.lock()` from the timer ISR
deadlocks outright, because the code the interrupt preempted may hold that
same lock **on the same CPU**, and a spinlock cannot be re-entered by the
interrupt that stopped its owner from releasing it. A contended tick skips, and
loses nothing — the ring is drained per *report*, not per tick, so whatever a
skipped tick left behind is still there 8 ms later.

A regression test (`keyboard::usb_hid_poller_self_test`) asserts the property
whose absence *was* the bug — that a poller exists whenever there is a keyboard
to poll — rather than reading a character, which is what let the defect hide:
a blocking read polls the device itself, so any test that merely read a byte
passed both before and after.

**Item 3 is a separate piece of work and is NOT done.** See the note at the end
of this entry.

**In short:** On a machine whose keyboard is USB (which is every modern machine
— PS/2 is a legacy port), the kernel never notices a keystroke on its own. It
only goes and asks the keyboard for one at the moment some program is already
sitting there waiting for input. Type-ahead therefore does not work: keys
pressed while the system is busy are not queued as they are pressed, they are
collected in a burst the next time something blocks on a read. Anything that
wants to *check* for a keypress without waiting for one — a poll, a
`select`/`epoll`-style readiness test, a hotkey, a Ctrl-C while a program is
running — cannot see the key at all, because nothing polled the device.

**Found by:** lane A, while fixing `BUG-CONSOLE-READ-UNINTERRUPTIBLE` (stage 1,
2026-08-21). Not caused by that change — the fix inherited the structure. It is
recorded separately because it is a distinct defect with a distinct fix, and
because stage 2 of that entry is **blocked on this one**.

**Where.** `kernel/src/keyboard.rs:1339` `poll_usb_keyboard()` is the only
caller of `kernel/src/xhci.rs:2256` `poll_keyboard()`, which is the only route
by which a USB HID report ever reaches the input ring. `poll_usb_keyboard` in
turn has exactly **two** call sites, both on the read path:

| Site | Context |
|---|---|
| `keyboard.rs:965`, in `try_read_char()` | a non-blocking read — polls, then checks the ring |
| `keyboard.rs:1033`, in `read_char_inner()` | the blocking loop — polls on every spin |

There is no timer call site, no interrupt call site, and no driver task. The
xHCI event ring is not serviced from an interrupt handler for the HID endpoint;
it is drained synchronously by whoever is asking for a character.

**Why this is worse than it first reads.** PS/2 keyboards go through IRQ 1 and
are pushed into the same ring by the interrupt handler, so they behave
correctly. That is why the defect has never been seen: **QEMU's default keyboard
is PS/2**, so every boot test to date has exercised the working path. The broken
path is the one that runs on real hardware — which, per `design-decisions.md`
§263, is about to start happening.

**Reproduce (once a USB keyboard is reachable).** Boot with
`-device qemu-xhci -device usb-kbd` and no PS/2 keyboard. At the shell, start
something that takes a few seconds and does not read stdin, type during it, and
observe that the characters appear all at once when the *next* read blocks,
rather than being echoed as typed. `try_read_char()` called in a tight loop by a
program that never blocks will also mostly return `None` regardless of what is
being typed, because each call polls only once.

**The proper fix.** Move HID polling off the read path and onto a driver-owned
periodic task:

1. A kernel task (or an hrtimer callback) polls the xHCI HID interrupt endpoint
   at the endpoint's own `bInterval` — 8 ms is the usual figure for a boot
   keyboard — and pushes reports into the input ring exactly as the IRQ 1
   handler does. The ring, the echo queue and `push_char` are already
   ISR-shaped, so nothing downstream changes.
2. `try_read_char()` and `read_char_inner()` then stop polling and simply read
   the ring, which makes them symmetric with the PS/2 path and makes a
   non-blocking readiness check meaningful for the first time.
3. Only then can `BUG-CONSOLE-READ-UNINTERRUPTIBLE` stage 2 proceed. That stage
   converts the blocking read from a `hlt` spin into a real park, and parking is
   *unsafe today precisely because of this bug*: a parked task does not spin,
   so nothing calls `poll_usb_keyboard`, so a USB keystroke can never arrive to
   wake it. Parking without fixing this first deadlocks USB keyboards outright.

Use the ISR-safe wake idiom when the poller pushes a byte —
`sched::try_wake(tid)`, falling back to `sched::defer_wake(tid)` — not
`WaitQueue::try_wake_one()`, which loses the wake on a lost `try_lock`.

**If never fixed:** the console keeps working in QEMU and keeps working for
blocking reads on real hardware, so nothing looks broken. What stays impossible
is type-ahead, non-blocking input, and any readiness-based console I/O on real
USB hardware — and stage 2 of the interruptible-read fix stays blocked
indefinitely. It gets more expensive with time, not less: every consumer written
against `try_read_char()` in the meantime is written against a function that
cannot do what its name says on the hardware we are about to start booting on.

**What item 3 still needs (read this before attempting stage 2).** This fix
removed the objection quoted in item 3 — a parked reader no longer takes the
USB keyboard down with it, because the poller runs regardless. But stage 2 is
**still not safe to do**, for a *different* reason that this entry originally
folded into the same sentence:

> the poller pushes into the ring and wakes nobody.

So a genuinely parked reader would now sleep straight through its own
keystroke. That is a strictly better failure than the old one — the keystroke
is captured rather than never fetched — but it is still a hang. Stage 2 needs
`push_char` to wake the waiting reader, with the ISR-safe idiom recorded above
(`sched::try_wake`, falling back to `sched::defer_wake`; **not**
`WaitQueue::try_wake_one`, which drops the wake on a lost `try_lock`).

Check `keyboard::usb_hid_poller_armed()` rather than assuming it: the poller
does not arm on a machine with no xHCI controller, and on such a machine the
`HLT` spin is still the only thing that looks at the ring.
