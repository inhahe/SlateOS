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
