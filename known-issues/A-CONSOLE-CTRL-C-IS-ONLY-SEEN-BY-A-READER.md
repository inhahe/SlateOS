### [A] `A-CONSOLE-CTRL-C-IS-ONLY-SEEN-BY-A-READER` — `^C` on the physical console still only reaches a program that is reading -- 2026-09-24
**Status:** OPEN (lane A). Designed below; not implemented.

**In short:** the entry above fixed Ctrl-C for terminal windows (ptys). The
physical keyboard-and-screen console has the same defect and was deliberately
left alone: a program running on the text console cannot be interrupted with
Ctrl-C unless it is reading its input at the time.

**Why it was not fixed with the pty.** A pty's input arrives through one door,
the master's write. The console's arrives in the keyboard IRQ, into a ring
that **three** consumers read directly: the terminal line discipline, the
kernel shell (`kshell.rs` — Ctrl-C exits its find mode, which needs byte 0x03
as *data*), and `SYS_CONSOLE_READ_CHAR`. Classifying `^C` at IRQ time would
take the byte away from the other two. So the console receives a keystroke
when a terminal reader pulls it off the ring (every key already typed is
received, oldest first, before the reader decides anything), which keeps the
order of type-ahead right but cannot help a program that is not reading.

**The proper fix, as designed.** Receive at arrival *while a user session owns
the console*: the IRQ path already defers work to the workqueue (echo does, in
`keyboard::queue_echo`); a second deferred item would drain the ring into the
console device through `tty::receive` whenever the console has a foreground
process group — the state in which a `^C` has somebody to signal — and leave
the ring raw otherwise, which is the kernel shell's state. What has to be
settled first is who owns keystrokes when both a session and the kernel shell
are live, since today they simply race for each key.

**Settled by reading the code (2026-10-09), and what it showed is in the
way.** The question above -- who owns keystrokes when a session and the
kernel shell are both live -- does not arise: the kernel shell starts only
when init cannot be spawned (`main.rs`, the fallback after
`spawn_process(INIT_ELF)`), so it never runs beside a session. But three
programs read the keyboard's raw ring themselves with
`SYS_CONSOLE_READ_CHAR`, past the terminal: `userspace/screen`,
`userspace/telnet` and `services/init`'s `read_char`. Receiving at arrival
would take their keys -- and turn `telnet`'s ^C, which it means to send to
the remote host, into a local `SIGINT`. So they move first, to `SYS_TTY_READ`
with raw mode set through `SYS_TTY_GET_TERMIOS`/`SET_TERMIOS`, as the C
library already did (§114): `requests/a-bd-read-the-console-through-the-terminal-so-ctrl-c-can-stop-a-program-that-is-not-reading.md`.

The kernel half, for when they have: a deferred item, submitted from the
keyboard interrupt on a false-to-true edge as echo is, drains the ring into
the console device through `tty::receive` while the console has a foreground
process group, delivers any signal it produces to that group
(`signal_foreground_group`), bumps a console-input generation and wakes the
keyboard waiters. Console readers stop consuming the ring outside the device
lock -- `console_wait_for`'s blocking step becomes a wait for "the ring is not
empty, or the generation moved" that takes nothing, then loops back to the
locked pull -- so the deferred item and a reader can never reorder two keys.
