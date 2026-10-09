# A → B, D: read the console through the terminal, so Ctrl-C can stop a program that is not reading

**Status:** OPEN · **Filed:** 2026-10-09 by lane A · **Priority:** normal --
nothing of yours breaks now; this unblocks a kernel fix that would break
these three readers if it went first.

## In short

On the text console, Ctrl-C stops a program only while it is reading input
(known-issues `A-CONSOLE-CTRL-C-IS-ONLY-SEEN-BY-A-READER`): a program busy
computing, or waiting on anything but the keyboard, cannot be interrupted
from it. A terminal window (a pty) does not have this problem, because each
key goes through the line discipline -- the terminal's own input processing,
which turns ^C into `SIGINT` -- the moment it is typed. The kernel fix is to
do the same for the console whenever a session owns it.

That fix takes each key off the keyboard's raw ring as it arrives. Three
programs still read that ring directly with `SYS_CONSOLE_READ_CHAR` (101),
past the terminal, and would lose their keys to it -- and `telnet`, which
wants ^C sent to the remote host as a byte, would see a local `SIGINT`
instead. So they need to read through the terminal first, as every Linux
program does and as the C library already does (`posix/src/file.rs`, the
`Console` arm of `read`, design-decisions §114).

## What to change

| Program | Lane | Now | Asked |
|---|---|---|---|
| `userspace/screen` | B | `console_read_char()` (`SYS_CONSOLE_READ_CHAR`) | read through the terminal in raw mode |
| `userspace/telnet` | B | `SYS_CONSOLE_READ_CHAR` (line 302) | the same; raw mode keeps ^C a byte for the remote |
| `services/init` | D | `read_char` / its non-blocking sibling (`SYS_CONSOLE_READ_CHAR`) | the same, or remove them if unused (`read_char` is `#[allow(dead_code)]`) |

Raw mode is the native termios calls a C program's `cfmakeraw` +
`tcsetattr` become: `SYS_TTY_GET_TERMIOS` (541), clear `ICANON`, `ECHO` and
`ISIG` (and `IXON`, `ICRNL` as `cfmakeraw` does), `SYS_TTY_SET_TERMIOS`
(542); then `SYS_TTY_READ` (543) returns bytes as they are typed. Restore the
saved termios on exit. With `ISIG` clear, ^C arrives as the byte 0x03 --
exactly what `screen` (its command key) and `telnet` (the remote's interrupt)
want -- and no signal is raised.

## What lane A does after

Once these are through the terminal, the console receives keys as they
arrive while a session owns it (a foreground process group), as a pty does:
^C, ^\ and ^Z reach the foreground group whether or not anything is reading.
`SYS_CONSOLE_READ_CHAR` stays for the kernel shell's case, a console no
session owns. Tell me when yours are moved, or if a program needs the raw
ring for a reason I have not seen.

— lane A
