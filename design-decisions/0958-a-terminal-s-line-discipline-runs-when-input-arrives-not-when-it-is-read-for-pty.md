## 958. A terminal's line discipline runs when input arrives, not when it is read — for ptys now, for the console once its keyboard ring has one owner

**Date:** 2026-09-24 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** the code that interprets what is typed into a terminal — Ctrl-C
becoming an interrupt, characters being echoed, lines being edited — used to run
only when a program *read* from the terminal. So Ctrl-C could not interrupt a
program that was busy, which is the only kind anyone tries to interrupt. For
terminal windows (ptys) that code now runs the moment input arrives, as it does
on every Unix. The physical console keeps the old arrangement for now, because
the kernel's own debug shell reads the same keyboard input directly and needs
Ctrl-C delivered to it as an ordinary byte.

**What was decided.** `tty::receive` is the line discipline, applied per byte on
arrival: input translation, `ISIG`, canonical editing, echo. Its output goes to
a per-device input queue (lines marked, `^D` as a mark that is never
delivered), and `read` only takes finished bytes out of that queue. A pty
receives in `pty::master_write`, which returns the signals it decided and lets
the syscall layer deliver them. Found through `ctest-pty`; see known-issues
`A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER`.

**Alternatives considered.**

| option | why not |
|---|---|
| Keep the discipline in the reader and only classify signal characters in the master write | fixes `^C` alone and leaves every other receive-side behaviour wrong — type-ahead not echoed, poll readiness only an upper bound for a canonical slave, a `^C` unable to flush lines already typed ahead. It would also have needed a *fourth* `ISIG` classifier beside the three the read paths already had, when the root problem was that there were several |
| Full Linux structure: one ring holding the partial line after the committed ones (`canon_head`) | fewer bytes per device and no line-to-queue copy, but it moves the editor's erase/kill/column logic onto a ring and discards `feed` — the pure, already-tested editor. The two-buffer form keeps `feed` intact and makes the mode switch an explicit, readable rule rather than a pointer reassignment |
| Receive console keystrokes in the keyboard IRQ too | the kernel shell (`kshell.rs`) and `SYS_CONSOLE_READ_CHAR` read that ring directly and use byte 0x03 as data; classifying it in the IRQ would take it from them. Deferred with a design in known-issues `A-CONSOLE-CTRL-C-IS-ONLY-SEEN-BY-A-READER` |

**Behaviour that changes, deliberately, because it is Linux's:** echo appears as
you type; a `^C` flushes complete lines not yet read as well as the line being
edited, and clears echo still pending from the same write (Linux's `isig`
does both); `FIONREAD` and poll on a canonical slave are exact; a line typed to
`MAX_CANON` keeps its terminator; `VEOL`/`VEOL2` end lines; a control character
set to 0 is disabled; `ICRNL`/`INLCR`/`IGNCR` apply in raw mode; a switch of
`ICANON` carries unread input across (canonical → raw: the half-typed line
becomes raw input; raw → canonical: unread raw input becomes one complete line);
`TCSETSF` flushes. One Linux quirk is deliberately *not* reproduced: an
end-of-file mark left over from canonical mode is dropped on the switch to raw,
where Linux delivers it as a NUL byte.

**Where this bites:** `kernel/src/tty/mod.rs` (`receive`, `InputQueue`,
`wait_for`, the read policies), `kernel/src/tty/pty.rs` (`master_write` and
`master_try_write` return `MasterWrite`), `kernel/src/syscall/handlers.rs`
(`pty_master_write_common` delivers the signals), `kernel/src/syscall/linux.rs`
(`TCSETSF`).
